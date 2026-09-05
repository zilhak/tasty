//! `docs/architecture/index.md` 가 주장하는 **절 순서**가 매니페스트 의존 방향과 맞는지
//! 본다.
//!
//! 그 문서는 절을 순서대로 늘어놓고 "의존은 아래 계층 순서로만 흐른다(상위 → 하위)" 고
//! 적는다. 그 주장에 못이 없던 동안 순서가 낡았다 — 실측: 절 넷이 잘못된 자리에 있어
//! **정상 의존 넷이 위반처럼 보였고**, 문서에 예외로 적힌 것은 하나뿐이었다. 그 상태의
//! 대가는 다음 사람이 "규칙이 틀렸나 코드가 틀렸나" 를 처음부터 다시 재는 것이다.
//!
//! **명부가 아니라 절 정의를 고치는 것이 처방이다.** 그래서 [`EXCEPTIONS`] 는 문서가
//! 본문에 이유와 함께 적은 것만 담는다 — 이름을 적는 순간 그 간선은 "쓰이는 것" 으로
//! 세어지므로, 여기 적는 것은 "고칠 수 없다" 가 아니라 **"고치지 않기로 했고 그 근거가
//! 어디 있다"** 는 뜻이다.
//!
//! 판정 로직과 그 판별력(위→아래는 통과)은 [`tasty_doc_guards::crate_layers`] 에 있다.
//!
//! ## 이 게이트가 빨개졌을 때 **쓰면 안 되는 논거**
//!
//! "절이 층이 아니라 역할로 묶여서 순서 축과 직교한다" 는 진단이 한 번 나왔다가
//! 철회됐다. 근거로 든 것은 **깊이 분포**였다 — 절마다 의존 그래프의 최장 경로가
//! 여러 값에 걸친다(도메인-IO 는 0·1·2·3·5). 그건 증거가 아니다: **계층은 깊이가
//! 균일할 필요가 없고, 위로 가는 간선만이 위반이다.**
//!
//! 실제로는 그 예외 하나를 뺀 절 그래프에 **순환이 없었다**. 즉 절들은 이미 유효한
//! 계층이었고 목록 순서만 틀렸으며, 처방은 두 줄이었다(sandbox 경계 ↔ plugin host 를
//! 맞바꾸고, 도구/standalone 을 CLI 앞으로). 틀린 술어가 없는 병을 만들었고 거기에
//! 큰 처방 셋이 붙었다가 접혔다.
//!
//! 그러니 이 가드가 빨개지면 **절 그래프에 순환이 있는지부터** 보라. 없으면 순서를
//! 다시 매기면 되고, 그때 소속은 하나도 안 옮겨도 된다.
//!
//! 자매 가드: `architecture_crate_list_complete.rs` 는 **다른 것**을 본다 — 이름이 문서
//! 어딘가에 등장하는지와 개수다. 여기는 **어느 절에 열거됐는지**를 본다.

use std::collections::BTreeMap;
use tasty_doc_guards::crate_layers::{internal_deps, inversions, sections};

const DOC: &str = "docs/architecture/index.md";

/// 문서가 본문에 이유와 함께 적은 예외 — `(소비자, 의존, 근거)`.
const EXCEPTIONS: &[(&str, &str, &str)] = &[(
    "tasty-remote",
    "tasty-ipc",
    "원격 client 능력이 IPC 호출이고 합칠 후보 둘이 각각 더 나쁜 의존을 들인다 (ADR-0089). \
     docs/architecture/index.md 의 도메인-IO 절이 본문에 적는다.",
)];

fn crate_names(root: &std::path::Path) -> Vec<String> {
    let mut v: Vec<String> = std::fs::read_dir(root.join("crates"))
        .expect("crates/ 를 못 읽었다")
        .flatten()
        .filter(|e| e.path().join("Cargo.toml").is_file())
        .filter_map(|e| e.file_name().into_string().ok())
        .collect();
    v.sort();
    v
}

#[test]
fn every_crate_is_enumerated_in_exactly_one_section() {
    let root = tasty_doc_guards::repo_root();
    let names = crate_names(&root);
    let doc = std::fs::read_to_string(root.join(DOC)).expect("아키텍처 문서를 못 읽었다");
    let secs = sections(&doc, &names);

    let mut seen: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for s in &secs {
        for c in &s.crates {
            seen.entry(c).or_default().push(&s.name);
        }
    }
    let missing: Vec<&String> = names
        .iter()
        .filter(|n| !seen.contains_key(n.as_str()))
        .collect();
    assert!(
        missing.is_empty(),
        "절 열거에 없는 크레이트다 — 열거는 항목마다 `` `이름` `` 으로 **시작**해야 읽힌다. \
         빠지면 그 크레이트의 의존이 순서 검사에서 통째로 안 세어진다:\n  {missing:?}"
    );
    let dup: Vec<(&&str, &Vec<&str>)> = seen.iter().filter(|(_, v)| v.len() > 1).collect();
    assert!(
        dup.is_empty(),
        "한 크레이트가 두 절에 열거됐다 — 소속이 갈리면 순서 판정이 어느 쪽이든 되어버린다:\n  {dup:?}"
    );
}

#[test]
fn dependencies_only_flow_down_the_section_order() {
    let root = tasty_doc_guards::repo_root();
    let names = crate_names(&root);
    let doc = std::fs::read_to_string(root.join(DOC)).expect("아키텍처 문서를 못 읽었다");
    let secs = sections(&doc, &names);

    let deps: BTreeMap<String, Vec<String>> = names
        .iter()
        .map(|n| {
            let m = std::fs::read_to_string(root.join("crates").join(n).join("Cargo.toml"))
                .unwrap_or_else(|e| panic!("{n}/Cargo.toml: {e}"));
            (n.clone(), internal_deps(&m, &names))
        })
        .collect();

    let found = inversions(&secs, &deps);
    let unlisted: Vec<&(String, String)> = found
        .iter()
        .filter(|(c, d)| !EXCEPTIONS.iter().any(|(ec, ed, _)| ec == c && ed == d))
        .collect();
    assert!(
        unlisted.is_empty(),
        "의존이 절 순서를 거슬러 올라간다 — 문서의 \"아래 계층 순서로만 흐른다\" 가 거짓이 \
         된다.\n**먼저 의심할 것은 절 순서지 이 간선이 아니다**: 실측에서 이런 넷은 전부 \
         정상 의존이었고 절이 잘못된 자리에 있었다. 절을 옮겨서 풀리면 그렇게 하고, \
         정말 예외면 문서 본문에 이유를 적고 EXCEPTIONS 에 근거와 함께 등록해라.\n  \
         {unlisted:?}"
    );

    // 역방향 — 명부가 실제보다 넓으면 그 간선이 다시 생겨도 통과한다.
    let stale: Vec<&str> = EXCEPTIONS
        .iter()
        .filter(|(c, d, _)| !found.iter().any(|(fc, fd)| fc == c && fd == d))
        .map(|(c, _, _)| *c)
        .collect();
    assert!(
        stale.is_empty(),
        "EXCEPTIONS 에 있으나 실제로는 순서를 안 거스른다 — 절을 옮겨 풀렸으면 명부에서도 \
         지워라(남겨두면 그 간선이 다시 위반이 돼도 통과한다):\n  {stale:?}"
    );
}
