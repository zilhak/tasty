//! modifier-hint 오버레이의 **테두리를 콘텐츠 뒤에 그리는 순서**를 못 박는다.
//!
//! ## 계약
//!
//! `draw_modifier_hint` 안에서 `draw_shell_border` 가 `draw_content` 보다 **뒤에**
//! 와야 한다. 이유는 취향이 아니라 덮어쓰기다 — `draw_content` 의 첫 동작이 드래그
//! 스트립 배경을 `radius 0.0` 불투명으로 채우는 것이라(`rect_filled`), 테두리를 먼저
//! 그리면 스트립이 지나가는 구간의 테두리가 **지워진다.** 그래서 셸의 테두리만 떼어
//! 맨 뒤에 다시 그린다(CSS 박스 모델의 border-on-top 재현).
//!
//! 어기면 컴파일도 되고 시험도 통과한다 — 오버레이 위쪽 몇 px 의 테두리가 사라질 뿐이다.
//! 실측(변이: `draw_shell_border` 호출을 `draw_content` 앞으로 옮김): `-p tasty
//! --bin tasty` 전체에서 죽는 시험이 **이 가드 하나뿐**이었다.
//!
//! ## 이 계약을 진술하던 자리가 둘이었다
//!
//! `draw_shell` 의 doc("테두리는 `draw_shell_border` 가 `draw_content` **이후에**
//! 별도로 그린다")과 `draw_shell_border` 자신의 doc("`draw_content` 호출 이후에
//! 실행해야 한다"). 둘 다 **결과**를 진술하고, 순서를 실제로 정하는 것은 셋을 잇달아
//! 부르는 `draw_modifier_hint` 의 세 줄이다. 집을 그 호출부로 옮기고 두 doc 은 그리로
//! 가리키게만 했다 — 두 번 적힌 계약은 한쪽만 고쳐진다.
//!
//! ## 무엇을 안 물었나
//!
//! `draw_shell` 이 `draw_content` 보다 앞이라는 것도 참이지만 **적혀 있지 않다.**
//! 순서만 그렇게 되어 있는 것은 계약이 아니므로 단언에 끼워 넣지 않고, 공허 방지의
//! 이웃 호출로만 쓴다.

use super::{fn_body, repo_root, strip_comments};

/// 계약이 집행되는 파일과 함수.
const HOME: &str = "src/adapters/ui/modifier_hint_overlay.rs";
const DRAW_FN: &str = "fn draw_modifier_hint";
/// 먼저 와야 하는 호출 — 스트립 배경을 불투명으로 채운다.
const CONTENT: &str = "draw_content(";
/// 뒤에 와야 하는 호출 — 덮인 테두리를 다시 그린다.
const BORDER: &str = "draw_shell_border(";

fn draw_body() -> String {
    let path = repo_root().join(HOME);
    let src = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("{HOME} 을 읽지 못했다: {e}"))
        .replace("\r\n", "\n");
    let body = fn_body(&src, DRAW_FN).unwrap_or_else(|| {
        panic!(
            "`{DRAW_FN}` 을 {HOME} 에서 자르지 못했다 — 이름이 바뀌었거나 자리를 옮겼다. \
             못 자른 상태의 초록은 통과가 아니라 미측정이니 이 가드의 상수를 함께 고쳐라"
        )
    });
    strip_comments(&body)
}

#[test]
fn the_shell_border_is_redrawn_after_the_content() {
    let body = draw_body();
    let content = body.find(CONTENT).unwrap_or_else(|| {
        panic!("`{DRAW_FN}` 안에 `{CONTENT}` 가 없다 — 콘텐츠 draw 가 사라졌다")
    });
    let border = body.find(BORDER).unwrap_or_else(|| {
        panic!(
            "`{DRAW_FN}` 안에 `{BORDER}` 가 없다 — 테두리 재도색이 사라졌다. 셸에 테두리를 \
             합쳤다면 스트립 배경이 그것을 덮는지부터 확인해라"
        )
    });
    assert!(
        content < border,
        "modifier-hint 도색 순서 계약이 깨졌다: `{DRAW_FN}` 안에서 `{BORDER}` 가 \
         `{CONTENT}` 보다 먼저 온다. 그러면 스트립 배경(`radius 0.0` 불투명)이 그 구간의 \
         테두리를 덮어 지운다 — 컴파일도 되고 어떤 시험에도 안 걸린다. 계약 본문은 \
         {HOME} 의 그 세 줄 위에 있다"
    );
}

#[test]
fn the_guard_reads_the_overlay_draw_fn_and_not_some_other_function() {
    // 공허 방지 — `fn_body` 가 엉뚱한 함수를 잘라도 두 호출이 우연히 들어 있을 수 있다.
    // 이 함수에만 있는 이웃으로 잘라 온 것을 확인한다(`draw_shell` 은 계약이 아니라
    // 여기서 이웃으로만 쓴다 — 모듈 doc "무엇을 안 물었나" 참고).
    let body = draw_body();
    for neighbour in ["draw_shell(", "egui::Area::new(", "clamp_rect("] {
        assert!(
            body.contains(neighbour),
            "잘라 온 본문이 `{DRAW_FN}` 이 아니다 — `{neighbour}` 가 안 보인다"
        );
    }
}
