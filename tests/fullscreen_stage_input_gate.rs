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

/// 종료 키 판정은 한 곳에만 있고, 그 한 곳은 `KeybindingSettings` 를 읽는다.
///
/// 판정 함수가 실제로 설정을 보는지(= 하드코딩이 아닌지)는 단위 테스트
/// (`stage_exit_follows_the_configured_binding`)가 값으로 단정한다. 여기서는 **판정
/// 지점이 하나라는 구조**와 **바인딩 조회가 무대 게이트 안에만 있다는 위치 계약**을
/// 고정한다 — 조회가 게이트 밖으로 나가면 무대가 없을 때도 기본값 ESC 가 매칭돼
/// settings/notifications 닫기·터미널 `\x1b` 전달을 훔친다.
#[test]
fn stage_exit_key_has_a_single_decision_site() {
    let src = read("src/view/main/keyboard.rs");
    only_at(&src, "fn stage_exit_key_matches(", "무대 종료 키 판정 함수");
    assert!(
        src.contains("    if stage_exit_key_matches(exit_bindings, key, mods) {"),
        "무대 종료 판정이 `stage_key_decision` 안에서 이 함수를 거치지 않는다."
    );
    // 값의 출처는 KeybindingSettings 하나뿐이고, 그 조회는 게이트 안에서 한 번만 한다.
    only_at(
        &src,
        "keybindings.fullscreen_stage_exit",
        "무대 종료 바인딩 조회",
    );
    let lookup = only_at(
        &src,
        "&self.core_state.settings.keybindings.fullscreen_stage_exit,",
        "게이트의 바인딩 조회",
    );
    let gate_fn = only_at(
        &src,
        "fn try_consume_fullscreen_stage_key(&mut self, event: &winit::event::KeyEvent) -> bool {",
        "0단계 게이트 함수",
    );
    let stage_active_guard = only_at(
        &src,
        "    if !stage_active {\n        return StageKeyDecision::PassThrough;\n    }",
        "무대 비활성 조기 반환",
    );
    assert!(
        gate_fn < lookup,
        "바인딩 조회가 0단계 게이트 밖으로 나갔다 — 무대가 없을 때도 이 바인딩이 \
         매칭되면 기존 ESC 동작을 훔친다."
    );
    let call_site = only_at(
        &src,
        "    if stage_exit_key_matches(exit_bindings, key, mods) {",
        "판정 호출부",
    );
    assert!(
        stage_active_guard < call_site,
        "`stage_key_decision` 의 무대 비활성 조기 반환이 판정 호출 뒤로 밀렸다 — 무대가 \
         없을 때도 종료 바인딩이 매칭된다."
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
    only_at(
        &src,
        "self.poll_pending_native_menu();",
        "네이티브 메뉴 폴링",
    );
}

/// 네이티브 메뉴 폴링은 렌더 **뒤**다. 무대와 직접 관련은 없지만
/// [`os_level_ui_is_suppressed_during_a_stage`] 의 "폴링은 계속 돈다" 근거가 이 순서에
/// 기대고 있어, 순서가 바뀌면 그 계약을 다시 검토해야 한다는 신호로 따로 고정한다.
#[test]
fn native_menu_polling_stays_after_render() {
    let src = read("src/view/main/redraw.rs");
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

/// 진입 엣지 정리 목록을 항목별로 고정한다.
///
/// 이 정리는 `MainView`(GPU/winit 필요) 없이는 런타임으로 돌릴 수 없어 단위 테스트가
/// 없었고, 그래서 한 줄을 지워도 아무 테스트도 깨지지 않았다. 각 줄이 막는 것이 서로
/// 달라(sticky divider / 유령 선택 / 유령 드래그 / 뒤 좌표 잔재) 하나만 빠져도 다른
/// 증상이 나오므로, 최소한 **배선의 존재**는 구조로 고정한다. 근거는
/// `docs/design/systems/fullscreen-stage.md` § 진입 시 정리.
#[test]
fn stage_entry_discards_every_in_flight_gesture() {
    let src = read("src/view/main/redraw.rs");
    let body = fn_body(&src, "fn sync_fullscreen_stage_transition(&mut self) {");
    for (line, why) in [
        (
            "self.clear_ime_preedit();",
            "조합 중 IME 가 뒤 PTY 로 확정된다",
        ),
        (
            "self.dragging_divider = None;",
            "divider 드래그가 sticky 로 남는다",
        ),
        (
            "self.left_mouse_down = false;",
            "좌클릭 선택 게이트가 눌린 채로 남는다",
        ),
        (
            "self.left_select_bypass = false;",
            "Shift+클릭 우회 플래그가 남아 다음 클릭 경로가 어긋난다",
        ),
        (
            "self.state.popups.cancel_pointer_interactions();",
            "popup 이동/리사이즈가 sticky 로 남는다",
        ),
        (
            "self.hovered_link = None;",
            "뒤 좌표 기반 링크 hover 가 남는다",
        ),
        (
            "self.state.pending_resize_cursor = None;",
            "뒤 좌표 기반 리사이즈 커서가 남는다",
        ),
        (
            "self.dismiss_pending_native_menu();",
            "떠 있던 네이티브 메뉴가 무대 위에 남는다",
        ),
        (
            "self.state.dialogs.pending_native_menu = None;",
            "진입 직전 큐잉된 메뉴 요청이 무대를 나온 뒤 엉뚱한 자리에 뜬다",
        ),
        (
            "self.state.dialogs.pending_file_drag = None;",
            "아무도 누르고 있지 않은 파일 드래그가 시작된다",
        ),
    ] {
        assert!(
            body.contains(line),
            "무대 진입 정리에서 `{line}` 이 사라졌다 — {why}."
        );
    }
}

/// `needle` 로 시작하는 함수의 본문(중괄호 균형 기준).
fn fn_body<'a>(src: &'a str, signature: &str) -> &'a str {
    let start = only_at(src, signature, "함수 시그니처") + signature.len();
    let mut depth = 1usize;
    for (i, c) in src[start..].char_indices() {
        match c {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return &src[start..start + i];
                }
            }
            _ => {}
        }
    }
    panic!("함수 본문의 끝을 찾지 못했다: {signature}");
}

/// IME 경로가 무대를 안다. 무대만 떠 있으면 `keyboard_overlay_open()` 의 네 항이 전부
/// false 라 아무도 안 막고, 조합 중이던 IME 의 Commit 이 뒤 터미널로 샌다.
#[test]
fn ime_overlay_gate_knows_the_stage() {
    let src = read("src/view/main/ime.rs");
    let expected = "    let overlay_open = w.state.keyboard_overlay_open() || w.state.fullscreen_stage_active();";
    assert!(
        src.contains(expected),
        "IME 의 `overlay_open` 이 무대를 보지 않는다 — 무대 중 IME Preedit/Commit 이 \
         뒤 터미널 PTY 로 샌다."
    );
}

/// plugin 단축키 경로가 무대를 안다. 이 경로는 `dispatch_window_event_to_view` **이전에**
/// 호출되므로 `keyboard.rs` 의 0단계 무대 게이트가 도달하지 못한다 — 별도 배선이 필요하다.
#[test]
fn plugin_shortcut_gate_knows_the_stage() {
    let src = read("src/app/plugin_glue/shortcut.rs");
    let expected =
        "        if main.state.keyboard_overlay_open() || main.state.fullscreen_stage_active() {";
    assert!(
        src.contains(expected),
        "plugin 단축키 가드가 무대를 보지 않는다 — 무대 중 plugin 단축키가 발화한다. \
         이 경로는 0단계 키보드 게이트보다 앞서 실행되므로 여기서 직접 막아야 한다."
    );
}

/// `overlay_open` 모양의 합성 판정이 **새로 생겨도** 무대를 빠뜨리지 못하게 하는 완전성 가드.
///
/// 이 트랙이 처음에 ime.rs 와 plugin shortcut 을 놓친 이유가 정확히 "가드가 아는 지점만
/// 봤다" 였다. 그래서 특정 지점을 나열하는 대신, 키보드 계열 오버레이 판정
/// (`AppState::keyboard_overlay_open()`)의 **호출부를 소스에서 기계적으로 전부 찾아**
/// 각각이 무대를 아는지 확인한다. 정의가 하나여도 소비 지점은 넷이고, 그 넷이 무대에
/// 대해 같은 답을 필요로 하지 않기 때문에 정의 한 곳을 보는 것으로는 부족하다.
///
/// 예외는 `keyboard.rs` 하나뿐이다 — 그 파일은 같은 식 **앞**에 0단계 무대 게이트
/// (`try_consume_fullscreen_stage_key`)를 따로 두어 이미 무대를 처리한다. 그 게이트가
/// 사라지면 이 테스트도 함께 깨진다.
///
/// 나머지 두 정의(`mouse_overlay_open` / `has_egui_overlay_open`)는 정의 자체에 무대가
/// 들어가므로 여기서 정의만 확인한다.
#[test]
fn every_overlay_open_composite_is_stage_aware() {
    const ALLOWED_WITHOUT_STAGE_TERM: &str = "src/view/main/keyboard.rs";
    let mut files = Vec::new();
    collect_rs(
        &PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src"),
        &mut files,
    );

    let mut seen = Vec::new();
    for rel in &files {
        let src = read(rel);
        for (i, _) in src.match_indices(".keyboard_overlay_open()") {
            let expr = enclosing_expr(&src, i);
            seen.push(rel.clone());
            if rel == ALLOWED_WITHOUT_STAGE_TERM {
                assert!(
                    src.contains("fn try_consume_fullscreen_stage_key")
                        || src.contains("self.try_consume_fullscreen_stage_key(event)"),
                    "{rel}: 무대 항 없이 예외로 허용되던 근거(0단계 무대 게이트)가 사라졌다."
                );
                continue;
            }
            assert!(
                expr.contains("fullscreen_stage_active()"),
                "{rel}: `keyboard_overlay_open()` 호출부가 무대를 보지 않는다 — 그 경로로 \
                 무대 중 입력이 뒤 세계로 샌다. 식에 `|| ...fullscreen_stage_active()` 를 \
                 더하거나, 별도 무대 게이트를 앞에 두고 이 테스트의 예외 목록에 근거와 함께 \
                 추가하라.\n문제의 식:\n{expr}"
            );
        }
    }
    seen.sort();
    seen.dedup();
    assert_eq!(
        seen,
        vec![
            "src/adapters/ipc/handler/debug_state.rs".to_string(),
            "src/app/plugin_glue/shortcut.rs".to_string(),
            "src/app/webview_keys.rs".to_string(),
            "src/view/main.rs".to_string(),
            "src/view/main/ime.rs".to_string(),
            "src/view/main/keyboard.rs".to_string(),
        ],
        "`keyboard_overlay_open()` 호출부 집합이 바뀌었다. 새 지점이 생겼다면 \
         docs/architecture/input-layer.md 의 열거도 함께 갱신하라."
    );

    // 나머지 두 정의는 정의 자체가 무대를 품는다.
    let mouse = read("src/view/main/mouse.rs");
    assert!(
        mouse.contains("self.state.settings_open || self.state.fullscreen_stage_active()"),
        "`mouse_overlay_open()` 정의에서 무대가 빠졌다 — 마우스 전 경로가 무대를 모르게 된다."
    );
    let state = read("src/state.rs");
    let webview = fn_body(&state, "pub fn has_egui_overlay_open(&self) -> bool {");
    assert!(
        webview.contains("self.fullscreen_stage.is_some()"),
        "`has_egui_overlay_open()` 에서 무대가 빠졌다 — WebView 가 무대를 뚫고 나온다."
    );
}

/// `src/` 아래 `.rs` 파일을 매니페스트 상대 경로로 모은다.
fn collect_rs(dir: &std::path::Path, out: &mut Vec<String>) {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let entries =
        std::fs::read_dir(dir).unwrap_or_else(|e| panic!("read_dir {}: {e}", dir.display()));
    for entry in entries {
        let path = entry.expect("dir entry").path();
        if path.is_dir() {
            collect_rs(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            let rel = path.strip_prefix(&root).expect("under manifest dir");
            out.push(rel.to_string_lossy().replace('\\', "/"));
        }
    }
}

/// `at` 을 포함하는 식의 대략적 경계 — 앞뒤로 가장 가까운 `;` / `{` / `}` 까지.
/// `;{}` 는 ASCII 라 자른 위치는 항상 char 경계다.
fn enclosing_expr(src: &str, at: usize) -> &str {
    let start = src[..at].rfind([';', '{', '}']).map_or(0, |i| i + 1);
    let end = src[at..].find([';', '{']).map_or(src.len(), |i| at + i + 1);
    &src[start..end]
}
