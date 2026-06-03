//! `docs/dev-guide/cli-naming.md` 의 host namespace 메서드 수 표가 실제
//! METHOD_TABLE 카운트와 일치하는지 검증한다. drift 발생 시 fail.
//!
//! 본 표는 0.7.x SemVer 가드 의 docs 측면 — 메서드가 추가되거나 (0.7.x 내
//! 허용) 의도치 않게 제거되었을 때 docs 갱신을 강제한다.

use std::collections::BTreeMap;

use tasty_ipc::method_meta::METHOD_TABLE;

const CLI_NAMING_PATH: &str = "docs/dev-guide/cli-naming.md";
const TABLE_BEGIN: &str = "<!-- count-table:host-namespaces -->";
const TABLE_END: &str = "<!-- /count-table:host-namespaces -->";

#[test]
fn cli_naming_namespace_counts_match_method_table() {
    let doc = std::fs::read_to_string(CLI_NAMING_PATH)
        .unwrap_or_else(|e| panic!("read {CLI_NAMING_PATH}: {e}"));
    let documented = parse_count_table(&doc);
    let actual = actual_namespace_counts();

    let mut errors = Vec::new();

    for (ns, count) in &actual {
        match documented.get(ns) {
            Some(d) if d == count => {}
            Some(d) => errors.push(format!("namespace `{ns}`: docs={d}, METHOD_TABLE={count}")),
            None => errors.push(format!(
                "namespace `{ns}` (METHOD_TABLE={count}) 가 cli-naming.md 표에 누락"
            )),
        }
    }
    for ns in documented.keys() {
        if !actual.contains_key(ns) {
            errors.push(format!(
                "namespace `{ns}` 가 cli-naming.md 표에 있지만 METHOD_TABLE 에 없음"
            ));
        }
    }

    assert!(
        errors.is_empty(),
        "cli-naming.md 의 host namespace count 표가 METHOD_TABLE 과 drift:\n  {}\n\
         갱신: 메서드 추가/제거 후 docs/dev-guide/cli-naming.md 의 \
         `<!-- count-table:host-namespaces -->` 표를 동기화.",
        errors.join("\n  ")
    );
}

fn parse_count_table(doc: &str) -> BTreeMap<String, usize> {
    let begin = doc
        .find(TABLE_BEGIN)
        .unwrap_or_else(|| panic!("{CLI_NAMING_PATH}: missing marker `{TABLE_BEGIN}`"));
    let end = doc
        .find(TABLE_END)
        .unwrap_or_else(|| panic!("{CLI_NAMING_PATH}: missing marker `{TABLE_END}`"));
    assert!(begin < end, "table markers out of order");

    let mut out = BTreeMap::new();
    for line in doc[begin..end].lines() {
        let line = line.trim();
        if !line.starts_with('|') {
            continue;
        }
        let cells: Vec<&str> = line
            .trim_matches('|')
            .split('|')
            .map(|s| s.trim())
            .collect();
        if cells.len() < 2 {
            continue;
        }
        let ns_cell = cells[0].trim_matches('`');
        let count_cell = cells[1];
        let Ok(count) = count_cell.parse::<usize>() else {
            continue;
        };
        out.insert(ns_cell.to_string(), count);
    }
    out
}

fn actual_namespace_counts() -> BTreeMap<String, usize> {
    let mut out: BTreeMap<String, usize> = BTreeMap::new();
    for (name, _) in METHOD_TABLE {
        if let Some((ns, _)) = name.split_once('.') {
            *out.entry(ns.to_string()).or_insert(0) += 1;
        }
    }
    out
}

#[cfg(test)]
mod helper_tests {
    use super::*;

    #[test]
    fn parse_count_table_basic() {
        let doc = format!(
            "before\n{TABLE_BEGIN}\n| ns | count |\n|----|-------|\n| `foo` | 3 |\n| `bar` | 12 |\n{TABLE_END}\nafter"
        );
        let parsed = parse_count_table(&doc);
        assert_eq!(parsed.get("foo"), Some(&3));
        assert_eq!(parsed.get("bar"), Some(&12));
        assert_eq!(parsed.len(), 2);
    }

    #[test]
    fn actual_counts_has_known_namespaces() {
        let counts = actual_namespace_counts();
        assert!(counts.contains_key("memory"));
        assert!(counts.contains_key("agent"));
        assert!(counts.contains_key("surface"));
    }
}
