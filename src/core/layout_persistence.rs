//! Legacy layout slot discovery and import compatibility, plus journal capture scheduling.
//! Product persistence uses the journal worker; legacy files remain import inputs.

pub(crate) mod import;
mod restore;
pub(crate) mod schema;
#[cfg(test)]
mod tests;

use std::path::{Path, PathBuf};
use std::time::Instant;

pub use schema::SavedLayout;

pub(crate) const LAYOUT_VERSION: u32 = 2;

pub(crate) type LayoutSlotId = u32;

const LAYOUTS_SUBDIR: &str = "layouts";
const LEGACY_LAYOUT_FILE: &str = "layout.json";
const SLOT_EXT: &str = "json";

/// 홈 선택·격리는 tasty_home에 맡기고 그 아래 layouts를 쓴다.
#[cfg(feature = "gui")]
fn layouts_dir() -> Option<PathBuf> {
    tasty_utils::path::tasty_home().map(|d| d.join(LAYOUTS_SUBDIR))
}

/// 두 자리 이상 숫자 파일명. 슬롯 정렬은 문자열이 아니라 번호로 한다.
fn slot_path_in(dir: &Path, slot: LayoutSlotId) -> PathBuf {
    dir.join(format!("{slot:02}.{SLOT_EXT}"))
}

/// 슬롯 번호 목록과, 디렉터리를 끝까지 읽었는지 여부.
/// 불완전한 목록에 빠진 슬롯의 참조는 알 수 없으므로 GC처럼 지우는 쪽은 complete를 확인해야 한다.
struct SlotList {
    slots: Vec<LayoutSlotId>,
    complete: bool,
}

/// 숫자 슬롯을 정렬·중복 제거한다. 디렉터리가 없으면 완전한 빈 목록이다.
/// 디렉터리나 개별 항목을 읽지 못하면 읽은 만큼 돌려주고 불완전으로 표시한다.
fn list_slots_in(dir: &Path) -> SlotList {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return SlotList {
                slots: Vec::new(),
                complete: true,
            };
        }
        Err(e) => {
            tracing::warn!("layout slots: read_dir {} failed: {e}", dir.display());
            return SlotList {
                slots: Vec::new(),
                complete: false,
            };
        }
    };
    let mut slots = Vec::new();
    let mut complete = true;
    for entry in entries {
        let entry = match entry {
            Ok(entry) => entry,
            Err(e) => {
                tracing::warn!(
                    "layout slots: reading an entry of {} failed: {e}",
                    dir.display()
                );
                complete = false;
                continue;
            }
        };
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
    SlotList { slots, complete }
}

/// 새 창의 슬롯 배정용. 불완전한 목록도 읽은 만큼 쓴다(journal이 아는 슬롯과 합친다).
#[cfg(feature = "gui")]
pub(crate) fn list_slots() -> Vec<LayoutSlotId> {
    match layouts_dir() {
        Some(dir) => list_slots_in(&dir).slots,
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

/// Legacy slot classification exercised by compatibility tests.
#[cfg(test)]
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
#[cfg(test)]
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
#[cfg(test)]
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

#[cfg(test)]
fn delete_slot_in(dir: &Path, slot: LayoutSlotId) {
    let path = slot_path_in(dir, slot);
    if let Err(e) = std::fs::remove_file(&path)
        && e.kind() != std::io::ErrorKind::NotFound
    {
        tracing::warn!("Failed to delete {}: {e}", path.display());
    }
}

/// 옛 layout.json 이관 결과. 시험이 실패 분기를 실제로 지났는지 확인하는 데 쓴다.
#[derive(Debug, PartialEq, Eq)]
enum LegacyMigration {
    NoLegacy,
    Moved,
    /// 슬롯 파일이 있는 layouts 디렉터리가 이미 있어 옛 파일을 그대로 둔다.
    SlotsExist,
    Failed,
}

/// 옛 layout.json을 슬롯 1로 rename한다. 부팅 때 호출한다.
/// layouts 디렉터리가 없거나, 있어도 슬롯 파일이 하나도 없다고 확인되면 이관한다.
/// 뒤의 경우는 앞선 부팅에서 디렉터리만 만들고 rename이 실패한 상태를 다시 시도하는 것이다.
fn migrate_legacy_in(home: &Path) {
    migrate_legacy_with(home, move_legacy_into_slot_one);
}

fn migrate_legacy_with(
    home: &Path,
    mover: impl FnOnce(&Path, &Path) -> std::io::Result<()>,
) -> LegacyMigration {
    let legacy = home.join(LEGACY_LAYOUT_FILE);
    if !legacy.exists() {
        return LegacyMigration::NoLegacy;
    }
    let dir = home.join(LAYOUTS_SUBDIR);
    if dir.exists() {
        let listed = list_slots_in(&dir);
        if !(listed.complete && listed.slots.is_empty()) {
            tracing::warn!(
                "legacy layout {} is ignored because the slots directory {} already holds slots \
                 or could not be listed",
                legacy.display(),
                dir.display()
            );
            return LegacyMigration::SlotsExist;
        }
    }
    match mover(&legacy, &dir) {
        Ok(()) => LegacyMigration::Moved,
        Err(e) => {
            tracing::warn!(
                "layout migration: {} -> {} failed: {e} — keeping the legacy file and its \
                 scrollback files",
                legacy.display(),
                dir.display()
            );
            LegacyMigration::Failed
        }
    }
}

fn move_legacy_into_slot_one(legacy: &Path, dir: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    std::fs::rename(legacy, slot_path_in(dir, 1))
}

/// 남아 있는 옛 layout.json의 scrollback 참조. 이관 전이거나 이관에 실패한 파일도 원본이다.
/// None은 파일을 읽거나 해석하지 못해 참조를 알 수 없다는 뜻이다.
fn legacy_scrollback_refs(legacy: &Path) -> Option<std::collections::HashSet<String>> {
    let json = match std::fs::read_to_string(legacy) {
        Ok(json) => json,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Some(Default::default()),
        Err(e) => {
            tracing::warn!("legacy layout {} could not be read: {e}", legacy.display());
            return None;
        }
    };
    match classify_slot_json(&json) {
        SlotLoad::Loaded(layout) => Some(layout.collect_scrollback_refs()),
        SlotLoad::Absent | SlotLoad::Unreadable | SlotLoad::Unparsable => {
            tracing::warn!(
                "legacy layout {} could not be parsed or has a newer version",
                legacy.display()
            );
            None
        }
    }
}

/// 읽은 모든 슬롯과 남은 옛 layout.json의 scrollback 참조를 합쳐야 다른 창의 파일을 지우지 않는다.
/// 참조 집합을 확정하지 못하면(목록 불완전, 슬롯이나 옛 파일을 읽지 못함) 그 회차 GC를 생략한다.
/// layouts 디렉터리가 없고 옛 파일도 없으면 참조가 없다고 확정되므로 모든 bin을 지운다.
fn gc_scrollback_orphans_all_slots_in(layouts: &Path, legacy: &Path, scrollback: &Path) {
    let listed = list_slots_in(layouts);
    if !listed.complete {
        tracing::warn!(
            "scrollback GC skipped: the layout slots in {} could not be listed completely — \
             keeping every scrollback file",
            layouts.display()
        );
        return;
    }
    let mut union = std::collections::HashSet::new();
    for slot in listed.slots {
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
    match legacy_scrollback_refs(legacy) {
        Some(refs) => union.extend(refs),
        None => {
            tracing::warn!(
                "scrollback GC skipped: the legacy layout {} could not be read — keeping every \
                 scrollback file",
                legacy.display()
            );
            return;
        }
    }
    crate::scrollback_store::gc_orphans_in(scrollback, &union);
}

fn gc_scrollback_orphans_all_slots(home: &Path) {
    let Some(scrollback) = crate::scrollback_store::scrollback_dir() else {
        return;
    };
    gc_scrollback_orphans_all_slots_in(
        &home.join(LAYOUTS_SUBDIR),
        &home.join(LEGACY_LAYOUT_FILE),
        &scrollback,
    );
}

/// engine 생성 전에 마이그레이션하고 GC한다. 옮겨진 슬롯의 참조도 GC에 포함해야 한다.
/// restore_layout이 꺼져 있으면 저장하지 않는 레이아웃을 근거로 지우지 않도록 GC를 생략한다.
/// 마이그레이션은 설정과 무관하게 시도한다.
pub(crate) fn migrate_and_gc_on_boot(restore_layout: bool) {
    let Some(home) = tasty_utils::path::tasty_home() else {
        return;
    };
    migrate_legacy_in(&home);
    if restore_layout {
        gc_scrollback_orphans_all_slots(&home);
    }
}

#[derive(Default)]
pub struct LayoutDirtyTracker {
    generation: std::sync::Arc<()>,
    dirty: bool,
    dirty_since: Option<Instant>,
}

impl LayoutDirtyTracker {
    pub fn new() -> Self {
        Self::default()
    }
}

impl LayoutDirtyTracker {
    pub(crate) fn generation(&self) -> std::sync::Weak<()> {
        std::sync::Arc::downgrade(&self.generation)
    }
    pub(crate) fn clear_if_generation(&mut self, expected: &std::sync::Weak<()>) {
        if expected.ptr_eq(&self.generation()) {
            self.dirty = false;
            self.dirty_since = None;
        }
    }

    /// 처음 변경된 시각을 유지해 연속 변경이 저장 예약을 계속 늦추지 않게 한다.
    /// 실제 저장 시각·성공 여부를 보장하지는 않는다.
    pub fn mark_dirty(&mut self) {
        self.generation = std::sync::Arc::new(());
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
    #[cfg(test)]
    pub fn clear(&mut self) {
        self.dirty = false;
        self.dirty_since = None;
    }

    pub fn is_dirty(&self) -> bool {
        self.dirty
    }
}
