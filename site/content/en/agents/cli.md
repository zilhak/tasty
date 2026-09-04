<!-- source-hash: a13dc525a349 -->
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
tasty close self                                        # close this very Surface
```

`--target-surface this` means yourself (`TASTY_SURFACE_ID`). You can also create non-terminal surfaces, for example `--type markdown --file README.md` ([Opening files](../using/files.md)).

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
- **Not sure which window `screenshot` captures** — `--window` is required when more than one window is open. An explicit `--window` may also name a window that `list windows` does not show, such as the settings window; omitting it works only when a single window is open (it never picks whichever window happens to be focused).
