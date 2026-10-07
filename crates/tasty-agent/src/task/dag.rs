//! 워크스페이스의 작업을 DAG별로 묶은 조회 결과. 별도 DAG 레코드는 저장하지 않는다.
//!
//! metadata.dag가 공백뿐이지 않은 문자열이면 같은 키로 묶고, 나머지는 약연결 컴포넌트로 나눈다.
//! depends_on·Fallback.task·Reduce.inputs와 metadata.fallback_of 관계를 사용한다.
//! fallback_of도 읽어 동적으로 만든 fallback이 원래 그룹에 포함되게 한다.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use serde::Serialize;
use tasty_utils::id::WorkspaceId;

use super::{
    OnFailure, Task, TaskCommand, TaskGraph, TaskId, TaskState, binding_task_ids,
    referenced_task_ids,
};
use crate::AgentError;

/// `DagSummary::id` 접두 — explicit(=`metadata.dag`) 그룹.
const EXPLICIT_ID_PREFIX: &str = "d:";
/// `DagSummary::id` 접두 — derived(=약연결 컴포넌트) 그룹.
const DERIVED_ID_PREFIX: &str = "c:";

/// `TaskState` 8종별 개수. 화면의 상태칩 집계에 그대로 쓰인다.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct DagStateCounts {
    pub waiting: usize,
    pub ready: usize,
    pub running: usize,
    pub succeeded: usize,
    pub failed: usize,
    pub cancelled: usize,
    pub skipped: usize,
    pub unknown: usize,
    /// `skipped` 중 경로가 선택되지 않아 끝난 v2 task. 실패가 아니다.
    pub not_selected: usize,
    /// `failed` 중 같은 그룹의 fallback 이 성공해 대신한 task. rollup 에서 실패로 세지 않는다.
    pub recovered: usize,
    /// `waiting` 중 저절로는 더 실행되지 않는 task. 선행 결과를 쓸 수 없는데 실패 정책이
    /// 건너뛰지 않고 대기로 남겨 둔 task(자기 실패에 쓸 fallback 을 둔 v1 task)에서 시작해,
    /// 그런 task 나 `unknown` task 를 기다리는 대기(하류, 입력으로 기다리는 Reduce, main 이
    /// 끝나지 않아 깨어날 수 없는 fallback)까지 전이적으로 닫는다. rollup 은 이 수를 진행
    /// 가능한 대기에서 뺀다.
    pub blocked: usize,
    /// 그룹의 끝 task(그룹 안에 하류가 없는 task) 중 성공한 수. fallback 이 대신 성공한 끝도
    /// 센다. fallback task 자신은 main 을 대신하므로 끝으로 세지 않는다. 실패가 섞인 DAG 가
    /// 부분 오류인지(끝까지 성공한 갈래가 있는지) 가르는 데 쓴다.
    pub succeeded_ends: usize,
}

impl DagStateCounts {
    fn add(&mut self, task: &Task) {
        if super::route::is_not_selected(task) {
            self.not_selected += 1;
        }
        match &task.state {
            TaskState::Waiting => self.waiting += 1,
            TaskState::Ready => self.ready += 1,
            TaskState::Running => self.running += 1,
            TaskState::Succeeded => self.succeeded += 1,
            TaskState::Failed { .. } => self.failed += 1,
            TaskState::Cancelled => self.cancelled += 1,
            TaskState::Skipped => self.skipped += 1,
            TaskState::Unknown => self.unknown += 1,
        }
    }

    /// DAG 하나의 대표 상태. 판정 순서는 화면의 상태칩 색과 직결되므로 고정이다.
    ///
    /// 저절로 진행할 수 있는 task 가 있으면 실패가 섞여 있어도 진행 상태다: `running` →
    /// `ready` → `waiting`(막히지 않은 대기). `unknown` 은 사람이 retry·cancel 해야 진행되므로
    /// 진행할 수 있는 것으로 보지 않는다. 더 진행할 수 없으면 fallback 이 대신하지 못한 실패가
    /// 있을 때 `partially_failed`(성공한 끝 task 가 하나라도 있음, `succeeded_ends`) 또는
    /// `failed`(끝까지 성공한 갈래 없음)다.
    /// 실패 없이 막힌 대기나 `unknown` 이 남으면 `waiting` 이다(끝난 것으로 보이면 개입이 필요한
    /// task 가 가려진다). 나머지는 전부 terminal 이며 `succeeded`(succeeded 와 선택되지 않은
    /// 경로뿐) 또는 `skipped`(cancelled 나 skip 된 task 섞임)다.
    ///
    /// 반환 가능한 상태는 위 일곱 가지이며 cancelled와 unknown은 직접 반환하지 않는다.
    pub fn rollup(&self) -> &'static str {
        if self.running > 0 {
            return "running";
        }
        if self.ready > 0 {
            return "ready";
        }
        if self.waiting > self.blocked {
            return "waiting";
        }
        if self.failed > self.recovered {
            return if self.succeeded_ends > 0 {
                "partially_failed"
            } else {
                "failed"
            };
        }
        if self.blocked > 0 || self.unknown > 0 {
            return "waiting";
        }
        if self.cancelled == 0 && self.skipped <= self.not_selected {
            "succeeded"
        } else {
            "skipped"
        }
    }
}

/// 한 DAG 의 요약. `agent.dag_list` 응답 원소이자 `agent.dag_get` 의 헤더.
///
/// **열거 범위 주의**: 이 타입 자체는 `&[Task]` 만 보지만, 호스트의 `agent.dag_list` 는
/// *지금 살아있는 workspace* 만 순회한다 — 삭제된 workspace 에 남은 고아 task 는 목록에
/// 뜨지 않는다(그 정리는 부팅 시 자동 GC 의 책임이다).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DagSummary {
    /// 그룹 식별자. explicit 은 `d:<metadata.dag 값>`, derived 는 `c:<root_task_id>`.
    /// 접두 덕에 두 출처가 섞여도 충돌하지 않는다.
    ///
    /// 완전한 신원은 `(workspace_id, id)` 다 — explicit 키는 사용자가 정하므로 서로 다른
    /// workspace 가 같은 `metadata.dag` 값을 쓰면 `id` 만으로는 구분되지 않는다.
    pub id: String,
    pub workspace_id: WorkspaceId,
    /// 표시 이름. `metadata.dag_name` > explicit 그룹 키 > root task 의 `name` 순.
    pub name: String,
    /// `"explicit"`(=`metadata.dag` 로 명시) 또는 `"derived"`(=연결성에서 도출).
    pub source: &'static str,
    pub task_count: usize,
    pub state_counts: DagStateCounts,
    /// [`DagStateCounts::rollup`] 결과.
    pub rollup_state: &'static str,
    /// 소속 task 의 `created_at` 최소.
    pub created_at: u64,
    /// 소속 task 의 (`finished_at` ∪ `started_at` ∪ `created_at`) 최대.
    pub updated_at: u64,
    /// 그룹 안에서 자기 그룹 내 다른 task 를 참조하지 않는 task 들 (그래프의 source).
    /// `(created_at, id)` 오름차순.
    pub root_task_ids: Vec<TaskId>,
    /// 그룹 부분집합만으로 사이클 검출을 돌린 결과.
    pub has_cycle: bool,
    /// 소속 task id 전부. `(created_at, id)` 오름차순.
    pub task_ids: Vec<TaskId>,
}

/// `&[Task]` 를 DAG 그룹으로 쪼갠다. `TaskStore`/memory 에 의존하지 않는 순수 함수.
///
/// 반환 순서는 `(workspace_id, id)` 오름차순으로 고정된다. 같은 task 집합을 몇 번 넣어도
/// 같은 `id` 가 같은 순서로 나온다 — 화면이 선택 상태를 `id` 로 들고 폴링마다 재계산하기
/// 때문에 이 결정론이 요구사항이다.
pub fn group_tasks_into_dags(tasks: &[Task]) -> Vec<DagSummary> {
    let mut out = Vec::new();
    // workspace 를 넘는 의존은 존재하지 않으므로(store 가 workspace 단위) 그룹핑도
    // workspace 안에서만 한다. 여러 workspace 의 task 를 한꺼번에 넣어도 안전하다.
    let mut by_workspace: BTreeMap<WorkspaceId, Vec<&Task>> = BTreeMap::new();
    for t in tasks {
        by_workspace.entry(t.workspace_id).or_default().push(t);
    }
    for (workspace_id, ws_tasks) in by_workspace {
        out.extend(group_within_workspace(workspace_id, &ws_tasks));
    }
    out
}

fn group_within_workspace(workspace_id: WorkspaceId, tasks: &[&Task]) -> Vec<DagSummary> {
    let mut explicit: BTreeMap<&str, Vec<&Task>> = BTreeMap::new();
    let mut derived_pool: Vec<&Task> = Vec::new();
    for t in tasks {
        match explicit_dag_key(t) {
            Some(key) => explicit.entry(key).or_default().push(t),
            None => derived_pool.push(t),
        }
    }

    // 막힌 대기 판정은 그룹 밖 선행도 봐야 하므로 workspace 전체로 한다.
    let owned: Vec<Task> = tasks.iter().map(|t| (*t).clone()).collect();
    let blocked = blocked_task_ids(&owned);

    let mut out: Vec<DagSummary> = explicit
        .into_iter()
        .map(|(key, group)| {
            summarize(
                &blocked,
                workspace_id,
                format!("{EXPLICIT_ID_PREFIX}{key}"),
                "explicit",
                Some(key),
                &group,
            )
        })
        .collect();

    for group in weakly_connected_components(&derived_pool) {
        // derived id 의 root 는 그룹 내 `(created_at, id)` 사전순 최소 task —
        // 같은 task 집합이면 호출 때마다 같은 id 가 나와야 한다.
        let root = group
            .iter()
            .min_by_key(|t| (t.created_at, t.id.as_str()))
            .expect("component is never empty");
        let id = format!("{DERIVED_ID_PREFIX}{}", root.id);
        out.push(summarize(
            &blocked,
            workspace_id,
            id,
            "derived",
            None,
            &group,
        ));
    }

    out.sort_by(|a, b| a.id.cmp(&b.id));
    out
}

/// `metadata.dag` 가 (공백만은 아닌) 문자열이면 그 값.
fn explicit_dag_key(task: &Task) -> Option<&str> {
    task.metadata
        .get("dag")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
}

/// derived pool 을 약연결 컴포넌트로 쪼갠다. 엣지는 무방향이며
/// `referenced_task_ids` 3종 + `metadata.fallback_of` 역참조.
///
/// pool 밖(=explicit 로 이미 묶인 task)이나 존재하지 않는 id 를 가리키는 참조는 엣지가
/// 되지 않는다 — explicit 이 연결성보다 우선하고, dangling 참조는 그룹을 만들 근거가 못
/// 된다.
fn weakly_connected_components<'a>(pool: &[&'a Task]) -> Vec<Vec<&'a Task>> {
    let index: HashMap<&str, usize> = pool
        .iter()
        .enumerate()
        .map(|(i, t)| (t.id.as_str(), i))
        .collect();
    let mut uf = UnionFind::new(pool.len());

    for (i, t) in pool.iter().enumerate() {
        for referenced in referenced_task_ids(t) {
            if let Some(&j) = index.get(referenced.as_str()) {
                uf.union(i, j);
            }
        }
        // inline fallback 으로 동적 생성된 task 는 main 쪽에 대상 id 가 없다 —
        // 관계가 기록된 유일한 자리가 fallback task 자신의 `metadata.fallback_of` 다.
        if let Some(main_id) = t.metadata.get("fallback_of").and_then(|v| v.as_str())
            && let Some(&j) = index.get(main_id)
        {
            uf.union(i, j);
        }
    }

    let mut components: BTreeMap<usize, Vec<&'a Task>> = BTreeMap::new();
    for (i, t) in pool.iter().enumerate() {
        components.entry(uf.find(i)).or_default().push(t);
    }
    components.into_values().collect()
}

/// 실패한 task 를 그룹 안의 fallback 이 대신 성공했는가. fallback 도 실패했으면 그 fallback 의
/// fallback 을 따라간다. 그룹 밖 fallback 은 보지 않는다. `seen` 은 fallback 순환을 끊는다.
fn recovered_by_fallback<'a>(task: &'a Task, group: &[&'a Task], seen: &mut Vec<&'a str>) -> bool {
    if !matches!(task.state, TaskState::Failed { .. }) || seen.contains(&task.id.as_str()) {
        return false;
    }
    seen.push(task.id.as_str());
    let fallback = match &task.on_failure {
        OnFailure::Fallback { task: Some(id), .. } => group.iter().find(|t| &t.id == id),
        OnFailure::Fallback {
            inline: Some(_), ..
        } => group.iter().find(|t| {
            t.metadata.get("fallback_of").and_then(|v| v.as_str()) == Some(task.id.as_str())
        }),
        _ => None,
    };
    match fallback {
        Some(fb) if fb.state == TaskState::Succeeded => true,
        Some(fb) => recovered_by_fallback(fb, group, seen),
        None => false,
    }
}

/// 그룹의 끝 task: 그룹 안 다른 task 가 결과·순서로 기다리지 않고 전이로 이어지는 task 도
/// 없다. fallback 간선과 경로가 선택되지 않아 건너뛴 task 는 하류로 보지 않는다. fallback task 자신(`fallback_task_ids`)은 main 을
/// 대신하므로 끝이 아니며, main 의 끝 여부와 복구 여부로 센다.
fn is_end(
    task: &Task,
    group: &[&Task],
    member_ids: &BTreeSet<&str>,
    fallback_task_ids: &BTreeSet<&str>,
) -> bool {
    if fallback_task_ids.contains(task.id.as_str()) {
        return false;
    }
    let selectable = |id: &str| {
        group
            .iter()
            .any(|m| m.id.as_str() == id && !super::route::is_not_selected(m))
    };
    let flows_on = super::route::transition_targets(task)
        .into_iter()
        .any(|t| member_ids.contains(t.as_str()) && selectable(t.as_str()));
    let awaited = group.iter().any(|m| {
        m.id != task.id && !super::route::is_not_selected(m) && {
            let forward = forward_refs(m);
            referenced_task_ids(m)
                .iter()
                .any(|r| r == &task.id && !forward.contains(&r.as_str()))
        }
    });
    !flows_on && !awaited
}

/// task 가 참조하지만 흐름은 task 에서 그쪽으로 가는 id: 전이 대상과 fallback task.
fn forward_refs(task: &Task) -> Vec<&str> {
    let mut out: Vec<&str> = super::route::transition_targets(task)
        .into_iter()
        .map(String::as_str)
        .collect();
    if let OnFailure::Fallback { task: Some(fb), .. } = &task.on_failure {
        out.push(fb.as_str());
    }
    out
}

/// task 가 실행되려면 먼저 끝나야 하는 task id: `depends_on`·binding·`Reduce.inputs`, 자기를
/// 전이 대상으로 둔 task, 자기가 fallback 인 main.
fn awaited_ids<'a>(task: &'a Task, all: &'a [Task]) -> Vec<&'a str> {
    let mut out: Vec<&str> = task.depends_on.iter().map(|id| id.as_str()).collect();
    if let TaskCommand::Reduce { inputs, .. } = &task.command {
        out.extend(inputs.iter().map(|id| id.as_str()));
    }
    out.extend(binding_task_ids(task).into_iter().map(|id| id.as_str()));
    out.extend(
        all.iter()
            .filter(|m| m.id != task.id && forward_refs(m).contains(&task.id.as_str()))
            .map(|m| m.id.as_str()),
    );
    if let Some(main) = task.metadata.get("fallback_of").and_then(|v| v.as_str()) {
        out.push(main);
    }
    out
}

/// 저절로는 더 실행되지 않는 Waiting task 의 id. 선행 결과를 쓸 수 없는데 대기로 남은 task
/// (readiness `Unavailable`)에서 시작해, 막힌 task 나 `unknown` task 를 기다리는 Waiting task 를
/// 고정점까지 더한다. 영구 대기 main 의 dormant fallback 과 막힌 입력을 기다리는 Reduce 도
/// 이렇게 닫힌다.
fn blocked_task_ids(all: &[Task]) -> BTreeSet<String> {
    let graph = TaskGraph::build(all);
    let mut blocked: BTreeSet<String> = all
        .iter()
        .filter(|t| {
            t.state == TaskState::Waiting
                && matches!(
                    graph.readiness(&t.id),
                    Some(super::graph::Readiness::Unavailable(_))
                )
        })
        .map(|t| t.id.to_string())
        .collect();
    let unknown: BTreeSet<&str> = all
        .iter()
        .filter(|t| t.state == TaskState::Unknown)
        .map(|t| t.id.as_str())
        .collect();
    let waiting: Vec<(&Task, Vec<&str>)> = all
        .iter()
        .filter(|t| t.state == TaskState::Waiting)
        .map(|t| (t, awaited_ids(t, all)))
        .collect();
    loop {
        let before = blocked.len();
        for (t, awaited) in &waiting {
            if !blocked.contains(t.id.as_str())
                && awaited
                    .iter()
                    .any(|a| blocked.contains(*a) || unknown.contains(a))
            {
                blocked.insert(t.id.to_string());
            }
        }
        if blocked.len() == before {
            return blocked;
        }
    }
}

fn summarize(
    blocked: &BTreeSet<String>,
    workspace_id: WorkspaceId,
    id: String,
    source: &'static str,
    explicit_key: Option<&str>,
    group: &[&Task],
) -> DagSummary {
    let mut sorted: Vec<&Task> = group.to_vec();
    sorted.sort_by(|a, b| (a.created_at, &a.id).cmp(&(b.created_at, &b.id)));

    let mut state_counts = DagStateCounts::default();
    let mut created_at = u64::MAX;
    let mut updated_at = 0u64;
    for t in &sorted {
        state_counts.add(t);
        if recovered_by_fallback(t, &sorted, &mut Vec::new()) {
            state_counts.recovered += 1;
        }
        if blocked.contains(t.id.as_str()) {
            state_counts.blocked += 1;
        }
        created_at = created_at.min(t.created_at);
        updated_at = updated_at
            .max(t.finished_at.unwrap_or(0))
            .max(t.started_at.unwrap_or(0))
            .max(t.created_at);
    }
    if created_at == u64::MAX {
        created_at = 0;
    }

    // 이름은 그룹 내 결정론적 최소 task 부터 훑어 첫 `metadata.dag_name` 을 채택한다.
    let name = sorted
        .iter()
        .find_map(|t| {
            t.metadata
                .get("dag_name")
                .and_then(|v| v.as_str())
                .map(str::trim)
                .filter(|s| !s.is_empty())
        })
        .map(str::to_string)
        .or_else(|| explicit_key.map(str::to_string))
        .or_else(|| sorted.first().map(|t| t.name.clone()))
        .unwrap_or_default();

    let member_ids: BTreeSet<&str> = sorted.iter().map(|t| t.id.as_str()).collect();
    // 전이와 fallback 은 앞 task 가 뒤 task 를 참조하지만 흐름은 앞에서 뒤로 간다
    // (`task_graph` 의 transition·fallback 간선 방향). 뒤 task 는 source 가 아니다.
    let flows_from_member: BTreeSet<&str> = sorted
        .iter()
        .flat_map(|t| forward_refs(t))
        .chain(
            sorted
                .iter()
                .filter(|t| {
                    t.metadata
                        .get("fallback_of")
                        .and_then(|v| v.as_str())
                        .is_some_and(|main| member_ids.contains(main))
                })
                .map(|t| t.id.as_str()),
        )
        .collect();
    // main 을 대신하는 fallback task: `Fallback.task` 대상과 `fallback_of` 로 main 을 가리키는 것.
    let fallback_task_ids: BTreeSet<&str> = sorted
        .iter()
        .filter_map(|t| match &t.on_failure {
            OnFailure::Fallback { task: Some(fb), .. } => Some(fb.as_str()),
            _ => None,
        })
        .chain(
            sorted
                .iter()
                .filter(|t| {
                    t.metadata
                        .get("fallback_of")
                        .and_then(|v| v.as_str())
                        .is_some_and(|main| member_ids.contains(main))
                })
                .map(|t| t.id.as_str()),
        )
        .collect();
    state_counts.succeeded_ends = sorted
        .iter()
        .filter(|t| is_end(t, &sorted, &member_ids, &fallback_task_ids))
        .filter(|t| {
            t.state == TaskState::Succeeded || recovered_by_fallback(t, &sorted, &mut Vec::new())
        })
        .count();

    let root_task_ids: Vec<TaskId> = sorted
        .iter()
        .filter(|t| {
            let forward = forward_refs(t);
            !flows_from_member.contains(t.id.as_str())
                && !referenced_task_ids(t)
                    .iter()
                    .any(|r| member_ids.contains(r.as_str()) && !forward.contains(&r.as_str()))
        })
        .map(|t| t.id.clone())
        .collect();

    // 사이클 검출은 이 그룹의 task 만으로 돌린다. explicit 그룹은 그룹 밖 task 를
    // `depends_on` 할 수 있어 `UnknownDependency` 가 정상적으로 나올 수 있는데,
    // 그건 사이클이 아니다.
    let subset: Vec<Task> = sorted.iter().map(|t| (*t).clone()).collect();
    let has_cycle = matches!(
        TaskGraph::build(&subset).detect_cycles(),
        Err(AgentError::DependencyCycle(_))
    );

    DagSummary {
        id,
        workspace_id,
        name,
        source,
        task_count: sorted.len(),
        rollup_state: state_counts.rollup(),
        state_counts,
        created_at,
        updated_at,
        root_task_ids,
        has_cycle,
        task_ids: sorted.iter().map(|t| t.id.clone()).collect(),
    }
}

/// 경로 압축 union-find. 약연결 컴포넌트 계산 전용이라 크레이트 밖으로 내지 않는다.
struct UnionFind {
    parent: Vec<usize>,
}

impl UnionFind {
    fn new(n: usize) -> Self {
        Self {
            parent: (0..n).collect(),
        }
    }

    fn find(&mut self, mut x: usize) -> usize {
        while self.parent[x] != x {
            self.parent[x] = self.parent[self.parent[x]];
            x = self.parent[x];
        }
        x
    }

    fn union(&mut self, a: usize, b: usize) {
        let (ra, rb) = (self.find(a), self.find(b));
        if ra != rb {
            let (lo, hi) = if ra < rb { (ra, rb) } else { (rb, ra) };
            self.parent[hi] = lo;
        }
    }
}
