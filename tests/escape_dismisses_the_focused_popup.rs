//! Escape 가 **포커스된 host popup** 을 푸는 경로의 순서와 위치를 고정한다.
//!
//! 왜 순서를 고정하나: `try_consume_escape_key` 안에 소비자가 셋이고 앞의 둘
//! (`settings_open` · `notifications`)은 **포커스와 무관하게** 열려만 있으면 먹는다.
//! 셋째(포커스된 popup 일반)를 앞으로 올리면 그 둘의 동작이 바뀐다 — 설정 창이 떠 있는
//! 채로 다른 popup 이 포커스를 가지면 Escape 가 설정을 안 닫는다. 순서가 곧 의미라
//! 여기에 박아 둔다.
//!
//! 왜 위치를 고정하나: 이 경로가 **오버레이 게이트보다 앞**에 있어야만 탈출구가 된다.
//! 게이트 뒤로 내려가면 포커스된 popup 이 자기 자신을 푸는 키까지 막는다.
//!
//! 동작 자체(무엇이 대상인가 · 무엇이 대상이 아닌가)는 `adapters::ui::popup` 의 단위
//! 테스트가 잡는다. 여기는 그 함수가 **어디서 어떤 순서로 불리는가**만 본다.

fn read(rel: &str) -> String {
    let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(rel);
    std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("read {}: {e}", p.display()))
}

fn only_at(hay: &str, needle: &str, what: &str) -> usize {
    let n = hay.matches(needle).count();
    assert_eq!(n, 1, "{what}: `{needle}` 이 {n} 곳이다 (1 곳이어야 한다)");
    hay.find(needle).unwrap()
}

/// 두 유일한 표지 사이를 자른다 — 함수 몸통으로 범위를 좁히는 데 쓴다.
fn between<'a>(hay: &'a str, start: &str, end: &str) -> &'a str {
    let a = only_at(hay, start, "범위 시작 표지");
    let b = only_at(hay, end, "범위 끝 표지");
    assert!(
        b > a,
        "범위 끝 표지가 시작보다 앞에 있다 — 함수 순서가 바뀌었다"
    );
    &hay[a..b]
}

const KEYBOARD: &str = "src/view/main/keyboard.rs";

/// 세 소비자의 순서 — settings → notifications → 포커스된 popup 일반.
#[test]
fn the_three_escape_consumers_keep_their_order() {
    let src = read(KEYBOARD);
    // ★ 파일 전체에서 찾으면 안 된다 — `if self.state.settings_open {` 은 이 파일에 **두 곳**
    // 있고(다른 하나는 단축키 경로의 지속 전제), 그것과 섞이면 순서를 엉뚱한 짝으로 잰다.
    // 먼저 그 함수의 몸통으로 좁힌다. 두 경계는 각각 유일하다.
    let body = between(
        &src,
        "fn try_consume_escape_key(",
        "fn try_consume_shortcut_key(",
    );
    let settings = only_at(
        body,
        "if self.state.settings_open {",
        "Escape 의 settings 분기",
    );
    let notifications = only_at(
        body,
        "if self.state.popups.is_open(\"notifications\") {",
        "Escape 의 notifications 분기",
    );
    let general = only_at(
        body,
        "if let Some((id, closes)) = self.state.popups.focused_dismissal_target() {",
        "Escape 의 포커스된 popup 분기",
    );

    assert!(
        settings < notifications,
        "settings 분기가 notifications 보다 뒤에 있다 — 앞의 둘은 포커스와 무관하게 먹으므로 \
         순서가 바뀌면 동작이 바뀐다."
    );
    assert!(
        notifications < general,
        "포커스된 popup 일반 분기가 특례 둘보다 앞에 있다. 그러면 설정 창이 떠 있는 채로 \
         다른 popup 이 포커스를 가질 때 Escape 가 설정을 안 닫는다 — 일반형은 **마지막**이다."
    );
}

/// 그리고 그 셋 전부가 오버레이 게이트보다 **앞**에서 돈다.
#[test]
fn the_escape_path_runs_before_the_overlay_gate() {
    let src = read(KEYBOARD);
    let escape_call = only_at(
        &src,
        "if self.try_consume_escape_key(event) {",
        "Escape 소비 호출",
    );
    let gate = only_at(
        &src,
        "let overlay_open = self.state.keyboard_overlay_open();",
        "오버레이 게이트",
    );
    assert!(
        escape_call < gate,
        "Escape 소비가 오버레이 게이트 뒤로 내려갔다. 그러면 포커스된 popup 이 **자기를 푸는 \
         키까지** 막아, 키보드만 쓰는 사용자에게 탈출구가 사라진다."
    );
}

/// 닫는 것은 `close_on_outside_click` 인 것뿐이다 — 바깥 클릭과 같은 의미여야 한다.
///
/// 이 단정이 지키는 것: Escape 가 popup 마다 새 정책을 만들지 않는다는 것. 조건 없이
/// 닫으면 "바깥 클릭엔 안 닫히는데 Escape 엔 닫히는" popup 이 생기고, 그때부터 각
/// popup 의 Escape 동작을 따로 판단해야 한다.
#[test]
fn escape_closes_only_what_an_outside_click_would_close() {
    let src = read(KEYBOARD);
    let start = only_at(
        &src,
        "if let Some((id, closes)) = self.state.popups.focused_dismissal_target() {",
        "Escape 의 포커스된 popup 분기",
    );
    let body = &src[start..start + 400];
    assert!(
        body.contains("set_focused(id, false)"),
        "포커스를 놓지 않는다 — 그러면 게이트가 안 열려 탈출구가 아니다: {body}"
    );
    assert!(
        body.contains("if closes {"),
        "닫기가 `close_on_outside_click` 로 갈리지 않는다. 조건 없이 닫으면 바깥 클릭과 \
         의미가 갈라지고, 그때부터 popup 마다 Escape 정책을 따로 정해야 한다: {body}"
    );
}
