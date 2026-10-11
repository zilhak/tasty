//! Metadata-only preparation for an ordinary managed Codex relay.
mod prompt;
mod store;
#[cfg(test)]
mod tests;

use anyhow::{Context, Result, bail};
use serde_json::{Value, json};
use std::path::Path;
use tasty_plugin_agent_common::host_call::HostCall;
use tasty_plugin_sdk::{HostHandle, IpcMethodError, PluginEnv};

pub fn handle(host: &HostHandle, params: &Value) -> Result<Value, IpcMethodError> {
    let result = (|| -> Result<Value> {
        let root = PluginEnv::load()?
            .data_dir
            .context("managed_data_dir_unavailable")?;
        execute(host, &root, params)
    })();
    result.map_err(|e| IpcMethodError::new(format!("managed_relay: {e:#}")))
}

fn execute<H: HostCall>(host: &H, root: &Path, params: &Value) -> Result<Value> {
    let action = text(params, "action")?;
    if action == "prepare" {
        return prepare(host, root, params);
    }
    let id = text(params, "id")?;
    if action == "forget" {
        std::fs::remove_file(store::path(root, id)?)?;
        return Ok(
            json!({"relay_id":id,"forgotten":true,"task_cancelled":false,"thread_archived":false}),
        );
    }
    let mut record = store::read(root, id)?;
    if instance(host)? != record.instance_id {
        bail!("relay_instance_changed: prepare a new relay");
    }
    match action {
        "get" => prompt::descriptor(&record),
        "attach" => {
            let thread = text(params, "thread_id")?;
            require_uuid(thread)?;
            if thread.eq_ignore_ascii_case(&record.parent_thread) {
                bail!("relay_must_differ_from_parent");
            }
            if let Some(existing) = &record.relay_thread {
                if !existing.eq_ignore_ascii_case(thread) {
                    bail!("relay_already_attached");
                }
            } else {
                record.relay_thread = Some(thread.to_ascii_lowercase());
                store::write(root, &record, false)?;
            }
            prompt::descriptor(&record)
        }
        "callback" => {
            if record.relay_thread.is_none() {
                bail!("relay_not_attached");
            }
            Ok(
                json!({"relay_id":record.relay_id,"phase":"attached","sent":false,
                "task_verified":false,"callback":prompt::callback(&record)?}),
            )
        }
        _ => bail!("unknown_managed_action"),
    }
}

fn prepare<H: HostCall>(host: &H, root: &Path, params: &Value) -> Result<Value> {
    let parent = text(params, "parent_thread")?;
    require_uuid(parent)?;
    if text(params, "expected_auth_mode")? != "local" {
        bail!("unsupported_auth_mode");
    }
    let workspace = params["workspace_id"]
        .as_u64()
        .and_then(|n| u32::try_from(n).ok())
        .context("invalid_workspace_id")?;
    let task = text(params, "task_id")?;
    if task.len() > 128 || task.chars().any(char::is_control) {
        bail!("invalid_task_id");
    }
    let cli = text(params, "cli_path")?;
    let cli_home = text(params, "cli_home")?;
    if !Path::new(cli_home).is_absolute() || !Path::new(cli_home).is_dir() {
        bail!("managed_cli_home_invalid");
    }
    let port = text(params, "port_file")?;
    for path in [cli, port] {
        if !Path::new(path).is_absolute() || !Path::new(path).is_file() {
            bail!("managed_path_must_be_absolute_file");
        }
    }
    let record = store::Record {
        relay_id: store::new_id()?,
        parent_thread: parent.to_ascii_lowercase(),
        workspace_id: workspace,
        task_id: task.into(),
        cli_path: cli.into(),
        cli_home: cli_home.into(),
        port_file: port.into(),
        instance_id: instance(host)?,
        relay_thread: None,
    };
    let args = prompt::create(&record)?;
    // Check both tool byte budgets before persisting an unusable descriptor.
    prompt::callback(&record)?;
    store::write(root, &record, true)?;
    Ok(
        json!({"relay_id":record.relay_id,"phase":"prepared","task_verified":false,
        "create_thread_args":args,"next":"Call codex_tui.create_thread with create_thread_args, then attach its returned threadId."}),
    )
}

fn instance<H: HostCall>(host: &H) -> Result<String> {
    let info = host.call("system.info", json!({}))?;
    Ok(text(&info, "instance_id")?.into())
}
fn text<'a>(params: &'a Value, key: &str) -> Result<&'a str> {
    params[key]
        .as_str()
        .filter(|v| !v.is_empty())
        .with_context(|| format!("missing_{key}"))
}
fn require_uuid(value: &str) -> Result<()> {
    if value.len() != 36
        || !value.bytes().enumerate().all(|(i, b)| {
            if [8, 13, 18, 23].contains(&i) {
                b == b'-'
            } else {
                b.is_ascii_hexdigit()
            }
        })
    {
        bail!("invalid_managed_thread_id");
    }
    Ok(())
}
