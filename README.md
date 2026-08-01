# Tasty

<img src="assets/icons/tasty-melon.svg" alt="Tasty logo" width="96" height="96" />

한국어: [README.ko.md](README.ko.md)

> **Tasty** is a cross-platform, GPU-accelerated terminal emulator purpose-built for AI coding agents. It provides multi-agent orchestration, headless operation, and a focus-independent IPC/CLI surface across Windows, macOS, and Linux. (Detailed docs are in Korean — start at [`docs/index.md`](docs/index.md).)

[![Version](https://img.shields.io/badge/version-0.9.11-blue)](CHANGELOG.md)
[![License: MIT](https://img.shields.io/badge/license-MIT-green)](#license)
[![Platforms](https://img.shields.io/badge/platforms-Windows%20%7C%20macOS%20%7C%20Linux-lightgrey)](docs/installation.md)
[![Workspace](https://img.shields.io/badge/workspace-43%20crates-orange)](crates/)

Where GPU-accelerated terminals like WezTerm and Alacritty focus on the human typing experience, Tasty adds another coordinate on top: a terminal an AI agent can operate directly — every surface is equally open to keyboard/mouse *and* IPC/CLI.

## Identity — Separation of User Actions and Agent Actions

Every Tasty API strictly separates **user actions** (keyboard/mouse/native OS input) from **agent actions** (IPC methods/CLI subcommands). Side effects of agent actions never touch the user's focus, history, or selection state — features that *replay* user input (key injection, forced focus switches, etc.) do not exist on the release IPC/CLI surface (debug-only isolation). Full principles: [`CLAUDE.md`](CLAUDE.md).

## Core Values

- **Cross-platform** — Windows / macOS / Linux, all native (winit + wgpu).
- **GPU-accelerated rendering** — cell-based shaders, stable prepare/draw even with 10+ surfaces.
- **Hexagonal architecture** — model + ports + adapters + view + host_api separation, 43-crate workspace.
- **AI agents as first-class citizens** — every IPC/CLI surface is focus-independent and ID-based. User actions and agent actions are fully separated (debug isolation).

## Installation

Full instructions: [`docs/installation.md`](docs/installation.md).

Grab prebuilt binaries for macOS (DMG), Windows (MSI), and Linux (AppImage, etc.) from **[GitHub Releases](https://github.com/zilhak/tasty/releases/latest)**. The source tree can be ahead of the latest release, so build from source below if you need the newest features.

```bash
# Build from source (all platforms)
git clone https://github.com/zilhak/tasty.git
cd tasty
cargo build --release
./target/release/tasty
```

## Key Features

- **Orchestrate multiple AI agents in one terminal** — a task DAG plus barrier / semaphore / lease / reduce / rate-limit collaboration primitives coordinate parallel work ([`docs/features/agent-collaboration/index.md`](docs/features/agent-collaboration/index.md))
- **Runs fully headless** — create, tear down, and drive surface I/O with CLI/IPC alone, so it drops straight into CI/server environments (`--headless`, [`docs/features/headless-pty/index.md`](docs/features/headless-pty/index.md))
- **Select and copy with the keyboard alone** — vi-style copy mode (hjkl movement, visual selection, search) with GPU cursor visualization ([`docs/features/clipboard/index.md`](docs/features/clipboard/index.md))
- **Produce installers in one step** — `cargo build --profile dist` plus a Justfile wrapper auto-builds DMG / MSI / AppImage
- **Extend it yourself with plugins** — an SDK with a manifest schema and a permission system ([`docs/features/plugin-system/index.md`](docs/features/plugin-system/index.md))
- **Share context between agents** — Blackboard / Plan / Cache let multiple agents exchange the same working context ([`docs/features/agent-collaboration/index.md`](docs/features/agent-collaboration/index.md))
- **Pinpoint output per shell command** — recognizes shell prompt boundaries to capture exactly "this command's output" ([`docs/features/terminal-output/index.md`](docs/features/terminal-output/index.md))
- **Watch terminal output live and trigger follow-up work** — parses PTY output lines and fans them out to memory/file sinks automatically ([`docs/features/terminal-output/index.md`](docs/features/terminal-output/index.md))
- **Cap agent token spend automatically** — tracks and aggregates usage, auto-blocking once a cost cap is exceeded ([`docs/features/telemetry/index.md`](docs/features/telemetry/index.md))
- **Theme it your way** — a user-customizable theme system built on a 4px grid and a 14px font-size ceiling ([`docs/features/themes/index.md`](docs/features/themes/index.md))
- **Run several child Claude instances and get notified as each finishes** — spawn/tell return immediately, and a completion notification arrives automatically whenever a child goes idle, needs input, or exits ([`docs/plugins/claude/index.md`](docs/plugins/claude/index.md))

## Documentation

- Index: [`docs/index.md`](docs/index.md)
- User guides: [`docs/installation.md`](docs/installation.md), [`docs/features/`](docs/features/index.md)
- Agent guides: [`docs/reference/`](docs/reference/index.md) (api / event-catalog / output-parsers / environments / plan.schema.json)
- Developer guides: [`docs/dev-guide/`](docs/dev-guide/)
- Stability policy: the "Stability Policy" section of [`docs/dev-guide/api-conventions.md`](docs/dev-guide/api-conventions.md)

## Architecture

A hexagonal architecture (model + ports + adapters + view + host_api separation) across a 43-crate workspace. Full structure: [`docs/architecture/`](docs/architecture/).

## License

MIT — [`LICENSES/`](LICENSES/). Third-party dependency license bundle: [`THIRD_PARTY_LICENSES.md`](THIRD_PARTY_LICENSES.md).
