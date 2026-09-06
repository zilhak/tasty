//! **사용자가 타이핑하는 훅 이벤트 이름이 늘면 사용자 가이드가 그것을 알아야 한다.**
//!
//! `CLAUDE.md` 의 "문서 갱신 (필수)" 가 묶은 축 중 CLI 쪽 갈래다. `tasty set hook --event
//! <이름>` 의 `<이름>` 은 사용자가 **손으로 치는** 문자열이라, 가이드가 그 이름을 안 적으면
//! 그 기능은 문서에 있으나 못 쓰는 상태가 된다.
//!
//! # 왜 이 축은 대조가 서는가
//!
//! 두 쪽이 같은 어휘를 쓴다 — 소스의 파서가 받는 문자열과 가이드가 적는 문자열이 **글자
//! 그대로 같다.** 그럴 수밖에 없다: 가이드가 다른 낱말로 쓰면 그대로 따라 친 사용자의
//! 명령이 실패한다. 표기 변형이 낄 자리가 없다.
//!
//! 같은 이유로 **안 서는 축**들과 대비된다(`docs/dev-guide/ci-gates.md` 의 축 표) — 단축키는
//! 설정 표기(`alt+up`)와 화면 표기(`Alt+↑`)가 애초에 둘이라 인용할 원본이 없다.
//!
//! # 모수
//!
//! [`crates/tasty-hooks/src/lib.rs`] 의 `HookEvent::to_display_string` 이 내는 이름 전부.
//! **`parse` 가 아니라 `to_display_string` 에서 뽑는다** — `parse` 는 `if/else` 사슬이라
//! 판독이 무르고, 무엇보다 사용자가 보는 정본 표기는 직렬화 쪽이다. 실측(2026-09-06) **6**.
//!
//! 인자를 받는 것(`output-match:` · `idle-timeout:` · `command-completed:`)은 **접두사까지**
//! 가 이름이고 뒤는 사용자 값이라, 콜론 앞만 본다.
//!
//! # 이 가드가 단정하지 않는 것
//!
//! - **가이드가 그 이벤트를 제대로 설명하는지.** 이름이 한 번 나오면 통과다.
//! - **`HookEvent::Custom`.** 고정된 이름이 없다(plugin 이 정한다) — 적을 이름 자체가 없다.
//! - **영어 번역(`site/content/en/`).** 원본이 정본이라 여기서 안 본다.
//!
//! # 채널
//!
//! `doc-guards.yml` — main push · PR 마다 경로 필터 없이 돈다. 이 축을 재는 채널은 그 하나다.

use std::path::{Path, PathBuf};

/// 가이드에 **일부러 없는** 이벤트와 그 사유. 자리로 적는다 — 부류로 적으면 도망길이 된다.
///
/// 지금 비어 있다. 오늘 실측이 **6/6** 이라 부채가 없다.
const NOT_IN_THE_GUIDE: &[(&str, &str)] = &[];

/// 훑어야 할 최소 이벤트 수 — **모수가 살아 있다는 증거**. 실측 6(2026-09-06).
///
/// ★ 이 수를 내려서 통과시키지 마라. 먼저 가른다 — `HookEvent` 의 변이가 정말 줄었나,
/// 아니면 `to_display_string` 의 모양이 바뀌어 판독이 못 읽나. 뒤쪽이면 하한을 내리는 것은
/// 고장을 초록으로 만드는 것이다.
const MIN_EVENTS: usize = 4;

fn repo_root() -> PathBuf {
    tasty_doc_guards::repo_root()
}

/// `HookEvent::to_display_string` 이 내는 이름.
///
/// 두 형태를 읽는다 — `"<이름>".to_string()` 과 `format!("<이름>:{}", …)`. 뒤쪽은 콜론
/// 앞까지가 이름이다.
fn event_names(root: &Path) -> Vec<String> {
    let src = std::fs::read_to_string(root.join("crates/tasty-hooks/src/lib.rs"))
        .expect("tasty-hooks/src/lib.rs 를 읽지 못했다");
    // ★ 이름만으로 찾으면 안 된다 — 같은 파일에 `HookBinding::to_display_string` 이
    // **먼저** 있어서, 이름으로 잡으면 그쪽 본문을 읽고 이벤트 이름을 하나도 못 낸다
    // (첫 실행에서 실제로 그랬고 독자 단정이 그것을 잡았다). 그래서 impl 블록으로 먼저
    // 좁힌다 — 바늘을 이름이 아니라 **소속**으로 든다.
    let impl_at = src
        .find("impl HookEvent {")
        .expect("`impl HookEvent` 를 못 찾았다 — 타입 이름이 바뀌었나");
    let impl_body = &src[impl_at..];
    let start = impl_at
        + impl_body
            .find("fn to_display_string")
            .expect("`HookEvent::to_display_string` 을 못 찾았다 — 직렬화 함수 이름이 바뀌었나");
    let body = &src[start..];
    let end = body.find("\n    }").expect("함수 끝을 못 찾았다");
    let body = &body[..end];

    let mut out = Vec::new();
    for (i, _) in body.match_indices('"') {
        let rest = &body[i + 1..];
        let Some(close) = rest.find('"') else {
            continue;
        };
        let literal = &rest[..close];
        // 이름은 `a-z` 와 `-` 로 되어 있다. `{}` 가 낀 형식 문자열은 콜론 앞만 쓴다.
        let name = literal.split(':').next().unwrap_or(literal);
        if name.is_empty() || !name.chars().all(|c| c.is_ascii_lowercase() || c == '-') {
            continue;
        }
        out.push(name.to_string());
    }
    out.sort();
    out.dedup();
    out
}

/// 한국어 가이드 원본 전체를 한 덩어리로.
fn guide_text(root: &Path) -> String {
    fn walk(dir: &Path, out: &mut String) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                if path.file_name().map(|n| n == "en").unwrap_or(false) {
                    continue; // 번역은 별도 절차다.
                }
                walk(&path, out);
            } else if path.extension().and_then(|e| e.to_str()) == Some("md")
                && let Ok(text) = std::fs::read_to_string(&path)
            {
                out.push_str(&text);
                out.push('\n');
            }
        }
    }
    let mut out = String::new();
    walk(&root.join("site/content"), &mut out);
    out
}

#[test]
fn every_hook_event_name_is_in_the_guide_or_registered_with_a_reason() {
    let root = repo_root();
    let events = event_names(&root);
    assert!(
        events.len() >= MIN_EVENTS,
        "훅 이벤트 이름을 {}개밖에 못 찾았다(하한 {MIN_EVENTS}) — 판독이 깨졌다.\n\
         ★ 이 수를 내려서 통과시키지 마라. `HookEvent` 의 변이를 먼저 세라.",
        events.len()
    );

    let guide = guide_text(&root);
    let missing: Vec<&String> = events
        .iter()
        .filter(|e| !guide.contains(e.as_str()))
        .filter(|e| !NOT_IN_THE_GUIDE.iter().any(|(n, _)| *n == e.as_str()))
        .collect();

    assert!(
        missing.is_empty(),
        "사용자가 `tasty set hook --event <이름>` 으로 **직접 치는** 이름인데 가이드가 한 번도 \
         안 적는다:\n  {}\n\n\
         이 이름은 표기 변형이 낄 자리가 없다 — 가이드가 다른 낱말로 쓰면 그대로 따라 친 \
         명령이 실패한다. 고치는 길 둘:\n\
           (가) 훅 장에 그 이름을 적는다.\n\
           (나) 사용자가 칠 이름이 **아니면** 이 파일의 `NOT_IN_THE_GUIDE` 에 사유와 함께 \
         등록해라. ★ 사유가 '아직 안 썼다' 면 그것은 예외가 아니라 부채다.",
        missing
            .iter()
            .map(|s| s.as_str())
            .collect::<Vec<_>>()
            .join("\n  ")
    );
}

#[test]
fn no_registered_event_is_already_in_the_guide() {
    let root = repo_root();
    let guide = guide_text(&root);
    let stale: Vec<&str> = NOT_IN_THE_GUIDE
        .iter()
        .map(|(n, _)| *n)
        .filter(|n| guide.contains(n))
        .collect();
    assert!(
        stale.is_empty(),
        "가이드가 이 이름을 이미 적는데 명부에 남아 있다: {stale:?} — 부채를 갚았으면 그 줄을 \
         지워라."
    );
}

#[test]
fn every_registered_event_still_exists() {
    let root = repo_root();
    let events = event_names(&root);
    let dead: Vec<&str> = NOT_IN_THE_GUIDE
        .iter()
        .map(|(n, _)| *n)
        .filter(|n| !events.iter().any(|e| e == n))
        .collect();
    assert!(
        dead.is_empty(),
        "명부가 이제 없는 이벤트를 붙들고 있다: {dead:?}"
    );
}

#[test]
fn the_reader_answers_both_yes_and_no() {
    let root = repo_root();
    let events = event_names(&root);
    for expected in ["process-exit", "bell", "notification", "output-match"] {
        assert!(
            events.iter().any(|e| e == expected),
            "판독이 {expected:?} 를 놓쳤다 — 직렬화 함수의 모양이 바뀌었나"
        );
    }
    assert!(
        !events.iter().any(|e| e.contains(':')),
        "인자 접두사에서 콜론을 못 떼어 냈다 — 사용자 값까지 이름으로 세게 된다"
    );
    assert!(
        !events.iter().any(|e| e.contains('{')),
        "형식 문자열 조각이 이름으로 새어 들어왔다"
    );

    let guide = guide_text(&root);
    assert!(guide.contains("process-exit"), "예: 있음");
    assert!(
        !guide.contains("nonexistent-hook-event"),
        "예: 없음 — 없는 것을 있다고 읽으면 이 가드는 아무것도 안 본다"
    );
}
