//! 목록 이동 키 계산. 20행, 행 높이 20, view 높이 100(5행이 들어가고 한 페이지는 4행).
use super::*;

const ROW_H: f32 = 20.0;
const ROWS: usize = 20;

fn view(offset: f32) -> ListView {
    ListView {
        offset,
        height: 100.0,
        row_h: ROW_H,
    }
}

fn all() -> Vec<bool> {
    vec![true; ROWS]
}

fn go(sel: &[bool], focus: Option<usize>, offset: f32, key: MoveKey) -> (usize, f32) {
    let m = step(sel, focus, view(offset), key).expect("고를 수 있는 행이 있다");
    (m.focus, m.offset)
}

/// start: 행 포커스가 없으면 ↓·PageDown·Home 은 첫 행, ↑·PageUp·End 는 끝 행이다. inert 행은 건너뛴다.
#[test]
fn without_row_focus_the_keys_take_the_first_or_last_row() {
    for key in [MoveKey::Down, MoveKey::PageDown, MoveKey::Home] {
        assert_eq!(go(&all(), None, 0.0, key), (0, 0.0), "{key:?}");
    }
    for key in [MoveKey::Up, MoveKey::PageUp, MoveKey::End] {
        assert_eq!(go(&all(), None, 0.0, key), (19, 300.0), "{key:?}");
    }
    let mut sel = all();
    sel[0] = false;
    sel[1] = false;
    sel[19] = false;
    assert_eq!(go(&sel, None, 0.0, MoveKey::Down).0, 2);
    assert_eq!(go(&sel, None, 0.0, MoveKey::End).0, 18);
}

/// ↑ ↓ 는 한 행, Home·End 는 첫·끝 행이고 끝에서 멈춘다(순환 없음).
#[test]
fn arrows_move_one_row_and_stop_at_the_ends() {
    assert_eq!(go(&all(), Some(3), 0.0, MoveKey::Down), (4, 0.0));
    assert_eq!(go(&all(), Some(3), 0.0, MoveKey::Up), (2, 0.0));
    assert_eq!(go(&all(), Some(0), 0.0, MoveKey::Up), (0, 0.0));
    assert_eq!(go(&all(), Some(19), 300.0, MoveKey::Down), (19, 300.0));
    assert_eq!(go(&all(), Some(7), 60.0, MoveKey::Home), (0, 0.0));
    assert_eq!(go(&all(), Some(7), 60.0, MoveKey::End), (19, 300.0));
}

/// 모든 키가 inert 행을 건너뛰고, 고를 수 있는 행이 없으면 아무 키도 동작하지 않는다.
#[test]
fn every_key_skips_inert_rows() {
    let sel = [true, false, false, true, false];
    let short = |focus, key| step(&sel, focus, view(0.0), key).map(|m| m.focus);
    assert_eq!(short(Some(0), MoveKey::Down), Some(3));
    assert_eq!(short(Some(3), MoveKey::Down), Some(3));
    assert_eq!(short(Some(3), MoveKey::Up), Some(0));
    assert_eq!(short(Some(3), MoveKey::Home), Some(0));
    assert_eq!(short(Some(0), MoveKey::End), Some(3));
    assert_eq!(short(Some(0), MoveKey::PageDown), Some(3));
    assert_eq!(short(Some(3), MoveKey::PageUp), Some(0));
    for key in [
        MoveKey::Down,
        MoveKey::Up,
        MoveKey::Home,
        MoveKey::End,
        MoveKey::PageDown,
    ] {
        assert_eq!(step(&[false, false], None, view(0.0), key), None, "{key:?}");
        assert_eq!(
            step(&[false, false], Some(0), view(0.0), key),
            None,
            "{key:?}"
        );
    }
}

/// PageDown: 먼저 view 를 두고 보이는 마지막 행, 그 행에서는 한 페이지(들어가는 행 − 1) 내려 새 마지막 행.
/// 목록 끝에서는 멈춘다.
#[test]
fn page_down_takes_the_last_visible_row_then_pages() {
    assert_eq!(go(&all(), Some(0), 0.0, MoveKey::PageDown), (4, 0.0));
    assert_eq!(go(&all(), Some(4), 0.0, MoveKey::PageDown), (8, 80.0));
    assert_eq!(go(&all(), Some(8), 80.0, MoveKey::PageDown), (12, 160.0));
    assert_eq!(go(&all(), Some(17), 300.0, MoveKey::PageDown), (19, 300.0));
    assert_eq!(go(&all(), Some(19), 300.0, MoveKey::PageDown), (19, 300.0));
}

/// PageUp 은 PageDown 의 대칭이다.
#[test]
fn page_up_takes_the_first_visible_row_then_pages() {
    assert_eq!(go(&all(), Some(19), 300.0, MoveKey::PageUp), (15, 300.0));
    assert_eq!(go(&all(), Some(15), 300.0, MoveKey::PageUp), (11, 220.0));
    assert_eq!(go(&all(), Some(2), 0.0, MoveKey::PageUp), (0, 0.0));
    assert_eq!(go(&all(), Some(0), 0.0, MoveKey::PageUp), (0, 0.0));
}

/// 이동은 행 전체가 보이도록 가장 적게 스크롤한다. 반쯤 보이는 행은 보이는 행이 아니다.
#[test]
fn moves_scroll_the_least_distance() {
    assert_eq!(go(&all(), Some(4), 0.0, MoveKey::Down), (5, 20.0));
    assert_eq!(go(&all(), Some(5), 20.0, MoveKey::Up), (4, 20.0));
    assert_eq!(go(&all(), Some(1), 20.0, MoveKey::Up), (0, 0.0));
    // 위치 10: 0행 위쪽과 5행 아래쪽이 잘려 온전히 보이는 행은 1..=4 다.
    assert_eq!(go(&all(), Some(1), 10.0, MoveKey::PageDown), (4, 10.0));
    assert_eq!(reveal(view(10.0), ROWS, 0), 0.0);
    assert_eq!(reveal(view(10.0), ROWS, 5), 20.0);
    assert_eq!(reveal(view(10.0), ROWS, 3), 10.0);
}

/// 행 포커스가 view 밖이면 다음 행 키가 먼저 최소 거리로 보이게 한 뒤 그 행에서 동작한다.
/// PageDown 은 아래에 있던 행을 아래 끝에, 위에 있던 행을 위 끝에 놓고 자기 규칙을 쓴다.
#[test]
fn an_out_of_view_focus_is_revealed_before_the_key_acts() {
    assert_eq!(go(&all(), Some(10), 0.0, MoveKey::Down), (11, 140.0));
    assert_eq!(go(&all(), Some(10), 0.0, MoveKey::PageDown), (14, 200.0));
    assert_eq!(go(&all(), Some(2), 200.0, MoveKey::PageDown), (6, 40.0));
    assert_eq!(go(&all(), Some(2), 200.0, MoveKey::Up), (1, 20.0));
}

/// 누름 하나를 키 통로가 피커 몫으로 지우는지 잰다(몫 판정은 모두 피커 몫).
fn body_takes(modifiers: egui::Modifiers, key: egui::Key) -> bool {
    let ctx = egui::Context::default();
    let raw = egui::RawInput {
        modifiers,
        events: vec![egui::Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers,
        }],
        ..Default::default()
    };
    ctx.begin_pass(raw);
    let mut keys = crate::adapters::ui::popup::keys::PopupKeys::new(
        &ctx,
        true,
        crate::adapters::ui::popup::PopupKeyboard::Text,
        &crate::adapters::ui::input::edit_key::EditBindings::default(),
    );
    let taken = keys.take_keymap_presses(classify, |_| true);
    let _output = ctx.end_pass();
    !taken.is_empty()
}

/// where keys go: 창 키 경로의 수신 판정(`list_keymap_receives`)은 본문이 지우는 누름과 정확히 같다. 세 모드가 같은
/// 표이고, 패턴에 없는 Shift·Alt 는 붙어도 맞고 Ctrl 은 맞지 않는다. Alt+↑ 는 행 이동이 아니라 상위 폴더다.
#[test]
fn the_list_keymap_receives_what_the_body_takes() {
    let mods = [
        egui::Modifiers::NONE,
        egui::Modifiers::SHIFT,
        egui::Modifiers::ALT,
        egui::Modifiers::CTRL,
        egui::Modifiers::ALT | egui::Modifiers::SHIFT,
        egui::Modifiers::COMMAND,
    ];
    let mut hits = 0;
    for key in egui::Key::ALL {
        for m in mods {
            let body = body_takes(m, *key);
            assert_eq!(list_keymap_receives(m, *key), body, "{m:?}+{key:?}");
            hits += usize::from(body);
        }
    }
    assert!(hits > 0);
    for key in [
        egui::Key::ArrowDown,
        egui::Key::ArrowUp,
        egui::Key::ArrowRight,
        egui::Key::Home,
        egui::Key::End,
        egui::Key::PageUp,
        egui::Key::PageDown,
        egui::Key::Space,
        egui::Key::Enter,
        egui::Key::Backspace,
    ] {
        assert!(list_keymap_receives(egui::Modifiers::NONE, key), "{key:?}");
    }
    assert!(!list_keymap_receives(
        egui::Modifiers::CTRL,
        egui::Key::ArrowUp
    ));
    assert!(!list_keymap_receives(
        egui::Modifiers::NONE,
        egui::Key::ArrowLeft
    ));
    assert_eq!(
        classify(egui::Modifiers::ALT, egui::Key::ArrowUp),
        Some(ListKey::Parent)
    );
    assert_eq!(
        classify(egui::Modifiers::NONE, egui::Key::ArrowUp),
        Some(ListKey::Move(MoveKey::Up))
    );
}
