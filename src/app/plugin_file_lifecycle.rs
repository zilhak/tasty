//! Admission-ordered plugin continuations; the common file worker owns every write.
use super::{App, file_writer::command};
use serde_json::Value;
use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
};

const MAX_PENDING: usize = 64;

#[derive(Clone, Debug)]
pub(crate) struct Failure {
    pub(crate) message: String,
    pub(crate) saved: Option<bool>,
    pub(crate) applied: bool,
    pub(crate) details: serde_json::Value,
}
impl Failure {
    pub(crate) fn response_data(&self) -> serde_json::Value {
        let mut data = if self.details.is_object() {
            self.details.clone()
        } else {
            serde_json::json!({})
        };
        if self.saved.is_none() {
            data["outcome"] = serde_json::json!("unknown");
        }
        data["saved"] = serde_json::json!(self.saved);
        data["applied"] = serde_json::json!(self.applied);
        data
    }
    pub(crate) fn before(error: impl std::fmt::Display) -> Self {
        Self {
            message: error.to_string(),
            saved: Some(false),
            applied: false,
            details: serde_json::Value::Null,
        }
    }
}
pub(crate) fn classify_result<T, E>(result: &Result<T, E>) -> command::CommandDisposition {
    command::CommandDisposition {
        outcome: if result.is_ok() {
            command::CommandOutcome::KnownSuccess
        } else {
            command::CommandOutcome::KnownFailure
        },
        partial: false,
    }
}
#[cfg(any(feature = "gui", test))]
pub(crate) fn classify_failure<T>(result: &Result<T, Failure>) -> command::CommandDisposition {
    match result {
        Ok(_) => command::CommandDisposition {
            outcome: command::CommandOutcome::KnownSuccess,
            partial: false,
        },
        Err(error) => command::CommandDisposition {
            outcome: if error.saved.is_none() {
                command::CommandOutcome::Unknown
            } else {
                command::CommandOutcome::KnownFailure
            },
            partial: error.applied
                || error.details["rollback_failed"].as_bool().unwrap_or(false)
                || error.details["cleanup_failed"].as_bool().unwrap_or(false),
        },
    }
}
#[derive(Clone, Default)]
pub(crate) struct Reply(Arc<Mutex<Option<Result<Value, Failure>>>>);
impl Reply {
    pub(crate) fn poll(&self) -> Option<Result<Value, Failure>> {
        self.0.lock().unwrap().take()
    }
    pub(crate) fn finish(&self, result: Result<Value, Failure>) {
        *self.0.lock().unwrap() = Some(result);
    }
}
type Start = Box<dyn FnOnce(&mut App)>;
type Finish = Box<dyn FnOnce(&mut App)>;
trait Pending {
    fn ready(&mut self, app: &mut App) -> Option<Finish>;
}
struct Accepted<T, F> {
    receipt: command::Ticket<T>,
    finish: Option<F>,
}
impl<T: Send + 'static, F: FnOnce(&mut App, Result<T, Failure>) + 'static> Pending
    for Accepted<T, F>
{
    fn ready(&mut self, app: &mut App) -> Option<Finish> {
        let _app = app;
        let output = self.receipt.poll()?.map_err(|error| Failure {
            message: error.message,
            saved: if error.unknown { None } else { Some(false) },
            applied: false,
            details: serde_json::Value::Null,
        });
        let finish = self.finish.take().expect("file continuation consumed once");
        Some(Box::new(move |app| finish(app, output)))
    }
}
struct Retired {
    id: String,
    finish: Option<Finish>,
}
impl Pending for Retired {
    fn ready(&mut self, app: &mut App) -> Option<Finish> {
        if app
            .plugin_manager
            .as_mut()
            .is_some_and(|mgr| !mgr.poll_file_retirement(&self.id))
        {
            return None;
        }
        self.finish.take()
    }
}
#[derive(Default)]
pub(crate) struct PluginFiles {
    queue: VecDeque<Start>,
    active: Option<Box<dyn Pending>>,
    closed: bool,
    #[cfg(feature = "gui")]
    pub(crate) shortcuts:
        Option<std::rc::Rc<std::cell::RefCell<super::plugin_glue::shortcut::Draft>>>,
    #[cfg(feature = "gui")]
    pub(crate) failed_shortcuts: super::plugin_glue::shortcut::Draft,
    #[cfg(feature = "gui")]
    retry_shortcuts: bool,
    #[cfg(feature = "gui")]
    unsaved_reported: bool,
}
impl PluginFiles {
    pub(crate) fn enqueue(
        &mut self,
        start: impl FnOnce(&mut App) + 'static,
    ) -> Result<(), Failure> {
        if self.closed || self.queue.len() + usize::from(self.active.is_some()) >= MAX_PENDING {
            return Err(Failure::before(
                "plugin file lifecycle admission is closed or full",
            ));
        }
        #[cfg(feature = "gui")]
        {
            self.shortcuts = None;
        }
        self.enqueue_retained(start)
    }
    /// Only an already failed snapshot uses this once during shutdown; commands never replay.
    #[cfg(feature = "gui")]
    pub(crate) fn enqueue_shortcut_retry(
        &mut self,
        start: impl FnOnce(&mut App) + 'static,
    ) -> Result<(), Failure> {
        self.enqueue_retained(start)
    }
    fn enqueue_retained(&mut self, start: impl FnOnce(&mut App) + 'static) -> Result<(), Failure> {
        if self.pending() >= MAX_PENDING {
            return Err(Failure::before("plugin file lifecycle queue full"));
        }
        self.queue.push_back(Box::new(start));
        Ok(())
    }
    #[cfg(feature = "gui")]
    pub(crate) fn coalesce_shortcuts(
        &mut self,
        draft: super::plugin_glue::shortcut::Draft,
    ) -> Result<(), super::plugin_glue::shortcut::Draft> {
        match &self.shortcuts {
            Some(pending) => {
                pending.borrow_mut().extend(draft);
                Ok(())
            }
            None => Err(draft),
        }
    }
    #[cfg(feature = "gui")]
    pub(crate) fn settle_shortcuts(
        &mut self,
        draft: super::plugin_glue::shortcut::Draft,
        saved: bool,
    ) {
        if saved {
            for key in draft.keys() {
                self.failed_shortcuts.remove(key);
            }
        } else {
            self.failed_shortcuts.extend(draft);
        }
    }
    pub(crate) fn stopping(&self) -> bool {
        self.closed
    }
    pub(crate) fn pending(&self) -> usize {
        self.queue.len() + usize::from(self.active.is_some())
    }
}
impl App {
    /// May submit the next stage of an already accepted operation. Drain these before closing files.
    pub(crate) fn finish_plugin_files(&mut self) {
        if let Some(mut active) = self.plugin_files.active.take() {
            match active.ready(self) {
                Some(finish) => finish(self),
                None => self.plugin_files.active = Some(active),
            }
        }
        #[cfg(feature = "gui")]
        if self.plugin_files.closed && self.plugin_files.pending() == 0 {
            if self.plugin_files.retry_shortcuts {
                self.plugin_files.retry_shortcuts = false;
                let dirty = std::mem::take(&mut self.plugin_files.failed_shortcuts);
                self.queue_plugin_shortcut_draft(dirty, true);
            } else if !self.plugin_files.failed_shortcuts.is_empty()
                && !self.plugin_files.unsaved_reported
            {
                self.plugin_files.unsaved_reported = true;
                tracing::warn!(
                    edits = self.plugin_files.failed_shortcuts.len(),
                    "plugin shortcuts remain unsaved after shutdown retry"
                );
            }
        }
        if self.plugin_files.active.is_none() {
            if let Some(start) = self.plugin_files.queue.pop_front() {
                start(self);
            }
            if self.plugin_files.pending() != 0 {
                (self.services.reply_wake())();
            }
        }
    }
    pub(crate) fn stop_plugin_file_producers(&mut self) {
        #[cfg(feature = "gui")]
        if !self.plugin_files.closed {
            self.plugin_files.retry_shortcuts = true;
        }
        self.plugin_files.closed = true;
    }
    pub(crate) fn plugin_files_pending(&self) -> usize {
        self.plugin_files.pending()
    }

    pub(crate) fn after_plugin_retired(
        &mut self,
        id: String,
        finish: impl FnOnce(&mut App) + 'static,
    ) {
        self.plugin_files.active = Some(Box::new(Retired {
            id,
            finish: Some(Box::new(finish)),
        }));
    }

    pub(crate) fn run_plugin_file<T: Send + 'static>(
        &mut self,
        bytes: usize,
        work: impl FnOnce() -> T + Send + 'static,
        weigh: impl FnOnce(&T) -> usize + Send + 'static,
        classify: impl FnOnce(&T) -> command::CommandDisposition + Send + 'static,
        finish: impl FnOnce(&mut App, Result<T, Failure>) + 'static,
    ) {
        let queue = self.services.file_command_queue();
        let submitted = if let Some(queue) = &queue {
            command::submit_classified(Some(queue), bytes, work, weigh, classify).map_err(|error| {
                Failure {
                    message: error.message,
                    saved: if error.unknown { None } else { Some(false) },
                    applied: false,
                    details: serde_json::Value::Null,
                }
            })
        } else {
            Err(Failure::before(
                "file writer unavailable; plugin operation not accepted",
            ))
        };
        match submitted {
            Ok(receipt) => {
                self.plugin_files.active = Some(Box::new(Accepted {
                    receipt,
                    finish: Some(finish),
                }))
            }
            Err(error) => finish(self, Err(error)),
        }
    }
}

#[cfg(all(test, not(feature = "gui")))]
mod tests;

#[cfg(all(test, feature = "gui"))]
mod shortcut_tests;

#[cfg(test)]
mod classification_tests;
