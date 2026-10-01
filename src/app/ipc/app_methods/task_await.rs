//! agent.task_await가 기다릴 허브를 요청 workspace의 소유 engine에서 고른다.

use crate::adapters::ipc::handler::params;
use crate::app::App;
use crate::ipc::server::{IpcCommand, send_response};
use crate::runtime::engine_access::EngineRef;

impl App {
    /// 요청 workspace를 가진 engine의 허브에서 기다린다. 포커스·창 순서로 고르지 않는다.
    /// 완료 통지는 소유 engine의 허브로만 가므로 다른 engine의 허브에서 기다리면 놓친다.
    pub(super) fn ipc_dispatch_task_await(&mut self, cmd: &IpcCommand) {
        let rpc_id = cmd.request.id.clone().unwrap_or(serde_json::Value::Null);
        let engines = || self.engines().all();
        if engines().next().is_none() {
            send_response(
                &cmd.response_tx,
                crate::app::services::surface::no_application_state(rpc_id),
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
            self.services.tasks.awaiter(engine.task_scope),
            rpc_id,
            cmd.request.params.clone(),
            &cmd.response_tx,
        );
    }
}

/// 창, parked, 임시 engine 순서로 받은 engine 중 workspace를 가진 것을 고른다.
fn task_await_engine<'a>(
    mut engines: impl Iterator<Item = EngineRef<'a>>,
    workspace_id: u32,
) -> Option<EngineRef<'a>> {
    engines.find(|e| e.has_workspace(workspace_id))
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::time::Duration;

    use serde_json::json;
    use tasty_agent::task::{OnFailure, TaskCommand, TaskCreateOpts};

    use super::*;

    fn engine_with_workspace(workspace_id: u32) -> crate::runtime::engine_session::EngineSession {
        let waker: crate::terminal::Waker = Arc::new(|| {});
        let mut engine_session =
            crate::runtime::engine_session::EngineSession::new(80, 24, waker).expect("engine");
        let mut engine = engine_session.borrow_mut();
        engine
            .workspace_at_mut(0)
            .expect("workspace index is valid")
            .id = workspace_id;
        engine_session
    }

    fn ready_task(
        core: &crate::app::services::AppServices,
        engine: &EngineRef<'_>,
        workspace_id: u32,
    ) -> String {
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
            .task_create(engine.task_scope, opts, false)
            .expect("task_create")
            .id
    }

    #[test]
    fn the_engine_owning_the_workspace_is_picked_over_the_first_one() {
        let mut first_session = engine_with_workspace(1);
        let first = first_session.borrow_mut();
        let mut owner_session = engine_with_workspace(2);
        let owner = owner_session.borrow_mut();
        let picked = task_await_engine([first.as_ref(), owner.as_ref()].into_iter(), 2)
            .expect("소유 engine이 있다");
        assert!(
            Arc::ptr_eq(picked.task_scope.waker_hub(), owner.task_scope.waker_hub()),
            "두 번째 engine의 workspace를 기다리면 그 engine의 허브를 써야 한다"
        );
    }

    #[test]
    fn a_workspace_no_engine_owns_picks_nothing() {
        let mut first_session = engine_with_workspace(1);
        let first = first_session.borrow_mut();
        let mut second_session = engine_with_workspace(2);
        let second = second_session.borrow_mut();
        assert!(task_await_engine([first.as_ref(), second.as_ref()].into_iter(), 3).is_none());
    }

    /// 두 engine이 메모리를 공유할 때 두 번째 engine의 작업 완료가 대기자에게 도달해야 한다.
    #[test]
    fn a_task_in_the_second_engine_wakes_its_awaiter() {
        let core = crate::adapters::ipc::handler::cli_entry_tests::test_core();
        let mut first_session = engine_with_workspace(1);
        let first = first_session.borrow_mut();
        let mut owner_session = engine_with_workspace(2);
        let owner = owner_session.borrow_mut();
        let task_id = ready_task(&core, &owner.as_ref(), 2);

        let picked =
            task_await_engine([first.as_ref(), owner.as_ref()].into_iter(), 2).expect("engine");
        let awaiter = core.tasks.awaiter(picked.task_scope);
        let params = json!({ "workspace_id": 2, "id": task_id, "timeout_ms": 3_000 });
        let waiting = std::thread::spawn(move || {
            crate::ipc::handler::agent::task::await_task_blocking(&awaiter, json!(1), &params)
        });
        // 대기자가 등록된 뒤 완료되도록 기다린다. 등록 전에 완료돼도 조회가 종결 상태를 본다.
        std::thread::sleep(Duration::from_millis(200));
        core.tasks
            .task_cancel(owner.task_scope, 2, &task_id, 2)
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
