<!-- source-hash: 7be0713f9156 -->
# Tasty guide

Tasty is a GPU-accelerated terminal where you and your AI agents work together. On Windows, macOS, and Linux, you can organize projects into workspaces and open several terminals side by side. Agents use the `tasty` CLI to create terminals, run commands, and read the results.

Start with installation and a first look if you are new to Tasty. If you already use it, choose the feature you need below.

## Getting started

- [Install](getting-started/install.md) — Install files per OS, the install procedure, first launch, updating and uninstalling.
- [A first look](getting-started/first-look.md) — What the sidebar · Workspaces · Panes · Tabs · splits · status bar each are.

## Using Tasty

- [Workspaces](using/workspaces.md) — Creating, naming, grouping into categories, switching, closing, restoring.
- [Build and reuse a working layout](using/workflow-layouts.md) — arrange implementation and tests, then save a preset.
- [Panes · Tabs · splits](using/panes-tabs-splits.md) — Dividing the screen, moving, changing type, fullscreen, saving layouts.
- [Working in the terminal](using/terminal.md) — Copy/paste, search, opening links, scrolling, mouse capture, shell integration.
- [Opening files](using/files.md) — Explorer · Markdown · image · HTML · git views and other non-terminal screens. File management in the explorer (copy · move · rename · trash, undo, the OS file clipboard, dragging out to other apps, changing files in a remote explorer, preview · properties).

## Make it yours

- [Keybindings](customize/keybindings.md) — The default keybinding table, presets, how to change them.
- [Settings](customize/settings.md) — The settings window and the main entries in `~/.tasty/config.toml`.
- [Themes](customize/themes.md) — Bundled themes, switching, making your own.
- [Lua scripts](customize/scripts.md) — Registering scripts and running them from a shortcut or an event.

## With AI agents

- [Driving the terminal with the tasty CLI](agents/cli.md) — A first walkthrough that creates a working terminal, sends a command and reads its output, plus the basic pattern.
- [CLI command reference](agents/cli-reference.md) — Reading from a saved position, metadata, queues, memory, observers and other commands beyond the introduction.
- [Bring a background task into a tab](agents/background-tasks.md) — inspect output and exit state, then move a running hidden PTY into a tab.
- [Working with Claude · Codex](agents/claude-codex.md) — Installing hooks, spawn and tell, and Claude Monitor setup.
- [Task workflows](agents/tasks.md) — Tying work together by dependency, running it in order, and watching the graph.
- [Hooks · notifications · webhooks](agents/hooks-notifications.md) — Surface hooks, global hooks, notifications, external HTTP triggers.

## Remote · plugins

- [Working remotely](remote/attach.md) — Connecting to Tasty on another computer with profiles and SSH.
- [Plugins](plugins/index.md) — Installing, permissions, an introduction to the bundled plugins.

## Help

- [Troubleshooting](help/troubleshooting.md) — macOS permissions, log file locations, the port file, frequently asked questions.
- [CLI response delays and connection diagnostics](help/cli-diagnostics.md) — Reply wait versus task wait, what to do for each error code, reading `tasty list pressure`.
