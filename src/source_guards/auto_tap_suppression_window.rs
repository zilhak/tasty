//! 구조 변경 중 켠 자동 tap 억제가 결과를 처리하기 전에 해제되는지 소스 순서로 확인한다.
//! 해제 전에 함수에서 빠져나가면 이후 추가한 터미널도 tap되지 않을 수 있다.
//!
//! 여는 setter 뒤의 다음 setter가 닫는 호출인지, 사이에 ?, return, break, continue가 없는지 본다.
//! 플래그 읽기가 tap 경로에 있는지도 확인한다. 다만 조건 분기나 panic 때의 복원까지 보장하지 않는다.
//! 구간에 호출 형태가 있는지만 검사하므로 정확한 도메인 함수를 감쌌는지도 판단하지 않는다.
//! RAII로 설계를 바꾸면 빈 검사로 통과시키지 말고 이 검사도 새 구조에 맞춰야 한다.

use super::{fn_body, line_of, mask_non_code, repo_root};

const HOME: &str = "src/core/attach_runtime.rs";
const WINDOW_FN: &str = "fn execute_forwarded_structural_op";
const OPEN: &str = "set_auto_tap_suppressed(true)";
const CLOSE: &str = "set_auto_tap_suppressed(false)";
const READ: &str = "is_auto_tap_suppressed()";
const READER_FN: &str = "fn tap_new_workspace_member";
const ESCAPES: &[&str] = &["?", "return", "break", "continue"];

/// 주석·리터럴을 호출로 세지 않도록 마스킹한다.
fn masked_home() -> String {
    let path = repo_root().join(HOME);
    let src = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("{HOME} 을 읽지 못했다: {e}"))
        .replace("\r\n", "\n");
    mask_non_code(&src)
}

fn body_of(masked: &str, signature: &str) -> String {
    fn_body(masked, signature).unwrap_or_else(|| {
        panic!("{HOME}에서 {signature} 본문을 읽지 못했다. 함수 이동·이름·파서를 확인한다.")
    })
}

fn all_positions(body: &str, needle: &str) -> Vec<usize> {
    body.match_indices(needle).map(|(i, _)| i).collect()
}

#[test]
fn every_suppression_window_is_closed_before_any_way_out() {
    let masked = masked_home();
    let body = body_of(&masked, WINDOW_FN);
    let opens = all_positions(&body, OPEN);
    let closes = all_positions(&body, CLOSE);

    assert!(
        !opens.is_empty(),
        "{WINDOW_FN}에서 {OPEN}을 찾지 못했다. RAII로 바꿨다면 새 구조를 검사하도록 갱신한다."
    );
    assert_eq!(
        opens.len(),
        closes.len(),
        "tap 억제 시작 {}개와 해제 {}개의 수가 다르다. 해제를 빠뜨리면 이후 workspace 멤버 추가도 억제될 수 있다.",
        opens.len(),
        closes.len(),
    );

    for open in opens {
        let after = open + OPEN.len();
        let next_close = body[after..].find(CLOSE).map(|p| p + after);
        let next_open = body[after..].find(OPEN).map(|p| p + after);
        let close = next_close.unwrap_or_else(|| {
            panic!(
                "{HOME}:{} 의 `{OPEN}` 뒤에 `{CLOSE}` 가 없다 — 구간이 안 닫힌다",
                line_of(&masked, open)
            )
        });
        if let Some(again) = next_open {
            assert!(
                close < again,
                "{HOME}:{} 의 구간이 닫히기 전에 {} 줄에서 다시 열린다 — 중첩된 억제는 \
                 안쪽이 닫힐 때 바깥 구간까지 함께 풀어 버린다",
                line_of(&masked, open),
                line_of(&masked, again),
            );
        }

        let span = &body[after..close];
        for escape in ESCAPES {
            let found = if *escape == "?" {
                span.contains('?')
            } else {
                span.split(|c: char| !c.is_alphanumeric() && c != '_')
                    .any(|w| w == *escape)
            };
            assert!(
                !found,
                "{HOME}:{}의 tap 억제 구간에 {escape}가 있다. 이 경로로 빠지면 {CLOSE}가 실행되지 않아 이후 추가한 터미널도 tap되지 않을 수 있다.",
                line_of(&masked, open),
            );
        }
        assert!(
            span.contains('('),
            "{HOME}:{}의 억제 구간에서 호출을 찾지 못했다. 필요한 작업을 감싸는지 확인한다.",
            line_of(&masked, open),
        );
    }
}

#[test]
fn the_flag_still_has_a_reader_and_it_is_the_tap_path() {
    let masked = masked_home();
    let reads = all_positions(&masked, READ);
    assert!(
        !reads.is_empty(),
        "{READ} 호출이 없다. tap 경로가 억제를 확인하는지 검토한다."
    );
    let reader = body_of(&masked, READER_FN);
    let reader_at = masked
        .find(&reader)
        .expect("잘라 온 본문이 원본에 없다 — 자르기가 깨졌다");
    for read in reads {
        assert!(
            read >= reader_at && read < reader_at + reader.len(),
            "{HOME}:{}의 {READ}가 {READER_FN} 밖에 있다. 구조 변경 중 다른 동작까지 억제하지 않는지 확인한다.",
            line_of(&masked, read),
        );
    }
}

#[test]
fn the_guard_cuts_the_two_functions_and_not_some_others() {
    // 함수 추출이 다른 본문을 반환하지 않았는지 주변의 호출 형태도 확인한다.
    let masked = masked_home();
    let window = body_of(&masked, WINDOW_FN);
    for neighbour in ["StructuralOp::SplitSurface", "exec::create_tab("] {
        assert!(
            window.contains(neighbour),
            "잘라 온 본문이 `{WINDOW_FN}` 이 아니다 — `{neighbour}` 가 안 보인다"
        );
    }
    let reader = body_of(&masked, READER_FN);
    for neighbour in ["add_workspace_member(", "tap_surface_for_stream("] {
        assert!(
            reader.contains(neighbour),
            "잘라 온 본문이 `{READER_FN}` 이 아니다 — `{neighbour}` 가 안 보인다"
        );
    }
    assert!(
        window.len() > reader.len(),
        "두 자름이 같은 본문을 가리킨다 — 자르기가 깨졌다"
    );
}
