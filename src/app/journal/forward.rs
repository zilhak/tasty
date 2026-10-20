//! A committed remote enqueue keeps its exact connection lease until the result is durable.
use super::*;
use crate::runtime::journal_product::EffectLease;
use tasty_core::{OperationId, OperationOutcome};

pub(crate) struct Draft {
    pub engine: EngineId,
    pub stream: String,
    pub response: crate::ipc::protocol::JsonRpcResponse,
    pub target: crate::app::attach_client::RemoteTarget,
    pub local_anchor: u32,
    pub remote_anchor: u32,
    pub op_id: u64,
    pub payload: Vec<u8>,
    pub focus: Option<tasty_remote::client_session::PendingOpFocus>,
    pub silent_failure: bool,
    /// 원격 결과를 기다리는 intent 요청의 run. 보내지 못하거나 결과 없이 사라지면 여기서 끝난다.
    #[cfg(feature = "gui")]
    pub completion: Option<crate::request_runs::RunGuard>,
}

/// 원격으로 보내기 전에 forward가 결과 없이 사라졌을 때.
#[cfg(feature = "gui")]
pub(crate) const FORWARD_ENDED: &str = "the remote change ended before it was sent";
impl Draft {
    pub fn weight(&self) -> usize {
        serde_json::to_vec(&self.response).map_or(0, |bytes| bytes.len())
            + self.payload.capacity()
            + self.stream.len()
            + self.focus.as_ref().map_or(0, |focus| match focus {
                tasty_remote::client_session::PendingOpFocus::Close { candidates } => {
                    candidates.capacity() * 4
                }
                _ => 0,
            })
            + std::mem::size_of::<Self>()
    }
}
pub(super) struct Forward {
    pub engine: EngineId,
    draft: Option<Draft>,
    queued: Option<Work>,
    stage: Phase,
    /// 보내기 전까지 요청의 run을 여기에 둔다. journal이 멈추면 보내지 않은 forward의 run을 끝낸다.
    #[cfg(feature = "gui")]
    completion: Option<crate::request_runs::RunGuard>,
}
enum Phase {
    Claim,
    Ready {
        lease: EffectLease,
        payload: Vec<u8>,
    },
    Sending {
        lease: EffectLease,
    },
    Finish,
}
impl JournalApplication {
    pub(super) fn start_forward(
        &mut self,
        #[cfg_attr(not(feature = "gui"), expect(unused_mut))] mut draft: Draft,
        operation: OperationId,
    ) -> Result<(), String> {
        let ticket = self.next_ticket;
        self.next_ticket = self
            .next_ticket
            .checked_add(1)
            .ok_or("journal ticket exhausted")?;
        self.forwards.insert(
            ticket,
            Forward {
                engine: draft.engine,
                queued: Some(Work::ClaimForward {
                    stream: draft.stream.clone(),
                    operation,
                }),
                #[cfg(feature = "gui")]
                completion: draft.completion.take(),
                draft: Some(draft),
                stage: Phase::Claim,
            },
        );
        (self.wake)();
        Ok(())
    }
    pub(super) fn submit_forwards(&mut self) -> Result<(), String> {
        for (ticket, forward) in &mut self.forwards {
            let Some(work) = forward.queued.take() else {
                continue;
            };
            match self.worker.submit_owned(Request {
                ticket: *ticket,
                work,
            }) {
                Ok(()) => {}
                Err((crate::runtime::journal_product::SubmitError::Busy, request)) => {
                    forward.queued = Some(request.work)
                }
                Err((error, _)) => {
                    return Err(format!("committed remote submission failed: {error:?}"));
                }
            }
        }
        Ok(())
    }
    pub(super) fn answer_forward(
        &mut self,
        ticket: u64,
        result: &Result<ResultValue, String>,
    ) -> Result<bool, String> {
        let Some(forward) = self.forwards.get_mut(&ticket) else {
            return Ok(false);
        };
        match (&forward.stage, result.as_ref().map_err(Clone::clone)?) {
            (Phase::Claim, ResultValue::ForwardClaimed { lease, payload }) => {
                forward.stage = Phase::Ready {
                    lease: lease.clone(),
                    payload: payload.clone(),
                }
            }
            (Phase::Finish, ResultValue::Executed(_) | ResultValue::Stored(_)) => {
                self.forwards.remove(&ticket);
                self.refresh_in_progress_commands();
            }
            _ => return Err("remote effect result is outside its admitted phase".into()),
        }
        (self.wake)();
        Ok(true)
    }
    pub(crate) fn take_ready_forwards(&mut self) -> Vec<(u64, Draft, Vec<u8>)> {
        let mut out = Vec::new();
        // 멈춘 journal은 송신 결과를 기록하지 못하고, 보내지 않은 run은 이미 실패로 끝났다.
        // 같은 poll에서 Ready가 된 forward도 보내지 않는다.
        if self.is_halted() {
            return out;
        }
        for (ticket, forward) in &mut self.forwards {
            if let Phase::Ready { lease, payload } = &mut forward.stage {
                let lease = lease.clone();
                let payload = std::mem::take(payload);
                if let Some(mut draft) = forward.draft.take() {
                    #[cfg(feature = "gui")]
                    {
                        draft.completion = forward.completion.take();
                    }
                    out.push((*ticket, draft, payload));
                    forward.stage = Phase::Sending { lease };
                }
            }
        }
        out
    }
    pub(crate) fn forward_dispatched(&mut self, ticket: u64, outcome: OperationOutcome) {
        if let Some(forward) = self.forwards.get_mut(&ticket)
            && let Phase::Sending { lease } = &forward.stage
        {
            forward.queued = Some(Work::ForwardFinished {
                lease: lease.clone(),
                outcome,
            });
            forward.stage = Phase::Finish;
            (self.wake)();
        }
    }
}

impl JournalApplication {
    /// journal이 멈추면 forward는 더 진행하지 않는다. 아직 보내지 않은 원격 변경의 요청에
    /// 멈춘 원인을 최종 결과로 남긴다. 이미 보낸 변경의 run은 원격 결과를 기다리는 쪽이 가진다.
    #[cfg(feature = "gui")]
    pub(super) fn fail_unsent_forwards(&mut self, reason: &str) {
        fail_unsent(&mut self.forwards, reason);
    }
}

#[cfg(all(test, unix, feature = "gui"))]
impl Forward {
    /// 원격 큐 확보를 기다리는 forward. 실제 확보 작업을 worker에 낸다.
    pub(super) fn claiming(
        engine: EngineId,
        stream: String,
        operation: OperationId,
        completion: crate::request_runs::RunGuard,
    ) -> Self {
        Self {
            engine,
            draft: None,
            queued: Some(Work::ClaimForward { stream, operation }),
            stage: Phase::Claim,
            completion: Some(completion),
        }
    }

    /// 원격 큐를 확보해 보낼 차례가 된 forward.
    pub(super) fn ready(mut draft: Draft, lease: EffectLease) -> Self {
        Self {
            engine: draft.engine,
            completion: draft.completion.take(),
            stage: Phase::Ready {
                lease,
                payload: draft.payload.clone(),
            },
            draft: Some(draft),
            queued: None,
        }
    }
}

#[cfg(feature = "gui")]
fn fail_unsent(forwards: &mut std::collections::BTreeMap<u64, Forward>, reason: &str) {
    for forward in forwards.values_mut() {
        if let Some(run) = forward.completion.take() {
            crate::request_runs::finish(
                run.take(),
                crate::request_runs::RunState::Failed,
                format!("the remote change was not sent because the journal stopped: {reason}"),
                serde_json::Value::Null,
            );
        }
    }
}

#[cfg(all(test, feature = "gui"))]
mod tests {
    use super::*;
    use crate::request_runs::{RunGuard, begin, lookup};

    fn forward(engine: EngineId, stage: Phase, completion: Option<RunGuard>) -> Forward {
        Forward {
            engine,
            draft: None,
            queued: None,
            stage,
            completion,
        }
    }

    #[test]
    fn a_halt_fails_only_the_runs_of_changes_not_yet_sent() {
        let (_, engine) = crate::state::tests::test_state();
        let claiming = begin("markdown.navigate", None, serde_json::Value::Null);
        let sent = begin("markdown.navigate", None, serde_json::Value::Null);
        let mut forwards = std::collections::BTreeMap::from([
            (
                1,
                forward(
                    engine.id,
                    Phase::Claim,
                    Some(RunGuard::new(claiming, FORWARD_ENDED)),
                ),
            ),
            // 보낸 forward의 run은 원격 결과 대기로 넘어가 여기에 없다.
            (2, forward(engine.id, Phase::Finish, None)),
        ]);
        let waiting = RunGuard::new(sent, FORWARD_ENDED);
        fail_unsent(&mut forwards, "worker stopped");
        let ended = lookup(claiming, None).unwrap();
        assert_eq!(ended["state"], "failed");
        assert_eq!(
            ended["result"],
            "the remote change was not sent because the journal stopped: worker stopped"
        );
        assert_eq!(lookup(sent, None).unwrap()["state"], "running");
        assert_eq!(forwards.len(), 2, "a halt keeps the forwards themselves");
        waiting.take();
    }
}

impl crate::app::App {
    pub(crate) fn execute_journal_forwards(&mut self) {
        for (ticket, mut draft, payload) in self.journal.take_ready_forwards() {
            let completion = draft.completion.take();
            let current = self.remote_target_is_current(&draft.target)
                && self
                    .remote
                    .sessions
                    .iter()
                    .find(|session| session.state.local_workspace == draft.target.workspace)
                    .is_some_and(|session| {
                        session.state.remote_to_local.get(&draft.remote_anchor)
                            == Some(&draft.local_anchor)
                    });
            let outcome = if !current {
                OperationOutcome::Cancelled {
                    reason: "remote connection or original mapping retired before enqueue".into(),
                }
            } else if payload != draft.payload {
                OperationOutcome::Failed {
                    reason: "committed remote instruction differs from its exact connection lease"
                        .into(),
                }
            } else {
                match draft
                    .target
                    .sender
                    .send(tasty_remote::client_session::OutFrame {
                        tag: tasty_ipc::stream::StreamTag::Control,
                        payload,
                    }) {
                    Ok(()) => {
                        if let Some(session) = self.remote.sessions.iter_mut().find(|session| {
                            session.state.local_workspace == draft.target.workspace
                                && session
                                    .transport
                                    .frame_tx
                                    .epoch()
                                    .same(&draft.target.sender.epoch())
                        }) {
                            if let Some(focus) = draft.focus {
                                session.state.pending_op_focus.insert(draft.op_id, focus);
                            }
                            session
                                .state
                                .agent_requests
                                .note_structural(draft.silent_failure, draft.op_id);
                        }
                        if let Some(run) = completion.clone() {
                            crate::app::attach_client::op_runs::watch(
                                draft.target.sender.epoch(),
                                draft.op_id,
                                run,
                            );
                        }
                        OperationOutcome::Succeeded
                    }
                    Err(_) => OperationOutcome::Failed {
                        reason: "remote connection rejected the structural enqueue".into(),
                    },
                }
            };
            if let Some(run) =
                completion.filter(|_| !matches!(outcome, OperationOutcome::Succeeded))
            {
                let (state, reason) = match &outcome {
                    OperationOutcome::Cancelled { reason } => {
                        (crate::request_runs::RunState::Cancelled, reason.clone())
                    }
                    OperationOutcome::Failed { reason } => {
                        (crate::request_runs::RunState::Failed, reason.clone())
                    }
                    other => (crate::request_runs::RunState::Failed, format!("{other:?}")),
                };
                crate::request_runs::finish(run.take(), state, reason, serde_json::Value::Null);
            }
            self.journal.forward_dispatched(ticket, outcome);
        }
    }
}
