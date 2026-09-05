//! `method_meta` 단위 테스트.

use crate::method_meta::{
    METHOD_TABLE, PREFIX_RULES, is_registered_plugin_prefix, method_meta, test_namespace_table,
};
use tasty_plugin_manifest::Permission;

/// 표를 조작하는 테스트가 쓰는 가짜 소유자. 소유는 plugin 단위라 prefix 만으로는
/// 등록할 수 없다 — 표가 답하는 물음이 "누가 소유하나" 이기 때문이다.
const TEST_OWNER: &str = "com.test.namespace";

fn ns_write() -> std::sync::RwLockWriteGuard<'static, crate::ipc_namespace::IpcNamespaceRegistry> {
    test_namespace_table()
        .write()
        .unwrap_or_else(|poison| poison.into_inner())
}

fn ns_clear() {
    ns_write().clear();
}

fn ns_register(prefix: &str) {
    ns_write()
        .register(TEST_OWNER, prefix)
        .expect("test prefix must be free");
}

fn ns_unregister(_prefix: &str) {
    ns_write().unregister_plugin(TEST_OWNER);
}

/// 표가 process-global 이라 동일 binary 안에서 병렬 test 가
/// 표를 동시 변형하지 못하게 직렬화.
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

/// 모든 등록 메서드는 명명 규칙을 따라야 한다 (docs/dev-guide/api-conventions.md):
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

/// 무대 debug IPC 4 종은 전부 plugin 에 노출되지 않는다. 무대는 창 전체를 덮는
/// 화면 점유라 plugin 이 열 수 있으면 사용자 화면을 가로챌 수 있다.
#[test]
#[cfg(debug_assertions)]
fn fullscreen_debug_methods_are_local_only() {
    for name in [
        "debug.fullscreen.list",
        "debug.fullscreen.open",
        "debug.fullscreen.close",
        "debug.fullscreen.state",
    ] {
        let m = method_meta(name).unwrap_or_else(|| panic!("{name} registered (debug build)"));
        assert!(!m.plugin_callable, "{name} must be local_only");
    }
}

/// release 에는 무대 debug IPC 가 아예 없어야 한다 — `#[cfg(debug_assertions)]`
/// 격리가 메타 테이블까지 일관되게 적용됐는지 확인한다.
#[test]
#[cfg(not(debug_assertions))]
fn fullscreen_debug_methods_absent_in_release() {
    for name in [
        "debug.fullscreen.list",
        "debug.fullscreen.open",
        "debug.fullscreen.close",
        "debug.fullscreen.state",
    ] {
        assert!(
            method_meta(name).is_none(),
            "{name} must not exist in release"
        );
    }
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

/// `clipboard.set_text` — release `METHOD_TABLE` 에 존재하고 plugin_callable +
/// `Permission::ClipboardWrite` 필수여야 한다 (원격 mirror 캡처를 원격 인스턴스의
/// clipboard.set_text 로 반영하는 attach 전송 경로도 이 등록을 사용).
#[test]
fn clipboard_set_text_is_release() {
    let m = method_meta("clipboard.set_text").expect("registered in release METHOD_TABLE");
    assert!(m.plugin_callable);
    assert!(m.required.contains(&Permission::ClipboardWrite));
}

#[test]
fn surface_list_requires_surface_read() {
    let m = method_meta("surface.list").expect("registered");
    assert!(m.plugin_callable);
    assert!(m.required.contains(&Permission::SurfaceRead));
}

/// ADR-0058: `file_picker.trigger` 는 `git_viewer.query` 와
/// 동일 근거(파일을 고르는 read 관심사)로 FsRead 권한이 필요하고, plugin 이 직접
/// host.call 로 호출 가능해야 한다.
#[test]
fn file_picker_trigger_requires_fs_read() {
    let m = method_meta("file_picker.trigger").expect("registered");
    assert!(m.plugin_callable);
    assert!(m.required.contains(&Permission::FsRead));
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
        "terminal.children",
        "terminal.parent",
        "terminal.kill",
        "terminal.respawn",
        "terminal.broadcast",
        "terminal.set_state",
        "terminal.adopt",
        "terminal.release",
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
        "agent.task_cancel",
        "agent.task_retry",
        "agent.task_graph",
        "agent.dag_list",
        "agent.dag_get",
        "agent.task_run",
        "agent.task_delete",
        "agent.task_purge",
    ] {
        let m = method_meta(name).unwrap_or_else(|| panic!("registered: {name}"));
        assert!(m.plugin_callable, "{name} should be plugin-callable");
        assert!(
            m.required.contains(&Permission::AgentManage),
            "{name} should require AgentManage"
        );
    }
}

// approval.await 와 대칭 — 진짜 blocking 이라 plugin 의 단일 워커 스레드를 막을
// 위험이 있어 local caller 전용으로 닫는다.
#[test]
fn agent_task_await_is_local_only() {
    let m = method_meta("agent.task_await").expect("registered");
    assert!(!m.plugin_callable);
}

/// 러너가 Custom task 의 생명주기를 단독 소유한다 — plugin 이 task_set_result
/// 로 같은 task 를 별도 전이시키면 쓰기 주체가 이중화돼 러너의 완료 판정과
/// 경합한다. plugin 은 완료 판정 전략 선언으로 우회한다(agent.task_await 와
/// 같은 이유 계열). 등재 자체는 "누락"과 "의도적 local_only" 를 구분하기 위한
/// 것이다 — 미등재면 plugin 호출자가 UnknownMethod 로 거부돼, 표를 읽는 쪽이
/// 정책인지 실수인지 판별할 수 없다.
#[test]
fn agent_task_set_result_is_local_only() {
    let m = method_meta("agent.task_set_result").expect("registered");
    assert!(!m.plugin_callable);
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
    ns_clear();
    ns_register("codex");
    let m = method_meta("codex.spawn").expect("registered via runtime");
    assert!(m.plugin_callable);
    assert!(m.required.is_empty());
    ns_clear();
}

#[test]
fn plugin_prefix_unregister_removes() {
    let _g = test_lock();
    ns_clear();
    ns_register("codex");
    ns_unregister("codex");
    assert!(method_meta("codex.spawn").is_none());
}

#[test]
fn static_table_wins_over_plugin_prefix() {
    let _g = test_lock();
    ns_clear();
    ns_register("image");
    let m = method_meta("image.open").expect("static");
    assert!(m.required.contains(&Permission::SurfaceWrite));
    ns_clear();
}

#[test]
fn plugin_prefix_idempotent_register() {
    let _g = test_lock();
    ns_clear();
    ns_register("codex");
    ns_register("codex");
    let m = method_meta("codex.spawn").expect("still registered");
    assert!(m.plugin_callable);
    ns_unregister("codex");
    assert!(method_meta("codex.spawn").is_none());
    ns_clear();
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

/// registry 락이 poison 돼도 소유자 우회 차단이 계속 선다.
///
/// 조준점이 **등록된** prefix 인 것이 이 테스트의 요점이다. 미등록 prefix 로 겨누면
/// 복구를 지워도 `unwrap_or(false)` 가 우연히 같은 답을 내서 변이가 살아남는다.
/// 호출부(`method_allowed_for_owner`)가 이 함수를 `!` 로 뒤집어 쓰므로, poison 시
/// `false` 를 돌려주는 것은 곧 Host/User 가 남의 plugin namespace 를 부르게 열어
/// 주는 것이다 — 이 함수가 애초에 막으려던 우회 그 자체다.
///
/// poison 은 sticky 라 이 테스트 이후 같은 바이너리의 모든 접근이 복구 경로를 지난다.
/// 그래도 다른 테스트가 깨지지 않는다는 것이 곧 복구가 자리잡았다는 증거다.
#[test]
fn a_poisoned_prefix_registry_still_blocks_the_owner_bypass() {
    let _g = test_lock();
    ns_clear();

    let panicked = std::thread::spawn(|| {
        let _held = test_namespace_table().write().expect("not poisoned yet");
        panic!("poison the prefix registry");
    })
    .join();
    assert!(panicked.is_err(), "the helper thread must have panicked");
    assert!(
        test_namespace_table().read().is_err(),
        "the registry lock must actually be poisoned now"
    );

    // 등록도 poison 이후에 한다 — 그래야 읽기 경로와 쓰기 경로가 **둘 다** 겨냥된다.
    ns_register("codex");
    assert!(
        is_registered_plugin_prefix("codex"),
        "a poisoned registry must not report a registered prefix as free — that opens the \
         very owner bypass this check exists to close"
    );
    assert!(
        method_meta("codex.spawn")
            .expect("still resolves")
            .plugin_callable,
        "prefix lookup must survive the poison too"
    );

    ns_unregister("codex");
    assert!(
        !is_registered_plugin_prefix("codex"),
        "writes must land on a poisoned registry as well, or a dead plugin keeps its namespace"
    );
    ns_clear();
}

/// 종단 응답의 셋째 갈래는 **정확 표 조회**로 갈린다 — prefix fallback 을 타면 안 된다.
///
/// [`method_meta`] 는 마지막 단계에서 런타임 등록 plugin prefix 까지 해소하므로, 그것으로
/// `unrouted_for_external_caller` 의 갈래를 태우면 설치된 plugin 의 이름과 그 아래 오타까지
/// host 가 삼킨다 — 실측 2026-09-05 로 `claude.children` · `agent_stream.list` ·
/// `markdown.no_such_thing` 이 전부 `-32017` 이 되고, plugin 으로 갈 호출이 안 갔다.
/// 근거는 [ADR-0167](../../../docs/adr/0167-a-registered-name-answers-whether-it-is-in-this-binary.md).
///
/// 이 테스트가 `method_meta_tests.rs` 에 사는 이유는 **런타임 prefix 레지스트리를 만지기
/// 때문**이다. 그 전역을 만지는 테스트는 이 파일의 `TEST_LOCK` 을 잡아야 한다.
#[test]
fn the_unrouted_third_branch_asks_the_exact_table_not_the_prefix_fallback() {
    let _g = test_lock();
    ns_clear();
    ns_register("zzztestns");

    let name = "zzztestns.whatever";
    assert!(
        method_meta(name).is_some(),
        "prefix 등록이 안 먹었다 — 이 테스트의 대조군이 죽었다"
    );
    assert!(
        !crate::method_meta::is_registered_name(name),
        "prefix fallback 을 '표에 있다' 로 셌다 — plugin 표면을 host 가 삼킨다"
    );
    let resp =
        crate::protocol::JsonRpcResponse::unrouted_for_external_caller(serde_json::json!(1), name);
    assert_eq!(
        resp.error.expect("에러여야 한다").code,
        -32601,
        "plugin namespace 이름이 -32601 이 아니면 헤드리스 forward 가 안 탄다"
    );

    // 대조군의 반대편 — 표에 그 이름 그대로 있는 것은 셋째 갈래를 탄다.
    let resp = crate::protocol::JsonRpcResponse::unrouted_for_external_caller(
        serde_json::json!(1),
        "window.create",
    );
    assert_eq!(resp.error.expect("에러여야 한다").code, -32017);

    ns_unregister("zzztestns");
}
