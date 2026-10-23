//! Owned command boundary shared by standalone CLI and host file workers.
use crate::registry::{Registry, managed_path, table_of, typed};
use crate::{FieldValue, Passkey, RemoteProfile, is_valid_passkey_name};
use anyhow::{Context, Result};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};
use toml_edit::{Item, value};

#[derive(Clone, Default, Debug)]
pub struct ProfilePatch {
    pub kind: Option<String>,
    pub fields: BTreeMap<String, Option<FieldValue>>,
    pub label: Option<Option<String>>,
    pub passkey_ref: Option<Option<String>>,
    /// A delayed detection must not restore an edited or removed profile.
    pub expected: Option<RemoteProfile>,
}
#[derive(Clone, Debug)]
pub enum ProfileCommand {
    /// Merge supplied fields, preserving fields absent from this command.
    Put(RemoteProfile),
    Insert(RemoteProfile),
    Patch {
        name: String,
        patch: ProfilePatch,
    },
    Remove {
        name: String,
    },
    Rename {
        original: String,
        name: String,
        patch: ProfilePatch,
    },
}
// Deliberately no Debug: Inline carries a secret.
pub enum PasskeyCommand {
    Path { name: String, path: String },
    Inline { name: String, secret: String },
    Remove { name: String },
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ManagedCleanup {
    NotManaged,
    Removed,
    Missing,
    Failed(String),
}
#[derive(Debug)]
pub struct PasskeyOutcome {
    pub replaced: bool,
    pub removed: bool,
    pub secret_written: Option<PathBuf>,
    pub cleanup: ManagedCleanup,
}
#[derive(Debug)]
pub struct ProfileOutcome {
    pub replaced: bool,
    pub removed: bool,
    pub profile: Option<RemoteProfile>,
    pub passkey: Option<PasskeyOutcome>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MutationStage {
    ProfileRead,
    PasskeyRead,
    SecretWrite,
    PasskeySave,
    ProfileSave,
}
/// Stable command errors for IPC not-found/name-conflict mapping without parsing messages.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MutationReason {
    AlreadyExists,
    NotFound,
    Changed,
    Storage,
}
impl std::fmt::Display for MutationReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::AlreadyExists => "registry entry already exists",
            Self::NotFound => "registry entry not found",
            Self::Changed => "profile changed during detection",
            Self::Storage => "registry storage failed",
        })
    }
}
impl std::error::Error for MutationReason {}
#[derive(Debug)]
pub struct MutationFailure {
    pub reason: MutationReason,
    pub stage: MutationStage,
    pub secret_written: Option<PathBuf>,
    pub passkeys_saved: bool,
    source: anyhow::Error,
}
impl std::fmt::Display for MutationFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{:?}: {} (secret_written={}, passkeys_saved={}; profile reference not committed)",
            self.stage,
            self.source,
            self.secret_written.is_some(),
            self.passkeys_saved
        )
    }
}
impl std::error::Error for MutationFailure {}
impl MutationFailure {
    fn new(stage: MutationStage, source: anyhow::Error) -> Self {
        Self {
            reason: source
                .downcast_ref::<MutationReason>()
                .copied()
                .unwrap_or(MutationReason::Storage),
            stage,
            source,
            secret_written: None,
            passkeys_saved: false,
        }
    }
}
#[derive(Clone, Debug)]
pub struct RemoteFileStore {
    root: PathBuf,
}
impl RemoteFileStore {
    pub fn at(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }
    pub fn for_home() -> Result<Self> {
        Ok(Self::at(
            tasty_utils::path::tasty_home().context("could not determine remote files home")?,
        ))
    }
    /// All guards are dropped before returning. Fixed compound lock order: profile then passkey.
    pub fn profile(
        &self,
        command: ProfileCommand,
        linked: Option<PasskeyCommand>,
    ) -> std::result::Result<ProfileOutcome, MutationFailure> {
        let mut profiles = Registry::open(self.root.join("remote-profiles.toml"))
            .map_err(|e| MutationFailure::new(MutationStage::ProfileRead, e))?;
        let mut outcome = apply_profile(&mut profiles, command)
            .map_err(|e| MutationFailure::new(MutationStage::ProfileRead, e))?;
        if matches!(
            &outcome,
            ProfileOutcome {
                replaced: false,
                profile: None,
                ..
            }
        ) {
            return Ok(outcome);
        }
        // Prepare and validate the profile delta before any secret/list side effect.
        let mut linked_registry = None;
        if let Some(command) = linked {
            if matches!(command, PasskeyCommand::Remove { .. }) {
                return Err(MutationFailure::new(
                    MutationStage::PasskeyRead,
                    anyhow::anyhow!("linked removal is not supported"),
                ));
            }
            let key_name = match &command {
                PasskeyCommand::Path { name, .. } | PasskeyCommand::Inline { name, .. } => name,
                PasskeyCommand::Remove { .. } => unreachable!(),
            };
            if outcome
                .profile
                .as_ref()
                .and_then(|p| p.passkey_ref.as_ref())
                != Some(key_name)
            {
                return Err(MutationFailure::new(
                    MutationStage::PasskeyRead,
                    anyhow::anyhow!("linked passkey does not match profile reference"),
                ));
            }
            profiles
                .reject_alias(&self.root.join("passkeys.toml"))
                .map_err(|e| MutationFailure::new(MutationStage::PasskeyRead, e))?;
            if let PasskeyCommand::Inline { name, .. } = &command {
                profiles
                    .reject_alias(&self.root.join("passkeys").join(name))
                    .map_err(|e| MutationFailure::new(MutationStage::PasskeyRead, e))?;
            }
            let keys = Registry::open(self.root.join("passkeys.toml"))
                .map_err(|e| MutationFailure::new(MutationStage::PasskeyRead, e))?;
            linked_registry = Some(keys);
            outcome.passkey = Some(self.apply_passkey(
                linked_registry.as_mut().expect("linked registry opened"),
                command,
            )?);
        }
        if let Err(e) = profiles.commit() {
            let mut error = MutationFailure::new(MutationStage::ProfileSave, e);
            if let Some(key) = &outcome.passkey {
                error.secret_written = key.secret_written.clone();
                error.passkeys_saved = true;
            }
            return Err(error);
        }
        drop(linked_registry);
        Ok(outcome)
    }
    pub fn passkey(
        &self,
        command: PasskeyCommand,
    ) -> std::result::Result<PasskeyOutcome, MutationFailure> {
        let mut keys = Registry::open(self.root.join("passkeys.toml"))
            .map_err(|e| MutationFailure::new(MutationStage::PasskeyRead, e))?;
        self.apply_passkey(&mut keys, command)
    }
    /// Rename in one list replacement. The new inline file precedes list commit;
    /// old managed-file cleanup follows it. Profile references remain name-based.
    pub fn rename_passkey(
        &self,
        original: String,
        replacement: PasskeyCommand,
    ) -> std::result::Result<PasskeyOutcome, MutationFailure> {
        let mut keys = Registry::open(self.root.join("passkeys.toml"))
            .map_err(|e| MutationFailure::new(MutationStage::PasskeyRead, e))?;
        let result = prepare_key_rename(&mut keys, &original, &replacement)
            .map_err(|e| MutationFailure::new(MutationStage::PasskeyRead, e))?;
        let mut outcome = self.apply_passkey(&mut keys, replacement)?;
        if let Some(old) = result {
            outcome.cleanup = self.cleanup(&old, &keys);
        }
        Ok(outcome)
    }
    fn apply_passkey(
        &self,
        keys: &mut Registry,
        command: PasskeyCommand,
    ) -> std::result::Result<PasskeyOutcome, MutationFailure> {
        let name = match &command {
            PasskeyCommand::Path { name, .. }
            | PasskeyCommand::Inline { name, .. }
            | PasskeyCommand::Remove { name } => name.clone(),
        };
        if !is_valid_passkey_name(&name) {
            return Err(MutationFailure::new(
                MutationStage::PasskeyRead,
                anyhow::anyhow!("invalid passkey name"),
            ));
        }
        if matches!(&command, PasskeyCommand::Inline { .. }) {
            keys.reject_alias(&self.root.join("passkeys").join(&name))
                .map_err(|e| MutationFailure::new(MutationStage::PasskeyRead, e))?;
        }
        let idx = keys
            .find("passkey", &name)
            .map_err(|e| MutationFailure::new(MutationStage::PasskeyRead, e))?;
        let mut outcome = PasskeyOutcome {
            replaced: idx.is_some(),
            removed: false,
            secret_written: None,
            cleanup: ManagedCleanup::NotManaged,
        };
        if matches!(&command, PasskeyCommand::Remove { .. }) && idx.is_none() {
            return Ok(outcome);
        }
        let mut cleanup = None;
        let preparation_stage = if matches!(&command, PasskeyCommand::Remove { .. }) {
            MutationStage::PasskeyRead
        } else {
            MutationStage::SecretWrite
        };
        let prepared = (|| -> Result<()> {
            let next = match command {
                PasskeyCommand::Path { name, path } => Some(Passkey {
                    name,
                    path,
                    kind: "path".into(),
                }),
                PasskeyCommand::Inline { name, secret } => {
                    let path = crate::passkey::materialize_inline_in(
                        &self.root.join("passkeys"),
                        &name,
                        &secret,
                    )?;
                    outcome.secret_written = Some(path.clone());
                    Some(Passkey {
                        name,
                        kind: "inline".into(),
                        path: path.to_string_lossy().into_owned(),
                    })
                }
                PasskeyCommand::Remove { .. } => {
                    if let Some(i) = idx {
                        let old: Passkey =
                            typed(keys.entries("passkey")?.get(i).expect("located entry"))?;
                        if old.is_managed() {
                            cleanup = Some(old);
                        }
                        keys.entries("passkey")?.remove(i);
                        outcome.removed = true;
                    }
                    None
                }
            };
            if let Some(next) = next {
                let table = table_of(&next)?;
                let entries = keys.entries("passkey")?;
                if let Some(i) = idx {
                    for (k, v) in table.iter() {
                        entries
                            .get_mut(i)
                            .expect("located entry")
                            .insert(k, v.clone());
                    }
                } else {
                    entries.push(table);
                }
            }
            Ok(())
        })();
        prepared.map_err(|e| {
            let mut error = MutationFailure::new(preparation_stage, e);
            error.secret_written = outcome.secret_written.clone();
            error
        })?;
        if let Err(e) = keys.commit() {
            let mut failure = MutationFailure::new(MutationStage::PasskeySave, e);
            failure.secret_written = outcome.secret_written;
            return Err(failure);
        }
        if let Some(old) = cleanup {
            outcome.cleanup = self.cleanup(&old, keys);
        }
        Ok(outcome)
    }
    fn cleanup(&self, old: &Passkey, registry: &Registry) -> ManagedCleanup {
        let path = Path::new(&old.path);
        if let Err(e) = registry.reject_alias(path) {
            return ManagedCleanup::Failed(e.to_string());
        }
        match fs::symlink_metadata(path) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return ManagedCleanup::Missing,
            Err(e) => return ManagedCleanup::Failed(e.to_string()),
            Ok(_) => {}
        }
        if !managed_path(&self.root.join("passkeys"), &old.name, path) {
            return ManagedCleanup::Failed(
                "managed path ownership check failed; file preserved".into(),
            );
        }
        let _guard = match tasty_utils::file_replace::lock(path) {
            Ok(guard) => guard,
            Err(e) => return ManagedCleanup::Failed(e.to_string()),
        };
        if !managed_path(&self.root.join("passkeys"), &old.name, path) {
            return ManagedCleanup::Failed(
                "managed path changed during cleanup; file preserved".into(),
            );
        }
        match fs::remove_file(path) {
            Ok(()) => ManagedCleanup::Removed,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => ManagedCleanup::Missing,
            Err(e) => ManagedCleanup::Failed(e.to_string()),
        }
    }
}
fn apply_profile(registry: &mut Registry, command: ProfileCommand) -> Result<ProfileOutcome> {
    let name = match &command {
        ProfileCommand::Put(p) | ProfileCommand::Insert(p) => &p.name,
        ProfileCommand::Patch { name, .. } | ProfileCommand::Remove { name } => name,
        ProfileCommand::Rename { original, .. } => original,
    };
    let idx = registry.find("profile", name)?;
    let mut outcome = ProfileOutcome {
        replaced: idx.is_some(),
        removed: false,
        profile: None,
        passkey: None,
    };
    match command {
        ProfileCommand::Remove { .. } => {
            if let Some(i) = idx {
                registry.entries("profile")?.remove(i);
                outcome.removed = true;
            }
        }
        ProfileCommand::Patch { name: _, patch } => {
            let i = idx.ok_or(MutationReason::NotFound)?;
            let table = registry
                .entries("profile")?
                .get_mut(i)
                .expect("located entry");
            patch_entry(table, patch)?;
            outcome.profile = projection(table);
        }
        ProfileCommand::Rename {
            original,
            name,
            patch,
        } => {
            if original != name && registry.find("profile", &name)?.is_some() {
                return Err(MutationReason::AlreadyExists.into());
            }
            let i = idx.ok_or(MutationReason::NotFound)?;
            let table = registry
                .entries("profile")?
                .get_mut(i)
                .expect("located entry");
            patch_entry(table, patch)?;
            table["name"] = value(name);
            outcome.profile = projection(table);
        }
        command => {
            let (profile, insert) = match command {
                ProfileCommand::Put(p) => (p, false),
                ProfileCommand::Insert(p) => (p, true),
                _ => unreachable!(),
            };
            if insert && idx.is_some() {
                return Err(MutationReason::AlreadyExists.into());
            }
            let entries = registry.entries("profile")?;
            if let Some(i) = idx {
                entries.get_mut(i).expect("located entry")["kind"] = value(&profile.kind);
                apply_patch(
                    entries.get_mut(i).expect("located entry"),
                    profile
                        .fields
                        .iter()
                        .map(|(k, v)| (k.clone(), Some(v.clone())))
                        .collect(),
                    Some(profile.label.clone()),
                    Some(profile.passkey_ref.clone()),
                )?;
            } else {
                entries.push(table_of(&profile)?);
            }
            outcome.profile = entries
                .iter()
                .find(|t| t.get("name").and_then(Item::as_str) == Some(profile.name.as_str()))
                .and_then(projection);
        }
    }
    Ok(outcome)
}
fn patch_entry(table: &mut toml_edit::Table, patch: ProfilePatch) -> Result<()> {
    if let Some(expected) = patch.expected {
        let current = projection(table).context("invalid target profile")?;
        if current != expected {
            return Err(MutationReason::Changed.into());
        }
    }
    if let Some(kind) = patch.kind {
        table["kind"] = value(kind);
    }
    apply_patch(table, patch.fields, patch.label, patch.passkey_ref)
}
fn prepare_key_rename(
    keys: &mut Registry,
    original: &str,
    replacement: &PasskeyCommand,
) -> Result<Option<Passkey>> {
    let (name, next_path) = match replacement {
        PasskeyCommand::Path { name, path } => (name, Some(path.as_str())),
        PasskeyCommand::Inline { name, .. } => (name, None),
        PasskeyCommand::Remove { .. } => anyhow::bail!("rename requires a replacement credential"),
    };
    let idx = keys
        .find("passkey", original)?
        .ok_or(MutationReason::NotFound)?;
    if original != name && keys.find("passkey", name)?.is_some() {
        return Err(MutationReason::AlreadyExists.into());
    }
    let table = keys
        .entries("passkey")?
        .get_mut(idx)
        .expect("located entry");
    let old: Passkey = typed(table)?;
    table["name"] = value(name);
    // Same-name inline replaces the same managed file; a path reference can take ownership
    // of that file without deleting it. A different replacement leaves the old file to clean.
    let clean_old = old.is_managed()
        && match next_path {
            Some(path) => path != old.path,
            None => original != name,
        };
    Ok(clean_old.then_some(old))
}

fn apply_patch(
    table: &mut toml_edit::Table,
    fields: BTreeMap<String, Option<FieldValue>>,
    label: Option<Option<String>>,
    passkey: Option<Option<String>>,
) -> Result<()> {
    for (key, val) in [("label", label), ("passkey_ref", passkey)] {
        if let Some(val) = val {
            match val {
                Some(s) => {
                    table[key] = value(s);
                }
                None => {
                    table.remove(key);
                }
            }
        }
    }
    if fields.is_empty() {
        return Ok(());
    }
    if !table.contains_key("fields") {
        table["fields"] = Item::Table(toml_edit::Table::new());
    }
    let target = table["fields"]
        .as_table_like_mut()
        .context("profile fields must be a table")?;
    for (key, val) in fields {
        match val {
            Some(FieldValue::Str(s)) => {
                target.insert(&key, value(s));
            }
            Some(FieldValue::List(values)) => {
                target.insert(
                    &key,
                    value(values.into_iter().collect::<toml_edit::Array>()),
                );
            }
            None => {
                target.remove(&key);
            }
        }
    }
    Ok(())
}
fn projection(table: &toml_edit::Table) -> Option<RemoteProfile> {
    let mut raw: toml::Value =
        toml::from_str(&toml_edit::Document::from(table.clone()).to_string()).ok()?;
    // Unknown/invalid fields remain in the document, but cannot be represented in FieldValue.
    if let Some(fields) = raw.get_mut("fields").and_then(toml::Value::as_table_mut) {
        fields.retain(|_, v| {
            v.is_str()
                || v.as_array()
                    .is_some_and(|a| a.iter().all(toml::Value::is_str))
        });
    }
    raw.try_into().ok()
}
