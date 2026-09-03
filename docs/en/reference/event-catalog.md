<!-- source-hash: c06ab753ccc2 -->
# Event Bus 1.0 catalogue

The wire contract for events shared between the host and plugins. **A public API that plugins depend on** and the single source of truth (SoT) for the compatibility policy. *How* a plugin uses the subscribe/publish API is in [concepts/plugins](../concepts/plugins.md) · dev-guide; this document covers only the *structure* of events. The payload Rust types in `tasty_plugin_protocol::events::payloads` are the truth; this table is a human-readable summary.

## Envelope

```jsonc
{
  "key": "surface.focused",
  "payload": { "surface_id": 42, "prev_surface_id": 7 },
  "meta": { "trace_id": "trace-abc", "hop": 0, "origin": { "kind": "host" }, "scope": "surface" }
}
```

| Field | Meaning |
|------|------|
| `key` | `<namespace>.<event_name>`. Reserved namespaces are published by the host only |
| `payload` | Per event (catalogue below) |
| `meta.trace_id` | An opaque id shared by the whole chain. Created when the host publishes; propagated when a plugin re-publishes |
| `meta.hop` | `0` when the host publishes, `+1` per plugin re-publish. **The dispatcher blocks at `hop > 16` (MAX_HOP)** |
| `meta.origin` | `{kind:host}` or `{kind:plugin, plugin_id}` |
| `meta.scope` | `system` (global) or `surface` (the target id is a payload field) |

- **Lifecycle `reason`** (the closing family `*.closed` / `plugin.unloaded`): `user` (directly by the user) / `ipc` (CLI / plugin automation) / `crash` (abnormal). No separate cascade class — the actor who closed the parent propagates to the child's reason.
- **Throttling**: `surface.resized` (per key) and `split.ratio_changed` (per group) are 150 ms leading + trailing. Drag start and end always fire once.

## Reserved namespaces (host publishes only)

```
system, surface, tab, pane, workspace, window, command, ime, split,
notification, hook, tool, plugin, extension, process, clipboard, theme, language, memory
```

Anything else is free for plugins — by convention, their own `id` (`com.tasty.claude.*`) as the namespace.

## Stability tiers

- **Stable** — key and required fields unchanged until the next major; optional fields may be added.
- **Experimental** — may change every minor; requires `experimental_events = true` in the manifest.
- **Internal** — debug builds only.

## Catalogue

### Surface (scope=surface)
| Key | When | payload | Tier |
|----|------|---------|------|
| `surface.created` | right after creation | `surface_id, kind, tab_id, pane_id, workspace_id, created_by` | Stable |
| `surface.closed` | right before teardown | `surface_id, kind, reason` | Stable |
| `surface.focused` | focus | `surface_id, prev_surface_id?` | Stable |
| `surface.resized` | size change (throttled) | `surface_id, width_px, height_px` | Stable |
| `surface.title_changed` | display name change | `surface_id, title` | Stable |

`created_by`: `{kind:user}` or `{kind:agent, source_plugin}`.

### Tab / Pane / Split (scope=system)
| Key | payload |
|----|---------|
| `tab.created` | `tab_id, pane_id, workspace_id, kind` |
| `tab.closed` | `tab_id, pane_id, reason` |
| `tab.focused` | `tab_id, pane_id, prev_tab_id?` |
| `tab.moved` | `tab_id, from_pane, to_pane` |
| `tab.renamed` | `tab_id, title` |
| `pane.created` | `pane_id, parent_pane_group?, workspace_id` |
| `pane.closed` | `pane_id, reason` |
| `pane.split` | `original_pane, new_pane, direction` |
| `split.ratio_changed` (throttled) | `group_id, level(pane/surface), ratio` |

### Workspace / Window (scope=system)
| Key | payload |
|----|---------|
| `workspace.created` | `workspace_id, window_id, name` |
| `workspace.closed` | `workspace_id, reason` |
| `workspace.activated` | `workspace_id, prev_workspace_id?` |
| `workspace.renamed` | `workspace_id, name?, subtitle?, description?` |
| `window.created` | `window_id, kind, modality` |
| `window.closed` | `window_id, reason` |
| `window.focused` | `window_id` |

`window.kind` / `modality` match [hierarchy](../concepts/hierarchy.md).

### Process (scope shown)
| Key | scope | payload |
|----|-------|---------|
| `process.started` | surface | `surface_id, pid, command` |
| `process.exited` | surface | `surface_id, exit_code?` |

### Plugin / Extension / Tool (scope=system)
| Key | payload |
|----|---------|
| `plugin.loaded` | `plugin_id, version` |
| `plugin.unloaded` | `plugin_id, reason` |
| `plugin.error` | `plugin_id, error_kind, message` |
| `plugin.enabled` / `plugin.disabled` | `plugin_id` |
| `extension.activated` | `extension_id, target_id` |
| `extension.pending` | `extension_id, target_id, reason` |
| `extension.conflict` | `extension_id, target_id, conflicting_id` |
| `tool.invoked` | `tool_id, source(builtin/plugin)` |

### Command (Option D — plugins never see shortcuts)
| Key | Delivery | payload |
|----|------|---------|
| `command.invoked` | **owner unicast** (not broadcast) | `plugin_id, command_id, scope, source_surface_id?, trigger(shortcut/menu/ipc)` |
| `command.shortcut_changed` | broadcast | `plugin_id, command_id, shortcut?, prev_shortcut?` |

scope=global command shortcuts must be chords; scope=surface also allows single keys.

### Memory (scope=system, Stable)
`memory.changed`: right after a put/delete/expire/cleanup of a regular entry — `scope, key, kind∈{created,updated,deleted,expired}, version?`. **The secret area never fires** (to avoid exposing owner/key). 1 change = 1 envelope. Subscription permission `memory.read`.

### IME / Theme / Language / Notification / Hook / System
| Key | scope | Tier | payload |
|----|-------|------|---------|
| `ime.composition_start` / `_end` | surface | Experimental | `surface_id` / `surface_id, committed_text` |
| `theme.changed` | system | Stable | `theme_id` |
| `language.changed` | system | Stable | `language_code` |
| `notification.created` | system | Stable | `id, title, body, source` |
| `notification.dismissed` | system | **Planned** (reserved, not fired) | `id` |
| `hook.fired` | surface/system | Experimental | `hook_id, event_kind, surface_id?, payload` |
| `system.startup_complete` | system | Stable | `{}` |
| `system.shutdown_initiated` | system | Stable | `reason` |
| `debug.*` | system | Internal | (varies, debug builds only) |

> `composition_update`, `process.output_match`, and `settings.changed` are excluded from 1.0. Marking a notification *read* fires no host event (it is display state only).

## Subscribe / publish permission patterns

The manifest's `event_subscribe` / `event_publish` are the permission gate.

| Form | Example | Matches |
|------|-----|------|
| Exact key | `surface.closed` | that key only |
| Namespace wildcard (trailing only) | `surface.*` | everything with the `surface.` prefix |
| Plugin-id namespace | `com.tasty.claude.*` | everything that plugin publishes (the target manifest must declare `[[events_emitted]]`) |

Rejected: `"*"` (everything), `"*.bar"` / `"foo*"` (leading/inner wildcards). `event_publish` rejects reserved-namespace keys.

## Change policy

Removing a Stable key or required field → major bump. Adding optional fields, adding new events, promoting Experimental → Stable → minor or below (plugin compatibility preserved). Adding a new reserved namespace can collide → major / migration notice.

## Related

- [concepts/plugins](../concepts/plugins.md) — plugin integration axis (including events)
- [reference/api](api.md) — the IPC/CLI surface
