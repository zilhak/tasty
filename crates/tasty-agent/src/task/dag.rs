//! DAG 그룹 — 한 workspace 의 flat 한 task 목록을 "서로 무관한 그래프" 단위로 쪼갠 뷰.
//!
//! Tasty 의 영속 모델에는 DAG 라는 1급 레코드가 없다. `Task` 는 workspace 에만 속하고,
//! `TaskStore::list(workspace_id)` 는 그 workspace 의 task 를 통째로 돌려준다. 그런데
//! 한 workspace 안에서 서로 무관한 그래프를 여럿 돌리는 사용(conductor 류 다중 에이전트)
//! 이 정상이라, "workspace = DAG" 로 간주하면 곧바로 어긋난다.
//!
//! 그래서 DAG 를 **영속 스키마를 바꾸지 않고 도출(derive)** 한다 — `Task` 에 필드를
//! 추가하면 이미 저장된 task 가 전부 "DAG 없음" 으로 떨어져 기존 그래프가 목록에서
//! 사라지지만, 도출은 기존 task 를 자동으로 편입시켜 마이그레이션이 필요 없다.
//!
//! 도출 규칙(우선순위 순):
//! 1. `task.metadata.dag` 가 문자열이면 그 값이 그룹 키 (**explicit**). 연결성과 무관하게
//!    같은 키끼리 한 DAG 로 묶인다.
//! 2. 나머지는 **약연결 컴포넌트**로 자동 그룹핑 (**derived**). 엣지는 무방향으로 보며,
//!    [`referenced_task_ids`] 3종(`depends_on` ∪ `Fallback.task` ∪ `Reduce.inputs`) 에
//!    `metadata.fallback_of` 역참조를 더한 4종이다 — inline fallback 으로 동적 생성된
//!    task 가 원래 DAG 에서 떨어져 나가지 않게 하려면 역참조가 필요하다. 이 4종은
//!    `collect_graph_edges`(호스트의 `agent.task_graph` 렌더)가 그리는 엣지와 같다.
//!
//! `metadata` 는 이미 `semaphore`/`lease`/`fallback_of` 가 쓰는 확장 지점이라 같은 관례를
//! 따른다.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use serde::Serialize;
use tasty_utils::id::WorkspaceId;

use super::{Task, TaskGraph, TaskId, TaskState, referenced_task_ids};
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
}

impl DagStateCounts {
    fn add(&mut self, state: &TaskState) {
        match state {
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

    /// DAG 하나의 대표 상태. 판정 순서는 화면의 상태칩 색과 직결되므로 고정이다:
    /// `running` 하나라도 있으면 `running` → `failed` 하나라도 있으면 `failed` →
    /// 전부 terminal 이면 `succeeded`(전부 succeeded) 또는 `skipped`(cancelled/skipped
    /// 섞임) → `ready` 하나라도 있으면 `ready` → 그 외 `waiting`.
    fn rollup(&self) -> &'static str {
        if self.running > 0 {
            return "running";
        }
        if self.failed > 0 {
            return "failed";
        }
        // 여기 도달하면 running/failed 는 0 이므로, 비-terminal 로 남은 건
        // waiting/ready/unknown 뿐이다.
        if self.waiting == 0 && self.ready == 0 && self.unknown == 0 {
            return if self.cancelled == 0 && self.skipped == 0 {
                "succeeded"
            } else {
                "skipped"
            };
        }
        if self.ready > 0 {
            return "ready";
        }
        "waiting"
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
    // 1) explicit: metadata.dag 가 문자열인 task 는 연결성과 무관하게 그 키로 묶는다.
    let mut explicit: BTreeMap<&str, Vec<&Task>> = BTreeMap::new();
    let mut derived_pool: Vec<&Task> = Vec::new();
    for t in tasks {
        match explicit_dag_key(t) {
            Some(key) => explicit.entry(key).or_default().push(t),
            None => derived_pool.push(t),
        }
    }

    let mut out: Vec<DagSummary> = explicit
        .into_iter()
        .map(|(key, group)| {
            summarize(
                workspace_id,
                format!("{EXPLICIT_ID_PREFIX}{key}"),
                "explicit",
                Some(key),
                &group,
            )
        })
        .collect();

    // 2) derived: 나머지를 약연결 컴포넌트로 union-find.
    for group in weakly_connected_components(&derived_pool) {
        // derived id 의 root 는 그룹 내 `(created_at, id)` 사전순 최소 task —
        // 같은 task 집합이면 호출 때마다 같은 id 가 나와야 한다.
        let root = group
            .iter()
            .min_by_key(|t| (t.created_at, t.id.as_str()))
            .expect("component is never empty");
        let id = format!("{DERIVED_ID_PREFIX}{}", root.id);
        out.push(summarize(workspace_id, id, "derived", None, &group));
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

fn summarize(
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
        state_counts.add(&t.state);
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
    let root_task_ids: Vec<TaskId> = sorted
        .iter()
        .filter(|t| {
            !referenced_task_ids(t)
                .iter()
                .any(|r| member_ids.contains(r.as_str()))
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
            // 작은 인덱스를 root 로 고정 — rank 균형보다 결정론이 중요하다.
            let (lo, hi) = if ra < rb { (ra, rb) } else { (rb, ra) };
            self.parent[hi] = lo;
        }
    }
}
