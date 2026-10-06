//! Task DAG (`TaskGraph`) — task 간 의존성 표현 + 위상정렬.

use std::collections::{HashMap, HashSet};

use super::binding::{InputBinding, field_may_be_absent};
use super::route::{self, EdgeStatus, SkipReason, transition_targets};
use super::{OnFailure, Task, TaskCommand, TaskId, TaskState};
use crate::{AgentError, Result};

pub struct TaskGraph<'a> {
    tasks: HashMap<&'a TaskId, &'a Task>,
    /// 활성화 레코드가 없는 그래프의 task. 실행 대상이 되지 않는다.
    inactive: HashSet<TaskId>,
    /// 전이 대상 → 그 대상을 고를 수 있는 task(제어 엣지).
    controls: HashMap<&'a TaskId, Vec<&'a Task>>,
}

/// Waiting task 의 판정.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Readiness {
    Ready,
    /// 필요한 선행이 성공 결과를 내지 못했다. 실패 정책을 적용한다. v2 task 는 이유를 싣는다.
    Unavailable(Option<SkipReason>),
    /// 들어오는 경로가 모두 선택되지 않았다. 실패 정책을 적용하지 않는다.
    NotSelected,
}

/// v2 binding 이 읽는 source task id. 값을 받는 데이터 의존성이다.
pub fn binding_task_ids(task: &Task) -> Vec<&TaskId> {
    let Some(c) = &task.contract else {
        return Vec::new();
    };
    c.bindings
        .values()
        .flat_map(|b| b.sources().iter().map(|s| &s.from_task))
        .collect()
}

impl<'a> TaskGraph<'a> {
    pub fn build(tasks: &'a [Task]) -> Self {
        let mut map = HashMap::new();
        let mut controls: HashMap<&'a TaskId, Vec<&'a Task>> = HashMap::new();
        for t in tasks {
            map.insert(&t.id, t);
            for target in transition_targets(t) {
                controls.entry(target).or_default().push(t);
            }
        }
        Self {
            tasks: map,
            inactive: HashSet::new(),
            controls,
        }
    }

    /// 아직 활성화되지 않은 그래프의 task 를 지정한다. 이 task 는 readiness 평가에서
    /// 항상 대기다.
    pub fn with_inactive(mut self, inactive: HashSet<TaskId>) -> Self {
        self.inactive = inactive;
        self
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
            // binding source 도 같은 이유로 존재하는 것만 본다.
            let reduce_inputs = match &task.command {
                TaskCommand::Reduce { inputs, .. } => inputs.iter().collect(),
                _ => Vec::new(),
            };
            let control_sources = self
                .controls
                .get(node)
                .map(|v| v.iter().map(|t| &t.id).collect::<Vec<_>>())
                .unwrap_or_default();
            for dep in reduce_inputs
                .into_iter()
                .chain(binding_task_ids(task))
                .chain(control_sources)
            {
                let Some((dep_ref, _)) = self.tasks.get_key_value(dep) else {
                    continue;
                };
                self.visit_cycle_edge(dep_ref, color, stack)?;
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
    /// `depends_on` 뿐 아니라 `Reduce.inputs`, v2 binding source, 전이 대상도 의존성으로 취급한다.
    pub fn downstream_of(&self, task_id: &TaskId) -> Vec<TaskId> {
        let mut out = Vec::new();
        for (id, t) in &self.tasks {
            let is_dep = t.depends_on.iter().any(|d| d == task_id)
                || matches!(&t.command, TaskCommand::Reduce { inputs, .. } if inputs.iter().any(|d| d == task_id))
                || binding_task_ids(t).contains(&task_id)
                || self
                    .controls
                    .get(&t.id)
                    .is_some_and(|sources| sources.iter().any(|s| &s.id == task_id))
                // v2 fallback 은 main 이 실패 없이 끝날 때도 판정해 마감한다.
                || (t.is_typed()
                    && self.tasks.get(task_id).is_some_and(|m| {
                        matches!(&m.on_failure, OnFailure::Fallback { task: Some(fb), .. } if *fb == t.id)
                    }));
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

    /// binding 은 값을 읽으므로 source 자신의 성공이 필요하다. fallback 의 성공은 main
    /// 의 출력을 대신하지 않는다. one_of 는 모든 source 가 종결된 뒤 성공한 것을 쓴다.
    /// 반환: `None` 대기, `Some(true)` 값을 받을 수 없음, `Some(false)` 준비됨.
    fn data_inputs_failed(&self, task: &Task) -> Option<bool> {
        let Some(c) = &task.contract else {
            return Some(false);
        };
        let mut failed = false;
        for (field, binding) in &c.bindings {
            match binding {
                InputBinding::Literal(_) => {}
                InputBinding::FromTask { source, .. } => {
                    match &self.tasks.get(&source.from_task)?.state {
                        TaskState::Succeeded => {}
                        s if s.is_terminal() => failed = true,
                        _ => return None,
                    }
                }
                InputBinding::OneOf { sources, .. } => {
                    let mut succeeded = 0;
                    for s in sources {
                        let state = &self.tasks.get(&s.from_task)?.state;
                        if !state.is_terminal() {
                            return None;
                        }
                        if matches!(state, TaskState::Succeeded) {
                            succeeded += 1;
                        }
                    }
                    if succeeded == 0 && !super::binding::field_may_be_absent(c, field) {
                        failed = true;
                    }
                }
            }
        }
        Some(failed)
    }

    /// `task_id`의 의존성 상태를 평가해 `Ready`로 진행 가능한지 판단.
    /// 반환:
    /// - `Some(TaskState::Ready)` — 모든 dep Succeeded
    /// - `Some(TaskState::Skipped)` — dep 중 Failed/Cancelled/Skipped 존재, 또는 v2 task 의
    ///   들어오는 경로가 모두 선택되지 않음
    /// - `None` — 아직 대기 (dep에 미완료가 있음, 또는 dormant fallback)
    pub fn evaluate_readiness(&self, task_id: &TaskId) -> Option<TaskState> {
        self.readiness(task_id).map(|r| match r {
            Readiness::Ready => TaskState::Ready,
            Readiness::Unavailable(_) | Readiness::NotSelected => TaskState::Skipped,
        })
    }

    /// [`Self::evaluate_readiness`] 의 상세 판정. v2 task 는 전이로 고른 경로를 따른다.
    pub fn readiness(&self, task_id: &TaskId) -> Option<Readiness> {
        let task = self.tasks.get(task_id)?;

        if self.inactive.contains(task_id) {
            return None;
        }
        if task.is_typed()
            && let Some(r) = self.fallback_not_needed(task)
        {
            return Some(r);
        }
        if self.dormant_as_pending_fallback(task_id) {
            return None;
        }
        if task.is_typed() {
            return self.typed_readiness(task);
        }
        let failed = match self.v1_readiness(task)? {
            TaskState::Ready => return Some(Readiness::Ready),
            _ => true,
        };
        failed.then_some(Readiness::Unavailable(None))
    }

    fn v1_readiness(&self, task: &Task) -> Option<TaskState> {
        let data_failed = self.data_inputs_failed(task)?;

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
            return Some(if data_failed {
                TaskState::Skipped
            } else {
                TaskState::Ready
            });
        }
        let mut any_failed = data_failed;
        for dep_id in &task.depends_on {
            let dep = self.tasks.get(dep_id)?;
            match &dep.state {
                TaskState::Succeeded => {}
                TaskState::Failed { .. } | TaskState::Cancelled | TaskState::Skipped => {
                    // main 실패 시 기존 또는 inline fallback의 결과를 대신 본다.
                    if matches!(dep.on_failure, OnFailure::Fallback { .. }) {
                        match self.fallback_state(dep) {
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

impl<'a> TaskGraph<'a> {
    /// v2 fallback 의 main 이 실패 없이 끝났으면 fallback 은 실행하지 않는다. main 이 성공했거나
    /// 선택되지 않았으면 이 fallback 도 선택되지 않은 것이고, 취소·실패 전파로 끝났으면
    /// 실패 정책을 적용한다.
    fn fallback_not_needed(&self, task: &Task) -> Option<Readiness> {
        let main = self.tasks.values().find(|m| {
            matches!(&m.on_failure, OnFailure::Fallback { task: Some(fb), .. } if *fb == task.id)
        })?;
        if !main.state.is_terminal() || matches!(main.state, TaskState::Failed { .. }) {
            return None;
        }
        Some(match route::path_status(main) {
            EdgeStatus::Available | EdgeStatus::NotSelected => Readiness::NotSelected,
            _ => Readiness::Unavailable(Some(SkipReason::UpstreamUnavailable {
                source: main.id.clone(),
                source_state: main.state.name().to_string(),
            })),
        })
    }

    /// main 의 fallback 상태. inline 은 같은 graph 안에서 `metadata.fallback_of == main.id`
    /// 인 task 를 찾는다. 아직 만들지 않았으면 `None`.
    fn fallback_state(&self, main: &Task) -> Option<&TaskState> {
        match &main.on_failure {
            OnFailure::Fallback { task: Some(id), .. } => self.tasks.get(id).map(|t| &t.state),
            OnFailure::Fallback {
                inline: Some(_), ..
            } => self
                .tasks
                .values()
                .find(|t| {
                    t.metadata.get("fallback_of").and_then(|v| v.as_str()) == Some(main.id.as_str())
                })
                .map(|t| &t.state),
            _ => None,
        }
    }

    /// v2 task 의 판정. 제어 엣지(전이)는 하나라도 이 task 를 골라야 진행한다. 순서·데이터
    /// 엣지는 선택되지 않은 선행을 기다리지 않는다(합류). 들어오는 엣지가 모두 선택되지
    /// 않았으면 이 task 도 선택되지 않는다. 선행이 성공 결과를 내지 못했으면 실패 정책을 적용한다.
    fn typed_readiness(&self, task: &Task) -> Option<Readiness> {
        let controls = self.controls.get(&task.id);
        if let Some(sources) = controls
            && let Some(decided) = control_readiness(sources, &task.id)?
        {
            return Some(decided);
        }
        let c = task.contract.as_ref()?;
        let mut tally = PathTally::default();
        for dep_id in &task.depends_on {
            let dep = self.tasks.get(dep_id)?;
            let status = route::path_status(dep);
            if let EdgeStatus::Unavailable { .. } = status
                && self.fallback_covers(dep)?
            {
                tally.see(EdgeStatus::Available);
                continue;
            }
            tally.see(status)?;
        }
        for (field, binding) in &c.bindings {
            match binding {
                InputBinding::Literal(_) => {}
                InputBinding::FromTask { source, .. } => {
                    let s = self.tasks.get(&source.from_task)?;
                    match route::path_status(s) {
                        // 선택되지 않은 source 의 값은 없다. 빠져도 되는 필드만 진행한다.
                        EdgeStatus::NotSelected if !field_may_be_absent(c, field) => {
                            tally.see(EdgeStatus::NotSelected);
                            tally.fail(s.id.clone(), "not_selected".into());
                        }
                        status => {
                            tally.see(status)?;
                        }
                    }
                }
                InputBinding::OneOf { sources, .. } => {
                    let mut succeeded = false;
                    for s in sources {
                        let status = route::path_status(self.tasks.get(&s.from_task)?);
                        succeeded |= status == EdgeStatus::Available;
                        tally.see_merge(status)?;
                    }
                    if !succeeded && !field_may_be_absent(c, field) {
                        let first = sources.first().map(|s| s.from_task.clone());
                        tally.fail(
                            first.unwrap_or_default(),
                            "no one_of source succeeded".into(),
                        );
                    }
                }
            }
        }
        // reduce 는 실패한 입력도 합성하므로 종결만 기다린다.
        if let TaskCommand::Reduce { inputs, .. } = &task.command {
            for id in inputs {
                tally.see_merge(route::path_status(self.tasks.get(id)?))?;
            }
        }
        Some(tally.decide(controls.is_none()))
    }

    /// 실패한 main 을 fallback 이 대신하는가. fallback 이 아직 끝나지 않았으면 `None`(대기).
    fn fallback_covers(&self, main: &Task) -> Option<bool> {
        if !matches!(main.on_failure, OnFailure::Fallback { .. }) {
            return Some(false);
        }
        match self.fallback_state(main) {
            Some(TaskState::Succeeded) => Some(true),
            Some(s) if !s.is_terminal() => None,
            None if matches!(main.state, TaskState::Failed { .. }) => None,
            _ => Some(false),
        }
    }
}

/// 제어 엣지의 판정. 하나라도 골랐으면 `Some(None)`(경로 엣지로 넘어간다), 아직 정해지지
/// 않았으면 `None`(대기).
fn control_readiness(sources: &[&Task], target: &TaskId) -> Option<Option<Readiness>> {
    let mut selected = false;
    let mut failed = None;
    for s in sources {
        match route::control_status(s, target) {
            EdgeStatus::Pending => return None,
            EdgeStatus::Available => selected = true,
            EdgeStatus::NotSelected => {}
            EdgeStatus::Unavailable { source, state } => {
                failed.get_or_insert((source, state));
            }
        }
    }
    Some(match failed {
        Some((source, state)) => Some(unavailable(source, state)),
        None if !selected => Some(Readiness::NotSelected),
        None => None,
    })
}

fn unavailable(source: TaskId, source_state: String) -> Readiness {
    Readiness::Unavailable(Some(SkipReason::UpstreamUnavailable {
        source,
        source_state,
    }))
}

/// 순서·데이터 엣지의 집계.
struct PathTally {
    any: bool,
    all_not_selected: bool,
    failed: Option<(TaskId, String)>,
}

impl Default for PathTally {
    fn default() -> Self {
        Self {
            any: false,
            all_not_selected: true,
            failed: None,
        }
    }
}

impl PathTally {
    /// 엄격한 엣지: 선행이 성공 결과를 내지 못하면 실패다. 대기면 `None`.
    fn see(&mut self, status: EdgeStatus) -> Option<()> {
        self.any = true;
        match status {
            EdgeStatus::Pending => return None,
            EdgeStatus::NotSelected => {}
            EdgeStatus::Available => self.all_not_selected = false,
            EdgeStatus::Unavailable { source, state } => {
                self.all_not_selected = false;
                self.fail(source, state);
            }
        }
        Some(())
    }

    /// 합류 엣지(one_of·reduce 입력): 종결만 기다리고 실패를 직접 일으키지 않는다.
    fn see_merge(&mut self, status: EdgeStatus) -> Option<()> {
        self.any = true;
        match status {
            EdgeStatus::Pending => return None,
            EdgeStatus::NotSelected => {}
            _ => self.all_not_selected = false,
        }
        Some(())
    }

    fn fail(&mut self, source: TaskId, state: String) {
        self.failed.get_or_insert((source, state));
    }

    fn decide(self, no_controls: bool) -> Readiness {
        if no_controls && self.any && self.all_not_selected {
            return Readiness::NotSelected;
        }
        match self.failed {
            Some((source, state)) => unavailable(source, state),
            None => Readiness::Ready,
        }
    }
}

/// `task` 하나가 참조하는 다른 task id 전체 — `depends_on` ∪
/// `OnFailure::Fallback.task` ∪ `TaskCommand::Reduce.inputs` ∪ v2 binding source.
/// `Fallback.inline` 은 생성 시점에 대상이 아직 존재하지 않는 게 정상(실패 전이 시
/// 동적 생성)이므로 제외한다.
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
    out.extend(binding_task_ids(task).into_iter().cloned());
    out.extend(transition_targets(task).into_iter().cloned());
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
