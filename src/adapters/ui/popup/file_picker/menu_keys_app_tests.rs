//! 목록 키 spec(`row-focus-and-list-keys`, page fe45514) 'priority' 첫 두 줄 — path bar `…` 메뉴와 키 — 을 창 키
//! 경로와 팝업 층 프레임으로 본다. 하네스는 [`super::list_keys_app_tests`] 의 것이다.
use super::list_keys_app_tests::{App, load, open_picker};
use crate::adapters::ui::popup::window_input_test_support::click_widget_at;

/// 경로가 피커 폭에 들어가지 않아 `…` 로 접히는 깊은 폴더. 맨 안쪽 폴더 경로를 돌려준다.
fn deep_tree(root: &std::path::Path) -> std::path::PathBuf {
    let mut dir = root.to_path_buf();
    for i in 0..12 {
        dir = dir.join(format!("a-long-folder-segment-{i:02}"));
    }
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("c.txt"), b"").unwrap();
    dir
}

/// 칠한 글자 중 `before` 에 없던 접힌 폴더 이름(메뉴 행) 하나와 그 영역.
fn new_segment(
    before: &[(String, egui::Rect)],
    after: &[(String, egui::Rect)],
) -> Option<(String, egui::Rect)> {
    after
        .iter()
        .find(|(t, _)| {
            t.starts_with("a-long-folder-segment-") && !before.iter().any(|(b, _)| b == t)
        })
        .cloned()
}

/// `…` 를 눌러 메뉴를 열고, 메뉴에 새로 칠해진 숨은 폴더 행 하나를 돌려준다.
fn open_menu(app: &mut App<'_>) -> (String, egui::Rect) {
    let before = app.texts.clone();
    let dots = app.at("…");
    app.click(dots.center());
    new_segment(&before, &app.texts).expect("메뉴가 숨은 폴더 행을 칠한다")
}

/// [priority] 열린 `…` 메뉴가 키를 갖는다: 세 모드에서 Tab 으로 포커스한 메뉴 행을 Enter·Space 가 눌러 그 조상 폴더로
/// 간다(목록이 키를 지우지 않는다).
#[test]
fn enter_and_space_press_the_focused_menu_row() {
    for mode in ["open", "save", "folder"] {
        for k in [egui::Key::Enter, egui::Key::Space] {
            let (mut state, mut session) = crate::state::tests::two_pane_fixture();
            let root = tempfile::tempdir().unwrap();
            let deep = deep_tree(root.path());
            open_picker(&mut state, &mut session.borrow_mut(), mode, &deep);
            let engine = session.borrow_mut();
            let read = engine.read();
            let mut app = App::new(&mut state, &read);
            let (label, row) = open_menu(&mut app);
            let id = click_widget_at(&app.ctx, row.center()).expect("메뉴 행 위젯");
            app.ctx.memory_mut(|m| m.request_focus(id));
            app.frame();
            assert_eq!(
                app.ctx.memory(|m| m.focused()),
                Some(id),
                "{mode}: 메뉴 행 포커스"
            );
            app.press(k);
            let dir = std::path::Path::new(&app.data().current_dir).to_path_buf();
            assert_eq!(
                dir.file_name().and_then(|n| n.to_str()),
                Some(label.as_str()),
                "{mode}: {k:?} 가 메뉴 행 {label} 을 눌렀다"
            );
            assert!(deep.starts_with(&dir), "{mode}: 조상 폴더");
            load(app.state);
        }
    }
}

fn key(k: egui::Key) -> egui::Event {
    egui::Event::Key {
        key: k,
        physical_key: Some(k),
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers::NONE,
    }
}

/// [priority · Enter] 접힌 경로에서 루트 링크가 포커스이고 열기 버튼이 켜진 채 Tab 과 Enter 가 한 패스에 오면, Tab 이
/// `…` 버튼으로 옮긴 포커스에서 Enter 는 확정하지 않고 메뉴를 연다.
#[test]
fn tab_then_enter_in_one_pass_opens_the_menu_without_confirming() {
    let (mut state, mut session) = crate::state::tests::two_pane_fixture();
    let root = tempfile::tempdir().unwrap();
    let deep = deep_tree(root.path());
    open_picker(&mut state, &mut session.borrow_mut(), "open", &deep);
    let engine = session.borrow_mut();
    let read = engine.read();
    let mut app = App::new(&mut state, &read);
    app.press(egui::Key::ArrowDown);
    assert_eq!(
        app.data().selected,
        vec!["c.txt".to_string()],
        "열기 버튼이 켜진다"
    );
    let link = click_widget_at(&app.ctx, app.at(&root_label(&deep)).center()).expect("루트 링크");
    app.ctx.memory_mut(|m| m.request_focus(link));
    app.frame();
    assert_eq!(
        app.ctx.memory(|m| m.focused()),
        Some(link),
        "루트 링크 포커스"
    );
    let before = app.texts.clone();
    for k in [egui::Key::Tab, egui::Key::Enter] {
        app.input.key(app.state, app.read, false, key(k));
    }
    let events = app.input.take_pass(app.state, &app.ctx);
    app.texts = crate::adapters::ui::popup::window_input_test_support::layer_frame(
        &app.ctx, app.state, app.read, events,
    );
    // egui 는 새로 연 메뉴 영역을 첫 프레임에 보이지 않게 잰다. 그다음 프레임에 행을 칠한다.
    app.frame();
    app.frame();
    assert!(app.finished().is_none(), "확정하지 않는다");
    assert!(
        super::instances::current(app.state).is_some(),
        "피커는 열려 있다"
    );
    assert!(
        new_segment(&before, &app.texts).is_some(),
        "메뉴가 열려 숨은 폴더 행을 칠한다"
    );
}

/// 로컬 `dir` 경로 막대의 루트 크럼 글자. paths.rs 가 만드는 표기(Unix `/`, Windows 드라이브·UNC 루트)다.
fn root_label(dir: &std::path::Path) -> String {
    let dir = dir.to_string_lossy();
    let ancestors = super::paths::path_ancestors(false, &dir);
    super::paths::crumb_label(false, &ancestors[0])
}
