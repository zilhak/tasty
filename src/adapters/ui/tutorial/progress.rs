//! Database effects run at user transitions, never in the geometry renderer.
use super::{TutorialRuntime, all_topics};

impl TutorialRuntime {
    pub fn load_progress(&mut self) {
        self.catalog_loaded = true;
        if self.save_error {
            for topic in 0..self.progress.len() {
                if self.progress[topic].dirty {
                    self.save_progress(topic);
                }
            }
            if self.progress.iter().any(|p| p.dirty) {
                return;
            }
        }
        let result = crate::db::with_db(|db| {
            all_topics()
                .iter()
                .map(|t| crate::store::tutorial_progress::load(&db.conn, t.id, t.revision))
                .collect::<rusqlite::Result<Vec<_>>>()
        });
        match result {
            Some(Ok(records)) => {
                for ((progress, topic), record) in
                    self.progress.iter_mut().zip(all_topics()).zip(records)
                {
                    progress.completed = record.completed;
                    progress.started = record.resume_step.is_some();
                    progress.row_version = record.version;
                    progress.resume = topic
                        .steps
                        .iter()
                        .position(|s| Some(s.id) == record.resume_step.as_deref())
                        .unwrap_or(0);
                }
                self.save_error = false;
            }
            Some(Err(e)) => {
                tracing::warn!("tutorial history load failed: {e}");
                self.save_error = true;
            }
            None => {
                self.save_error = true;
            }
        }
    }

    pub fn save_progress(&mut self, topic: usize) {
        let definition = &all_topics()[topic];
        let progress = &self.progress[topic];
        let result = crate::db::with_db(|db| {
            crate::store::tutorial_progress::save(
                &mut db.conn,
                definition.id,
                definition.revision,
                definition.steps[progress.resume].id,
                progress.completed,
                progress.row_version,
            )
        });
        match result {
            Some(Ok((record, updated))) => {
                self.progress[topic].completed |= record.completed;
                // On conflict the database position wins until a new catalog session.
                // Do not adopt its version and then overwrite it on the next step.
                if updated {
                    self.progress[topic].row_version = record.version;
                }
                self.progress[topic].dirty = false;
                self.save_error = false;
            }
            Some(Err(e)) => {
                tracing::warn!("tutorial history save failed: {e}");
                self.save_error = true;
            }
            None => {
                self.save_error = true;
            }
        }
    }
}
