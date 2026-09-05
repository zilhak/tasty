//! 호스트 IPC 를 동기 호출하는 최소 표면과, 그 위에 서는 형제 hook 정리.

use serde_json::{Value, json};
use tasty_plugin_sdk::HostHandle;

/// 호스트 IPC 를 동기 호출하는 최소 표면. `HostHandle` 로 실동작하고, 테스트에서는
/// in-memory mock 으로 대체해 형제 hook 등록/발화/정리 사이클을 재현·검증한다.
pub trait HostCall {
    fn call(&self, method: &str, params: Value) -> Result<Value, tasty_plugin_sdk::PluginError>;
}

impl HostCall for HostHandle {
    fn call(&self, method: &str, params: Value) -> Result<Value, tasty_plugin_sdk::PluginError> {
        HostHandle::call(self, method, params)
    }
}

/// `hook.list` 응답 배열에서 정리 대상 형제 hook 의 id 들을 고른다 — command 문자열이
/// `expected_command` 와 정확히 일치하는 hook 만. 상태를 공유하지 않는(clobber 불가)
/// 순수 선택 로직이라 concurrent 등록에도 그룹 격리가 성립한다: 같은 target surface 에
/// 서로 다른 command(예: `--command spawn` vs `--command tell`)로 등록된 두 그룹은
/// 서로의 정리 대상에 포함되지 않는다.
pub fn siblings_to_unset(hooks: &[Value], expected_command: &str) -> Vec<u64> {
    hooks
        .iter()
        .filter(|h| h.get("command").and_then(|v| v.as_str()) == Some(expected_command))
        .filter_map(|h| h.get("id").and_then(|v| v.as_u64()))
        .collect()
}

/// 발화한 형제 하나가 자기 그룹(같은 command)의 남은 형제 once-hook 들을 정리한다.
/// `hook.list` 는 반드시 `surface_id` 로 필터해 다른 surface(=다른 child)의 hook 을
/// 건드리지 않는다. best-effort — 실패해도 알림 자체는 이미 전달됐다.
pub fn cleanup_sibling_hooks<H: HostCall>(host: &H, target_surface: u32, expected_command: &str) {
    if let Ok(resp) = host.call("hook.list", json!({ "surface_id": target_surface }))
        && let Some(hooks) = resp.as_array()
    {
        for hook_id in siblings_to_unset(hooks, expected_command) {
            // best-effort 정리 — 실패하면 좀비로 남을 수 있으나 알림 자체는 이미
            // 전달됐으므로 caller 관점 결과에는 영향 없음.
            let _ = host.call("hook.unset", json!({ "hook_id": hook_id }));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 같은 target surface 에 두 그룹(예: spawn / tell)이 공존해도 서로의 정리
    /// 대상에 들어가지 않는다.
    #[test]
    fn siblings_to_unset_isolates_by_command() {
        let spawn_cmd = "tasty hook done --command spawn";
        let tell_cmd = "tasty hook done --command tell";
        let hooks = vec![
            json!({ "id": 1, "command": spawn_cmd, "event": "process-exit" }),
            json!({ "id": 2, "command": tell_cmd, "event": "process-exit" }),
            json!({ "id": 3, "command": spawn_cmd, "event": "agent-idle" }),
        ];
        assert_eq!(siblings_to_unset(&hooks, spawn_cmd), vec![1, 3]);
        assert_eq!(siblings_to_unset(&hooks, tell_cmd), vec![2]);
    }

    /// `command` 가 없거나 `id` 가 없는 항목은 조용히 건너뛴다 — 호스트 응답 스키마가
    /// 늘어나도 정리가 패닉하지 않는다.
    #[test]
    fn malformed_hook_entries_are_skipped() {
        let hooks = vec![
            json!({ "id": 1 }),
            json!({ "command": "c" }),
            json!({ "id": 2, "command": "c" }),
        ];
        assert_eq!(siblings_to_unset(&hooks, "c"), vec![2]);
    }
}
