use crate::app::file_writer::{FileExecutor, command};
use std::{path::Path, sync::Arc};
use tasty_remote::profile_detection::persistence::{
    Failure, Input, Receipt, Request, Submit, Value,
};
use tasty_remote_profiles::{
    Passkeys, ProfileCommand, ProfilePatch, RemoteFileStore, RemoteProfiles,
};
use tasty_utils::batch_writer::Handle;

pub(crate) fn sink(queue: Option<Handle<FileExecutor>>) -> Option<Submit> {
    let queue = queue?;
    Some(Arc::new(move |request| {
        let bytes = input_bytes(&request);
        command::submit_classified(
            Some(&queue),
            bytes,
            move || execute(request),
            output_bytes,
            |result| command::CommandDisposition {
                outcome: if result.is_ok() {
                    command::CommandOutcome::KnownSuccess
                } else {
                    command::CommandOutcome::KnownFailure
                },
                partial: false,
            },
        )
        .map(|raw| Box::new(Queued(raw)) as Box<dyn Receipt>)
        .map_err(transport)
    }))
}
struct Queued(command::Ticket<Result<Value, Failure>>);
impl Receipt for Queued {
    fn poll(&mut self) -> Option<Result<Value, Failure>> {
        self.0
            .poll()
            .map(|result| result.map_err(transport).and_then(|value| value))
    }
}
fn transport(error: command::Failure) -> Failure {
    Failure {
        message: error.message,
        unknown: error.unknown,
    }
}
fn known(message: impl Into<String>) -> Failure {
    Failure {
        message: message.into(),
        unknown: false,
    }
}
fn input_bytes(request: &Request) -> usize {
    match request {
        Request::Load {
            root,
            name,
            expected,
        } => root
            .as_os_str()
            .len()
            .saturating_add(name.len())
            .saturating_add(expected.as_ref().map_or(0, command::value_bytes))
            .saturating_add(256),
        Request::Save { root, original, .. } => root
            .as_os_str()
            .len()
            .saturating_add(command::value_bytes(original))
            .saturating_add(256),
    }
}
fn output_bytes(result: &Result<Value, Failure>) -> usize {
    match result {
        Ok(Value::Saved) => 64,
        Ok(Value::Loaded(input)) => command::value_bytes(&input.original)
            .saturating_add(command::value_bytes(&input.passkeys))
            .saturating_add(input.root.as_os_str().len())
            .saturating_add(128),
        Err(error) => error.message.len().saturating_add(64),
    }
}
pub(super) fn read<T: serde::de::DeserializeOwned + Default>(
    root: &Path,
    file: &str,
) -> Result<T, Failure> {
    let bytes = match std::fs::read(root.join(file)) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(T::default()),
        Err(error) => return Err(known(format!("could not read {file}: {}", error.kind()))),
    };
    let text = std::str::from_utf8(&bytes)
        .map_err(|_| known(format!("{file} is not UTF-8; original preserved")))?;
    toml::from_str(text).map_err(|_| known(format!("{file} is not valid TOML; original preserved")))
}
fn execute(request: Request) -> Result<Value, Failure> {
    match request {
        Request::Load {
            root,
            name,
            expected,
        } => {
            let profiles: RemoteProfiles = read(&root, "remote-profiles.toml")?;
            let original = profiles
                .get(&name)
                .cloned()
                .ok_or_else(|| known("remote profile not found"))?;
            if let Some(mut expected) = expected {
                let mut current = original.clone();
                expected.remove_field("detect_failed");
                current.remove_field("detect_failed");
                if expected != current {
                    return Err(known("remote profile changed before detection"));
                }
            }
            let passkeys: Passkeys = read(&root, "passkeys.toml")?;
            Ok(Value::Loaded(Input {
                original,
                passkeys,
                root,
            }))
        }
        Request::Save {
            root,
            original,
            failed,
            cancel,
        } => {
            if cancel.is_cancelled() {
                return Err(known("remote profile detection cancelled"));
            }
            let mut patch = ProfilePatch {
                expected: Some(original.clone()),
                ..Default::default()
            };
            patch
                .fields
                .insert("detect_failed".into(), failed.then(|| "true".into()));
            RemoteFileStore::at(root)
                .profile(
                    ProfileCommand::Patch {
                        name: original.name,
                        patch,
                    },
                    None,
                )
                .map_err(|error| known(format!("save detection status: {error}")))?;
            Ok(Value::Saved)
        }
    }
}

#[cfg(test)]
mod tests;
