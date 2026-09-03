<!-- source-hash: 90acc4edaa45 -->
# Tasty Documentation

Design, specification, and development docs for a cross-platform, GPU-accelerated native terminal emulator. This index is the entry point.

> Every document has been rewritten and verified against the [documentation model](documentation-model.md) (behavior-first). The old `docs-old/` tree was fully migrated, absorbed, or retired and has been removed.

## Read before working

| Document | Description |
|------|------|
| [identity.md](identity.md) | **Tasty's identity and inviolable principles** — concurrency (local user / AI agent / remote user), user/agent separation, headless, occupancy. The axis of every design. **Read this first** |
| [concepts/actors.md](concepts/actors.md) | **Actors** — the canonical definition of the three actors and the occupancy model |
| [concepts/hierarchy.md](concepts/hierarchy.md) | **Structural hierarchy** — Window/View › Workspace › Pane › Tab › Surface, plus the two layout levels |
| [concepts/plugins.md](concepts/plugins.md) | **Plugins** — distribution/integration axes, `surface_kind` render dispatch, permissions |
| [concepts/ubiquitous-language.md](concepts/ubiquitous-language.md) | **Unified glossary** — one-line definitions of every term with canonical links, tmux/iTerm2 equivalents, code-symbol crosswalk |
| [concepts/typed-length.md](concepts/typed-length.md) | **Typed lengths** — the `PhysicalPx` / `LogicalPx` newtypes (DPI mix-ups rejected at compile time) |
| [documentation-model.md](documentation-model.md) | **Documentation model** — category map plus the rule separating behavior docs (1st priority) from screen docs (2nd priority). Read before writing a new document |

## Documents

| Category | Entry point | Status |
|----------|------|------|
| Concepts | [concepts/index.md](concepts/index.md) | ✅ |
| Features (behavior · screens) | [features/index.md](features/index.md) | ✅ |
| Bundled plugins | [plugins/index.md](plugins/index.md) | ✅ (8 plugins, 7 with dedicated docs) |
| Design | [policies/](design/policies/focus.md) · [systems/](design/systems/theme.md) ([popup](design/systems/popup.md) · [toast](design/systems/toast.md) · [banner](design/systems/banner.md) · [icons](design/systems/icons.md) · [token-crosswalk](design/systems/token-crosswalk.md)) · [flows/](design/flows/index.md) | ✅ (policies · systems · flows) |
| Reference (lookup) | [reference/index.md](reference/index.md) | ✅ |
| Developer guide | [dev-guide/index.md](dev-guide/index.md) | ✅ |
| Architecture | [architecture/index.md](architecture/index.md) | ✅ |
| AI self-verification | [ai-verification/index.md](ai-verification/index.md) | ✅ |
| Decisions (ADR) | [adr/index.md](adr/index.md) | ✅ (immutable once Accepted) |
| Installation | [installation.md](installation.md) | ✅ |
