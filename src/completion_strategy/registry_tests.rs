//! `CompletionStrategyRegistry` 단위 테스트 (TODO80 §B, 훅 핸들러
//! `registry_tests.rs` 미러). Settings UI CRUD 표면이 아직 없으므로 export/save
//! 류는 다루지 않는다 — 3출처 병합·patch semantics·id 규약·결정 2(namespace 제한)·
//! push 참조 무결성·결정 6(default_for_methods 충돌)·plugin uninstall·이름 해석
//! 을 커버한다.

use super::*;
use crate::completion_strategy::types::CompletionStrategyId;
use crate::hook_handler::types::{
    HookHandler, HookHandlerAction, HookHandlerId, HookHandlerOwner, HookSource,
};

fn poll_toml(id: &str, priority: i32, method: &str, default_for: &str) -> String {
    format!(
        r#"
        [[strategy]]
        id = "{id}"
        priority = {priority}
        default_for_methods = [{default_for}]
        [strategy.spec]
        kind = "poll"
        poll_method = "{method}"
        state_field = "state"
        terminal_states = ["idle", "needs_input"]
        "#
    )
}

/// 훅 핸들러 전역 레지스트리에 `id` 를 IpcSequence 핸들러로 upsert한다 — push 형
/// notify_via 참조 무결성 테스트용(전역 싱글턴이라 process 전체에서 공유되지만,
/// 이 테스트 파일이 쓰는 id 는 다른 곳과 충돌하지 않는 고유 접두어로 고른다).
fn ensure_hook_handler(id: &str, owner: HookHandlerOwner) {
    crate::hook_handler::global()
        .upsert_full_handler(HookHandler {
            id: HookHandlerId::new(id),
            source: HookSource::Hook,
            priority: 100,
            owner,
            action: HookHandlerAction::IpcSequence { calls: vec![] },
            display_name_i18n_key: None,
            disabled: false,
        })
        .expect("test hook handler upsert");
}

// ── host defaults / 기본 install ────────────────────────────────────────

#[test]
fn plugin_poll_strategy_installs_and_resolves() {
    let reg = CompletionStrategyRegistry::new();
    let decls = parse_strategy_section(&poll_toml(
        "spawn-wait",
        100,
        "acme.wait",
        r#""acme.spawn""#,
    ))
    .expect("parse");
    reg.install_plugin_strategies("acme", &decls);

    let id = CompletionStrategyId::new("acme/spawn-wait");
    let s = reg.get(&id).expect("strategy");
    assert_eq!(s.owner, CompletionStrategyOwner::Plugin("acme".into()));
    assert_eq!(s.priority, 100);
    assert_eq!(s.default_for_methods, vec!["acme.spawn".to_string()]);
    match &s.kind {
        CompletionStrategyKind::Poll(spec) => {
            assert_eq!(spec.poll_method, "acme.wait");
            assert_eq!(spec.interval_ms, 500); // §A-5 기본값
        }
        CompletionStrategyKind::Push { .. } => panic!("expected poll"),
    }

    // 이름 해석(§B 체크리스트 "이름 → 실제 사양 해석").
    let spec = reg.resolve_poll_spec(&id).expect("resolve");
    assert_eq!(spec.poll_method, "acme.wait");

    // 결정 6 — default_for_methods 인덱스 조회.
    let winner = reg
        .resolve_default_for_method("acme.spawn")
        .expect("default");
    assert_eq!(winner.id, id);
}

/// TOML → `CompletionStrategySpecDecl::Poll(PollStrategyDecl)` → 레지스트리
/// install/finalize/resolve 전 구간이 필드를 안 잃고 통과하는지 확인하는
/// **배선(wiring) 스모크 테스트**다 — poll 필드 하나하나의 이름·기본값 대응
/// 자체는 `completion_strategy_to_poll_spec()`(`src/core/agent/
/// completion_strategy.rs::field_correspondence_is_preserved`)가 이미 단일
/// 지점에서 고정한다(TODO80 §A-3, Gate4 리뷰 지적으로 중복 제거). 여기서는
/// map/scalar 필드 하나씩만 대표로 확인해 "레지스트리 경로가 그 함수를 실제로
/// 거치는지"만 검증한다.
#[test]
fn poll_decl_survives_registry_install_and_resolve() {
    let toml = r#"
        [[strategy]]
        id = "full-map"
        priority = 5
        [strategy.spec]
        kind = "poll"
        poll_method = "acme.wait"
        map_from_response = { child_index = "child_index" }
        state_field = "st"
        terminal_states = ["done"]
        timeout_ms = 9000
    "#;
    let decls = parse_strategy_section(toml).expect("parse");
    let reg = CompletionStrategyRegistry::new();
    reg.install_plugin_strategies("acme", &decls);
    let spec = reg
        .resolve_poll_spec(&CompletionStrategyId::new("acme/full-map"))
        .expect("resolve");
    assert_eq!(spec.poll_method, "acme.wait");
    assert_eq!(
        spec.map_from_response.get("child_index").unwrap(),
        "child_index"
    );
    assert_eq!(spec.timeout_ms, Some(9000));
}

// ── 결정 2 — namespace 제한 ─────────────────────────────────────────────

#[test]
fn plugin_poll_method_outside_own_namespace_is_dropped() {
    let reg = CompletionStrategyRegistry::new();
    // "acme" plugin 이 "other.wait" 를 poll_method 로 선언 — 자기 namespace 아님.
    let decls = parse_strategy_section(&poll_toml("evil", 100, "other.wait", "")).expect("parse");
    reg.install_plugin_strategies("acme", &decls);
    assert!(reg.get(&CompletionStrategyId::new("acme/evil")).is_none());
}

#[test]
fn host_poll_method_inside_registered_plugin_prefix_is_dropped() {
    tasty_ipc::method_meta::register_plugin_prefix("cstest_acme");
    let reg = CompletionStrategyRegistry::new();
    reg.install_host_defaults(&poll_toml("h1", 100, "cstest_acme.wait", ""));
    assert!(reg.get(&CompletionStrategyId::new("host/h1")).is_none());
    tasty_ipc::method_meta::unregister_plugin_prefix("cstest_acme");
}

#[test]
fn host_poll_method_outside_any_plugin_prefix_is_kept() {
    let reg = CompletionStrategyRegistry::new();
    reg.install_host_defaults(&poll_toml("h2", 100, "terminal.child_state", ""));
    assert!(reg.get(&CompletionStrategyId::new("host/h2")).is_some());
}

// ── push 참조 무결성 ─────────────────────────────────────────────────────

fn push_toml(id: &str, notify_via: &str) -> String {
    format!(
        r#"
        [[strategy]]
        id = "{id}"
        priority = 100
        [strategy.spec]
        kind = "push"
        notify_via = "{notify_via}"
        timeout_ms = 5000
        "#
    )
}

#[test]
fn push_strategy_missing_hook_handler_is_dropped() {
    let reg = CompletionStrategyRegistry::new();
    let decls = parse_strategy_section(&push_toml("orphan", "acme/does-not-exist")).expect("parse");
    reg.install_plugin_strategies("acme", &decls);
    assert!(reg.get(&CompletionStrategyId::new("acme/orphan")).is_none());
}

#[test]
fn push_strategy_self_owned_hook_handler_resolves() {
    ensure_hook_handler(
        "cstest-plugin/notify-self",
        HookHandlerOwner::Plugin("cstest-plugin".into()),
    );
    let reg = CompletionStrategyRegistry::new();
    let decls = parse_strategy_section(&push_toml("notify-strategy", "cstest-plugin/notify-self"))
        .expect("parse");
    reg.install_plugin_strategies("cstest-plugin", &decls);
    let s = reg
        .get(&CompletionStrategyId::new("cstest-plugin/notify-strategy"))
        .expect("kept");
    match s.kind {
        CompletionStrategyKind::Push { timeout_ms, .. } => assert_eq!(timeout_ms, 5000),
        CompletionStrategyKind::Poll(_) => panic!("expected push"),
    }
}

#[test]
fn push_strategy_host_owned_hook_handler_resolves() {
    ensure_hook_handler("host/cstest-notify", HookHandlerOwner::Host);
    let reg = CompletionStrategyRegistry::new();
    let decls =
        parse_strategy_section(&push_toml("via-host", "host/cstest-notify")).expect("parse");
    reg.install_plugin_strategies("cstest-plugin2", &decls);
    assert!(
        reg.get(&CompletionStrategyId::new("cstest-plugin2/via-host"))
            .is_some()
    );
}

#[test]
fn push_strategy_other_plugin_owned_hook_handler_is_rejected() {
    ensure_hook_handler(
        "cstest-other/notify",
        HookHandlerOwner::Plugin("cstest-other".into()),
    );
    let reg = CompletionStrategyRegistry::new();
    let decls = parse_strategy_section(&push_toml("cross", "cstest-other/notify")).expect("parse");
    reg.install_plugin_strategies("cstest-plugin3", &decls);
    // owner 가 자기 자신도 host 도 아니므로 drop.
    assert!(
        reg.get(&CompletionStrategyId::new("cstest-plugin3/cross"))
            .is_none()
    );
}

// ── 결정 6 — default_for_methods 충돌 ───────────────────────────────────

#[test]
fn default_for_methods_conflict_picks_lower_priority_winner() {
    let reg = CompletionStrategyRegistry::new();
    let low = parse_strategy_section(&poll_toml("low-prio", 10, "acme.wait_a", r#""acme.spawn""#))
        .expect("parse");
    let high = parse_strategy_section(&poll_toml(
        "high-prio",
        200,
        "acme.wait_b",
        r#""acme.spawn""#,
    ))
    .expect("parse");
    reg.install_plugin_strategies("acme", &low);
    reg.install_plugin_strategies("acme", &high);
    let winner = reg
        .resolve_default_for_method("acme.spawn")
        .expect("winner");
    assert_eq!(winner.id, CompletionStrategyId::new("acme/low-prio"));
}

// ── plugin uninstall ─────────────────────────────────────────────────────

#[test]
fn uninstall_plugin_removes_its_strategies() {
    let reg = CompletionStrategyRegistry::new();
    let decls = parse_strategy_section(&poll_toml("temp", 100, "acme.wait", "")).expect("parse");
    reg.install_plugin_strategies("acme", &decls);
    assert!(reg.get(&CompletionStrategyId::new("acme/temp")).is_some());
    reg.uninstall_plugin("acme");
    assert!(reg.get(&CompletionStrategyId::new("acme/temp")).is_none());
}

// ── 이름 해석 실패 사유 ───────────────────────────────────────────────────

#[test]
fn resolve_poll_spec_not_found() {
    let reg = CompletionStrategyRegistry::new();
    let err = reg
        .resolve_poll_spec(&CompletionStrategyId::new("acme/nope"))
        .unwrap_err();
    assert!(matches!(err, StrategyResolveError::NotFound { .. }));
}

#[test]
fn resolve_poll_spec_rejects_push_kind() {
    ensure_hook_handler(
        "cstest-plugin4/n",
        HookHandlerOwner::Plugin("cstest-plugin4".into()),
    );
    let reg = CompletionStrategyRegistry::new();
    let decls = parse_strategy_section(&push_toml("push-one", "cstest-plugin4/n")).expect("parse");
    reg.install_plugin_strategies("cstest-plugin4", &decls);
    let err = reg
        .resolve_poll_spec(&CompletionStrategyId::new("cstest-plugin4/push-one"))
        .unwrap_err();
    assert!(matches!(err, StrategyResolveError::NotPollKind { .. }));
}

#[test]
fn resolve_poll_spec_rejects_disabled() {
    let reg = CompletionStrategyRegistry::new();
    let toml = r#"
        [[strategy]]
        id = "disabled-one"
        priority = 100
        disabled = true
        [strategy.spec]
        kind = "poll"
        poll_method = "acme.wait"
        state_field = "state"
        terminal_states = ["done"]
    "#;
    let decls = parse_strategy_section(toml).expect("parse");
    reg.install_plugin_strategies("acme", &decls);
    let err = reg
        .resolve_poll_spec(&CompletionStrategyId::new("acme/disabled-one"))
        .unwrap_err();
    assert!(matches!(err, StrategyResolveError::Disabled { .. }));
}

/// 실제 host defaults TOML 두 파일(`hook_handler`/
/// `completion_strategy` 각자의 `defaults/`)이 서로의 참조를 만족하는지 확인하는
/// 배선 스모크 테스트. `notify_via = "host/command-completed"` 가 가리키는 훅
/// 핸들러가 실제로 존재해야 이 전략이 조용히 drop 되지 않는다(§B-5).
#[test]
fn host_command_completed_default_strategy_resolves_after_hook_handler_registered() {
    // hook_handler 전역에 실제 host defaults 를 설치 — `host/command-completed`
    // 핸들러가 여기서 나온다. 다른 테스트와 공유하는 프로세스 전역이지만
    // install 은 idempotent(같은 owner 재설치는 덮어씀)이라 순서 무관 안전.
    crate::hook_handler::global().install_host_defaults(include_str!(
        "../hook_handler/defaults/default-hook-handlers.toml"
    ));

    let reg = CompletionStrategyRegistry::new();
    reg.install_host_defaults(include_str!("defaults/default-completion-strategies.toml"));

    let strat = reg
        .resolve_strategy(&CompletionStrategyId::new("host/command-completed"))
        .expect("host/command-completed should resolve now that its hook handler exists");
    match strat.kind {
        CompletionStrategyKind::Push {
            notify_via,
            timeout_ms,
        } => {
            assert_eq!(notify_via.as_str(), "host/command-completed");
            assert_eq!(timeout_ms, 300_000);
        }
        CompletionStrategyKind::Poll(_) => panic!("expected push"),
    }
}

// ── resolve_strategy (kind-agnostic) ─────────────────────────────────────

#[test]
fn resolve_strategy_returns_poll_kind() {
    let reg = CompletionStrategyRegistry::new();
    let decls =
        parse_strategy_section(&poll_toml("spawn-wait", 100, "acme.wait", "")).expect("parse");
    reg.install_plugin_strategies("acme", &decls);

    let s = reg
        .resolve_strategy(&CompletionStrategyId::new("acme/spawn-wait"))
        .expect("resolve");
    assert!(matches!(s.kind, CompletionStrategyKind::Poll(_)));
}

#[test]
fn resolve_strategy_returns_push_kind_unlike_resolve_poll_spec() {
    ensure_hook_handler(
        "cstest-resolve-strategy/h",
        HookHandlerOwner::Plugin("cstest-resolve-strategy".into()),
    );
    let reg = CompletionStrategyRegistry::new();
    let decls =
        parse_strategy_section(&push_toml("push-one", "cstest-resolve-strategy/h")).expect("parse");
    reg.install_plugin_strategies("cstest-resolve-strategy", &decls);

    let id = CompletionStrategyId::new("cstest-resolve-strategy/push-one");
    // resolve_poll_spec 은 push-kind 를 거부하지만(위 rejects_push_kind 테스트),
    // kind-agnostic resolve_strategy 는 poll/push 를 가리지 않고 반환한다.
    let s = reg.resolve_strategy(&id).expect("resolve");
    assert!(matches!(s.kind, CompletionStrategyKind::Push { .. }));
}

#[test]
fn resolve_strategy_not_found() {
    let reg = CompletionStrategyRegistry::new();
    let err = reg
        .resolve_strategy(&CompletionStrategyId::new("acme/nope"))
        .unwrap_err();
    assert!(matches!(err, StrategyResolveError::NotFound { .. }));
}

#[test]
fn resolve_strategy_rejects_disabled() {
    let reg = CompletionStrategyRegistry::new();
    let toml = r#"
        [[strategy]]
        id = "disabled-one"
        priority = 100
        disabled = true
        [strategy.spec]
        kind = "poll"
        poll_method = "acme.wait"
        state_field = "state"
        terminal_states = ["done"]
    "#;
    let decls = parse_strategy_section(toml).expect("parse");
    reg.install_plugin_strategies("acme", &decls);
    let err = reg
        .resolve_strategy(&CompletionStrategyId::new("acme/disabled-one"))
        .unwrap_err();
    assert!(matches!(err, StrategyResolveError::Disabled { .. }));
}

#[test]
fn plugin_display_name_i18n_key_is_preserved() {
    let toml = r#"
        [[strategy]]
        id = "with-label"
        priority = 100
        display_name_i18n_key = "acme.strategy.with_label"
        [strategy.spec]
        kind = "poll"
        poll_method = "acme.wait"
        state_field = "state"
        terminal_states = ["done"]
    "#;
    let decls = parse_strategy_section(toml).expect("parse");
    let reg = CompletionStrategyRegistry::new();
    reg.install_plugin_strategies("acme", &decls);
    let s = reg
        .get(&CompletionStrategyId::new("acme/with-label"))
        .expect("kept");
    assert_eq!(
        s.display_name_i18n_key.as_deref(),
        Some("acme.strategy.with_label")
    );
}

// ── patch semantics ───────────────────────────────────────────────────────

// ── 실증 — 번들 claude/codex 매니페스트가 실제로 결정 6 을 통과하는지 ──
//
// 배경(회귀 방지): plugin owner 를 매니페스트의 reverse-DNS `id`("com.tasty.claude")
// 로 넘기면 poll_method("claude.state")의 dot-prefix("claude")와 절대 문자열이
// 일치하지 않아 `method_allowed_for_owner`(결정 2)가 항상 drop 시킨다. 위쪽의
// `plugin_poll_strategy_installs_and_resolves` 류 테스트는 `install_plugin_strategies`
// 의 owner 인자로 처음부터 짧은 "acme" 를 직접 넘겨 이 불일치를 우연히 피해가므로
// 실제 plugin 배선의 버그를 못 잡는다 — 여기서는 `lifecycle.rs::completion_strategy_
// owner_id`(첫 `ipc_namespace` prefix, 없으면 manifest id 폴백)와 동일한 규칙으로
// owner 를 유도해, 번들 매니페스트가 실제로 그 규칙을 통과하는지 확인한다.
fn owner_id_for(m: &tasty_plugin_manifest::Manifest) -> &str {
    m.contributes
        .ipc_namespace
        .first()
        .map(|ns| ns.prefix.as_str())
        .unwrap_or(m.id.as_str())
}

fn install_bundled_manifest_strategies(plugin_dir: &str) -> (CompletionStrategyRegistry, String) {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("crates")
        .join(plugin_dir);
    let m = tasty_plugin_manifest::Manifest::load(&path).expect("bundled manifest should load");
    let owner_id = owner_id_for(&m).to_string();
    let decls: Vec<CompletionStrategyDecl> = m
        .contributes
        .completion_strategy
        .iter()
        .map(|v| {
            serde_json::from_value(v.clone()).expect("bundled completion_strategy decl parses")
        })
        .collect();
    assert!(
        !decls.is_empty(),
        "{plugin_dir} manifest should declare at least one completion strategy"
    );
    let reg = CompletionStrategyRegistry::new();
    reg.install_plugin_strategies(&owner_id, &decls);
    (reg, owner_id)
}

#[test]
fn bundled_claude_manifest_spawn_default_strategy_resolves() {
    let (reg, owner_id) = install_bundled_manifest_strategies("tasty-plugin-claude");
    assert_eq!(owner_id, "claude");
    let winner = reg
        .resolve_default_for_method("claude.spawn")
        .expect("claude.spawn should resolve a default completion strategy (decision 6)");
    match winner.kind {
        CompletionStrategyKind::Poll(spec) => assert_eq!(spec.poll_method, "claude.state"),
        CompletionStrategyKind::Push { .. } => panic!("expected poll"),
    }
}

#[test]
fn bundled_codex_manifest_spawn_default_strategy_resolves() {
    let (reg, owner_id) = install_bundled_manifest_strategies("tasty-plugin-codex");
    assert_eq!(owner_id, "codex");
    let winner = reg
        .resolve_default_for_method("codex.spawn")
        .expect("codex.spawn should resolve a default completion strategy (decision 6)");
    match winner.kind {
        CompletionStrategyKind::Poll(spec) => assert_eq!(spec.poll_method, "codex.state"),
        CompletionStrategyKind::Push { .. } => panic!("expected poll"),
    }
}

#[test]
fn user_override_patches_priority_only() {
    let dir = tempfile::tempdir().unwrap();
    let reg = CompletionStrategyRegistry::new();
    let decls = parse_strategy_section(&poll_toml("patchme", 100, "acme.wait", "")).expect("parse");
    reg.install_plugin_strategies("acme", &decls);

    let p = dir.path().join("completion-strategies.toml");
    std::fs::write(
        &p,
        r#"
        [[strategy]]
        id = "acme/patchme"
        priority = 1
        "#,
    )
    .unwrap();
    reg.install_user_config(&p);

    let s = reg
        .get(&CompletionStrategyId::new("acme/patchme"))
        .expect("kept");
    assert_eq!(s.priority, 1); // user 가 덮음
    assert_eq!(s.owner, CompletionStrategyOwner::User); // 마지막 contributor 로 owner 갱신(훅 핸들러와 동일 동작)
    match s.kind {
        CompletionStrategyKind::Poll(spec) => assert_eq!(spec.poll_method, "acme.wait"), // plugin spec 유지
        CompletionStrategyKind::Push { .. } => panic!("expected poll"),
    }
}
