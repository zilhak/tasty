//! Plan — workspace 단위 선언적 work breakdown.
//!
//! Plan 은 "할 일 트리" 의 상태만 보관한다 — 스케줄러도 실행기도 아니다.
//! 한 plan 은 `tasty.plan.<plan_id>` key 한 개로 직렬화 (단일 JSON value).
//! 따라서 step 갱신 한 번이 전체 plan JSON 의 put 한 번에 대응한다.
//!
//! agent.task_* (DAG + scheduler) 와 다른 표면:
//!   - Plan: 상태 기록용. step state 변경은 agent/사용자가 직접 호출.
//!   - Task: 실행기. ready → running → done 을 호스트가 진행.
//!
//! Plan 은 다음 invariants 를 강제한다:
//!   - `id` 는 `[a-z0-9_-]+`, 1..=64 자.
//!   - `steps` 의 step id 는 unique (중복 금지).
//!   - 총 step 수 (flat) 는 [`PLAN_STEP_MAX`] (256) 이하.
//!   - `depends_on` 의 step id 는 같은 plan 내 다른 step 을 가리켜야 함.
//!   - dependency 사이클 금지.

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

use crate::{MemoryEntry, MemoryError, MemoryStore, MemoryValue, PutOpts, Result, Scope};

pub const PLAN_KEY_PREFIX: &str = "tasty.plan.";
pub const PLAN_ID_MAX: usize = 64;
pub const PLAN_STEP_ID_MAX: usize = 64;
pub const PLAN_TITLE_MAX: usize = 256;
pub const PLAN_NOTES_MAX: usize = 2048;
pub const PLAN_STEP_MAX: usize = 256;

/// Plan step state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlanStepState {
    Pending,
    InProgress,
    Completed,
    Failed,
    Skipped,
}

impl Default for PlanStepState {
    fn default() -> Self {
        PlanStepState::Pending
    }
}

/// Plan step. ordering 은 `depends_on` 으로 표현 — 배열 순서는 표시 순서일 뿐.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlanStep {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub state: PlanStepState,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub depends_on: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
}

/// Plan 전체. 직렬화 결과가 `tasty.plan.<id>` entry 의 value 가 된다.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Plan {
    pub id: String,
    pub title: String,
    pub created_at: i64,
    pub created_by: String,
    pub updated_at: i64,
    #[serde(default)]
    pub steps: Vec<PlanStep>,
}

/// id 검증 (plan_id, step_id 공용). `[a-z0-9_-]+`, 1..=max.
pub fn validate_plan_id(id: &str) -> Result<()> {
    validate_id_inner(id, PLAN_ID_MAX, "plan_id")
}

pub fn validate_step_id(id: &str) -> Result<()> {
    validate_id_inner(id, PLAN_STEP_ID_MAX, "step_id")
}

fn validate_id_inner(id: &str, max: usize, label: &str) -> Result<()> {
    if id.is_empty() {
        return Err(MemoryError::InvalidKey(format!("{label}: empty")));
    }
    if id.len() > max {
        return Err(MemoryError::InvalidKey(format!(
            "{label}: too long ({} > {max})",
            id.len()
        )));
    }
    for (i, c) in id.bytes().enumerate() {
        let ok = c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'_' || c == b'-';
        if !ok {
            return Err(MemoryError::InvalidKey(format!(
                "{label}: invalid char {:?} at {i}",
                c as char
            )));
        }
    }
    Ok(())
}

pub fn plan_key(plan_id: &str) -> String {
    format!("{PLAN_KEY_PREFIX}{plan_id}")
}

/// 한 plan 의 모든 invariants 검증. step 수, id 중복, depends_on 유효성, 사이클.
fn validate_plan(plan: &Plan) -> Result<()> {
    validate_plan_id(&plan.id)?;
    if plan.title.is_empty() {
        return Err(MemoryError::InvalidKey("plan title empty".into()));
    }
    if plan.title.len() > PLAN_TITLE_MAX {
        return Err(MemoryError::InvalidKey(format!(
            "plan title too long ({} > {PLAN_TITLE_MAX})",
            plan.title.len()
        )));
    }
    if plan.steps.len() > PLAN_STEP_MAX {
        return Err(MemoryError::InvalidKey(format!(
            "plan step count exceeds cap ({} > {PLAN_STEP_MAX})",
            plan.steps.len()
        )));
    }

    let mut ids: HashSet<&str> = HashSet::with_capacity(plan.steps.len());
    for step in &plan.steps {
        validate_step_id(&step.id)?;
        if step.title.is_empty() {
            return Err(MemoryError::InvalidKey(format!(
                "step '{}': title empty",
                step.id
            )));
        }
        if step.title.len() > PLAN_TITLE_MAX {
            return Err(MemoryError::InvalidKey(format!(
                "step '{}': title too long",
                step.id
            )));
        }
        if let Some(n) = &step.notes
            && n.len() > PLAN_NOTES_MAX
        {
            return Err(MemoryError::InvalidKey(format!(
                "step '{}': notes too long",
                step.id
            )));
        }
        if !ids.insert(step.id.as_str()) {
            return Err(MemoryError::InvalidKey(format!(
                "duplicate step id: {}",
                step.id
            )));
        }
    }

    // depends_on 의 ref 유효성.
    for step in &plan.steps {
        for dep in &step.depends_on {
            if dep == &step.id {
                return Err(MemoryError::InvalidKey(format!(
                    "step '{}' depends on itself",
                    step.id
                )));
            }
            if !ids.contains(dep.as_str()) {
                return Err(MemoryError::InvalidKey(format!(
                    "step '{}' depends on unknown step '{}'",
                    step.id, dep
                )));
            }
        }
    }

    // 사이클 검출 (DFS).
    let adj: HashMap<&str, Vec<&str>> = plan
        .steps
        .iter()
        .map(|s| (s.id.as_str(), s.depends_on.iter().map(|d| d.as_str()).collect()))
        .collect();
    let mut marks: HashMap<&str, Mark> = HashMap::new();
    for step in &plan.steps {
        if dfs_cycle(step.id.as_str(), &adj, &mut marks)? {
            return Err(MemoryError::InvalidKey(format!(
                "dependency cycle involving step '{}'",
                step.id
            )));
        }
    }
    Ok(())
}

fn dfs_cycle<'a>(
    node: &'a str,
    adj: &HashMap<&'a str, Vec<&'a str>>,
    marks: &mut HashMap<&'a str, Mark>,
) -> Result<bool> {
    match marks.get(node) {
        Some(Mark::Done) => return Ok(false),
        Some(Mark::Visiting) => return Ok(true),
        None => {}
    }
    marks.insert(node, Mark::Visiting);
    if let Some(deps) = adj.get(node) {
        for dep in deps {
            if dfs_cycle(dep, adj, marks)? {
                return Ok(true);
            }
        }
    }
    marks.insert(node, Mark::Done);
    Ok(false)
}

enum Mark {
    Visiting,
    Done,
}

/// 새 plan 생성. 이미 같은 id 가 있으면 `AlreadyExists`.
pub fn plan_create(
    store: &mut MemoryStore,
    owner: &str,
    workspace_id: u32,
    plan_id: &str,
    title: &str,
    steps: Vec<PlanStep>,
) -> Result<u64> {
    let now = now_ms_local();
    let plan = Plan {
        id: plan_id.to_string(),
        title: title.to_string(),
        created_at: now,
        created_by: owner.to_string(),
        updated_at: now,
        steps,
    };
    validate_plan(&plan)?;
    let scope = Scope::Workspace(workspace_id);
    let key = plan_key(plan_id);
    if store.get(&scope, &key)?.is_some() {
        return Err(MemoryError::AlreadyExists {
            scope: scope.as_token(),
            key,
        });
    }
    let value = MemoryValue::Json(serde_json::to_value(&plan).map_err(|e| {
        MemoryError::InvalidContentType(format!("serialize plan: {e}"))
    })?);
    store.put(owner, &scope, &key, &value, &PutOpts::default())
}

/// plan 조회.
pub fn plan_get(
    store: &MemoryStore,
    workspace_id: u32,
    plan_id: &str,
) -> Result<Option<Plan>> {
    validate_plan_id(plan_id)?;
    let Some(entry) = store.get(&Scope::Workspace(workspace_id), &plan_key(plan_id))? else {
        return Ok(None);
    };
    plan_from_entry(&entry).map(Some)
}

/// 한 entry → Plan 디시리얼. JSON 이 아니거나 형식 깨졌으면 에러.
fn plan_from_entry(entry: &MemoryEntry) -> Result<Plan> {
    let MemoryValue::Json(v) = &entry.value else {
        return Err(MemoryError::InvalidContentType(format!(
            "plan entry is not application/json: {}",
            entry.key
        )));
    };
    serde_json::from_value::<Plan>(v.clone())
        .map_err(|e| MemoryError::InvalidContentType(format!("invalid plan json: {e}")))
}

/// 워크스페이스의 plan id 목록 (정렬).
pub fn plan_list(store: &MemoryStore, workspace_id: u32) -> Result<Vec<String>> {
    let opts = crate::ListOpts {
        prefix: Some(PLAN_KEY_PREFIX.to_string()),
        ..Default::default()
    };
    let entries = store.list(&Scope::Workspace(workspace_id), &opts)?;
    Ok(entries
        .into_iter()
        .filter_map(|e| {
            e.key
                .strip_prefix(PLAN_KEY_PREFIX)
                .map(|s| s.to_string())
        })
        .collect())
}

/// plan 삭제.
pub fn plan_delete(
    store: &mut MemoryStore,
    owner: &str,
    workspace_id: u32,
    plan_id: &str,
) -> Result<()> {
    validate_plan_id(plan_id)?;
    store.delete(
        owner,
        &Scope::Workspace(workspace_id),
        &plan_key(plan_id),
        None,
    )
}

/// step 추가. `position` = None 이면 끝에 append, Some(i) 이면 인덱스 i 에 insert.
/// step id 중복/총 step 수 초과 시 에러.
pub fn plan_add_step(
    store: &mut MemoryStore,
    owner: &str,
    workspace_id: u32,
    plan_id: &str,
    step: PlanStep,
    position: Option<usize>,
    cas: Option<u64>,
) -> Result<u64> {
    update_plan(store, owner, workspace_id, plan_id, cas, |plan| {
        let pos = position.unwrap_or(plan.steps.len()).min(plan.steps.len());
        plan.steps.insert(pos, step);
        Ok(())
    })
}

/// step 제거. 다른 step 이 `depends_on` 으로 참조 중이면 에러.
pub fn plan_remove_step(
    store: &mut MemoryStore,
    owner: &str,
    workspace_id: u32,
    plan_id: &str,
    step_id: &str,
    cas: Option<u64>,
) -> Result<u64> {
    update_plan(store, owner, workspace_id, plan_id, cas, |plan| {
        // 다른 step 이 의존하지 않는지.
        for s in &plan.steps {
            if s.id != step_id && s.depends_on.iter().any(|d| d == step_id) {
                return Err(MemoryError::InvalidKey(format!(
                    "step '{step_id}' is referenced by '{0}'",
                    s.id
                )));
            }
        }
        let before = plan.steps.len();
        plan.steps.retain(|s| s.id != step_id);
        if plan.steps.len() == before {
            return Err(MemoryError::NotFound {
                scope: format!("plan:{}", plan.id),
                key: step_id.to_string(),
            });
        }
        Ok(())
    })
}

/// step state 갱신 (+ notes 옵션). notes = Some(None) 은 해제, Some(Some(s)) 는 set.
pub fn plan_update_step(
    store: &mut MemoryStore,
    owner: &str,
    workspace_id: u32,
    plan_id: &str,
    step_id: &str,
    new_state: Option<PlanStepState>,
    notes: Option<Option<String>>,
    cas: Option<u64>,
) -> Result<u64> {
    update_plan(store, owner, workspace_id, plan_id, cas, |plan| {
        let step = plan
            .steps
            .iter_mut()
            .find(|s| s.id == step_id)
            .ok_or_else(|| MemoryError::NotFound {
                scope: format!("plan:{}", plan.id),
                key: step_id.to_string(),
            })?;
        if let Some(st) = new_state {
            step.state = st;
        }
        if let Some(n) = notes {
            step.notes = n;
        }
        Ok(())
    })
}

/// 공통: plan 을 읽어 mutate 한 뒤 다시 put. CAS 는 entry version 에 적용.
fn update_plan<F>(
    store: &mut MemoryStore,
    owner: &str,
    workspace_id: u32,
    plan_id: &str,
    cas: Option<u64>,
    mutate: F,
) -> Result<u64>
where
    F: FnOnce(&mut Plan) -> Result<()>,
{
    validate_plan_id(plan_id)?;
    let scope = Scope::Workspace(workspace_id);
    let key = plan_key(plan_id);
    let entry = store
        .get(&scope, &key)?
        .ok_or_else(|| MemoryError::NotFound {
            scope: scope.as_token(),
            key: key.clone(),
        })?;
    let mut plan = plan_from_entry(&entry)?;
    mutate(&mut plan)?;
    plan.updated_at = now_ms_local();
    validate_plan(&plan)?;
    let value = MemoryValue::Json(serde_json::to_value(&plan).map_err(|e| {
        MemoryError::InvalidContentType(format!("serialize plan: {e}"))
    })?);
    let opts = PutOpts {
        expires_at: None,
        cas,
    };
    store.put(owner, &scope, &key, &value, &opts)
}

fn now_ms_local() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::HOST_OWNER;

    fn open() -> MemoryStore {
        MemoryStore::open_in_memory().expect("open in memory")
    }

    fn step(id: &str, title: &str) -> PlanStep {
        PlanStep {
            id: id.into(),
            title: title.into(),
            state: PlanStepState::Pending,
            depends_on: Vec::new(),
            notes: None,
        }
    }

    #[test]
    fn create_then_get_roundtrip() {
        let mut s = open();
        let steps = vec![step("a", "first"), step("b", "second")];
        plan_create(&mut s, HOST_OWNER, 1, "p1", "demo", steps.clone()).unwrap();
        let plan = plan_get(&s, 1, "p1").unwrap().expect("plan");
        assert_eq!(plan.id, "p1");
        assert_eq!(plan.title, "demo");
        assert_eq!(plan.steps, steps);
        assert_eq!(plan.created_by, HOST_OWNER);
    }

    #[test]
    fn create_duplicate_fails() {
        let mut s = open();
        plan_create(&mut s, HOST_OWNER, 1, "p", "t", vec![]).unwrap();
        let err = plan_create(&mut s, HOST_OWNER, 1, "p", "t", vec![]).unwrap_err();
        assert!(matches!(err, MemoryError::AlreadyExists { .. }), "{err:?}");
    }

    #[test]
    fn duplicate_step_id_rejected() {
        let mut s = open();
        let steps = vec![step("a", "1"), step("a", "2")];
        let err = plan_create(&mut s, HOST_OWNER, 1, "p", "t", steps).unwrap_err();
        assert!(matches!(err, MemoryError::InvalidKey(_)), "{err:?}");
    }

    #[test]
    fn step_cap_enforced() {
        let mut s = open();
        let too_many: Vec<PlanStep> = (0..(PLAN_STEP_MAX + 1))
            .map(|i| step(&format!("s{i}"), "x"))
            .collect();
        let err = plan_create(&mut s, HOST_OWNER, 1, "p", "t", too_many).unwrap_err();
        assert!(matches!(err, MemoryError::InvalidKey(_)), "{err:?}");
    }

    #[test]
    fn depends_on_unknown_rejected() {
        let mut s = open();
        let mut a = step("a", "1");
        a.depends_on = vec!["ghost".into()];
        let err = plan_create(&mut s, HOST_OWNER, 1, "p", "t", vec![a]).unwrap_err();
        assert!(matches!(err, MemoryError::InvalidKey(_)), "{err:?}");
    }

    #[test]
    fn cycle_rejected() {
        let mut s = open();
        let mut a = step("a", "1");
        let mut b = step("b", "2");
        a.depends_on = vec!["b".into()];
        b.depends_on = vec!["a".into()];
        let err = plan_create(&mut s, HOST_OWNER, 1, "p", "t", vec![a, b]).unwrap_err();
        assert!(matches!(err, MemoryError::InvalidKey(_)), "{err:?}");
    }

    #[test]
    fn self_dep_rejected() {
        let mut s = open();
        let mut a = step("a", "1");
        a.depends_on = vec!["a".into()];
        let err = plan_create(&mut s, HOST_OWNER, 1, "p", "t", vec![a]).unwrap_err();
        assert!(matches!(err, MemoryError::InvalidKey(_)), "{err:?}");
    }

    #[test]
    fn update_step_state() {
        let mut s = open();
        plan_create(&mut s, HOST_OWNER, 1, "p", "t", vec![step("a", "x")]).unwrap();
        plan_update_step(
            &mut s,
            HOST_OWNER,
            1,
            "p",
            "a",
            Some(PlanStepState::InProgress),
            None,
            None,
        )
        .unwrap();
        let plan = plan_get(&s, 1, "p").unwrap().unwrap();
        assert_eq!(plan.steps[0].state, PlanStepState::InProgress);
    }

    #[test]
    fn update_step_notes() {
        let mut s = open();
        plan_create(&mut s, HOST_OWNER, 1, "p", "t", vec![step("a", "x")]).unwrap();
        plan_update_step(
            &mut s,
            HOST_OWNER,
            1,
            "p",
            "a",
            None,
            Some(Some("hello".into())),
            None,
        )
        .unwrap();
        let plan = plan_get(&s, 1, "p").unwrap().unwrap();
        assert_eq!(plan.steps[0].notes.as_deref(), Some("hello"));

        plan_update_step(&mut s, HOST_OWNER, 1, "p", "a", None, Some(None), None).unwrap();
        let plan = plan_get(&s, 1, "p").unwrap().unwrap();
        assert!(plan.steps[0].notes.is_none());
    }

    #[test]
    fn add_step_at_position() {
        let mut s = open();
        plan_create(
            &mut s,
            HOST_OWNER,
            1,
            "p",
            "t",
            vec![step("a", "1"), step("c", "3")],
        )
        .unwrap();
        plan_add_step(&mut s, HOST_OWNER, 1, "p", step("b", "2"), Some(1), None).unwrap();
        let plan = plan_get(&s, 1, "p").unwrap().unwrap();
        assert_eq!(
            plan.steps.iter().map(|s| s.id.as_str()).collect::<Vec<_>>(),
            vec!["a", "b", "c"]
        );
    }

    #[test]
    fn add_duplicate_step_rejected() {
        let mut s = open();
        plan_create(&mut s, HOST_OWNER, 1, "p", "t", vec![step("a", "1")]).unwrap();
        let err = plan_add_step(&mut s, HOST_OWNER, 1, "p", step("a", "dup"), None, None)
            .unwrap_err();
        assert!(matches!(err, MemoryError::InvalidKey(_)), "{err:?}");
    }

    #[test]
    fn remove_step() {
        let mut s = open();
        plan_create(
            &mut s,
            HOST_OWNER,
            1,
            "p",
            "t",
            vec![step("a", "1"), step("b", "2")],
        )
        .unwrap();
        plan_remove_step(&mut s, HOST_OWNER, 1, "p", "a", None).unwrap();
        let plan = plan_get(&s, 1, "p").unwrap().unwrap();
        assert_eq!(plan.steps.len(), 1);
        assert_eq!(plan.steps[0].id, "b");
    }

    #[test]
    fn remove_referenced_step_rejected() {
        let mut s = open();
        let mut b = step("b", "2");
        b.depends_on = vec!["a".into()];
        plan_create(&mut s, HOST_OWNER, 1, "p", "t", vec![step("a", "1"), b]).unwrap();
        let err = plan_remove_step(&mut s, HOST_OWNER, 1, "p", "a", None).unwrap_err();
        assert!(matches!(err, MemoryError::InvalidKey(_)), "{err:?}");
    }

    #[test]
    fn list_isolated_by_workspace() {
        let mut s = open();
        plan_create(&mut s, HOST_OWNER, 1, "alpha", "t", vec![]).unwrap();
        plan_create(&mut s, HOST_OWNER, 2, "beta", "t", vec![]).unwrap();
        assert_eq!(plan_list(&s, 1).unwrap(), vec!["alpha".to_string()]);
        assert_eq!(plan_list(&s, 2).unwrap(), vec!["beta".to_string()]);
    }

    #[test]
    fn delete_removes_plan() {
        let mut s = open();
        plan_create(&mut s, HOST_OWNER, 1, "p", "t", vec![]).unwrap();
        plan_delete(&mut s, HOST_OWNER, 1, "p").unwrap();
        assert!(plan_get(&s, 1, "p").unwrap().is_none());
    }

    #[test]
    fn cas_conflict_on_update() {
        let mut s = open();
        plan_create(&mut s, HOST_OWNER, 1, "p", "t", vec![step("a", "1")]).unwrap();
        let err = plan_update_step(
            &mut s,
            HOST_OWNER,
            1,
            "p",
            "a",
            Some(PlanStepState::Completed),
            None,
            Some(99),
        )
        .unwrap_err();
        assert!(matches!(err, MemoryError::CasConflict { .. }), "{err:?}");
    }
}
