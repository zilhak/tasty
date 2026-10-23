use super::Failure;
#[cfg(feature = "gui")]
use super::detection;
use crate::app::{file_writer::command, services::AppServices};
use std::path::PathBuf;
use tasty_remote_profiles::{
    ManagedCleanup, PasskeyCommand, PasskeyOutcome, ProfileCommand, ProfileOutcome, ProfilePatch,
    RemoteFileStore,
};

#[cfg(feature = "gui")]
use tasty_remote_profiles::{Passkey, Passkeys};

pub(crate) enum Request {
    Profile {
        command: Box<ProfileCommand>,
        linked: Option<PasskeyCommand>,
        detect: bool,
    },
    Passkey(PasskeyCommand),
    #[cfg(feature = "gui")]
    InsertPasskey(PasskeyCommand),
    #[cfg(feature = "gui")]
    RenamePasskey {
        original: String,
        replacement: PasskeyCommand,
    },
    #[cfg(feature = "gui")]
    Reveal(Passkey),
}
pub(crate) enum Value {
    Profile(ProfileOutcome),
    Passkey(PasskeyOutcome),
    #[cfg(feature = "gui")]
    Revealed(String),
}
pub(crate) struct Output {
    pub(crate) root: PathBuf,
    pub(crate) value: Value,
    detect: bool,
}
impl Output {
    /// Backend receipt already arrived. Admission may queue a read, but never starts SSH here.
    pub(crate) fn begin_detection(
        &self,
        core: &AppServices,
    ) -> Option<(tasty_remote::profile_detection::DetectionId, String)> {
        if !self.detect {
            return None;
        }
        let Value::Profile(outcome) = &self.value else {
            return None;
        };
        let profile = outcome.profile.as_ref()?;
        match core
            .profile_detections
            .enqueue_saved_at(self.root.clone(), profile.clone())
        {
            Ok(id) => Some((id, profile.name.clone())),
            Err(error) => {
                tracing::warn!(%error,"profile saved but detection preparation was not accepted");
                None
            }
        }
    }
}
pub(crate) type Ticket = command::Ticket<Result<Output, Failure>>;
pub(crate) fn submit(core: &AppServices, request: Request) -> Result<Ticket, Failure> {
    validate(&request)?;
    let root = tasty_utils::path::tasty_home()
        .ok_or_else(|| Failure::Read("remote files home is unavailable".into()))?;
    let bytes = request_bytes(&request)
        .saturating_add(root.as_os_str().len())
        .saturating_add(256);
    command::submit_classified(
        core.file_command_queue().as_ref(),
        bytes,
        move || execute(root, request),
        output_bytes,
        |result| command::CommandDisposition {
            outcome: match result {
                Ok(_) => command::CommandOutcome::KnownSuccess,
                Err(error) if error.unknown() => command::CommandOutcome::Unknown,
                Err(_) => command::CommandOutcome::KnownFailure,
            },
            partial: result.as_ref().err().is_some_and(Failure::partial),
        },
    )
    .map_err(Failure::Transport)
}
fn key_bytes(command: &PasskeyCommand) -> usize {
    match command {
        PasskeyCommand::Path { name, path } => name.len().saturating_add(path.len()),
        PasskeyCommand::Inline { name, secret } => name.len().saturating_add(secret.len()),
        PasskeyCommand::Remove { name } => name.len(),
    }
}
fn patch_bytes(patch: &ProfilePatch) -> usize {
    command::value_bytes(&patch.fields)
        .saturating_add(command::value_bytes(&patch.label))
        .saturating_add(command::value_bytes(&patch.passkey_ref))
        .saturating_add(command::value_bytes(&patch.kind))
        .saturating_add(patch.expected.as_ref().map_or(0, command::value_bytes))
}
fn request_bytes(request: &Request) -> usize {
    match request {
        Request::Profile {
            command: profile,
            linked,
            ..
        } => {
            let bytes = match profile.as_ref() {
                ProfileCommand::Put(value) | ProfileCommand::Insert(value) => {
                    command::value_bytes(value)
                }
                ProfileCommand::Remove { name } => name.len(),
                ProfileCommand::Patch { name, patch } => {
                    name.len().saturating_add(patch_bytes(patch))
                }
                ProfileCommand::Rename {
                    original,
                    name,
                    patch,
                } => original
                    .len()
                    .saturating_add(name.len())
                    .saturating_add(patch_bytes(patch)),
            };
            bytes.saturating_add(linked.as_ref().map_or(0, key_bytes))
        }
        Request::Passkey(value) => key_bytes(value),
        #[cfg(feature = "gui")]
        Request::InsertPasskey(value) => key_bytes(value),
        #[cfg(feature = "gui")]
        Request::RenamePasskey {
            original,
            replacement,
        } => original.len().saturating_add(key_bytes(replacement)),
        #[cfg(feature = "gui")]
        Request::Reveal(key) => command::value_bytes(key),
    }
}
fn key_output_bytes(output: &PasskeyOutcome) -> usize {
    let path = output
        .secret_written
        .as_ref()
        .map_or(0, |path| path.as_os_str().len());
    path.saturating_add(128)
}
fn output_bytes(result: &Result<Output, Failure>) -> usize {
    match result {
        Ok(output) => output
            .root
            .as_os_str()
            .len()
            .saturating_add(match &output.value {
                #[cfg(feature = "gui")]
                Value::Revealed(value) => value.len(),
                Value::Passkey(value) => key_output_bytes(value),
                Value::Profile(value) => value
                    .profile
                    .as_ref()
                    .map_or(0, command::value_bytes)
                    .saturating_add(value.passkey.as_ref().map_or(0, key_output_bytes))
                    .saturating_add(128),
            })
            .saturating_add(128),
        Err(error) => error.message().len().saturating_add(256),
    }
}
fn key_result(result: PasskeyOutcome) -> Result<Value, Failure> {
    if let ManagedCleanup::Failed(message) = &result.cleanup {
        return Err(Failure::Cleanup {
            message: message.clone(),
            removed: result.removed,
            secret_written: result.secret_written.is_some(),
            profile_saved: false,
        });
    }
    Ok(Value::Passkey(result))
}
fn execute(root: PathBuf, request: Request) -> Result<Output, Failure> {
    let store = RemoteFileStore::at(root.clone());
    let (value, detect) = match request {
        Request::Profile {
            command,
            linked,
            detect,
        } => {
            let removing = matches!(command.as_ref(), ProfileCommand::Remove { .. });
            let mut value = store.profile(*command, linked).map_err(Failure::Mutation)?;
            if removing && !value.removed {
                return Err(Failure::Missing("remote profile not found".into()));
            }
            if let Some(key) = &value.passkey
                && let ManagedCleanup::Failed(message) = &key.cleanup
            {
                return Err(Failure::Cleanup {
                    message: message.clone(),
                    removed: key.removed,
                    secret_written: key.secret_written.is_some(),
                    profile_saved: true,
                });
            }
            super::status::clear_explicit(&store, &mut value)?;
            (Value::Profile(value), detect)
        }
        Request::Passkey(command) => {
            let removing = matches!(&command, PasskeyCommand::Remove { .. });
            let value = store.passkey(command).map_err(Failure::Mutation)?;
            if removing && !value.removed {
                return Err(Failure::Missing("passkey not found".into()));
            }
            (key_result(value)?, false)
        }
        #[cfg(feature = "gui")]
        Request::InsertPasskey(command) => (
            key_result(store.insert_passkey(command).map_err(Failure::Mutation)?)?,
            false,
        ),
        #[cfg(feature = "gui")]
        Request::RenamePasskey {
            original,
            replacement,
        } => (
            key_result(
                store
                    .rename_passkey(original, replacement)
                    .map_err(Failure::Mutation)?,
            )?,
            false,
        ),
        #[cfg(feature = "gui")]
        Request::Reveal(expected) => {
            let keys: Passkeys = detection::read(&root, "passkeys.toml")
                .map_err(|error| Failure::Read(error.message))?;
            if keys.get(&expected.name) != Some(&expected) {
                return Err(Failure::Invalid(
                    "passkey changed before local reveal".into(),
                ));
            }
            let value = if expected.kind == "inline" {
                std::fs::read_to_string(&expected.path).map_err(|error| {
                    Failure::Read(format!("could not read credential file: {}", error.kind()))
                })?
            } else {
                expected.path
            };
            (Value::Revealed(value), false)
        }
    };
    Ok(Output {
        root,
        value,
        detect,
    })
}

fn validate(request: &Request) -> Result<(), Failure> {
    let key = match request {
        Request::Profile { linked, .. } => linked.as_ref(),
        Request::Passkey(key) => Some(key),
        #[cfg(feature = "gui")]
        Request::InsertPasskey(key) => Some(key),
        #[cfg(feature = "gui")]
        Request::RenamePasskey { replacement, .. } => Some(replacement),
        #[cfg(feature = "gui")]
        Request::Reveal(_) => None,
    };
    if let Some(key) = key {
        let name = match key {
            PasskeyCommand::Path { name, .. }
            | PasskeyCommand::Inline { name, .. }
            | PasskeyCommand::Remove { name } => name,
        };
        if !tasty_remote_profiles::is_valid_passkey_name(name) {
            return Err(Failure::Invalid(
                "invalid passkey name (letters, digits, - and _ only)".into(),
            ));
        }
    }
    Ok(())
}
