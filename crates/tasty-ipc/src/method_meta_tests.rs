//! `method_meta` 단위 테스트.

use crate::method_meta::{
    METHOD_TABLE, PREFIX_RULES, clear_plugin_prefixes_for_tests, method_meta,
    register_plugin_prefix, unregister_plugin_prefix,
};
use tasty_plugin_manifest::Permission;

/// runtime registry 가 process-global 이라 동일 binary 안에서 병렬 test 가
/// PLUGIN_PREFIXES 를 동시 변형하지 못하게 직렬화.
static TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn test_lock() -> std::sync::MutexGuard<'static, ()> {
    TEST_LOCK
        .lock()
        .unwrap_or_else(|poison| poison.into_inner())
}

#[test]
fn unknown_method_returns_none() {
    assert!(method_meta("not.a.real.method").is_none());
}

#[test]
fn no_duplicate_method_names() {
    let mut seen = std::collections::HashSet::new();
    for (name, _) in METHOD_TABLE {
        assert!(seen.insert(*name), "duplicate method name: {name}");
    }
}

/// 모든 등록 메서드는 명명 규칙을 따라야 한다 (docs/dev-guide/cli-naming.md):
///
/// 1. `<namespace>.<verb>` 또는 `<namespace>.<sub>.<verb>` 3단까지
/// 2. 또는 [`ROOT_EXCEPTIONS`]에 등록된 root 메서드
/// 3. 각 부분은 소문자 알파벳/숫자/`_` 만 허용
#[test]
fn all_registered_methods_match_naming_policy() {
    const ROOT_EXCEPTIONS: &[&str] = &["split", "tree"];

    for (name, _) in METHOD_TABLE {
        if ROOT_EXCEPTIONS.contains(name) {
            continue;
        }
        let parts: Vec<&str> = name.split('.').collect();
        assert!(
            parts.len() >= 2 && parts.len() <= 3,
            "method '{name}' must be <namespace>.<verb> or <namespace>.<sub>.<verb> \
             (or registered in ROOT_EXCEPTIONS)"
        );
        for part in &parts {
            assert!(!part.is_empty(), "method '{name}' has empty segment");
            assert!(
                part.chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_'),
                "method '{name}': segment '{part}' has invalid characters \
                 (only lowercase a-z, 0-9, _)"
            );
        }
    }
}

#[test]
fn prefix_rules_target_valid_namespaces() {
    for (prefix, _) in PREFIX_RULES {
        assert!(
            prefix.contains('.'),
            "prefix '{prefix}' must include a namespace separator"
        );
        assert!(
            prefix.ends_with('_') || prefix.ends_with('.'),
            "prefix '{prefix}' should end with `_` or `.` to avoid mid-token matches"
        );
    }
}

#[test]
#[cfg(debug_assertions)]
fn debug_methods_are_local_only() {
    let m = method_meta("debug.inject_key").expect("registered (debug build)");
    assert!(!m.plugin_callable);
}

#[test]
#[cfg(not(debug_assertions))]
fn debug_methods_absent_in_release() {
    assert!(method_meta("debug.inject_key").is_none());
    assert!(method_meta("system.shutdown").is_none());
    // ui.screenshot 은 focus-독립 정식 기능으로 승격됨 — release 에 노출된다
    // (아래 `ui_screenshot_promoted_to_release` 참조).
}

/// `ui.screenshot` 은 debug 전용에서 focus-독립 정식 기능으로 승격됐다 —
/// release `METHOD_TABLE` 에 존재하고 local_only(파일 쓰기 표면, plugin 미노출)여야
/// 한다. (구 debug-only 대칭 테스트 `debug_methods_absent_in_release` 와 짝.)
#[test]
fn ui_screenshot_promoted_to_release() {
    let m = method_meta("ui.screenshot").expect("registered in release METHOD_TABLE");
    assert!(!m.plugin_callable, "ui.screenshot must be local_only");
    assert!(m.required.is_empty());
}

#[test]
fn surface_list_requires_surface_read() {
    let m = method_meta("surface.list").expect("registered");
    assert!(m.plugin_callable);
    assert!(m.required.contains(&Permission::SurfaceRead));
}

/// occupancy-05: codex/claude plugin 이 자식 관리를 호스트 `terminal.*` 로 위임할 수
/// 있어야 한다. 모든 terminal.* 메서드가 plugin_callable 이고, 요구 권한이 두
/// plugin 이 매니페스트에 이미 선언한 권한 집합(surface.read/write, terminal.spawn/
/// write/read) 부분집합이어야 위임이 permission_denied 없이 통과한다.
#[test]
fn terminal_star_is_plugin_callable_within_agent_plugin_permissions() {
    use Permission::*;
    // codex/claude 매니페스트가 보유한 terminal-관련 권한 상한.
    let held = [
        SurfaceRead,
        SurfaceWrite,
        TerminalSpawn,
        TerminalWrite,
        TerminalRead,
    ];
    for method in [
        "terminal.spawn",
        "terminal.tell",
        "terminal.wait",
        "terminal.children",
        "terminal.parent",
        "terminal.kill",
        "terminal.respawn",
        "terminal.broadcast",
        "terminal.set_state",
    ] {
        let m = method_meta(method).unwrap_or_else(|| panic!("{method} not registered"));
        assert!(m.plugin_callable, "{method} must be plugin-callable");
        for needed in m.required {
            assert!(
                held.contains(needed),
                "{method} requires '{}' which codex/claude do not hold",
                needed.as_token()
            );
        }
    }
}

#[test]
fn recent_query_requires_surface_read() {
    // generic per-kind recent 조회는 임의 파일 read(FsRead) 가 아니라 이미 열었던 목록
    // 반환뿐 → 더 약한 SurfaceRead 권한. plugin(주소창 03)이 호출 가능해야 한다.
    let m = method_meta("recent.query").expect("registered");
    assert!(
        m.plugin_callable,
        "address bar plugin must be able to query recents"
    );
    assert!(m.required.contains(&Permission::SurfaceRead));
    assert!(
        !m.required.contains(&Permission::FsRead),
        "recent 은 임의 path read 가 아니므로 FsRead 를 요구하지 않는다"
    );
}

#[test]
fn tab_create_requires_surface_write() {
    let m = method_meta("tab.create").expect("registered");
    assert!(m.plugin_callable);
    assert!(m.required.contains(&Permission::SurfaceWrite));
}

#[test]
fn surface_completion_requires_notification() {
    // completion 은 read 가 아니라 highlight 발동(PushNotification 계열) →
    // notification.* 와 동일한 Notification 권한, plugin 이 호출 가능해야 한다.
    let m = method_meta("surface.completion").expect("registered");
    assert!(
        m.plugin_callable,
        "agents must be able to signal completion"
    );
    assert!(m.required.contains(&Permission::Notification));
}

#[test]
fn ime_methods_are_local_only_via_prefix() {
    let m = method_meta("surface.ime_commit").expect("registered");
    assert!(!m.plugin_callable);
}

#[test]
fn plugin_management_is_local_only() {
    let m = method_meta("plugin.enable").expect("registered");
    assert!(!m.plugin_callable);
}

#[test]
fn agent_task_methods_require_agent_manage() {
    for name in [
        "agent.task_create",
        "agent.task_list",
        "agent.task_get",
        "agent.task_await",
        "agent.task_cancel",
        "agent.task_retry",
        "agent.task_graph",
    ] {
        let m = method_meta(name).unwrap_or_else(|| panic!("registered: {name}"));
        assert!(m.plugin_callable, "{name} should be plugin-callable");
        assert!(
            m.required.contains(&Permission::AgentManage),
            "{name} should require AgentManage"
        );
    }
}

#[test]
fn agent_lease_methods_require_agent_manage() {
    for name in [
        "agent.lease_acquire",
        "agent.lease_release",
        "agent.lease_list",
    ] {
        let m = method_meta(name).unwrap_or_else(|| panic!("registered: {name}"));
        assert!(m.plugin_callable, "{name} should be plugin-callable");
        assert!(
            m.required.contains(&Permission::AgentManage),
            "{name} should require AgentManage"
        );
    }
}

#[test]
fn agent_barrier_semaphore_methods_require_agent_manage() {
    for name in [
        "agent.barrier_create",
        "agent.barrier_signal",
        "agent.barrier_await",
        "agent.barrier_state",
        "agent.semaphore_create",
        "agent.semaphore_acquire",
        "agent.semaphore_release",
    ] {
        let m = method_meta(name).unwrap_or_else(|| panic!("registered: {name}"));
        assert!(m.plugin_callable, "{name} should be plugin-callable");
        assert!(
            m.required.contains(&Permission::AgentManage),
            "{name} should require AgentManage"
        );
    }
}

#[test]
fn agent_rate_limit_methods_require_agent_manage() {
    for name in [
        "agent.rate_limit_set",
        "agent.rate_limit_list",
        "agent.rate_limit_remove",
        "agent.rate_limit_status",
    ] {
        let m = method_meta(name).unwrap_or_else(|| panic!("registered: {name}"));
        assert!(m.plugin_callable, "{name} should be plugin-callable");
        assert!(
            m.required.contains(&Permission::AgentManage),
            "{name} should require AgentManage"
        );
    }
}

#[test]
fn session_issue_revoke_require_agent_manage() {
    for name in ["session.issue", "session.revoke"] {
        let m = method_meta(name).unwrap_or_else(|| panic!("registered: {name}"));
        assert!(m.plugin_callable, "{name} should be plugin-callable");
        assert!(
            m.required.contains(&Permission::AgentManage),
            "{name} should require AgentManage"
        );
    }
}

#[test]
fn session_list_is_local_only() {
    let m = method_meta("session.list").expect("registered");
    assert!(!m.plugin_callable);
}

#[test]
fn agent_grant_revoke_are_local_only() {
    for name in [
        "plugin.grant_agent_permission",
        "plugin.revoke_agent_permission",
    ] {
        let m = method_meta(name).unwrap_or_else(|| panic!("registered: {name}"));
        assert!(!m.plugin_callable, "{name} should be local-only");
    }
}

#[test]
fn agent_list_permissions_is_plugin_readonly() {
    let m = method_meta("plugin.list_agent_permissions").expect("registered");
    assert!(m.plugin_callable);
    assert!(m.required.is_empty(), "list should not require permissions");
}

#[test]
fn request_permission_is_plugin_callable_with_approval() {
    let m = method_meta("plugin.request_permission").expect("registered");
    assert!(m.plugin_callable, "agents must be able to self-request");
    assert!(
        m.required.contains(&Permission::Approval),
        "should require Approval"
    );
}

#[test]
fn plugin_prefix_registration_resolves() {
    let _g = test_lock();
    clear_plugin_prefixes_for_tests();
    register_plugin_prefix("codex");
    let m = method_meta("codex.spawn").expect("registered via runtime");
    assert!(m.plugin_callable);
    assert!(m.required.is_empty());
    clear_plugin_prefixes_for_tests();
}

#[test]
fn plugin_prefix_unregister_removes() {
    let _g = test_lock();
    clear_plugin_prefixes_for_tests();
    register_plugin_prefix("codex");
    unregister_plugin_prefix("codex");
    assert!(method_meta("codex.spawn").is_none());
}

#[test]
fn static_table_wins_over_plugin_prefix() {
    let _g = test_lock();
    clear_plugin_prefixes_for_tests();
    register_plugin_prefix("image");
    let m = method_meta("image.open").expect("static");
    assert!(m.required.contains(&Permission::SurfaceWrite));
    clear_plugin_prefixes_for_tests();
}

#[test]
fn plugin_prefix_idempotent_register() {
    let _g = test_lock();
    clear_plugin_prefixes_for_tests();
    register_plugin_prefix("codex");
    register_plugin_prefix("codex");
    let m = method_meta("codex.spawn").expect("still registered");
    assert!(m.plugin_callable);
    unregister_plugin_prefix("codex");
    assert!(method_meta("codex.spawn").is_none());
    clear_plugin_prefixes_for_tests();
}

#[test]
fn audit_methods_are_local_only() {
    for name in [
        "plugin.audit_query",
        "plugin.audit_summary",
        "plugin.audit_follow",
        "plugin.audit_clear",
    ] {
        let m = method_meta(name).unwrap_or_else(|| panic!("registered: {name}"));
        assert!(!m.plugin_callable, "{name} should be local-only");
    }
}
