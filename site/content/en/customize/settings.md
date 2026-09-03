<!-- source-hash: 7306b58ad9bd -->
# Settings

After reading this page you will know how the settings window is organised, what each tab contains, and how the same content is stored in `~/.tasty/config.toml`. Keybindings and themes are covered separately in [Keybindings](keybindings.md) · [Themes](themes.md).

## Opening the settings window

Press the **Settings** button at the very bottom of the sidebar, or press `Ctrl+,`.

```
┌────────────────────────────────────────────────────────────────────────────┐
│ [General] [Terminal] [Appearance] [Keybindings] [Handler] [Misc] [Plugins] │  top tabs
├──────────────────┬─────────────────────────────────────────────────────────┤
│ Filter sections… │                                                         │
│ ▸ General        │   Settings entries of the selected section              │
│   Notifications  │                                                         │
│   Accessibility  │                                                         │
├──────────────────┴─────────────────────────────────────────────────────────┤
│                                                      [ Cancel ]  [ Save ]  │
└────────────────────────────────────────────────────────────────────────────┘
```

- Seven top tabs: **General** · **Terminal** · **Appearance** · **Keybindings** · **Handler** · **Misc** · **Plugins**.
- The list on the left holds that tab's sections. Typing into **Filter sections…** above it filters the list. Switching tabs clears the filter text.
- Changes are written to the file and applied on screen only when you press **Save**. **Cancel** discards them all. There is no close button in the header.
- Only a **Language** change requires restarting Tasty after saving.

## Entries by tab

### General

| Section | Entries |
|------|------|
| **General** | **Restore layout on startup** · **Restore surface content on restart** (terminal scrollback) · **Workspace categories (folders)** · **Next/prev workspace crosses categories** · **Close behavior** (Ask / Minimize to background / Quit) · **Language** (English / 한국어 / 日本語) |
| **Notifications** | **Notifications enabled** · **Sound** · **Coalesce interval (ms)** |
| **Accessibility** | **Reduced motion** (skips the toast fade) · **Show modifier key hints** |
| **Overlay** | **Toast duration** (1~10 seconds) |
| **Remote transfer** | **Save folder** (default `~/.tasty/transfers/`) · **Maximum size** (MiB) — where files received from a remote Workspace are saved |
| **Display** (macOS only) | **Alt key display** · **Option key display** · **Shift key display** — choose text or symbols for keybinding notation |
| **Permissions** (macOS only) | **Full Disk Access** · **Screen recording** · **Accessibility (key injection)** status and a link to open System Settings |

### Terminal

| Section | Entries |
|------|------|
| **General** | **Shell** · **Startup command** · **Scrollback lines** (default 10000) · **Confirm close running process** · **Inherit working directory** · **Reverse-screen flash (DECSCNM)** · **Show bell notification** · **Link click modifier** (Ctrl / Alt / None) · macOS: **Use Option as Meta** · Windows: **Shell mode** |
| **Mouse Capture** | **Show mouse-capture hint** · **Disable mouse capture for these programs** · **Suppress the capture hint banner for these programs** — process names or patterns such as `ht*` |
| **TUI** | **Allow clipboard read (OSC 52)** — off by default. When on, programs inside the terminal can read the clipboard |
| **Performance** | **Targeted PTY polling** · **Scrollback disk swap** — both take effect after a restart |

If you set **Link click modifier** to **None**, a plain click opens links and cannot be told apart from text selection. The meaning of the mouse capture entries is in [Working in the terminal](../using/terminal.md).

### Appearance

| Section | Entries |
|------|------|
| **Theme** | The theme card list — [Themes](themes.md) |
| **Colors** | Override the current theme's colours entry by entry — [Themes](themes.md) |
| **General** | **Default Font Settings**: **Font family** · **Custom font file** · **Font size** (default 14) · **Line height** · **Font DPI scaling** (Auto / Fixed). Plus **Ligatures** · **Background opacity** |
| **Display** | **UI Scale** — Small / Medium / Large |
| **Tasty** | App chrome colours — **Accent** · **Sidebar background** · **Active tab indicator** (Underline / Fill / Dot) |
| **Terminal** | The terminal Surface's **Focused background** · **Unfocused background** and font override |
| **Explorer** | Explorer-only font override |
| **Markdown** · **HTML** | Pages added by plugins — the Markdown font override, and the HTML viewer's **Default zoom** · **Color scheme** · **Allow remote content** · **Sandbox scripts** |

For fonts, the defaults under **General** apply to terminal · Markdown · explorer all at once, and turning off **Use default** in a section lets only that type use different values. With **Font DPI scaling** set to **Auto**, the physical size of the text stays the same across monitors; with **Fixed**, the pixel size stays the same, so text gets smaller on high-resolution monitors.

### Keybindings

Covered in [Keybindings](keybindings.md).

### Handler

The tables that decide how to "identify what kind of file it is" when opening a file, and "what to open it with". Markdown · images · HTML open on their own with the defaults, so you rarely need to touch this.

- **File Extension Mapping** — Priority when several detectors claim the same extension.
- **File Detectors** — Rules that identify a file's type by extension · path pattern. You can add your own rules.
- **File Handlers** — Which Surface opens an identified type, or whether to hand it to the OS default app.
- **Hook Handlers** — Shell commands to run when a hook · webhook event arrives. [Hooks · notifications · webhooks](../agents/hooks-notifications.md).

These tables are stored not in `config.toml` but in `~/.tasty/file-handlers.toml` and `~/.tasty/hook-handlers.toml`.

### Misc

- **Scripts** — Register Lua scripts to run from a keybinding. Enter the file path (for example `~/.tasty/scripts/my-script.lua`) and a display name; attach the keybinding under **Keybindings** > **Run Scripts**. If the file changes after registration, a **changed** marker appears and you are asked to confirm on the next run.
- On Windows a **Tastyrc** section is added. On other OSes this tab holds only Scripts.

### Plugins

Settings pages added by plugins are collected here. In a default install there are **Claude Code** and **Codex** pages — [Plugins](../plugins/index.md).

## The settings file `~/.tasty/config.toml`

Everything in the settings window is stored in this single file. Missing keys are read as defaults, so you only need to write the ones you need.

```toml
[general]
language = "ko"                  # "en" | "ko" | "ja"
close_behavior = "ask"           # "ask" | "minimize" | "quit"
restore_layout = true
restore_surface_content = true
scrollback_lines = 10000
inherit_cwd = true
confirm_close_running = true
link_click_modifier = "ctrl"     # "ctrl" | "alt" | "none"
allow_clipboard_read = false
bell_notification = true
workspace_categories_enabled = false
mouse_capture_blacklist = ["htop"]
shell = ""                       # empty = auto-detect
startup_command = ""

[appearance]
theme = "mocha"                  # ~/.tasty/themes/<id>.toml
ui_scale = "medium"              # "small" | "medium" | "large"
ligatures = true
background_opacity = 1.0
active_tab_indicator = "underline"   # "underline" | "fill" | "dot"

[appearance.default_font]
font_family = ""
font_size = 14.0
line_height = 1.0
font_scale_mode = "auto"         # "auto" | "fixed"

[appearance.terminal_font]       # leave empty to follow default_font
font_size = 15.0

[notification]
enabled = true
sound = false
coalesce_ms = 500

[accessibility]
reduced_motion = false

[modifier_hint]
enabled = true

[overlay]
toast_duration_ms = 2000

[performance]
targeted_pty_polling = true
scrollback_disk_swap = false

[remote_transfer]
dir = ""                         # empty = ~/.tasty/transfers/
max_mb = 500

[keybindings]
new_tab = ["alt+t"]              # the rest is in keybindings.md
```

- `[appearance]` also stores the full set of theme colours (`theme_base`) and the values overridden in the Colors tab (`theme_overrides`). Using the settings window is safer than editing these by hand.
- Values from plugin settings pages go into a section keyed by the plugin id, such as `[plugin_settings."com.tasty.html"]`.
- Tasty does not watch this file while running. Hand-edited values are read on the next start, and if you save from the settings window before then, the whole file is rewritten and your hand edits are lost. Quit Tasty before editing the file directly.

Only the remote transfer entries can also be read and written from the CLI.

```sh
tasty settings get-remote-transfer
tasty settings set-remote-transfer --dir ~/incoming --max-mb 1000
```

## What is in the `~/.tasty` folder

| Path | Contents |
|------|------|
| `config.toml` | The settings above |
| `themes/` | Theme files — [Themes](themes.md) |
| `plugins/` · `plugins-logs/` | Installed plugins and their logs — [Plugins](../plugins/index.md) |
| `file-handlers.toml` · `hook-handlers.toml` | User entries from the Handler tab |
| `remote-profiles.toml` | Remote connection profiles — [Remote attach](../remote/attach.md) |
| `scripts/` | The conventional place for Lua scripts |
| `transfers/` | Default save folder for files received from a remote |
| `tasty.port` · `debug.log` | The running instance's port, and the warning-and-above log — [Troubleshooting](../help/troubleshooting.md) |

## What to read next

- [Keybindings](keybindings.md) — The default table · presets · recording.
- [Themes](themes.md) — Switching themes · overriding colours · making your own.
- [Plugins](../plugins/index.md) — Plugin settings pages and the management window.
