//! 종결된 task 의 retry.

use super::super::{OnFailure, Readiness, Task, TaskGraph, TaskId, TaskState, route};
use super::*;
use crate::{AgentError, Result};

impl TaskStore<'_> {
    /// retry. 현재 state가 Failed/Cancelled/Skipped/Unknown인 경우만 허용.
    /// `reset_downstream=true`면 downstream 중 Skipped/Failed인 것도 Waiting으로 되돌림.
    pub fn retry(
        &mut self,
        workspace_id: WorkspaceId,
        id: &TaskId,
        reset_downstream: bool,
        now_ms: u64,
    ) -> Result<Task> {
        let mut task = self
            .get(workspace_id, id)?
            .ok_or_else(|| AgentError::TaskNotFound(id.clone()))?;
        match &task.state {
            TaskState::Failed { .. }
            | TaskState::Cancelled
            | TaskState::Skipped
            | TaskState::Unknown { .. } => {}
            other => {
                return Err(AgentError::InvalidTransition {
                    from: other.name().to_string(),
                    to: "waiting (retry)".to_string(),
                });
            }
        }
        self.refuse_retry_after_fallback(workspace_id, &task)?;
        self.refuse_retry_of_unselected(workspace_id, &task)?;
        if reset_downstream && task.is_typed() {
            return Err(AgentError::InvalidArgument(format!(
                "typed task {id} cannot be retried with reset_downstream: downstream tasks already \
                 settled on this run's failure or route and are not rewound; retry without it, \
                 or submit a new graph for the follow-up work"
            )));
        }
        task.state = TaskState::Waiting;
        task.started_at = None;
        task.finished_at = None;
        task.result = None;
        task.typed_result = None;
        task.input_snapshot = None;
        task.route = None;
        task.skip = None;
        self.put(&task)?;

        // readiness 즉시 평가 — deps 가 이미 종결(예: 여전히 실패/skip 상태)이면 이 자리에서
        // 곧장 Skipped 로 되돌아갈 수 있다. set_state 를 거치지 않는 직접-put 이므로, 그
        // terminal 타임스탬프 기록을 여기서 재현한다(cascade_downstream 과 동일 이유).
        let all = self.list(workspace_id)?;
        if let Some((next, skip)) =
            settle_target(&self.readiness_graph(workspace_id, &all)?, &task, &all)
            && next != TaskState::Waiting
        {
            let is_terminal = next.is_terminal();
            let mut nt = task.clone();
            nt.state = next;
            nt.skip = skip;
            if is_terminal {
                nt.finished_at = Some(now_ms);
            }
            self.put(&nt)?;
            task = nt;
        }

        if reset_downstream {
            let downstream = TaskGraph::build(&all).transitive_downstream(id);
            for d_id in downstream {
                if let Some(mut d) = self.get(workspace_id, &d_id)?
                    && matches!(
                        d.state,
                        TaskState::Skipped | TaskState::Failed { .. } | TaskState::Cancelled
                    )
                {
                    d.state = TaskState::Waiting;
                    d.started_at = None;
                    d.finished_at = None;
                    d.result = None;
                    d.typed_result = None;
                    d.input_snapshot = None;
                    d.route = None;
                    d.skip = None;
                    self.put(&d)?;
                }
            }
            // downstream 모두 갱신했으니 cascade 한번 더 — 재-skip 되는 downstream 의
            // finished_at 은 cascade_downstream 이 now_ms 로 다시 채운다.
            self.cascade_downstream(workspace_id, id, now_ms)?;
        }

        Ok(task)
    }

    /// v2 task 의 fallback 이 이미 실행됐으면 재시도를 거절한다. main 이 다시 성공하면 둘 중
    /// 하나를 받는 소비자(`one_of`)가 성공한 원본 둘을 보게 되고, 이미 끝난 fallback 의 결과를
    /// 되돌릴 수도 없다. fallback 이 실패했거나 실행 전에 끝났으면 재시도할 수 있다.
    fn refuse_retry_after_fallback(&self, workspace_id: WorkspaceId, task: &Task) -> Result<()> {
        let OnFailure::Fallback {
            task: Some(fallback_id),
            ..
        } = &task.on_failure
        else {
            return Ok(());
        };
        if !task.is_typed() {
            return Ok(());
        }
        let Some(fallback) = self.get(workspace_id, fallback_id)? else {
            return Ok(());
        };
        if matches!(
            fallback.state,
            TaskState::Ready | TaskState::Running | TaskState::Succeeded
        ) {
            return Err(AgentError::InvalidArgument(format!(
                "typed task {} cannot be retried: its fallback {fallback_id} is {} and \
                 stays the outcome of this run; submit a new task to run it again",
                task.id,
                fallback.state.name()
            )));
        }
        Ok(())
    }

    /// 경로가 선택되지 않아 건너뛴 v2 task 를 다시 판정해도 선택되지 않으면 재시도를 거절한다.
    /// 받아들이면 같은 판정으로 곧장 건너뛰어 아무것도 실행하지 않은 채 성공 응답만 남는다.
    fn refuse_retry_of_unselected(&self, workspace_id: WorkspaceId, task: &Task) -> Result<()> {
        if !task.is_typed() || !route::is_not_selected(task) {
            return Ok(());
        }
        let all = self.list(workspace_id)?;
        if matches!(
            self.readiness_graph(workspace_id, &all)?
                .readiness(&task.id),
            Some(Readiness::NotSelected)
        ) {
            return Err(AgentError::InvalidArgument(format!(
                "typed task {} cannot be retried: its branch was not selected and is still not \
                 selected, so it would be skipped again without running; submit a new task to \
                 run it",
                task.id
            )));
        }
        Ok(())
    }
}
