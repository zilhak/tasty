//! IPC 핸들러의 debug 전용 항목이 일반 핸들러 파일에 섞이지 않도록 확인한다.
//! 모듈 선언이나 파일 전체에 debug 조건이 있으면 허용한다. release 전용 항목은 대상이 아니다.
//! 규칙은 [debug-ipc](../../docs/dev-guide/debug-ipc.md)의 디버그 코드 격리 정책을 따른다.
//!
//! 핸들러 디렉터리만 검사하므로 view 입력과 라우터의 debug 분기는 범위 밖이다.
//! 한 줄 cfg·mod 선언을 텍스트로 읽으며 매크로를 전개하지 않는다. 매크로 인자로 전달한 모듈 이름은
//! 선언으로 읽지 못해 정상 코드도 실패할 수 있다. 파일 수 하한으로는 이런 오독을 찾을 수 없다.

use std::collections::BTreeMap;

use tasty_doc_guards::cfg_predicate::implies;

use super::{mask_non_code, repo_root};

const DISPATCH: &str = "src/adapters/ipc/handler.rs";
const HANDLER_DIR: &str = "src/adapters/ipc/handler";

/// 2026-09-05 핸들러 파일 45개를 측정한 뒤 수집 누락을 찾도록 둔 하한이다.
const MIN_HANDLER_FILES: usize = 30;

fn declared_debug_gated(dispatch: &str) -> BTreeMap<String, bool> {
    let masked = mask_non_code(dispatch);
    let mut out = BTreeMap::new();
    let mut pending: Option<bool> = None;
    for line in masked.lines() {
        let t = line.trim();
        if t.starts_with("#[cfg(") {
            // 같은 선언의 여러 cfg는 AND로 결합되므로 하나가 debug를 요구하면 전체도 debug 전용이다.
            let gated = cfg_pred(t).is_some_and(|pred| implies(pred, "debug_assertions"));
            pending = Some(pending.unwrap_or(false) || gated);
            continue;
        }
        if let Some(name) = module_decl(t) {
            out.insert(name, pending.take().unwrap_or(false));
            continue;
        }
        if !t.is_empty() {
            pending = None;
        }
    }
    out
}

/// 파일 모듈 선언만 읽는다. 인라인 모듈은 제외한다.
fn module_decl(line: &str) -> Option<String> {
    let rest = line.strip_suffix(';')?;
    let rest = rest.strip_prefix("pub(crate) ").unwrap_or(rest);
    let rest = rest.strip_prefix("pub ").unwrap_or(rest);
    let name = rest.strip_prefix("mod ")?.trim();
    if name.is_empty()
        || !name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
    {
        return None;
    }
    Some(name.to_string())
}

/// 항목의 debug 전용 cfg 줄 번호. 파일 전체 조건과 release 전용 조건은 제외한다.
fn debug_only_items(masked: &str) -> Vec<usize> {
    masked
        .lines()
        .enumerate()
        .filter(|(_, l)| {
            let t = l.trim();
            t.starts_with("#[cfg(")
                && cfg_pred(t).is_some_and(|pred| implies(pred, "debug_assertions"))
        })
        .map(|(i, _)| i + 1)
        .collect()
}

fn file_level_debug(masked: &str) -> bool {
    masked.lines().any(|l| {
        let t = l.trim();
        t.starts_with("#![cfg(")
            && cfg_pred(t).is_some_and(|pred| implies(pred, "debug_assertions"))
    })
}

/// 조건 문자열에 debug_assertions가 포함됐는지만 보면 any·not을 오판하므로 implies에서 논리식을 판단한다.
fn cfg_pred(attr: &str) -> Option<&str> {
    let t = attr.trim();
    t.strip_prefix("#[cfg(")
        .or_else(|| t.strip_prefix("#![cfg("))
        .and_then(|s| s.strip_suffix(")]"))
}

#[test]
fn a_debug_handler_is_isolated_by_its_declaration_not_its_name() {
    let root = repo_root();
    let dispatch = std::fs::read_to_string(root.join(DISPATCH))
        .unwrap_or_else(|e| panic!("{DISPATCH} 를 읽을 수 없다: {e}"));
    let gated = declared_debug_gated(&dispatch);

    let dir = root.join(HANDLER_DIR);
    let mut scanned = 0usize;
    let mut violations: Vec<String> = Vec::new();
    for entry in std::fs::read_dir(&dir).expect("핸들러 디렉토리를 읽어야 한다") {
        let path = entry.expect("디렉토리 항목").path();
        if path.extension().is_none_or(|e| e != "rs") {
            continue;
        }
        let stem = path
            .file_stem()
            .and_then(|s| s.to_str())
            .expect("파일 이름")
            .to_string();
        let src = std::fs::read_to_string(&path).expect("핸들러 파일을 읽어야 한다");
        let masked = mask_non_code(&src);
        scanned += 1;

        if *gated.get(&stem).unwrap_or(&false) || file_level_debug(&masked) {
            continue;
        }
        let lines = debug_only_items(&masked);
        if !lines.is_empty() {
            violations.push(format!("{HANDLER_DIR}/{stem}.rs: {lines:?}"));
        }
    }

    assert!(
        scanned >= MIN_HANDLER_FILES,
        "핸들러 파일을 {scanned}개만 읽었다(하한 {MIN_HANDLER_FILES}, 2026-09-05 측정 45개). 실제 파일 수와 스캔 경로를 확인한다."
    );
    assert!(
        violations.is_empty(),
        "일반 핸들러 파일에 debug 전용 항목이 있다. debug 기능을 파일 단위로 제거할 수 있도록 별도 모듈로 옮기고 파일 또는 모듈 선언에 cfg를 지정한다(docs/dev-guide/debug-ipc.md의 디버그 코드 격리 정책): {violations:?}"
    );
}

#[cfg(test)]
mod detector {
    use super::*;

    #[test]
    fn it_reads_the_gate_from_the_declaration() {
        let d = declared_debug_gated(
            "mod plain;\n\
             #[cfg(debug_assertions)]\n\
             mod dbg;\n\
             #[cfg(all(debug_assertions, feature = \"gui\"))]\n\
             pub mod dbg_gui;\n\
             #[cfg(feature = \"gui\")]\n\
             pub(crate) mod gui_only;\n",
        );
        assert_eq!(d.get("plain"), Some(&false));
        assert_eq!(d.get("dbg"), Some(&true));
        assert_eq!(d.get("dbg_gui"), Some(&true));
        assert_eq!(d.get("gui_only"), Some(&false));
    }

    #[test]
    fn an_any_cfg_does_not_imply_debug() {
        let d = declared_debug_gated(
            "#[cfg(any(debug_assertions, feature = \"gui\"))]\npub mod maybe;\n",
        );
        assert_eq!(d.get("maybe"), Some(&false));
        assert!(
            debug_only_items("#[cfg(any(debug_assertions, feature = \"gui\"))]\nfn f() {}")
                .is_empty()
        );
    }

    #[test]
    fn a_release_only_attribute_is_not_a_debug_item() {
        assert!(debug_only_items("#[cfg(not(debug_assertions))]\nfn f() {}").is_empty());
        assert_eq!(
            debug_only_items("#[cfg(debug_assertions)]\nfn f() {}"),
            vec![1]
        );
    }

    #[test]
    fn a_file_level_attribute_is_not_an_item() {
        let src = "#![cfg(debug_assertions)]\nfn f() {}";
        assert!(file_level_debug(src));
        assert!(debug_only_items(src).is_empty());
    }

    #[test]
    fn the_scan_separates_code_from_comments_and_literals() {
        let masked = mask_non_code(
            "// #[cfg(debug_assertions)]\n\
             let s = \"#[cfg(debug_assertions)]\";\n\
             #[cfg(debug_assertions)]\n",
        );
        assert_eq!(debug_only_items(&masked), vec![3]);
    }

    #[test]
    fn an_inline_module_is_not_a_file_declaration() {
        assert_eq!(module_decl("mod x {"), None);
        assert_eq!(module_decl("mod x;"), Some("x".to_string()));
    }
}
