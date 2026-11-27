//! 목록 키와 행 포커스를 창 키 경로(`WindowInput` → 팝업 층 `layer_frame`)로 눌러 본다. 규칙의 정본은 tasty-page spec
//! `row-focus-and-list-keys`(page fe45514) Meta 표이고, 시험 이름 앞의 [행] 이 그 표의 행이다.
use super::*;
use crate::adapters::ui::popup::window_input_test_support::{
    WindowInput, layer_click, layer_frame, painted_at,
};
use crate::state::FilePickerData;
use serde_json::json;

/// 시험 폴더: 하위 폴더 `dirs`, 빈 파일 `files`.
pub(super) fn tree(dirs: &[&str], files: &[&str]) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    for d in dirs {
        std::fs::create_dir(dir.path().join(d)).unwrap();
        std::fs::write(dir.path().join(d).join("inner.txt"), b"").unwrap();
    }
    for f in files {
        std::fs::write(dir.path().join(f), b"").unwrap();
    }
    dir
}

/// 피커가 열린 창 하나. 키는 창 키 경로로 넣고 프레임마다 팝업 층을 그린다.
pub(super) struct App<'a> {
    pub(super) state: &'a mut crate::state::RequestContext,
    pub(super) read: &'a crate::runtime::engine_read::EngineRead<'a>,
    pub(super) ctx: egui::Context,
    pub(super) input: WindowInput,
    pub(super) texts: Vec<(String, egui::Rect)>,
}

/// `mode`("open"·"save"·"folder")로 `dir` 에서 피커를 연다. 저장 모드 기본 이름은 `keep.toml` 이다.
pub(super) fn open_picker(
    state: &mut crate::state::RequestContext,
    engine: &mut crate::runtime::engine_access::EngineMut<'_>,
    mode: &str,
    dir: &std::path::Path,
) {
    crate::i18n::init("en");
    for def in crate::adapters::ui::popup::defs::all_defs() {
        state.popups.register_def(def, 1.0);
    }
    let caller = tasty_ipc::caller::CallerContext::Plugin {
        plugin_id: "com.example.keys".into(),
    };
    let origin = state.focused_surface_id(&engine.read());
    let params = json!({"mode": mode, "location": "local", "start_dir": dir, "default_name": "keep.toml", "origin_surface_id": origin});
    let response = crate::adapters::ipc::handler::file_picker::handle_trigger(
        state,
        engine,
        &caller,
        json!(1),
        &params,
    );
    assert!(response.result.is_some(), "{mode} picker opens");
    load(state);
}

/// 지금 폴더를 읽어 목록을 채운다(로컬 읽기 응답과 같은 정렬).
pub(super) fn load(state: &mut crate::state::RequestContext) {
    let d = instances::current_mut(state).unwrap();
    let mut entries =
        crate::core::fs_list::read_dir_entries(std::path::Path::new(&d.current_dir)).unwrap();
    crate::core::fs_list::sort_entries(
        &mut entries,
        tasty_model::SortColumn::Name,
        tasty_model::SortDir::Asc,
    );
    d.load = if entries.is_empty() {
        FpLoadState::Empty
    } else {
        FpLoadState::Loaded
    };
    d.entries = entries;
    d.local_query = None;
}

fn key(key: egui::Key, modifiers: egui::Modifiers) -> egui::Event {
    egui::Event::Key {
        key,
        physical_key: Some(key),
        pressed: true,
        repeat: false,
        modifiers,
    }
}

impl<'a> App<'a> {
    pub(super) fn new(
        state: &'a mut crate::state::RequestContext,
        read: &'a crate::runtime::engine_read::EngineRead<'a>,
    ) -> Self {
        state.normalize_focus(read);
        let ctx = egui::Context::default();
        let mut app = Self {
            state,
            read,
            ctx,
            input: WindowInput::default(),
            texts: Vec::new(),
        };
        app.frame();
        app.frame();
        app
    }

    pub(super) fn data(&mut self) -> &FilePickerData {
        instances::current(self.state).expect("picker open")
    }

    pub(super) fn frame(&mut self) {
        self.texts = layer_frame(&self.ctx, self.state, self.read, Vec::new());
    }

    /// 창이 받은 키 하나를 창 키 경로로 넣고 한 프레임 그린 뒤 한 프레임 더 그린다(예약 스크롤 반영).
    pub(super) fn press_with(&mut self, k: egui::Key, modifiers: egui::Modifiers) {
        let fed = self
            .input
            .key(self.state, self.read, false, key(k, modifiers));
        assert!(fed.is_some(), "{modifiers:?}+{k:?} 는 피커로 간다");
        let events = self.input.take_pass(self.state, &self.ctx);
        self.texts = layer_frame(&self.ctx, self.state, self.read, events);
        self.frame();
    }

    pub(super) fn press(&mut self, k: egui::Key) {
        self.press_with(k, egui::Modifiers::NONE);
    }

    /// 키 누름과 그 글자 사건(Space 의 " ")을 한 사건 묶음으로 넣는다.
    fn type_space(&mut self) {
        let events = vec![
            key(egui::Key::Space, egui::Modifiers::NONE),
            egui::Event::Text(" ".into()),
        ];
        assert!(
            self.input
                .key_events(self.state, self.read, false, events)
                .is_some()
        );
        let events = self.input.take_pass(self.state, &self.ctx);
        self.texts = layer_frame(&self.ctx, self.state, self.read, events);
        self.frame();
    }

    pub(super) fn at(&self, text: &str) -> egui::Rect {
        painted_at(&self.texts, text).unwrap_or_else(|| panic!("{text} 이 칠해진다"))
    }

    pub(super) fn click(&mut self, pos: egui::Pos2) {
        self.texts = layer_click(&self.ctx, self.state, self.read, pos);
    }

    pub(super) fn row_focus(&mut self) -> (Option<String>, bool) {
        let f = &self.data().row_focus;
        (f.row.clone(), f.ring)
    }

    /// 닫힌 피커가 남긴 마지막 결과(닫기 훅 `finish` 가 쌓는다).
    pub(super) fn finished(&self) -> Option<FilePickerResult> {
        self.state
            .dialogs
            .finished_file_pickers
            .last()
            .map(|f| f.result.clone())
    }

    pub(super) fn selected(&mut self) -> Vec<String> {
        self.data().selected.clone()
    }

    /// 지금 나열된 이름(정렬 순서, 필터 반영 전).
    pub(super) fn names(&mut self) -> Vec<String> {
        self.data().entries.iter().map(|e| e.name.clone()).collect()
    }
}

pub(super) fn some(s: &str) -> Option<String> {
    Some(s.to_string())
}

/// [where keys go · on open · start] 세 모드에서 연 직후 위젯 포커스·행 포커스·링이 없고, 창 키 경로의 ↓ 가 바로 첫 행을
/// 잡는다. ↑ 는 행 포커스가 없을 때 끝 행을 잡는다.
#[test]
fn every_mode_takes_list_keys_at_once_from_no_row_focus() {
    for mode in ["open", "save", "folder"] {
        let (mut state, mut session) = crate::state::tests::two_pane_fixture();
        let dir = tree(&["a-dir", "b-dir"], &["c.txt", "d.txt"]);
        open_picker(&mut state, &mut session.borrow_mut(), mode, dir.path());
        let engine = session.borrow_mut();
        let read = engine.read();
        let mut app = App::new(&mut state, &read);
        assert_eq!(app.row_focus(), (None, false), "{mode}: 연 직후");
        assert_eq!(
            app.ctx.memory(|m| m.focused()),
            None,
            "{mode}: 위젯 포커스 없음"
        );
        let names = app.names();
        app.press(egui::Key::ArrowDown);
        assert_eq!(app.row_focus(), (Some(names[0].clone()), true), "{mode}: ↓");
        let (mut state2, mut session2) = crate::state::tests::two_pane_fixture();
        open_picker(&mut state2, &mut session2.borrow_mut(), mode, dir.path());
        let engine2 = session2.borrow_mut();
        let read2 = engine2.read();
        let mut app2 = App::new(&mut state2, &read2);
        app2.press(egui::Key::ArrowUp);
        // 폴더 모드는 파일이 inert 라 끝 폴더다.
        let last = if mode == "folder" { "b-dir" } else { "d.txt" };
        assert_eq!(app2.row_focus(), (some(last), true), "{mode}: ↑");
    }
}

/// [start · ↑ ↓ · Home · End · new in folder mode] Home·End·PageDown·PageUp 도 행 포커스 없이 첫·끝 행을 잡고, 끝에서
/// 멈추며, 폴더 모드는 inert 파일을 건너뛴다.
#[test]
fn home_end_and_page_keys_start_and_stop_at_the_ends() {
    for mode in ["open", "folder"] {
        let (first, last) = if mode == "folder" {
            ("a-dir", "b-dir")
        } else {
            ("a-dir", "d.txt")
        };
        // 네 행이 모두 보이므로 PageDown 둘째 누름은 보이는 마지막 행(끝 행), PageUp 둘째 누름은 첫 행이다.
        for (k, want, then) in [
            (egui::Key::Home, first, first),
            (egui::Key::PageDown, first, last),
            (egui::Key::End, last, last),
            (egui::Key::PageUp, last, first),
        ] {
            let (mut state, mut session) = crate::state::tests::two_pane_fixture();
            let dir = tree(&["a-dir", "b-dir"], &["c.txt", "d.txt"]);
            open_picker(&mut state, &mut session.borrow_mut(), mode, dir.path());
            let engine = session.borrow_mut();
            let read = engine.read();
            let mut app = App::new(&mut state, &read);
            app.press(k);
            assert_eq!(app.row_focus().0, some(want), "{mode}: {k:?}");
            app.press(k);
            assert_eq!(app.row_focus().0, some(then), "{mode}: 둘째 {k:?}");
            app.press(k);
            assert_eq!(
                app.row_focus().0,
                some(then),
                "{mode}: 끝에서 {k:?} 는 멈춘다"
            );
        }
        let (mut state, mut session) = crate::state::tests::two_pane_fixture();
        let dir = tree(&["a-dir", "b-dir"], &["c.txt", "d.txt"]);
        open_picker(&mut state, &mut session.borrow_mut(), mode, dir.path());
        let engine = session.borrow_mut();
        let read = engine.read();
        let mut app = App::new(&mut state, &read);
        app.press(egui::Key::End);
        app.press(egui::Key::ArrowDown);
        assert_eq!(
            app.row_focus().0,
            some(last),
            "{mode}: 끝 행 ↓ 는 순환하지 않는다"
        );
        app.press(egui::Key::Home);
        app.press(egui::Key::ArrowUp);
        assert_eq!(
            app.row_focus().0,
            some(first),
            "{mode}: 첫 행 ↑ 는 순환하지 않는다"
        );
    }
}

/// [selection] 열기·폴더 모드는 이동 키가 선택도 옮기고, 저장 모드는 행 포커스만 옮긴다(이름 칸·버튼 그대로).
#[test]
fn moves_select_in_open_and_folder_mode_but_only_focus_in_save_mode() {
    for (mode, selects) in [("open", true), ("folder", true), ("save", false)] {
        let (mut state, mut session) = crate::state::tests::two_pane_fixture();
        let dir = tree(&["a-dir"], &["c.txt"]);
        open_picker(&mut state, &mut session.borrow_mut(), mode, dir.path());
        let engine = session.borrow_mut();
        let read = engine.read();
        let mut app = App::new(&mut state, &read);
        app.press(egui::Key::ArrowDown);
        assert_eq!(app.row_focus().0, some("a-dir"), "{mode}");
        let want: Vec<String> = if selects {
            vec!["a-dir".into()]
        } else {
            vec![]
        };
        assert_eq!(app.selected(), want, "{mode}: 선택");
        if mode == "save" {
            assert_eq!(app.data().save_name.as_deref(), Some("keep.toml"));
        }
    }
}

/// [selection · row focus] 저장 모드에서 이름을 고치면 고른 행 이름과 다를 때만 선택을 풀고 행 포커스는 둔다.
#[test]
fn editing_the_save_name_keeps_row_focus() {
    let (mut state, mut session) = crate::state::tests::two_pane_fixture();
    let dir = tree(&[], &["c.txt", "d.txt"]);
    open_picker(&mut state, &mut session.borrow_mut(), "save", dir.path());
    let engine = session.borrow_mut();
    let read = engine.read();
    let mut app = App::new(&mut state, &read);
    app.press(egui::Key::ArrowDown);
    app.press(egui::Key::Space);
    assert_eq!(app.selected(), vec!["c.txt".to_string()]);
    let edit = |app: &mut App<'_>, name: &str| {
        assert!(matches!(
            instances::apply_to_current(
                app.state,
                app.read,
                FilePickerAction::EditName(name.into())
            ),
            PopupAction::None
        ));
    };
    edit(&mut app, "c.txt");
    assert_eq!(
        app.selected(),
        vec!["c.txt".to_string()],
        "같은 이름: 선택 유지"
    );
    edit(&mut app, "new.toml");
    assert!(app.selected().is_empty(), "다른 이름: 선택 해제");
    assert_eq!(app.row_focus().0, some("c.txt"), "행 포커스는 그대로");
}

/// [Space] 저장 모드: 파일은 이름 칸에 그 이름을 쓰고 덮어쓰기 상태가 되며, 폴더는 고르되 이름은 둔다. 열기 모드는 클릭과
/// 같이 고른다. 링이 켜진다.
#[test]
fn space_picks_the_focused_row_like_a_click() {
    let (mut state, mut session) = crate::state::tests::two_pane_fixture();
    let dir = tree(&["a-dir"], &["c.txt"]);
    open_picker(&mut state, &mut session.borrow_mut(), "save", dir.path());
    let engine = session.borrow_mut();
    let read = engine.read();
    let mut app = App::new(&mut state, &read);
    app.press(egui::Key::ArrowDown);
    app.press(egui::Key::Space);
    assert_eq!(app.selected(), vec!["a-dir".to_string()], "폴더를 고른다");
    assert_eq!(
        app.data().save_name.as_deref(),
        Some("keep.toml"),
        "이름은 둔다"
    );
    app.press(egui::Key::ArrowDown);
    app.press(egui::Key::Space);
    assert_eq!(
        app.data().save_name.as_deref(),
        Some("c.txt"),
        "파일 이름을 쓴다"
    );
    let d = app.data();
    assert!(
        save::overwrites("c.txt", &d.entries, &d.filters),
        "덮어쓰기 상태"
    );
    assert!(
        app.texts
            .iter()
            .any(|(t, _)| t == t_str("filepicker.save.overwrite_button"))
    );
    assert_eq!(app.row_focus(), (some("c.txt"), true));
}

fn t_str(key: &'static str) -> &'static str {
    crate::i18n::t(key)
}

/// [Space · shows] 클릭은 고르고 링을 숨긴다. 그 뒤 Space 는 그 행에 링을 켜고 다른 것은 바꾸지 않는다.
#[test]
fn a_click_hides_the_ring_and_space_shows_it_on_the_clicked_row() {
    let (mut state, mut session) = crate::state::tests::two_pane_fixture();
    let dir = tree(&[], &["c.txt", "d.txt"]);
    open_picker(&mut state, &mut session.borrow_mut(), "open", dir.path());
    let engine = session.borrow_mut();
    let read = engine.read();
    let mut app = App::new(&mut state, &read);
    app.press(egui::Key::ArrowDown);
    assert_eq!(app.row_focus(), (some("c.txt"), true));
    let d = app.at("d.txt").center();
    app.click(d);
    assert_eq!(
        app.row_focus(),
        (some("d.txt"), false),
        "클릭: 행 포커스 이동, 링 숨김"
    );
    assert_eq!(app.selected(), vec!["d.txt".to_string()]);
    app.press(egui::Key::Space);
    assert_eq!(app.row_focus(), (some("d.txt"), true), "Space: 링");
    assert_eq!(app.selected(), vec!["d.txt".to_string()], "선택 그대로");
}

/// [row focus · shows] 열기 모드 목록 빈 곳 클릭은 선택·행 포커스를 두고 링만 숨긴다. 폴더 모드 빈 곳 클릭은 선택·행
/// 포커스를 지우고, 다음 → 는 무동작, 다음 ↓ 는 첫 폴더다.
#[test]
fn an_empty_list_click_hides_the_ring_and_clears_focus_only_in_folder_mode() {
    for mode in ["open", "folder"] {
        let (mut state, mut session) = crate::state::tests::two_pane_fixture();
        let dir = tree(&["a-dir", "b-dir"], &[]);
        open_picker(&mut state, &mut session.borrow_mut(), mode, dir.path());
        let engine = session.borrow_mut();
        let read = engine.read();
        let mut app = App::new(&mut state, &read);
        app.press(egui::Key::ArrowDown);
        app.press(egui::Key::ArrowDown);
        let row_h = app.at("b-dir").top() - app.at("a-dir").top();
        let below = app.at("b-dir").center() + egui::vec2(0.0, row_h * 1.5);
        let dir_before = app.data().current_dir.clone();
        app.click(below);
        if mode == "folder" {
            assert_eq!(app.row_focus(), (None, false), "{mode}");
            assert!(app.selected().is_empty(), "{mode}");
            app.press(egui::Key::ArrowRight);
            assert_eq!(app.data().current_dir, dir_before, "{mode}: → 무동작");
            app.press(egui::Key::ArrowDown);
            assert_eq!(app.row_focus().0, some("a-dir"), "{mode}: ↓ 첫 폴더");
        } else {
            assert_eq!(app.row_focus(), (some("b-dir"), false), "{mode}");
            assert_eq!(app.selected(), vec!["b-dir".to_string()], "{mode}");
        }
    }
}

/// [inert rows · shows] 폴더 모드 파일 행 클릭은 선택·행 포커스를 두고 링을 숨긴다. 행 키는 파일을 건너뛴다.
#[test]
fn a_folder_mode_file_click_hides_the_ring_and_keys_skip_files() {
    let (mut state, mut session) = crate::state::tests::two_pane_fixture();
    let dir = tree(&["a-dir"], &["c.txt", "d.txt"]);
    open_picker(&mut state, &mut session.borrow_mut(), "folder", dir.path());
    let engine = session.borrow_mut();
    let read = engine.read();
    let mut app = App::new(&mut state, &read);
    app.press(egui::Key::ArrowDown);
    app.press(egui::Key::ArrowDown);
    assert_eq!(
        app.row_focus(),
        (some("a-dir"), true),
        "파일을 건너뛰고 끝에서 멈춘다"
    );
    let file = app.at("c.txt").center();
    app.click(file);
    assert_eq!(app.row_focus(), (some("a-dir"), false));
    assert_eq!(app.selected(), vec!["a-dir".to_string()]);
}

/// [inert rows] inert 파일만 있으면 행 키는 아무것도 하지 않고, Enter 는 지금 폴더를 확정한다.
#[test]
fn only_inert_files_leave_row_keys_idle_and_enter_confirms() {
    let (mut state, mut session) = crate::state::tests::two_pane_fixture();
    let dir = tree(&[], &["c.txt"]);
    open_picker(&mut state, &mut session.borrow_mut(), "folder", dir.path());
    let engine = session.borrow_mut();
    let read = engine.read();
    let mut app = App::new(&mut state, &read);
    for k in [
        egui::Key::ArrowDown,
        egui::Key::End,
        egui::Key::PageDown,
        egui::Key::Space,
        egui::Key::ArrowRight,
    ] {
        app.press(k);
        assert_eq!(app.row_focus(), (None, false), "{k:?}");
    }
    let current = app.data().current_dir.clone();
    app.press(egui::Key::Enter);
    let finished = app.finished();
    assert_eq!(
        finished,
        Some(FilePickerResult::Confirmed {
            paths: vec![current]
        })
    );
}

/// [→ · after navigation · shows] → 는 세 모드에서 행 포커스 폴더로 들어가고 선택·행 포커스·링을 지운다. 파일에서는
/// 무동작이지만 링을 켠다.
#[test]
fn right_enters_the_focused_folder_and_shows_the_ring_on_a_file() {
    for mode in ["open", "save", "folder"] {
        let (mut state, mut session) = crate::state::tests::two_pane_fixture();
        let dir = tree(&["a-dir"], &["c.txt"]);
        open_picker(&mut state, &mut session.borrow_mut(), mode, dir.path());
        let engine = session.borrow_mut();
        let read = engine.read();
        let mut app = App::new(&mut state, &read);
        if mode != "folder" {
            app.press(egui::Key::End);
            let before = app.data().current_dir.clone();
            app.data_mut().row_focus.ring = false;
            app.press(egui::Key::ArrowRight);
            assert_eq!(app.data().current_dir, before, "{mode}: 파일 → 무동작");
            assert_eq!(app.row_focus(), (some("c.txt"), true), "{mode}: 링");
        }
        app.press(egui::Key::Home);
        app.press(egui::Key::ArrowRight);
        assert!(
            app.data().current_dir.ends_with("a-dir"),
            "{mode}: 들어간다"
        );
        assert_eq!(app.row_focus(), (None, false), "{mode}: 행 포커스 지움");
        assert!(app.selected().is_empty(), "{mode}: 선택 지움");
    }
}

impl App<'_> {
    fn data_mut(&mut self) -> &mut FilePickerData {
        instances::current_mut(self.state).expect("picker open")
    }
}

/// [Alt+↑ · Backspace · after navigation] 세 모드에서 둘 다 상위 폴더로 가고 행 포커스를 지운다. 목록을 읽는 중에도
/// 간다.
#[test]
fn alt_up_and_backspace_go_up_in_every_mode_even_while_loading() {
    for mode in ["open", "save", "folder"] {
        for (k, m) in [
            (egui::Key::ArrowUp, egui::Modifiers::ALT),
            (egui::Key::Backspace, egui::Modifiers::NONE),
        ] {
            let (mut state, mut session) = crate::state::tests::two_pane_fixture();
            let dir = tree(&["a-dir"], &[]);
            let inner = dir.path().join("a-dir");
            open_picker(&mut state, &mut session.borrow_mut(), mode, &inner);
            let engine = session.borrow_mut();
            let read = engine.read();
            let mut app = App::new(&mut state, &read);
            app.press(egui::Key::ArrowDown);
            app.press_with(k, m);
            assert_eq!(
                std::path::Path::new(&app.data().current_dir)
                    .canonicalize()
                    .unwrap(),
                dir.path().canonicalize().unwrap(),
                "{mode}: {m:?}+{k:?}"
            );
            assert_eq!(app.row_focus(), (None, false), "{mode}: 행 포커스 지움");
            // 지금은 읽는 중(Loading)이다. 한 번 더 누르면 또 올라간다.
            let loading_at = app.data().current_dir.clone();
            app.press_with(k, m);
            assert_ne!(
                app.data().current_dir,
                loading_at,
                "{mode}: 읽는 중에도 간다"
            );
        }
    }
}

/// [Alt+↑ · Backspace · priority] 저장 이름 칸이 텍스트 포커스면 Backspace·Space·Home 은 칸이 쓰고 ↓·Alt+↑ 는 목록이 쓴다.
#[test]
fn the_save_name_field_keeps_its_editing_keys() {
    let (mut state, mut session) = crate::state::tests::two_pane_fixture();
    let dir = tree(&["a-dir"], &[]);
    let inner = dir.path().join("a-dir");
    open_picker(&mut state, &mut session.borrow_mut(), "save", &inner);
    let engine = session.borrow_mut();
    let read = engine.read();
    let mut app = App::new(&mut state, &read);
    let field = app.at("keep.toml").right_center() - egui::vec2(1.0, 0.0);
    app.click(field);
    app.frame();
    let here = app.data().current_dir.clone();
    app.press(egui::Key::Backspace);
    assert_eq!(
        app.data().save_name.as_deref(),
        Some("keep.tom"),
        "Backspace 는 칸"
    );
    assert_eq!(app.data().current_dir, here, "위로 가지 않는다");
    app.type_space();
    assert_eq!(app.row_focus(), (None, false), "Space 는 칸");
    app.press(egui::Key::ArrowDown);
    assert_eq!(app.row_focus(), (some("inner.txt"), true), "↓ 는 목록");
    assert_eq!(
        app.data().save_name.as_deref(),
        Some("keep.tom "),
        "칸에 공백이 들어갔다"
    );
    app.press_with(egui::Key::ArrowUp, egui::Modifiers::ALT);
    assert_ne!(app.data().current_dir, here, "Alt+↑ 는 목록");
}

/// [Enter] 열기 모드: 고른 파일을 연다. 고른 폴더면 들어가고 행 포커스를 지운다. Enter 자체는 행 포커스를 옮기지 않는다.
#[test]
fn enter_presses_the_primary_button() {
    let (mut state, mut session) = crate::state::tests::two_pane_fixture();
    let dir = tree(&["a-dir"], &["c.txt"]);
    open_picker(&mut state, &mut session.borrow_mut(), "open", dir.path());
    let engine = session.borrow_mut();
    let read = engine.read();
    let mut app = App::new(&mut state, &read);
    app.press(egui::Key::Enter);
    assert_eq!(
        app.row_focus(),
        (None, false),
        "선택 없음: 무동작, 행 포커스 그대로"
    );
    app.press(egui::Key::ArrowDown);
    app.press(egui::Key::Enter);
    assert!(app.data().current_dir.ends_with("a-dir"), "폴더로 들어간다");
    assert_eq!(app.row_focus(), (None, false));
    load(app.state);
    app.frame();
    app.press(egui::Key::ArrowDown);
    app.press(egui::Key::Enter);
    let finished = app.finished();
    assert!(matches!(
        &finished,
        Some(FilePickerResult::Confirmed { paths }) if paths.len() == 1 && paths[0].ends_with("inner.txt")
    ));
}

/// [Enter] 저장 모드: 행 포커스와 무관하게 이름 칸으로 확정한다.
#[test]
fn enter_saves_the_typed_name_even_from_the_list() {
    let (mut state, mut session) = crate::state::tests::two_pane_fixture();
    let dir = tree(&[], &["c.txt"]);
    open_picker(&mut state, &mut session.borrow_mut(), "save", dir.path());
    let engine = session.borrow_mut();
    let read = engine.read();
    let mut app = App::new(&mut state, &read);
    app.press(egui::Key::ArrowDown);
    app.press(egui::Key::Enter);
    let finished = app.finished();
    assert!(matches!(
        &finished,
        Some(FilePickerResult::Confirmed { paths }) if paths[0].ends_with("keep.toml")
    ));
}

/// [PageUp · PageDown · scroll] 첫 PageDown 은 스크롤 없이 보이는 마지막 행, 다음 PageDown 은 한 페이지(들어가는 행 − 1)
/// 내려 새 마지막 행이다. PageUp 은 대칭이다. ↓ 는 넘친 행을 아래 끝에 맞춘다(최소 거리).
#[test]
fn page_keys_page_through_the_visible_rows() {
    let files: Vec<String> = (0..60).map(|i| format!("f{i:02}.txt")).collect();
    let refs: Vec<&str> = files.iter().map(String::as_str).collect();
    let (mut state, mut session) = crate::state::tests::two_pane_fixture();
    let dir = tree(&[], &refs);
    open_picker(&mut state, &mut session.borrow_mut(), "open", dir.path());
    let engine = session.borrow_mut();
    let read = engine.read();
    let mut app = App::new(&mut state, &read);
    let top = app.at("f00.txt").top();
    let row_h = app.at("f01.txt").top() - top;
    app.press(egui::Key::Home);
    app.press(egui::Key::PageDown);
    let k = files
        .iter()
        .position(|f| Some(f.clone()) == app.row_focus().0)
        .unwrap();
    assert!(k >= 3, "보이는 마지막 행 {k}");
    assert_eq!(
        app.at("f00.txt").top(),
        top,
        "첫 PageDown 은 스크롤하지 않는다"
    );
    app.press(egui::Key::PageDown);
    assert_eq!(
        app.row_focus().0,
        Some(files[2 * k].clone()),
        "한 페이지 = {k}행"
    );
    assert!(
        (app.at(&files[k]).top() - top).abs() < 0.5,
        "{} 이 맨 위",
        files[k]
    );
    app.press(egui::Key::PageUp);
    assert_eq!(
        app.row_focus().0,
        Some(files[k].clone()),
        "PageUp: 보이는 첫 행"
    );
    app.press(egui::Key::PageUp);
    assert_eq!(app.row_focus().0, Some(files[0].clone()));
    assert_eq!(app.at("f00.txt").top(), top);
    // ↓ 로 보이는 마지막 행을 넘으면 그 행이 아래 끝에 온다(한 행 이하만 스크롤).
    for _ in 0..=k {
        app.press(egui::Key::ArrowDown);
    }
    let moved = top - app.at("f00.txt").top();
    assert!(
        moved > 0.0 && moved <= row_h + 0.5,
        "최소 거리 스크롤 {moved}"
    );
}

/// [scroll] 휠은 view 만 옮기고 행 포커스는 그대로다. view 밖 행 포커스는 다음 행 키가 먼저 보이게 한 뒤 동작한다.
#[test]
fn the_wheel_moves_only_the_view_and_the_next_key_reveals_focus() {
    let files: Vec<String> = (0..60).map(|i| format!("f{i:02}.txt")).collect();
    let refs: Vec<&str> = files.iter().map(String::as_str).collect();
    let (mut state, mut session) = crate::state::tests::two_pane_fixture();
    let dir = tree(&[], &refs);
    open_picker(&mut state, &mut session.borrow_mut(), "open", dir.path());
    let engine = session.borrow_mut();
    let read = engine.read();
    let mut app = App::new(&mut state, &read);
    let top = app.at("f00.txt").top();
    app.press(egui::Key::ArrowDown);
    let over = app.at("f00.txt").center();
    for _ in 0..3 {
        app.texts = layer_frame(
            &app.ctx,
            app.state,
            app.read,
            vec![
                egui::Event::PointerMoved(over),
                egui::Event::MouseWheel {
                    unit: egui::MouseWheelUnit::Point,
                    delta: egui::vec2(0.0, -600.0),
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
    }
    app.frame();
    assert!(
        painted_at(&app.texts, "f00.txt").is_none_or(|r| r.top() < top),
        "휠이 내렸다"
    );
    assert_eq!(
        app.row_focus().0,
        some("f00.txt"),
        "휠은 행 포커스를 옮기지 않는다"
    );
    app.press(egui::Key::ArrowDown);
    assert_eq!(app.row_focus().0, some("f01.txt"));
    assert!(
        (app.at("f00.txt").top() - top).abs() < 0.5,
        "f00 을 위 끝에 보이고 동작했다"
    );
}

/// [shows · Esc] Tab·이름 아닌 입력(휠)은 링을 그대로 두고, Esc 는 피커를 닫는다.
#[test]
fn tab_leaves_the_ring_and_esc_closes() {
    let (mut state, mut session) = crate::state::tests::two_pane_fixture();
    let dir = tree(&[], &["c.txt"]);
    open_picker(&mut state, &mut session.borrow_mut(), "open", dir.path());
    let engine = session.borrow_mut();
    let read = engine.read();
    let mut app = App::new(&mut state, &read);
    app.press(egui::Key::ArrowDown);
    // Tab 은 피커 목록 키가 아니다. egui 로 가든 아니든 링은 그대로여야 하므로 게이트 결과는 보지 않는다.
    app.input.key(
        app.state,
        app.read,
        false,
        key(egui::Key::Tab, egui::Modifiers::NONE),
    );
    let events = app.input.take_pass(app.state, &app.ctx);
    app.texts = layer_frame(&app.ctx, app.state, app.read, events);
    assert_eq!(app.row_focus(), (some("c.txt"), true), "Tab: 그대로");
    app.texts = crate::adapters::ui::popup::window_input_test_support::layer_escape(
        &app.ctx, app.state, app.read,
    );
    assert!(instances::current(app.state).is_none(), "Esc: 닫힘");
}

/// take_keys 단위: `focused` 위젯 포커스와 직전 프레임 위젯 id 표로 사건 몫을 가른다. 피커 몫으로 뺀 조작과 egui 입력에
/// 남은 키를 돌려준다.
fn split(
    mode: FilePickerMode<'_>,
    selected: &[String],
    focused: Option<egui::Id>,
    ids: list_keys::WidgetIds,
    menu_open: bool,
    presses: &[(egui::Key, egui::Modifiers)],
) -> (Vec<list_keys::ListKey>, Vec<egui::Key>) {
    let th = crate::theme::theme();
    let entries = vec![FilePickerEntryView {
        name: "c.txt".into(),
        is_dir: false,
        size_display: String::new(),
        modified_display: String::new(),
    }];
    let mut props = super::layout_tests::bare_props(&th, &entries, selected, mode);
    props.state = FpViewState::Loaded;
    let ctx = egui::Context::default();
    // 직전 프레임에 그 위젯이 있었던 것처럼 포커스를 둔다.
    if let Some(id) = focused {
        ctx.memory_mut(|m| m.request_focus(id));
    }
    if menu_open && let Some(menu) = ids.menu {
        ctx.memory_mut(|m| m.open_popup(menu));
    }
    let mut out = (Vec::new(), Vec::new());
    drop(ctx.run(
        egui::RawInput {
            events: presses.iter().map(|(k, m)| key(*k, *m)).collect(),
            ..Default::default()
        },
        |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                let mut keys = crate::adapters::ui::popup::keys::PopupKeys::new(
                    ctx,
                    true,
                    crate::adapters::ui::popup::PopupKeyboard::Text,
                    &crate::adapters::ui::input::edit_key::EditBindings::default(),
                )
                .with_window_route(true);
                out.0 = list_keys::take_keys(ui, &props, &mut keys, ids).keys;
                out.1 = ui.input(|i| {
                    i.events
                        .iter()
                        .filter_map(|e| match e {
                            egui::Event::Key {
                                key, pressed: true, ..
                            } => Some(*key),
                            _ => None,
                        })
                        .collect()
                });
            });
        },
    ));
    out
}

const NONE: egui::Modifiers = egui::Modifiers::NONE;

fn ids() -> list_keys::WidgetIds {
    list_keys::WidgetIds {
        name_field: Some(egui::Id::new("name")),
        menu_button: Some(egui::Id::new("menu-button")),
        menu: Some(egui::Id::new("menu")),
        focus_hold: None,
    }
}

fn save_mode(can_confirm: bool) -> FilePickerMode<'static> {
    FilePickerMode::Save {
        name: "keep.toml",
        overwrite: false,
        can_confirm,
    }
}

/// [priority] 이 피커의 `…` 메뉴가 열려 있으면 메뉴가 모든 키를 갖는다: 피커는 목록 키를 쓰지 않고 누름을 입력에 남겨
/// 메뉴 행이 읽게 한다. `…` 버튼이 포커스면 그 버튼이 메뉴를 다시 토글하지 않게 Enter·Space 만 빼서 버린다. 메뉴 행이
/// 그 키로 눌리는 것은 `menu_keys_app_tests` 가 본다.
#[test]
fn an_open_menu_takes_every_key() {
    let presses = [
        (egui::Key::Enter, NONE),
        (egui::Key::Space, NONE),
        (egui::Key::ArrowDown, NONE),
        (egui::Key::Backspace, NONE),
    ];
    let all: Vec<egui::Key> = presses.iter().map(|(k, _)| *k).collect();
    for focused in [
        ids().menu_button,
        ids().name_field,
        Some(egui::Id::new("menu-row")),
        None,
    ] {
        let (owned, left) = split(save_mode(true), &[], focused, ids(), true, &presses);
        assert!(owned.is_empty(), "{focused:?}");
        let want = if focused == ids().menu_button {
            vec![egui::Key::ArrowDown, egui::Key::Backspace]
        } else {
            all.clone()
        };
        assert_eq!(left, want, "{focused:?}: 메뉴가 읽을 키");
    }
}

/// [priority · Enter] `…` 버튼이 Tab 포커스이고 메뉴가 닫혀 있으면 Enter 는 확정하지 않고 버튼(메뉴 열기)에 남으며,
/// 그 뒤 같은 패스의 키도 피커가 쓰지 않는다.
#[test]
fn enter_on_the_menu_button_opens_the_menu_without_confirming() {
    let (owned, left) = split(
        save_mode(true),
        &[],
        ids().menu_button,
        ids(),
        false,
        &[(egui::Key::Enter, NONE), (egui::Key::ArrowDown, NONE)],
    );
    assert!(owned.is_empty());
    assert_eq!(left, vec![egui::Key::Enter, egui::Key::ArrowDown]);
}

/// [priority · Enter] 확정이 활성이면 다른 버튼이 Tab 포커스여도 Enter 는 확정이다(버튼에 남지 않는다). 비활성이면 Enter
/// 는 Tab 포커스 버튼에 남는다. 이름 칸 포커스면 비활성이어도 피커가 가져간다(칸이 포커스를 놓지 않는다).
#[test]
fn enter_confirms_over_a_focused_button_when_the_primary_is_enabled() {
    let cancel = Some(egui::Id::new("cancel"));
    let (owned, left) = split(
        save_mode(true),
        &[],
        cancel,
        ids(),
        false,
        &[(egui::Key::Enter, NONE)],
    );
    assert_eq!(owned, vec![list_keys::ListKey::Confirm]);
    assert!(left.is_empty());
    let (owned, left) = split(
        save_mode(false),
        &[],
        cancel,
        ids(),
        false,
        &[(egui::Key::Enter, NONE)],
    );
    assert!(owned.is_empty());
    assert_eq!(
        left,
        vec![egui::Key::Enter],
        "비활성: Tab 포커스 버튼이 누른다"
    );
    let (owned, _) = split(
        save_mode(false),
        &[],
        ids().name_field,
        ids(),
        false,
        &[(egui::Key::Enter, NONE)],
    );
    assert_eq!(owned, vec![list_keys::ListKey::Confirm], "이름 칸: 피커 몫");
    // 열기 모드 미선택: 앞선 ↓ 가 고를 수 있으므로 같은 패스의 Enter 도 피커 몫이다.
    let open = FilePickerMode::Open { selection_text: "" };
    let (owned, _) = split(
        open,
        &[],
        None,
        ids(),
        false,
        &[(egui::Key::ArrowDown, NONE), (egui::Key::Enter, NONE)],
    );
    assert_eq!(
        owned,
        vec![
            list_keys::ListKey::Move(list_keys::MoveKey::Down),
            list_keys::ListKey::Confirm
        ]
    );
}

/// [priority] Tab 포커스 버튼은 Space 를 갖는다. 저장 이름 칸 텍스트 포커스는 Space·Backspace·Home·End·→ 를 갖고
/// ↑ ↓·PageUp·PageDown·Alt+↑ 는 목록으로 보낸다.
#[test]
fn focused_widgets_keep_their_keys() {
    let cancel = Some(egui::Id::new("cancel"));
    let (owned, left) = split(
        save_mode(true),
        &[],
        cancel,
        ids(),
        false,
        &[(egui::Key::Space, NONE), (egui::Key::ArrowDown, NONE)],
    );
    assert_eq!(
        owned,
        vec![list_keys::ListKey::Move(list_keys::MoveKey::Down)]
    );
    assert_eq!(left, vec![egui::Key::Space]);
    let field_keys = [
        egui::Key::Space,
        egui::Key::Backspace,
        egui::Key::Home,
        egui::Key::End,
        egui::Key::ArrowRight,
    ];
    let list_keys_ = [
        (egui::Key::ArrowDown, NONE),
        (egui::Key::ArrowUp, NONE),
        (egui::Key::PageUp, NONE),
        (egui::Key::PageDown, NONE),
        (egui::Key::ArrowUp, egui::Modifiers::ALT),
    ];
    let presses: Vec<_> = field_keys
        .iter()
        .map(|k| (*k, NONE))
        .chain(list_keys_.iter().copied())
        .collect();
    let (owned, left) = split(
        save_mode(true),
        &[],
        ids().name_field,
        ids(),
        false,
        &presses,
    );
    assert_eq!(left, field_keys.to_vec(), "칸 몫");
    assert_eq!(owned.len(), list_keys_.len(), "목록 몫 {owned:?}");
}
