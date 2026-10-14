<!-- source-hash: 73982c4c1c98 -->
# Working with Claude and Codex

Connect Claude Code and Codex CLI to share work across several agents. One agent can launch others and receive their results, so implementation, testing, and review can run alongside each other.

Install Claude Code and Codex CLI separately. Tasty manages launching, placement, and the connection to the agent that delegated the work. Set up both the hooks and the parent’s receiving channel. Completion lands in the same completion log whatever the parent is, and a Claude Code parent uses Monitor to subscribe to that log. Follow [Receiving completion notifications](#4-receiving-completion-notifications).

## 1. Install the hooks (once)

For Tasty to know an agent's state (working / idle / needs input / exited), each CLI's hook configuration must contain a Tasty entry.

```sh
tasty claude install    # add the Tasty entry to hooks in ~/.claude/settings.json
tasty codex install     # add the Tasty entry to [hooks] in ~/.codex/config.toml
```

- Hooks you added yourself are preserved as they are. Running it several times does not create duplicates.
- **Run it again after updating Tasty.** The hook command string is baked into the settings file, so a reinstall is needed to pick up the new format.
- The hook that reports a turn Claude Code ended on an API error (server overload, rate limit, authentication failure, and so on) as idle also arrives only with a reinstall. Until then such a child keeps looking "working".
- When a child ends its turn on an API error while background work (a background shell or a background subagent) is still running, Tasty keeps it "working" until the turn that follows that work finishes. This also arrives only with a reinstall.
- If Codex shows a `hook returned invalid ... JSON output` error, update and run `tasty codex install` again. It sets things up so Tasty's status reporting does not mix into Codex's hook response.
- These hooks do not run when you use Claude Code outside Tasty.
- Codex does not run new or changed hooks until you review them. After installing, review and approve the Tasty hooks with `/hooks` in Codex. Tasty does not skip this review.
- To remove: `tasty claude uninstall` / `tasty codex uninstall`.

Select the actual Codex home or profile when it differs from the default. Use one location
option. Other hooks and model settings are preserved.

```sh
tasty codex install --codex-home /absolute/codex-home
# To select a profile file directly:
tasty codex install --config-file /absolute/codex-home/work.config.toml
```

When the installed hooks run successfully in the current session, Tasty receives state and session information.

- When an agent finishes a response or asks a question, an **attention border** lights up on that Surface and a badge appears on the Workspace in the sidebar (waiting for a question takes priority, in yellow).
- When you close and restore a Tab, or restart Tasty, the same session resumes (`claude -r` / `codex resume`).
- Receiving a child’s state and results also requires [parent setup](#4-receiving-completion-notifications). Completion lands in the completion log as one line, and a Claude Code parent uses Monitor to subscribe to it.

After `tasty codex install`, typing `codex` or `codex resume` in a Tasty Bash or zsh terminal
also records the conversation for restoration. The native resume picker stays available, and the selected
conversation is recorded before you send another message. Approval, sandbox, and model options are preserved.
After upgrading, run the install command again and start in a new terminal. Your global shell startup files are unchanged.

Tasty starts a dedicated Codex server for each integrated terminal and ends it when Codex exits or the terminal closes.
It does not reconnect conversations or shared servers that are already running. This is based on Codex CLI 0.154.0.
Automatic restoration on the startup screen waits up to 30 seconds for Tasty to become ready. You do not need to wake the terminal manually after startup.

Automatic integration supports approval, sandbox, model, working directory (`-C`), and session selection options.
Unsupported options such as `-c`, profiles, and `fork` show a notice and run the original Codex command without automatic restoration.
`command codex`, explicit `--remote` connections, and noninteractive commands also run unchanged.
For shells other than Bash and zsh, use `tasty codex launch` below.

## 2. Launching

```sh
tasty claude launch --workspace myproj --directory ~/proj --task "Fix the tests"
tasty codex launch --workspace review --directory ~/proj
```

This creates a new Workspace and runs the CLI in its terminal. If you omit `--workspace`, the name is `claude` / `codex`.

For Codex you can attach approval and sandbox policies with `--approval untrusted|on-request|never`, `--sandbox read-only|workspace-write|danger-full-access`, and `--full-auto` (see "Codex approval policy" below).

For Claude Code you can set the permission mode with `--permission-mode` (see "Claude permission mode" below).

Even when you start Tasty from inside a Claude Code session, the shells in Tasty terminals and hook commands do not receive the variables that identify that session (`CLAUDECODE`, `CLAUDE_CODE_CHILD_SESSION`, `CLAUDE_CODE_MESSAGING_TOKEN`, and so on). Claude Code started in such a terminal runs as an independent session and saves its transcript. Claude Code variables you set yourself, such as `CLAUDE_CODE_GIT_BASH_PATH`, are passed through unchanged. Likewise, when you start Tasty from a Codex conversation, `CODEX_THREAD_ID`, which points at that conversation, is not passed on, so `tasty` commands run by the shell or by Claude in such a terminal are not treated as calls from the outer Codex. `CODEX_HOME` is passed through unchanged.

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

`spawn` **returns immediately**. There is no separate wait command. With the child’s hooks and [receiving setup](#4-receiving-completion-notifications) in place, the child’s reported idle state is delivered to the parent. Idle does not establish task success. A Claude child is not idle when one of its subagents finishes or when it ends a response while background work is still running. It is reported once the background work finishes and the response that follows ends. The same holds when a [Stop gate](#8-claude-session-profiles-and-the-stop-gate) sends the checklist back and the response continues: the child is reported idle once the response passes the gate and ends. If it leaves a command running in the background that never ends, such as a dev server, it never becomes idle. While it waits on background work, if there is no output for 10 minutes, one line naming the kind of work and the minutes waited is added to the parent’s completion log.

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

A Claude child started without a prompt is `idle` once its input screen appears. No completion notice is sent then; a completion notice comes only when a response ends. A child started with `--prompt` is `active` while it works on that prompt.

`tasty codex …` has the same subcommands (`tell` / `children` / `state` / `broadcast` / `kill` / `respawn` / `parent`). Run from a Codex conversation, results come back differently ([Automatic delivery to a Codex parent](#automatic-delivery-to-a-codex-parent)).

When there are too many children, a warning is attached to the spawn response. Change the threshold at **Settings** › **Plugins** › **Claude Code** / **Codex** › **Spawn child warning threshold** (default 6 for both Claude Code and Codex).

## 4. Receiving completion notifications

Completion lands in the same log file as one line per event, whatever the parent is, and for both Claude and Codex children.
Sending tell to the same child several times still writes one line per state change. When another parent sends tell to the same child, its notifications go to that parent's log separately.
Input requests,
interruptions, errors, and process exits are state notifications too; they do not establish task success.
When a Claude child ended its turn on an API error, the end of the completion line says so and names the error kind (for example `overloaded`). If background work is still running and the child keeps waiting, a single line saying so is written instead of a completion line.
While a child waits on a background shell or subagent and that work's output has not grown for 2 minutes, one line naming the work is written (`background work <name> has written no output for <min> min`). If Tasty cannot find that work's output, it writes one line after 10 minutes without screen output instead.

### Receiving through Claude Code's Monitor

When the parent is Claude Code, subscribe to the completion log with Monitor.

```text
Monitor({ command: "tail -n0 -F \"$TASTY_PARENT_HOME/notify/$TASTY_SURFACE_ID.log\"", timeout_ms: 1800000 })
```

Monitor expires after at most 30 minutes. When you get the expiry notice, subscribe again with the same command, then check child states once with `tasty claude children`. The list includes Codex children too (`tasty codex children` shows the same children). That way you do not miss a child that finished while the subscription was down.

Log messages follow the app language. Claude and Codex children record notifications one line at a time in the same file. **Restarting Tasty deletes completion logs from the previous run.** Reading a log without Monitor does not provide automatic resumption or permanent storage.

Before writing a new line, if the file is already **256 KiB** or larger, Tasty empties the whole file and then writes. It does not keep the last part, so unread notifications are lost too. Adding a line or concurrent writes can take it beyond that size. When it empties the file, it normally raises the `retention_start` total below by the number of bytes discarded. If other children are writing at the same time and Tasty can't get its turn to clean up in time, it empties the file without raising the total, so notifications lost then can't be detected from the total either. There is no time limit or file-count limit; a closed tab’s log stays until the next restart.

If you read logs directly and resume later, also keep track of `retention_start` in the adjacent `<number>.log.meta` file. It counts the total bytes removed by cleanup. Compare it with your saved read position to detect a gap. Cleanup or metadata-write failures can leave losses out of that total, so this does not detect every missing notification. Monitor subscriptions do not need to read this metadata file.

**Do not run two copies of tasty against the same data folder.** The one that starts later wipes
the completion-log folder as it boots, which takes the live log the earlier one was writing to.
Give each copy its own data folder with `TASTY_HOME` if you need both.

### Automatic delivery to a Codex parent

Even when Tasty starts from the desktop, it automatically uses the Codex executable and search path available to the calling conversation. You do not need to specify an executable path or a result destination for each `spawn` or `tell`.

When you run `tasty codex spawn` (or `tell` or `respawn`) inside a Codex conversation, the child's result returns to that
conversation each time the child finishes a response. There is no separate setup command.

```sh
tasty codex spawn --workspace workers --cwd ~/proj --sandbox read-only \
  --prompt "Review the diff that was just committed"
```

The parent conversation receives a short reference message like the one below. The child's answer itself is not included.

```text
[tasty] A child agent reported a turn boundary. provider=codex status=turn_ended event=<event ID>. ...
```

The parent reads the result with the command in the message.

```sh
tasty codex result <event ID>
```

- Only the conversation that received the result can read it. Reading it any number of times does not delete it.
- The result is recorded after the child Codex has actually finished the turn, with the last answer of that turn. If another Stop hook makes the response continue, you get the last answer, not an intermediate one.
- `status` tells how the response ended, not whether the work went well: `turn_ended` (the response ended), `interrupted` (the user stopped it), `exited` (the session ended; a child that only starts a new conversation with `/new` in the same terminal has not exited), or `turn_failed` (it ended with an error).
- An answer over 1 MiB is cut and marked with `body_truncated`.
- If the same response is reported twice, it is delivered once.
- A result that could not be sent because the shared Codex server was briefly unreachable is sent again at growing intervals (up to one minute). After twelve failures, it is sent again when that conversation's next result arrives or when Tasty restarts.
- Results are kept for 30 days after delivery finishes and then deleted.

A parent can receive results in two cases.

- The parent is a Codex started through Tasty's launch commands. If the parent Codex is closed for a while, the result is kept and sent when that conversation opens in Tasty again. If you start a new conversation with `/new` or switch back to another conversation in the same terminal, the conversation that is running receives results from then on.
- The parent is a Codex outside Tasty, and that conversation is open in your shared Codex server.

Otherwise, Tasty does not create the child and shows an error saying the parent cannot receive results. A Claude Code parent receives through the [completion log](#4-receiving-completion-notifications).

A Codex conversation manages only the children it created; a new conversation started with `/new` in the same terminal cannot list or close the children of the previous one either. A `tell` to another parent's child, or naming another parent with `--surface`, is refused. If Claude Code or a plain terminal sends a new instruction to the same child, later results no longer return to the original Codex conversation, including the result of a response that had not finished or been confirmed at that moment. Results that already returned can still be read.

If the children of a Codex conversation are spread over several workspaces, name one of them with `--workspace` for `children`, `broadcast`, `kill` and `respawn`. A request reaches only the children in the window that holds that workspace, so send one request per window when the children are in several windows. You do not need it when all children are in one workspace or when the command targets one child.

### Running Claude children from a Codex conversation

When you run `tasty claude spawn`, `tell`, `children`, `broadcast`, `kill`, or `respawn` inside a Codex conversation, the parent is found automatically. You don't need `--surface`, and a surface that belongs to another conversation is refused. The Codex plugin must be installed and enabled; without it the command ends with an error instead of falling back to another parent.

```sh
tasty claude spawn --workspace workers --cwd ~/proj --prompt "Run the tests"
tasty claude tell "Run clippy too this time" --surface 57   # you still name the target child
```

When the child Claude finishes its response, the answer goes back to the original Codex conversation. If a Stop gate made the response continue, or the child is waiting for background work, nothing is sent; the result is sent once, when the response really ends. Tells merged into one response come back as one result. A returned result means the response ended, not that the work is correct. A response that ended on an API error and a child that exited mid-response are reported with that status.

Responses after a `/clear` or `/resume` in the child still go back to the same Codex conversation. If Claude Code starts the new session without waiting for the end notice of the old one, the route continues when the new session starts its next response.

If the child ended and Claude was started again in the same surface, telling that child again from the Codex conversation brings the new run's responses back. If you `tell` or `respawn` the same child from somewhere other than Codex (a plain shell or a Claude parent), its results stop going to the Codex conversation. Results of responses that already ended are still sent.

If a result could not be handed to the Codex plugin, it is kept rather than dropped. It is sent again automatically at growing intervals (up to 5 minutes), and automatic retries stop after a day without success. Sending the same result again still lands in the Codex conversation only once.

```sh
tasty claude codex-returns           # show pending results and their text
tasty claude codex-returns --retry   # put every pending result back in line to be sent now
```

`--retry` also puts back results whose automatic retry stopped, and returns without waiting for the sending. The list it prints can therefore still show the results it just put back; run `codex-returns` again a moment later to check.

When the children of one Codex conversation are in more than one workspace, pass `--workspace` to `children`, `kill`, `broadcast`, or `respawn` to say which workspace's children you mean. Without it the command is refused and lists the candidates. The workspace a child is in now counts, so a child that was restored into another workspace is found where it is now.

### Resuming Claude automatically after an API error

When a temporary API error such as a server overload ends Claude's turn, Tasty can send a "please continue" message for you after a short wait so the work carries on. **It is off by default.** Turn it on at **Settings** › **Plugins** › **Claude Code**.

- **Resume automatically after a temporary API error (server overload)** — on/off.
- **Seconds to wait before resuming** — 10 seconds by default. Any value from 1 second to one day (86400 seconds) works, and a value outside that range is moved to the nearest end.
- **Consecutive automatic resumes before giving up** — 5 by default. When failures in a row reach this number, Tasty stops sending and shows one notification.
- Errors that would fail the same way again — rate limits, authentication failures, billing problems — are not resumed.
- If you type something yourself while it waits, nothing is sent. If you pressed any key or pasted anything into that surface after the turn began (there may be a half-written message in the input box), it does not resume.
- The number of automatic resumes is in the surface meta `claude-auto-resume-count`.
- This needs the hooks installed by `tasty claude install`. Run it again after updating Tasty.

## 5. Codex approval policy

`tasty codex spawn/launch/respawn/reboot` accept Codex's approval and sandbox policies as flags.

`launch`/`spawn`/`respawn`/`reboot` do not go through shell `codex` aliases or functions and run Codex installed on `PATH`. Specify approval and sandbox options through the flags below or global settings. The launch command uses sh/bash/zsh and Windows Git Bash syntax.

- **Approval**: `--approval untrusted|on-request|never`. Without a per-call option, Tasty uses the plugin’s global default. If that is unset or **Inherit**, it uses **`never`** so automation does not wait for an approval response. Specify `untrusted` / `on-request` only when a person is beside it to approve.
- **Sandbox**: `--sandbox read-only|workspace-write|danger-full-access`. Without a per-call option, Tasty uses the plugin’s global default. If that is unset or Inherit, it leaves the choice to Codex. `read-only` suits children used for review and cross-checking.
- `--full-auto`: bypasses both approval and sandbox. Cannot be combined with `--approval`/`--sandbox`.
- The global defaults are **Default approval policy** / **Default sandbox mode** at **Settings** › **Plugins** › **Codex**. Per-call flags take precedence.
- `reboot` and Tab restore, which reopen an existing conversation, apply the same policy. If the policy cannot be applied, Codex does not start and the terminal shows the reason.

In environments where nested sandboxes are not possible, such as containers, if specifying `--sandbox` fails with something like `RTM_NEWADDR: Operation not permitted`, use `--full-auto`. This hint is also attached to the completion notification.

## 6. Claude permission mode

`tasty claude launch/spawn/respawn/reboot/child-profile` accept the child's permission mode as a flag.

- `--permission-mode acceptEdits|auto|bypassPermissions|manual|dontAsk|plan` — passed straight through to Claude Code.
- **Without a per-call option, Tasty uses the plugin’s global default.** If that is unset or **Inherit**, no permission-mode flag is added and the existing Claude Code settings apply. Tasty does not automatically switch it to a mode that skips approval.
- If a child pausing for approval would break an unattended run, name the mode you want on that call. A paused child’s state is also delivered through the configured [receiving channel](#4-receiving-completion-notifications).
- The global default is **Default permission mode for child sessions** at **Settings** › **Plugins** › **Claude Code**. It defaults to **Inherit** (no flag), and per-call flags take precedence.
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

A successful `tasty codex reboot` reply means the restart was scheduled. Check the plugin log (`tasty plugin logs com.tasty.codex`) to see whether it finished. If you start Codex again in the same terminal while it waits, the restart stops without typing into that Codex.

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
tasty claude checklist-status                               # check whether it is on now
tasty claude spawn --workspace w --profile continue-checklist
```

`checklist-enable`, `checklist-disable` and `checklist-status` take `--gate <name>` to target another registered gate (default `continue-checklist`). To see whether every gate is on at once, use `tasty claude gate-list`.

- If the agent puts `[[TASTY-CHECKLIST-DONE]]` at the end of its response it passes; otherwise it receives the checklist again. When the round limit (default 3) is reached it passes automatically.
- The limit is **Default gate round limit** at **Settings** › **Plugins** › **Claude Code**.
- To create your own gate: `tasty claude gate-register <name> --body-file <file> [--sentinel <string>] [--rounds N]`. The body must contain the sentinel string. Check with `gate-list` / `gate-show`.

## Troubleshooting

- **No completion notification arrives** — check the child CLI’s hook installation and execution, then [receiving setup](#4-receiving-completion-notifications). If the parent is Claude Code, check that Monitor subscribes to the completion log. Hook delivery failures are recorded in `~/.tasty/hook-failures.log`. Plugin logs: `tasty plugin logs com.tasty.claude --follow`.
- **`reboot` fails with "claude-session-id meta not set"** — the session-start hook failed to record the session ID. Set it directly with `tasty surface-meta set --key claude-session-id --value <session ID>`.
- **The child is not spawned and you get an "occupied" error** — the target Workspace is being attached from a remote, or is a mirror. Use another Workspace.
- **`kill` is refused with a "hard-occupied" error** — a remote user is attached to the Workspace that holds that child. The child relationship and its Tab stay as they are. Run `kill` again after the remote side disconnects or you release the hold with Force detach.
- **Codex does not start and says "app-server could not start"** — Tasty could not start the dedicated Codex server for that terminal. Check that `codex app-server --help` runs in the same terminal. This feature was checked against Codex CLI 0.154.0.
- **Results do not reach a Codex parent** — check that the Tasty hooks are approved in Codex's `/hooks` and that you spawned the child inside the Codex conversation. If you started the parent by typing `codex` yourself, that conversation must be open in your shared Codex server.
- **No notifications when launched from the app icon on macOS** — Tasty calls `tasty` again when it writes notifications, but Tasty adds its own executable path to PATH automatically, so this is normally not a problem. If it still fails, look at `hook-failures.log`.

<a id="what-to-read-next"></a>

## Keep exploring

- [Task workflows](tasks.md) — Running spawn and tell as one dependency graph.
- [Hooks · notifications · webhooks](hooks-notifications.md) — Completion notices and approval gates.

Reinstalling the Codex integration also registers SessionEnd. When the current execution ends, the completion wait for that task ends with it.
