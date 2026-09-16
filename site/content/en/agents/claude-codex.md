<!-- source-hash: addfbe0b7bd2 -->
# Working with Claude and Codex

Connect Claude Code and Codex CLI to share work across several agents. One agent can launch others and receive their results, so implementation, testing, and review can run alongside each other.

Install Claude Code and Codex CLI separately. Tasty manages launching, placement, and the connection to the agent that delegated the work. Set up the hooks below to receive completion notifications.

## 1. Install the hooks (once)

For Tasty to know an agent's state (working / idle / needs input / exited), each CLI's hook configuration must contain a Tasty entry.

```sh
tasty claude install    # add the Tasty entry to hooks in ~/.claude/settings.json
tasty codex install     # add the Tasty entry to [hooks] in ~/.codex/config.toml
```

- Hooks you added yourself are preserved as they are. Running it several times does not create duplicates.
- **Run it again after updating Tasty.** The hook command string is baked into the settings file, so a reinstall is needed to pick up the new format.
- These hooks do not run when you use Claude Code outside Tasty.
- To remove: `tasty claude uninstall` / `tasty codex uninstall`.

Once the hooks are installed, the following works automatically.

- When an agent finishes a response or asks a question, an **attention border** lights up on that Surface and a badge appears on the Workspace in the sidebar (waiting for a question takes priority, in yellow).
- When you close and restore a Tab, or restart Tasty, the same session resumes (`claude -r` / `codex resume`).
- Completion notifications from child agents (below) reach the parent.

## 2. Launching

```sh
tasty claude launch --workspace myproj --directory ~/proj --task "Fix the tests"
tasty codex launch --workspace review --directory ~/proj
```

This creates a new Workspace and runs the CLI in its terminal. If you omit `--workspace`, the name is `claude` / `codex`.

For Codex you can attach approval and sandbox policies with `--approval untrusted|on-request|never`, `--sandbox read-only|workspace-write|danger-full-access`, and `--full-auto` (see "Codex approval policy" below).

For Claude Code you can set the permission mode with `--permission-mode` (see "Claude permission mode" below).

<a id="3-driving-child-agents-spawn--tell"></a>

## 3. Delegating work to another agent (spawn / tell)

From a Claude Code session, use the following command to launch another agent. The agent that delegates the work is called the parent, and the new agent is called the child. The new agent opens in a tab within a pane of the chosen workspace, and Tasty records this relationship.

```sh
tasty claude spawn --workspace workers --cwd ~/proj --role tester --nickname t1 \
  --prompt "Run cargo test and report the cause of any failures"
tasty codex spawn --workspace workers --cwd ~/proj --sandbox read-only \
  --prompt "Review the diff that was just committed"
```

| Option | Meaning |
|---|---|
| `--workspace <ID or name>` | Required. The Workspace the child goes into |
| `--pane <ID>` | A specific Pane of the Workspace (default: the first Pane) |
| `--cwd <path>` | The child's working directory |
| `--role <label>` | Role label. Used to pick recipients with `broadcast --role` |
| `--nickname <name>` | Name shown on the Tab |
| `--prompt <text>` | First instruction sent right after launch |
| `--surface <ID>` | Parent Surface (default: yourself) |

`spawn` **returns immediately**. There is no separate wait command; a completion notification arrives when the child becomes idle (next section).

It is safer for the parent to put children in a **different Workspace** than its own. You cannot spawn into a remote mirror Workspace.

Afterwards, send further instructions to the child or inspect its state.

```sh
tasty claude tell "Run clippy this time too" --surface 57       # multi-line allowed, submits automatically
tasty claude children                                       # child list (index, surface, state)
tasty claude state --surface 57                             # idle / needs_input / active / exited
tasty claude broadcast "Report your progress\r" --role tester   # send to all of a role at once (\r submits)
tasty claude kill --child 0                                 # terminate by index
tasty claude respawn --child 0 --prompt "Start over"          # restart in the same place
tasty claude parent --surface 57                            # the parent of this child
```

`tasty codex …` has the same subcommands (`tell` / `children` / `state` / `broadcast` / `kill` / `respawn` / `parent`).

When there are too many children, a warning is attached to the spawn response. Change the threshold at **Settings** › **Plugin** › **Claude Code** / **Codex** › **Spawn child warning threshold** (Codex default 6).

## 4. Receiving completion notifications

Delivery depends on the **parent agent**, for both Claude and Codex children. Input requests,
interruptions, errors, and process exits are state notifications too; they do not establish task success.

### When the parent is Codex

Tool results go to the parent conversation running on the **same Codex 0.154.0 server**.
A plain `codex` TUI does not automatically join a separately started server. Connect the TUI to
an existing supported endpoint with `codex --remote <address>`. Opening a copy of its saved
conversation on another server does not establish this connection. This is separate from Tasty SSH workspaces.

Hook setup and parent binding are separate steps. Select the actual Codex home or profile when
it differs from the default. Use one location option. Other hooks and model settings are preserved.

```sh
tasty codex install --codex-home /absolute/codex-home
# To select a profile file directly:
tasty codex install --config-file /absolute/codex-home/work.config.toml
```

After a SessionStart hook has arrived, inspect the state and bind. Replace the IDs below with
actual parent values: diagnostics contains the hook session identity; the parent server returns
`thread.id` and `sessionId`. Equal values are still verified as distinct fields.

```sh
tasty codex completion diagnose
tasty codex completion bind --endpoint unix:///absolute/app-server.sock \
  --thread-id 'THREAD_ID' --session-id 'SESSION_ID' --hook-session 'HOOK_SESSION_ID'
tasty codex completion status
```

Use `--surface <parent ID>` from a different surface. Verification is asynchronous; check that
`verifying` becomes `verified`. Version, live-conversation, and server-home mismatches retain the
result and show a reason. Use `--codex-home` to check an expected server home as well.
Tasty does not start a server or silently restart the current TUI.

Unix connections use an absolute socket path. For TCP use `ws://127.0.0.1:<port>` or
`ws://localhost:<port>`; use `wss://<host>` for TLS. On Windows, use TCP/TLS. If authentication is
required, pass the environment variable **name** with `--auth-env TOKEN_ENV_NAME`. The Tasty
process must be able to read it, and the TUI must use the same authentication context. Never put
the token itself in command arguments or result text. Follow version and connection diagnostics;
Linux Unix/TCP verification is not evidence of execution on other operating systems.

An idle parent starts a turn; a regular active turn queues the output. Inspect diagnostics for
special states, including review rejection. Completion events never fall back to `tell`, keystrokes,
or user messages. Existing `tell` commands remain available for instructions you choose to send.

If the remote daemon cannot deliver SessionStart to this Tasty instance, explicitly verify the parent surface and actual hook identity and add `--register` to bind. Diagnostics marks this as caller registration, not an observed hook. Repeat this registration and binding after a Tasty restart; live endpoint/thread verification is still required.

### When the parent is Claude Code

Use the existing log and Monitor workflow.

```text
Monitor({ command: "tail -n0 -F \"$TASTY_PARENT_HOME/notify/$TASTY_SURFACE_ID.log\"", persistent: true })
```

Log messages follow the app language. The file is truncated at 256 KiB. You can read it without
Monitor, but file reading alone guarantees neither automatic resumption nor permanent retention.
This log is not a substitute for a Codex parent connection.

### Unconfirmed delivery and ending subscriptions

`status` shows each event's reason, next retry time, and subscription.

| State | Meaning and action |
|---|---|
| `pending` / `blocked` | Definitely unsent or explicitly rejected. Fix the cause, then use `tasty codex completion retry --event <ID>`. Automatic connection retries stop after 8 attempts. |
| `accepted` | Server acceptance does not guarantee persistence or processing. Do not repeatedly submit the result. |
| `unknown` | Acceptance is uncertain. Preserve the original while it is reconciled with the same server's history; do not blindly resend. |
| `recorded` | The same tool output was found in persisted history, without inferring model consumption. |
| `unbound` | No verified connection. Check the binding and SessionStart observation. |
| `cancelled` | The subscription ended while nonacceptance was known; retries stop. |

Partial history or missing notifications do not prove nonreceipt. A busy queue can lose results
on interruption or restart, so acceptance is not completion. After a host or server restart, verify
that the binding still resolves to the same parent conversation.

Releasing a Codex parent's spawn relationship stops new results and definitely-unsent retries,
but leaves the child terminal open. It does not recall accepted or uncertain results. Explicit tell
subscriptions have a separate lifetime; end one with
`tasty codex completion unsubscribe --subscription <ID>`. Reusing a surface does not transfer an old subscription.

## 5. Codex approval policy

`tasty codex spawn/launch/respawn/reboot` accept Codex's approval and sandbox policies as flags.

`launch`/`spawn`/`respawn` bypass shell `codex` aliases and functions in sh/bash/zsh and Windows Git Bash, running Codex installed on `PATH`. Specify approval and sandbox options through the flags below or global settings. Alias/function bypass for `reboot` applies on Linux/macOS. Windows `reboot` keeps its existing launch behavior and does not yet bypass aliases or functions.

- **Approval**: `--approval untrusted|on-request|never`. If you pass nothing, it runs with **`never`** — to prevent automation from getting stuck forever at an approval prompt. Specify `untrusted` / `on-request` only when a person is beside it to approve.
- **Sandbox**: `--sandbox read-only|workspace-write|danger-full-access`. If not given, the Codex default. `read-only` suits children used for review and cross-checking.
- `--full-auto`: bypasses both approval and sandbox. Cannot be combined with `--approval`/`--sandbox`.
- The global defaults are **Default approval policy** / **Default sandbox mode** at **Settings** › **Plugin** › **Codex**. Per-call flags take precedence.

In environments where nested sandboxes are not possible, such as containers, if specifying `--sandbox` fails with something like `RTM_NEWADDR: Operation not permitted`, use `--full-auto`. This hint is also attached to the completion notification.

## 6. Claude permission mode

`tasty claude launch/spawn/respawn/reboot/child-profile` accept the child's permission mode as a flag.

- `--permission-mode acceptEdits|auto|bypassPermissions|manual|dontAsk|plan` — passed straight through to Claude Code.
- **If you pass nothing, no flag is added at all.** The child starts with the Claude Code settings you already use. Unlike Codex it does not quietly become "never ask" — Claude Code has no separate sandbox axis, so making it stop asking is the same as letting it run unrestricted.
- If a child pausing for approval would break an unattended run, name the mode you want on that call. A paused child still notifies its parent, so it is not left unnoticed.
- The global default is **Default permission mode for child sessions** at **Settings** › **Plugin** › **Claude Code**. It defaults to **Inherit** (no flag), and per-call flags take precedence.
- If the settings JSON behind `--profile` / `--profile-file` sets `permissions.defaultMode`, it cannot be combined with `--permission-mode` — the two decide the same thing, so you get an error asking you to pick one.
- A mode given to `reboot` / `child-profile` applies **to that restart only**. It is not carried over when the tab is restored later.

<a id="6-restarting-a-session-reboot"></a>

## 7. Restarting a session (reboot)

After changing hooks or settings, relaunch the agent with the same session.

```sh
tasty claude reboot --surface 57 --delay 5
tasty codex reboot --surface 58
```

After the specified delay, Tasty stops the process and resumes the same session. When an agent calls this on **itself**, it should make this its last action of the turn, since stopping the process interrupts any remaining response. Restarting a child does not interrupt the parent's response.

<a id="7-claude-session-profiles-and-the-stop-gate"></a>

## 8. Claude session profiles and the Stop gate

Claude Code reads hooks only once, at startup. To attach extra hooks and permissions to a specific session only, register a profile and pass `--profile` at launch.

```sh
tasty claude profile-register strict --file ./strict.json   # register a settings JSON under a name
tasty claude profile-list
tasty claude spawn --workspace w --profile strict           # applies to this child only
tasty claude reboot --profile strict                        # carried over to later restarts too
tasty claude child-profile --child 0 --profile strict       # attach persistently to a child
```

Use a **Stop gate** to have an agent review a checklist before finishing its response. Enable the built-in `continue-checklist` gate and connect it to the session.

```sh
tasty claude checklist-enable                               # turn the gate on (checklist-disable turns it off)
tasty claude spawn --workspace w --profile continue-checklist
```

- If the agent puts `[[TASTY-CHECKLIST-DONE]]` at the end of its response it passes; otherwise it receives the checklist again. When the round limit (default 3) is reached it passes automatically.
- The limit is **Default gate round limit** at **Settings** › **Plugin** › **Claude Code**.
- To create your own gate: `tasty claude gate-register <name> --body-file <file> [--sentinel <string>] [--rounds N]`. The body must contain the sentinel string. Check with `gate-list` / `gate-show`.

## Troubleshooting

- **No completion notification arrives** — check that you have rerun `tasty claude install`. Hook delivery failures are recorded in `~/.tasty/hook-failures.log`. Plugin logs: `tasty plugin logs com.tasty.claude --follow`.
- **`reboot` fails with "claude-session-id meta not set"** — the session-start hook failed to record the session ID. Set it directly with `tasty surface-meta set --key claude-session-id --value <session ID>`.
- **The child is not spawned and you get an "occupied" error** — the target Workspace is being attached from a remote, or is a mirror. Use another Workspace.
- **No notifications when launched from the app icon on macOS** — Tasty calls `tasty` again when it writes notifications, but Tasty adds its own executable path to PATH automatically, so this is normally not a problem. If it still fails, look at `hook-failures.log`.

<a id="what-to-read-next"></a>

## Keep exploring

- [Task workflows](tasks.md) — Running spawn and tell as one dependency graph.
- [Hooks · notifications · webhooks](hooks-notifications.md) — Completion notices and approval gates.

Reinstalling the Codex integration also registers SessionEnd. When the current execution ends, its task subscriptions close; results already accepted by the server are not marked as recalled.

Use `tasty codex completion status --all` to inspect undelivered and cancelled records after the parent terminal has closed.
