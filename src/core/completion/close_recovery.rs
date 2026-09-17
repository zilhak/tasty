//! Durable close compensation independent of the fallible main journal UPDATE.
use super::*;
use anyhow::Result;
use close_effects::CloseTask;

impl Completion {
    pub fn exited(&self, surface: u32, cause: &str) -> Result<()> {
        self.forget_live(surface)?;
        let id = {
            let mut guard = self
                .inner
                .lock()
                .map_err(|_| anyhow::anyhow!("completion journal poisoned"))?;
            if let Some(task) = guard
                .1
                .pending_closes
                .values()
                .find(|t| t.blocks_surface(&guard.1, surface))
            {
                task.id.clone()
            } else {
                let id = format!("{:032x}", rand::random::<u128>());
                let mut task = CloseTask::capture(&guard.1, surface, cause, id.clone())?;
                // Even failure to persist the intent stays explicit and blocks sends
                // in this process; retry attempts intent persistence before applying it.
                match persist_task(&guard.0, &task) {
                    Ok(()) => task.durable = true,
                    Err(error) => task.error = format!("close_intent_not_durable: {error:#}"),
                }
                guard.1.pending_closes.insert(id.clone(), task);
                id
            }
        };
        self.retry_close(&id)
    }
    pub fn retry_pending_closes(&self) -> Result<()> {
        let ids: Vec<_> = self
            .inner
            .lock()
            .map_err(|_| anyhow::anyhow!("completion journal poisoned"))?
            .1
            .pending_closes
            .values()
            .filter(|t| t.retry_at <= now())
            .map(|t| t.id.clone())
            .collect();
        let mut errors = Vec::new();
        for id in ids {
            if let Err(error) = self.retry_close(&id) {
                errors.push(format!("{id}: {error:#}"));
            }
        }
        if !errors.is_empty() {
            anyhow::bail!("close recovery pending: {}", errors.join("; "));
        }
        Ok(())
    }
    fn retry_close(&self, id: &str) -> Result<()> {
        let mut guard = self
            .inner
            .lock()
            .map_err(|_| anyhow::anyhow!("completion journal poisoned"))?;
        let Some(mut task) = guard.1.pending_closes.get(id).cloned() else {
            return Ok(());
        };
        let result = (|| -> Result<Journal> {
            if !task.durable {
                persist_task(&guard.0, &task)?;
                task.durable = true;
            }
            let mut next = guard.1.clone();
            task.apply(&mut next);
            next.pending_closes.remove(id);
            let body = serde_json::to_string(&next)?;
            let transaction = guard.0.transaction()?;
            transaction.execute("INSERT INTO completion_journal VALUES(1,?1) ON CONFLICT(id) DO UPDATE SET body=excluded.body", [&body])?;
            transaction.execute("DELETE FROM completion_closures WHERE id=?1", [id])?;
            transaction.commit()?;
            Ok(next)
        })();
        match result {
            Ok(next) => {
                guard.1 = next;
                Ok(())
            }
            Err(error) => {
                task.attempts += 1;
                task.retry_at = now() + 2_u64.pow(task.attempts.min(6)).min(60);
                task.error = format!("close_persistence_pending: {error:#}");
                if let Err(save_error) = persist_task(&guard.0, &task) {
                    task.error
                        .push_str(&format!("; intent status save failed: {save_error:#}"));
                } else {
                    task.durable = true;
                }
                guard.1.pending_closes.insert(id.to_string(), task);
                Err(error)
            }
        }
    }
}
fn persist_task(connection: &Connection, task: &CloseTask) -> Result<()> {
    connection.execute("INSERT INTO completion_closures(id,body) VALUES(?1,?2) ON CONFLICT(id) DO UPDATE SET body=excluded.body", [&task.id, &serde_json::to_string(task)?])?;
    Ok(())
}
