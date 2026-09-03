<!-- source-hash: 784886172f8f -->
# Tasty's Identity and Inviolable Principles

> Defines *what Tasty is and why it was built that way*, and the **principles that must never be broken during development** that follow necessarily from that identity. Every document, API, and feature design sits on top of this one. **Read it first, before any work.**

## 1. What Tasty is

### A terminal built to its author's taste

Tasty is "the terminal its author built because no existing one fit their own working environment, so they made one to taste". As the name says (*tasty* = to one's taste), **the baseline is the author's workflow, not a general consensus**. The default features and bundled plugins are the things the author was already using.

At the same time, Tasty is **open to customisation for AI agents and personalised environments** — themes, keybindings, and plugins adapt it to each setup. (The code rules that support this customisation — no hard-coded colours, lengths, shortcuts, or strings — are *implementation policy*, not identity, so they live in the `CLAUDE.md` code policy and dev-guide.)

### Concurrency is the core (the most important part of the identity)

The single most important element of Tasty is **concurrency**. **The base assumption: three kinds of actors use the same tasty at the same time — ① the local user, ② several AI agents, ③ remote users attached over SSH.**

- **Several AI agents run concurrently across several terminals**, and that can be **orchestrated**.
- **The local user does their own work independently.**
- **Remote users** **occupy (attach to)** a surface/workspace from across SSH and then work *only within that occupancy* — while occupied, that target becomes read-only for the local user and AI agents, and only the local user can break the occupancy (mechanism: [`dev-guide/attach-behavior.md`](dev-guide/attach-behavior.md)).
- In short, Tasty is a terminal focused on concurrent work by *multiple agents plus local and remote users*.

This concurrency is the root of every inviolable principle below — the moment one actor intrudes on another, concurrency breaks.

> **Actor classification**: ① local user = *user actions* (direct input, owner of focus), ② AI agent = *agent actions* (IPC/CLI). ③ A remote user is a person, but is **connection (attach) + occupancy based**, so for classification purposes they sit closer to agents (no direct GUI input, not the owner of local focus, occupancy required). The canonical definition of each actor is in [`concepts/actors.md`](concepts/actors.md).

### Cross-platform

Windows · macOS · Linux are all first-class.

## 2. The inviolable principles that follow from the identity

> The moment one of these breaks, Tasty stops being "a trustworthy multi-agent terminal focused on concurrency".

### 2.1 Separation of user actions and agent actions (the soul)

The foundation of concurrency. Concurrent work only holds if user and agent share one environment without intruding on each other.

- **User action** = the local user's direct input (keyboard / mouse / OS). **Agent action** = an IPC/CLI call (an agent doing its own work). (A remote user is a person, but being attach + occupancy based, they classify on the agent side — [actors](concepts/actors.md).)
- **① Side effects of agent actions never touch user state** — focus / closed-item history (Ctrl+Shift+T) / selection, scroll, cursor. If an agent opens and closes a hundred surfaces, the user's restore stack still restores *only what the user closed*.
- **② Replaying user input does not exist in release** — key/mouse injection, forcing a popup open or closed, forcing a menu invocation, and programmatic focus changes are not on the release IPC/CLI surface. They are provided in isolation only in debug builds (`#[cfg(debug_assertions)]`), with debug code gathered under `debug/` — details in [`dev-guide/debug-ipc.md`](dev-guide/debug-ipc.md).
- **The purpose of ② — in debug, even user-only behaviour is driven over IPC for self-verification.** The *reason* the replay features of ② exist in debug is so an agent can verify the features it built. In a debug build, actions normally reserved for the user (key/mouse injection via `debug.inject_key` / `inject_mouse`, forcing popups via `debug.popup.*`, clicking tool-menu items via `debug.tool.invoke`, and so on) can be **driven over IPC**, so a feature — including its user-input flow — is verified in isolation from release (= the user's environment). → dev-guide [independent verification](dev-guide/independent-verification.md).
- **The test**: *does an agent need this to do its own work (→ release), or does it replay something the user does by hand (→ debug)?*

### 2.2 Operability by AI agents

The positive side of 2.1. Whatever an agent needs for its own work is **always provided**.

- Agent features (creating / closing / listing surfaces, tabs, and workspaces; clipboard; notifications; opening files; metadata; …) must work through **both IPC and CLI**. GUI-only agent features are forbidden.
- If an agent lacks the means to inspect or manipulate something directly, that *means Tasty is not a terminal an agent can operate freely* — so the feature gets added.
- **Headless, behaviour-first**: the truth of a feature is its internal behaviour; the screen is its projection → [`documentation-model.md`](documentation-model.md).
- **Be conscious of headless environments**: a headless instance has no local (GUI) user — only AI agents (IPC/CLI) and remote users (attach) use it. When adding a feature, never assume *a local GUI user is always present*.

### 2.3 Focus independence — focus belongs to the user

Under Tasty's base assumption (one local user plus several AI agents working at once), **focus (the active window / tab / workspace / pane) is "the user's"** — it is the user's point of view: what they are looking at and where they are typing. If an agent or some other IPC action moves focus while the local user is working, the experience is ruined. Therefore:

- Agent/IPC actions **never change focus.** Release builds have no API that changes focus.
- Every command addresses its target **directly by ID** (never via focus). `list` **walks every workspace**.
- *Reading* the active state is allowed (the `focused` field and so on); behaviour that *depends* on the active state is forbidden.
- A remote user **occupies (attaches to)** a target; they never move the local user's focus or viewpoint (and the local user can break the occupancy).
- Details: [`design/policies/focus.md`](design/policies/focus.md).

### 2.4 Cross-platform

- Every feature must work on Windows · macOS · Linux. A feature specific to one OS **must not break compilation on the others.** (The implementation-side `#[cfg(...)]` branching rules are in the CLAUDE.md code policy / dev-guide.)

## Related

- [`documentation-model.md`](documentation-model.md) — the document structure derived from this identity (headless, behaviour-first in particular)
- [`adr/0006-docs-taxonomy-behavior-first.md`](adr/0006-docs-taxonomy-behavior-first.md) — the documentation-taxonomy decision
- Terminology: [`concepts/actors.md`](concepts/actors.md) (actors) · [`concepts/hierarchy.md`](concepts/hierarchy.md) (structural hierarchy) · [`concepts/plugins.md`](concepts/plugins.md). Unified glossary: [`concepts/ubiquitous-language.md`](concepts/ubiquitous-language.md).
- [`design/policies/focus.md`](design/policies/focus.md) — operational detail on focus independence
