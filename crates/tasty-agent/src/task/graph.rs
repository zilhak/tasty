//! Task DAG (`TaskGraph`) — task 간 의존성 표현 + 위상정렬.

use std::collections::{HashMap, HashSet};

use super::{OnFailure, Task, TaskId, TaskState};
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
                match color.get(dep_ref).copied().unwrap_or(0) {
                    0 => self.dfs_cycle(dep_ref, color, stack)?,
                    1 => {
                        // cycle: stack의 dep_ref 이후 부분을 반환
                        let from = stack.iter().position(|t| *t == dep_ref).unwrap_or(0);
                        let cycle: Vec<TaskId> =
                            stack[from..].iter().map(|t| (*t).clone()).collect();
                        return Err(AgentError::DependencyCycle(cycle));
                    }
                    _ => {}
                }
            }
        }
        color.insert(node, 2);
        stack.pop();
        Ok(())
    }

    /// `task_id`의 직접 downstream (이 task에 의존하는 task들).
    pub fn downstream_of(&self, task_id: &TaskId) -> Vec<TaskId> {
        let mut out = Vec::new();
        for (id, t) in &self.tasks {
            if t.depends_on.iter().any(|d| d == task_id) {
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

    /// `task_id`의 의존성 상태를 평가해 `Ready`로 진행 가능한지 판단.
    /// 반환:
    /// - `Some(TaskState::Ready)` — 모든 dep Succeeded
    /// - `Some(TaskState::Skipped)` — dep 중 Failed/Cancelled/Skipped 존재
    /// - `None` — 아직 대기 (dep에 미완료가 있음)
    pub fn evaluate_readiness(&self, task_id: &TaskId) -> Option<TaskState> {
        let task = self.tasks.get(task_id)?;
        if task.depends_on.is_empty() {
            return Some(TaskState::Ready);
        }
        let mut any_failed = false;
        for dep_id in &task.depends_on {
            let dep = self.tasks.get(dep_id)?;
            match &dep.state {
                TaskState::Succeeded => {}
                TaskState::Failed { .. } | TaskState::Cancelled | TaskState::Skipped => {
                    // dep 가 Fallback 정책이면 fallback task 의 상태를 본다 — 성공 시 dep 충족.
                    // S4: existing(task) + inline(metadata.fallback_of == dep.id) 두 경로 모두 지원.
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

// ============================================================
// 영속 (memory-backed)
// ============================================================
