<!-- source-hash: f5980ea6f8f6 -->
# IPC / CLI API reference

The whole IPC/CLI surface for operating tasty, listed by namespace. **The truth for methods and permissions is the code** — `crates/tasty-ipc/src/method_meta.rs` (`METHOD_TABLE` / `DEBUG_METHODS`) and the `src/adapters/ipc/handler.rs` router. This document is a map for humans; the *behaviour* of each method is delegated to its feature document.

## Connecting

```python
import socket, json, os
port = int(open(os.path.expanduser("~/.tasty/tasty.port")).read().strip())  # dynamic port
s = socket.socket(); s.connect(("127.0.0.1", port))
def call(method, params=None):
    s.sendall((json.dumps({"jsonrpc":"2.0","id":1,"method":method,"params":params or {}}) + "\n").encode())
    return json.loads(s.recv(1<<16).decode())
```

- Transport: loopback TCP, dynamic port (`~/.tasty/tasty.port`), line-delimited JSON-RPC 2.0. Rationale: [ADR-0004](../adr/0004-ipc-transport-tcp.md).
- Most CLI subcommands wrap the IPC method of the same name. **Every command is focus-independent** — targets are addressed directly by ID ([focus policy](../design/policies/focus.md)).
- **Focus independence**: the release surface has no focus-changing API. Replaying user input (key/mouse injection, forcing a popup open, `window.focus`) is debug-only → [debug-ipc](../dev-guide/debug-ipc.md).

## Permissions

Plugin callers need a per-method permission token (`method_meta`). Local callers (CLI / the user) are unrestricted. Token list and gate: [concepts/plugins](../concepts/plugins.md) · [dev-guide/plugin-permissions](../dev-guide/plugin-permissions.md). Runtime capability elevation and audit: [features/capability-elevation](../features/capability-elevation/index.md).

## Namespaces

### Structure — workspace / pane / tab / surface / split / tree
`workspace.{list,create,update,move}` · `pane.{list,close}` · `split` · `tab.{list,create,close,move}` · `tree`. The domain is [work-area](../features/work-area/index.md).

### Surface interaction
- Input: `surface.{send,send_key,send_combo,send_to,send_wait_idle,wake,respawn_terminal}`
- Read / mark: `surface.{set_mark,read_since_mark,parse_since_mark,screen_text,cursor_position,foreground_process,is_typing,locate}`. `screen_text` (and `pty.read`) exclude dim (ghost-suggestion, e.g. Claude Code autocomplete) cells by default — include them with `show_dim:true` (CLI `--show-dim`).
  - `lines:N` (CLI `--lines N`): omitted, the whole visible screen. When given, **the last N lines by content** — blank rows below the content are skipped, and if the screen content falls short of N the rest is filled from scrollback (as much as exists if still short). Blank lines *in the middle* of content are part of the output, so they are preserved and counted. Blank-row detection uses the same value as `show_dim`, so `--show-dim` does not change the number of lines returned. Even while an alternate screen (TUI) is up, the shortfall is filled from the primary scrollback — the alt screen has no scrollback of its own.
- Commands (OSC 133): `surface.{commands,last_command,command_at}`
- Meta: `surface.meta.{set,get,unset,list}` · `surface.set_cwd`
- Output observers: `output.observe_{start,stop,list,info}`
- Behaviour and parsers: [terminal-output](../features/terminal-output/index.md); parser catalogue [output-parsers](output-parsers.md). (IME `surface.ime_*` is local-only.)

### Memory (`memory.*` / `memory.secret.*`)
regular (`put/get/delete/list/exists/count/scopes/stats/query/export/import`) · secret (same verbs) · `gc` · blackboard (`bb_*`) · plan (`plan_*`) · cache (`cache_*`) · goal (`goal_*` — a single goal sentence scoped to a surface; `surface_id` is required). Model and permissions: [design/systems/memory](../design/systems/memory.md).

### Agent collaboration (`agent.*`)
`task_{create,list,get,cancel,retry,graph,reduce,run,delete,purge}` · `dag_{list,get}` · `barrier_*` · `semaphore_*` · `lease_*` · `rate_limit_*`. All under the `agent` (AgentManage) permission — including `task_run` (workspace runner thread start/stop/status; the host does not restart runners automatically on restart, so a plugin must be able to revive its own workspace's runner). **Local callers only** (plugin calls rejected): `task_await` (truly blocking — symmetric with `approval.await`, to keep the plugin SDK's single worker thread from stalling; default timeout 10 minutes, `timeout_ms:0` waits forever) and `task_set_result` (an external task-completion signal — the runner solely owns the Custom task lifecycle, so a plugin transitioning it separately would duplicate the writer; plugins work around this by declaring a completion strategy). Both are **explicitly registered** with `local_only()` in the `METHOD_TABLE` of [method_meta.rs](../../crates/tasty-ipc/src/method_meta.rs) — an unregistered method (an `UnknownMethod` rejection) cannot be told apart from an omission, so every method with a router branch is in the table (`tests/ipc_router_table_parity.rs` enforces it). `task_delete` / `task_purge` run a reference-safety check (`depends_on` / `Fallback.task` / `Reduce.inputs`) — rejected by default with the list of referrers; `--cascade` (cascading delete) / `--force` (skips only the reference check; cannot bypass the `running` state constraint). For `task_command.kind = "run"` (a bare subprocess with no Surface), the result carries a stdout/stderr capture (the last 64 KiB tail of each, plus `truncated` / `dropped_bytes`) in `result.output` of `task_get` / `task_await` — on failure (non-zero exit) the same content is included in the `result.error` string. `dag_{list,get}` are **a query surface that slices a workspace's flat tasks per DAG** — a DAG is not a persisted record but is derived from `metadata.dag` (explicit) or graph connectivity (derived). `dag_list` walks every live workspace when `workspace_id` is omitted (response `scope: "live_workspaces"`), and `dag_get` produces the same `nodes` / `edges` (or dot) as `task_graph`, restricted to that DAG's subset. [agent-collaboration](../features/agent-collaboration/index.md).

### Human handoff (`approval.*`)
`request,respond,await,cancel,get,list,history,summary.{set,get}`. [human-handoff](../features/human-handoff/index.md).

### Telemetry (`telemetry.*`)
`record,record_batch,summary,timeseries,top` · `cap.{set,list,remove,status,reset}` · `anomaly.list` · `session_summary`. [telemetry](../features/telemetry/index.md).

### Session / attach
`session.{issue,revoke,list}` · `attach.{acquire,release,force_detach,force_detach_workspace,into_gui,list}`. attach is [remote-attach](../features/remote-attach/index.md); identity tokens are [capability-elevation](../features/capability-elevation/index.md).

### Other host methods
- Notifications: `notification.{list,create}` — [notifications](../features/notifications/index.md)
- Hooks: `hook.{set,list,unset}` · `global_hook.{set,list,unset}` · `surface.fire_hook`
- Webhooks (inbound HTTP): `webhook.{register,list,info,unregister,sweep,config}` (local-only) — [webhook](../features/webhook/index.md)
- Message passing: `message.{send,read,count,clear}`
- File handlers: `file_handler.{reload,dispatch}` — [file-handler](../features/file-handler/index.md)
- Hook handlers: `hook_handler.{list,reload,dispatch}` (local-only) — query the handler registry shared by hooks/webhooks (including disabled entries), reload the user config (`~/.tasty/hook-handlers.toml`), fire one manually by id (IpcSequence / ShellCommand). dispatch is fire-and-forget (the response is only an accepted ACK)
- Completion strategies: `completion_strategy.list` (local-only) — query the registry of completion strategies that a `Custom.poll` name in `agent.task_create` refers to (including disabled entries). No reload/dispatch counterpart (it is a judging function, not something to fire) — [agent-runner](../dev-guide/agent-runner.md)
- Image: `image.{open,save,export_png,next,prev,paste,list}` — [image plugin](../plugins/image/index.md)
- Remote connection profiles: `remote.profile.{list,get,add,detect,remove,list_local,import}` (`list_local` = enumerate local `~/.ssh/config` aliases, read-only; `import` = register such an alias as an ssh profile, no shell detection) (the old `tool.ssh.*` / `ssh.profile.*` remain as temporary aliases) — [remote-profiles](../features/remote-profiles/index.md)
- webview: `webview.set_url`
- Screenshot: `ui.screenshot {path, surface_id?, window_id?}` (local-only, focus-independent — target addressed by ID) — [screenshot-methods](../ai-verification/screenshot-methods.md)
- System: `system.info` · `system.gpu_stats` (local-only, a snapshot of GPU resource counts — the wgpu global report plus per-window renderer counts, for memory-leak soak verification. CLI `tasty list gpu-stats`) — [memory-leak-soak](../dev-guide/memory-leak-soak.md)
- Timer observation: `timer.list` (local-only, read-only — a snapshot of registered periodic jobs' key / period / next deadline / precision, plus a summary of the hard deadlines currently waking this instance. Main hub + plugin-manager hub combined. CLI `tasty list timers`) — [timer-hub](../dev-guide/timer-hub.md)
- Plugin settings read-back: `settings.get_plugin_setting {storage_key}` — reads only the caller's own `plugin_settings` value (scoped to the caller)
- Remote transfer storage policy: `settings.get_remote_transfer` / `settings.set_remote_transfer {dir?, max_mb?}` (local-only, focus-independent — a global setting) — the receiving-side folder for remote bulk file transfers (`dir`, empty = the default `~/.tasty/transfers/`) and its maximum size (`max_mb`, MiB). set overlays only the given fields on the current setting and saves. CLI `tasty settings {get-remote-transfer,set-remote-transfer}`. — [remote-attach](../features/remote-attach/index.md)

### Plugin management (`plugin.*`, local-only)
`list,show,install,remove,enable,disable,upgrade_builtins,permissions,grant,revoke` · `grant_agent_permission` / `revoke_agent_permission` / `list_agent_permissions` · `request_permission` · `audit_{query,summary,follow,clear}` (**only denials are recorded** — allows are not stored, so an empty result in normal operation is expected, [ADR-0085](../adr/0085-ipc-log-retention-bounded.md)) · `extension.list`. [plugin-system](../features/plugin-system/index.md) · [capability-elevation](../features/capability-elevation/index.md).

### Lua scripts
No release IPC — scripts run only from the registered list and via shortcut triggers (ADR-0031). Arbitrary Lua injection is the debug-build-only `debug.lua.eval`. [lua-hooks](../features/lua-hooks/index.md).

### Plugin extension namespaces
When a plugin declares a prefix with `[[contributes.ipc_namespace]]`, `<prefix>.<method>` is forwarded to that plugin (e.g. `claude.*`, `codex.*`). [plugins/](../plugins/index.md).

### Debug-only (debug builds only, `DEBUG_METHODS`)
`ui.state` · `debug.{info,cell_info,screen_attrs,glyph_color,feed_bytes,inject_mouse,inject_key,tool.*,popup.*,event_bus.*,extension.invoke_hook}` · `window.focus` / `view.focus` · `system.shutdown`. Not exposed in release → [debug-ipc](../dev-guide/debug-ipc.md).

(`ui.screenshot` was promoted to a focus-independent official feature — see "Other host methods" above. [screenshot-methods](../ai-verification/screenshot-methods.md).)

## CLI mapping

The CLI wraps the IPC above (`tasty workspace list`, `tasty send text`, `tasty memory put`, `tasty agent task-create`, `tasty approval request`, `tasty plugin list`, `tasty screenshot --path …`, …). Debug subcommands exist only in debug builds. Per-environment connect / run / shutdown patterns: [environments](environments.md).

## Related

- [event-catalog](event-catalog.md) — the Event Bus, a channel separate from IPC
- [identity](../identity.md) — separation of user and agent actions (the design axis of this surface)
