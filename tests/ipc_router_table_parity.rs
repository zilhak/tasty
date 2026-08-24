//! IPC 라우터의 dispatch 팔이 전부 권한 표에 등재돼 있는지 검증한다.
//!
//! `method_meta()` 가 `None` 을 반환하는 메서드는 plugin/agent 호출자에게
//! `UnknownMethod` 로 거부된다. 라우터에 분기가 있는데 표에 없으면 그 거부가
//! **정책인지 등재 누락인지 구분되지 않는다** — 코드를 읽는 쪽에서도, 나중에
//! 권한을 재검토하는 쪽에서도. local caller 전용으로 두려는 의도라면
//! `local_only()` 로 명시 등재해 의도를 표에 남긴다.
//!
//! 이 가드가 없던 동안 `agent.task_set_result` · `debug.close_workspace` /
//! `debug.switch_workspace` / `debug.switch_tab` · `plugin.upgrade_builtins`
//! 5 종이 형제 메서드가 전부 등재된 상태에서 조용히 빠져 있었다. 정책·근거
//! 본문은 [`docs/dev-guide/api-conventions.md`] · [`docs/dev-guide/debug-ipc.md`].
//!
//! **[`ROUTER_SOURCES`] 에 파일을 빠뜨리면 그만큼 사각지대가 그대로 남는다** —
//! 실제로 `src/app/` 쪽 3 파일이 처음 목록에서 빠져 `plugin.upgrade_builtins`
//! 하나가 가드를 통과했다. 새 dispatch match 를 다른 파일에 만들면 여기에
//! 추가한다.
//!
//! release 빌드에서는 `DEBUG_METHODS` 가 설계상 비어 있어(`debug.*` 는 release
//! IPC 표면에서 완전히 사라진다) 이 대조가 성립하지 않는다. 따라서 debug
//! 빌드에서만 돈다 — CI 의 `cargo test --workspace --locked` 가 debug 다.
#![cfg(debug_assertions)]

use std::path::Path;

use tasty_ipc::method_meta::method_meta;

/// 라우터 dispatch 팔이 있는 소스. 각 파일에서 `"<method>" =>` 형태의 줄만 읽는다.
const ROUTER_SOURCES: &[&str] = &[
    "src/adapters/ipc/handler.rs",
    "src/adapters/ipc/handler/ime.rs",
    "src/adapters/ipc/handler/debug_plugin.rs",
    "src/app/dispatch/list_global.rs",
    "src/app/ipc/app_methods.rs",
    "src/app/ipc/debug_methods.rs",
];

/// `    "foo.bar" => ...` 형태의 match 팔에서 메서드 이름만 뽑는다.
///
/// 팔 문법(`"..." =>`)으로 좁히는 게 핵심이다 — `method.starts_with("agent.rate_limit_")`
/// 같은 판정 헬퍼의 문자열 리터럴을 구조적으로 배제하므로 예외 allowlist 가 필요 없다.
fn arm_method(line: &str) -> Option<&str> {
    let t = line.trim_start();
    let rest = t.strip_prefix('"')?;
    let (name, after) = rest.split_once('"')?;
    if !after.trim_start().starts_with("=>") {
        return None;
    }
    // 메서드 이름 형태(`ns.method`)만 — 문자열 매칭 팔 전반이 아니라.
    if !name.contains('.')
        || !name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c == '_' || c == '.')
    {
        return None;
    }
    Some(name)
}

#[test]
fn every_router_arm_is_registered_in_method_table() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut missing: Vec<String> = Vec::new();
    let mut scanned = 0usize;

    for rel in ROUTER_SOURCES {
        let path = root.join(rel);
        let src = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("라우터 소스를 읽을 수 없다: {rel}: {e}"));
        for line in src.lines() {
            let Some(name) = arm_method(line) else {
                continue;
            };
            scanned += 1;
            if method_meta(name).is_none() {
                missing.push(format!("{rel}: {name}"));
            }
        }
    }

    assert!(
        scanned > 100,
        "라우터 팔을 {scanned} 개밖에 못 찾았다 — 스캔 패턴이 깨졌을 가능성이 크다"
    );
    missing.sort();
    missing.dedup();
    assert!(
        missing.is_empty(),
        "라우터에 분기가 있는데 METHOD_TABLE/DEBUG_METHODS/PREFIX_RULES 어디에도 \
         등재되지 않은 메서드가 있다. plugin 에 열 것이면 plugin(&[..]) 으로, \
         local caller 전용으로 둘 것이면 local_only() 로 **명시 등재**하라 \
         (미등재는 UnknownMethod 거부라 의도와 구분되지 않는다):\n  {}",
        missing.join("\n  ")
    );
}
