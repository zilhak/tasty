//! engine별 레이아웃을 Tasty 홈의 layouts/NN.json 슬롯 파일로 저장한다.
//! 슬롯 목록은 파일명에서 읽으며 별도 인덱스는 없다. 구조와 surface 복원 정보를 담고
//! 화면·scrollback 바이트는 별도 저장소에 둔다. plugin 종류는 등록부를 통해 저장·복원한다.

#[cfg(any(feature = "gui", test))]
mod capture;
mod restore;
mod schema;
#[cfg(any(feature = "gui", test))]
mod scrollback;
#[cfg(test)]
mod tests;

use std::path::{Path, PathBuf};
use std::time::Instant;

pub use schema::SavedLayout;

#[cfg(any(feature = "gui", test))]
use crate::core::CoreState;

pub(super) const LAYOUT_VERSION: u32 = 2;

pub(crate) type LayoutSlotId = u32;

const LAYOUTS_SUBDIR: &str = "layouts";
const LEGACY_LAYOUT_FILE: &str = "layout.json";
const SLOT_EXT: &str = "json";

/// 홈 선택·격리는 tasty_home에 맡기고 그 아래 layouts를 쓴다.
fn layouts_dir() -> Option<PathBuf> {
    tasty_utils::path::tasty_home().map(|d| d.join(LAYOUTS_SUBDIR))
}

/// 두 자리 이상 숫자 파일명. 슬롯 정렬은 문자열이 아니라 번호로 한다.
fn slot_path_in(dir: &Path, slot: LayoutSlotId) -> PathBuf {
    dir.join(format!("{slot:02}.{SLOT_EXT}"))
}

/// 숫자 슬롯을 정렬·중복 제거한다. 디렉터리 읽기 실패는 빈 목록, 개별 항목 오류는 건너뛴다.
fn list_slots_in(dir: &Path) -> Vec<LayoutSlotId> {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
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
    slots.dedup();
    slots
}

#[cfg(feature = "gui")]
pub(crate) fn list_slots() -> Vec<LayoutSlotId> {
    match layouts_dir() {
        Some(dir) => list_slots_in(&dir),
        None => Vec::new(),
    }
}

/// 파일 부재와 읽기·해석 실패를 구별해 기존 레이아웃을 무심코 덮지 않게 한다.
pub(crate) enum SlotLoad {
    Loaded(SavedLayout),
    Absent,
    /// 읽기 오류 또는 지원 범위보다 높은 version. 호출자는 저장을 막아야 한다.
    Unreadable,
    /// 해석 실패. 읽을 때는 유지하고 덮어쓸 때 재확인·백업한다.
    Unparsable,
}

/// 읽기만 한다. GC와 engine이 같은 슬롯을 읽으므로 백업 이동은 저장 직전에 한다.
/// 높은 version은 구버전이 덮어쓰지 않도록 Unreadable로 반환한다.
fn load_slot_in(dir: &Path, slot: LayoutSlotId) -> SlotLoad {
    let path = slot_path_in(dir, slot);
    match std::fs::read_to_string(&path) {
        Ok(json) => parse_slot_json(&path, &json),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => SlotLoad::Absent,
        Err(e) => {
            tracing::error!(
                "failed to read layout slot {}: {e} — starting without it and refusing to \
                 overwrite it (fix permissions or move the file to start fresh)",
                path.display()
            );
            SlotLoad::Unreadable
        }
    }
}

/// 로드와 저장 직전 재확인이 공유하는 JSON 판정. 여기서는 로그를 남기지 않는다.
fn classify_slot_json(json: &str) -> SlotLoad {
    match serde_json::from_str::<SavedLayout>(json) {
        Ok(layout) if layout.version > LAYOUT_VERSION => SlotLoad::Unreadable,
        Ok(layout) => SlotLoad::Loaded(layout),
        Err(_) => SlotLoad::Unparsable,
    }
}

fn parse_slot_json(path: &Path, json: &str) -> SlotLoad {
    let load = classify_slot_json(json);
    match &load {
        SlotLoad::Unreadable => tracing::error!(
            "layout slot {} holds a version this build does not support ({} or lower) — \
             starting without it and leaving it untouched so a newer build can still read it",
            path.display(),
            LAYOUT_VERSION
        ),
        SlotLoad::Unparsable => tracing::error!(
            "failed to parse layout slot {}; leaving it in place and requiring backup before a later save",
            path.display()
        ),
        SlotLoad::Loaded(_) | SlotLoad::Absent => {}
    }
    load
}

/// 홈을 찾지 못하면 Unreadable로 반환해 경로 없는 상태를 새 슬롯으로 취급하지 않는다.
pub(crate) fn load_slot(slot: LayoutSlotId) -> SlotLoad {
    match layouts_dir() {
        Some(dir) => load_slot_in(&dir, slot),
        None => SlotLoad::Unreadable,
    }
}

/// 첫 저장보다 이른 부팅 안내에서 백업 공간 부족을 알릴 수 있도록 현재 예산만 조회한다.
pub(crate) fn slot_preservation_is_blocked_in(dir: &Path, slot: LayoutSlotId) -> bool {
    tasty_utils::path::backup_budget_is_exhausted(&slot_path_in(dir, slot))
}

/// layouts 경로가 없으면 저장 자체를 못 하므로 백업 공간 부족으로 분류하지 않는다.
pub(crate) fn slot_preservation_is_blocked(slot: LayoutSlotId) -> bool {
    layouts_dir().is_some_and(|dir| slot_preservation_is_blocked_in(&dir, slot))
}

/// 레이아웃을 동기 저장한다. 오류는 로그로 남기며 호출자에게 성공 여부를 반환하지 않는다.
/// capture가 새 scrollback 저장 ID를 터미널에도 기록하므로 engine을 변경할 수 있다.
#[cfg(any(feature = "gui", test))]
pub(crate) fn save_slot(engine: &mut CoreState, active_workspace: usize, slot: LayoutSlotId) {
    // 검사에서 실제 저장 경로 전체를 사용하되 사용자 홈 대신 주입한 디렉터리를 쓴다.
    #[cfg(test)]
    let resolved = engine.layouts_dir_override.clone().or_else(layouts_dir);
    #[cfg(not(test))]
    let resolved = layouts_dir();
    let dir = match resolved {
        Some(d) => d,
        None => {
            tracing::error!(
                "cannot determine the tasty home directory; current layout was not saved"
            );
            return;
        }
    };
    save_slot_in_dir(engine, active_workspace, slot, &dir);
}

/// 손상 원본 보존과 쓰기를 함께 실행한다. 읽지 못한 슬롯의 보호 검사는 이 함수에 없으므로
/// 제품 호출은 Core::apply의 SaveLayoutNow 검사를 거쳐야 한다.
#[cfg(any(feature = "gui", test))]
pub(crate) fn save_slot_in_dir(
    engine: &mut CoreState,
    active_workspace: usize,
    slot: LayoutSlotId,
    dir: &Path,
) {
    let Some(json) = serialize_layout(engine, active_workspace) else {
        return;
    };
    // 손상 원본을 백업하지 못하면 새 상태로 덮어쓰지 않는다.
    if engine.layout_slot_unparsable {
        if !preserve_unparsable_slot(dir, slot) {
            engine.layout_slot_preserve_failed = true;
            return;
        }
        engine.layout_slot_unparsable = false;
        engine.layout_slot_preserve_failed = false;
    }
    save_slot_in(dir, slot, &json);
}

#[cfg(any(feature = "gui", test))]
enum SlotReplace {
    /// 여전히 해석되지 않아 먼저 백업해야 한다.
    MoveAside,
    /// 지금은 해석되거나 파일이 없다.
    WriteOver,
    /// 읽을 수 없거나 version이 높아 덮어쓰면 안 된다.
    Refuse,
}

/// 부팅 후 다른 인스턴스가 파일을 바꿨을 수 있어 다시 읽는다. 슬롯 점유는 프로세스 안에서만 관리한다.
/// read와 rename 사이에는 잠금이 없어 이 재확인만으로 동시 쓰기 경합을 막지는 못한다.
#[cfg(any(feature = "gui", test))]
fn recheck_slot_before_replacing(path: &Path) -> SlotReplace {
    let json = match std::fs::read_to_string(path) {
        Ok(json) => json,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return SlotReplace::WriteOver,
        Err(e) => {
            tracing::error!(
                "refusing to overwrite the layout slot {}: it could not be re-read before being \
                 replaced ({e}) — this session's window layout will not be saved",
                path.display()
            );
            return SlotReplace::Refuse;
        }
    };
    match classify_slot_json(&json) {
        // 정상 파일을 불필요하게 백업해 보관 한도를 쓰지 않는다.
        SlotLoad::Loaded(_) => SlotReplace::WriteOver,
        SlotLoad::Unreadable => {
            tracing::error!(
                "refusing to overwrite the layout slot {}: it now holds a layout from a newer \
                 build — this session's window layout will not be saved",
                path.display()
            );
            SlotReplace::Refuse
        }
        SlotLoad::Unparsable | SlotLoad::Absent => SlotReplace::MoveAside,
    }
}

/// 재확인 뒤 필요하면 원본을 백업한다. true는 후속 쓰기 허용이며 쓰기 성공은 아니다.
#[cfg(any(feature = "gui", test))]
fn preserve_unparsable_slot(dir: &Path, slot: LayoutSlotId) -> bool {
    let path = slot_path_in(dir, slot);
    match recheck_slot_before_replacing(&path) {
        SlotReplace::WriteOver => return true,
        SlotReplace::Refuse => return false,
        SlotReplace::MoveAside => {}
    }
    match tasty_utils::path::preserve_corrupt_file(&path) {
        Ok(Some(backup)) => {
            tracing::error!(
                "the layout slot {} could not be parsed at startup; it was moved to {} before \
                 being replaced",
                path.display(),
                backup.display()
            );
            true
        }
        Ok(None) => true,
        Err(e) => {
            tracing::error!(
                "refusing to overwrite the layout slot {}: the unparsable original could not be \
                 preserved ({e}) — this session's window layout will not be saved; move the \
                 file aside first",
                path.display()
            );
            false
        }
    }
}

#[cfg(any(feature = "gui", test))]
fn serialize_layout(engine: &mut CoreState, active_workspace: usize) -> Option<String> {
    let saved = SavedLayout::capture(engine, active_workspace);
    match serde_json::to_string_pretty(&saved) {
        Ok(j) => Some(j),
        Err(e) => {
            tracing::error!("failed to serialize current layout: {e}; layout was not saved");
            None
        }
    }
}

#[cfg(any(feature = "gui", test))]
fn save_slot_in(dir: &Path, slot: LayoutSlotId, json: &str) {
    let path = slot_path_in(dir, slot);
    if let Err(e) = write_slot_atomic(dir, &path, json) {
        tracing::error!(
            "failed to write current layout to slot {}: {e}",
            path.display()
        );
    }
}

/// 같은 폴더의 임시 파일에 쓴 뒤 rename한다. rename 실패 때 임시 파일 삭제를 시도한다.
/// 고정된 임시 이름을 쓰며 다른 프로세스와의 잠금이나 fsync는 하지 않는다.
#[cfg(any(feature = "gui", test))]
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

#[cfg(any(feature = "gui", test))]
fn delete_slot_in(dir: &Path, slot: LayoutSlotId) {
    let path = slot_path_in(dir, slot);
    if let Err(e) = std::fs::remove_file(&path)
        && e.kind() != std::io::ErrorKind::NotFound
    {
        tracing::warn!("Failed to delete {}: {e}", path.display());
    }
}

/// 레이아웃 복원 설정이 꺼진 창을 닫을 때 슬롯을 지운다. 파일 부재는 무시한다.
#[cfg(feature = "gui")]
pub(crate) fn delete_slot(slot: LayoutSlotId) {
    let Some(dir) = layouts_dir() else { return };
    delete_slot_in(&dir, slot);
}

/// layouts 디렉터리가 없을 때만 옛 layout.json을 슬롯 1로 rename한다. 부팅 때 호출한다.
fn migrate_legacy_in(home: &Path) {
    let legacy = home.join(LEGACY_LAYOUT_FILE);
    if !legacy.exists() {
        return;
    }
    let dir = home.join(LAYOUTS_SUBDIR);
    if dir.exists() {
        tracing::warn!(
            "legacy layout {} is ignored because the slots directory {} already exists",
            legacy.display(),
            dir.display()
        );
        return;
    }
    if let Err(e) = move_legacy_into_slot_one(&legacy, &dir) {
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

/// 읽은 모든 슬롯의 scrollback 참조를 합쳐야 다른 창의 파일을 지우지 않는다.
/// 열거된 슬롯 중 하나라도 로드하지 못하면 GC를 생략한다.
/// 단, 목록 읽기 실패·개별 항목 누락은 list_slots_in이 오류를 반환하지 않으므로 이 보호가 적용되지 않는다.
fn gc_scrollback_orphans_all_slots_in(layouts: &Path, scrollback: &Path) {
    let mut union = std::collections::HashSet::new();
    for slot in list_slots_in(layouts) {
        match load_slot_in(layouts, slot) {
            SlotLoad::Loaded(layout) => union.extend(layout.collect_scrollback_refs()),
            // 열거 뒤 파일이 사라진 경우도 참조를 알 수 없으므로 GC를 건너뛴다.
            SlotLoad::Absent | SlotLoad::Unreadable | SlotLoad::Unparsable => {
                tracing::warn!(
                    "scrollback GC skipped: layout slot {slot} in {} could not be read — \
                     keeping every scrollback file",
                    layouts.display()
                );
                return;
            }
        }
    }
    crate::scrollback_store::gc_orphans_in(scrollback, &union);
}

/// 읽힌 슬롯 목록이 비면 빈 참조 집합으로 GC를 호출한다.
fn gc_scrollback_orphans_all_slots() {
    let (Some(layouts), Some(scrollback)) =
        (layouts_dir(), crate::scrollback_store::scrollback_dir())
    else {
        return;
    };
    gc_scrollback_orphans_all_slots_in(&layouts, &scrollback);
}

/// engine 생성 전에 마이그레이션하고 GC한다. 옮겨진 슬롯의 참조도 GC에 포함해야 한다.
/// restore_layout이 꺼져 있으면 저장하지 않는 레이아웃을 근거로 지우지 않도록 GC를 생략한다.
/// 마이그레이션은 설정과 무관하게 시도한다.
pub(crate) fn migrate_and_gc_on_boot(restore_layout: bool) {
    if let Some(home) = tasty_utils::path::tasty_home() {
        migrate_legacy_in(&home);
    }
    if restore_layout {
        gc_scrollback_orphans_all_slots();
    }
}

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
    /// 처음 변경된 시각을 유지해 연속 변경이 저장 예약을 계속 늦추지 않게 한다.
    /// 실제 저장 시각·성공 여부를 보장하지는 않는다.
    pub fn mark_dirty(&mut self) {
        if !self.dirty {
            self.dirty = true;
            self.dirty_since = Some(Instant::now());
        }
    }

    /// 호스트가 저장 기한 계산에 사용할 최초 변경 시각.
    #[cfg(feature = "gui")]
    pub fn dirty_since(&self) -> Option<Instant> {
        self.dirty_since
    }

    /// dirty 표시와 시각을 비운다. 호출자가 실제 저장 성공을 확인했는지는 검사하지 않는다.
    #[cfg(any(feature = "gui", test))]
    pub fn clear(&mut self) {
        self.dirty = false;
        self.dirty_since = None;
    }

    #[cfg(any(feature = "gui", test))]
    pub fn is_dirty(&self) -> bool {
        self.dirty
    }
}
