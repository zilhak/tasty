//! `method_meta` 단위 테스트.

#![cfg(test)]

use super::*;

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
    assert!(method_meta("ui.screenshot").is_none());
}

#[test]
fn surface_list_requires_surface_read() {
    let m = method_meta("surface.list").expect("registered");
    assert!(m.plugin_callable);
    assert!(m.required.contains(&Permission::SurfaceRead));
}

#[test]
fn tab_create_requires_surface_write() {
    let m = method_meta("tab.create").expect("registered");
    assert!(m.plugin_callable);
    assert!(m.required.contains(&Permission::SurfaceWrite));
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
