//! `contributes` 권한 게이트 표가 코드와 문서에서 1:1 로 맞는지 검증한다.
//!
//! 게이트를 하나 늘리면 두 곳을 고쳐야 한다 — 매니페스트 검증 코드와
//! `docs/dev-guide/plugin-permissions.md` 의 표. 문서 쪽을 빠뜨려도 컴파일도 테스트도
//! 통과하므로 수동 대조로는 놓친다(실제로 `[[contributes.banner]]` 행이 표에서 빠진 채로
//! 유지된 적이 있다). 매니페스트 작성자에게는 그 표가 정본이라, 표에 없는 게이트는
//! **거부는 되는데 왜 거부되는지 문서에 없는 권한**이 된다.
//!
//! 선례인 `tests/permission_token_docs_parity.rs` 는 소스를 텍스트로 파싱해 토큰을 뽑지만,
//! 여기서는 그럴 필요가 없다 — 본 크레이트가 `tasty-plugin-manifest` 를 의존하므로
//! [`ContributesGate::ALL`] 을 **런타임에 열거**한다. 파서가 없으니 파서가 틀릴 일도 없다.
//!
//! 그래서 양방향을 다 본다. 문서에만 있는 행(삭제된 게이트의 잔재)도, 코드에만 있는
//! 게이트(문서 누락)도 잡는다.

use std::path::Path;

use tasty_plugin_manifest::ContributesGate;

const DOC: &str = "docs/dev-guide/plugin-permissions.md";

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

/// 게이트가 문서 행과 맞는가. 행은 (키, 토큰) 둘 다로 식별한다 — `ui.popup` 은 두 행에
/// 나오고 `[[contributes.detector]]` 도 두 행에 나오므로 한쪽만으로는 특정되지 않는다.
///
/// 둘 다 **정확 일치**다. 사람이 읽는 단서는 [`code_span`] 이 이미 잘라냈으므로 접두사
/// 비교로 느슨하게 둘 이유가 없다.
fn matches(gate: ContributesGate, row: &(String, String)) -> bool {
    row.0 == gate.contributes_key() && row.1 == gate.token().doc_form()
}

#[test]
fn every_code_gate_has_exactly_one_doc_row() {
    let text = std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join(DOC))
        .unwrap_or_else(|e| panic!("read {DOC}: {e}"));
    let rows = doc_rows(&text);

    let mut missing = Vec::new();
    for gate in ContributesGate::ALL {
        let hits = rows.iter().filter(|r| matches(*gate, r)).count();
        if hits != 1 {
            missing.push(format!(
                "  {} | {} → 문서 표에서 {hits} 행 매칭 (1 이어야 한다)",
                gate.contributes_key(),
                gate.token().doc_form()
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
    let text = std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join(DOC))
        .unwrap_or_else(|e| panic!("read {DOC}: {e}"));
    let rows = doc_rows(&text);
    assert!(!rows.is_empty(), "{DOC}: 게이트 표가 비어 있다");

    let stray: Vec<&(String, String)> = rows
        .iter()
        .filter(|row| !ContributesGate::ALL.iter().any(|g| matches(*g, row)))
        .collect();
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
    let text = std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join(DOC))
        .unwrap_or_else(|e| panic!("read {DOC}: {e}"));
    assert_eq!(
        doc_rows(&text).len(),
        ContributesGate::ALL.len(),
        "{DOC} 의 게이트 표 행 수와 ContributesGate::ALL 의 게이트 수가 다르다"
    );
}
