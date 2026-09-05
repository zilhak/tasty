//! Plugin IPC handlers — `App`이 `PluginManager`를 들고 있으므로 일반 핸들러 라우팅
//! (`&mut AppState`)와 별도로, `App::process_ipc`에서 직접 호출된다.

use serde_json::{Value, json};

use crate::plugin::PluginManager;
use tasty_ipc::protocol::JsonRpcResponse;

/// 매니저가 아직 없으면 **에러로 답한다** — 빈 목록을 성공으로 돌려주지 않는다.
///
/// 이 파일의 다른 세 핸들러(`handle_show` / `handle_extension_list` /
/// `handle_permissions`)가 모두 그렇게 하고, 여기만 `Vec::new()` 로 갈라져 있었다.
/// 빈 목록을 성공으로 주면 "설치된 plugin 이 없다" 와 "아직 매니저를 안 띄웠다" 가
/// 같은 응답이 되는데, 헤드리스 데몬은 plugin 메서드가 한 번 forward 되기 전까지
/// 매니저가 `None` 인 것이 기본값이라(`src/boot/headless_dispatch.rs` 의 lazy 기동)
/// 그 혼동이 실제로 일어난다. 호출자가 그 둘을 가를 수 없는 답은 없느니만 못하다.
pub fn handle_list(mgr: Option<&PluginManager>, id: Value) -> JsonRpcResponse {
    let mgr = match mgr {
        Some(m) => m,
        None => return JsonRpcResponse::error(id, -32000, "plugin manager not initialized"),
    };
    let arr: Vec<Value> = mgr
        .packages
        .iter()
        .map(|p| {
            json!({
                "id": p.manifest.id,
                "name": p.manifest.name,
                "version": p.manifest.version,
                "description": p.manifest.description,
                "enabled": !mgr.config.is_disabled(&p.manifest.id),
                "running": mgr.is_running(&p.manifest.id),
                "surface_kinds": p.manifest.surface_kinds.iter().map(|k| &k.kind).collect::<Vec<_>>(),
                "log_path": mgr.log_path(&p.manifest.id).to_string_lossy(),
            })
        })
        .collect();
    JsonRpcResponse::success(id, json!({ "plugins": arr }))
}

pub fn handle_show(mgr: Option<&PluginManager>, id: Value, params: &Value) -> JsonRpcResponse {
    let mgr = match mgr {
        Some(m) => m,
        None => return JsonRpcResponse::error(id, -32000, "plugin manager not initialized"),
    };
    let plugin_id = match params.get("id").and_then(|v| v.as_str()) {
        Some(s) => s.to_string(),
        None => return JsonRpcResponse::invalid_params(id, "Missing 'id' parameter"),
    };
    let pkg = match mgr.packages.iter().find(|p| p.manifest.id == plugin_id) {
        Some(p) => p,
        None => {
            return JsonRpcResponse::error(
                id,
                -32003,
                format!("plugin '{plugin_id}' not installed"),
            );
        }
    };
    let manifest = &pkg.manifest;

    let granted: Vec<String> = mgr
        .config
        .granted_permissions(&plugin_id)
        .into_iter()
        .collect();

    let surface_kinds: Vec<Value> = manifest
        .surface_kinds
        .iter()
        .map(|k| {
            json!({
                "kind": k.kind,
                "display_name_i18n_key": k.display_name_i18n_key,
                "icon": k.icon,
                "rendering": format!("{:?}", k.rendering).to_lowercase(),
            })
        })
        .collect();

    let events_emitted: Vec<Value> = manifest
        .events_emitted
        .iter()
        .map(|e| {
            json!({
                "key": e.key,
                "description": e.description,
                "stability": format!("{:?}", e.stability).to_lowercase(),
                "payload_schema": e.payload_schema,
            })
        })
        .collect();

    let hook_events: Vec<Value> = manifest
        .contributes
        .hook_events
        .iter()
        .map(|h| {
            json!({
                "key": h.key,
                "description": h.description,
                "stability": format!("{:?}", h.stability).to_lowercase(),
            })
        })
        .collect();

    let commands: Vec<Value> = manifest
        .contributes
        .commands
        .iter()
        .map(|c| {
            let override_repr =
                mgr.config
                    .shortcut_override(&plugin_id, &c.id)
                    .map(|ov| match ov {
                        crate::plugin::registry_state::ShortcutOverride::Key { value } => {
                            json!({ "mode": "key", "value": value })
                        }
                        crate::plugin::registry_state::ShortcutOverride::Inherit { source } => {
                            json!({ "mode": "inherit", "source": source })
                        }
                        crate::plugin::registry_state::ShortcutOverride::None => {
                            json!({ "mode": "none" })
                        }
                    });
            json!({
                "id": c.id,
                "title_i18n_key": c.title_i18n_key,
                "scope": format!("{:?}", c.scope).to_lowercase(),
                "binding_mode": format!("{:?}", c.binding_mode).to_lowercase(),
                "default_keybinding": c.default_keybinding,
                "shortcut_override": override_repr,
            })
        })
        .collect();

    let menu_items: Vec<Value> = manifest
        .contributes
        .menu_items
        .iter()
        .map(|m| json!({ "menu": m.menu, "command": m.command, "when": m.when }))
        .collect();

    let ipc_namespace: Vec<Value> = manifest
        .contributes
        .ipc_namespace
        .iter()
        .map(|n| json!({ "prefix": n.prefix }))
        .collect();

    let extension_state = mgr.extension_state(&plugin_id).map(extension_state_to_json);

    let extends = manifest.extends.as_ref().map(|d| {
        let to_event_hook = |h: &crate::plugin::manifest::EventHookDecl| {
            json!({
                "event": h.event,
                "modifies": h.modifies,
                "mode": format!("{:?}", h.mode).to_lowercase(),
                "timeout_ms": h.timeout_ms,
            })
        };
        let to_ipc_hook = |h: &crate::plugin::manifest::IpcHookDecl| {
            json!({
                "method": h.method,
                "modifies": h.modifies,
                "mode": format!("{:?}", h.mode).to_lowercase(),
                "timeout_ms": h.timeout_ms,
            })
        };
        json!({
            "plugin_id": d.plugin_id,
            "version_req": d.version_req,
            "api_version": d.api_version,
            "pre_event": d.pre_event.iter().map(to_event_hook).collect::<Vec<_>>(),
            "post_event": d.post_event.iter().map(to_event_hook).collect::<Vec<_>>(),
            "pre_ipc": d.pre_ipc.iter().map(to_ipc_hook).collect::<Vec<_>>(),
            "post_ipc": d.post_ipc.iter().map(to_ipc_hook).collect::<Vec<_>>(),
        })
    });

    let cli: Vec<Value> = manifest
        .contributes
        .cli
        .iter()
        .map(|c| {
            json!({
                "name": c.name,
                "description": c.description,
                "subcommands": c.subcommands.iter().map(|s| {
                    json!({ "name": s.name, "ipc_method": s.ipc_method, "description": s.description })
                }).collect::<Vec<_>>(),
            })
        })
        .collect();

    JsonRpcResponse::success(
        id,
        json!({
            "id": manifest.id,
            "name": manifest.name,
            "version": manifest.version,
            "description": manifest.description,
            "authors": manifest.authors,
            "homepage": manifest.homepage,
            "api_version": manifest.api_version,
            "manifest_version": manifest.manifest_version,
            "dir": pkg.dir.to_string_lossy(),
            "enabled": !mgr.config.is_disabled(&plugin_id),
            "running": mgr.is_running(&plugin_id),
            "log_path": mgr.log_path(&plugin_id).to_string_lossy(),
            "permissions": {
                "manifest": manifest.permissions,
                "granted": granted,
            },
            "event_subscribe": manifest.event_subscribe,
            "event_publish": manifest.event_publish,
            "events_emitted": events_emitted,
            "hook_events": hook_events,
            "surface_kinds": surface_kinds,
            "commands": commands,
            "menu_items": menu_items,
            "ipc_namespace": ipc_namespace,
            "cli": cli,
            "extends": extends,
            "extension_state": extension_state,
        }),
    )
}

/// `plugin.extension.list` — 모든 extension의 현재 상태를 반환.
/// `[extends]` 블록이 없는 plugin은 결과에 포함되지 않는다.
pub fn handle_extension_list(mgr: Option<&PluginManager>, id: Value) -> JsonRpcResponse {
    let mgr = match mgr {
        Some(m) => m,
        None => return JsonRpcResponse::error(id, -32000, "plugin manager not initialized"),
    };
    let mut entries: Vec<Value> = mgr
        .extensions_iter()
        .map(|(ext_id, state)| {
            let target_id = mgr
                .packages
                .iter()
                .find(|p| p.manifest.id == ext_id)
                .and_then(|p| p.manifest.extends.as_ref().map(|e| e.plugin_id.clone()));
            json!({
                "extension_id": ext_id,
                "target_id": target_id,
                "state": extension_state_to_json(state),
            })
        })
        .collect();
    entries.sort_by(|a, b| {
        a.get("extension_id")
            .and_then(|v| v.as_str())
            .cmp(&b.get("extension_id").and_then(|v| v.as_str()))
    });
    JsonRpcResponse::success(id, json!({ "extensions": entries }))
}

fn extension_state_to_json(state: &crate::plugin::extension_registry::ExtensionState) -> Value {
    use crate::plugin::extension_registry::{ExtensionState, PendingReason};
    match state {
        ExtensionState::Active {
            target_id,
            target_version,
        } => json!({
            "status": "active",
            "target_id": target_id,
            "target_version": target_version,
        }),
        ExtensionState::Pending(reason) => {
            let r = match reason {
                PendingReason::TargetMissing => json!({ "kind": "target_missing" }),
                PendingReason::TargetDisabled => json!({ "kind": "target_disabled" }),
                PendingReason::VersionMismatch {
                    target_version,
                    required,
                } => json!({
                    "kind": "version_mismatch",
                    "target_version": target_version,
                    "required": required,
                }),
                PendingReason::InvalidTargetVersion { target_version } => json!({
                    "kind": "invalid_target_version",
                    "target_version": target_version,
                }),
                PendingReason::PermissionNotGranted => {
                    json!({ "kind": "permission_not_granted" })
                }
            };
            json!({ "status": "pending", "reason": r })
        }
        ExtensionState::Disabled => json!({ "status": "disabled" }),
        ExtensionState::Conflict { other_extension_id } => json!({
            "status": "conflict",
            "other_extension_id": other_extension_id,
        }),
    }
}

pub fn handle_permissions(
    mgr: Option<&PluginManager>,
    id: Value,
    params: &Value,
) -> JsonRpcResponse {
    let mgr = match mgr {
        Some(m) => m,
        None => return JsonRpcResponse::error(id, -32000, "plugin manager not initialized"),
    };
    let plugin_id = match params.get("id").and_then(|v| v.as_str()) {
        Some(s) => s.to_string(),
        None => return JsonRpcResponse::invalid_params(id, "Missing 'id' parameter"),
    };
    let pkg = match mgr.packages.iter().find(|p| p.manifest.id == plugin_id) {
        Some(p) => p,
        None => {
            return JsonRpcResponse::error(
                id,
                -32003,
                format!("plugin '{plugin_id}' not installed"),
            );
        }
    };
    let manifest_perms: Vec<&str> = pkg
        .manifest
        .permissions
        .iter()
        .map(|s| s.as_str())
        .collect();
    let granted: Vec<String> = mgr
        .config
        .granted_permissions(&plugin_id)
        .into_iter()
        .collect();
    JsonRpcResponse::success(
        id,
        json!({
            "id": plugin_id,
            "manifest": manifest_perms,
            "granted": granted,
        }),
    )
}

/// [`dispatch_readonly`] 가 답하는 메서드 이름.
///
/// 호출자가 "이 메서드를 내가 처리하나" 를 **답을 만들기 전에** 물어야 해서 따로 둔다 —
/// 헤드리스는 이 판정으로 매니저를 세울지 정하므로, 판정이 답변보다 앞선다.
/// 이 표와 `dispatch_readonly` 의 match 가 갈라지지 않는 것은 이 모듈의 테스트가 본다.
pub const READONLY_METHODS: &[&str] = &[
    "plugin.list",
    "plugin.show",
    "plugin.permissions",
    "plugin.extension.list",
    "plugin.audit_query",
    "plugin.audit_summary",
    "plugin.list_agent_permissions",
];

/// 이 메서드를 [`dispatch_readonly`] 가 답하는가 — **답을 만들기 전에** 묻는 순수 판정.
///
/// 헤드리스는 이 값으로 매니저를 세울지 정하므로 판정이 답변보다 앞선다. 그리고
/// `Core` 없이 답할 수 있어야 단위 테스트가 표를 검사할 수 있다.
pub fn is_readonly_method(method: &str) -> bool {
    READONLY_METHODS.contains(&method)
}

/// 창이 없어도 답이 정의되는 **읽기 전용** `plugin.*` 조회를 한 자리에서 라우팅한다.
/// 속하지 않는 메서드면 `None` — 호출자가 이어서 처리한다.
///
/// gui 라우터와 헤드리스 pump 가 **같은 이 함수**를 부른다. 두 벌로 복제하면 한쪽만
/// 고쳐지는 순간 갈라지고, 이 레포는 그 실패형을 이미 한 번 겪었다(같은 정규식이
/// 두 곳에 복제돼 서로 다르게 자란 건). 그래서 라우팅 표를 하나만 둔다.
///
/// 여기 있는 것은 전부 **읽기**다. 쓰기(`plugin.audit_clear` ·
/// `plugin.grant_agent_permission` · `plugin.revoke_agent_permission`)와 plugin
/// 수명주기(`enable`/`disable`/`install`/`remove`/`grant`/`revoke`/`upgrade_builtins`),
/// 그리고 창을 요구하는 `plugin.request_permission` 은 들어오지 않는다 —
/// 각각이 왜 빠졌는지는 `docs/dev-guide/headless-ipc-surface.md` 에 메서드별로 적혀 있다.
pub fn dispatch_readonly(
    core: &crate::core::Core,
    mgr: Option<&PluginManager>,
    method: &str,
    id: Value,
    params: &Value,
) -> Option<JsonRpcResponse> {
    if !is_readonly_method(method) {
        return None;
    }
    let response = match method {
        "plugin.list" => handle_list(mgr, id),
        "plugin.show" => handle_show(mgr, id, params),
        "plugin.permissions" => handle_permissions(mgr, id, params),
        "plugin.extension.list" => handle_extension_list(mgr, id),
        "plugin.audit_query" => super::audit::handle_query(core, id, params),
        "plugin.audit_summary" => super::audit::handle_summary(core, id, params),
        "plugin.list_agent_permissions" => {
            super::session::handle_list_agent_permissions(core, id, params)
        }
        // 위에서 표로 걸렀으므로 여기 오는 것은 **표에 이름을 넣고 arm 을 안 넣은**
        // 경우뿐이다. 조용히 `None` 을 돌려주면 그 메서드가 `-32601` 로 새어나가
        // "구현 안 됨" 과 구별되지 않으므로, 그 자리에서 크게 실패한다.
        other => JsonRpcResponse::internal_error(
            id,
            format!("READONLY_METHODS 에 '{other}' 가 있으나 dispatch arm 이 없다"),
        ),
    };
    Some(response)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 매니저가 없을 때 네 핸들러가 **같은 형태로** 답하는지 고정한다.
    ///
    /// 이 파일은 한때 `handle_list` 만 빈 목록을 성공으로 돌려주어, 호출자가
    /// "plugin 이 없다" 와 "매니저가 아직 없다" 를 가를 수 없었다. 한 곳만 고치면
    /// 다음에 핸들러가 늘 때 같은 이탈이 다시 생기므로, 넷을 한 자리에서 비교한다.
    #[test]
    fn every_plugin_handler_reports_a_missing_manager_the_same_way() {
        let id = || Value::from(1);
        let params = json!({"id": "any"});
        let responses = [
            ("plugin.list", handle_list(None, id())),
            ("plugin.show", handle_show(None, id(), &params)),
            ("plugin.extension.list", handle_extension_list(None, id())),
            (
                "plugin.permissions",
                handle_permissions(None, id(), &params),
            ),
        ];

        for (method, resp) in &responses {
            let err = resp
                .error
                .as_ref()
                .unwrap_or_else(|| panic!("{method} 가 매니저 없음을 에러로 답하지 않았다"));
            assert_eq!(err.code, -32000, "{method} 의 에러 코드");
            assert_eq!(
                err.message, "plugin manager not initialized",
                "{method} 의 에러 문구"
            );
        }
    }

    /// 읽기 전용 표가 무엇을 담고 무엇을 안 담는지 고정한다.
    ///
    /// `dispatch_readonly` 자신은 `Core` 를 요구해 단위 테스트가 만들 수 없다(이 크레이트에
    /// 테스트용 `Core` 생성자가 없다). 그래서 표 판정만 여기서 보고, 표와 arm 이 실제로
    /// 이어져 있는지는 헤드리스 통합 테스트가 각 메서드에 응답을 받아 확인한다.
    /// arm 을 빠뜨리면 `dispatch_readonly` 가 조용히 넘기지 않고 internal_error 로 답한다.
    #[test]
    fn the_readonly_table_holds_reads_and_excludes_writes() {
        for method in READONLY_METHODS {
            assert!(
                is_readonly_method(method),
                "표에 있는 {method} 를 판정이 부정했다"
            );
        }
        assert_eq!(
            READONLY_METHODS.len(),
            7,
            "표 크기가 바뀌었다 — 문서도 같이 고쳐라"
        );

        // 비영 대조 — 이 넷이 false 여야 위 단언이 "무조건 true" 가 아니게 된다.
        // 셋은 쓰기(`audit_clear` · 두 agent 권한 변경)고 하나는 창을 요구한다.
        for method in [
            "plugin.audit_clear",
            "plugin.grant_agent_permission",
            "plugin.revoke_agent_permission",
            "plugin.request_permission",
            "plugin.enable",
        ] {
            assert!(
                !is_readonly_method(method),
                "{method} 는 읽기 전용이 아닌데 표가 받아들였다"
            );
        }
    }

    // 이 테스트의 반대 극(= 매니저가 있을 때 에러가 아니다)은 여기서 못 만든다 —
    // `PluginManager::new` 는 `tasty-host-plugin` 크레이트의 `#[cfg(test)]` 라
    // 본 크레이트의 unit test 에서는 존재하지 않고, `with_registries` 는 waker 와
    // 두 registry port 를 요구해 단위 테스트가 감당할 대상이 아니다. 그 극은
    // 헤드리스 통합 테스트가 실제 데몬에 `plugin.list` 를 쳐서 성공 응답을 받는
    // 것으로 덮는다 — 그쪽이 없으면 위 단언은 "넷 다 무조건 에러" 여도 통과한다.
}
