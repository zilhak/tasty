//! DAG report 조회와 custom 기록. 저장 규칙은 [`tasty_agent::task::report`] 에 있다.

use serde_json::{Value, json};
use tasty_agent::task::report::{AppendOutcome, ReportAddress, ReportLimits, project_task};
use tasty_agent::{AgentError, TaskStore};
use tasty_memory::HOST_OWNER;

use crate::{TaskScope, TaskService};

impl TaskService {
    /// 설정이 바뀌면 App 이 새 상한을 넣는다. 검증은 설정 쪽이 한다.
    pub fn set_report_limits(&self, limits: ReportLimits) {
        self.report_limits().set(limits);
    }

    /// custom 기록 한 줄을 지금 상한으로 저장한다.
    pub fn report_append(
        &self,
        scope: &TaskScope,
        addr: &ReportAddress,
        text: &str,
    ) -> Result<AppendOutcome, AgentError> {
        crate::runner_host::report_append(
            self.memory(),
            scope.agent_seq(),
            self.report_limits().get(),
            addr,
            text,
        )
    }

    /// DAG 하나의 report. `task` 를 주면 그 task 만, `attempt` 는 그 task 의 회차 하나만 싣는다.
    /// DAG 가 없으면 `None`, 주어진 task 가 DAG 에 없으면 오류다.
    pub fn dag_report(
        &self,
        scope: &TaskScope,
        workspace_ids: &[u32],
        dag_id: &str,
        task: Option<&str>,
        attempt: Option<u32>,
        include_raw: bool,
    ) -> Result<Option<Value>, AgentError> {
        let Some((dag, tasks)) = self.dag_get(scope, workspace_ids, dag_id)? else {
            return Ok(None);
        };
        let selected: Vec<_> = match task {
            None => dag
                .task_ids
                .iter()
                .filter_map(|id| tasks.iter().find(|t| &t.id == id))
                .collect(),
            Some(id) => {
                let t = tasks.iter().find(|t| t.id == id).ok_or_else(|| {
                    AgentError::InvalidArgument(format!("task {id} is not in dag {dag_id}"))
                })?;
                vec![t]
            }
        };
        let seq = scope.agent_seq().clone();
        let entries = self.with_memory(|mem| {
            let store = TaskStore::new(mem, HOST_OWNER, seq.as_ref());
            selected
                .iter()
                .map(|t| {
                    Ok(project_task(
                        t,
                        &store.report_blocks(t)?,
                        attempt,
                        include_raw,
                    ))
                })
                .collect::<Result<Vec<_>, AgentError>>()
        })?;
        Ok(Some(json!({
            "dag": dag.id,
            "workspace_id": dag.workspace_id,
            "name": dag.name,
            "tasks": entries,
        })))
    }
}
