//! Export receipt ownership; no View, draft text or payload has a diagnostic representation.
use super::{SettingsFileRequest, SettingsFileResult};
use crate::app::file_writer::{FileExecutor, command};
use sha2::{Digest, Sha256};
use std::{
    io,
    path::PathBuf,
    sync::{Arc, Weak},
};
use tasty_utils::batch_writer::Handle;

pub(crate) enum ExportError {
    Io(io::Error),
    Refused(String),
    Unknown,
}
#[derive(Default)]
pub(crate) struct Observation(Option<(Arc<()>, [u8; 32], PathBuf)>);
impl Observation {
    pub(crate) fn begin(&mut self, path: &std::path::Path, text: &str) -> Option<Weak<()>> {
        self.observe(text);
        if self.0.is_some() {
            return None;
        }
        let life = Arc::new(());
        let weak = Arc::downgrade(&life);
        self.0 = Some((
            life,
            Sha256::digest(text.as_bytes()).into(),
            path.to_owned(),
        ));
        Some(weak)
    }
    pub(crate) fn observe(&mut self, text: &str) {
        if self
            .0
            .as_ref()
            .is_some_and(|(_, hash, _)| *hash != <[u8; 32]>::from(Sha256::digest(text.as_bytes())))
        {
            self.0 = None;
        }
    }
    pub(crate) fn clear(&mut self) {
        self.0 = None;
    }
    pub(crate) fn pending(&self) -> bool {
        self.0.is_some()
    }
    pub(crate) fn finish(&mut self, observer: &Weak<()>) -> Option<PathBuf> {
        if self
            .0
            .as_ref()
            .is_some_and(|(life, _, _)| observer.ptr_eq(&Arc::downgrade(life)))
        {
            self.0.take().map(|(_, _, path)| path)
        } else {
            None
        }
    }
}

pub(crate) fn write(path: &std::path::Path, text: &str) -> io::Result<()> {
    let _lock = tasty_utils::file_replace::lock(path)?;
    tasty_utils::file_replace::replace(path, text.as_bytes())
}

pub(crate) struct PendingExport {
    observer: Weak<()>,
    ticket: command::Ticket<SettingsFileResult>,
}
impl PendingExport {
    pub(crate) fn submit(
        queue: Option<&Handle<FileExecutor>>,
        request: SettingsFileRequest,
    ) -> Result<Self, Box<SettingsFileResult>> {
        let SettingsFileRequest::Export {
            path,
            text,
            observer,
        } = request
        else {
            unreachable!("only exports are queued")
        };
        let failure_observer = observer.clone();
        let failure = |error| {
            Box::new(SettingsFileResult::ExportTransport {
                observer: failure_observer.clone(),
                error,
            })
        };
        let Some(queue) = queue else {
            return Err(failure(ExportError::Refused(
                "file writer is unavailable; export was not accepted".into(),
            )));
        };
        // Collector metadata is fixed size. Destination/text stay inside the budgeted request
        // or typed output; Unknown never leaves an uncharged path copy in the pending list.
        let retained = std::mem::size_of::<Self>() + std::mem::size_of::<winit::window::WindowId>();
        let bytes = text
            .capacity()
            .saturating_add(path.capacity())
            .saturating_add(std::mem::size_of::<SettingsFileRequest>())
            .saturating_add(retained);
        let pending_observer = observer.clone();
        let request = SettingsFileRequest::Export {
            path,
            text,
            observer,
        };
        let ticket = command::submit_classified(
            Some(queue),
            bytes,
            move || request.execute(),
            move |output| weigh(output).saturating_add(retained),
            classify,
        );
        match ticket {
            Ok(ticket) => Ok(Self {
                observer: pending_observer,
                ticket,
            }),
            Err(error) => Err(failure(transport(error))),
        }
    }
    pub(crate) fn poll(&self) -> Option<SettingsFileResult> {
        self.ticket.poll().map(|result| {
            result.unwrap_or_else(|error| SettingsFileResult::ExportTransport {
                observer: self.observer.clone(),
                error: transport(error),
            })
        })
    }
}
fn transport(error: command::Failure) -> ExportError {
    if error.unknown {
        ExportError::Unknown
    } else {
        ExportError::Refused(error.message)
    }
}
fn weigh(output: &SettingsFileResult) -> usize {
    match output {
        SettingsFileResult::Export { path, result, .. } => path
            .capacity()
            .saturating_add(std::mem::size_of::<SettingsFileResult>())
            .saturating_add(match result {
                Err(ExportError::Io(error)) => error.to_string().len(),
                Err(ExportError::Refused(message)) => message.capacity(),
                _ => 0,
            }),
        SettingsFileResult::Import { .. } | SettingsFileResult::ExportTransport { .. } => {
            unreachable!("only exports are queued")
        }
    }
}
fn classify(output: &SettingsFileResult) -> command::CommandDisposition {
    let outcome = match output {
        SettingsFileResult::Export { result: Ok(()), .. } => command::CommandOutcome::KnownSuccess,
        SettingsFileResult::Export {
            result: Err(ExportError::Unknown),
            ..
        } => command::CommandOutcome::Unknown,
        _ => command::CommandOutcome::KnownFailure,
    };
    command::CommandDisposition {
        outcome,
        partial: false,
    }
}

#[cfg(test)]
mod tests;
