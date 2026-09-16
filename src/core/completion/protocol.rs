//! App Server session validation and evidence-based recovery.
use super::{Binding, Event, transport::Transport};
use anyhow::{Context, Result, bail};
use serde_json::{Value, json};
use std::time::{Duration, Instant};

pub struct Identity {
    pub server: String,
    pub home: String,
    pub pid: u64,
    pub history: String,
}
pub struct Client {
    pub transport: Transport,
    next: u64,
    pub observed: Vec<Value>,
}
impl Client {
    pub fn connect(binding: &Binding) -> Result<(Self, Identity)> {
        let identity = super::transport::endpoint_identity(&binding.endpoint)?;
        if !binding.endpoint_identity.is_empty() && binding.endpoint_identity != identity {
            bail!("endpoint_replaced: explicit rebind required");
        }
        let mut client = Self {
            transport: Transport::connect(&binding.endpoint, binding.auth_env.as_deref())?,
            next: 0,
            observed: Vec::new(),
        };
        let hello = client.call("initialize", json!({"clientInfo":{"name":"tasty_completion","version":env!("CARGO_PKG_VERSION")},"capabilities":{"experimentalApi":true}}))?;
        let server = hello["userAgent"].as_str().unwrap_or("unknown");
        let version = server
            .split('/')
            .nth(1)
            .and_then(|v| v.split_whitespace().next())
            .unwrap_or("");
        if version != "0.154.0" {
            bail!("unsupported_server_version: {version}; verified protocol baseline is 0.154.0");
        }
        client.transport.send(&json!({"method":"initialized"}))?;
        let home = hello["codexHome"].as_str().unwrap_or("").to_string();
        if !binding.codex_home.is_empty() && binding.codex_home != home {
            bail!("daemon_home_changed: explicit rebind required");
        }
        if binding
            .expected_home
            .as_deref()
            .is_some_and(|expected| expected != home)
        {
            bail!("codex_home_mismatch: actual={home}");
        }
        let diagnostics = client.call("server/diagnostics", json!({}))?;
        let pid = diagnostics["process"]["id"]
            .as_u64()
            .context("daemon_process_identity_unavailable")?;
        if binding.daemon_pid.is_some_and(|previous| previous != pid) {
            bail!("daemon_replaced: rebind after verifying TUI ownership");
        }
        let mut cursor = Value::Null;
        let mut loaded = false;
        for _ in 0..100 {
            let page = client.call("thread/loaded/list", json!({"cursor":cursor,"limit":100}))?;
            loaded |= page["data"]
                .as_array()
                .is_some_and(|a| a.iter().any(|v| v.as_str() == Some(&binding.thread_id)));
            cursor = page["nextCursor"].clone();
            if cursor.is_null() {
                break;
            }
        }
        if !loaded {
            bail!("thread_not_loaded_on_endpoint: refusing to resume a disk copy");
        }
        let read = client.call(
            "thread/read",
            json!({"threadId":binding.thread_id,"includeTurns":false}),
        )?;
        verify_thread(&read["thread"], binding)?;
        // Resume only an already loaded matching thread, without sticky setting overrides.
        let resumed = client.call(
            "thread/resume",
            json!({"threadId":binding.thread_id,"excludeTurns":true}),
        )?;
        verify_thread(&resumed["thread"], binding)?;
        Ok((
            client,
            Identity {
                server: hello["userAgent"].as_str().unwrap_or("unknown").into(),
                home,
                pid,
                history: read["thread"]["historyMode"]
                    .as_str()
                    .unwrap_or("legacy")
                    .into(),
            },
        ))
    }
    pub fn call(&mut self, method: &str, params: Value) -> Result<Value> {
        self.next += 1;
        let id = self.next;
        self.transport
            .send(&json!({"id":id,"method":method,"params":params}))?;
        self.response(id)
    }
    pub fn response(&mut self, id: u64) -> Result<Value> {
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline {
            let value = self.transport.receive()?;
            if value["id"].as_u64() == Some(id) {
                if let Some(error) = value.get("error") {
                    bail!("rpc_error: {}", error);
                }
                return value.get("result").cloned().context("missing_rpc_result");
            }
            // Server approval/tool requests remain owned by the parent TUI. Never answer them.
            if value.get("method").is_some()
                && value.get("id").is_none()
                && self.observed.len() < 512
            {
                self.observed.push(value);
            }
        }
        bail!("app_server_response_timeout")
    }
    pub fn prepare_output(&mut self, binding: &Binding, event: &Event) -> (u64, Value) {
        self.next += 1;
        (
            self.next,
            json!({"id":self.next,"method":"turn/start","params":{"threadId":binding.thread_id,"input":[],"toolOutput":{"name":"child_completion","namespace":"tasty","output":serde_json::to_string(&json!({"event_id":event.delivery_id.clone(),"subscription":event.subscription,"child":event.child,"child_kind":event.child_kind,"child_generation":event.child_generation,"epoch":event.epoch,"state":event.state,"cause":event.cause,"summary":event.summary,"result_reference":{"kind":"tasty_surface","surface":event.child}})).expect("JSON values serialize")}}}),
        )
    }
}

fn verify_thread(thread: &Value, binding: &Binding) -> Result<()> {
    let runtime = thread["status"]["type"]
        .as_str()
        .context("thread_runtime_status_missing")?;
    if runtime == "notLoaded" {
        bail!("thread_not_loaded_on_endpoint: refusing disk-history resume");
    }
    // The supported 0.154.0 TUI root/fork hook reports the live thread id.
    // Keep the fields distinct and reject other mappings instead of guessing a session-tree member.
    if binding.hook_session != binding.thread_id {
        bail!("unsupported_hook_thread_mapping: current parent hook does not identify this thread");
    }

    if thread["id"].as_str() != Some(&binding.thread_id)
        || thread["sessionId"].as_str() != Some(&binding.session_id)
    {
        bail!(
            "thread_identity_mismatch: actual thread.id={} sessionId={}",
            thread["id"],
            thread["sessionId"]
        );
    }
    Ok(())
}
