<!-- source-hash: 8324f4590776 -->
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

When a child becomes idle (idle) or needs input (needs_input), or exits, one line is appended to the **notification log file** of the Surface that called `spawn`/`tell`.

```
$TASTY_PARENT_HOME/notify/$TASTY_SURFACE_ID.log
```

- Both environment variables are already present in shells that Tasty opened. Use these variables to locate the log file.
- Example line (English): `surface 57 task complete (via spawn)`. The wording follows the app language.
- A notification is added whenever the state changes while the child agent is running.
- When the file exceeds 256 KiB it is emptied and written afresh.
- If a Claude child has been stalled for more than 30 seconds after an API error, a separate "stalled" line arrives in the same file.

When a Claude Code session is the parent, hook this file with the Monitor tool once, and from then on the completion of every child arrives as a notification.

```
Monitor({ command: "tail -n0 -F \"$TASTY_PARENT_HOME/notify/$TASTY_SURFACE_ID.log\"", persistent: true })
```

In environments where Monitor is not available, read the file directly (`tail -f`). Delivery may be delayed by tens of seconds, but it is never lost.

Codex children have no `needs_input` notification (Codex CLI has no such event). A pause at an approval prompt will not appear in state notifications, so check the approval policy below before starting.

## 5. Codex approval policy

`tasty codex spawn/launch/respawn/reboot` accept Codex's approval and sandbox policies as flags.

- **Approval**: `--approval untrusted|on-request|never`. If you pass nothing, it runs with **`never`** — to prevent automation from getting stuck forever at an approval prompt. Specify `untrusted` / `on-request` only when a person is beside it to approve.
- **Sandbox**: `--sandbox read-only|workspace-write|danger-full-access`. If not given, the Codex default. `read-only` suits children used for review and cross-checking.
- `--full-auto`: bypasses both approval and sandbox. Cannot be combined with `--approval`/`--sandbox`.
- The global defaults are **Default approval policy** / **Default sandbox mode** at **Settings** › **Plugin** › **Codex**. Per-call flags take precedence.

In environments where nested sandboxes are not possible, such as containers, if specifying `--sandbox` fails with something like `RTM_NEWADDR: Operation not permitted`, use `--full-auto`. This hint is also attached to the completion notification.

## 6. Restarting a session (reboot)

After changing hooks or settings, relaunch the agent with the same session.

```sh
tasty claude reboot --surface 57 --delay 5
tasty codex reboot --surface 58
```

After the specified delay, Tasty stops the process and resumes the same session. When an agent calls this on **itself**, it should make this its last action of the turn, since stopping the process interrupts any remaining response. Restarting a child does not interrupt the parent's response.

## 7. Claude session profiles and the Stop gate

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
