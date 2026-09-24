//! HookEvent::to_display_string이 내는 고정 이벤트 이름이 한국어 사용자 가이드에 있는지 확인한다.
//! 인자가 있는 이벤트는 콜론 앞 이름만 대조한다. parse 대신 사용자에게 표시되는 이름을 읽는다.
//! 가이드의 설명 품질·영어 번역·플러그인이 정하는 Custom 이름은 검사하지 않는다.
//! doc-guards.yml의 경로 필터 없는 main push·PR에서 실행된다.

use std::path::{Path, PathBuf};
use tasty_doc_guards::floored_walk::{Descend, Floor, walk_with_floor};
use tasty_doc_guards::temp_scratch::Scratch;

/// 가이드에 싣지 않는 이벤트와 사유.
const NOT_IN_THE_GUIDE: &[(&str, &str)] = &[];

/// 2026-09-06 실측 6개를 기준으로 둔다. 미달하면 실제 이벤트 감소와 판독 실패를 구별한다.
const MIN_EVENTS: usize = 4;

fn repo_root() -> PathBuf {
    tasty_doc_guards::repo_root()
}

/// 표시 함수의 문자열에서 콜론 앞 고정 이름을 추출한다.
fn event_names(root: &Path) -> Vec<String> {
    let src = std::fs::read_to_string(root.join("crates/tasty-hooks/src/lib.rs"))
        .expect("tasty-hooks/src/lib.rs 를 읽지 못했다");
    // HookBinding에도 같은 메서드가 있어 먼저 HookEvent의 impl 범위로 좁힌다.
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

/// 한국어 Markdown 가이드 수집의 하한.
const GUIDE_FLOOR: Floor = Floor {
    min: 12,
    measured: 18,
    measured_on: "2026-09-07",
    counted_on: tasty_doc_guards::floored_walk::CountedOn::NEVER_COUNTED,
    why_this_gap: "site/content의 한국어 Markdown만 세고 en 번역은 제외한다. 문서 분할·통합에 여유를 주되 하한 12 미달이면 실제 감소와 순회 오류를 확인한다.",
};

fn guide_text(root: &Path) -> String {
    guide_text_under(&root.join("site/content"), &GUIDE_FLOOR)
}

/// 작은 합성 트리도 같은 순회로 검증하도록 경로·하한을 인자로 받는다.
fn guide_text_under(dir: &Path, floor: &Floor) -> String {
    let walked = walk_with_floor(dir, dir, floor, Descend::Everything, &|w| {
        w.rel.ends_with(".md") && !w.rel.starts_with("en/")
    })
    .unwrap_or_else(|why| panic!("{why}"));

    let mut out = String::new();
    for w in walked {
        // 수집 하한은 파일 수만 보므로 본문 읽기 실패도 별도로 보고해야 한다.
        let text = std::fs::read_to_string(&w.path)
            .unwrap_or_else(|e| panic!("가이드 원본을 읽지 못했다: {} — {e}", w.path.display()));
        out.push_str(&text);
        out.push('\n');
    }
    out
}

#[test]
fn every_hook_event_name_is_in_the_guide_or_registered_with_a_reason() {
    let root = repo_root();
    let events = event_names(&root);
    assert!(
        events.len() >= MIN_EVENTS,
        "훅 이벤트를 {}개만 읽었다(하한 {MIN_EVENTS}). HookEvent의 정의와 표시 함수 판독을 확인한다.",
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
        "사용자가 입력하는 훅 이벤트 이름이 가이드에 없다:\n  {}\n훅 가이드에 정확한 이름을 적거나 사용자용이 아닌 이유를 NOT_IN_THE_GUIDE에 등록한다. 아직 작성하지 못한 경우는 정책 예외와 구별해 부채로 기록한다.",
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
        "가이드에 설명한 이벤트가 제외 목록에 남았다: {stale:?}. 완료한 항목을 제거한다."
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
        "정의에서 사라진 이벤트가 제외 목록에 남았다: {dead:?}"
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

/// 잘못된 impl이나 영어 번역이 섞이면 수가 늘어 하한을 통과할 수 있으므로 합성 입력에서 구별한다.
#[test]
fn both_readers_answer_on_a_substituted_tree() {
    let probe = Scratch::new("hook-event-reader");
    let dir = probe.path();

    let src = dir.join("crates/tasty-hooks/src");
    std::fs::create_dir_all(&src).expect("합성 소스 트리를 만들지 못했다");
    std::fs::write(
        src.join("lib.rs"),
        "impl HookBinding {\n    \
             fn to_display_string(&self) -> String {\n        \
                 \"decoy-alpha\".to_string()\n    \
             }\n\
         }\n\n\
         impl HookEvent {\n    \
             fn to_display_string(&self) -> String {\n        \
                 match self {\n            \
                     A => \"zeta-signal\".to_string(),\n            \
                     B => format!(\"omega-probe:{}\", v),\n            \
                     C => \"NotLowercase\".to_string(),\n        \
                 }\n    \
             }\n\
         }\n",
    )
    .expect("합성 lib.rs 를 쓰지 못했다");

    let names = event_names(dir);
    assert_eq!(
        names,
        vec!["omega-probe".to_string(), "zeta-signal".to_string()],
        "판독이 합성 트리에서 다른 답을 냈다"
    );
    assert!(
        !names.iter().any(|n| n == "decoy-alpha"),
        "소속으로 안 좁혔다 — 앞선 다른 impl 의 같은 이름 함수를 읽었다"
    );
    assert!(
        !names.iter().any(|n| n.contains(':') || n.contains('{')),
        "인자 접두사에서 콜론 뒤를 못 떼어 냈다"
    );
    assert!(
        !names.iter().any(|n| n == "NotLowercase"),
        "이름 자리가 아닌 리터럴이 새어 들어왔다"
    );

    let content = dir.join("content");
    std::fs::create_dir_all(content.join("en")).expect("합성 가이드 트리를 만들지 못했다");
    std::fs::write(content.join("hooks.md"), "본문에 zeta-signal 이 있다\n")
        .expect("합성 원본을 쓰지 못했다");
    std::fs::write(content.join("more.md"), "여기는 다른 장이다\n").expect("둘째 원본 실패");
    std::fs::write(
        content.join("en").join("hooks.md"),
        "translated omega-probe\n",
    )
    .expect("합성 번역을 쓰지 못했다");
    std::fs::write(content.join("notes.txt"), "md 가 아니다 sigma-decoy\n").expect("잡파일 실패");

    let fixture_floor = Floor {
        min: 2,
        measured: 2,
        measured_on: "2026-09-08",
        counted_on: tasty_doc_guards::floored_walk::CountedOn::SyntheticTree,
        why_this_gap: "합성 트리라 파일 수를 이 시험이 직접 정한다 — 간격이 0 인 것이 맞다.",
    };
    let text = guide_text_under(&content, &fixture_floor);
    assert!(text.contains("zeta-signal"), "원본 `.md` 를 안 읽었다");
    assert!(
        text.contains("다른 장"),
        "하위가 아닌 형제 `.md` 를 빠뜨렸다"
    );
    assert!(
        !text.contains("translated"),
        "영어 번역이 한국어 가이드 수집에 섞였다"
    );
    assert!(
        !text.contains("sigma-decoy"),
        "Markdown이 아닌 파일이 가이드에 포함됐다"
    );
}
