//! 권한 토큰을 **하나도** 요구하지 않고 plugin 이 부를 수 있는 IPC 메서드가
//! `docs/dev-guide/plugin-permissions.md` 의 군 표와 1:1 로 맞는지 검증한다.
//!
//! 그 문서의 주 표는 **토큰을 축으로** 하므로, 요구 토큰이 0개인 메서드는 어느 행에도
//! 나타나지 않는다. 즉 문서를 끝까지 읽어도 "아무 권한 없이도 부를 수 있는 것이 있다" 를
//! 알 수 없다 — 매니페스트 작성자에게 그 표가 정본이라, 표에 없는 개방 범위는 **열려
//! 있는데 문서에 없는 권한**이 된다.
//!
//! `METHOD_TABLE` 은 [`tasty_doc_guards::method_table`] 로 **텍스트에서** 읽는다. 한때
//! 런타임 열거였고 그 근거는 "파서가 없으니 파서가 틀릴 일도 없다" 였는데, 그 대가가
//! 이 가드의 **자동 채널**이었다 — `tasty_ipc` 를 링크하느라 본체 패키지에 살았고, 그
//! 유일한 자동 잡은 `paths-ignore: docs/**` 뒤에 있다. 이 가드의 입력은 문서 하나뿐이라
//! **위반될 수 있는 유일한 push 에서만 안 돌았다**(ADR-0138 과 같은 형태).
//!
//! 파서가 틀릴 위험은 없어지지 않고 **다른 곳으로 옮겨 붙박았다** —
//! `tests/method_table_readings_agree.rs`(본체 패키지)가 이 판독과 런타임 열거가 같은
//! 집합을 내는지 본다. 옮길 때 실측으로 확인했다: 양쪽 다 276 개, 차분 0.
//!
//! 양방향을 본다. 코드에만 있는 메서드(문서 누락)도, 문서에만 있는 메서드(삭제된 항목의
//! 잔재)도 잡는다.
//!
//! ## 이 가드가 보지 않는 것
//!
//! 메서드가 **어느 군에** 적혔는지는 보지 않는다 — 군은 "왜 토큰이 없나" 라는 근거의
//! 분류라 기계가 판정할 값이 아니다. 새 메서드를 추가할 때 군을 잘못 고르면 이 가드는
//! 통과한다. 근거가 갈리는 지점이라 사람이 봐야 한다.

use std::collections::BTreeSet;

const DOC: &str = "docs/dev-guide/plugin-permissions.md";

/// 표를 찾는 기준. 문서에 표가 여럿이라 헤더 행으로 특정한다.
const TABLE_HEADER: &str = "| 군 | 메서드 | 왜 토큰을 요구하지 않나 |";

/// 셀 안의 **모든 백틱 코드 스팬**. 메서드 열은 한 셀에 여럿을 `·` 로 늘어놓는다.
///
/// 코드 스팬만 뽑으므로 백틱 밖의 사람이 읽는 단서는 자연히 잘린다 — 그래서 대조를
/// `starts_with` 로 느슨하게 할 이유가 없고 **정확 일치**로 본다. 느슨하게 두면 문서 쪽
/// 접미사 오타(`attach.list_TYPO`)가 그대로 통과해, 이 가드의 존재 이유인 문서↔코드
/// drift 의 한 방향이 뚫린다.
fn code_spans(cell: &str) -> Vec<String> {
    cell.split('`')
        .skip(1)
        .step_by(2)
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

fn doc_text() -> String {
    let path = tasty_doc_guards::repo_root().join(DOC);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {DOC}: {e}"))
}

/// `METHOD_TABLE` 판독 — 메서드 → (plugin 이 부를 수 있으면) 요구 variant 목록.
fn method_table() -> std::collections::BTreeMap<String, Option<Vec<String>>> {
    let path = tasty_doc_guards::repo_root().join("crates/tasty-ipc/src/method_meta.rs");
    let src =
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    tasty_doc_guards::method_table(&src)
}

/// 표의 메서드 열에 적힌 메서드 이름을 **적힌 순서 그대로**(중복 포함) 모은다.
fn doc_methods(text: &str) -> Vec<String> {
    let after_header = text
        .split_once(TABLE_HEADER)
        .unwrap_or_else(|| panic!("{DOC}: `{TABLE_HEADER}` 표 헤더를 찾지 못했다"))
        .1;
    after_header
        .lines()
        .skip(1) // 헤더 바로 다음 줄은 `|---|---|---|` 구분선
        .take_while(|l| l.trim_start().starts_with('|'))
        .flat_map(|line| {
            let cells: Vec<&str> = line.trim().trim_matches('|').split('|').collect();
            if cells.len() != 3 {
                panic!("{DOC}: 군 표 행의 열 수가 3이 아니다: {line}");
            }
            if cells[0].trim().starts_with("---") {
                return Vec::new();
            }
            let methods = code_spans(cells[1]);
            assert!(
                !methods.is_empty(),
                "{DOC}: 군 표 행의 메서드 열이 비었다: {line}"
            );
            methods
        })
        .collect()
}

/// `plugin_callable` 이면서 요구 권한이 0개인 메서드 — 이 가드가 지키는 집합.
fn permission_free_methods() -> BTreeSet<String> {
    method_table()
        .into_iter()
        .filter_map(|(name, required)| match required {
            // `None` = plugin 이 못 부른다. `Some(v)` 에서 v 가 비면 토큰 0개로 열려 있다.
            Some(v) if v.is_empty() => Some(name),
            _ => None,
        })
        .collect()
}

#[test]
fn every_permission_free_method_is_documented() {
    let listed: BTreeSet<String> = doc_methods(&doc_text()).into_iter().collect();

    let missing: Vec<String> = permission_free_methods()
        .into_iter()
        .filter(|m| !listed.contains(m))
        .collect();
    assert!(
        missing.is_empty(),
        "요구 권한 0개로 plugin 에 열려 있는데 {DOC} 에 없는 메서드: {missing:?}\n\
         `method_meta.rs` 에 `plugin(&[])` 을 추가했으면 그 문서의 군 표에도 적어라 — \
         어느 군인지(근거가 무엇인지)까지 골라야 한다."
    );
}

#[test]
fn every_documented_method_is_permission_free() {
    let actual = permission_free_methods();

    let stray: Vec<String> = doc_methods(&doc_text())
        .into_iter()
        .filter(|m| !actual.contains(m.as_str()))
        .collect();
    assert!(
        stray.is_empty(),
        "{DOC} 의 군 표에 있으나 요구 권한 0개가 아닌(또는 없는) 메서드: {stray:?}\n\
         권한을 붙였거나 메서드를 없앴으면 표에서도 빼라."
    );
}

/// 본문이 **수를 적어두므로** 그 수도 함께 고정한다. 표는 맞는데 문장만 옛 수로 남는
/// drift 는 표 대조만으로는 잡히지 않고, 읽는 사람은 표를 세기 전에 그 수를 믿는다.
#[test]
fn the_prose_count_matches_the_table() {
    let text = doc_text();
    let n = permission_free_methods().len();
    let expected = format!("`required` 가 빈 {n}개다");
    assert!(
        text.contains(&expected),
        "{DOC}: 본문의 개수 표기가 실제({n}개)와 다르다 — \"{expected}\" 를 찾지 못했다"
    );
}

/// 위 두 테스트가 실제로 전단사를 보장하려면 목록에 중복이 없어야 한다 — 한 메서드를
/// 두 군에 적으면 근거가 둘이라는 뜻이라 그 자체가 결함이다.
#[test]
fn the_doc_list_has_no_duplicates() {
    let listed = doc_methods(&doc_text());
    let unique: BTreeSet<&String> = listed.iter().collect();
    assert_eq!(
        listed.len(),
        unique.len(),
        "{DOC} 의 군 표에 같은 메서드가 두 번 나온다: {listed:?}"
    );
    assert!(!listed.is_empty(), "{DOC}: 군 표가 비어 있다");
}
