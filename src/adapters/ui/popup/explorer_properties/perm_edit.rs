//! Properties 의 권한 편집 상자. 시안 `PermEditor`(갤러리 "Properties actions")를 따른다.
//! Permissions 행의 Edit 가 행 아래에 연다. Unix 는 소유자·그룹·기타 × 읽기·쓰기·실행 3×3 체크박스와
//! 바뀔 모드(글자·8진), Windows 는 "읽기 전용" 특성 하나다. Apply 전에는 아무것도 쓰지 않는다.
//! Apply 는 explorer 파일 작업 대기열로 가고, 끝나면 성공은 상자를 닫고 실패는 이유를 보인다.
//! 둘 다 실제 값을 다시 읽는다. 폴더 안으로 내려가지 않고 권한 상승을 요청하지 않는다.

use std::path::PathBuf;

use tasty_ui_widgets::{PermBoxClick, PermBoxProps, PermBoxText, PermBoxValue, perm_box};

use crate::app::file_jobs::permissions::PermChange;
use crate::app::local_reads::{ItemFacts, ItemKind};
use crate::i18n::t;
use crate::theme::Theme;

/// 편집 상자 상태.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Phase {
    Editing,
    /// 대기열에 넣은 요청을 기다린다. 컨트롤을 잠근다.
    Applying(u64),
    /// 적용하지 못했다. OS 이유를 보이고 다시 읽은 값으로 칸을 맞춘다.
    Failed(String),
}

/// 열린 편집 상자.
#[derive(Debug, Clone)]
pub(crate) struct PermEdit {
    pub(crate) path: PathBuf,
    /// 마지막으로 읽은 값. Apply 는 `draft` 가 이것과 다를 때만 켠다.
    pub(crate) current: PermChange,
    pub(crate) draft: PermChange,
    pub(crate) folder: bool,
    /// setuid·setgid·sticky 가 있으면 남긴다는 줄을 보인다.
    pub(crate) special: bool,
    pub(crate) phase: Phase,
}

/// 상자 안에서 누른 것.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PermClick {
    None,
    Cancel,
    Apply,
}

/// 권한 행이 보일 것: 편집 가능, 소유자 아님 안내, 아무것도 없음.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PermAccess {
    Editable,
    Locked,
    None,
}

/// 한 로컬 항목의 권한을 바꿀 수 있는가. 링크는 바꾸지 않는다. Unix 는 소유자이거나 root 여야 한다.
pub(crate) fn access(item: &ItemFacts) -> PermAccess {
    if matches!(item.kind, ItemKind::Link { .. }) {
        return PermAccess::None;
    }
    if cfg!(windows) {
        return PermAccess::Editable;
    }
    match (item.mode_bits, item.can_change_mode) {
        (Some(_), true) => PermAccess::Editable,
        (Some(_), false) => PermAccess::Locked,
        (None, _) => PermAccess::None,
    }
}

/// 읽은 값으로 바꿀 값을 만든다.
pub(crate) fn current_of(item: &ItemFacts) -> Option<PermChange> {
    if cfg!(windows) {
        Some(PermChange::ReadOnly(item.read_only))
    } else {
        item.mode_bits.map(|bits| PermChange::Mode(bits & 0o777))
    }
}

impl PermEdit {
    pub(crate) fn open(item: &ItemFacts) -> Option<Self> {
        let current = current_of(item)?;
        Some(Self {
            path: item.path.clone(),
            current,
            draft: current,
            folder: matches!(item.kind, ItemKind::Folder),
            special: item.mode_bits.is_some_and(|bits| bits & 0o7000 != 0),
            phase: Phase::Editing,
        })
    }

    /// 다시 읽은 값으로 맞춘다. 실패 뒤라면 칸도 실제 값으로 돌린다.
    pub(crate) fn reread(&mut self, item: &ItemFacts) {
        if item.path != self.path {
            return;
        }
        let Some(current) = current_of(item) else {
            return;
        };
        self.current = current;
        self.special = item.mode_bits.is_some_and(|bits| bits & 0o7000 != 0);
        if !matches!(self.phase, Phase::Applying(_)) {
            self.draft = current;
        }
    }

    pub(crate) fn changed(&self) -> bool {
        self.draft != self.current
    }

    pub(crate) fn busy(&self) -> bool {
        matches!(self.phase, Phase::Applying(_))
    }
}

/// 상자를 그린다. 배치는 공용 `perm_box` 가 맡고, 여기서는 상태와 문구를 넘긴다.
pub(crate) fn draw(ui: &mut egui::Ui, th: &Theme, edit: &mut PermEdit) -> PermClick {
    let busy = edit.busy();
    let changed = edit.changed();
    let mut notes = Vec::new();
    if edit.folder {
        notes.push(t("explorer.props.perm.folder_only"));
    }
    if edit.special {
        notes.push(t("explorer.props.perm.special_kept"));
    }
    let note = notes.join(" ");
    let failure = match &edit.phase {
        Phase::Failed(reason) => Some(reason.as_str()),
        _ => None,
    };
    let value = match &mut edit.draft {
        PermChange::Mode(bits) => PermBoxValue::Mode(bits),
        PermChange::ReadOnly(read_only) => PermBoxValue::ReadOnly(read_only),
    };
    let click = perm_box(
        ui,
        th,
        PermBoxProps {
            text: box_text(),
            value,
            note: (!note.is_empty()).then_some(note.as_str()),
            failure,
            busy,
            changed,
        },
    );
    match click {
        PermBoxClick::None => PermClick::None,
        PermBoxClick::Cancel => PermClick::Cancel,
        PermBoxClick::Apply => PermClick::Apply,
    }
}

fn box_text() -> PermBoxText<'static> {
    PermBoxText {
        heads: [
            t("explorer.props.perm.read"),
            t("explorer.props.perm.write"),
            t("explorer.props.perm.execute"),
        ],
        roles: [
            t("explorer.props.perm.owner"),
            t("explorer.props.perm.group"),
            t("explorer.props.perm.others"),
        ],
        read_only: t("explorer.props.perm.read_only"),
        windows_hint: t("explorer.props.perm.windows_hint"),
        failed: t("explorer.props.perm.failed"),
        apply: t("explorer.props.perm.apply"),
        applying: t("explorer.props.perm.applying"),
        cancel: t("button.cancel"),
    }
}

#[cfg(test)]
mod tests {
    // 권한 판정 시험은 unix 모드 비트를 쓰므로 그 시험만 쓰는 준비물도 unix 에서만 둔다.
    #[cfg(unix)]
    use super::*;
    #[cfg(unix)]
    use std::path::PathBuf;

    #[cfg(unix)]
    fn item(kind: ItemKind, bits: Option<u32>, owner: bool) -> ItemFacts {
        ItemFacts {
            path: PathBuf::from("/w/deploy.env"),
            kind,
            size: 0,
            modified: None,
            created: None,
            mode: None,
            read_only: false,
            mode_bits: bits,
            can_change_mode: owner,
            hardlink: None,
        }
    }

    #[test]
    fn the_readout_names_the_mode_and_its_octal() {
        use tasty_ui_widgets::perm_readout as readout;
        assert_eq!(readout(0o644), "rw-r--r-- · 644");
        assert_eq!(readout(0o750), "rwxr-x--- · 750");
        assert_eq!(readout(0o4755), "rwxr-xr-x · 755");
    }

    #[cfg(unix)]
    #[test]
    fn only_the_owner_or_root_edits_and_links_never_do() {
        assert_eq!(
            access(&item(ItemKind::File, Some(0o644), true)),
            PermAccess::Editable
        );
        assert_eq!(
            access(&item(ItemKind::File, Some(0o644), false)),
            PermAccess::Locked
        );
        let link = ItemKind::Link {
            target: PathBuf::from("t"),
            state: crate::app::local_reads::LinkTarget::Missing,
        };
        assert_eq!(access(&item(link, None, true)), PermAccess::None);
    }

    #[cfg(unix)]
    #[test]
    fn apply_turns_on_only_after_a_change_and_a_failure_rereads_the_real_mode() {
        let mut edit = PermEdit::open(&item(ItemKind::Folder, Some(0o1775), true)).unwrap();
        assert!(edit.folder && edit.special);
        assert!(!edit.changed());
        edit.draft = PermChange::Mode(0o770);
        assert!(edit.changed());
        edit.phase = Phase::Applying(7);
        // 적용 중에 읽은 값은 칸을 덮지 않는다.
        edit.reread(&item(ItemKind::Folder, Some(0o1775), true));
        assert_eq!(edit.draft, PermChange::Mode(0o770));
        edit.phase = Phase::Failed("Read-only file system (os error 30)".into());
        edit.reread(&item(ItemKind::Folder, Some(0o1775), true));
        assert_eq!(
            edit.draft,
            PermChange::Mode(0o775),
            "the grid shows the real mode"
        );
        assert!(!edit.changed());
    }
}
