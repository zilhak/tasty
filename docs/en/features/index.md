<!-- source-hash: 229ec6d25ebd -->
# Features

Each feature is one folder — `features/<f>/index.md` (behaviour, internal operation · 1st priority) + `screens/<s>.md` (screen, projection · 0..N). The model and writing rules are in [documentation-model.md](../documentation-model.md); read [identity.md](../identity.md) before writing.

Templates: [`_feature.template.md`](_feature.template.md) (behaviour) · [`_screen.template.md`](_screen.template.md) (screen). Copy a template and fill it in for a new feature.


## Catalogue

| Feature | Actors | Screens |
|------|------|------|
| [main-view](main-view/index.md) — MainView (the main window) | Local user · AI agent · Remote | [Full layout](main-view/screens/main-view.md) |
| [work-area](work-area/index.md) — Work area (the Workspace/Pane/Tab/Surface domain) | Local user · AI agent · Remote | [Screen](work-area/screens/work-area.md) |
| [terminal](terminal/index.md) — Terminal (PTY · VTE · scrollback · GPU) | Local user · AI agent · Remote | GPU grid |
| [terminal-search](terminal-search/index.md) — Terminal search (scrollback + screen) | Local user | Search-bar popup |
| [terminal-link](terminal-link/index.md) — Link hover/click (modifier key) · "Open path" on right-click of a drag selection | Local user | Hover highlight |
| [workspace-tabs](workspace-tabs/index.md) — Tab strip (per-Pane tab bar) | Local user | [Screen](workspace-tabs/screens/workspace-tabs.md) |
| [workspace-category](workspace-category/index.md) — Workspace categories (sidebar folders) | AI agent · Local user | None (sidebar UI not implemented) |
| [window-chrome](window-chrome/index.md) — Window chrome (CSD title bar) | Local user | [Screen](window-chrome/screens/window-chrome.md) |
| [workspace-status-bar](workspace-status-bar/index.md) — Status bar (bottom of the work area) | Local user | [Screen](workspace-status-bar/screens/workspace-status-bar.md) |
| [sidebar](sidebar/index.md) — Sidebar (MainView's left panel) | Local user | [Screen](sidebar/screens/sidebar.md) |
| [tools-menu](tools-menu/index.md) — Tools menu (sidebar tools button) | Local user | [Menu](tools-menu/screens/tools-menu.md) |
| [settings](settings/index.md) — Settings window (sidebar settings button) | Local user | [Window](settings/screens/settings.md) |
| [plugin-system](plugin-system/index.md) — Plugin management (sidebar plugins button) | Local user · AI agent | [Window](plugin-system/screens/plugins-window.md) |
| [command-palette](command-palette/index.md) — Command palette (tools-menu item) | Local user | [Screen](command-palette/screens/command-palette.md) |
| [fullscreen-stage](fullscreen-stage/index.md) — Fullscreen stage (an independent surface that monopolises the window) | Local user | None (the stage is the screen) |
| [tutorial](tutorial/index.md) — Tutorial (marker-overlay in-app tour, tools-menu item) | Local user | Marker + callout + topic popup |
| [remote-profiles](remote-profiles/index.md) — Remote profiles + Passkeys (tools-menu item) | Local user · AI agent | [Window](remote-profiles/screens/remote-tool.md) |
| [remote-attach](remote-attach/index.md) — Remote attach (occupancy / mirror) | Remote · AI agent · Local (force-detach) | [GUI mirror](remote-attach/screens/remote-attach.md) |
| [remote-screenshot-clipboard](remote-screenshot-clipboard/index.md) — Remote screenshot → clipboard (remote clipboard applied when a mirror is focused) | Local user | None (toast only) |
| [listening-ports](listening-ports/index.md) — Listening-port viewer | Local user | [Popup](listening-ports/screens/listening-ports.md) |
| [keybindings](keybindings/index.md) — Keybindings (the KeybindingSettings domain) | Local user | [Settings tab](settings/screens/settings.md) |
| [clipboard](clipboard/index.md) — Clipboard (copy / paste / selection) | Local user | [Viewer plugin](../plugins/clipboard-viewer/index.md) |
| [notifications](notifications/index.md) — Notifications (OSC / system / panel / badge) | Local user · AI agent | Panel popup |
| [surface-highlight](surface-highlight/index.md) — Surface attention (shared state · 3 channels · completion) | AI agent · Local user | None (border / tab / badge) |
| [file-handler](file-handler/index.md) — File handlers (identify → dispatch) | Local user · AI agent · plugin | [Settings tab](settings/screens/settings.md) · picker |
| [native-file-picker](native-file-picker/index.md) — Native file picker (local + remote, Tools menu · plugin trigger) | Local user · plugin | Popup (gallery specimen) |
| [themes](themes/index.md) — Adding / managing themes (TOML) | Local user | [Settings tab](settings/screens/settings.md) |
| [lua-hooks](lua-hooks/index.md) — Lua scripts (registration + shortcut/event auto-run triggers, host API) | Local user | [0031](../adr/0031-lua-host-api-only-worker-isolated.md) |
| [agent-collaboration](agent-collaboration/index.md) — Multi-agent collaboration (`agent.*`) | AI agent | [DAG graph surface](agent-collaboration/screens/dag-graph-surface.md) · [DAG list popup](agent-collaboration/screens/dag-list-popup.md) |
| [child-terminal](child-terminal/index.md) — Child terminal management (`tasty terminal`, soft occupancy) | AI agent | None (headless) |
| [headless-pty](headless-pty/index.md) — PTY primitive without a Surface (`tasty pty`, exit code · promotion) | AI agent | None (headless) |
| [human-handoff](human-handoff/index.md) — Human handoff (approval) | AI agent · Local user | Approval popup |
| [telemetry](telemetry/index.md) — Telemetry (observation / cost / caps) | AI agent · Local user | None |
| [terminal-output](terminal-output/index.md) — Structured output (parse / commands / observe) | AI agent | None |
| [capability-elevation](capability-elevation/index.md) — Capability elevation & audit | AI agent · Local user | Elevation popup |
| [hooks](hooks/index.md) — Hooks (surface / global, automatic execution) | Local user · AI agent | [Settings tab](settings/screens/settings.md) Hook Handlers |
| [webhook](webhook/index.md) — Inbound webhook listener (external HTTP trigger) | Local user · AI agent | [Settings tab](settings/screens/settings.md) Hook Handlers (the listener is headless) |
| [closed-tab-restore](closed-tab-restore/index.md) — Restore closed items (`Ctrl+Shift+T`) | Local user | None |
| [convert-surface](convert-surface/index.md) — Surface type conversion (`Alt+'`) | Local user · AI agent | Convert popup |
| [surface-move](surface-move/index.md) — Moving a Surface (cut / move here) | Local user | OS context menu |
| [explorer](explorer/index.md) — Built-in file-manager surface (browse / open / view modes) | Local user · AI agent | Host surface |
| [layout-persistence](layout-persistence/index.md) — Layout persistence (per-window slot files `layouts/NN.json` · scrollback) | Local user | None |
| [layout-presets](layout-presets/index.md) — Layout presets (`preset.*`) | Local user · AI agent | PresetView |
| [accessibility](accessibility/index.md) — Accessibility (reduced motion, …) | Local user | [Settings tab](settings/screens/settings.md) |
| [macos-permissions](macos-permissions/index.md) — macOS permissions (TCC file / screen-recording / accessibility pre-warm, Full Disk Access guidance) | Local user | [Settings tab](settings/screens/settings.md) General > Permissions |
