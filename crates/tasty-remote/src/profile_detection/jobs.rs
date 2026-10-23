use super::{DetectionId, DetectionUpdate, persistence};
use std::path::PathBuf;
use std::thread::JoinHandle;
use tasty_remote_profiles::RemoteProfile;

pub(super) struct Job {
    id: DetectionId,
    pub(super) name: String,
    pub(super) cancel: tasty_ssh::SshCancel,
    stage: Stage,
}
enum Stage {
    Consumed,
    Preparing(Box<dyn persistence::Receipt>),
    Probing {
        original: RemoteProfile,
        root: PathBuf,
        worker: JoinHandle<Result<String, String>>,
    },
    Saving {
        receipt: Box<dyn persistence::Receipt>,
        result: Result<String, String>,
    },
}
pub(super) enum Progress {
    Pending(Job),
    Done(DetectionUpdate),
}
impl Job {
    pub(super) fn preparing(
        id: DetectionId,
        name: String,
        receipt: Box<dyn persistence::Receipt>,
    ) -> Self {
        Self {
            id,
            name,
            cancel: tasty_ssh::SshCancel::new(),
            stage: Stage::Preparing(receipt),
        }
    }
    fn done(self, result: Result<String, String>) -> Progress {
        Progress::Done(DetectionUpdate {
            id: self.id,
            name: self.name,
            result,
        })
    }
    fn pending(self, stage: Stage) -> Progress {
        Progress::Pending(Self { stage, ..self })
    }
    pub(super) fn advance(
        mut self,
        stopping: bool,
        sink: Option<&persistence::Submit>,
    ) -> Progress {
        // Replace only the consumed stage. All early Pending paths restore its owner.
        let stage = std::mem::replace(&mut self.stage, Stage::Consumed);
        match stage {
            Stage::Consumed => self.done(Err(
                "profile detection result owner was lost; outcome unknown".into(),
            )),
            Stage::Preparing(mut receipt) => {
                let Some(result) = receipt.poll() else {
                    return self.pending(Stage::Preparing(receipt));
                };
                if stopping || self.cancel.is_cancelled() {
                    return self.done(Err("remote profile detection cancelled".into()));
                }
                match result {
                    Ok(persistence::Value::Loaded(input)) => self.probe(input),
                    Ok(_) => self.done(Err("unexpected profile preparation result".into())),
                    Err(error) => self.done(Err(error.message)),
                }
            }
            Stage::Probing {
                original,
                root,
                worker,
            } => {
                if !worker.is_finished() {
                    return self.pending(Stage::Probing {
                        original,
                        root,
                        worker,
                    });
                }
                let result = worker
                    .join()
                    .unwrap_or_else(|_| Err("remote profile detection worker panicked".into()));
                if stopping || self.cancel.is_cancelled() {
                    return self.done(Err("remote profile detection cancelled".into()));
                }
                self.save(original, root, result, sink)
            }
            Stage::Saving {
                mut receipt,
                result,
            } => {
                let Some(saved) = receipt.poll() else {
                    return self.pending(Stage::Saving { receipt, result });
                };
                match saved {
                    Ok(persistence::Value::Saved) => self.done(result),
                    Ok(_) => self.done(Err("unexpected profile save result".into())),
                    Err(error) => self.done(Err(error.message)),
                }
            }
        }
    }
    fn probe(self, input: persistence::Input) -> Progress {
        let persistence::Input {
            original,
            passkeys,
            root,
        } = input;
        let value = original.clone();
        let cancel = self.cancel.clone();
        match std::thread::Builder::new()
            .name("remote-profile-detect".into())
            .spawn(move || {
                let _scope = cancel.scope();
                tasty_ssh::detect_for_profile(&value, &passkeys)
                    .map(|mode| mode.as_str().to_owned())
                    .map_err(|error| error.to_string())
            }) {
            Ok(worker) => self.pending(Stage::Probing {
                original,
                root,
                worker,
            }),
            Err(error) => self.done(Err(error.to_string())),
        }
    }
    fn save(
        self,
        original: RemoteProfile,
        root: PathBuf,
        result: Result<String, String>,
        sink: Option<&persistence::Submit>,
    ) -> Progress {
        let Some(sink) = sink else {
            return self.done(Err("profile file writer is unavailable".into()));
        };
        let request = persistence::Request::Save {
            root,
            original,
            failed: result.is_err(),
            cancel: self.cancel.clone(),
        };
        match sink(request) {
            Ok(receipt) => self.pending(Stage::Saving { receipt, result }),
            Err(error) => self.done(Err(error.message)),
        }
    }
}

#[cfg(test)]
mod tests;
