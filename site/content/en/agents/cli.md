<!-- source-hash: e45db207e7dc -->
# Driving terminals with the tasty CLI

The `tasty` command drives the terminals of a running Tasty from the outside. This page covers the basic pattern: list the Surfaces, send a command, and read back only its result.

When an AI coding agent (Claude Code, Codex, and so on) runs inside a Tasty terminal, this CLI is the tool it uses to handle the terminals next to it. It works the same way when a person uses it from a script.

## Prerequisites

- Tasty must be running. The CLI connects to the running instance through the port written in `~/.tasty/tasty.port`.
- Inside a terminal that Tasty opened, `tasty` is already on the PATH. To use it from outside (another terminal app), add the directory containing the Tasty executable to your PATH. The path for each install method is listed in [Install](../getting-started/install.md#install-locations).
- A shell that Tasty opened carries the `TASTY_SURFACE_ID` environment variable. Commands that omit `--surface` use this value, so you do not need to type an ID when driving your own terminal.

```sh
echo $TASTY_SURFACE_ID     # e.g. 42
tasty list info            # version, Workspace count, etc. — use it to confirm the connection
```

## Terms and IDs

Tasty's screen is nested as **Workspace > Pane > Tab > Surface**. A Surface is one terminal. Every target in the CLI is addressed directly by this ID — the result is the same no matter which window has focus.

```sh
tasty list tree            # whole hierarchy as a tree
tasty list workspaces      # Workspace list
tasty list surfaces        # Surface (terminal) list — across all Workspaces
tasty list panes           # Pane list
tasty list tabs --pane 3   # Tabs of a specific Pane
```

`list tree` also shows the split structure. The focused Surface is marked `*focus`.

```
└─ vertical (L|R) 60:40
   ├─ surface:396 (terminal)
   └─ horizontal (T|B) 50:50
      ├─ surface:417 (terminal) *focus
      └─ surface:418 (markdown)
```

Each row of `list workspaces` has the form `name (id:N) (pane count)`. The active Workspace is marked `*`, and a remote mirror is marked `[mirror]` ([Remote attach](../remote/attach.md)).

## Basic pattern: mark → send → read since mark

This is the standard procedure for extracting just the result of a single command.

1. `tasty set mark` — leave a marker at the current output position.
2. `tasty send text "command\r"` — send the text. `\r` is Enter.
3. Wait a moment, then `tasty read since-mark --strip-ansi` — read only the output that appeared after the marker.

```sh
tasty set mark --surface 42
tasty send text "cargo test 2>&1 | tail -20\r" --surface 42
sleep 5
tasty read since-mark --surface 42 --strip-ansi
```

- `send text` interprets the `\r` `\n` `\t` `\\` `\0` escapes. Write them as-is inside shell quotes.
- Add `--strip-ansi` to get plain text with colour and other control sequences removed. Always add it when you are going to parse the output.
- A mark stays until you call `set mark` again. `read since-mark` does not move the mark, so reading several times returns the same range.

When you are not sure whether the command has finished, read the screen and check whether the prompt is back.

```sh
tasty read screen --surface 42 --lines 5     # bottom 5 lines of the screen (reaches into scrollback if needed)
tasty is-typing --surface 42                  # whether a person pressed a key in the last 5 seconds
```

By default, `read screen` excludes dimmed autocomplete suggestions (for example Claude Code's grey suggestion text). Use `--show-dim` to include them.

If you asked for `--lines N` and got fewer, the response tells you why: `scrollback_len` is how many lines of history exist. `0` means what you got is everything there is — a full-screen app (TUI) that took over the screen right away leaves nothing behind it. Fewer than N with a non-zero `scrollback_len` is a real problem. `alt_screen` tells you whether a full-screen app is up right now.

## Sending keys

Keys other than Enter are sent with `send key`.

```sh
tasty send key enter --surface 42
tasty send key ctrl+c --surface 42
tasty send key escape --surface 42
tasty send key up --surface 42
```

Key names: `enter` `tab` `escape` (or `esc`) `backspace` `delete` `insert` `up` `down` `left` `right` `home` `end` `pageup` `pagedown` `f1`~`f12`. Join combinations with `+`, as in `ctrl+c` `alt+x` `ctrl+shift+c`.

## With shell integration: reading per command

Tasty injects shell integration into bash / zsh automatically, so you can query the commands that ran and their exit codes on a per-command basis.

```sh
tasty read commands --surface 42       # list of recorded commands
tasty read last-command --surface 42   # last command (command string and exit code)
tasty read command-at --surface 42 --index -1   # first from the end (0-based, negative counts from the end)
```

Other shells such as fish return an empty list unless you install shell integration yourself.

## Creating and closing terminals

```sh
tasty new workspace --name build --cwd ~/proj          # new Workspace
tasty split --level surface --target-surface this --direction vertical   # split my Surface left/right
tasty split --level pane --target-pane 3 --direction horizontal          # split a Pane
tasty new tab --pane 3 --cwd ~/proj                     # new Tab in a Pane
tasty close surface --surface 99                        # close a Surface
tasty close tab --tab 12
tasty close workspace --id 3                            # a whole Workspace, tabs and surfaces included
tasty close window --id 1                               # close a window
tasty close self                                        # close this very Surface
```

`--target-surface this` means yourself (`TASTY_SURFACE_ID`). You can also create non-terminal surfaces, for example `--type markdown --file README.md` ([Opening files](../using/files.md)).

The last remaining workspace and the last remaining window cannot be closed. Closing a workspace
never takes the window down with it; it is refused instead, so reach for `tasty close window` when
that is what you mean. A target holding your own terminal is refused too - use `tasty close self`
there. A workspace mirroring a remote connection is refused as well - end that connection instead.
The other way round, a workspace holding **a terminal someone is using over a remote connection**
is refused too - it closes once that person lets go of it.
Closing a workspace you are not looking at leaves the one on screen where it was.

**Closing a workspace cannot be undone.** Every terminal running inside it ends, it does not come
back from "recently closed", and its scrollback is gone. Only what a person closed by hand can be
restored. Check with `tasty list workspaces` before you close.

## Looking into a surface

```sh
tasty surface cursor-position --surface 42     # which row and column the cursor sits at
tasty surface foreground-process --surface 42  # what is running in front (a shell means idle)
tasty surface mouse-tracking --surface 42      # whether the program grabbed the mouse, and whether tasty honours it
tasty surface locate --surface 42              # the pane it belongs to, and whether it still exists
tasty surface respawn-terminal --surface 42    # restart the shell, keeping the surface in place
tasty surface fire-hook --surface 42 --event process-exit    # fire a hook yourself
tasty surface fire-hook --surface 42 --event idle-timeout:300 # some events carry a number
```

## Not sending while a person is typing

```sh
tasty send text "make test\r" --surface 42 --wait-idle
```

`--wait-idle` decides and sends in one step. Checking with `tasty is-typing` first leaves a gap in
which the person may start typing; this flag closes it. When they are typing nothing is sent and you
get `"sent": false` with the reason.

## Granting permissions to a child agent

```sh
tasty session issue --agent-id build-bot --permission surface.read --permission terminal.write
tasty session list
tasty session revoke --token <token>
```

A child holding the issued token in `TASTY_SESSION_TOKEN` may use exactly the permissions named on it.

## Sending notifications

Tell a person when a long task has finished. The notification goes to the notification panel and to the OS notification system.

```sh
tasty notify "Build finished" --title "cargo"
tasty list notifications
```

For notification behaviour in detail and automatic execution (hooks), see [Hooks, notifications and webhooks](hooks-notifications.md).

## Leaving notes on a Surface (metadata)

You can attach key-value pairs to each Surface. Use them when several agents label their roles or exchange state.

```sh
tasty surface-meta set --key role --value builder --surface 42
tasty surface-meta get --key role --surface 42
tasty surface-meta list --surface 42
tasty surface-meta unset --key role --surface 42
```

## Passing messages between Surfaces (queue)

A queue that passes messages between Surfaces without touching terminal input.

```sh
tasty send queue --to 42 "Tests done, please check the results"
tasty list queue --surface 42            # pending count and preview
tasty read queue --surface 42            # pop the oldest message
tasty read queue --surface 42 --peek     # look without popping
tasty read queue --surface 42 --clear    # empty everything
```

## Running child terminals like agents

Spawn a child terminal inside a workspace to run a command, send it messages, list them, and clean up when it finishes. This works for **any program**, not only Claude · Codex (their [dedicated commands](claude-codex.md) do the same thing with session management layered on top).

```sh
tasty terminal spawn --workspace build --command "cargo watch -x test\r" --cwd ~/proj --role worker
tasty terminal children                        # children under me
tasty terminal tell "y\r" --surface 57         # send input to a child (line breaks kept, submitted automatically)
tasty terminal broadcast "git pull\r" --role worker   # to every child with the same role
tasty terminal kill --child 1                   # kill a child by index
```

`spawn` returns immediately, and when a child goes idle · waits for input · exits, a notification comes back to the parent surface — there is no separate command to wait on. Tag children with `--role` to address them together with `broadcast`.

## Headless PTY

Run a program on a real PTY (pseudo-terminal) with no tab and no screen. Use it when a command needs a TTY but you drive it from a script without taking up screen space. Feed input and read the screen by the id that `spawn` returns.

```sh
tasty pty spawn --cwd ~/proj -- python3         # start a command on a PTY and get its id
tasty pty write --id 3 "print(1+1)\n"           # send to stdin (a newline submits)
tasty pty read --id 3 --lines 20                # the last 20 lines on the screen right now
tasty pty list                                  # PTYs that are up
tasty pty kill --id 3                            # stop it
```

## Memory shared between agents

A key-value store where several agents in the same Tasty exchange values. Pick a scope (global · surface · workspace · window · account) to store under, and optionally let entries expire (TTL) or guard against overwrites (CAS).

```sh
tasty memory put --workspace 7 --key build/status --value running --ttl 600
tasty memory get --workspace 7 --key build/status
tasty memory list --workspace 7 --prefix build/
tasty memory delete --workspace 7 --key build/status
```

Switch scope with `--global` · `--surface 3` · `--window 42` · `--account me`. A value that parses as JSON is stored as JSON, otherwise as a string.

## Pulling signals out of the output (observers)

Watch the output as it scrolls past and collect only the **structured signals** — paths · URLs · exit codes · prompt boundaries. Use it so a script can react to those signals without a person watching the screen.

```sh
tasty output observe start --surface 42 --parsers exit_code,url --sink file
tasty output observe list                        # observers running now
tasty output observe info --observer 1           # one observer's state and collected count
tasty output observe stop --observer 1
```

`--sink memory` collects into an in-memory ring buffer, `--sink file` into a file. Leave `--parsers` empty and the default parsers (paths · URLs · prompt boundaries · exit codes) are all on.

## Measuring agent activity

Several agents record their own activity as numbers (token counts · call counts and so on), and you look at it as sums · time series · top rankings. Use it to see at a glance what the whole fleet is doing and how much.

```sh
tasty telemetry record --metric tokens --value 1200 --tags '{"model":"opus"}'
tasty telemetry summary --metric tokens           # sum and count
tasty telemetry top --by agent --metric tokens    # top by agent
tasty telemetry timeseries --metric tokens --window 1h
```

`record` attributes the call to the caller automatically (`TASTY_AGENT_ID`). To insert several values at once with their order preserved, use `tasty telemetry record-batch`.

## Other queries and settings

Things an agent reaches for occasionally. `tasty <command> --help` lists them all.

```sh
tasty list theme                       # the theme snapshot in effect (colors, font sizes, UI scale)
tasty list recent --kind markdown      # files recently opened as that kind
tasty set cwd --surface 42 --path /tmp # change the working directory a remote surface reports
tasty set url --surface 42 --url URL   # change the address of a webview surface
tasty file-handler dispatch PATH       # open a file the same way a double-click in the explorer does
```

`set cwd` and `set url` only apply to a remote surface and a webview surface respectively. Point them at a plain terminal surface and they say so.

## Frequently used commands

| What you want | Command |
|---|---|
| Show the hierarchy | `tasty list tree` |
| Surface list | `tasty list surfaces` |
| Send text (including Enter) | `tasty send text "ls\r" --surface ID` |
| Send a key | `tasty send key enter --surface ID` |
| Set a mark | `tasty set mark --surface ID` |
| Read since the mark | `tasty read since-mark --surface ID --strip-ansi` |
| Read the screen | `tasty read screen --surface ID --lines N` |
| Notification | `tasty notify "body" --title "title"` |
| Screenshot | `tasty screenshot --path out.png [--surface ID] [--window ID]` |
| Help | `tasty --help`, `tasty <command> --help`, `tasty -a -h` (full tree) |

## Troubleshooting

- **Cannot connect** — check that Tasty is running and that the `~/.tasty/tasty.port` file exists. If the file is there but the connection fails, the previous instance exited abnormally ([Troubleshooting](../help/troubleshooting.md)).
- **Calling without `--surface` is rejected** — in a shell without `TASTY_SURFACE_ID` (outside Tasty) there is no target Surface, so the command ends in an error. Tasty never guesses the focused one: the same command gives the same result no matter which window is in front. Always write `--surface` in scripts.
- **`read since-mark` is empty** — either the output finished before you set the mark, or the command has not finished yet. Check the current state with `read screen`.
- **Not sure which window `screenshot` captures** — automatic selection counts **main (terminal) windows only**. With one main window open, omitting `--window` captures it; with several, `--window` is required (it never picks whichever window happens to be focused). Windows that `list windows` does not show, such as the settings window, are not counted: `--window` stays optional while the settings window is up, and capturing the settings window itself means naming its ID with `--window`.

## What to read next

- [Claude · Codex](claude-codex.md) — Spawning child agents and being told when they land.
- [Task DAG](tasks.md) — Tying several pieces of work together by dependency.
- [Hooks · notifications · webhooks](hooks-notifications.md) — Running commands automatically on an event.
