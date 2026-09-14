//! 설정 창 안의 로컬 파일 선택 — 메인 창 파일 피커 popup 의 **순수 view 를 재사용**한다.
//!
//! 설정 창은 메인 윈도우와 별개의 winit 창이라 메인 창 popup 스택에 사는 파일 피커를
//! 그대로 열 수 없다. 대신 view(`draw_file_picker_view`)가 AppState/CoreState 에
//! 기대지 않으므로, 상태를 이 창의 `SettingsUiState` 에 두고 이 창의 `PopupManager`
//! 에서 같은 view 를 그린다.
//!
//! - **로컬 전용**: 원격(attach mirror) 조회 경로는 메인 창 App 루프가 소유한다. 설정은
//!   이 인스턴스 자신의 구성이라 로컬 파일시스템만 본다.
//! - **OS 네이티브 다이얼로그를 쓰지 않는 이유**: 포털 없는 Linux 에서 끝나지 않는다
//!   (`docs/adr/0162-a-host-blocking-native-dialog-is-not-an-agent-surface.md`). 여기서
//!   쓰는 `read_dir_entries` 는 프레임 안의 동기 I/O 라 느린 디스크에서는 그 프레임이
//!   늘어지지만 **유한하게 끝난다** — 메인 창 파일 피커의 로컬 경로와 같은 성질이다.
//! - **저장 모드**: view 의 footer 이름 칸이 편집 가능해지고 footer primary 버튼이 **유일한
//!   확정 수단**이다. 목록에서 파일을 고르면 확정이 아니라 이름 칸이 채워지고, 이름이 고른
//!   행과 달라지는 순간 선택이 풀린다 — "고른 파일" 과 "입력한 이름" 이 두 경로가 되지 않는다.
//!
//! 기능 문서: `docs/features/native-file-picker/index.md` "설정 창에서의 로컬 전용 재사용".

use std::path::{Path, PathBuf};

use tasty_type_appearance::theme::Theme;

// 크기는 메인 창 파일 피커와 같은 상수를 읽는다 — 저장 모드도 같은 640×480 프레임이다.
use crate::adapters::ui::popup::file_picker::{
    CrumbView, FilePickerAction, FilePickerEntryView, FilePickerMode, FilePickerProps, FpViewState,
    POPUP_HEIGHT, POPUP_WIDTH, crumb_label, draw_file_picker_view, matches_filters, path_ancestors,
};
use crate::core::fs_list::DirEntryInfo;
use crate::i18n::t;

/// 설정 창 `PopupManager` 에 등록되는 id.
pub(crate) const FILE_CHOOSER_POPUP_ID: &str = "settings_file_chooser";

/// 무엇을 고르는가.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum FileChooserMode {
    /// 기존 파일 하나를 고른다.
    Open,
    /// 디렉토리를 고르고 파일명을 입력해 저장 경로를 정한다. `default_name` 은 이름 칸의
    /// 초깃값이다.
    Save { default_name: String },
}

/// 선택 결과. 호출처가 자기 `consumer` 키로 [`SettingsFileChooser::take_outcome`] 한다.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum FileChooserOutcome {
    Confirmed(PathBuf),
    Cancelled,
}

/// 열려 있는 선택 화면 하나의 상태.
struct ChooserSession {
    consumer: &'static str,
    mode: FileChooserMode,
    current_dir: PathBuf,
    entries: Vec<DirEntryInfo>,
    load: FpViewState,
    /// 선택된 행. 저장 모드에서는 비었거나 `[save_name]`(나열된 파일) 둘 중 하나다.
    selected: Vec<String>,
    /// 확장자 필터(점 없이). 비면 필터 없음. 디렉토리는 거르지 않는다.
    filters: Vec<String>,
    /// 저장 모드의 파일명 입력.
    save_name: String,
    /// 호출처가 정한 타이틀. 없으면 모드 기본 문구.
    title: Option<&'static str>,
}

/// `SettingsUiState` 가 갖는 파일 선택 상태. 한 번에 하나만 열린다.
#[derive(Default)]
pub(crate) struct SettingsFileChooser {
    session: Option<ChooserSession>,
    /// 닫힌 선택의 결과. 호출처가 가져갈 때까지 남는다.
    outcome: Option<(&'static str, FileChooserOutcome)>,
}

impl SettingsFileChooser {
    /// 선택 화면 상태를 만들고 홈 디렉토리를 동기로 읽는다. popup 을 여는 것은 호출처
    /// (`SettingsUiState::open_file_chooser`)다 — 이 타입은 popup 매니저를 모른다.
    pub(crate) fn begin(
        &mut self,
        consumer: &'static str,
        mode: FileChooserMode,
        filters: Vec<String>,
    ) {
        let start = directories::BaseDirs::new()
            .map(|d| d.home_dir().to_path_buf())
            .unwrap_or_else(|| PathBuf::from("/"));
        self.begin_at(consumer, mode, filters, start);
    }

    fn begin_at(
        &mut self,
        consumer: &'static str,
        mode: FileChooserMode,
        filters: Vec<String>,
        start: PathBuf,
    ) {
        let save_name = match &mode {
            FileChooserMode::Save { default_name } => default_name.clone(),
            FileChooserMode::Open => String::new(),
        };
        let mut session = ChooserSession {
            consumer,
            mode,
            current_dir: start,
            entries: Vec::new(),
            load: FpViewState::Empty,
            selected: Vec::new(),
            filters,
            save_name,
            title: None,
        };
        session.reload();
        self.session = Some(session);
        self.outcome = None;
    }

    /// 열려 있는 선택의 타이틀을 호출처 문구로 바꾼다(popup 타이틀과 view 헤더 공용).
    pub(crate) fn set_title(&mut self, title: &'static str) {
        if let Some(s) = self.session.as_mut() {
            s.title = Some(title);
        }
    }

    /// 열려 있는 선택의 타이틀.
    pub(crate) fn title(&self) -> &'static str {
        match self.session.as_ref() {
            Some(s) => s.title(),
            None => chooser_title(false),
        }
    }

    /// popup 이 view 밖의 경로(타이틀바 ✕)로 닫혔을 때 — 미확정이면 취소로 남긴다.
    pub(crate) fn cancel(&mut self) {
        if let Some(s) = self.session.take() {
            self.outcome = Some((s.consumer, FileChooserOutcome::Cancelled));
        }
    }

    /// `consumer` 가 연 선택의 결과를 1 회 가져간다.
    pub(crate) fn take_outcome(&mut self, consumer: &'static str) -> Option<FileChooserOutcome> {
        if self.outcome.as_ref().is_some_and(|(c, _)| *c == consumer) {
            self.outcome.take().map(|(_, o)| o)
        } else {
            None
        }
    }

    /// popup 콘텐츠. 닫혀야 하면 `true`.
    pub(crate) fn draw(&mut self, ui: &mut egui::Ui, th: &Theme, owns_escape: bool) -> bool {
        let Some(s) = self.session.as_mut() else {
            return true;
        };
        let action = draw_view(ui, th, s, owns_escape);
        match s.apply(action) {
            Some(o) => {
                self.finish(o);
                true
            }
            None => false,
        }
    }

    fn finish(&mut self, outcome: FileChooserOutcome) {
        if let Some(s) = self.session.take() {
            self.outcome = Some((s.consumer, outcome));
        }
    }
}

/// popup 크기 — 두 모드 모두 메인 피커와 같은 640×480 이다. 저장 모드의 이름 칸·경고 줄은
/// footer 안에 있고 본문이 그만큼 준다.
pub(crate) fn chooser_size() -> egui::Vec2 {
    egui::vec2(POPUP_WIDTH.value(), POPUP_HEIGHT.value())
}

/// popup 타이틀. view 헤더도 같은 문자열을 쓴다.
pub(crate) fn chooser_title(save_mode: bool) -> &'static str {
    if save_mode {
        t("filepicker.save.title")
    } else {
        t("filepicker.title")
    }
}

impl ChooserSession {
    fn title(&self) -> &'static str {
        self.title
            .unwrap_or_else(|| chooser_title(matches!(self.mode, FileChooserMode::Save { .. })))
    }

    /// 현재 디렉토리를 동기로 다시 읽는다. 선택은 비운다.
    fn reload(&mut self) {
        self.selected.clear();
        match crate::core::fs_list::read_dir_entries(&self.current_dir) {
            Ok(mut entries) => {
                crate::core::fs_list::sort_entries(
                    &mut entries,
                    tasty_model::SortColumn::Name,
                    tasty_model::SortDir::Asc,
                );
                self.load = if entries.is_empty() {
                    FpViewState::Empty
                } else {
                    FpViewState::Loaded
                };
                self.entries = entries;
            }
            Err(e) => {
                let msg = if e.kind() == std::io::ErrorKind::PermissionDenied {
                    t("filepicker.error_perm.reason_permission").to_string()
                } else {
                    e.to_string()
                };
                self.entries.clear();
                self.load = FpViewState::ErrorPerm(msg);
            }
        }
    }

    fn navigate(&mut self, target: PathBuf) {
        self.current_dir = target;
        self.reload();
    }

    fn ancestors(&self) -> Vec<String> {
        path_ancestors(false, &self.current_dir.to_string_lossy())
    }

    fn visible(&self, e: &DirEntryInfo) -> bool {
        e.is_dir || matches_filters(&self.filters, &e.name)
    }

    fn is_file_entry(&self, name: &str) -> bool {
        self.entries
            .iter()
            .any(|e| e.name == name && !e.is_dir && self.visible(e))
    }

    fn is_save(&self) -> bool {
        matches!(self.mode, FileChooserMode::Save { .. })
    }

    /// 저장 모드: 입력한 이름이 지금 나열된 폴더에 파일로 이미 있다(덮어쓰기 상태).
    /// 판정은 나열된 목록만 본다 — 나열되지 않은 경로를 stat 하지 않는다.
    fn overwrites(&self) -> bool {
        self.is_save() && self.is_file_entry(self.save_name.trim())
    }

    /// view 의 의도를 상태 변경으로 환원한다. 선택이 끝났으면 결과를 낸다.
    fn apply(&mut self, action: FilePickerAction) -> Option<FileChooserOutcome> {
        match action {
            FilePickerAction::None => None,
            FilePickerAction::Cancel => Some(FileChooserOutcome::Cancelled),
            FilePickerAction::Select(name) => {
                if self.is_save() {
                    // 저장 모드에서 행을 고르는 것은 확정이 아니라 이름 칸을 채우는 것이다.
                    // 폴더 행은 이름이 될 수 없으므로 선택도 이름도 바꾸지 않는다(진입은 더블클릭).
                    if self.is_file_entry(&name) {
                        self.save_name = name.clone();
                        self.selected = vec![name];
                    }
                } else {
                    self.selected = vec![name];
                }
                None
            }
            FilePickerAction::EditName(name) => {
                if self.is_save() {
                    // 이름이 고른 행과 달라지는 순간 선택이 풀린다 — 화면에 두 답이 남지 않는다.
                    if self.selected.first() != Some(&name) {
                        self.selected.clear();
                    }
                    self.save_name = name;
                }
                None
            }
            FilePickerAction::NavigateInto(name) => {
                let target = self.current_dir.join(name);
                self.navigate(target);
                None
            }
            FilePickerAction::NavigateTo(idx) => {
                if let Some(target) = self.ancestors().get(idx) {
                    self.navigate(PathBuf::from(target));
                }
                None
            }
            FilePickerAction::NavigateUp => {
                let ancestors = self.ancestors();
                if ancestors.len() > 1 {
                    self.navigate(PathBuf::from(&ancestors[ancestors.len() - 2]));
                }
                None
            }
            FilePickerAction::Refresh => {
                self.reload();
                None
            }
            // 저장 모드의 확정 대상은 이름 칸 하나다.
            FilePickerAction::Confirm if self.is_save() => self.confirm_save(),
            FilePickerAction::Confirm => {
                // view 의 활성 조건을 우회해도 디렉토리를 파일로 확정하지 않는다.
                let [name] = self.selected.as_slice() else {
                    return None;
                };
                self.is_file_entry(name)
                    .then(|| FileChooserOutcome::Confirmed(self.current_dir.join(name)))
            }
            // 저장 모드의 파일 행 더블클릭은 고르기와 같다 — 덮어쓰기 경고를 보지 않고 기존
            // 파일을 확정하는 길을 만들지 않는다.
            FilePickerAction::ConfirmEntry(name) if self.is_save() => {
                self.apply(FilePickerAction::Select(name))
            }
            FilePickerAction::ConfirmEntry(name) => self
                .is_file_entry(&name)
                .then(|| FileChooserOutcome::Confirmed(self.current_dir.join(name))),
        }
    }

    /// 저장 모드 이름 칸의 확정 — 현재 디렉토리가 읽혔고 파일명이 한 경로 성분이며, 나열된
    /// 폴더 이름과 겹치지 않을 때만(폴더 자리에 파일을 쓸 수 없다).
    fn confirm_save(&self) -> Option<FileChooserOutcome> {
        if !matches!(self.load, FpViewState::Loaded | FpViewState::Empty) {
            return None;
        }
        let name = self.save_name.trim();
        let is_listed_dir = self.entries.iter().any(|e| e.is_dir && e.name == name);
        (is_plain_file_name(name) && !is_listed_dir)
            .then(|| FileChooserOutcome::Confirmed(self.current_dir.join(name)))
    }
}

/// 디렉토리 구분자·`.`·`..` 가 없는 한 성분짜리 이름인가. 이름 칸이 다른 디렉토리를
/// 가리키게 두지 않는다 — 디렉토리 이동은 목록이 한다.
fn is_plain_file_name(name: &str) -> bool {
    let mut comps = Path::new(name).components();
    matches!(
        (comps.next(), comps.next()),
        (Some(std::path::Component::Normal(_)), None)
    ) && !name.contains(['/', '\\'])
}

fn draw_view(
    ui: &mut egui::Ui,
    th: &Theme,
    s: &ChooserSession,
    owns_escape: bool,
) -> FilePickerAction {
    let crumbs: Vec<CrumbView> = s
        .ancestors()
        .iter()
        .map(|full| CrumbView {
            label: crumb_label(false, full),
        })
        .collect();
    let entries: Vec<FilePickerEntryView> = s
        .entries
        .iter()
        .filter(|e| s.visible(e))
        .map(|e| FilePickerEntryView {
            name: e.name.clone(),
            is_dir: e.is_dir,
            size_display: crate::core::fs_list::human_size(e.is_dir, e.size),
            modified_display: crate::core::fs_list::format_modified(e.modified),
        })
        .collect();
    let selection_text = s.selected.join(", ");
    let overwrite = s.overwrites();
    let (mode, name_placeholder, confirm_label) = if s.is_save() {
        (
            FilePickerMode::Save {
                name: &s.save_name,
                overwrite,
                can_confirm: s.confirm_save().is_some(),
            },
            t("filepicker.save.name_placeholder"),
            if overwrite {
                t("filepicker.save.overwrite_button")
            } else {
                t("filepicker.save.save_button")
            },
        )
    } else {
        (
            FilePickerMode::Open {
                selection_text: &selection_text,
            },
            t("filepicker.no_file_selected"),
            t("filepicker.open_button"),
        )
    };
    let props = FilePickerProps {
        theme: th,
        remote_host: None,
        crumbs: &crumbs,
        state: s.load.clone(),
        entries: &entries,
        selected: &s.selected,
        mode,
        owns_escape,
        title_label: s.title(),
        name_field_label: t("filepicker.name_field_label"),
        name_placeholder,
        cancel_label: t("button.cancel"),
        confirm_label,
        overwrite_warning: t("filepicker.save.overwrite_warning"),
        hidden_folders_label: t("filepicker.hidden_folders"),
        empty_label: t("filepicker.empty"),
        loading_label: t("filepicker.loading"),
        loading_body_local: t("filepicker.loading_body_local"),
        loading_body_remote: t("filepicker.loading_body_remote"),
        error_perm_title: t("filepicker.error_perm.title"),
        error_perm_retry: t("filepicker.error_perm.retry"),
        error_conn_title: t("filepicker.error_conn.title"),
        error_conn_reconnect: t("filepicker.error_conn.reconnect"),
    };
    draw_file_picker_view(ui, &props)
}

#[cfg(test)]
mod tests {
    use super::*;

    const C: &str = "test_consumer";

    fn tempdir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "tasty-settings-file-chooser-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::create_dir_all(dir.join("sub")).expect("mkdir");
        std::fs::write(dir.join("a.lua"), "x").expect("write");
        std::fs::write(dir.join("b.txt"), "x").expect("write");
        dir
    }

    fn session(chooser: &mut SettingsFileChooser) -> &mut ChooserSession {
        chooser.session.as_mut().expect("open session")
    }

    #[test]
    fn open_mode_confirms_existing_file_and_hands_path_to_consumer() {
        let dir = tempdir();
        let mut ch = SettingsFileChooser::default();
        ch.begin_at(C, FileChooserMode::Open, Vec::new(), dir.clone());
        assert!(session(&mut ch).load == FpViewState::Loaded);

        let s = session(&mut ch);
        assert_eq!(s.apply(FilePickerAction::Select("a.lua".into())), None);
        let out = s.apply(FilePickerAction::Confirm).expect("confirmed");
        ch.finish(out);

        assert_eq!(ch.take_outcome("someone_else"), None);
        assert_eq!(
            ch.take_outcome(C),
            Some(FileChooserOutcome::Confirmed(dir.join("a.lua")))
        );
        assert_eq!(ch.take_outcome(C), None, "결과는 한 번만 가져간다");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn directories_and_filtered_out_files_are_not_confirmable() {
        let dir = tempdir();
        let mut ch = SettingsFileChooser::default();
        ch.begin_at(C, FileChooserMode::Open, vec!["lua".into()], dir.clone());
        let s = session(&mut ch);
        assert_eq!(s.apply(FilePickerAction::ConfirmEntry("sub".into())), None);
        assert_eq!(
            s.apply(FilePickerAction::ConfirmEntry("b.txt".into())),
            None
        );
        s.apply(FilePickerAction::Select("sub".into()));
        assert_eq!(s.apply(FilePickerAction::Confirm), None);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn navigation_moves_into_and_back_up() {
        let dir = tempdir();
        let mut ch = SettingsFileChooser::default();
        ch.begin_at(C, FileChooserMode::Open, Vec::new(), dir.clone());
        let s = session(&mut ch);
        s.apply(FilePickerAction::NavigateInto("sub".into()));
        assert_eq!(s.current_dir, dir.join("sub"));
        assert!(s.load == FpViewState::Empty);
        s.apply(FilePickerAction::NavigateUp);
        assert_eq!(s.current_dir, dir);
        let root_idx = 0;
        s.apply(FilePickerAction::NavigateTo(root_idx));
        assert_eq!(s.current_dir.parent(), None, "첫 크럼은 루트다");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn unreadable_directory_is_an_error_state_not_a_hang() {
        let dir = tempdir();
        let mut ch = SettingsFileChooser::default();
        ch.begin_at(
            C,
            FileChooserMode::Open,
            Vec::new(),
            dir.join("does-not-exist"),
        );
        assert!(matches!(session(&mut ch).load, FpViewState::ErrorPerm(_)));
        assert_eq!(session(&mut ch).confirm_save(), None);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn save_mode_joins_typed_name_and_rejects_path_like_names() {
        let dir = tempdir();
        let mut ch = SettingsFileChooser::default();
        ch.begin_at(
            C,
            FileChooserMode::Save {
                default_name: "keys.toml".into(),
            },
            Vec::new(),
            dir.clone(),
        );
        let s = session(&mut ch);
        assert!(s.is_save());
        assert_eq!(
            s.confirm_save(),
            Some(FileChooserOutcome::Confirmed(dir.join("keys.toml")))
        );
        for bad in [
            "",
            "  ",
            ".",
            "..",
            "sub/x.toml",
            "../x.toml",
            "a\\b",
            "sub",
        ] {
            s.save_name = bad.into();
            assert_eq!(s.confirm_save(), None, "{bad:?} 는 저장 이름이 아니다");
        }
        // 기존 파일을 고르면 그 이름이 입력으로 간다.
        s.apply(FilePickerAction::Select("b.txt".into()));
        assert_eq!(s.save_name, "b.txt");
        // 디렉토리를 고르는 것은 입력도 선택도 바꾸지 않는다.
        s.apply(FilePickerAction::Select("sub".into()));
        assert_eq!(s.save_name, "b.txt");
        assert_eq!(s.selected, vec!["b.txt".to_string()]);
        std::fs::remove_dir_all(&dir).ok();
    }

    fn save_session(dir: &Path) -> SettingsFileChooser {
        let mut ch = SettingsFileChooser::default();
        ch.begin_at(
            C,
            FileChooserMode::Save {
                default_name: "keys.toml".into(),
            },
            Vec::new(),
            dir.to_path_buf(),
        );
        ch
    }

    /// 저장 모드의 확정 대상은 이름 칸 하나다 — 목록에서 고른 뒤 이름을 고치면 확정은 고친
    /// 이름을 돌려주고, 고른 행은 풀린다.
    #[test]
    fn save_mode_has_one_target_the_name_field() {
        let dir = tempdir();
        let mut ch = save_session(&dir);
        let s = session(&mut ch);

        // 새 이름 — 덮어쓰기 아님.
        assert!(!s.overwrites());

        // 기존 파일을 고르면 확정이 아니라 이름 칸이 채워지고, 곧 덮어쓰기 상태다.
        assert_eq!(s.apply(FilePickerAction::Select("a.lua".into())), None);
        assert_eq!(s.save_name, "a.lua");
        assert_eq!(s.selected, vec!["a.lua".to_string()]);
        assert!(s.overwrites());

        // 이름을 고친 순간 선택이 풀리고 덮어쓰기에서 벗어난다.
        assert_eq!(s.apply(FilePickerAction::EditName("a2.lua".into())), None);
        assert!(
            s.selected.is_empty(),
            "이름이 고른 행과 달라지면 선택이 풀린다"
        );
        assert!(!s.overwrites());

        // 확정은 이름 칸을 읽는다 — 앞서 고른 파일이 아니다.
        assert_eq!(
            s.apply(FilePickerAction::Confirm),
            Some(FileChooserOutcome::Confirmed(dir.join("a2.lua")))
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    /// 고른 이름과 같은 값으로의 편집(예: 같은 글자를 지웠다 다시 씀)은 선택을 유지한다.
    #[test]
    fn save_mode_keeps_selection_while_the_name_still_matches() {
        let dir = tempdir();
        let mut ch = save_session(&dir);
        let s = session(&mut ch);
        s.apply(FilePickerAction::Select("a.lua".into()));
        s.apply(FilePickerAction::EditName("a.lua".into()));
        assert_eq!(s.selected, vec!["a.lua".to_string()]);
        std::fs::remove_dir_all(&dir).ok();
    }

    /// 직접 입력한 이름이 나열된 파일과 같아도 덮어쓰기 상태다(판정은 나열된 폴더 조회).
    #[test]
    fn typed_existing_name_is_an_overwrite() {
        let dir = tempdir();
        let mut ch = save_session(&dir);
        let s = session(&mut ch);
        s.apply(FilePickerAction::EditName("b.txt".into()));
        assert!(s.overwrites());
        assert!(s.selected.is_empty());
        std::fs::remove_dir_all(&dir).ok();
    }

    /// 저장 모드에서 파일 행 더블클릭은 확정하지 않는다 — 덮어쓰기 경고를 건너뛰는 길이다.
    #[test]
    fn save_mode_double_click_fills_the_name_instead_of_confirming() {
        let dir = tempdir();
        let mut ch = save_session(&dir);
        let s = session(&mut ch);
        assert_eq!(
            s.apply(FilePickerAction::ConfirmEntry("a.lua".into())),
            None
        );
        assert_eq!(s.save_name, "a.lua");
        assert!(s.overwrites());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn external_close_leaves_a_cancel_outcome() {
        let dir = tempdir();
        let mut ch = SettingsFileChooser::default();
        ch.begin_at(C, FileChooserMode::Open, Vec::new(), dir.clone());
        ch.cancel();
        assert_eq!(ch.take_outcome(C), Some(FileChooserOutcome::Cancelled));
        std::fs::remove_dir_all(&dir).ok();
    }
}
