//! View requests and receipts only; App owns the tutorial database writer.
use super::{TutorialRuntime, all_topics};
use crate::app::tutorial_progress::{Receipt, Reply, Save};

pub(super) struct Pending {
    pub(super) topic: Option<usize>,
    generations: Vec<u64>,
    receipt: Receipt,
}
impl TutorialRuntime {
    pub fn load_progress(&mut self) {
        self.catalog_loaded = true;
        if self
            .pending_progress
            .iter()
            .any(|pending| pending.topic.is_none())
        {
            return;
        }
        for topic in 0..self.progress.len() {
            if self.progress[topic].dirty {
                self.save_progress(topic);
            }
        }
        if self.pending_progress.len() >= 32 {
            self.save_error = true;
            return;
        }
        match self.persistence.load() {
            Ok(receipt) => self.pending_progress.push_back(Pending {
                topic: None,
                generations: self.progress_generations.clone(),
                receipt,
            }),
            Err(error) => {
                tracing::warn!(%error, "tutorial load admission failed");
                self.save_error = true;
            }
        }
    }
    pub fn save_progress(&mut self, topic: usize) {
        self.progress[topic].dirty = true;
        self.progress_generations[topic] = self.progress_generations[topic].wrapping_add(1);
        if self.pending_progress.len() >= 32 {
            self.save_error = true;
            return;
        }
        let definition = &all_topics()[topic];
        let progress = &self.progress[topic];
        let save = Save {
            topic: definition.id,
            revision: definition.revision,
            step: definition.steps[progress.resume].id,
            completed: progress.completed,
            expected: progress.row_version,
        };
        match self.persistence.save(save) {
            Ok(receipt) => self.pending_progress.push_back(Pending {
                topic: Some(topic),
                generations: vec![self.progress_generations[topic]],
                receipt,
            }),
            Err(error) => {
                tracing::warn!(%error, "tutorial save admission failed");
                self.save_error = true;
            }
        }
    }
    pub(crate) fn poll_progress(&mut self) -> bool {
        let mut changed = false;
        while let Some(pending) = self.pending_progress.front() {
            let result = match pending.receipt.try_recv() {
                Ok(result) => result,
                Err(std::sync::mpsc::TryRecvError::Empty) => break,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    Err("tutorial persistence disconnected".into())
                }
            };
            let pending = self.pending_progress.pop_front().expect("progress receipt");
            match result {
                Ok(Reply::Loaded(records)) => {
                    for (topic, record) in records.into_iter().enumerate() {
                        if self.progress[topic].dirty
                            || self.progress_generations[topic] != pending.generations[topic]
                        {
                            continue;
                        }
                        let definition = &all_topics()[topic];
                        let progress = &mut self.progress[topic];
                        progress.completed = record.completed;
                        progress.started = record.resume_step.is_some();
                        progress.row_version = record.version;
                        progress.resume = definition
                            .steps
                            .iter()
                            .position(|step| Some(step.id) == record.resume_step.as_deref())
                            .unwrap_or(0);
                    }
                    self.save_error = self.progress.iter().any(|progress| progress.dirty);
                }
                Ok(Reply::Saved { record, updated }) => {
                    let topic = pending.topic.expect("save reply");
                    let progress = &mut self.progress[topic];
                    progress.completed |= record.completed;
                    if updated {
                        progress.row_version = record.version;
                    }
                    if self.progress_generations[topic] == pending.generations[0] {
                        progress.dirty = false;
                    }
                    self.save_error = self.progress.iter().any(|progress| progress.dirty)
                        && self.pending_progress.is_empty();
                }
                Err(error) => {
                    tracing::warn!(%error, "tutorial persistence failed");
                    self.save_error = true;
                }
            }
            changed = true;
        }
        changed
    }
}
