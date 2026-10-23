//! A single outstanding write separates admission order from committed registry state.
use super::*;
use mutation::Prepared;

pub(super) struct Pending {
    pub(super) operation: Operation,
    pub(super) reply: Arc<Reply>,
}
pub(super) struct InFlight {
    raw: command::Ticket<Result<(), String>>,
    prepared: Prepared,
    reply: Arc<Reply>,
    registered: Option<String>,
    notify_failure: bool,
}
struct Delivery {
    reply: Arc<Reply>,
    result: Result<Completed, Failure>,
    warning: Option<String>,
}
enum Submission {
    Ready(Completed),
    Writing(command::Ticket<Result<(), String>>, Box<Prepared>),
}

impl Registry {
    pub(crate) fn poll(&self) {
        loop {
            let mut deliveries = Vec::new();
            let callback = {
                let mut s = self.lock();
                if s.publishing {
                    return;
                }
                while let Ok(settled) = collect_ready(&mut s) {
                    if let Some(settled) = settled {
                        s.settled = s.settled.saturating_add(1);
                        deliveries.push(settled);
                    }
                    let Some(pending) = s.pending.pop_front() else {
                        break;
                    };
                    if let Some(delivery) = self.prepare_next(&mut s, pending) {
                        s.settled = s.settled.saturating_add(1);
                        deliveries.push(delivery);
                    }
                }
                let callback = if s.in_flight.is_none() && s.pending.is_empty() {
                    let callback = s.after_boot.take();
                    if callback.is_some() {
                        s.booting = false;
                    }
                    callback
                } else {
                    None
                };
                if deliveries.is_empty() && callback.is_none() {
                    return;
                }
                // Concurrent poll callers skip rather than wait; only this owner publishes receipts.
                s.publishing = true;
                callback
            };
            // Publish warning and receipt notifications outside the registry guard, in that order.
            for delivery in &deliveries {
                if let Some(warning) = &delivery.warning {
                    tracing::warn!(error=%warning,"webhook persistence failed; retained existing consumption policy");
                    crate::hook_runtime::notice::webhook_save_failed(warning);
                }
            }
            for delivery in deliveries {
                delivery.reply.finish(delivery.result);
            }
            if let Some(callback) = callback {
                callback();
            }
            self.lock().publishing = false;
            // Admissions made during notification are still driven without another caller or wake.
        }
    }
    fn prepare_next(&self, s: &mut State, pending: Pending) -> Option<Delivery> {
        let notify_failure = matches!(&pending.operation, Operation::Persist);
        let registered = match &pending.operation {
            Operation::Register(e) => Some(e.id.clone()),
            _ => None,
        };
        match self.prepare_submission(s, pending.operation, &pending.reply) {
            Ok(Submission::Writing(raw, prepared)) => {
                s.in_flight = Some(InFlight {
                    raw,
                    prepared: *prepared,
                    reply: pending.reply,
                    registered,
                    notify_failure,
                });
                None
            }
            Ok(Submission::Ready(completed)) => {
                clear_reserved(s, registered);
                Some(Delivery {
                    reply: pending.reply,
                    result: Ok(completed),
                    warning: None,
                })
            }
            Err(error) => {
                clear_reserved(s, registered);
                if notify_failure {
                    s.dirty = true;
                }
                let warning = notify_failure.then(|| error.message.clone());
                Some(Delivery {
                    reply: pending.reply,
                    result: Err(error),
                    warning,
                })
            }
        }
    }
    fn prepare_submission(
        &self,
        s: &mut State,
        operation: Operation,
        reply: &Reply,
    ) -> Result<Submission, Failure> {
        if s.uncertain {
            return Err(Failure::refused(
                "earlier webhook save outcome is unknown; mutation not executed",
            ));
        }
        let mut prepared = mutation::prepare(s, operation, reply)?;
        if !prepared.persistent {
            prepared.apply(s);
            return Ok(Submission::Ready(Completed {
                value: prepared.value,
                saved: None,
            }));
        }
        let raw = self.start_write(s, &prepared, reply)?;
        Ok(Submission::Writing(raw, Box::new(prepared)))
    }
    fn start_write(
        &self,
        s: &State,
        prepared: &Prepared,
        reply: &Reply,
    ) -> Result<command::Ticket<Result<(), String>>, Failure> {
        let Some(queue) = self.queue.as_ref() else {
            return Err(Failure::refused(
                "file writer unavailable; webhook mutation not executed",
            ));
        };
        let bytes = prepared
            .section_bytes(s)
            .saturating_add(self.path.as_os_str().len())
            .saturating_add(128);
        reply.grow(bytes)?;
        let request =
            super::super::persist::WriteRequest::new(self.path.clone(), prepared.section(s));
        #[cfg(test)]
        let panic_after_replace = self
            .panic_after_replace
            .swap(false, std::sync::atomic::Ordering::Relaxed);
        command::submit_classified(
            Some(queue),
            bytes,
            move || {
                let result = request.execute();
                #[cfg(test)]
                if panic_after_replace && result.is_ok() {
                    panic!("owned webhook post-replacement fault");
                }
                result
            },
            |result| match result {
                Ok(()) => 64,
                Err(e) => e.len().saturating_add(64),
            },
            |result| command::CommandDisposition {
                outcome: if result.is_ok() {
                    command::CommandOutcome::KnownSuccess
                } else {
                    command::CommandOutcome::KnownFailure
                },
                partial: false,
            },
        )
        .map_err(|error| Failure {
            message: error.message,
            unknown: error.unknown,
        })
    }
}
fn collect_ready(s: &mut State) -> Result<Option<Delivery>, ()> {
    let Some(active) = s.in_flight.as_ref() else {
        return Ok(None);
    };
    let result = active.raw.poll().ok_or(())?;
    let active = s.in_flight.take().expect("in-flight checked");
    Ok(Some(settle(s, active, result)))
}
fn clear_reserved(s: &mut State, id: Option<String>) {
    if let Some(id) = id {
        s.reserved.remove(&id);
    }
}
fn settle(
    s: &mut State,
    mut active: InFlight,
    result: Result<Result<(), String>, command::Failure>,
) -> Delivery {
    clear_reserved(s, active.registered);
    let (result, warning) = match result {
        Ok(saved) => {
            if saved.is_ok() {
                s.dirty = false;
            } else if active.prepared.continue_on_failure {
                s.dirty = true;
            }
            if saved.is_ok() || active.prepared.continue_on_failure {
                active.prepared.apply(s);
            }
            match saved {
                Ok(()) => (
                    Ok(Completed {
                        value: active.prepared.value,
                        saved: Some(true),
                    }),
                    None,
                ),
                Err(error) if active.prepared.continue_on_failure => (
                    Ok(Completed {
                        value: active.prepared.value,
                        saved: Some(false),
                    }),
                    Some(error),
                ),
                Err(error) => (
                    Err(Failure {
                        message: error,
                        unknown: false,
                    }),
                    None,
                ),
            }
        }
        Err(error) => {
            if error.unknown {
                s.uncertain = true;
            }
            let warning = active.notify_failure.then(|| error.message.clone());
            (
                Err(Failure {
                    message: error.message,
                    unknown: error.unknown,
                }),
                warning,
            )
        }
    };
    Delivery {
        reply: active.reply,
        result,
        warning,
    }
}
