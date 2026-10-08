//! report 토큰 발급, custom 기록 저장과 조회. 규칙은 [`super::super::report`] 에 있다.

use tasty_memory::{MemoryValue, PutOpts, Scope};

use super::super::report::{
    AppendOutcome, ReportAddress, ReportBlock, ReportLimits, ReportToken, max_attempt, report_key,
};
use super::super::{Task, TaskId, TaskState};
use super::{TaskStore, WorkspaceId};
use crate::{AgentError, Result};

impl TaskStore<'_> {
    /// dispatch 가 회차 `attempt` 의 토큰을 task 에 남긴다. v2 task 만 report 를 둔다.
    pub fn issue_report_token(
        &mut self,
        workspace_id: WorkspaceId,
        id: &TaskId,
        token: ReportToken,
    ) -> Result<Task> {
        let mut task = self
            .get(workspace_id, id)?
            .ok_or_else(|| AgentError::TaskNotFound(id.clone()))?;
        if !task.is_typed() {
            return Err(AgentError::InvalidArgument(format!(
                "task {id} is not a typed task; it has no report"
            )));
        }
        task.report_token = Some(token);
        self.put(&task)?;
        Ok(task)
    }

    /// custom 기록 한 줄을 더한다. 회차가 열려 있고 토큰이 같아야 한다. 상한에 걸리면
    /// 잘라 저장하거나 저장하지 않고 세기만 하며, 그때도 실패가 아니다.
    pub fn append_report(
        &mut self,
        addr: &ReportAddress,
        text: &str,
        limits: ReportLimits,
        now_ms: u64,
    ) -> Result<AppendOutcome> {
        let task = self
            .get(addr.workspace_id, &addr.task_id)?
            .ok_or_else(|| AgentError::TaskNotFound(addr.task_id.clone()))?;
        let open = task
            .report_token
            .as_ref()
            .is_some_and(|t| t.token == addr.token && t.attempt == addr.attempt);
        if !open {
            return Err(AgentError::InvalidArgument(format!(
                "report: the token does not open a block of task {} attempt {}",
                addr.task_id, addr.attempt
            )));
        }
        if !matches!(task.state, TaskState::Ready | TaskState::Running) {
            return Err(AgentError::InvalidArgument(format!(
                "report: task {} attempt {} is closed ({})",
                addr.task_id,
                addr.attempt,
                task.state.name()
            )));
        }
        let mut block = self
            .report_block(addr.workspace_id, &addr.task_id, addr.attempt)?
            .unwrap_or_else(|| ReportBlock::new(addr.attempt));
        let outcome = block.append(addr.source, text, limits, now_ms);
        self.put_report_block(addr.workspace_id, &addr.task_id, &block)?;
        Ok(outcome)
    }

    /// 회차 하나의 custom 블록. 기록이 없으면 `None`.
    pub fn report_block(
        &self,
        workspace_id: WorkspaceId,
        id: &TaskId,
        attempt: u32,
    ) -> Result<Option<ReportBlock>> {
        let key = report_key(id, attempt)?;
        let Some(entry) = self.mem.get(&Scope::Workspace(workspace_id), &key)? else {
            return Ok(None);
        };
        let MemoryValue::Json(v) = entry.value else {
            return Err(AgentError::InvalidArgument(format!(
                "report block {key} is not json"
            )));
        };
        Ok(Some(serde_json::from_value(v)?))
    }

    /// task 의 custom 블록 전부(회차 오름차순).
    pub fn report_blocks(&self, task: &Task) -> Result<Vec<ReportBlock>> {
        let mut out = Vec::new();
        for n in 1..=max_attempt(task) {
            if let Some(b) = self.report_block(task.workspace_id, &task.id, n)? {
                out.push(b);
            }
        }
        Ok(out)
    }

    fn put_report_block(
        &mut self,
        workspace_id: WorkspaceId,
        id: &TaskId,
        block: &ReportBlock,
    ) -> Result<()> {
        self.mem.put(
            &self.owner,
            &Scope::Workspace(workspace_id),
            &report_key(id, block.attempt)?,
            &MemoryValue::Json(serde_json::to_value(block)?),
            &PutOpts::default(),
        )?;
        Ok(())
    }

    /// 다음 회차가 시작되기 전(retry) 끝난 회차의 상태를 그 블록에 남긴다. 다음 회차가 열리면
    /// task 레코드는 이전 회차의 상태를 잃는다. 실행한 적 없는(skipped) 회차는 남기지 않는다.
    pub(super) fn settle_report_block(&mut self, task: &Task) -> Result<()> {
        let Some(n) = super::super::report::current_attempt(task) else {
            return Ok(());
        };
        if matches!(task.state, TaskState::Skipped) {
            return Ok(());
        }
        let mut block = self
            .report_block(task.workspace_id, &task.id, n)?
            .unwrap_or_else(|| ReportBlock::new(n));
        // 새 회차 없이 두 번째 retry 를 하면(retry 뒤 실행 전에 cancel 등) 이 회차는 이미 남겼다.
        if block.settled.is_some() {
            return Ok(());
        }
        block.settled = Some(task.state.clone());
        self.put_report_block(task.workspace_id, &task.id, &block)
    }

    /// task 를 지울 때 그 task 의 블록도 지운다.
    pub(super) fn delete_report_blocks(&mut self, task: &Task) -> Result<()> {
        let scope = Scope::Workspace(task.workspace_id);
        for n in 1..=max_attempt(task) {
            let key = report_key(&task.id, n)?;
            if self.mem.get(&scope, &key)?.is_some() {
                self.mem.delete(&self.owner, &scope, &key, None)?;
            }
        }
        Ok(())
    }
}
