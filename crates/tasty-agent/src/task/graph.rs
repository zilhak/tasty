//! Task DAG (`TaskGraph`) — task 간 의존성 표현 + 위상정렬.

use std::collections::{HashMap, HashSet};

use super::{OnFailure, Task, TaskCommand, TaskId, TaskState};
use crate::{AgentError, Result};

pub struct TaskGraph<'a> {
    tasks: HashMap<&'a TaskId, &'a Task>,
}

impl<'a> TaskGraph<'a> {
    pub fn build(tasks: &'a [Task]) -> Self {
        let mut map = HashMap::new();
        for t in tasks {
            map.insert(&t.id, t);
        }
        Self { tasks: map }
    }

    /// 사이클 검출. 발견 시 `Err(DependencyCycle)`.
    pub fn detect_cycles(&self) -> Result<()> {
        // DFS 3-color: 0=white(unvisited), 1=gray(in stack), 2=black(done).
        let mut color: HashMap<&TaskId, u8> = HashMap::new();
        let mut stack_path: Vec<&TaskId> = Vec::new();

        for &start in self.tasks.keys() {
            if color.get(start).copied().unwrap_or(0) != 0 {
                continue;
            }
            self.dfs_cycle(start, &mut color, &mut stack_path)?;
        }
        Ok(())
    }

    fn dfs_cycle(
        &self,
        node: &'a TaskId,
        color: &mut HashMap<&'a TaskId, u8>,
        stack: &mut Vec<&'a TaskId>,
    ) -> Result<()> {
        color.insert(node, 1);
        stack.push(node);
        if let Some(task) = self.tasks.get(node) {
            for dep in &task.depends_on {
                let dep_ref = self
                    .tasks
                    .get_key_value(dep)
                    .map(|(k, _)| *k)
                    .ok_or_else(|| AgentError::UnknownDependency(dep.clone()))?;
                self.visit_cycle_edge(dep_ref, color, stack)?;
            }
            // 옛 Reduce 레코드의 없는 참조가 무관한 작업 생성까지 막지 않도록
            // 존재하는 입력만 순회한다. 신규 참조 검증은 TaskStore::create가 담당한다.
            if let TaskCommand::Reduce { inputs, .. } = &task.command {
                for dep in inputs {
                    let Some((dep_ref, _)) = self.tasks.get_key_value(dep) else {
                        continue;
                    };
                    self.visit_cycle_edge(dep_ref, color, stack)?;
                }
            }
        }
        color.insert(node, 2);
        stack.pop();
        Ok(())
    }

    fn visit_cycle_edge(
        &self,
        dep_ref: &'a TaskId,
        color: &mut HashMap<&'a TaskId, u8>,
        stack: &mut Vec<&'a TaskId>,
    ) -> Result<()> {
        match color.get(dep_ref).copied().unwrap_or(0) {
            0 => self.dfs_cycle(dep_ref, color, stack)?,
            1 => {
                let from = stack.iter().position(|t| *t == dep_ref).unwrap_or(0);
                let cycle: Vec<TaskId> = stack[from..].iter().map(|t| (*t).clone()).collect();
                return Err(AgentError::DependencyCycle(cycle));
            }
            _ => {}
        }
        Ok(())
    }

    /// `task_id`의 직접 downstream (이 task에 의존하는 task들).
    /// `depends_on` 뿐 아니라 `Reduce.inputs` 도 암묵적 의존성으로 취급한다.
    pub fn downstream_of(&self, task_id: &TaskId) -> Vec<TaskId> {
        let mut out = Vec::new();
        for (id, t) in &self.tasks {
            let is_dep = t.depends_on.iter().any(|d| d == task_id)
                || matches!(&t.command, TaskCommand::Reduce { inputs, .. } if inputs.iter().any(|d| d == task_id));
            if is_dep {
                out.push((*id).clone());
            }
        }
        out
    }

    /// `task_id`의 transitive downstream.
    pub fn transitive_downstream(&self, task_id: &TaskId) -> Vec<TaskId> {
        let mut seen: HashSet<TaskId> = HashSet::new();
        let mut queue: Vec<TaskId> = self.downstream_of(task_id);
        let mut out = Vec::new();
        while let Some(cur) = queue.pop() {
            if !seen.insert(cur.clone()) {
                continue;
            }
            out.push(cur.clone());
            queue.extend(self.downstream_of(&cur));
        }
        out
    }

    /// `task_id`가 어떤 main task 의 `on_failure = Fallback{task: Some(task_id)}`
    /// 대상이면서, 그 main 이 아직 `Failed` 로 전이하지 않은 경우 `true`.
    /// `Fallback` 은 "main 이 실패했을 때만 도는 대체 경로" 계약이므로, main 이
    /// 실패하기 전(또는 실패 없이 Succeeded/Cancelled/Skipped 로 끝난 뒤)까지는
    /// 이 fallback 을 `depends_on` 유무와 무관하게 Ready 로 올리면 안 된다 —
    /// main 이 Failed 로 전이하는 순간의 `advance_existing_fallback` 승격 경로가
    /// 유일한 정식 진입로다.
    ///
    /// 예약된 fallback은 main 생성 전에도 대기시켜 두 생성 호출 사이의 조기 실행을 막는다.
    fn dormant_as_pending_fallback(&self, task_id: &TaskId) -> bool {
        if self
            .tasks
            .get(task_id)
            .is_some_and(|t| t.reserved_for_fallback)
        {
            return true;
        }
        self.tasks.values().any(|main| {
            matches!(
                &main.on_failure,
                OnFailure::Fallback { task: Some(fb_id), .. } if fb_id == task_id
            ) && !matches!(main.state, TaskState::Failed { .. })
        })
    }

    /// `task_id`의 의존성 상태를 평가해 `Ready`로 진행 가능한지 판단.
    /// 반환:
    /// - `Some(TaskState::Ready)` — 모든 dep Succeeded
    /// - `Some(TaskState::Skipped)` — dep 중 Failed/Cancelled/Skipped 존재
    /// - `None` — 아직 대기 (dep에 미완료가 있음, 또는 dormant fallback)
    pub fn evaluate_readiness(&self, task_id: &TaskId) -> Option<TaskState> {
        let task = self.tasks.get(task_id)?;

        if self.dormant_as_pending_fallback(task_id) {
            return None;
        }

        // Reduce는 실패 결과도 합성하므로 성공 여부와 무관하게 모든 입력의 종결을 기다린다.
        if let TaskCommand::Reduce { inputs, .. } = &task.command {
            for input_id in inputs {
                let input = self.tasks.get(input_id)?;
                if !input.state.is_terminal() {
                    return None;
                }
            }
        }

        if task.depends_on.is_empty() {
            return Some(TaskState::Ready);
        }
        let mut any_failed = false;
        for dep_id in &task.depends_on {
            let dep = self.tasks.get(dep_id)?;
            match &dep.state {
                TaskState::Succeeded => {}
                TaskState::Failed { .. } | TaskState::Cancelled | TaskState::Skipped => {
                    // main 실패 시 기존 또는 inline fallback의 결과를 대신 본다.
                    if let OnFailure::Fallback {
                        task: fb_id,
                        inline,
                    } = &dep.on_failure
                    {
                        let fb_state = if let Some(id) = fb_id {
                            self.tasks.get(id).map(|t| &t.state)
                        } else if inline.is_some() {
                            // inline: 같은 graph 안에서 metadata.fallback_of == dep.id 인 task 찾기.
                            self.tasks
                                .values()
                                .find(|t| {
                                    t.metadata.get("fallback_of").and_then(|v| v.as_str())
                                        == Some(dep.id.as_str())
                                })
                                .map(|t| &t.state)
                        } else {
                            None
                        };
                        match fb_state {
                            Some(TaskState::Succeeded) => continue,
                            Some(
                                TaskState::Failed { .. }
                                | TaskState::Cancelled
                                | TaskState::Skipped,
                            ) => any_failed = true,
                            // fallback 진행 중(또는 미존재 — inline 미materialize 포함) → 대기.
                            _ => return None,
                        }
                    } else {
                        any_failed = true;
                    }
                }
                _ => return None,
            }
        }
        if any_failed {
            Some(TaskState::Skipped)
        } else {
            Some(TaskState::Ready)
        }
    }
}

/// `task` 하나가 참조하는 다른 task id 전체 — `depends_on` ∪
/// `OnFailure::Fallback.task` ∪ `TaskCommand::Reduce.inputs`. `Fallback.inline`
/// 은 생성 시점에 대상이 아직 존재하지 않는 게 정상(실패 전이 시 동적 생성)
/// 이므로 제외한다.
pub fn referenced_task_ids(task: &Task) -> Vec<TaskId> {
    let mut out = task.depends_on.clone();
    if let OnFailure::Fallback {
        task: Some(fb_id), ..
    } = &task.on_failure
    {
        out.push(fb_id.clone());
    }
    if let TaskCommand::Reduce { inputs, .. } = &task.command {
        out.extend(inputs.iter().cloned());
    }
    out
}

/// `target_id` 를 직접 참조하는 task id 전체 (역방향, 1-hop).
pub fn referencing_task_ids(tasks: &[Task], target_id: &TaskId) -> Vec<TaskId> {
    tasks
        .iter()
        .filter(|t| referenced_task_ids(t).iter().any(|r| r == target_id))
        .map(|t| t.id.clone())
        .collect()
}

/// `target_id` 를 참조하는 task 전체 (직접 + transitive) — cascade 삭제용.
/// `target_id` 를 참조하는 X, X 를 참조하는 Y, ... 를 전부 모은다. `target_id`
/// 자신은 포함하지 않는다(호출자가 필요하면 별도로 더한다).
pub fn transitive_referencing_task_ids(tasks: &[Task], target_id: &TaskId) -> Vec<TaskId> {
    let mut seen: HashSet<TaskId> = HashSet::new();
    let mut queue: Vec<TaskId> = referencing_task_ids(tasks, target_id);
    let mut out = Vec::new();
    while let Some(cur) = queue.pop() {
        if !seen.insert(cur.clone()) {
            continue;
        }
        out.push(cur.clone());
        queue.extend(referencing_task_ids(tasks, &cur));
    }
    out
}
