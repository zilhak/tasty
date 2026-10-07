//! 참조 검사를 거치는 task 삭제와 purge·GC 의 정리 계획.

use std::collections::HashSet;

use super::super::{TaskId, TaskState, referencing_task_ids, transitive_referencing_task_ids};
use super::*;
use crate::{AgentError, Result};

/// [`TaskStore::delete_checked`] 옵션.
#[derive(Debug, Clone, Copy, Default)]
pub struct TaskDeleteOpts {
    /// 대상의 전이적 참조자도 함께 삭제한다.
    pub cascade: bool,
    /// 참조 검사만 생략한다. Running 작업은 삭제할 수 없다.
    pub force: bool,
}

/// [`TaskStore::delete_checked`] 결과. `cascade` 가 아니면 `deleted` 는 항상
/// 대상 task id 하나뿐이다.
#[derive(Debug, Clone, Default)]
pub struct TaskDeleteReport {
    pub deleted: Vec<TaskId>,
}

/// [`TaskStore::plan_sweep`] 필터 — 상태 집합 ∩ 경과시간 조건을 모두 만족하는
/// task 만 후보로 삼는다. 둘 다 `None` 이면 워크스페이스 전체가 후보가 되므로,
/// 최소 하나를 지정하도록 강제하는 건 호출자(`Core::task_purge`/CLI) 의 몫이다.
#[derive(Debug, Clone)]
pub struct TaskPurgeFilter {
    /// TaskState::name()과 비교할 상태 이름. None이면 상태를 제한하지 않는다.
    pub states: Option<Vec<String>>,
    /// `now_ms - 기준시각 >= older_than_ms` 인 task 만 후보. 기준시각은 terminal
    /// task 는 `finished_at`, 그 외(Waiting/Ready)는 `created_at`.
    pub older_than_ms: Option<u64>,
    pub now_ms: u64,
}

/// 조회와 실제 삭제가 함께 사용하는 정리 계획.
#[derive(Debug, Clone, Default)]
pub struct TaskSweepPlan {
    /// 필터를 만족하고, Running 이 아니며, 후보 집합 밖에서 참조되지 않아
    /// 안전하게 지울 수 있는 task id.
    pub deleted: Vec<TaskId>,
    /// 필터는 만족했지만 후보 집합 밖의 task 가 여전히 참조 중이라 이번 sweep
    /// 에서는 제외된 task id — 그 참조자가 먼저(또는 같이) 지워지면 이후 sweep
    /// 에서 지워진다.
    pub retained: Vec<TaskId>,
    /// 필터는 만족했지만 호출자가 지키라고 넘긴(`plan_sweep` 의 `keep`) task id. Running 처럼 다룬다.
    pub skipped: Vec<TaskId>,
}

impl TaskStore<'_> {
    /// 참조 무결성 + 상태 제약을 지키는 task 삭제. `raw delete`
    /// (위 [`Self::delete`])는 이 검사들을 전혀 하지 않으므로 직접 호출하면
    /// dangling 참조·영구 `Waiting`·자원 누수를 만들 수 있다 — 호스트/CLI 는
    /// 항상 이 메서드를 거쳐야 한다.
    ///
    /// - `Running` 상태는 `cascade`/`force` 와 무관하게 항상 거부 —
    ///   먼저 `cancel` 로 정리해야 한다.
    /// - 기본(둘 다 `false`): 참조자가 하나라도 있으면 거부하고 그 목록을 반환.
    /// - `cascade`: 전이적 참조자 전부를 함께 지운다. 참조자 중 `Running` 이
    ///   있으면 그 하나 때문에 통째로 거부한다(부분 cascade 없음).
    /// - `force`(비-cascade): 참조 검사만 생략. 대상 자체의 `Running` 제약은
    ///   여전히 적용된다.
    pub fn delete_checked(
        &mut self,
        workspace_id: WorkspaceId,
        id: &TaskId,
        opts: TaskDeleteOpts,
    ) -> Result<TaskDeleteReport> {
        let task = self
            .get(workspace_id, id)?
            .ok_or_else(|| AgentError::TaskNotFound(id.clone()))?;
        if matches!(task.state, TaskState::Running) {
            return Err(AgentError::TaskRunning(id.clone()));
        }

        let all = self.list(workspace_id)?;
        let targets: Vec<TaskId> = if opts.cascade {
            let mut seen: HashSet<TaskId> = HashSet::new();
            seen.insert(id.clone());
            seen.extend(transitive_referencing_task_ids(&all, id));
            for t_id in &seen {
                if let Some(t) = all.iter().find(|t| &t.id == t_id)
                    && matches!(t.state, TaskState::Running)
                {
                    return Err(AgentError::TaskRunning(t_id.clone()));
                }
            }
            seen.into_iter().collect()
        } else {
            if !opts.force {
                let referencers = referencing_task_ids(&all, id);
                if !referencers.is_empty() {
                    return Err(AgentError::TaskReferenced {
                        task: id.clone(),
                        referenced_by: referencers,
                    });
                }
            }
            vec![id.clone()]
        };

        for t_id in &targets {
            self.delete(workspace_id, t_id)?;
        }
        self.prune_graph_records(workspace_id)?;
        Ok(TaskDeleteReport { deleted: targets })
    }

    /// `filter` 를 만족하는 task 중 안전하게 지울 수 있는 것만 골라낸다
    /// (순수 함수, 영속 변경 없음). Running 과 `keep`(`skipped` 로 보고)은 후보에서 제외되고,
    /// 후보 집합 밖에서 여전히 참조되는 task 는 fixed-point 로 반복 제외한다 —
    /// 그래야 "후보 A 를 참조하는 후보 B" 처럼 후보끼리의 참조는 함께 지워지되,
    /// 후보 밖 task 의 참조는 안전하게 보존된다. dry-run 은 이 결과를 그대로
    /// 보여주면 되고, 실제 삭제는 [`Self::apply_sweep_plan`] 이 이어받는다.
    pub fn plan_sweep(
        &self,
        workspace_id: WorkspaceId,
        filter: &TaskPurgeFilter,
        keep: &HashSet<TaskId>,
    ) -> Result<TaskSweepPlan> {
        let all = self.list(workspace_id)?;
        let mut candidates: HashSet<TaskId> = all
            .iter()
            .filter(|t| !matches!(t.state, TaskState::Running))
            .filter(|t| match &filter.states {
                None => true,
                Some(states) => states.iter().any(|s| s == t.state.name()),
            })
            .filter(|t| match filter.older_than_ms {
                None => true,
                Some(threshold) => {
                    let base = t.finished_at.unwrap_or(t.created_at);
                    filter.now_ms.saturating_sub(base) >= threshold
                }
            })
            .map(|t| t.id.clone())
            .collect();
        let mut skipped: Vec<TaskId> = candidates.intersection(keep).cloned().collect();
        skipped.sort();
        candidates.retain(|id| !keep.contains(id));

        let mut eligible = candidates.clone();
        loop {
            let blocked: Vec<TaskId> = eligible
                .iter()
                .filter(|id| {
                    referencing_task_ids(&all, id)
                        .iter()
                        .any(|r| !eligible.contains(r))
                })
                .cloned()
                .collect();
            if blocked.is_empty() {
                break;
            }
            for id in blocked {
                eligible.remove(&id);
            }
        }

        let mut deleted: Vec<TaskId> = eligible.iter().cloned().collect();
        deleted.sort();
        let mut retained: Vec<TaskId> = candidates.difference(&eligible).cloned().collect();
        retained.sort();
        Ok(TaskSweepPlan {
            deleted,
            retained,
            skipped,
        })
    }

    /// [`Self::plan_sweep`] 이 만든 계획을 실제로 적용(삭제)한다. 계획 자체가
    /// 이미 Running 제외 + 참조 안전을 보장하므로 추가 검증 없이 그대로 지운다.
    pub fn apply_sweep_plan(
        &mut self,
        workspace_id: WorkspaceId,
        plan: &TaskSweepPlan,
    ) -> Result<()> {
        for id in &plan.deleted {
            self.delete(workspace_id, id)?;
        }
        self.prune_graph_records(workspace_id)
    }
}
