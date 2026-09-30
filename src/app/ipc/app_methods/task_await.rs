//! agent.task_await가 기다릴 허브를 요청 workspace의 소유 engine에서 고른다.

use crate::adapters::ipc::handler::params;
use crate::app::App;
use crate::core::CoreState;
use crate::ipc::server::{IpcCommand, send_response};

impl App {
    /// 요청 workspace를 가진 engine의 허브에서 기다린다. 포커스·창 순서로 고르지 않는다.
    /// 완료 통지는 소유 engine의 허브로만 가므로 다른 engine의 허브에서 기다리면 놓친다.
    pub(super) fn ipc_dispatch_task_await(&mut self, cmd: &IpcCommand) {
        let rpc_id = cmd.request.id.clone().unwrap_or(serde_json::Value::Null);
        let engines = || self.engines().all();
        if engines().next().is_none() {
            send_response(
                &cmd.response_tx,
                crate::core::app_surface::no_application_state(rpc_id),
            );
            return;
        }
        let workspace_id = match params::require_u32(&cmd.request.params, "workspace_id", &rpc_id) {
            Ok(w) => w,
            Err(resp) => {
                send_response(&cmd.response_tx, resp);
                return;
            }
        };
        let Some(engine) = task_await_engine(engines(), workspace_id) else {
            send_response(
                &cmd.response_tx,
                crate::ipc::handler::agent::task::unowned_await_workspace(rpc_id, workspace_id),
            );
            return;
        };
        crate::ipc::handler::agent::task::spawn_task_await(
            self.core.tasks.awaiter(&engine.task_scope),
            rpc_id,
            cmd.request.params.clone(),
            &cmd.response_tx,
        );
    }
}

/// 창, parked, 임시 engine 순서로 받은 engine 중 workspace를 가진 것을 고른다.
fn task_await_engine<'a>(
    mut engines: impl Iterator<Item = &'a CoreState>,
    workspace_id: u32,
) -> Option<&'a CoreState> {
    engines.find(|e| e.has_workspace(workspace_id))
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::time::Duration;

    use serde_json::json;
    use tasty_agent::task::{OnFailure, TaskCommand, TaskCreateOpts};

    use super::*;

    fn engine_with_workspace(workspace_id: u32) -> CoreState {
        let waker: crate::terminal::Waker = Arc::new(|| {});
        let mut engine = CoreState::new(80, 24, waker).expect("engine");
        engine.workspaces[0].id = workspace_id;
        engine
    }

    fn ready_task(core: &crate::core::Core, engine: &CoreState, workspace_id: u32) -> String {
        let opts = TaskCreateOpts {
            workspace_id,
            name: "t".to_string(),
            command: TaskCommand::Run {
                command: vec!["true".into()],
                workspace_id,
                cwd: None,
            },
            depends_on: Vec::new(),
            on_failure: OnFailure::default(),
            metadata: serde_json::Value::Null,
            now_ms: 1,
        };
        core.tasks
            .task_create(&engine.task_scope, opts, false)
            .expect("task_create")
            .id
    }

    #[test]
    fn the_engine_owning_the_workspace_is_picked_over_the_first_one() {
        let first = engine_with_workspace(1);
        let owner = engine_with_workspace(2);
        let picked =
            task_await_engine([&first, &owner].into_iter(), 2).expect("소유 engine이 있다");
        assert!(
            Arc::ptr_eq(picked.task_scope.waker_hub(), owner.task_scope.waker_hub()),
            "두 번째 engine의 workspace를 기다리면 그 engine의 허브를 써야 한다"
        );
    }

    #[test]
    fn a_workspace_no_engine_owns_picks_nothing() {
        let first = engine_with_workspace(1);
        let second = engine_with_workspace(2);
        assert!(task_await_engine([&first, &second].into_iter(), 3).is_none());
    }

    /// 두 engine이 메모리를 공유할 때 두 번째 engine의 작업 완료가 대기자에게 도달해야 한다.
    #[test]
    fn a_task_in_the_second_engine_wakes_its_awaiter() {
        let core = crate::adapters::ipc::handler::cli_entry_tests::test_core();
        let first = engine_with_workspace(1);
        let owner = engine_with_workspace(2);
        let task_id = ready_task(&core, &owner, 2);

        let picked = task_await_engine([&first, &owner].into_iter(), 2).expect("engine");
        let awaiter = core.tasks.awaiter(&picked.task_scope);
        let params = json!({ "workspace_id": 2, "id": task_id, "timeout_ms": 3_000 });
        let waiting = std::thread::spawn(move || {
            crate::ipc::handler::agent::task::await_task_blocking(&awaiter, json!(1), &params)
        });
        // 대기자가 등록된 뒤 완료되도록 기다린다. 등록 전에 완료돼도 조회가 종결 상태를 본다.
        std::thread::sleep(Duration::from_millis(200));
        core.tasks
            .task_cancel(&owner.task_scope, 2, &task_id, 2)
            .expect("cancel");

        let resp = waiting.join().expect("await thread");
        let result = resp.result.expect("success response");
        assert_eq!(
            result["outcome"], "terminal",
            "다른 engine의 허브에서 기다리면 완료를 놓친다: {result}"
        );
        assert_eq!(result["state"], "cancelled");
    }
}
