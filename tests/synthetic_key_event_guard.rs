//! winit 합성 키 이벤트 차단의 **배선 위치**를 소스 구조로 고정하는 가드.
//!
//! winit 은 창이 포커스를 얻/잃는 순간 그때 물리적으로 눌려 있던 모든 키에 대해
//! `Pressed`/`Released` 를 합성해 보낸다(X11·Windows). 사용자가 이 창 안에서 누른 적
//! 없는 키이므로 단축키·PTY·egui 어디에도 닿아선 안 된다. 정책 본문은
//! `docs/design/policies/key-mapping.md` 의 "합성 키 이벤트" 절.
//!
//! 판정 자체(`is_synthetic_key_event`)는 순수 함수라 단위 테스트로 덮인다. 하지만
//! **어디에 꽂혀 있는가** 는 `App` 을 GPU/winit 없이 구성할 수 없어 런타임으로 단정할
//! 수 없는데, 이 트랙의 계약은 전적으로 위치 계약이다:
//!
//! 1. 게이트는 `window_event` **진입부** — shell setup / 종료 / 부팅 / 모달 / plugin
//!    단축키 가로채기보다 앞에 있어야 한다. 이 분기들은 전부 조기 return 하는 배타적
//!    경로라, 하나라도 게이트보다 앞서면 그 모드에서 합성 키가 그대로 샌다.
//! 2. View 5 종(`MainView`/`SettingsView`/`PresetView`/`PluginsView`/`QuitView`)은
//!    `KeyboardInput` 을 패턴 매칭하지 않고 `handle_egui_event` 로 통째로 넘기는 경로도
//!    갖는다(`PresetView`·`PluginsView` 는 그 경로**뿐**). 그래서 지점별 차단으로는
//!    누락이 생기고, 진입부 단일 게이트만이 전부를 덮는다.
//! 3. `DoubleTapDetector` 는 포커스 전환 시 지워야 한다. 합성 release 를 버리면 짝 없는
//!    press 가 남아 다음 실제 탭 한 번에 double-tap 이 오발화한다. MainView 와
//!    SettingsView 는 **별개 인스턴스**라 두 곳 모두 배선돼야 한다.
//!
//! 선례: `tests/fullscreen_stage_input_gate.rs`.

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

/// 합성 키 게이트는 `window_event` 의 모든 모드 분기보다 앞이다.
#[test]
fn synthetic_gate_precedes_every_mode_branch() {
    let src = read("src/app/event_handler.rs");
    let gate = only_at(
        &src,
        "if is_synthetic_key_event(&event) {",
        "합성 키 진입부 게이트",
    );

    // 각 분기는 조기 return 하는 배타적 경로다. 게이트가 뒤로 밀리면 그 모드에서만
    // 합성 키가 새는, 재현하기 까다로운 부분 회귀가 된다.
    for (needle, what) in [
        ("if self.shell_setup_mode {", "shell setup 모드 분기"),
        (
            "self.handle_shutdown_window_event(event_loop, id, event);",
            "종료 상태 머신 분기",
        ),
        (
            "self.handle_boot_window_event(event_loop, event);",
            "부팅 상태 머신 분기",
        ),
        (
            "self.handle_active_modal_window_event(event_loop, id, event);",
            "활성 모달 분기",
        ),
        (
            "let plugin_consumed = if let WindowEvent::KeyboardInput { event: ke, .. } = &event {",
            "plugin 단축키 가로채기",
        ),
        (
            "self.dispatch_window_event_to_view(event_loop, id, event);",
            "View 위임(축 A·B 전부가 이 아래)",
        ),
    ] {
        let at = only_at(&src, needle, what);
        assert!(
            gate < at,
            "합성 키 게이트가 {what} 뒤로 밀렸다 — 그 경로로 유령 키 입력이 샌다."
        );
    }
}

/// double-tap detector 는 포커스 전환마다 지워진다 — 두 인스턴스 모두.
#[test]
fn double_tap_detectors_reset_on_focus_change() {
    for (rel, what, arm) in [
        (
            "src/view/main.rs",
            "MainView",
            "WindowEvent::Focused(focused) => {",
        ),
        (
            "src/view/settings.rs",
            "SettingsView",
            "WindowEvent::Focused(_) => {",
        ),
    ] {
        let src = read(rel);
        let reset = only_at(&src, "self.double_tap.reset();", &format!("{what} reset"));
        let focused = only_at(&src, arm, &format!("{what} Focused arm"));
        assert!(
            focused < reset && reset - focused < 600,
            "{what}: reset 이 Focused arm 안에 있지 않다 — 포커스 전환에 걸리지 않는다."
        );
    }
}

/// 판정 함수가 실제로 `is_synthetic` 플래그를 본다.
///
/// 위 두 테스트는 **배선 위치**만 고정한다. 그래서 `is_synthetic_key_event` 의 몸통을
/// 상수 `false` 로 바꿔 게이트를 통째로 무력화해도 둘 다 통과한다 — 수정 전체가 죽은 채
/// CI 가 초록인 구멍이 생긴다. 이 단정이 그 구멍을 막는다.
///
/// 런타임으로 단정할 수 없어 소스 텍스트로 고정한다: winit `KeyEvent` 의
/// `platform_specific` 필드가 `pub(crate)` 라 크레이트 밖에서는
/// `WindowEvent::KeyboardInput` 을 구성할 수 없다.
#[test]
fn predicate_reads_the_synthetic_flag() {
    let src = read("src/adapters/ui/input/synthetic.rs");
    let body = src
        .split_once("pub fn is_synthetic_key_event")
        .expect("`is_synthetic_key_event` 정의가 사라졌다")
        .1;
    // 아래 `#[cfg(test)]` 모듈은 주석에서 같은 이름을 언급하므로 잘라낸다.
    let body = body
        .split("#[cfg(test)]")
        .next()
        .expect("split always yields one");

    for needle in ["WindowEvent::KeyboardInput", "is_synthetic: true"] {
        assert!(
            body.contains(needle),
            "is_synthetic_key_event 본문에 `{needle}` 이 없다 — 판정이 합성 플래그를 \
             보지 않으면 진입부 게이트는 배선만 남고 아무것도 거르지 않는다."
        );
    }
}
