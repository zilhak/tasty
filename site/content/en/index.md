<!-- source-hash: 3e261acf0556 -->
# Tasty guide

Tasty is a GPU-accelerated terminal that works the same on Windows · macOS · Linux. You keep several Workspaces in one window and divide each Workspace into Panes · Tabs · splits. Almost everything a person does with the keyboard can also be done with the `tasty` CLI, so AI coding agents such as Claude Code · Codex can drive the terminal directly.

This guide is for people who download, install, and use Tasty. If you are new, read it from the top in order. If you are looking for a specific feature, jump in from the table of contents below.

## Getting started

- [Install](getting-started/install.md) — Install files per OS, the install procedure, first launch, updating and uninstalling.
- [A first look](getting-started/first-look.md) — What the sidebar · Workspaces · Panes · Tabs · splits · status bar each are.

## Using Tasty

- [Workspaces](using/workspaces.md) — Creating, naming, grouping into categories, switching, closing, restoring.
- [Panes · Tabs · splits](using/panes-tabs-splits.md) — Dividing the screen, moving, changing type, fullscreen, saving layouts.
- [Working in the terminal](using/terminal.md) — Copy/paste, search, opening links, scrolling, mouse capture, shell integration.
- [Opening files](using/files.md) — Explorer · Markdown · image · HTML · git views and other non-terminal screens.

## Make it yours

- [Keybindings](customize/keybindings.md) — The default keybinding table, presets, how to change them.
- [Settings](customize/settings.md) — The settings window and the main entries in `~/.tasty/config.toml`.
- [Themes](customize/themes.md) — Bundled themes, switching, making your own.
- [Lua scripts](customize/scripts.md) — Registering scripts and running them from a shortcut or an event.

## With AI agents

- [Driving the terminal with the tasty CLI](agents/cli.md) — Basic `list` / `send` / `read` / `mark` / `notify` patterns.
- [Working with Claude · Codex](agents/claude-codex.md) — Installing hooks, spawning child instances, tell, completion notifications.
- [Task DAG](agents/tasks.md) — Tying work together by dependency, running it in order, and watching the graph.
- [Hooks · notifications · webhooks](agents/hooks-notifications.md) — Surface hooks, global hooks, notifications, external HTTP triggers.

## Remote · plugins

- [Remote attach](remote/attach.md) — Mirroring another machine's Tasty onto your screen with profiles and SSH.
- [Plugins](plugins/index.md) — Installing, permissions, an introduction to the bundled plugins.

## Help

- [Troubleshooting](help/troubleshooting.md) — macOS permissions, log file locations, the port file, frequently asked questions.
