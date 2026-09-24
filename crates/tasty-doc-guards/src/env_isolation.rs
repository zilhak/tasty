//! 테스트의 환경변수·cwd 변경에 직렬화 표지가 있는지 검사한다(ADR-0045).
//! 이 상태는 프로세스 전체가 공유하므로 같은 바이너리의 병렬 테스트가 서로 영향을 줄 수 있다.
//! cfg 범위와 테스트 전용 파일 판정은 공용 구현을 사용하며 제품 코드는 검사하지 않는다.
//!
//! 인정하는 표지는 SERIAL/*_LOCK/GLOBALS 같은 코드 이름, 직렬화/serialize/이유:/reason:
//! 주석, 단일 #[test] 격리 주석이다. 락을 호출자가 보유하는 RAII 가드에도 주석을 허용한다.
//! 표지의 존재만 확인하며 실제 락 수명이나 가드를 락 없이 호출하는 경로는 검증하지 못한다.
//! 간접 호출로 숨긴 변경이나 실행 중 조립하는 환경변수 키도 추적하지 않는다.

use std::path::Path;

use crate::cfg_predicate::cfg_gated_lines;
use crate::shipping_scope::test_only_files;
use crate::source_text::{mask_literals, mask_non_code, rust_sources};

/// 프로세스 전역을 바꾸는 호출. 여는 괄호까지 넣어 동명 식별자(`set_current_dir_is_…`
/// 같은 테스트 함수명)에 오탐하지 않게 한다.
const MUTATION_TOKENS: &[&str] = &["env::set_var(", "env::remove_var(", "set_current_dir("];

/// 그 변형이 직렬화됨을 밝히는 증거(코드의 락 참조 + 주석 마커).
const SERIAL_TOKENS: &[&str] = &[
    "SERIAL",
    "_LOCK",
    "GLOBALS",
    "직렬화",
    "serialize",
    "serialized",
    "단일 #[test]",
    "single #[test]",
    "이유:",
    "reason:",
];

/// 변형 지점에서 가장 가까운 fn 선언까지 직렬화 표지를 찾는 최대 줄 수.
const FN_LOOKBACK: usize = 80;

/// 한 파일 분류 결과. 줄 번호는 0 기반.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct FileClass {
    /// 테스트 맥락의 env/cwd 변형 자리 전부.
    pub mutations: Vec<usize>,
    /// 그중 직렬화가 밝혀진 줄.
    pub serialized: Vec<usize>,
    /// 그중 직렬화 증거가 없는 줄(위반).
    pub bare: Vec<usize>,
}

/// masked 코드·마커 줄과 그 파일의 test 맥락 정보로 env/cwd 변형을 분류한다.
///
/// `code` 는 [`mask_non_code`](crate::source_text::mask_non_code)(변형 토큰 탐지용),
/// `markers` 는 [`mask_literals`](crate::source_text::mask_literals)(코드의 락 참조와
/// 주석 마커를 함께 남긴다). `cfg_test` 는 [`cfg_gated_lines`] 결과. `file_is_test_only`
/// 면 파일 전체가 test 맥락이다.
pub fn classify(
    code: &[&str],
    markers: &[&str],
    cfg_test: &[bool],
    file_is_test_only: bool,
) -> FileClass {
    assert_eq!(code.len(), markers.len(), "두 마스크 줄 수가 다르다");
    assert_eq!(code.len(), cfg_test.len(), "cfg 마스크 줄 수가 다르다");
    let mut out = FileClass::default();
    for idx in 0..code.len() {
        if !MUTATION_TOKENS.iter().any(|t| code[idx].contains(t)) {
            continue;
        }
        if !(file_is_test_only || cfg_test[idx]) {
            continue;
        }
        out.mutations.push(idx);
        // 락이 함수 앞부분에 있을 수 있어 가장 가까운 fn 선언까지 확인한다.
        let floor = idx.saturating_sub(FN_LOOKBACK);
        let fn_start = (floor..=idx)
            .rev()
            .find(|&j| code[j].contains("fn "))
            .unwrap_or(floor);
        let hi = (idx + 1).min(code.len() - 1);
        let serialized =
            (fn_start..=hi).any(|j| SERIAL_TOKENS.iter().any(|t| markers[j].contains(t)));
        if serialized {
            out.serialized.push(idx);
        } else {
            out.bare.push(idx);
        }
    }
    out
}

/// 워크스페이스 전체 검사 결과.
#[derive(Debug, Default)]
pub struct Census {
    pub files_scanned: usize,
    /// scan_roots 순서대로 센 파일 수. 겹치는 경로는 처음 일치한 루트에만 포함한다.
    /// 작은 루트의 누락이 전체 파일 수에 가려지지 않도록 소비자가 각각 하한을 검사한다.
    pub per_root: Vec<(String, usize)>,
    pub mutations: usize,
    pub serialized: usize,
    /// `"레포상대경로:1기반줄: 원문"` 형태의 위반 목록.
    pub bare: Vec<String>,
}

/// scan_roots 아래의 테스트 코드를 검사한다.
pub fn census(root: &Path, scan_roots: &[&str]) -> Census {
    let sources = rust_sources(root, scan_roots);
    let test_only = test_only_files(root, &sources);

    let mut c = Census {
        per_root: scan_roots.iter().map(|r| ((*r).to_string(), 0)).collect(),
        ..Census::default()
    };
    for (rel, raw) in &sources {
        c.files_scanned += 1;
        // 경로 구성요소 단위로 비교해 src와 srcgen을 구분하고 Windows도 지원한다.
        if let Some((_, n)) = c.per_root.iter_mut().find(|(r, _)| rel.starts_with(r)) {
            *n += 1;
        }
        let code_src = mask_non_code(raw);
        let marker_src = mask_literals(raw);
        let code: Vec<&str> = code_src.lines().collect();
        let markers: Vec<&str> = marker_src.lines().collect();
        let raw_lines: Vec<&str> = raw.lines().collect();
        let cfg_test = cfg_gated_lines(&code, "test");
        let fc = classify(&code, &markers, &cfg_test, test_only.contains(rel));

        c.mutations += fc.mutations.len();
        c.serialized += fc.serialized.len();
        for &idx in &fc.bare {
            let text = raw_lines.get(idx).map(|s| s.trim()).unwrap_or("");
            c.bare
                .push(format!("{}:{}: {text}", rel.display(), idx + 1));
        }
    }
    c
}

#[cfg(test)]
mod tests {
    use super::*;

    fn classify_src(src: &str, test_only: bool) -> FileClass {
        let code_src = mask_non_code(src);
        let marker_src = mask_literals(src);
        let code: Vec<&str> = code_src.lines().collect();
        let markers: Vec<&str> = marker_src.lines().collect();
        let cfg_test = cfg_gated_lines(&code, "test");
        classify(&code, &markers, &cfg_test, test_only)
    }

    #[test]
    fn a_bare_set_var_in_test_code_is_caught() {
        let fc = classify_src(
            "#[cfg(test)]\nmod t {\n    #[test]\n    fn x() {\n        unsafe { std::env::set_var(\"K\", \"v\") };\n    }\n}",
            false,
        );
        assert_eq!(fc.mutations.len(), 1);
        assert_eq!(fc.bare.len(), 1, "직렬화 없는 test env 변형을 잡아야 한다");
    }

    #[test]
    fn a_lock_in_scope_passes() {
        let fc = classify_src(
            "#[cfg(test)]\nmod t {\n    #[test]\n    fn x() {\n        let _s = SERIAL.lock().unwrap();\n        unsafe { std::env::set_var(\"K\", \"v\") };\n    }\n}",
            false,
        );
        assert!(fc.bare.is_empty());
        assert_eq!(fc.serialized.len(), 1);
    }

    #[test]
    fn a_marker_comment_passes() {
        let fc = classify_src(
            "#[cfg(test)]\nmod t {\n    impl G {\n        fn set(&self) {\n            // SAFETY: ENV_LOCK 가드로 직렬화된 단위 테스트 한정.\n            unsafe { std::env::set_var(\"K\", \"v\") };\n        }\n    }\n}",
            false,
        );
        assert!(fc.bare.is_empty(), "직렬화 마커가 붙으면 통과");
    }

    #[test]
    fn a_production_env_mutation_is_out_of_scope() {
        let fc = classify_src(
            "fn export_locale(key: &str, v: &str) {\n    unsafe { std::env::set_var(key, v) };\n}",
            false,
        );
        assert!(fc.mutations.is_empty(), "프로덕션 env 변형은 안 본다");
    }

    #[test]
    fn a_mutation_in_a_test_only_file_is_in_scope() {
        let fc = classify_src(
            "pub fn helper() {\n    unsafe { std::env::set_var(\"K\", \"v\") };\n}",
            true,
        );
        assert_eq!(fc.mutations.len(), 1);
        assert_eq!(fc.bare.len(), 1, "마커 없으면 test-only 파일에서도 잡는다");
    }

    #[test]
    fn set_current_dir_is_also_covered() {
        let fc = classify_src(
            "#[cfg(test)]\nmod t {\n    #[test]\n    fn x() {\n        std::env::set_current_dir(\"/tmp\").unwrap();\n    }\n}",
            false,
        );
        assert_eq!(fc.bare.len(), 1);
    }

    #[test]
    fn a_single_test_containment_marker_passes() {
        let fc = classify_src(
            "#[cfg(test)]\nmod t {\n    impl G {\n        fn unset(&self) {\n            // SAFETY: 이 키를 만지는 시나리오를 단일 #[test] 안에 모아 격리했다.\n            unsafe { std::env::remove_var(\"K\") };\n        }\n    }\n}",
            false,
        );
        assert!(fc.bare.is_empty(), "단일 #[test] 격리 마커면 통과");
        assert_eq!(fc.serialized.len(), 1);
    }

    #[test]
    fn a_marker_inside_a_string_does_not_count() {
        let fc = classify_src(
            "#[cfg(test)]\nmod t {\n    #[test]\n    fn x() {\n        let _m = \"SERIAL 직렬화\";\n        unsafe { std::env::set_var(\"K\", \"v\") };\n    }\n}",
            false,
        );
        assert_eq!(fc.bare.len(), 1, "문자열 속 마커는 증거가 아니다");
    }
}
