//! `hook-handler dispatch`: 접수 뒤 `hook_handler.result`로 최종 결과를 기다린다.
//! 출력·종료 코드·대기 상한 처리는 [`super::run_wait`]를 따른다.

use std::time::Duration;

use anyhow::Result;
use serde_json::Value;

use crate::Commands;
use crate::dispatch::{ClientCommand, ClientCtx};

/// 호스트의 스텝 응답 대기 상한과 같다. 시퀀스는 스텝 수만큼 기다린다.
const STEP_WAIT: Duration = Duration::from_secs(10);

pub struct HookDispatch<'a> {
    pub command: &'a Commands,
}

impl ClientCommand for HookDispatch<'_> {
    fn run(self: Box<Self>, ctx: &ClientCtx<'_>) -> Result<()> {
        let request = crate::request::command_to_request(self.command);
        super::run_wait::dispatch_and_wait(
            ctx.port_file,
            &request,
            "hook_handler.result",
            wait_limit,
        )
    }
}

/// 셸 핸들러는 프로세스 시작만 기다리고, 시퀀스는 스텝마다 응답 대기 상한을 더한다.
fn wait_limit(accepted: &Value) -> Duration {
    let steps = accepted.get("steps").and_then(Value::as_u64).unwrap_or(0) as u32;
    STEP_WAIT * (steps + 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_sequence_waits_one_step_limit_per_step() {
        assert_eq!(
            wait_limit(&serde_json::json!({"steps": 2})),
            Duration::from_secs(30)
        );
        assert_eq!(wait_limit(&serde_json::json!({})), Duration::from_secs(10));
    }
}
