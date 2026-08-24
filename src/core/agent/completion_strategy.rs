//! `CompletionStrategyDecl`(plugin manifest, `tasty-plugin-manifest`) →
//! `PollSpec`(agent DAG, `tasty-agent`) 변환.
//!
//! 두 크레이트 어느 쪽도 서로를 의존하지 않으므로(`tasty-agent` 는 manifest
//! 파싱을 모르고, `tasty-plugin-manifest` 는 agent DAG 실행형을 모른다) 이
//! 변환은 둘 다 의존 가능한 본 바이너리(`src/`)가 소유한다 —
//! `src/hook_handler/config.rs` 의 `impl From<PluginHookHandlerActionDecl> for
//! HookHandlerAction` 과 동일한 지위의 host-side decl→runtime 변환.
//!
//! `src/completion_strategy/config.rs`(완료 판정 전략 레지스트리) 의
//! `CompletionStrategySpecDecl::Poll(PollStrategyDecl)` → `CompletionStrategyKind`
//! 변환이 이 함수를 호출한다 — 필드 대응은 이 파일의 단위테스트가 단일 지점에서
//! 고정한다.

use tasty_agent::PollSpec;
use tasty_plugin_manifest::CompletionStrategyDecl;

/// `CompletionStrategyDecl` → `PollSpec`. 필드는 이름이 같은 `poll_method`/
/// `state_field`/`terminal_states`/`failure_states`/`interval_ms`/`timeout_ms` 를 그대로 옮기고,
/// 두 맵(`map_from_response`/`map_from_request`)도 그대로 복사한다 — 변환에
/// 실패할 수 있는 조건이 없으므로 `Result` 가 아니라 값을 직접 반환한다.
pub(crate) fn completion_strategy_to_poll_spec(decl: &CompletionStrategyDecl) -> PollSpec {
    PollSpec {
        poll_method: decl.poll_method.clone(),
        map_from_response: decl.map_from_response.clone(),
        map_from_request: decl.map_from_request.clone(),
        state_field: decl.state_field.clone(),
        terminal_states: decl.terminal_states.clone(),
        failure_states: decl.failure_states.clone(),
        interval_ms: decl.interval_ms,
        timeout_ms: decl.timeout_ms,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_decl() -> CompletionStrategyDecl {
        let toml = r#"
            poll_method = "claude.wait_by_surface"
            map_from_response = { child_surface_id = "surface_id" }
            map_from_request = { surface = "surface" }
            state_field = "state"
            terminal_states = ["idle", "needs_input"]
            failure_states = ["exited"]
            interval_ms = 250
            timeout_ms = 30000
        "#;
        toml::from_str(toml).expect("valid CompletionStrategyDecl toml")
    }

    /// 필드 대응이 갈라지면(이름이든 값이든) 이 테스트가 실패한다 — `task.rs`의
    /// "동기화 책임" 주석이 가리키는 바로 그 테스트.
    #[test]
    fn field_correspondence_is_preserved() {
        let decl = sample_decl();
        let spec = completion_strategy_to_poll_spec(&decl);
        assert_eq!(spec.poll_method, decl.poll_method);
        assert_eq!(spec.map_from_response, decl.map_from_response);
        assert_eq!(spec.map_from_request, decl.map_from_request);
        assert_eq!(spec.state_field, decl.state_field);
        assert_eq!(spec.terminal_states, decl.terminal_states);
        assert_eq!(spec.failure_states, decl.failure_states);
        assert_eq!(spec.interval_ms, decl.interval_ms);
        assert_eq!(spec.timeout_ms, decl.timeout_ms);
    }

    #[test]
    fn interval_ms_and_timeout_ms_default_when_omitted() {
        let toml = r#"
            poll_method = "claude.wait_by_surface"
            state_field = "state"
            terminal_states = ["idle"]
        "#;
        let decl: CompletionStrategyDecl = toml::from_str(toml).expect("valid minimal toml");
        let spec = completion_strategy_to_poll_spec(&decl);
        assert_eq!(spec.interval_ms, 500);
        assert_eq!(spec.timeout_ms, None);
    }

    /// `failure_states` 를 선언하지 않은 기존 매니페스트가 그대로 로드돼야 한다 —
    /// 빈 목록이면 폴링 판정은 이 필드 도입 전과 완전히 같게 동작한다.
    #[test]
    fn failure_states_default_to_empty_when_omitted() {
        let toml = r#"
            poll_method = "claude.wait_by_surface"
            state_field = "state"
            terminal_states = ["idle", "exited"]
        "#;
        let decl: CompletionStrategyDecl = toml::from_str(toml).expect("valid minimal toml");
        let spec = completion_strategy_to_poll_spec(&decl);
        assert!(spec.failure_states.is_empty());
    }
}
