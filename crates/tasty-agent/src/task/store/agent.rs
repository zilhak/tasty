//! agent task 회차의 세션 연결 기록.

use super::super::agent::AgentLink;
use super::super::{TaskCommand, TaskId, TaskState};
use super::{TaskStore, WorkspaceId};
use crate::{AgentError, Result};

impl TaskStore<'_> {
    /// Running 인 agent task 의 `attempt_id` 회차에 세션 연결을 기록한다. 다른 회차이거나 이미
    /// 끝났으면 기록하지 않고 `false` 를 돌려준다. 값이 같으면 쓰지 않는다.
    pub fn set_agent_link(
        &mut self,
        workspace_id: WorkspaceId,
        id: &TaskId,
        attempt_id: &str,
        link: AgentLink,
    ) -> Result<bool> {
        let mut task = self
            .get(workspace_id, id)?
            .ok_or_else(|| AgentError::TaskNotFound(id.clone()))?;
        if !matches!(task.command, TaskCommand::Agent { .. })
            || !matches!(task.state, TaskState::Running)
        {
            return Ok(false);
        }
        let Some(attempt) = task.attempt.as_mut().filter(|a| a.id == attempt_id) else {
            return Ok(false);
        };
        if attempt.agent.as_ref() == Some(&link) {
            return Ok(true);
        }
        attempt.agent = Some(link);
        self.put(&task)?;
        Ok(true)
    }
}
