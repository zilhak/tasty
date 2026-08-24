//! 전체화면 무대 **입력 게이트의 배선**을 소스 구조로 고정하는 가드.
//!
//! 게이트의 판정 자체는 순수 함수로 떼어 단위 테스트한다
//! (`src/view/main/keyboard.rs` 의 `stage_key_decision`). 하지만 "그 판정이 파이프라인의
//! **어디에** 꽂혀 있는가" 는 `MainView` 를 GPU/winit 없이 구성할 수 없어 런타임으로
//! 단정할 수 없다 — 그런데 이 트랙의 계약은 대부분 위치 계약이다:
//!
//! 1. 키보드 0단계 게이트가 double-tap(1~3단계)·ESC(4단계)보다 **앞**에 있다.
//!    뒤로 밀리면 무대 중 ESC 가 settings/notifications 를 함께 닫아 "뒤로 전파되지
//!    않는다" 는 사용자 확정 계약이 깨진다.
//! 2. 마우스 세 핸들러 + OS 리사이즈 양보가 **같은 판정**(`mouse_overlay_open`)을 본다.
//!    한 지점만 빠져도 그 경로로 입력이 샌다(modifier-hint 오버레이가 겪은 4 지점 문제).
//! 3. OS 레벨 UI(네이티브 메뉴 · 파일 드래그)는 입력 게이트 **밖**이라 별도 배선이
//!    있어야 한다.
//!
//! 선례: `tests/fullscreen_stage_render_gate.rs` / `tests/design_token_adherence.rs`.

use std::path::PathBuf;

fn read(rel: &str) -> String {
    let p: PathBuf = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(rel);
    std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("read {}: {e}", p.display()))
}

/// `needle` 이 정확히 한 번 나오는 바이트 오프셋.
fn only_at(hay: &str, needle: &str, what: &str) -> usize {
    let n = hay.matches(needle).count();
    assert_eq!(
        n, 1,
        "{what}: `{needle}` 이 {n} 번 나온다 — 이 가드는 유일 출현을 전제한다. \
         구조가 바뀌었으면 가드도 함께 갱신하라."
    );
    hay.find(needle).expect("checked above")
}

/// 무대 게이트는 파이프라인 맨 앞이다 — double-tap 도 ESC 도 무대 중에는 못 본다.
#[test]
fn keyboard_stage_gate_precedes_double_tap_and_escape() {
    let src = read("src/view/main/keyboard.rs");
    let gate = only_at(
        &src,
        "if self.try_consume_fullscreen_stage_key(event) {",
        "0단계 무대 게이트 호출",
    );
    let double_tap = only_at(
        &src,
        "if self.try_consume_double_tap_key() {",
        "1~3단계 double-tap",
    );
    let escape = only_at(&src, "if self.try_consume_escape_key(event) {", "4단계 ESC");
    assert!(
        gate < double_tap,
        "무대 게이트가 double-tap 뒤로 밀렸다 — 무대 중 Shift+Shift 류 단축키가 발화한다."
    );
    assert!(
        gate < escape,
        "무대 게이트가 4단계 ESC 뒤로 밀렸다 — 무대 중 ESC 가 settings/notifications 까지 \
         닫아 '뒤로 전파되지 않는다' 는 확정 계약이 깨진다."
    );
}

/// 게이트는 소비 시 **즉시 return** 이어야 한다. `return` 이 빠지면 무대 중 키가 그대로
/// 아래 단계로 흘러간다.
#[test]
fn keyboard_stage_gate_returns_immediately() {
    let src = read("src/view/main/keyboard.rs");
    let call = "if self.try_consume_fullscreen_stage_key(event) {\n            return;\n        }";
    assert!(
        src.contains(call),
        "0단계 게이트가 소비 즉시 return 하지 않는다 — 무대 중 키가 뒤 단계로 샌다."
    );
}

/// 종료 키 판정은 한 곳에만 있다 — `KeybindingSettings` 연동 시 그 한 곳만 바뀐다.
#[test]
fn stage_exit_key_has_a_single_decision_site() {
    let src = read("src/view/main/keyboard.rs");
    only_at(&src, "fn stage_exit_key_matches(", "무대 종료 키 판정 함수");
    assert!(
        src.contains("    if stage_exit_key_matches(key) {"),
        "무대 종료 판정이 `stage_key_decision` 안에서 이 함수를 거치지 않는다."
    );
}

/// 마우스 계층 네 지점이 전부 같은 판정을 본다. modifier-hint 오버레이가 겪은 문제와
/// 동형 — 한 지점만 빠져도 그 경로로 입력이 샌다.
#[test]
fn mouse_layers_share_one_stage_aware_gate() {
    let src = read("src/view/main/mouse.rs");
    only_at(
        &src,
        "fn mouse_overlay_open(&self) -> bool {\n        self.state.settings_open || self.state.fullscreen_stage_active()",
        "통합 판정 정의",
    );
    // 세 핸들러의 지역 바인딩 + OS 리사이즈 양보 인자 + 링크 hover = 5 회 호출.
    let calls = src.matches("self.mouse_overlay_open()").count();
    assert_eq!(
        calls, 5,
        "`mouse_overlay_open()` 호출이 {calls} 회다 — handle_cursor_moved / \
         handle_mouse_input(click-to-activate 포함) / handle_mouse_wheel / \
         try_begin_os_resize / update_hovered_link 다섯 지점 전부가 같은 판정을 봐야 한다."
    );
    // click-to-activate 는 통합 가드보다 위에 있어 별도 인자로 받는다 — 그 인자가
    // 여전히 같은 값에서 오는지 확인한다.
    assert!(
        src.contains("self.try_click_to_activate(button, button_state, overlay_open)"),
        "click-to-activate 가 통합 판정과 다른 값을 받는다 — 무대 중 뒤 surface 로 \
         포커스가 옮겨간다."
    );
    // 옛 정의(`settings_open` 단독)가 되살아나지 않았는지.
    assert!(
        !src.contains("let overlay_open = self.state.settings_open;"),
        "`overlay_open = settings_open` 정의가 되살아났다 — 그 경로는 무대를 모른다."
    );
}

/// 커서 아이콘은 무대 중 뒤 세계 좌표로 판정하지 않는다.
#[test]
fn cursor_icon_bails_out_during_a_stage() {
    let src = read("src/state/mouse.rs");
    let bail = only_at(
        &src,
        "if self.fullscreen_stage_active() {",
        "커서 판정 무대 조기 반환",
    );
    let contains = only_at(
        &src,
        "if !terminal_rect.contains(PhysicalPx(x), PhysicalPx(y)) {",
        "terminal_rect 판정",
    );
    assert!(
        bail < contains,
        "무대 조기 반환이 좌표 판정 뒤로 밀렸다 — 뒤의 divider/터미널 커서가 무대 위에 뜬다."
    );
}

/// OS 레벨 UI 는 입력 게이트가 막아주지 않는다 — 별도 배선이 살아 있는지.
#[test]
fn os_level_ui_is_suppressed_during_a_stage() {
    let src = read("src/view/main/redraw.rs");
    only_at(
        &src,
        "self.sync_fullscreen_stage_transition();",
        "무대 진입 정리 훅 호출",
    );
    let menu_guard = "if self.state.fullscreen_stage_active() {\n            self.state.dialogs.pending_native_menu = None;\n            return;\n        }";
    assert!(
        src.contains(menu_guard),
        "무대 중 네이티브 컨텍스트 메뉴 억제가 사라졌다 — OS 팝업은 wgpu 표면 위에 떠서 \
         무대가 덮지 못한다."
    );
    let drag_guard = "if self.state.fullscreen_stage_active() {\n            self.state.dialogs.pending_file_drag = None;\n        }";
    assert!(
        src.contains(drag_guard),
        "무대 중 네이티브 파일 드래그 억제가 사라졌다."
    );
    // 폴링은 계속 돌아야 한다 — dismiss 의 결과 회수가 그 경로다.
    let poll = only_at(
        &src,
        "self.poll_pending_native_menu();",
        "네이티브 메뉴 폴링",
    );
    let render = only_at(&src, "self.render_if_dirty(", "렌더 호출");
    assert!(
        render < poll,
        "네이티브 메뉴 폴링 위치가 바뀌었다 — 이 가드는 렌더 뒤 폴링을 전제한다."
    );
}
