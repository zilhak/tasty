<!-- source-hash: 19e60503038c -->
# Bundled plugins

The official plugins that **ship with tasty and are auto-installed on first boot** (distribution axis = bundled, [plugin concepts](../concepts/plugins.md)). Each follows the same lifecycle as an external plugin (enable / disable / remove / permissions) — the management UI is [plugin-system](../features/plugin-system/index.md).

Each plugin is one folder — `plugins/<id>/index.md` (behaviour) + `screens/` (if it has a UI). This area has the same structure as [features/](../features/index.md), but is separated because the behaviour is **provided by a plugin, not the host**. The templates are the features templates, unchanged.

## Catalogue (`BUILTINS`)

| Plugin (id) | What | Main contributions |
|---------------|------|-----------|
| [markdown](markdown/index.md) — `com.tasty.markdown` | Markdown viewer | surface_kind (webview) · file handler · cli · settings_page |
| [image](image/index.md) — `com.tasty.image` | Image viewer / paint | surface_kind (egui-mesh) · file handler · cli |
| [html](html/index.md) — `com.tasty.html` | HTML viewer | surface_kind (webview) · file handler · cli |
| [clipboard-viewer](clipboard-viewer/index.md) — `com.tasty.clipboard-viewer` | Clipboard viewer (current contents) | tools menu · popup |
| [git-viewer](git-viewer/index.md) — `com.tasty.git-viewer` | git status / log / diff viewer | tools menu · popup |
| [claude](claude/index.md) — `com.tasty.claude` | Claude Code CLI integration | cli · ipc · multi-agent |
| [codex](codex/index.md) — `com.tasty.codex` | Codex CLI integration | cli · ipc · multi-agent |
| mesh-demo — `com.tasty.mesh-demo` (no dedicated doc) | egui-mesh channel PoC (A1), excluded from distribution with `bundle=false` | surface_kind (egui-mesh) · popup |
| [agent-stream](agent-stream/index.md) — `com.tasty.agent-stream` | Agent session transcript tail → structured stream event collection (headless), excluded from distribution with `bundle=false` | cli · ipc |

> Each plugin is also **an example for the authoring guide** — the "As an example" note at the top of each doc says which contribution pattern it is the reference for and points at the relevant section of [dev-guide/plugin-development](../dev-guide/plugin-development.md).

## Related

- Concepts, classification axes, permissions: [concepts/plugins](../concepts/plugins.md)
- **Authoring guide (citing these as examples)**: [dev-guide/plugin-development](../dev-guide/plugin-development.md) · [plugin-permissions](../dev-guide/plugin-permissions.md) · [plugin-sensitive-data](../dev-guide/plugin-sensitive-data.md)
- Management / install / permissions UI: [features/plugin-system](../features/plugin-system/index.md)
- Entry point for tools-menu contributions: [features/tools-menu](../features/tools-menu/index.md)
