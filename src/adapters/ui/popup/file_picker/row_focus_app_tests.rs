//! 목록 키 spec(`row-focus-and-list-keys`, page fe45514) 중 탐색 뒤 지움 · 세션 소실 · 키 포커스 없는 링을 창 키 경로와
//! 팝업 층 한 프레임의 도형으로 본다. 하네스는 [`super::list_keys_app_tests`] 의 것이다.
use super::list_keys_app_tests::{App, load, open_picker, some, tree};
use super::*;
use crate::adapters::ui::popup::window_input_test_support::layer_pass;

/// 팝업 층 한 프레임을 그려 행 포커스 링(행 높이 · `fp-row-focus-ring-width` · `radius-sm` 선)의 사각형을 모은다.
fn rings(app: &mut App<'_>) -> Vec<egui::Rect> {
    let th = crate::theme::theme();
    let out = layer_pass(&app.ctx, app.state, app.read, Vec::new());
    let row_h = row_height(&th).value();
    out.shapes
        .iter()
        .filter_map(|c| match &c.shape {
            egui::Shape::Rect(r)
                if r.stroke.width == th.fp_row_focus_ring_width().value()
                    && f32::from(r.corner_radius.nw) == th.corner_radius_sm.value()
                    && (r.rect.height() - row_h).abs() < 0.5 =>
            {
                Some(r.rect)
            }
            _ => None,
        })
        .collect()
}

/// 글리프 이미지(텍스처 사각형)의 사각형.
fn glyph_rects(app: &mut App<'_>) -> Vec<egui::Rect> {
    let out = layer_pass(&app.ctx, app.state, app.read, Vec::new());
    out.shapes
        .iter()
        .filter_map(|c| match &c.shape {
            egui::Shape::Rect(r) if r.brush.is_some() => Some(r.rect),
            _ => None,
        })
        .collect()
}

fn same_dir(a: &str, b: &std::path::Path) -> bool {
    std::path::Path::new(a).canonicalize().ok() == b.canonicalize().ok()
}

/// [Alt+↑ · Backspace] 세션 소실(출처 mirror 워크스페이스 없음)이면 두 키 모두 아무것도 하지 않는다. 세 모드가 같고,
/// Up 버튼이 읽는 판정(`up_enabled`)을 같이 읽는다.
#[test]
fn a_lost_session_leaves_alt_up_and_backspace_idle() {
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
            instances::current_mut(app.state)
                .expect("picker open")
                .mirror_ws_id = Some(u32::MAX);
            app.frame();
            app.press_with(k, m);
            assert!(
                same_dir(&app.data().current_dir, &inner),
                "{mode}: {m:?}+{k:?} 는 세션 소실 때 그대로"
            );
        }
    }
}

/// [after navigation · shows] crumb 와 Refresh 는 선택 · 행 포커스 · 링을 지우고, 읽기가 끝난 뒤에도 다음 행 키 전까지
/// 링이 없다. 다음 ↓ 는 첫 행부터 잡는다.
#[test]
fn a_crumb_refresh_and_finished_load_leave_no_selection_or_ring() {
    let (mut state, mut session) = crate::state::tests::two_pane_fixture();
    let dir = tree(&["a-dir"], &["b.txt"]);
    let inner = dir.path().join("a-dir");
    open_picker(&mut state, &mut session.borrow_mut(), "open", &inner);
    let engine = session.borrow_mut();
    let read = engine.read();
    let mut app = App::new(&mut state, &read);
    // 이미지 글리프(Refresh)를 찾으려면 로더가 있어야 한다.
    egui_extras::install_image_loaders(&app.ctx);
    app.frame();
    app.frame();

    app.press(egui::Key::ArrowDown);
    assert_eq!(app.row_focus(), (some("inner.txt"), true), "↓");
    assert_eq!(app.rings().len(), 1, "↓ 뒤 링");
    // Refresh 는 경로 막대 맨 오른쪽 글리프다.
    let crumb = app.at("a-dir");
    let refresh = glyph_rects(&mut app)
        .into_iter()
        .filter(|r| (r.center().y - crumb.center().y).abs() < 2.0)
        .max_by(|a, b| a.center().x.total_cmp(&b.center().x))
        .expect("Refresh 글리프");
    app.frame();
    app.click(refresh.center());
    assert!(app.selected().is_empty(), "Refresh: 선택 지움");
    assert_eq!(app.row_focus(), (None, false), "Refresh: 행 포커스 지움");
    load(app.state);
    app.frame();
    assert!(app.rings().is_empty(), "읽기가 끝나도 링 없음");
    assert!(app.selected().is_empty(), "읽기가 끝나도 선택 없음");

    app.press(egui::Key::ArrowDown);
    assert_eq!(
        app.row_focus(),
        (some("inner.txt"), true),
        "다음 ↓ 는 첫 행"
    );
    let parent = dir
        .path()
        .file_name()
        .and_then(|n| n.to_str())
        .expect("임시 폴더 이름")
        .to_string();
    let at = app.at(&parent);
    app.frame();
    app.click(at.center());
    assert!(
        same_dir(&app.data().current_dir, dir.path()),
        "crumb 로 상위"
    );
    assert!(app.selected().is_empty(), "crumb: 선택 지움");
    assert_eq!(app.row_focus(), (None, false), "crumb: 행 포커스 지움");
    load(app.state);
    app.frame();
    assert!(app.rings().is_empty(), "crumb 읽기 뒤 링 없음");
}

/// [shows] 피커가 키 포커스를 잃으면 링을 그리지 않고 행 포커스는 남는다.
#[test]
fn the_ring_hides_while_the_picker_lacks_key_focus() {
    let (mut state, mut session) = crate::state::tests::two_pane_fixture();
    let dir = tree(&[], &["c.txt", "d.txt"]);
    open_picker(&mut state, &mut session.borrow_mut(), "open", dir.path());
    let engine = session.borrow_mut();
    let read = engine.read();
    let mut app = App::new(&mut state, &read);
    app.press(egui::Key::ArrowDown);
    let rings = app.rings();
    assert_eq!(rings.len(), 1, "키 포커스가 있으면 링");
    assert!(rings[0].contains(app.at("c.txt").center()), "c.txt 의 링");
    // 다른 칸의 surface 로 키 포커스를 옮긴다. 피커는 출처 surface 의 팝업이라 열린 채 남는다.
    assert!(app.state.focus_surface_by_id(app.read, 101));
    app.frame();
    assert!(instances::current(app.state).is_some(), "피커는 열려 있다");
    assert!(app.rings().is_empty(), "키 포커스가 없으면 링 없음");
    assert_eq!(app.row_focus(), (some("c.txt"), true), "행 포커스는 남는다");
}

/// [Tab order · Enter · Space] Tab 은 목록 행에 위젯 포커스를 주지 않는다. 행이 Tab 포커스를 받으면 egui 가 목록 키
/// 몫 밖에 남은 Enter(확정이 꺼졌을 때)·Space(위젯 포커스가 있을 때)를 그 행 클릭으로 바꿔, 이름이 빈 저장 모드에서
/// Enter 가 파일 이름을 채운다.
#[test]
fn tab_never_gives_a_list_row_widget_focus() {
    let (mut state, mut session) = crate::state::tests::two_pane_fixture();
    let dir = tree(&[], &["c.txt", "d.txt"]);
    open_picker(&mut state, &mut session.borrow_mut(), "save", dir.path());
    let engine = session.borrow_mut();
    let read = engine.read();
    let mut app = App::new(&mut state, &read);
    assert!(matches!(
        instances::apply_to_current(
            app.state,
            app.read,
            FilePickerAction::EditName(String::new())
        ),
        PopupAction::None
    ));
    app.frame();
    let rows: Vec<egui::Id> = ["c.txt", "d.txt"]
        .into_iter()
        .map(|n| {
            crate::adapters::ui::popup::window_input_test_support::click_widget_at(
                &app.ctx,
                app.at(n).center(),
            )
            .expect("행 위젯")
        })
        .collect();
    let mut seen = Vec::new();
    for _ in 0..16 {
        app.input.key(
            app.state,
            app.read,
            false,
            egui::Event::Key {
                key: egui::Key::Tab,
                physical_key: Some(egui::Key::Tab),
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            },
        );
        let events = app.input.take_pass(app.state, &app.ctx);
        app.texts = crate::adapters::ui::popup::window_input_test_support::layer_frame(
            &app.ctx, app.state, app.read, events,
        );
        if let Some(id) = app.ctx.memory(|m| m.focused())
            && !seen.contains(&id)
        {
            seen.push(id);
        }
    }
    assert!(seen.len() >= 3, "Tab 이 위젯 사이를 돈다: {seen:?}");
    assert!(
        !seen.iter().any(|id| rows.contains(id)),
        "Tab 이 행에 섰다: {seen:?} / 행 {rows:?}"
    );
    assert_eq!(
        app.data().save_name.as_deref(),
        Some(""),
        "이름은 비어 있다"
    );
}

/// 목록 세로 스크롤바의 입력 사각형(끝난 패스의 드래그 감지 위젯 중 키가 크고 폭이 좁은 것).
fn scrollbar(app: &App<'_>) -> egui::Rect {
    app.ctx.viewport(|vp| {
        vp.prev_pass
            .widgets
            .layers()
            .flat_map(|(_, rects)| rects.iter())
            .filter(|w| {
                w.sense.senses_drag()
                    && w.interact_rect.height() > 100.0
                    && w.interact_rect.width() < 30.0
            })
            .map(|w| w.interact_rect)
            .max_by(|a, b| a.height().total_cmp(&b.height()))
            .expect("목록 스크롤바")
    })
}

/// [row focus · shows · scroll] 스크롤바 클릭과 드래그는 view 만 옮긴다: 열기·폴더 모드에서 선택·행 포커스·링이 그대로다
/// (폴더 모드는 목록 빈 곳 클릭이 선택을 지우므로 스크롤바가 그 판정에 들지 않아야 한다).
#[test]
fn the_scrollbar_moves_only_the_view() {
    for mode in ["open", "folder"] {
        let names: Vec<String> = (0..60).map(|i| format!("d{i:02}")).collect();
        let refs: Vec<&str> = names.iter().map(String::as_str).collect();
        let (mut state, mut session) = crate::state::tests::two_pane_fixture();
        let dir = tree(&refs, &[]);
        open_picker(&mut state, &mut session.borrow_mut(), mode, dir.path());
        let engine = session.borrow_mut();
        let read = engine.read();
        let mut app = App::new(&mut state, &read);
        let top = app.at("d00").top();
        app.press(egui::Key::ArrowDown);
        assert_eq!(app.row_focus(), (some("d00"), true), "{mode}: ↓");
        let bar = scrollbar(&app);
        // 트랙 아래쪽 클릭.
        let low = egui::pos2(bar.center().x, bar.bottom() - 4.0);
        app.click(low);
        assert!(
            painted_texts_top(&app, "d00").is_none_or(|t| t < top),
            "{mode}: 트랙 클릭이 view 를 내렸다"
        );
        assert_eq!(app.row_focus(), (some("d00"), true), "{mode}: 트랙 클릭 뒤");
        assert_eq!(
            app.selected(),
            vec!["d00".to_string()],
            "{mode}: 트랙 클릭 뒤 선택"
        );
        // 손잡이를 위 끝으로 끌어 올린다.
        let bar = scrollbar(&app);
        let from = egui::pos2(bar.center().x, bar.bottom() - 4.0);
        let to = egui::pos2(bar.center().x, bar.top() - 200.0);
        let button = |pos, pressed| egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        for events in [
            vec![egui::Event::PointerMoved(from)],
            vec![button(from, true)],
            vec![egui::Event::PointerMoved(from + egui::vec2(0.0, -20.0))],
            vec![egui::Event::PointerMoved(to)],
            vec![button(to, false)],
            Vec::new(),
        ] {
            app.texts = crate::adapters::ui::popup::window_input_test_support::layer_frame(
                &app.ctx, app.state, app.read, events,
            );
        }
        assert!(
            (app.at("d00").top() - top).abs() < 0.5,
            "{mode}: 드래그가 view 를 맨 위로 올렸다"
        );
        assert_eq!(app.row_focus(), (some("d00"), true), "{mode}: 드래그 뒤");
        assert_eq!(
            app.selected(),
            vec!["d00".to_string()],
            "{mode}: 드래그 뒤 선택"
        );
    }
}

fn painted_texts_top(app: &App<'_>, text: &str) -> Option<f32> {
    crate::adapters::ui::popup::window_input_test_support::painted_at(&app.texts, text)
        .map(|r| r.top())
}

/// [row focus · shows] 저장 모드 목록 빈 곳 클릭은 선택·행 포커스·이름을 두고 링만 숨긴다.
#[test]
fn a_save_mode_empty_list_click_keeps_selection_focus_and_name() {
    let (mut state, mut session) = crate::state::tests::two_pane_fixture();
    let dir = tree(&[], &["c.txt", "d.txt"]);
    open_picker(&mut state, &mut session.borrow_mut(), "save", dir.path());
    let engine = session.borrow_mut();
    let read = engine.read();
    let mut app = App::new(&mut state, &read);
    app.press(egui::Key::ArrowDown);
    app.press(egui::Key::ArrowDown);
    app.press(egui::Key::Space);
    assert_eq!(app.row_focus(), (some("d.txt"), true));
    assert_eq!(app.selected(), vec!["d.txt".to_string()]);
    assert_eq!(app.data().save_name.as_deref(), Some("d.txt"));
    let row_h = app.at("d.txt").top() - app.at("c.txt").top();
    let below = app.at("d.txt").center() + egui::vec2(0.0, row_h * 1.5);
    app.click(below);
    assert_eq!(app.row_focus(), (some("d.txt"), false), "링만 숨긴다");
    assert_eq!(app.selected(), vec!["d.txt".to_string()], "선택 그대로");
    assert_eq!(
        app.data().save_name.as_deref(),
        Some("d.txt"),
        "이름 그대로"
    );
}

impl App<'_> {
    fn rings(&mut self) -> Vec<egui::Rect> {
        rings(self)
    }
}
