//! Layout persistence: save/restore workspace layout to `~/.tasty/layouts/NN.json`.
//!
//! 창마다 독립된 레이아웃을 갖도록 디스크 레이어가 **슬롯 단위 파일**로 나뉘어
//! 있다. 슬롯 하나 = [`SavedLayout`] 하나 = engine 하나의 전체 상태. 순서·목록은
//! 파일명(`NN.json`)의 숫자에서 전부 파생되고 별도 인덱스 파일을 두지 않는다 —
//! 인덱스는 실제 파일과 desync 되는 두 번째 진실원이 된다.
//!
//! Captures the structural tree (workspaces → pane nodes → panes → tabs → surface layouts)
//! with minimal per-surface info (cwd, file path, url). No screen/scrollback content.
//!
//! `SavedSurface` is `Terminal` + `Generic { kind, data }`. New surface kinds (including
//! plugins) round-trip via the SurfaceKindRegistry without touching this file.

mod capture;
mod restore;
mod schema;
mod scrollback;
#[cfg(test)]
mod tests;

use std::path::{Path, PathBuf};
use std::time::Instant;

pub use schema::SavedLayout;

use crate::core::CoreState;

const DEBOUNCE_MS: u128 = 500;
pub(super) const LAYOUT_VERSION: u32 = 2;

// ── Disk I/O (slot files) ──

/// 레이아웃 슬롯 번호. 파일명(`NN.json`)의 숫자와 같다.
pub(crate) type LayoutSlotId = u32;

const LAYOUTS_SUBDIR: &str = "layouts";
const LEGACY_LAYOUT_FILE: &str = "layout.json";
const SLOT_EXT: &str = "json";

/// 슬롯 디렉터리 (`tasty_home()/layouts`).
///
/// debug/release 격리는 루트(`tasty_home()`)가 담당한다 — debug 빌드는
/// `~/.tasty-debug/layouts/`, release 는 `~/.tasty/layouts/`. 루트가 갈리므로
/// 파일명 접미사(`-debug`)는 두지 않는다.
fn layouts_dir() -> Option<PathBuf> {
    tasty_utils::path::tasty_home().map(|d| d.join(LAYOUTS_SUBDIR))
}

/// `dir/{slot:02}.json`. 2 자리 zero-pad 라 사전순 정렬이 99 까지는 숫자순과
/// 일치하고, 100 이상은 자릿수가 자연 확장된다(정렬 키는 어차피 파싱한 숫자다).
fn slot_path_in(dir: &Path, slot: LayoutSlotId) -> PathBuf {
    dir.join(format!("{slot:02}.{SLOT_EXT}"))
}

// ── 열거 ──

/// 존재하는 슬롯 번호를 오름차순으로. 파일명이 숫자가 아니면 무시한다.
fn list_slots_in(dir: &Path) -> Vec<LayoutSlotId> {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        // 슬롯이 한 번도 저장된 적 없으면 디렉터리 자체가 없다 — 정상.
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Vec::new(),
        Err(e) => {
            tracing::warn!("layout slots: read_dir {} failed: {e}", dir.display());
            return Vec::new();
        }
    };
    let mut slots = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some(SLOT_EXT) {
            continue;
        }
        match path
            .file_stem()
            .and_then(|s| s.to_str())
            .and_then(|s| s.parse::<LayoutSlotId>().ok())
        {
            Some(slot) => slots.push(slot),
            None => tracing::warn!("layout slots: ignoring {}", path.display()),
        }
    }
    slots.sort_unstable();
    // `1.json` 과 `01.json` 이 함께 있으면 같은 슬롯으로 접힌다 — 목록에 같은
    // 번호가 두 번 나오면 호출자가 같은 파일을 두 번 읽는다.
    slots.dedup();
    slots
}

/// 존재하는 슬롯 번호를 오름차순으로.
///
/// 아직 호출자가 없다 — 창마다 다른 슬롯을 배정하는 후속 작업이 "비어 있는 가장
/// 작은 번호" 를 고르는 데 쓴다. 슬롯 열거는 저장소 API 의 일부라 여기서 함께
/// 완성해 둔다(소비자만 나중에 붙는다).
#[allow(dead_code)]
pub(crate) fn list_slots() -> Vec<LayoutSlotId> {
    match layouts_dir() {
        Some(dir) => list_slots_in(&dir),
        None => Vec::new(),
    }
}

// ── 로드 ──

/// 슬롯 하나를 읽는다. 파일이 없거나 파싱 실패거나 지원 범위 밖 version 이면 `None`.
fn load_slot_in(dir: &Path, slot: LayoutSlotId) -> Option<SavedLayout> {
    let path = slot_path_in(dir, slot);
    let json = std::fs::read_to_string(&path).ok()?;
    match serde_json::from_str::<SavedLayout>(&json) {
        Ok(layout) => {
            if layout.version > LAYOUT_VERSION {
                tracing::warn!(
                    "{} version {} is newer than supported {}",
                    path.display(),
                    layout.version,
                    LAYOUT_VERSION
                );
                return None;
            }
            Some(layout)
        }
        Err(e) => {
            tracing::warn!("Failed to parse {}: {e}", path.display());
            None
        }
    }
}

/// 슬롯 하나를 읽는다. 파일이 없거나 무효면 `None`.
pub(crate) fn load_slot(slot: LayoutSlotId) -> Option<SavedLayout> {
    load_slot_in(&layouts_dir()?, slot)
}

// ── 저장 ──

/// Save layout to disk. Non-blocking best-effort.
///
/// `&mut CoreState` 인 이유: `SavedLayout::capture` 가 새 persist_id 를 발급하면
/// 해당 surface 인스턴스의 `scrollback_persist_id` 필드에 기록해 다음 capture 가
/// 같은 ID 를 재사용한다.
///
/// 호출자는 항상 `DomainIntent::SaveLayoutNow` (Core::apply 내부) 를 경유한다 —
/// module 외부에서 본 fn 을 직접 부르지 않도록 `pub(crate)` 로 제한.
pub(crate) fn save_slot(engine: &mut CoreState, active_workspace: usize, slot: LayoutSlotId) {
    let dir = match layouts_dir() {
        Some(d) => d,
        None => {
            tracing::warn!("Cannot determine ~/.tasty path for layout save");
            return;
        }
    };
    let Some(json) = serialize_layout(engine, active_workspace) else {
        return;
    };
    save_slot_in(&dir, slot, &json);
}

fn serialize_layout(engine: &mut CoreState, active_workspace: usize) -> Option<String> {
    let saved = SavedLayout::capture(engine, active_workspace);
    match serde_json::to_string_pretty(&saved) {
        Ok(j) => Some(j),
        Err(e) => {
            tracing::warn!("Failed to serialize layout: {e}");
            None
        }
    }
}

/// 슬롯 파일 write. **원자적**이어야 한다 — 슬롯이 여러 개이므로 잘린 JSON 하나가
/// union GC 를 통해 *다른* 슬롯의 scrollback 까지 잃게 만든다
/// ([`gc_scrollback_orphans_all_slots`] 의 "모르면 지우지 않는다"). tmp write →
/// rename 은 `store::scrollback::write_in` 과 같은 패턴이다.
fn save_slot_in(dir: &Path, slot: LayoutSlotId, json: &str) {
    let path = slot_path_in(dir, slot);
    if let Err(e) = write_slot_atomic(dir, &path, json) {
        tracing::warn!("Failed to write {}: {e}", path.display());
    }
}

/// tmp write → rename. rename 이 실패하면 tmp 를 치운다 — 확장자가 `.tmp` 라
/// [`list_slots_in`] 이 슬롯으로 오인하지는 않지만 그대로 두면 계속 쌓인다.
fn write_slot_atomic(dir: &Path, path: &Path, json: &str) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    let tmp = path.with_extension(format!("{SLOT_EXT}.tmp"));
    std::fs::write(&tmp, json)?;
    match std::fs::rename(&tmp, path) {
        Ok(()) => Ok(()),
        Err(e) => {
            if let Err(cleanup) = std::fs::remove_file(&tmp) {
                tracing::debug!("layout slots: cleanup {} failed: {cleanup}", tmp.display());
            }
            Err(e)
        }
    }
}

// ── 삭제 ──

#[allow(dead_code)]
fn delete_slot_in(dir: &Path, slot: LayoutSlotId) {
    let path = slot_path_in(dir, slot);
    if let Err(e) = std::fs::remove_file(&path)
        && e.kind() != std::io::ErrorKind::NotFound
    {
        tracing::warn!("Failed to delete {}: {e}", path.display());
    }
}

/// 슬롯 파일을 지운다. 없으면 no-op.
///
/// 아직 호출자가 없다 — 창을 닫을 때 그 창의 슬롯을 회수하는 후속 작업이 쓴다.
#[allow(dead_code)]
pub(crate) fn delete_slot(slot: LayoutSlotId) {
    let Some(dir) = layouts_dir() else { return };
    delete_slot_in(&dir, slot);
}

// ── 레거시 마이그레이션 ──

/// 단일 파일 시절의 `tasty_home()/layout.json` 을 슬롯 1 로 옮긴다.
///
/// `layouts/` 가 **없을 때만** 동작한다 — 이미 슬롯을 쓰고 있는 인스턴스에
/// 옛 파일이 남아 있어도 덮어쓰지 않는다. 복사가 아니라 rename 이라 중간
/// 상태(양쪽에 반쪽 파일)가 생기지 않는다.
///
/// 부팅 1 회 호출을 전제로 한다([`migrate_and_gc_on_boot`]).
fn migrate_legacy_in(home: &Path) {
    let legacy = home.join(LEGACY_LAYOUT_FILE);
    if !legacy.exists() {
        return;
    }
    let dir = home.join(LAYOUTS_SUBDIR);
    if dir.exists() {
        // 조용히 방치하면 사용자는 이 파일이 왜 무시되는지 알 수 없다.
        tracing::warn!(
            "{} is ignored — layout slots live in {} now (safe to delete the legacy file)",
            legacy.display(),
            dir.display()
        );
        return;
    }
    if let Err(e) = move_legacy_into_slot_one(&legacy, &dir) {
        // 레이아웃이 유실된 것이 아니라 "이번 부팅에 복원되지 않는" 강등이다.
        tracing::warn!(
            "layout migration: {} -> {} failed: {e}",
            legacy.display(),
            dir.display()
        );
    }
}

fn move_legacy_into_slot_one(legacy: &Path, dir: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    std::fs::rename(legacy, slot_path_in(dir, 1))
}

// ── scrollback union GC ──

/// 전 슬롯이 참조하는 `scrollback_ref` 의 **합집합**으로 GC 를 1 회 돌린다.
///
/// 슬롯별로 GC 하면 슬롯 1 을 읽은 창이 슬롯 2·3 이 참조하는 `.bin` 을 전부
/// orphan 으로 판정해 지운다. 그래서 GC 는 engine 생성 시점이 아니라 **부팅 1 회**,
/// 전 슬롯 union 으로만 일어난다.
///
/// **존재하는데 파싱이 안 되는 슬롯이 하나라도 있으면 아무것도 지우지 않는다** —
/// 그 슬롯이 무엇을 참조하는지 모르는 채 GC 하면 손상은 JSON 하나인데 손실은
/// 그 슬롯의 scrollback 전체가 된다. "모르면 지우지 않는다" 가 안전한 방향이다.
/// (파일이 아예 없는 슬롯 번호는 애초에 [`list_slots_in`] 에 안 잡히므로 해당 없음.)
fn gc_scrollback_orphans_all_slots_in(layouts: &Path, scrollback: &Path) {
    let mut union = std::collections::HashSet::new();
    for slot in list_slots_in(layouts) {
        match load_slot_in(layouts, slot) {
            Some(layout) => union.extend(layout.collect_scrollback_refs()),
            None => {
                tracing::warn!(
                    "scrollback GC skipped: layout slot {slot} in {} is unreadable",
                    layouts.display()
                );
                return;
            }
        }
    }
    crate::scrollback_store::gc_orphans_in(scrollback, &union);
}

/// 전 슬롯 union scrollback GC. 슬롯이 하나도 없으면 빈 집합으로 호출한다
/// (알려진 ref 가 없으므로 전부 orphan — 슬롯 도입 전과 같은 의미).
fn gc_scrollback_orphans_all_slots() {
    let (Some(layouts), Some(scrollback)) =
        (layouts_dir(), crate::scrollback_store::scrollback_dir())
    else {
        return;
    };
    gc_scrollback_orphans_all_slots_in(&layouts, &scrollback);
}

/// 부팅 1 회 훅. **마이그레이션 → GC 순서가 고정**이다 — GC 가 마이그레이션
/// 결과(슬롯 1 로 옮겨진 레거시 레이아웃)를 봐야 그 참조를 union 에 넣는다.
/// engine(슬롯 로드)이 만들어지기 전에 불러야 한다.
///
/// `restore_layout` 이 꺼져 있으면 GC 를 건너뛴다 — 그 설정에서는 애초에 슬롯을
/// 저장하지 않으므로(`impl_workspace::save_layout_now` 의 게이트), union 이 비어
/// 옛 `.bin` 을 전부 지우게 된다. 슬롯 도입 전에도 GC 는 같은 조건 안에 있었다.
/// 마이그레이션 자체는 설정과 무관하게 한다(나중에 켰을 때 복원되도록).
pub(crate) fn migrate_and_gc_on_boot(restore_layout: bool) {
    if let Some(home) = tasty_utils::path::tasty_home() {
        migrate_legacy_in(&home);
    }
    if restore_layout {
        gc_scrollback_orphans_all_slots();
    }
}

// ── Dirty flag / debounce state ──

/// Tracks whether the layout has been modified and needs saving.
#[derive(Default)]
pub struct LayoutDirtyTracker {
    dirty: bool,
    dirty_since: Option<Instant>,
}

impl LayoutDirtyTracker {
    pub fn new() -> Self {
        Self::default()
    }
}

impl LayoutDirtyTracker {
    /// Mark layout as dirty (called on structural changes).
    pub fn mark_dirty(&mut self) {
        if !self.dirty {
            self.dirty = true;
            self.dirty_since = Some(Instant::now());
        }
    }

    /// Check if enough time has elapsed and a flush is needed.
    /// Returns true if the caller should save now.
    pub fn should_flush(&self) -> bool {
        if !self.dirty {
            return false;
        }
        match self.dirty_since {
            Some(since) => since.elapsed().as_millis() >= DEBOUNCE_MS,
            None => false,
        }
    }

    /// Reset after a successful save.
    pub fn clear(&mut self) {
        self.dirty = false;
        self.dirty_since = None;
    }

    /// Force check if dirty (for shutdown flush).
    pub fn is_dirty(&self) -> bool {
        self.dirty
    }
}
