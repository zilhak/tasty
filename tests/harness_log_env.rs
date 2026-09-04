//! e2e 하네스가 자식에게 주는 **로그 필터 env 의 이름과 값**을 제품 소스와 대조한다.
//!
//! 이 자리는 한 번 실제로 틀렸다 — 두 하네스가 `RUST_LOG` 를 넣고 있었고, 제품은
//! `TASTY_LOG` 를 읽으므로 필터가 아무 효과 없이 오랫동안 통과했다. 이름이 틀려도
//! 컴파일되고 테스트도 전부 초록이라, 되돌아가는 변경을 **아무것도 잡지 못한다.**
//! 값도 마찬가지다: `TASTY_LOG=warn` 처럼 제품 기본 필터와 모양이 다르면 억제가 풀려
//! 자식 stderr 이 늘어나고, 그만큼 30 줄짜리 진단 tail 이 밀려난다.
//!
//! 그래서 여기서는 소스를 읽어 대조한다. 하네스가 상수를 안 쓰고 문자열을 직접 넣는
//! 형태로 되돌아가도 걸리게, **상수의 값**과 **하네스가 그 상수를 쓰는지** 둘 다 본다.

#[path = "spawn_diag/mod.rs"]
mod spawn_diag;

use std::path::{Path, PathBuf};

fn repo_root() -> PathBuf {
    // 이 파일은 <root>/tests/ 에 있다.
    Path::new(env!("CARGO_MANIFEST_DIR")).to_path_buf()
}

fn read(rel: &str) -> String {
    let path = repo_root().join(rel);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("{} 를 읽지 못했다: {e}", path.display()))
}

/// 제품이 실제로 읽는 env 이름과 기본 필터를 `crash_report.rs` 에서 뽑는다.
///
/// 정규식 없이 고정 토큰으로 자른다 — 이 테스트가 잡으려는 것은 "제품이 바뀌었는데
/// 하네스가 안 따라갔다" 이지 `crash_report.rs` 의 서식이 아니다.
fn product_log_env_and_filter() -> (String, String) {
    let src = read("src/platform/crash_report.rs");
    let env = extract_between(&src, "EnvFilter::try_from_env(\"", "\")")
        .expect("crash_report.rs 에서 try_from_env(\"…\") 를 찾지 못했다 — 이 테스트를 갱신하라");
    let filter = extract_between(&src, "EnvFilter::new(\"", "\")")
        .expect("crash_report.rs 에서 EnvFilter::new(\"…\") 를 찾지 못했다 — 이 테스트를 갱신하라");
    (env, filter)
}

fn extract_between(haystack: &str, open: &str, close: &str) -> Option<String> {
    let start = haystack.find(open)? + open.len();
    let rest = &haystack[start..];
    let end = rest.find(close)?;
    Some(rest[..end].to_string())
}

/// 이름이 제품과 같은가. 되돌리기(`RUST_LOG`)가 여기서 걸린다.
#[test]
fn the_harness_uses_the_env_name_the_product_reads() {
    let (env, _) = product_log_env_and_filter();
    assert_eq!(
        spawn_diag::LOG_ENV,
        env,
        "제품은 `{env}` 를 읽는데 하네스 상수는 `{}` 다 — 하네스가 준 필터가 통째로 무시된다",
        spawn_diag::LOG_ENV
    );
}

/// 값이 제품 기본 필터와 같은 모양인가.
///
/// `TASTY_LOG` 를 지정하면 제품 기본 필터는 **대체**된다. 그러므로 억제 목록을 같이
/// 주지 않으면 아무것도 안 준 것보다 로그가 늘어난다 — "cap 을 건다" 는 변경이
/// 정확히 반대로 작동한다.
#[test]
fn the_harness_filter_keeps_the_product_default_suppressions() {
    let (_, filter) = product_log_env_and_filter();
    assert_eq!(
        spawn_diag::LOG_FILTER,
        filter,
        "하네스 필터가 제품 기본과 다르다. 제품 기본에만 있는 억제는 하네스에서 풀린다"
    );
    assert!(
        spawn_diag::LOG_FILTER_WEBHOOK.starts_with(spawn_diag::LOG_FILTER),
        "웹훅 필터는 공용 필터를 그대로 앞에 두고 뒤에만 덧붙여야 한다: {}",
        spawn_diag::LOG_FILTER_WEBHOOK
    );
}

/// 하네스가 **그 상수를 실제로 쓰는가.** 위 두 테스트는 상수만 보므로, 하네스가
/// 상수를 무시하고 문자열을 직접 넣으면 통과해 버린다.
#[test]
fn both_harnesses_set_the_filter_through_the_shared_constants() {
    for rel in ["tests/common/mod.rs", "tests/webhook_common/mod.rs"] {
        let src = read(rel);
        assert!(
            src.contains(".env(spawn_diag::LOG_ENV, spawn_diag::LOG_FILTER"),
            "{rel} 이 공용 상수로 로그 필터를 설정하지 않는다"
        );
        assert!(
            !src.contains("\"RUST_LOG\""),
            "{rel} 에 `RUST_LOG` 가 되살아났다 — 제품은 그 변수를 읽지 않는다"
        );
        assert!(
            !src.contains(".env(\"TASTY_LOG\""),
            "{rel} 이 env 이름을 직접 적었다 — 정의 자리는 `spawn_diag` 하나여야 한다"
        );
    }
}
