//! contributes 권한 표와 매니페스트 검증 코드의 게이트를 양방향으로 대조한다.
//! 문서만 바꾼 push도 검사하도록 경로 필터 없는 doc-guards 크레이트에 둔다(ADR-0048).
//! 런타임 열거 대신 manifest_text로 소스를 읽는다. 실제 ContributesGate::ALL과의 일치는
//! 루트의 tests/contributes_gate_readings_agree.rs에서 별도로 확인한다.

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

/// 셀의 첫 백틱 코드만 대조한다. 뒤 설명은 제외하되 토큰 접미사 오타는 놓치지 않도록 정확히 비교한다.
fn code_span(cell: &str) -> String {
    let mut parts = cell.split('`');
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

/// 한 문서 행이 여러 게이트와 일치하는 경우를 놓치지 않도록 총개수도 대조한다.
#[test]
fn the_doc_table_and_the_code_table_are_the_same_size() {
    assert_eq!(
        doc_rows(&read(DOC)).len(),
        code_gates().len(),
        "{DOC} 의 게이트 표 행 수와 코드 게이트 수가 다르다"
    );
}
