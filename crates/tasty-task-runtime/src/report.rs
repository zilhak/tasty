//! DAG report 조회와 custom 기록. 저장 규칙은 [`tasty_agent::task::report`] 에 있다.

use serde_json::Value;
use tasty_agent::task::attempt::attempt_id;
use tasty_agent::task::report::{AppendOutcome, ReportAddress, ReportLimits};

use crate::agent_task::Submitter;
use tasty_agent::AgentError;

use crate::{TaskScope, TaskService};

impl TaskService {
    /// 설정이 바뀌면 App 이 새 상한을 넣는다. 검증은 설정 쪽이 한다.
    pub fn set_report_limits(&self, limits: ReportLimits) {
        self.report_limits().set(limits);
    }

    /// custom 기록 한 줄을 지금 상한으로 저장한다. 세션 토큰으로 부른 agent 세션
    /// ([`Submitter::Session`])은 `agent.task_submit_result` 처럼 자기 세션에 지시를 보낸 회차에만
    /// 쓴다. 사용자 CLI·플러그인은 주소의 토큰만 본다.
    pub fn report_append(
        &self,
        scope: &TaskScope,
        addr: &ReportAddress,
        text: &str,
        writer: Submitter,
    ) -> Result<AppendOutcome, AgentError> {
        let (scope, addr, text) = (scope.clone(), addr.clone(), text.to_string());
        self.transact(move |tx| tx.report_append(&scope, &addr, &text, writer))
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
        let (scope, workspace_ids, dag_id) =
            (scope.clone(), workspace_ids.to_vec(), dag_id.to_string());
        let task = task.map(str::to_string);
        self.transact(move |tx| {
            tx.dag_report(
                &scope,
                &workspace_ids,
                &dag_id,
                task.as_deref(),
                attempt,
                include_raw,
            )
        })
    }
}

/// 세션 토큰으로 부른 쓰기가 이 회차에 쓸 수 있는가. 사용자 CLI·플러그인은 주소의 토큰만 본다.
pub(crate) fn bound_here(svc: &TaskService, addr: &ReportAddress, writer: Submitter) -> bool {
    match writer {
        Submitter::Trusted => true,
        Submitter::Session(surface) => svc
            .agent_turns()
            .find(addr.workspace_id, &addr.task_id)
            .is_some_and(|(s, b)| {
                s == surface && b.attempt == attempt_id(&addr.task_id, addr.attempt)
            }),
    }
}
