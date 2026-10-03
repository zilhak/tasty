//! Typed admission and durable key lookup, before target resolution.
use super::*;

pub(super) fn admit(
    executor: &Executor<StructureDecider>,
    pending: &mut HashMap<u64, Pending>,
    ticket: u64,
    admission: Admission,
    deferred: &mut Option<Admission>,
) -> Result<ResultValue, JournalError> {
    if pending.contains_key(&ticket) || pending.values().any(|p| p.followers.contains(&ticket)) {
        return Err("journal admission ticket already exists".into());
    }
    let pending_count: usize = pending.values().map(|p| 1 + p.followers.len()).sum();
    if let Some(key) = &admission.key {
        for (leader, existing) in pending.iter_mut() {
            if existing.admission.key.as_ref() == Some(key) {
                if existing.admission.original_digest != admission.original_digest {
                    return Err(JournalError::KeyConflict);
                }
                if pending_count >= QUEUE_CAPACITY {
                    return Err(JournalError::QueueFull(
                        "journal admission capacity exhausted",
                    ));
                }
                existing.followers.push(ticket);
                return Ok(ResultValue::JoinedAdmission {
                    #[cfg(test)]
                    leader_ticket: *leader,
                });
            }
        }
    }
    // Recover before exposing any stored success; lookup still precedes target resolution.
    executor.with_state(|_| ()).map_err(|e| e.to_string())?;
    if let Some(key) = &admission.key {
        let inner = executor.inner.lock().map_err(|e| e.to_string())?;
        match inner
            .store
            .lookup_command(key, &admission.original_digest)
            .map_err(|e| e.to_string())?
        {
            CommandLookup::Hit(record) => {
                return Ok(recovery::command_result(&inner.state, record, true));
            }
            CommandLookup::DigestMismatch(_) => {
                return Err(JournalError::KeyConflict);
            }
            CommandLookup::Miss => {}
        }
    }
    let disk_credit = {
        let outstanding = pending.values().map(|entry| entry.disk_credit).sum();
        let mut inner = executor.inner.lock().map_err(|error| error.to_string())?;
        let epoch = inner.epoch;
        match inner.store.ensure_new_admission(epoch, outstanding) {
            Ok(_) => {}
            // Credits held by unresolved admissions return on their Resolve or Cancel.
            // Wait for them instead of failing; with none outstanding the store is full.
            Err(tasty_event_store::StoreError::AdmissionCapacity { .. }) if !pending.is_empty() => {
                *deferred = Some(admission);
                return Ok(ResultValue::NeedsResolution);
            }
            Err(error) => return Err(error.into()),
        }
        inner.store.admission_budget().command_credit_bytes
    };
    let held_bytes: usize = pending
        .values()
        .map(|pending| pending.admission.original_digest.len())
        .sum();
    if held_bytes.saturating_add(admission.original_digest.len())
        > crate::runtime::journal_product::MAX_QUEUED_BYTES
    {
        return Err(JournalError::QueueFull(
            "journal pending admission byte capacity exhausted",
        ));
    }
    if pending_count >= QUEUE_CAPACITY {
        return Err(JournalError::QueueFull(
            "journal admission capacity exhausted",
        ));
    }
    pending.insert(
        ticket,
        Pending {
            disk_credit,
            admission,
            followers: Vec::new(),
            reservations: Vec::new(),
            inputs: Vec::new(),
        },
    );
    Ok(ResultValue::NeedsResolution)
}
