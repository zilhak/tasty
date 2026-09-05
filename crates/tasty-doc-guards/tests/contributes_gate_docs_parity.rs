//! `contributes` 권한 게이트 표가 코드와 문서에서 1:1 로 맞는지 검증한다.
//!
//! 게이트를 하나 늘리면 두 곳을 고쳐야 한다 — 매니페스트 검증 코드와
//! `docs/dev-guide/plugin-permissions.md` 의 표. 문서 쪽을 빠뜨려도 컴파일도 테스트도
//! 통과하므로 수동 대조로는 놓친다(실제로 `[[contributes.banner]]` 행이 표에서 빠진 채로
//! 유지된 적이 있다). 매니페스트 작성자에게는 그 표가 정본이라, 표에 없는 게이트는
//! **거부는 되는데 왜 거부되는지 문서에 없는 권한**이 된다.
//!
//! **이 크레이트에 사는 이유는 채널이다.** 이 가드의 입력은 전부 `docs/**` 인데,
//! 본체 패키지의 통합 테스트를 돌리는 `check-headless` 는 push 트리거에
//! `paths-ignore`(`docs/**` · `site/**` · `**/*.md`)가 걸려 있다 — **그 문서만 고치는
//! push 에서 정확히 안 도는** 형태였다. 실측(2026-09-05, 연속 push 30 구간): 전부 무시
//! 대상 경로인 push 가 2 건이었다. 여기 doc-guards 는 경로 필터가 없다(ADR-0138).
//!
//! 대가로 `ContributesGate::ALL` 을 런타임에 열거하지 못하고 표를 텍스트로 읽는다
//! (`tasty_doc_guards::manifest_text`). 판독이 진짜 표와 갈리는 위험은 본체 패키지의
//! `tests/contributes_gate_readings_agree.rs` 가 받는다 — 거기서는 링크해 열거할 수 있다.
//!
//! 그래서 양방향을 다 본다. 문서에만 있는 행(삭제된 게이트의 잔재)도, 코드에만 있는
//! 게이트(문서 누락)도 잡는다.

const DOC: &str = "docs/dev-guide/plugin-permissions.md";

fn read(rel: &str) -> String {
    let p = tasty_doc_guards::repo_root().join(rel);
    std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("read {}: {e}", p.display()))
}

/// 코드 쪽 게이트 표 — (매니페스트 키, 문서 표기).
fn code_gates() -> Vec<(String, String)> {
    tasty_doc_guards::manifest_text::contributes_gates(
        &read("crates/tasty-plugin-manifest/src/gates.rs"),
        &read("crates/tasty-plugin-manifest/src/types.rs"),
    )
}

/// 표를 찾는 기준. 문서에 표가 여럿이라 헤더 행으로 특정한다.
const TABLE_HEADER: &str = "| contributes | 요구 권한 |";

/// 셀에서 **첫 백틱 코드 스팬**을 뽑는다.
///
/// 표의 셀은 `` `[[contributes.commands]]` (`action.kind = "open_popup"`) `` 나
/// `` `ui.settings_page` (카테고리 무관) `` 처럼 토큰 뒤에 사람이 읽는 단서가 붙는다.
/// 그 단서는 **백틱 밖**에 있으므로 코드 스팬만 뽑으면 자연히 잘린다 — 그래서 대조를
/// `starts_with` 로 느슨하게 할 이유가 없고 **정확 일치**로 본다.
///
/// 느슨하게 두면 문서 쪽 토큰의 접미사 오타(`ui.tool_item_TYPO`)가 그대로 통과한다 —
/// 문서↔코드 drift 를 잡는 것이 이 가드의 존재 이유인데 그 한 방향이 뚫린다.
fn code_span(cell: &str) -> String {
    let mut parts = cell.split('`');
    // split 결과: [백틱 앞, 코드 스팬, 백틱 뒤, …]
    parts.next();
    match parts.next() {
        Some(span) if !span.trim().is_empty() => span.trim().to_string(),
        _ => panic!("{DOC}: 게이트 표의 셀에 백틱 코드 스팬이 없다: {cell:?}"),
    }
}

/// 문서에서 게이트 표의 (contributes, 요구 권한) 행을 읽어온다.
fn doc_rows(text: &str) -> Vec<(String, String)> {
    let after_header = text
        .split_once(TABLE_HEADER)
        .unwrap_or_else(|| panic!("{DOC}: `{TABLE_HEADER}` 표 헤더를 찾지 못했다"))
        .1;
    after_header
        .lines()
        .skip(1) // 헤더 바로 다음 줄은 `|---|---|` 구분선
        .take_while(|l| l.trim_start().starts_with('|'))
        .filter_map(|line| {
            let cells: Vec<&str> = line.trim().trim_matches('|').split('|').collect();
            if cells.len() != 2 {
                panic!("{DOC}: 게이트 표 행의 열 수가 2가 아니다: {line}");
            }
            // 구분선(`---`)이 섞여 들어오면 코드 스팬을 찾기 전에 걸러낸다.
            if cells[0].trim().starts_with("---") {
                return None;
            }
            Some((code_span(cells[0]), code_span(cells[1])))
        })
        .collect()
}

#[test]
fn every_code_gate_has_exactly_one_doc_row() {
    let rows = doc_rows(&read(DOC));

    let mut missing = Vec::new();
    for gate in code_gates() {
        let hits = rows.iter().filter(|r| **r == gate).count();
        if hits != 1 {
            missing.push(format!(
                "  {} | {} → 문서 표에서 {hits} 행 매칭 (1 이어야 한다)",
                gate.0, gate.1
            ));
        }
    }
    assert!(
        missing.is_empty(),
        "코드의 contributes 게이트가 {DOC} 의 표와 어긋난다:\n{}\n\
         게이트를 추가/변경했으면 그 표의 행도 함께 고쳐라.",
        missing.join("\n")
    );
}

#[test]
fn every_doc_row_has_a_code_gate() {
    let rows = doc_rows(&read(DOC));
    assert!(!rows.is_empty(), "{DOC}: 게이트 표가 비어 있다");

    let gates = code_gates();
    let stray: Vec<&(String, String)> = rows.iter().filter(|row| !gates.contains(row)).collect();
    assert!(
        stray.is_empty(),
        "{DOC} 의 표에 코드 게이트가 없는 행이 있다: {stray:?}\n\
         게이트를 없앴으면 표에서도 빼라."
    );
}

/// 행 수와 게이트 수가 같아야 위 두 테스트가 실제로 전단사를 보장한다 — 한쪽만으로는
/// 한 행이 두 게이트에 매칭되는 경우를 못 잡는다.
#[test]
fn the_doc_table_and_the_code_table_are_the_same_size() {
    assert_eq!(
        doc_rows(&read(DOC)).len(),
        code_gates().len(),
        "{DOC} 의 게이트 표 행 수와 코드 게이트 수가 다르다"
    );
}
