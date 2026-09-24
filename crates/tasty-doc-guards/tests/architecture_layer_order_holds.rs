//! 아키텍처 문서의 계층 순서를 매니페스트 의존 방향과 대조한다.
//! 먼저 크레이트 열거가 완전하고 중복이 없는지 확인한다.
//! 순서 위반이 나오면 실제 의존 그래프와 문서 순서를 확인한다.
//! 의도한 예외만 문서의 근거와 함께 EXCEPTIONS에 등록한다.

use std::collections::BTreeMap;
use tasty_doc_guards::crate_layers::{internal_deps, inversions, sections};

const DOC: &str = "docs/architecture/index.md";

/// 문서가 본문에 이유와 함께 적은 예외 — `(소비자, 의존, 근거)`.
const EXCEPTIONS: &[(&str, &str, &str)] = &[
    (
        "tasty-file-format",
        "tasty-plugin-protocol",
        "형식 레지스트리 조회 trait은 tasty-plugin-protocol에 있고 구현은 레지스트리 타입을 소유한 크레이트에 둔다. 선택한 의존 방향의 근거는 ADR-0001과 docs/architecture/index.md의 도메인-IO 절에 있다.",
    ),
    (
        "tasty-file-handler",
        "tasty-plugin-protocol",
        "handler 레지스트리 조회 trait은 tasty-plugin-protocol에 있고 구현은 레지스트리 타입을 소유한 크레이트에 둔다. tasty-file-format과 같은 구조이며 ADR-0001과 docs/architecture/index.md의 도메인-IO 절에서 설명한다.",
    ),
    (
        "tasty-remote",
        "tasty-ipc",
        "원격 client가 IPC 호출을 사용한다. tasty-ssh나 tasty-ipc에 합치면 각각 불필요한 의존이 추가되므로 분리를 유지한다(ADR-0001, docs/architecture/index.md의 도메인-IO 절).",
    ),
];

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
        "계층 목록에서 빠진 크레이트다. 항목을 `이름`으로 시작해야 파서가 읽는다. 누락되면 해당 의존도 검사에서 빠진다:\n  {missing:?}"
    );
    let dup: Vec<(&&str, &Vec<&str>)> = seen.iter().filter(|(_, v)| v.len() > 1).collect();
    assert!(
        dup.is_empty(),
        "크레이트가 여러 계층에 중복 등록됐다:\n  {dup:?}"
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
        "의존 방향이 문서의 계층 순서를 거스른다. 실제 의존과 문서 순서를 대조해 잘못된 순서를 고친다. 의도한 예외라면 문서와 EXCEPTIONS에 근거를 남긴다:\n  {unlisted:?}"
    );

    // 더 이상 필요한 예외가 아닌 항목은 제거해야 이후 순서 위반을 잡을 수 있다.
    let stale: Vec<&str> = EXCEPTIONS
        .iter()
        .filter(|(c, d, _)| !found.iter().any(|(fc, fd)| fc == c && fd == d))
        .map(|(c, _, _)| *c)
        .collect();
    assert!(
        stale.is_empty(),
        "EXCEPTIONS에 있지만 현재 계층 순서를 거스르지 않는 항목이다. 이후 위반을 놓치지 않도록 오래된 예외를 제거한다:\n  {stale:?}"
    );
}
