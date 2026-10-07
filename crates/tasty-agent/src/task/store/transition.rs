//! 상태 전이와 그 뒤의 하류 readiness·fallback 전파.

use super::super::{
    InlineFallbackSpec, OnFailure, Task, TaskGraph, TaskId, TaskResult, TaskState,
    is_valid_transition,
};
use super::{
    TaskCreateOpts, TaskStore, WorkspaceId, record_result, settle_target, settle_typed_terminal,
};
use crate::{AgentError, Result};

impl TaskStore<'_> {
    /// task의 state를 변경. 변경 규칙은 [`TaskState`] 문서 참조.
    /// 변경 후 downstream의 readiness를 자동 재평가해 `Waiting → Ready/Skipped`로
    /// 전이시키고 영속한다. 반환값은 (갱신된 자기 자신, 자동 전이된 downstream).
    pub fn set_state(
        &mut self,
        workspace_id: WorkspaceId,
        id: &TaskId,
        new_state: TaskState,
        now_ms: u64,
    ) -> Result<(Task, Vec<Task>)> {
        let mut task = self
            .get(workspace_id, id)?
            .ok_or_else(|| AgentError::TaskNotFound(id.clone()))?;
        if !is_valid_transition(&task.state, &new_state) {
            return Err(AgentError::InvalidTransition {
                from: task.state.name().to_string(),
                to: new_state.name().to_string(),
            });
        }
        let new_state = settle_typed_terminal(&mut task, new_state);
        let new_state = self.settle_route(&mut task, new_state);
        match new_state {
            TaskState::Running => {
                task.started_at = Some(now_ms);
                task.route = None;
                task.skip = None;
                if let Some(attempt) = super::super::attempt::next_attempt(&task, now_ms) {
                    task.attempt = Some(attempt);
                }
            }
            TaskState::Succeeded
            | TaskState::Failed { .. }
            | TaskState::Cancelled
            | TaskState::Skipped => {
                task.finished_at = Some(now_ms);
                super::super::postprocess::close_phase(&mut task);
            }
            _ => {}
        }
        task.state = new_state.clone();
        self.put(&task)?;
        let transitioned = self.propagate_transition(workspace_id, &task, now_ms)?;
        Ok((task, transitioned))
    }

    /// 저장을 마친 상태 전이의 후속 효과: fallback 승격·정리, 하류 readiness, 이 task 를
    /// fallback 으로 둔 main 의 하류 재평가. 같은 전이로 다시 불러도 결과가 같다(이미 옮긴
    /// task 는 Waiting 이 아니라 건너뛴다).
    pub(super) fn propagate_transition(
        &mut self,
        workspace_id: WorkspaceId,
        task: &Task,
        now_ms: u64,
    ) -> Result<Vec<Task>> {
        let id = &task.id;
        let new_state = task.state.clone();
        let mut transitioned = Vec::new();

        if matches!(new_state, TaskState::Failed { .. })
            && let OnFailure::Fallback {
                task: fb_id_opt,
                inline: inline_opt,
            } = task.on_failure.clone()
        {
            if let Some(fb_id) = fb_id_opt
                && let Some(fb) = self.advance_existing_fallback(workspace_id, &task.id, &fb_id)?
            {
                transitioned.push(fb);
            }
            if let Some(spec) = inline_opt
                && let Some(new_fb) =
                    self.materialize_inline_fallback(workspace_id, task, *spec, now_ms)?
            {
                transitioned.push(new_fb);
            }
        }

        // main이 실패 없이 끝났다면 실행할 일이 없는 기존 fallback도 종결한다.
        // inline fallback은 실패 시에만 생성되므로 정리할 대상이 없다.
        // v2 fallback 은 하류 판정(cascade)이 이유와 함께 마감한다.
        if !task.is_typed()
            && matches!(
                new_state,
                TaskState::Succeeded | TaskState::Cancelled | TaskState::Skipped
            )
            && let OnFailure::Fallback {
                task: Some(fb_id), ..
            } = task.on_failure.clone()
            && matches!(
                self.get(workspace_id, &fb_id)?.map(|t| t.state),
                Some(TaskState::Waiting)
            )
        {
            let (fb_task, more) =
                self.set_state(workspace_id, &fb_id, TaskState::Skipped, now_ms)?;
            transitioned.push(fb_task);
            transitioned.extend(more);
        }

        transitioned.extend(self.cascade_downstream(workspace_id, id, now_ms)?);

        // terminal 전이 시: 자기를 fallback 으로 지정한 main task 가 있으면 그 main
        // 의 downstream 도 재평가 (main 입장에선 fallback 결과로 effective state 가 정해짐).
        if matches!(
            new_state,
            TaskState::Succeeded
                | TaskState::Failed { .. }
                | TaskState::Cancelled
                | TaskState::Skipped
        ) {
            let all_now = self.list(workspace_id)?;
            let parent_main_ids: Vec<TaskId> = all_now
                .iter()
                .filter(|t| match &t.on_failure {
                    // existing 경로: main 의 task field 가 id 를 직접 가리킴.
                    OnFailure::Fallback {
                        task: Some(fb_id), ..
                    } => fb_id == id,
                    _ => false,
                })
                .map(|t| t.id.clone())
                .collect();
            // inline 경로: self.metadata.fallback_of 가 main id — main 을 찾으려면 reverse lookup.
            let mut inline_main_ids: Vec<TaskId> = Vec::new();
            if let Some(self_task) = all_now.iter().find(|t| t.id == *id)
                && let Some(main_id) = self_task
                    .metadata
                    .get("fallback_of")
                    .and_then(|v| v.as_str())
            {
                inline_main_ids.push(main_id.to_string());
            }
            for main_id in parent_main_ids.into_iter().chain(inline_main_ids) {
                transitioned.extend(self.cascade_downstream(workspace_id, &main_id, now_ms)?);
            }
        }
        Ok(transitioned)
    }

    /// `set_state`의 Failed→Fallback 분기 중 "케이스 1: existing fallback" 처리.
    /// fallback 대상이 `Ready`/`Skipped` 로 진행 가능하면 그 상태로 올리고 반환.
    fn advance_existing_fallback(
        &mut self,
        workspace_id: WorkspaceId,
        main_task_id: &TaskId,
        fb_id: &TaskId,
    ) -> Result<Option<Task>> {
        let Some(mut fb) = self.get(workspace_id, fb_id)? else {
            // 옛 레코드의 끊긴 참조는 남을 수 있다. downstream 대기 원인을 로그에 남긴다.
            tracing::warn!(
                task_id = %main_task_id,
                fallback_task_id = %fb_id,
                "on_failure.fallback.task references a task that no longer exists; \
                 downstream depending on this task will remain Waiting indefinitely"
            );
            return Ok(None);
        };
        let all_now = self.list(workspace_id)?;
        let Some(target) = self
            .readiness_graph(workspace_id, &all_now)?
            .evaluate_readiness(fb_id)
        else {
            return Ok(None);
        };
        if target == TaskState::Waiting || !is_valid_transition(&fb.state, &target) {
            return Ok(None);
        }
        fb.state = target;
        self.put(&fb)?;
        Ok(Some(fb))
    }

    /// `set_state`의 Failed→Fallback 분기 중 "케이스 2: inline → 동적 생성" 처리.
    /// 같은 main 이 이미 inline fallback 을 만든 적 있으면(idempotency) `None`.
    fn materialize_inline_fallback(
        &mut self,
        workspace_id: WorkspaceId,
        main_task: &Task,
        spec: InlineFallbackSpec,
        now_ms: u64,
    ) -> Result<Option<Task>> {
        let existing = self.list(workspace_id)?;
        let already = existing.iter().any(|t| {
            t.metadata.get("fallback_of").and_then(|v| v.as_str()) == Some(main_task.id.as_str())
        });
        if already {
            return Ok(None);
        }
        let mut metadata = spec.metadata;
        if !metadata.is_object() {
            metadata = serde_json::json!({});
        }
        if let Some(obj) = metadata.as_object_mut() {
            obj.insert(
                "fallback_of".into(),
                serde_json::Value::String(main_task.id.clone()),
            );
        }
        let opts = TaskCreateOpts {
            workspace_id,
            name: spec.name,
            command: spec.command,
            depends_on: spec
                .depends_on_override
                .unwrap_or_else(|| main_task.depends_on.clone()),
            on_failure: spec.on_failure,
            metadata,
            now_ms,
        };
        Ok(Some(self.create(opts)?))
    }

    /// task의 result를 기록 (state 전이는 별도). 보통 set_state(Succeeded/Failed) 전에 호출.
    pub fn set_result(
        &mut self,
        workspace_id: WorkspaceId,
        id: &TaskId,
        result: TaskResult,
    ) -> Result<Task> {
        let mut task = self
            .get(workspace_id, id)?
            .ok_or_else(|| AgentError::TaskNotFound(id.clone()))?;
        record_result(&mut task, result);
        self.put(&task)?;
        Ok(task)
    }

    /// `task_id`의 모든 transitive downstream에서 `Waiting` 상태인 것들을 평가해
    /// 가능하면 `Ready/Skipped`로 전이. on_failure 정책도 함께 적용.
    ///
    /// `now_ms`는 `Skipped`(terminal)로 전이하는 downstream의 `finished_at`을 채우는 데
    /// 쓰인다 — 이 경로는 `set_state`를 거치지 않고 상태를 직접 갈아끼우므로, `set_state`의
    /// terminal 타임스탬프 기록을 스스로 재현해야 한다.
    pub(super) fn cascade_downstream(
        &mut self,
        workspace_id: WorkspaceId,
        task_id: &TaskId,
        now_ms: u64,
    ) -> Result<Vec<Task>> {
        let mut all = self.list(workspace_id)?;
        let downstream_ids = TaskGraph::build(&all).transitive_downstream(task_id);
        self.settle_waiting(workspace_id, &mut all, &downstream_ids, now_ms)
    }

    /// `ids` 순서대로 Waiting task 의 readiness 를 평가하고 바뀐 task 를 저장한다.
    ///
    /// 저장소 목록은 호출자가 한 번 읽어 `all` 로 넘기고, 이 함수는 저장한 상태를 `all` 에도
    /// 반영해 다음 평가에 쓴다. 저장소를 task 마다 다시 읽지 않는다. 호출 동안 다른 쓰기가
    /// 끼지 않는 것은 `&mut self` 가 보장한다. 활성화 여부는 호출 시점에 한 번 읽는다.
    /// `ids` 가 위상 순서가 아니어도 되도록 바뀐 것이 없을 때까지 다시 훑는다. 합류 task 가
    /// 아직 Waiting 인 선행보다 먼저 나와도 그 선행이 끝난 뒤 다시 평가된다.
    pub(super) fn settle_waiting(
        &mut self,
        workspace_id: WorkspaceId,
        all: &mut [Task],
        ids: &[TaskId],
        now_ms: u64,
    ) -> Result<Vec<Task>> {
        let inactive = self.inactive_task_ids(workspace_id, all)?;
        let index: std::collections::HashMap<TaskId, usize> = all
            .iter()
            .enumerate()
            .map(|(i, t)| (t.id.clone(), i))
            .collect();
        let mut updated = Vec::new();
        let mut changed = true;
        while changed {
            changed = false;
            for id in ids {
                let Some(&i) = index.get(id) else {
                    continue;
                };
                if !matches!(all[i].state, TaskState::Waiting) {
                    continue;
                }
                let target = {
                    let graph = TaskGraph::build(all).with_inactive(inactive.clone());
                    settle_target(&graph, &all[i], all)
                };
                if let Some((next, skip)) = target
                    && next != TaskState::Waiting
                    && is_valid_transition(&all[i].state, &next)
                {
                    let mut nt = all[i].clone();
                    if next.is_terminal() {
                        nt.finished_at = Some(now_ms);
                    }
                    nt.state = next;
                    nt.skip = skip;
                    #[cfg(test)]
                    if super::graph_submit::FAIL_ACTIVATION_PUT.with(|f| f.replace(false)) {
                        return Err(AgentError::InvalidArgument(
                            "injected activation failure".into(),
                        ));
                    }
                    self.put(&nt)?;
                    all[i] = nt.clone();
                    updated.push(nt);
                    changed = true;
                }
            }
        }
        Ok(updated)
    }
}
