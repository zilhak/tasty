//! 락 poison 복구 코드에서 보고 호출이 빠진 곳을 찾는다.
//! 복구 정책은 docs/dev-guide/error-handling.md를 따르며, recover_* 헬퍼나 인라인 로그로 보고한다.
//! 테스트 코드는 cfg 범위와 테스트 전용 파일 판정으로 제외한다.
//!
//! 지원 형태는 unwrap_or_else의 복구 클로저, Err(p) 분기, TryLockError::Poisoned 분기다.
//! 인자로 받은 PoisonError나 PoisonError::into_inner 함수 포인터는 식별하지 못한다.
//! 검사 통과가 모든 복구 경로의 보고를 보장하는 것은 아니다.

use std::path::{Path, PathBuf};

use crate::cfg_predicate::cfg_gated_lines;
use crate::shipping_scope::test_only_files;
use crate::source_text::{mask_non_code, rust_sources};

/// 복구 블록에 보고 코드가 있는지 확인할 문자열.
const REPORT_TOKENS: &[&str] = &[
    "report(",
    "report_poison",
    "tracing::warn",
    "tracing::error",
    "warn!",
    "error!",
];

/// 한 파일을 분류한 결과. 줄 번호는 0 기반(into_inner 이 있는 줄).
#[derive(Debug, Default, PartialEq, Eq)]
pub struct FileClass {
    /// poison 복구로 판정된 `into_inner()` 자리 전부.
    pub poison_sites: Vec<usize>,
    /// 그중 `#[cfg(test)]` 가 덮는 줄.
    pub cfg_gated: Vec<usize>,
    /// 그중 복구 arm 에 보고가 있는 줄.
    pub reported: Vec<usize>,
    /// 그중 cfg 밖이고 보고가 없는 줄 — 조용한 복구.
    pub silent: Vec<usize>,
}

/// `head_line` 이 복구 헤드면 그 poison 바인더 이름을 돌려준다.
///
/// 세 형태: `unwrap_or_else(|B|` · (`=>` 를 낀) `Poisoned(B)` · (`=>` 를 낀) `Err(B)`.
/// `Err(TryLockError::Poisoned(p))` 는 `Poisoned(` 가 먼저 잡혀 안쪽 `p` 를 쓴다.
fn recovery_binder(line: &str) -> Option<String> {
    let ident_after = |s: &str, at: usize| -> Option<String> {
        let rest = &s[at..];
        let id: String = rest
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
            .collect();
        (!id.is_empty()).then_some(id)
    };

    // closure 헤드: `unwrap_or_else(|BINDER|` (타입 주석 `|p: PoisonError<..>|` 포함).
    if let Some(pos) = line.find("unwrap_or_else(|") {
        return ident_after(line, pos + "unwrap_or_else(|".len());
    }

    // match arm 헤드: 반드시 `=>` 가 같은 줄에 있어야 한다.
    if line.contains("=>") {
        if let Some(pos) = line.find("Poisoned(") {
            return ident_after(line, pos + "Poisoned(".len());
        }
        if let Some(pos) = line.find("Err(") {
            return ident_after(line, pos + "Err(".len());
        }
    }
    None
}

/// 가장 가까운 복구 선언을 찾는 최대 줄 수. 무관한 앞 분기를 함께 검사하지 않도록 제한한다.
const HEAD_LOOKBACK: usize = 25;

/// masked 소스 줄들과 그 파일의 `#[cfg(test)]` 마스크로 poison 복구를 분류한다.
///
/// 입력은 **`mask_non_code` 를 거친 줄**이어야 한다 — 주석·문자열 속 `into_inner` 언급을
/// 코드로 세지 않기 위해서다. `cfg_test` 는 [`cfg_gated_lines`] 의 결과로, 길이가 줄 수와
/// 같아야 한다.
pub fn classify(masked: &[&str], cfg_test: &[bool]) -> FileClass {
    assert_eq!(
        masked.len(),
        cfg_test.len(),
        "cfg 마스크 길이가 줄 수와 다르다 — 판정 좌표가 어긋난다"
    );
    let mut out = FileClass::default();
    for idx in 0..masked.len() {
        if !masked[idx].contains(".into_inner()") {
            continue;
        }
        // 가장 가까운 복구 헤드와 바인더를 찾는다.
        let lo = idx.saturating_sub(HEAD_LOOKBACK);
        let Some((head, binder)) = (lo..=idx)
            .rev()
            .find_map(|j| recovery_binder(masked[j]).map(|b| (j, b)))
        else {
            continue; // 복구 헤드 없음 → poison 복구가 아니다(헬퍼 인자형·비-poison).
        };
        // 이 줄의 into_inner 가 그 바인더에 걸리는가.
        if !masked[idx].contains(&format!("{binder}.into_inner()")) {
            continue;
        }
        out.poison_sites.push(idx);
        if cfg_test[idx] {
            out.cfg_gated.push(idx);
            continue;
        }
        let reports = (head..=idx).any(|j| REPORT_TOKENS.iter().any(|t| masked[j].contains(t)));
        if reports {
            out.reported.push(idx);
        } else {
            out.silent.push(idx);
        }
    }
    out
}

/// 제품 코드의 보고 없는 poison 복구를 모은 결과.
#[derive(Debug, Default)]
pub struct Census {
    pub files_scanned: usize,
    pub poison_sites: usize,
    pub cfg_gated: usize,
    pub test_only_sites: usize,
    pub reported: usize,
    /// `"레포상대경로:1기반줄: 원문"` 형태의 위반 목록.
    pub silent: Vec<String>,
}

/// scan_roots 아래의 코드를 검사한다.
pub fn census(root: &Path, scan_roots: &[&str]) -> Census {
    let sources = rust_sources(root, scan_roots);
    let test_only: std::collections::BTreeSet<PathBuf> = test_only_files(root, &sources);

    let mut c = Census::default();
    for (rel, raw) in &sources {
        c.files_scanned += 1;
        let masked_src = mask_non_code(raw);
        let masked: Vec<&str> = masked_src.lines().collect();
        let raw_lines: Vec<&str> = raw.lines().collect();
        let cfg_test = cfg_gated_lines(&masked, "test");
        let fc = classify(&masked, &cfg_test);

        c.poison_sites += fc.poison_sites.len();
        c.cfg_gated += fc.cfg_gated.len();
        c.reported += fc.reported.len();

        let file_is_test_only = test_only.contains(rel);
        if file_is_test_only {
            // 파일 통째가 test-only 면 그 조용한 복구는 위반이 아니다(테스트 코드).
            c.test_only_sites += fc.silent.len();
            continue;
        }
        for &idx in &fc.silent {
            let text = raw_lines.get(idx).map(|s| s.trim()).unwrap_or("");
            c.silent
                .push(format!("{}:{}: {text}", rel.display(), idx + 1));
        }
    }
    c
}

#[cfg(test)]
mod tests {
    use super::*;

    fn classify_src(src: &str) -> FileClass {
        let masked_src = mask_non_code(src);
        let masked: Vec<&str> = masked_src.lines().collect();
        let cfg_test = cfg_gated_lines(&masked, "test");
        classify(&masked, &cfg_test)
    }

    #[test]
    fn a_bare_unwrap_or_else_closure_is_silent() {
        let fc = classify_src("fn f() { let g = m.lock().unwrap_or_else(|p| p.into_inner()); }");
        assert_eq!(fc.poison_sites.len(), 1);
        assert_eq!(
            fc.silent.len(),
            1,
            "보고 없는 closure 복구를 조용으로 잡아야 한다"
        );
    }

    #[test]
    fn a_bare_err_arm_is_silent() {
        let fc = classify_src(
            "fn f() {\n    let g = match m.lock() {\n        Ok(g) => g,\n        Err(p) => p.into_inner(),\n    };\n}",
        );
        assert_eq!(fc.silent.len(), 1);
    }

    #[test]
    fn a_try_lock_poisoned_arm_is_silent() {
        let fc = classify_src(
            "fn f() {\n    match m.try_write() {\n        Ok(g) => g,\n        Err(TryLockError::Poisoned(p)) => p.into_inner(),\n        Err(TryLockError::WouldBlock) => return,\n    };\n}",
        );
        assert_eq!(fc.silent.len(), 1, "Poisoned(p) 의 안쪽 p 로 잡아야 한다");
    }

    #[test]
    fn a_closure_with_a_report_is_not_silent() {
        let fc = classify_src(
            "fn f() {\n    m.lock().unwrap_or_else(|poisoned| {\n        tracing::error!(\"x poisoned\");\n        poisoned.into_inner()\n    });\n}",
        );
        assert_eq!(fc.poison_sites.len(), 1);
        assert!(fc.silent.is_empty(), "보고가 있으니 조용이 아니다");
        assert_eq!(fc.reported.len(), 1);
    }

    #[test]
    fn a_multiline_logged_arm_is_reported() {
        let fc = classify_src(
            "fn f() {\n    match m.lock() {\n        Ok(g) => g,\n        Err(poisoned) => {\n            tracing::warn!(\n                \"long message spanning\\\n                 two lines\"\n            );\n            poisoned.into_inner()\n        }\n    };\n}",
        );
        assert!(fc.silent.is_empty());
        assert_eq!(fc.reported.len(), 1);
    }

    #[test]
    fn a_silent_recovery_under_cfg_test_is_gated_not_silent() {
        let fc = classify_src(
            "pub fn ship() {}\n\n#[cfg(test)]\nmod t {\n    #[test]\n    fn x() {\n        let _g = m.lock().unwrap_or_else(|p| p.into_inner());\n    }\n}",
        );
        assert_eq!(fc.poison_sites.len(), 1);
        assert_eq!(fc.cfg_gated.len(), 1, "cfg(test) 안이라 gated 여야 한다");
        assert!(fc.silent.is_empty());
    }

    #[test]
    fn a_non_poison_into_inner_is_not_counted() {
        let fc = classify_src(
            "fn f() {\n    let v = cell.into_inner();\n    for x in staged.into_inner() {}\n}",
        );
        assert!(
            fc.poison_sites.is_empty(),
            "락 복구가 아닌 into_inner 는 제외"
        );
    }

    #[test]
    fn a_mention_in_a_comment_is_not_counted() {
        let fc = classify_src("/// 이 자리를 into_inner() 로 되돌린 변이가 살아남았다.\nfn f() {}");
        assert!(fc.poison_sites.is_empty());
    }

    /// 인자로 받은 PoisonError는 복구 선언을 찾지 못하므로 이 검사에서 제외된다.
    #[test]
    fn a_param_binder_poison_into_inner_is_out_of_scope() {
        let fc = classify_src(
            "fn recover<T>(poisoned: PoisonError<T>) -> T {\n    report();\n    poisoned.into_inner()\n}",
        );
        assert!(fc.poison_sites.is_empty());
    }
}
