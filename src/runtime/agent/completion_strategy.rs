//! 매니페스트 poll 선언을 agent PollSpec으로 변환한다.
//! 두 크레이트가 서로 의존하지 않아 둘을 사용하는 호스트에 둔다.

use tasty_agent::PollSpec;
use tasty_plugin_manifest::CompletionStrategyDecl;

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

    /// failure_states를 생략한 매니페스트는 빈 실패 목록으로 읽는다.
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
