<!-- source-hash: e94a5203807b -->
# Troubleshooting

If something is not working, find the matching symptom below. Check installation, permissions, terminal connections, and notifications, or use the reporting steps at the end if you still need help.

## Files to check first

All of them live under `~/.tasty/` (on Windows, `%USERPROFILE%\.tasty\`).

| File | Contents |
|---|---|
| `config.toml` | Settings. Values saved from the settings window end up here |
| `tasty.port` | The IPC port number of the running Tasty. Created when Tasty starts |
| `debug.log` | Warning-and-above log of the previous run. Cleared when Tasty starts again |
| `crash-reports/crash-*.log` | Crash reports — version · OS · location · message · backtrace |
| `crash-reports/hang-*.log` | Written automatically when the window freezes for more than 5 seconds |
| `hook-failures.log` | Records of Claude / Codex hooks that failed to reach Tasty |
| `plugins-logs/<plugin id>.log` | Per-plugin logs. Also viewable with `tasty plugin logs <id>` |
| `state.db` | Data the app manages on its own, such as recent files |

If you need more detailed logs, run Tasty from a terminal with the log level raised. The variable is `TASTY_LOG`, not `RUST_LOG`.

```sh
TASTY_LOG=debug tasty 2> tasty.log
```

## Install · first launch

- **Windows: a "Windows protected your PC" warning appears** — the binary is not code-signed. Click **More info → Run anyway**.
- **macOS: it will not open, saying "unidentified developer"** — that is Gatekeeper. Allow it in **System Settings > Privacy & Security**, or run `xattr -dr com.apple.quarantine /Applications/Tasty.app` in a terminal.
- **Linux: the AppImage does not run** — it lacks the execute bit or FUSE is missing. Run `chmod +x Tasty-*.AppImage` first; if it still fails, start it with `./Tasty-*.AppImage --appimage-extract-and-run`.
- **Linux: it does not start, with `GLIBC_2.39 not found`** — your distribution is older than the build baseline (Ubuntu 24.04), for example Ubuntu 20.04 · Debian 11. There is no build for older distributions.
- **Linux `.tar.gz`: it exits saying a library is missing** — `tasty` lists the missing library and exits. Install the packages it names (`libfreetype6` · `libfontconfig1` · `libgtk-3` · `libwebkit2gtk-4.1` and so on). To have them pulled in automatically, use the `.deb` / `.rpm` instead.
- **"No GPU adapter found" appears and it exits** — Tasty could not find a usable GPU adapter (Vulkan / DirectX 12 / Metal). Install or update the driver. On Linux, Tasty uses Vulkan when `libvulkan1` / `vulkan-loader` is present, OpenGL when it is not, and software rendering if that fails too. To run on a server or VM without a graphical desktop, use a [headless build](../getting-started/install.md#headless-build).
- **Windows: "Git Bash not found"** — Tasty uses Git Bash as the shell on Windows. Install Git for Windows, or set the bash path yourself in **Settings** > **Terminal** > **Shell**.
- **It exits right after starting with "Database initialization error"** — read the message body. "The database is locked" means another Tasty is already running. "corrupted" / "schema version mismatch" means you can back up `~/.tasty/state.db`, delete it, and start fresh. Only the recent-files list and tutorial progress are lost.
- **You typed `tasty` inside a Tasty terminal but no new window appeared** — run with no arguments inside Tasty, it shows the help instead of opening a new window. For a new window use `tasty new window` (it leaves the focus on the window you were looking at); to force the GUI to launch, `tasty --launch`.

The install procedure itself is in [Install](../getting-started/install.md).

## macOS permission prompts

**Symptom** — a permission prompt appears mid-task and stalls what you were doing. It happens the first time a command inside the terminal (or an agent inside it) touches a new folder.

**Cause** — when a command run inside the terminal reads a file, macOS attributes that access to Tasty (Terminal.app · iTerm2 behave the same). macOS only asks at the moment of actual access, so unless you grant the permissions up front, the prompt lands in the middle of your work.

**Fix — grant them up front, in one go.** In **Settings** > **General** > **Permissions**, click **Request all permissions**: it asks one at a time, in the order Downloads · Documents · Desktop folders → (if connected) external · network volumes → screen recording. The next prompt only appears once you answer the previous one, and the window works normally while they are up. Items already allowed · denied are not asked again, so clicking it repeatedly is harmless — click it again after mounting a new volume and only that one is asked.

Tasty does **not** raise these prompts automatically at startup. That keeps a first launch from throwing a stack of unexplained prompts at you. Instead, if any permission is still missing, a notice at startup leads you to this screen.

**How to answer**

| Permission | If you do not allow it | To change it later |
|---|---|---|
| Folder access (Downloads · Documents · Desktop · volumes) | Commands that read or write that folder may fail | System Settings > Privacy & Security > Files and Folders |
| Screen recording | The `Ctrl+Alt+S` screenshot-to-clipboard feature only shows a "Screen recording permission is required" notice. Once denied, it is not asked again | System Settings > Privacy & Security > Screen & System Audio Recording |

You can see the current state in the same tab (only shown on macOS). Full Disk Access is not part of that button because no app can request it — use **Open Full Disk Access settings** below it and add Tasty in System Settings yourself. Folder permissions have no way to be queried, so they read "Cannot be observed".

- **A "Some permissions are not granted" notice appeared** — it appears at every start while Tasty does not seem to have Full Disk Access or screen recording. The **Settings** > **General** > **Permissions** tab shows which ones and what state they are in. **Open permission settings** opens Tasty's own Permissions screen. Tasty cannot request Full Disk Access itself, so from there use **Open Full Disk Access settings** to open System Settings and add Tasty to the list yourself. Granting it makes the file access prompts (other apps' data · Downloads · Documents · Desktop · volumes) go away, and the notice stops appearing from the next start. Controlling other apps (Automation) · screen recording are separate permissions, though, and remain. There is no setting to disable this notice. Tasty checks the current state at each startup.
- **I granted everything, but folder prompts still appear** — the startup notice only looks at permissions it can check. Folder permissions (Downloads · Documents · Desktop · volumes) are left out because macOS offers no way to ask for their state — asking *is* the prompt. Granting Full Disk Access covers those folders too.
- **The notice keeps coming back even though I granted it** — the signature of a locally built app or macOS permission settings may have changed. Rebuilding with ad-hoc signing can require you to grant permission again. Create the "Tasty Dev" certificate once with `./scripts/macos-codesign-identity.sh --create` and use it to sign later builds. This does not guarantee that permissions will persist; check their state in System Settings too. Right after switching, remove the old Tasty entry from the list and add the new one.
- **The Full Disk Access status shows "Unknown"** — macOS offers no API to ask whether an app has this permission, so the status is an estimate. When the file used for the estimate does not exist on your macOS version, there is nothing to judge from, so it reads Unknown and this permission alone does not raise the startup notice (a missing screen recording permission still does). No feature is blocked by this value.
- **"Tasty would like to access data from other apps" keeps appearing for every app folder** — paths like `~/Library/Application Support/<app>` are asked per app, so **Request all permissions** cannot cover them up front. Granting Full Disk Access as above makes them go away.
- **"wants to control another app" appears when you use `osascript`** — the Automation permission must be approved per target app, and Full Disk Access does not cover it. There is nothing Tasty can do in advance.

## The window freezes or crashes

- **The window does not respond to clicks · key input · the CLI at all** — when it freezes for more than 5 seconds, `~/.tasty/crash-reports/hang-*.log` is written automatically. A `Render phase` of `acquire` / `submit` / `present` records where rendering was stalled. A GPU driver issue is one possibility, so check for driver updates. Tasty does not recover on its own, so force-quit it and start it again.
- **It exited suddenly** — look at `~/.tasty/crash-reports/crash-*.log`. Attach this file when you report the problem.

## My settings or window layout look like they were reset

- **The settings went back to their defaults** — if `~/.tasty/config.toml` cannot be parsed as TOML, Tasty starts from the defaults. Your original file is not deleted. It is left where it is, and moved next to it as `config.toml.bak` the moment settings are saved over it. Fix that file and rename it back to `config.toml` and your settings come back as they were. Which line failed is written to `~/.tasty/debug.log`.
- **A saved window layout was not restored** — the same applies to a damaged slot file under `~/.tasty/layouts/`. The original is kept beside it as `01.json.bak`.
- **A notification said saving is blocked until you move or delete the `.bak` files** — Tasty tried to move your original aside but all nine slots (`.bak` through `.bak.9`) are already taken, so there is nowhere to put it. It stops saving rather than delete your file, and it stays that way for the whole session. Move or delete the backups you no longer need, then start Tasty again.
- **A notification said the settings file could not be read** — the file is there but Tasty could **not read** it (a permission problem or a disk error). In that case Tasty leaves the file alone and does not save over it, so the defaults on screen never replace your real settings. Fix the permissions or move the file aside, then start Tasty again.

  ```sh
  ls -l ~/.tasty/config.toml ~/.tasty/layouts/
  ```

## The `tasty` command cannot connect

- **`No running tasty instance found (port file not found at …)`** — there is no port file at the path checked by the CLI. If the path in the message is not `~/.tasty/tasty.port`, the command is looking at a different home directory (`TASTY_HOME`). The message follows your configured language (`general.language`, English by default), so it is worded differently if you set another one. A wrong argument (broken JSON, a `--cwd` folder that does not exist, and so on) is reported before this message, so if you see this message the argument checks performed before connecting passed.
- **The port file exists but it cannot connect** — it may be left over from an earlier run, or the CLI may be looking at another instance’s path. Check the path in the error and `TASTY_HOME`, then confirm that the Tasty instance using that file has stopped. Only delete the file if it belongs to that stopped instance. Do not delete it solely because a process-name search found nothing.


- **`tasty: command not found`** — inside a terminal that Tasty opened it is on the PATH automatically, and a `.deb` · `.rpm` · `.msi` install puts it on the PATH for other terminal apps too. With any other install method you have to add it yourself. The path for each install method is in [Install location](../getting-started/install.md#install-locations).

## Notifications do not arrive · there are too many

- **OS notifications do not appear** — while the Tasty window is active, no OS notification is sent; you are notified only inside the app, through the panel · border · badge. OS notifications go out only while the window is inactive, and are limited to one per second. Check that **Notifications enabled** under **Settings** > **General** > **Notifications** is not turned off. The panel opens with `Ctrl+Shift+I`.
- **A notification for every bell (`\a`) is noisy** — turn off **Settings** > **Terminal** > **Show bell notification**. In `config.toml`, that is `bell_notification = false` under `[general]`. Bell hooks still fire.
- **There is no sound** — **Settings** > **General** > **Notifications** > **Sound** is off by default. Even when it is on, consecutive notifications from the same source within the merge interval are combined into one, so the sound plays only once.

The full list of settings is in [Hooks · notifications · webhooks](../agents/hooks-notifications.md#settings).

## Claude · Codex hooks do not work

**Symptom** — completion notifications from child agents do not arrive. The Surface border · sidebar badge does not light up even after the agent finishes its response. Restoring a Tab or restarting Tasty does not resume the same session. `tasty claude reboot` fails with `claude-session-id meta not set`. The state shown by `tasty claude children` differs from reality.

**Cause** — the hooks are not installed, or an old hook command is still sitting in the settings file after you updated Tasty.

**Fix** — reinstall the hooks. Running it several times does not create duplicates.

```sh
tasty claude install    # ~/.claude/settings.json
tasty codex install     # ~/.codex/config.toml
```

If that still does not help, look at `~/.tasty/hook-failures.log` and `tasty plugin logs com.tasty.claude --follow`. Details are in [Working with Claude · Codex](../agents/claude-codex.md#troubleshooting).

## A plugin has stopped

- **`tasty plugin list` shows enabled but not running** — a plugin that fails to run 3 times within 10 seconds is stopped automatically. Check the cause with `tasty plugin logs <id>`, then start it again with `tasty plugin enable <id>`.
- **Right after `plugin enable` it shows running, then a moment later it does not** — `plugin enable` returns without waiting for the plugin to connect to Tasty. A plugin you just turned on therefore shows as running before it connects, and requests sent in the meantime are delivered once it connects. If it does not connect within 10 seconds, that counts as a failed run and it is no longer running. Check the cause with `tasty plugin logs <id>`.
- **A bundled plugin is broken** — copy it again from the bundle with `tasty plugin upgrade-builtins --force`. Plugin data (bookmarks · profiles and so on) is kept.

## A command cannot open the file picker

- **A script or an agent that tries to open the file picker over IPC gets a `-32016` error** — this is expected. The file picker is opened by you from **Tools** > **Open File…** in the sidebar, or by a plugin (for example when you press **Browse…** in the Markdown open-file popup). A command that opened it would take the input focus away from where you were typing, and the picked path would not come back to that command anyway. Scripts and agents should pass the file path directly.

<a id="i-do-not-know-which-port-my-dev-server-came-up-on"></a>

## Finding the port of a development server

Open **Listening ports...** from the **Tools** menu in the sidebar. It shows the TCP ports opened by processes started from Tasty terminals, together with the port · process · Workspace · Tab.

- By default only the LISTEN state is shown. If the list is empty and "No ports match the state filter" is displayed, ports in other states may still exist — use the **State** button on the right of the filter row to turn on other states and click **Apply**.
- To include processes outside Tasty, turn on **Show all (system-wide)**.
- Click a row to select it and use **Copy address** to put `host:port` on the clipboard.
- Add a port to favorites with the star icon and it always stays at the top, surviving restarts (`~/.tasty/port-favorites.toml`).

## Reporting a problem

Describe the problem in a [GitHub issue](https://github.com/zilhak/tasty/issues). Include the information below to help us investigate.

- The output of `tasty --version`, plus your OS · version
- Steps to reproduce
- The matching `crash-*.log` / `hang-*.log` from `~/.tasty/crash-reports/`
- `~/.tasty/debug.log` from right after the symptom (it is cleared on the next start, so copy it first)

### The parent agent does not receive a child result

Child attention indicators and parent delivery are separate. If the parent is Claude Code, check the completion log and the Monitor subscription. See [connection and recovery](../agents/claude-codex.md).

### Creating a semaphore or barrier fails with "invalid char"

The name contains a character that is not allowed, such as an uppercase letter or a space. The position in the message is counted in the name you typed, starting from 0. Rename it using only lowercase letters, digits, `.`, `_`, and `-`. See [Concurrency limits and signals](../agents/tasks.md#concurrency-limits-and-signals).
