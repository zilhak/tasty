//! Per-spawn Claude invocation options, kept with the session for hook-driven restoration.

use std::path::{Path, PathBuf};

use serde_json::{Value, json};
use tasty_plugin_agent_common::host_call::HostCall;
use tasty_plugin_sdk::{IpcMethodError, i18n::Translator};

use crate::hook::{HostCall as PlannedCall, RESTORE_COMMAND_META_KEY};

pub(crate) const META_KEY: &str = "claude-session-options";
/// The CLI help omits ultracode; the official CLI reference documents it since 2.1.203.
const EFFORTS: &[&str] = &["low", "medium", "high", "xhigh", "max", "ultracode"];

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct SessionOptions {
    pub model: Option<String>,
    pub effort: Option<String>,
}

impl SessionOptions {
    /// A supplied value must be a single non-option token. Model availability belongs to the
    /// provider, so aliases, full IDs, deployment names and inference-profile ARNs are accepted.
    pub fn parse(params: &Value, tr: &Translator) -> Result<Self, IpcMethodError> {
        let read = |key: &str| -> Result<Option<String>, IpcMethodError> {
            let Some(value) = params.get(key) else {
                return Ok(None);
            };
            let Some(s) = value.as_str().filter(|s| {
                !s.is_empty()
                    && !s.starts_with('-')
                    && !s.chars().any(|c| c.is_whitespace() || c.is_control())
            }) else {
                return Err(IpcMethodError::invalid_params(
                    &tr.t_fmt("claude.params.invalid_session_option", key),
                ));
            };
            if key == "effort" && !EFFORTS.contains(&s) {
                return Err(IpcMethodError::invalid_params(
                    &tr.t_fmt("claude.params.invalid_effort", &EFFORTS.join(", ")),
                ));
            }
            Ok(Some(s.to_string()))
        };
        Ok(Self {
            model: read("model")?,
            effort: read("effort")?,
        })
    }

    pub fn json(&self) -> Value {
        let mut out = json!({});
        if let Some(model) = &self.model {
            out["model"] = json!(model);
        }
        if let Some(effort) = &self.effort {
            out["effort"] = json!(effort);
        }
        out
    }

    /// Values are shell data, including provider IDs with punctuation or quotes.
    pub fn flags(&self) -> String {
        let mut out = String::new();
        for (key, value) in [("model", &self.model), ("effort", &self.effort)] {
            if let Some(value) = value {
                out.push_str(&format!(" --{key} {}", shell_escape::escape(value.into())));
            }
        }
        out
    }

    /// Store even an empty object: a fresh respawn must not inherit a previous session's options.
    pub fn record<H: HostCall>(&self, host: &H, surface_id: u32) -> Result<(), IpcMethodError> {
        host.call(
            "surface.meta.set",
            json!({
                "surface_id": surface_id, "key": META_KEY, "value": self.json().to_string(),
            }),
        )
        .map_err(IpcMethodError::from)?;
        Ok(())
    }
}

fn record_path(data_dir: Option<&Path>, session: &str) -> Option<PathBuf> {
    if !tasty_plugin_agent_common::reboot::is_safe_session_id(session) {
        return None;
    }
    Some(
        data_dir?
            .join("session-options")
            .join(format!("{session}.json")),
    )
}

/// Surface meta is authoritative while launching; the session file survives closed-tab restore.
/// Corrupt records are errors rather than silently dropping an explicit selection.
pub(crate) fn load<H: HostCall>(
    host: &H,
    surface: u32,
    data_dir: Option<&Path>,
    session: &str,
    tr: &Translator,
) -> Result<SessionOptions, IpcMethodError> {
    let meta = host
        .call(
            "surface.meta.get",
            json!({ "surface_id": surface, "key": META_KEY }),
        )
        .map_err(IpcMethodError::from)?;
    let text = if let Some(value) = meta.get("value").filter(|v| !v.is_null()) {
        Some(
            value
                .as_str()
                .ok_or_else(|| {
                    IpcMethodError::invalid_params(
                        &tr.t_fmt("claude.params.invalid_session_option", META_KEY),
                    )
                })?
                .to_string(),
        )
    } else if let Some(path) = record_path(data_dir, session) {
        match std::fs::read_to_string(path) {
            Ok(text) => Some(text),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(e) => return Err(IpcMethodError::new(e.to_string())),
        }
    } else {
        None
    };
    let Some(text) = text else {
        return Ok(SessionOptions::default());
    };
    let value: Value =
        serde_json::from_str(&text).map_err(|e| IpcMethodError::new(e.to_string()))?;
    if !value.is_object() {
        return Err(IpcMethodError::invalid_params(
            &tr.t_fmt("claude.params.invalid_session_option", META_KEY),
        ));
    }
    SessionOptions::parse(&value, tr)
}

pub(crate) fn store(data_dir: Option<&Path>, session: &str, options: &SessionOptions) {
    let Some(path) = record_path(data_dir, session) else {
        return;
    };
    let result = path
        .parent()
        .map_or(Ok(()), std::fs::create_dir_all)
        .and_then(|()| std::fs::write(&path, options.json().to_string()));
    if let Err(e) = result {
        tracing::warn!(
            "claude session options: cannot save {}: {e}",
            path.display()
        );
    }
}

#[cfg(test)]
mod tests;

/// Keep the established double-quoted settings path syntax while escaping shell expansion.
pub(crate) fn quote_path(path: &str) -> String {
    let mut out = String::from("\"");
    for c in path.chars() {
        if matches!(c, '\\' | '"' | '$' | '`') {
            out.push('\\');
        }
        out.push(c);
    }
    out.push('"');
    out
}

/// Refresh the restore command after profile resolution, preserving invocation-only selections.
pub(crate) fn session_start<H: HostCall>(
    calls: &mut Vec<PlannedCall>,
    host: &H,
    surface_id: u32,
    data_dir: Option<&Path>,
    session_id: &str,
    tr: &Translator,
) {
    match load(host, surface_id, data_dir, session_id, tr) {
        Ok(options) => {
            for call in calls.iter_mut() {
                if let PlannedCall::MetaSet { key, value, .. } = call
                    && *key == RESTORE_COMMAND_META_KEY
                {
                    value.push_str(&options.flags());
                }
            }
            calls.push(PlannedCall::MetaSet {
                surface_id,
                key: META_KEY,
                value: options.json().to_string(),
            });
            store(data_dir, session_id, &options);
        }
        Err(e) => {
            // Do not replace a previously explicit restoration with an unqualified command.
            calls.retain(|call| {
                !matches!(call, PlannedCall::MetaSet { key, .. }
                    if *key == RESTORE_COMMAND_META_KEY)
            });
            tracing::warn!(
                "claude session-start: cannot restore session options: {}",
                e.message
            );
        }
    }
}
