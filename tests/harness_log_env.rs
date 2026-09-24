//! 하네스의 로그 환경변수·필터 값과 공용 상수 사용을 제품 설정과 대조한다.
//! 필터가 달라지면 불필요한 로그가 stderr 진단 꼬리를 밀어낼 수 있다.

#[path = "spawn_diag/mod.rs"]
mod spawn_diag;

use std::path::{Path, PathBuf};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).to_path_buf()
}

fn read(rel: &str) -> String {
    let path = repo_root().join(rel);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("{} 를 읽지 못했다: {e}", path.display()))
}

/// 고정된 토큰 사이를 읽는다. 제품 소스의 호출 형식이 바뀌면 판독도 갱신해야 한다.
fn product_log_env_and_filter() -> (String, String) {
    let src = read("crates/tasty-platform/src/crash_report.rs");
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

/// 명시한 필터는 기본값을 대체하므로 제품의 억제 설정도 함께 포함해야 한다.
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
