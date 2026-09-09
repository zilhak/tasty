<!-- source-hash: 2bfa42519860 -->
# Bring a background task into a tab

Not every task needs a visible tab from the start. Run work in a hidden PTY, inspect its output, then bring the same process and history into a tab when needed.

<!-- tasty-diagram: background-task -->
```
The log I am reading: stays in place | Running hidden PTY: moves into a new tab
```

## Preparation

Open a Tasty window and run these commands in its terminal. If `tasty` cannot connect, check the [CLI guide](cli.md).

This is a **PTY without a tab inside a running Tasty instance**, separate from running Tasty itself with a GUI-free headless build on a server.

Input examples use Bash, Zsh, or Git Bash. Replace example PTY ID `2147483648` and Pane ID `1` with actual IDs from your instance.

## 1. Open a shell without a tab

```sh
tasty pty spawn
tasty pty list
```

Note the PTY ID in the creation response. It appears in the list without creating a visible tab. With no command supplied, the shell waits for input.

## 2. Send a command and read output

```sh
tasty pty write --id 2147483648 $'pwd\n'
tasty pty read --id 2147483648 --lines 20
```

`write` sends characters as received. Bash-family `$'...\n'` includes an actual newline to submit the command; ordinary double quotes in `"...\n"` do not.

From PowerShell, use:

```powershell
tasty pty write --id 2147483648 "pwd`n"
```

Check that output shows the working directory. Read again if output has not appeared yet. Run your own task the same way.

## 3. Check the process state

```sh
tasty pty wait --id 2147483648
```

Despite its name, `wait` returns the current exit state immediately; it does not block until exit. While the shell is alive, `exited` is `false`. After exit, the response also includes its exit code and success flag.

A command finishing inside a shell does not mean the shell exited. Distinguish the PTY's exit state from individual shell command results.

## 4. View the same task in a tab

```sh
tasty list panes
tasty pty attach-surface --pty-id 2147483648 --pane-id 1
```

Use a Pane ID from the list. **Only a running PTY** can be brought into a tab. A new background tab keeps its process and output history, and the old PTY ID leaves `pty list`.

Your view does not switch automatically. Select the tab when you want to inspect it. The response includes new tab and Surface IDs for subsequent CLI operations. Do not use the old PTY ID with `pty read` or `pty kill`.

## 5. Clean up

- For a PTY that is still hidden, run `tasty pty kill --id 2147483648` with its actual ID. This terminates the process and reclaims its slot.
- After bringing it into a tab, close that tab when finished. It is no longer managed through the hidden PTY list.

The default limit is eight hidden PTYs. After five minutes of inactivity they become eligible for cleanup. `read`, `write`, and `wait` refresh the idle timer. Do not assume background output alone keeps a PTY alive. Use a regular terminal tab for work you will leave unchecked for a long time.

## If something goes wrong

- If a PTY cannot be found, check `tasty pty list`. It may have moved into a tab or been cleaned up.
- An exited PTY cannot be brought into a tab. Inspect its output and exit state, then clean it up.
- If input does not execute, check that you sent an actual newline.

For regular child terminals and output reading, see the [CLI guide](cli.md). For dependencies and failure handling, see [task orchestration](tasks.md).
