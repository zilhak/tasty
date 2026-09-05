//! **락 poison 을 보고 없이 복구하는 자리**를 소스에서 집는다.
//!
//! 방침은 저장소의 `docs/dev-guide/error-handling.md` "락 poison" 절 ②축이다 — 임계구역이
//! 불변식을 깨진 채 남기지 않아 복구하기로 했다면, 그 복구는 **첫 1 회라도 보고**해야
//! 한다. 조용한 복구는 조용한 유실과 구분되지 않기 때문이다. `tasty_utils::poison` 의
//! `recover_*` 헬퍼가 그 보고를 태우고, 헬퍼가 안 닿는 자리는 인라인으로 보고한다.
//!
//! ## 술어는 성질이다 — 허용 목록이 아니다
//!
//! 이 가드는 "헬퍼를 거쳤는가" 를 묻지 않는다(그건 목록으로 관리하는 허용 방식이라
//! 자기보고하는 자리를 일일이 면제해야 한다). 대신 **성질**을 묻는다: poison 을
//! 복구하는 `into_inner()` 인데 그 복구 arm/closure 안에 보고가 없는가. 그래서
//! 자기보고하는 자리(헬퍼 자신 · `event_bus` · plugin-sdk runtime · cli attach ·
//! agent-stream pump/handlers · approval)는 **면제 없이** 통과한다 — 보고가 있으니까.
//!
//! ## cfg(test) 는 목록이 아니라 구조로 빠진다
//!
//! 테스트 코드의 조용한 복구는 프로덕션 유실을 숨기지 않는다. 그것을 빼는 근거는
//! 이름이 아니라 **소스 자신의 `#[cfg(test)]`** 다 — [`crate::cfg_predicate::cfg_gated_lines`]
//! 가 줄 단위로, [`crate::shipping_scope::test_only_files`] 가 파일 단위로 판정한다.
//! 실측(2026-09-05): shipping 파일 안 `#[cfg(test)]` 블록에 조용한 복구가 스물 남짓
//! 있어서 줄 단위 판정이 없으면 전부 오탐이 된다.
//!
//! ## 잡지 못하는 것 (R16)
//!
//! 이 가드는 **인라인 세 형태**만 본다 — `x.lock().unwrap_or_else(|p| p.into_inner())`,
//! `Err(p) => p.into_inner()`, `Err(TryLockError::Poisoned(p)) => p.into_inner()`.
//! 이미 손에 쥔 `PoisonError` 를 함수 인자로 받아 `p.into_inner()` 하는 형태(복구 헤드가
//! 근처에 없는 형태)는 텍스트로 poison 인지 확정할 수 없어 보지 않는다 — 그 형태는
//! `recover_poisoned` 가 존재하는 이유이고, 현재 트리의 그런 자리(condvar 재획득 등)는
//! 모두 그 헬퍼를 지난다. 새 그런 자리는 이 가드가 아니라 `recover_poisoned` 규율로 막는다.

use std::path::{Path, PathBuf};

use crate::cfg_predicate::cfg_gated_lines;
use crate::shipping_scope::test_only_files;
use crate::source_text::{mask_non_code, rust_sources};

/// 복구 arm/closure 안에 이것이 있으면 **보고를 동반한** 복구다.
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

/// 헤드를 거슬러 찾을 때 보는 최대 줄 수. 블록형 복구의 긴 다중행 로그 메시지를
/// 넉넉히 덮되(실측 최장 8 줄), 무관한 앞선 arm 까지 번지지 않게 가장 가까운 헤드에서
/// 멈춘다.
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

/// 워크스페이스 전역 census. shipping(비-test-only) 파일의 조용한 복구를 모은다.
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

/// `scan_roots`(예: `["src", "crates"]`) 아래를 훑어 census 를 만든다.
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

    // ── 합성 회귀 (R363) : 세 인라인 형태를 조용/보고로 가른다 ──────────────────

    /// 형태 ① closure — 보고 없는 `unwrap_or_else` 는 조용하다.
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

    /// 형태 ② match arm — 보고 없는 `Err(p) => p.into_inner()` 는 조용하다.
    #[test]
    fn a_bare_err_arm_is_silent() {
        let fc = classify_src(
            "fn f() {\n    let g = match m.lock() {\n        Ok(g) => g,\n        Err(p) => p.into_inner(),\n    };\n}",
        );
        assert_eq!(fc.silent.len(), 1);
    }

    /// 형태 ③ TryLockError::Poisoned — 안쪽 바인더로 잡는다.
    #[test]
    fn a_try_lock_poisoned_arm_is_silent() {
        let fc = classify_src(
            "fn f() {\n    match m.try_write() {\n        Ok(g) => g,\n        Err(TryLockError::Poisoned(p)) => p.into_inner(),\n        Err(TryLockError::WouldBlock) => return,\n    };\n}",
        );
        assert_eq!(fc.silent.len(), 1, "Poisoned(p) 의 안쪽 p 로 잡아야 한다");
    }

    /// 보고를 동반하면 — closure 블록 안에 report — 조용이 아니다.
    #[test]
    fn a_closure_with_a_report_is_not_silent() {
        let fc = classify_src(
            "fn f() {\n    m.lock().unwrap_or_else(|poisoned| {\n        tracing::error!(\"x poisoned\");\n        poisoned.into_inner()\n    });\n}",
        );
        assert_eq!(fc.poison_sites.len(), 1);
        assert!(fc.silent.is_empty(), "보고가 있으니 조용이 아니다");
        assert_eq!(fc.reported.len(), 1);
    }

    /// arm 블록 안 다중행 로그 뒤의 into_inner 도 보고로 본다(헤드에서 이어진 span).
    #[test]
    fn a_multiline_logged_arm_is_reported() {
        let fc = classify_src(
            "fn f() {\n    match m.lock() {\n        Ok(g) => g,\n        Err(poisoned) => {\n            tracing::warn!(\n                \"long message spanning\\\n                 two lines\"\n            );\n            poisoned.into_inner()\n        }\n    };\n}",
        );
        assert!(fc.silent.is_empty());
        assert_eq!(fc.reported.len(), 1);
    }

    /// `#[cfg(test)]` 블록 안의 조용한 복구는 위반이 아니다(줄 단위로 빠진다).
    #[test]
    fn a_silent_recovery_under_cfg_test_is_gated_not_silent() {
        let fc = classify_src(
            "pub fn ship() {}\n\n#[cfg(test)]\nmod t {\n    #[test]\n    fn x() {\n        let _g = m.lock().unwrap_or_else(|p| p.into_inner());\n    }\n}",
        );
        assert_eq!(fc.poison_sites.len(), 1);
        assert_eq!(fc.cfg_gated.len(), 1, "cfg(test) 안이라 gated 여야 한다");
        assert!(fc.silent.is_empty());
    }

    /// 비-poison `into_inner()` 는 복구 헤드가 없어 세지 않는다.
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

    /// 주석·문자열 속 `into_inner` 언급은 코드로 세지 않는다.
    #[test]
    fn a_mention_in_a_comment_is_not_counted() {
        let fc = classify_src("/// 이 자리를 into_inner() 로 되돌린 변이가 살아남았다.\nfn f() {}");
        assert!(fc.poison_sites.is_empty());
    }

    /// 함수 인자로 받은 `PoisonError` 를 into_inner 하는 형태(recover_poisoned 모양)는
    /// 복구 헤드가 근처에 없어 보지 않는다 — R16 에 적은 사각을 못박는다.
    #[test]
    fn a_param_binder_poison_into_inner_is_out_of_scope() {
        let fc = classify_src(
            "fn recover<T>(poisoned: PoisonError<T>) -> T {\n    report();\n    poisoned.into_inner()\n}",
        );
        assert!(fc.poison_sites.is_empty());
    }
}
