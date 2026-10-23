//! Accepted remote file requests outlive their Views; UI effects require the original observer.
mod preparation;
mod retention;
use crate::adapters::ui::popup::remote_tool::{self, AttachForm, PasskeyForm, ProfileForm};
use crate::app::remote_file_commands as files;
use crate::view::ui::View as _;
use std::sync::{
    Arc, Weak,
    atomic::{AtomicUsize, Ordering},
};
use tasty_remote_profiles::{Passkey, PasskeyCommand, ProfileCommand};

#[derive(Clone)]
pub(crate) enum FileAction {
    SaveProfile(ProfileForm),
    SaveAttach(AttachForm),
    SavePasskey(PasskeyForm),
    DeleteProfile(String),
    DeletePasskey(String),
    Reveal { key: Passkey, editing: bool },
    Detect(String),
}
/// The final request copy releases its pending form state, including rejected/retired observers.
pub(crate) struct PendingToken(Arc<AtomicUsize>);
impl PendingToken {
    pub(crate) fn admit(counter: Arc<AtomicUsize>) -> Arc<Self> {
        counter.fetch_add(1, Ordering::Relaxed);
        Arc::new(Self(counter))
    }
}
impl Drop for PendingToken {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::Relaxed);
    }
}
#[derive(Clone)]
pub(crate) struct FileRequest {
    pub(crate) popup: Weak<()>,
    pub(crate) view: Weak<()>,
    pub(crate) action: FileAction,
    pub(crate) _pending: Arc<PendingToken>,
}
pub(crate) enum FileValue {
    Saved,
    Detection(tasty_remote::profile_detection::DetectionId, String),
    Revealed(String),
}
#[derive(Default)]
pub(crate) struct Pending {
    budget: retention::Budget,
    requests: Vec<Waiting>,
}
struct Waiting {
    window: winit::window::WindowId,
    request: FileRequest,
    ticket: files::Ticket,
    _retained: retention::Reservation,
}
impl Drop for Pending {
    fn drop(&mut self) {
        if !self.requests.is_empty() {
            tracing::warn!(
                pending = self.requests.len(),
                "remote file results not collected before owner drop"
            );
        }
    }
}
fn prepare(action: FileAction) -> Result<files::Request, String> {
    match action {
        FileAction::SaveProfile(form) => preparation::profile(form),
        FileAction::SaveAttach(form) => preparation::attach(form),
        FileAction::SavePasskey(form) => preparation::key(form),
        FileAction::DeleteProfile(name) => Ok(files::Request::Profile {
            command: Box::new(ProfileCommand::Remove { name }),
            linked: None,
            detect: false,
        }),
        FileAction::DeletePasskey(name) => {
            Ok(files::Request::Passkey(PasskeyCommand::Remove { name }))
        }
        FileAction::Reveal { key, .. } => Ok(files::Request::Reveal(key)),
        FileAction::Detect(_) => Err("detection preparation belongs to its retained owner".into()),
    }
}

impl super::App {
    pub(crate) fn process_remote_tool_requests(&mut self, id: winit::window::WindowId) {
        let Some(main) = self.view.views.get(&id).and_then(|view| view.as_main()) else {
            return;
        };
        let ctx = main.base.gpu.egui_ctx.clone();
        let identity = main.base.state.identity();
        for request in remote_tool::take_file_requests(&ctx) {
            if !request.view.ptr_eq(&identity) {
                continue;
            }
            if matches!(&request.action, FileAction::Reveal { .. })
                && request.popup.strong_count() == 0
            {
                continue;
            }
            let retained = match self
                .remote_io
                .budget
                .reserve(retention::weight(&request.action))
            {
                Ok(reservation) => reservation,
                Err(error) => {
                    remote_tool::accept_file_result(&ctx, &request, Err(error.into()));
                    continue;
                }
            };
            if let FileAction::Detect(name) = &request.action {
                let result = self
                    .services
                    .profile_detections
                    .enqueue(name.clone())
                    .map(|id| FileValue::Detection(id, name.clone()));
                remote_tool::accept_file_result(&ctx, &request, result);
                continue;
            }
            let command = match prepare(request.action.clone()) {
                Ok(command) => command,
                Err(error) => {
                    remote_tool::accept_file_result(&ctx, &request, Err(error));
                    continue;
                }
            };
            match files::submit(&self.services, command) {
                Ok(ticket) => self.remote_io.requests.push(Waiting {
                    window: id,
                    request,
                    ticket,
                    _retained: retained,
                }),
                Err(error) => {
                    remote_tool::accept_file_result(&ctx, &request, Err(error.gui_message()));
                }
            }
        }
    }
    pub(crate) fn finish_remote_files(&mut self) {
        let mut running = Vec::new();
        for pending in std::mem::take(&mut self.remote_io.requests) {
            let Some(result) = files::poll(&pending.ticket) else {
                running.push(pending);
                continue;
            };
            let result = result
                .map(|output| {
                    let detection = output.begin_detection(&self.services);
                    match output.value {
                        files::Value::Revealed(value) => FileValue::Revealed(value),
                        _ => detection.map_or(FileValue::Saved, |(id, name)| {
                            FileValue::Detection(id, name)
                        }),
                    }
                })
                .map_err(|error| error.gui_message());
            let error = result.as_ref().err().cloned();
            let delivered = self
                .view
                .views
                .get_mut(&pending.window)
                .and_then(|view| view.as_main_mut())
                .filter(|main| pending.request.view.ptr_eq(&main.base.state.identity()))
                .is_some_and(|main| {
                    let delivered = remote_tool::accept_file_result(
                        &main.base.gpu.egui_ctx,
                        &pending.request,
                        result,
                    );
                    main.mark_dirty();
                    delivered
                });
            if !delivered && let Some(error) = error {
                tracing::warn!(%error,"remote file request failed after its observer retired");
            }
        }
        self.remote_io.requests = running;
    }
    pub(crate) fn poll_profile_detections(&mut self) {
        for update in self.services.profile_detections.poll() {
            for view in self.view.views.values_mut() {
                if let Some(main) = view.as_main_mut()
                    && remote_tool::accept_detection(&main.base.gpu.egui_ctx, &update)
                {
                    main.mark_dirty();
                }
            }
        }
    }
}

#[cfg(test)]
mod tests;
