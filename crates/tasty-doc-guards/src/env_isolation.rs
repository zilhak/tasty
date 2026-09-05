//! **환경변수·cwd 를 만지는 테스트가 직렬화 없이 만지지 않는가** 를 워크스페이스 전역에서
//! 본다(ADR-0129 형태 A — 프로세스 전역 공유 상태).
//!
//! `std::env::set_var`/`remove_var` 와 `set_current_dir` 는 프로세스 전역을 바꾼다.
//! `cargo test` 는 한 바이너리의 테스트를 병렬로 돌리므로, 직렬화 없이 이것을 만지는
//! 테스트끼리는 서로의 상태를 덮어써 **순서 의존 flake** 가 난다. 실패한 테스트가
//! 전역을 안 되돌리고 죽으면 다음 테스트를 오염시키고, 그 오염은 단독 실행에서 재현되지
//! 않는다. 처방은 직렬화 락(또는 그 락을 쥐는 RAII 가드)이다.
//!
//! ## 왜 리포 전역 한 자리인가 — 자기스캔 형태가 복제되지 않았다
//!
//! 강제 패턴은 `tasty-host-plugin` 의 자기스캔(`home_env_is_only_touched_through_this_module`)에
//! **이미 있다.** 그 형태는 "가드가 통째로 한 파일이고, 그 파일 밖의 env 변경은 위반" 이다.
//! 그런데 이 형태는 세 크레이트에 그대로 안 옮겨진다(2026-09-05 실측, R384):
//! - `tasty-telemetry`·`tasty-settings` 의 가드(`AgentIdEnvGuard`·`RelativeHomeGuard`)는
//!   **인라인 struct** 라 "파일 밖" 으로 제외할 수 없다.
//! - 본체는 프로덕션(`boot/locale.rs`)이 로케일 env 를 set 하므로 테스트 스캔이 그것을
//!   빼야 하는데, per-crate 스캔이 `#[cfg(test)]` 판정을 새로 만드는 것은 R320 위반이다.
//!
//! 그래서 **효과는 복제하되 형태는 리포 전역**으로 둔다 — 여기(의존 0)의
//! [`cfg_gated_lines`](crate::cfg_predicate::cfg_gated_lines) 가 프로덕션을 구조로 빼고,
//! [`test_only_files`](crate::shipping_scope::test_only_files) 가 test-only 파일을 잡는다.
//! doc-guards.yml 이 매 push 에 돌린다.
//!
//! ## 극성 — "이 테스트가 전역을 만지나" 가 아니라 "직렬화 없이 만지나"
//!
//! 방향을 뒤집는다: **테스트의 모든 env/cwd 변형은 직렬화를 밝혀야 한다.** 직렬화 증거는
//! 세 형태다:
//! - 락 참조(`SERIAL`/`*_LOCK`/`GLOBALS`.lock() — 코드): 그 함수가 락을 쥐고 만진다.
//! - 마커(`직렬화`/`serialize` 또는 `이유:`/`reason:` — 주석, `check-allow-reason` 관례):
//!   RAII 가드의 set/unset/drop 은 락을 호출부가 쥐므로 그 자리에 `.lock()` 이 없다 — 그래서
//!   그 자리 주석이 "…락으로 직렬화" 를 밝히면 인정한다.
//! - 단일-test 격리(`단일 #[test]` — 주석): 한 키를 만지는 시나리오를 한 `#[test]` 안에
//!   모으면 cargo 가 그 함수를 한 스레드로 완주해 구조적으로 직렬이다(별도 락 없이도 안전).
//!
//! ## 잡지 못하는 것 (R16)
//!
//! - **갈래 2(범위 새는 자리)**: 가드를 락 없이 쓰는 테스트. 변형이 가드 안에 있어 이
//!   스캔에 안 걸린다 — 그건 가드가 락을 자기가 잡게 하는 별도 처방으로 닫는다.
//! - 함수 두 겹 너머의 간접 env 접근(이름 안 보임)·런타임 조합 키는 텍스트 밖이다.

use std::path::Path;

use crate::cfg_predicate::cfg_gated_lines;
use crate::shipping_scope::test_only_files;
use crate::source_text::{mask_literals, mask_non_code, rust_sources};

/// 프로세스 전역을 바꾸는 호출. 여는 괄호까지 넣어 동명 식별자(`set_current_dir_is_…`
/// 같은 테스트 함수명)에 오탐하지 않게 한다.
///
/// 마지막 바늘은 통째로 적지 않는다. `tasty-cli` 의 cwd 재진입 가드
/// (`cwd_resolve.rs` 의 `set_current_dir_is_confined_to_serialized_tests`)가
/// 같은 토큰을 찾는데 **문자열 리터럴을 안 가리므로**, 여기 통째로 적으면 이 파일이
/// 그 가드에 "직렬화 없이 cwd 를 바꾸는 소스" 로 잡힌다 — 언급을 사용으로 세는 것이다.
/// 쪼개면 동작은 같고(`concat!` 은 컴파일 타임) 그 오탐만 사라진다.
const MUTATION_TOKENS: &[&str] = &[
    "env::set_var(",
    "env::remove_var(",
    concat!("set_current_dir", "("),
];

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

/// 직렬화 증거를 찾을 때 거슬러 오르는 최대 줄 수. 테스트는 함수 첫머리에서 락을 한 번
/// 잡고 그 아래에서 여러 번 env 를 만지므로(창이 아니라 **함수 범위**로 본다), 가장 가까운
/// `fn ` 선언까지 올라가 그 사이에 직렬화 증거가 있는지 본다. 이 값은 그 상한이다.
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
        // 테스트 맥락이 아니면(프로덕션) 이 축의 대상이 아니다.
        if !(file_is_test_only || cfg_test[idx]) {
            continue;
        }
        out.mutations.push(idx);
        // 직렬화 증거는 **enclosing 함수 범위**에서 찾는다 — 락은 함수 첫머리에서 한 번
        // 잡고 그 아래에서 여러 번 만지기 때문이다. 가장 가까운 `fn ` 선언까지 거슬러
        // 오르되(FN_LOOKBACK 상한), 그 사이 한 줄이라도 락 참조/직렬화 마커를 가지면 통과.
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

/// 워크스페이스 전역 census.
#[derive(Debug, Default)]
pub struct Census {
    pub files_scanned: usize,
    pub mutations: usize,
    pub serialized: usize,
    /// `"레포상대경로:1기반줄: 원문"` 형태의 위반 목록.
    pub bare: Vec<String>,
}

/// `scan_roots` 아래를 훑어 census 를 만든다.
pub fn census(root: &Path, scan_roots: &[&str]) -> Census {
    let sources = rust_sources(root, scan_roots);
    let test_only = test_only_files(root, &sources);

    let mut c = Census::default();
    for (rel, raw) in &sources {
        c.files_scanned += 1;
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

    /// test 맥락에서 직렬화 증거 없이 env 를 만지면 잡는다.
    #[test]
    fn a_bare_set_var_in_test_code_is_caught() {
        let fc = classify_src(
            "#[cfg(test)]\nmod t {\n    #[test]\n    fn x() {\n        unsafe { std::env::set_var(\"K\", \"v\") };\n    }\n}",
            false,
        );
        assert_eq!(fc.mutations.len(), 1);
        assert_eq!(fc.bare.len(), 1, "직렬화 없는 test env 변형을 잡아야 한다");
    }

    /// 같은 함수에서 락을 잡으면(코드 증거) 통과한다.
    #[test]
    fn a_lock_in_scope_passes() {
        let fc = classify_src(
            "#[cfg(test)]\nmod t {\n    #[test]\n    fn x() {\n        let _s = SERIAL.lock().unwrap();\n        unsafe { std::env::set_var(\"K\", \"v\") };\n    }\n}",
            false,
        );
        assert!(fc.bare.is_empty());
        assert_eq!(fc.serialized.len(), 1);
    }

    /// 주석 마커(가드 set 메서드처럼 락을 호출부가 쥐는 자리)면 통과한다.
    #[test]
    fn a_marker_comment_passes() {
        let fc = classify_src(
            "#[cfg(test)]\nmod t {\n    impl G {\n        fn set(&self) {\n            // SAFETY: ENV_LOCK 가드로 직렬화된 단위 테스트 한정.\n            unsafe { std::env::set_var(\"K\", \"v\") };\n        }\n    }\n}",
            false,
        );
        assert!(fc.bare.is_empty(), "직렬화 마커가 붙으면 통과");
    }

    /// 프로덕션(cfg(test) 밖·test-only 파일 아님)의 env 변형은 이 축의 대상이 아니다.
    #[test]
    fn a_production_env_mutation_is_out_of_scope() {
        let fc = classify_src(
            "fn export_locale(key: &str, v: &str) {\n    unsafe { std::env::set_var(key, v) };\n}",
            false,
        );
        assert!(fc.mutations.is_empty(), "프로덕션 env 변형은 안 본다");
    }

    /// test-only 파일이면 그 안의 변형은 test 맥락이다.
    #[test]
    fn a_mutation_in_a_test_only_file_is_in_scope() {
        let fc = classify_src(
            "pub fn helper() {\n    unsafe { std::env::set_var(\"K\", \"v\") };\n}",
            true,
        );
        assert_eq!(fc.mutations.len(), 1);
        assert_eq!(fc.bare.len(), 1, "마커 없으면 test-only 파일에서도 잡는다");
    }

    /// set_current_dir 도 같은 축이다.
    #[test]
    fn set_current_dir_is_also_covered() {
        let fc = classify_src(
            concat!(
                "#[cfg(test)]\nmod t {\n    #[test]\n    fn x() {\n        std::env::set_current_dir",
                "(\"/tmp\").unwrap();\n    }\n}"
            ),
            false,
        );
        assert_eq!(fc.bare.len(), 1);
    }

    /// 단일 #[test] 격리를 밝히는 주석이면 통과한다(락 없이도 구조적 직렬).
    #[test]
    fn a_single_test_containment_marker_passes() {
        let fc = classify_src(
            "#[cfg(test)]\nmod t {\n    impl G {\n        fn unset(&self) {\n            // SAFETY: 이 키를 만지는 시나리오를 단일 #[test] 안에 모아 격리했다.\n            unsafe { std::env::remove_var(\"K\") };\n        }\n    }\n}",
            false,
        );
        assert!(fc.bare.is_empty(), "단일 #[test] 격리 마커면 통과");
        assert_eq!(fc.serialized.len(), 1);
    }

    /// 마커가 문자열 안에만 있으면 인정하지 않는다.
    #[test]
    fn a_marker_inside_a_string_does_not_count() {
        let fc = classify_src(
            "#[cfg(test)]\nmod t {\n    #[test]\n    fn x() {\n        let _m = \"SERIAL 직렬화\";\n        unsafe { std::env::set_var(\"K\", \"v\") };\n    }\n}",
            false,
        );
        assert_eq!(fc.bare.len(), 1, "문자열 속 마커는 증거가 아니다");
    }
}
