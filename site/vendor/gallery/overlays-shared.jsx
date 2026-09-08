// Tasty Gallery — Overlays SHARED frames. Loaded by every overlays-*.html page
// (Dialogs / Windows / Popups & menus / Banners). Defines the scrim Backdrop, the
// icon set, and all live overlay frame components, then exposes them on
// window.OverlaysShared. No page content here — see overlays-<family>.jsx.

// Tasty Gallery — Overlays. The modal / popup layer: scrim recipe,
// command palette, the two-tier settings window, agent approval, and
// rename. These are the patterns an agent should copy for any new
// dialog — exact frame dimensions and the scrim contract are spec'd.
const { Section, Spec, Stage, Meta, Note, Do, Dont } = window.Gallery;
const { MenuItem, Input, Select, Switch, Checkbox, Button, IconButton, Tag, Kbd, Badge, Table, StatusDot, Spinner, Icon } = window.TastyDesignSystem_41fd3f;

const NAV = [
  { id: "scrim", label: "Scrim & frame" },
  { id: "banner", label: "Banner" },
  { id: "palette", label: "Command palette" },
  { id: "tools", label: "Tools menu" },
  { id: "ports", label: "Listening ports" },
  { id: "remote", label: "Remote connections" },
  { id: "search", label: "Search bar" },
  { id: "switch", label: "Switch-number overlay" },
  { id: "approval", label: "Agent approval" },
  { id: "convert", label: "Convert surface" },
  { id: "filehandler", label: "File handler picker" },
  { id: "preset", label: "Apply preset" },
  { id: "preseteditor", label: "Preset editor" },
  { id: "markdown", label: "Markdown open" },
  { id: "rename", label: "Rename popup" },
  { id: "settings", label: "Settings window" },
];

// glyphs by name from the canonical set (icons/*.svg via <Icon name>)
const ic = {
  search: <Icon name="search" />,
  plus: <Icon name="plus" />,
  term: <Icon name="terminal" />,
  md: <Icon name="markdown" />,
  split: <Icon name="split" />,
  settings: <Icon name="settings" />,
  rocket: <Icon name="rocket" />,
  x: <Icon name="close" />,
  refresh: <Icon name="refresh" />,
  back: <Icon name="chevronLeft" />,
  tools: <Icon name="tools" />,
  port: <Icon name="port" />,
  remote: <Icon name="remote" />,
  swap: <Icon name="swap" />,
  file: <Icon name="file" />,
  edit: <Icon name="edit" />,
  layers: <Icon name="layers" />,
  download: <Icon name="download" />,
  funnel: <Icon name="filter" />,
  mouse: <Icon name="mouse" />,
  check: <Icon name="check" />,
  warn: <Icon name="alertTriangle" />,
  folder: <Icon name="folder" />,
};

// a faux app backdrop so the scrim reads correctly
function Backdrop({ height = 360, blur = false, children }) {
  return (
    <div style={{ position: "relative", width: "100%", height, borderRadius: "var(--tasty-radius)", overflow: "hidden",
      border: "1px solid var(--tasty-border-default)", background: "var(--tasty-bg-app)" }}>
      {/* hint of app chrome behind */}
      <div style={{ position: "absolute", inset: 0, padding: 14, opacity: 0.5 }}>
        <div style={{ height: 28, background: "var(--tasty-bg-sidebar)", borderRadius: "var(--tasty-radius)", marginBottom: 10 }} />
        <div style={{ display: "flex", gap: 10, height: "calc(100% - 38px)" }}>
          <div style={{ width: 120, background: "var(--tasty-bg-sidebar)", borderRadius: "var(--tasty-radius)" }} />
          <div style={{ flex: 1, background: "#000", borderRadius: "var(--tasty-radius)" }} />
        </div>
      </div>
      {/* scrim — flat fill by default; the 1px blur (documented recipe) is opt-in
          so only the canonical Scrim spec pays the per-frame compositing cost */}
      <div style={{ position: "absolute", inset: 0, background: "rgba(0,0,0,.5)", backdropFilter: blur ? "blur(1px)" : undefined,
        display: "flex", alignItems: children.props.align === "top" ? "flex-start" : "center", justifyContent: "center",
        paddingTop: children.props.align === "top" ? 40 : 0 }}>
        {children}
      </div>
    </div>
  );
}

function PaletteFrame() {
  const items = [
    { label: "New Terminal", sc: "Ctrl+T", icon: ic.term, active: true },
    { label: "Split Pane Vertical", sc: "Ctrl+D", icon: ic.split },
    { label: "Toggle Theme (Mocha / Latte)", sc: "", icon: <Icon name="theme" /> },
    { label: "Settings", sc: "Ctrl+,", icon: ic.settings },
  ];
  return (
    <div align="top" style={{ width: 480, background: "var(--tasty-surface-raised)", border: "1px solid var(--tasty-border-strong)",
      borderRadius: "var(--tasty-radius)", overflow: "hidden", boxShadow: "0 16px 48px rgba(0,0,0,.5)" }}>
      <div style={{ padding: 10, borderBottom: "1px solid var(--tasty-separator)" }}>
        <Input block icon={ic.search} placeholder="Type to search commands…" defaultValue="" />
      </div>
      <div style={{ padding: 6 }}>
        {items.map((i) => (
          <MenuItem key={i.label} label={i.label} icon={i.icon} active={i.active}
            shortcut={i.sc ? <Kbd keys={i.sc} /> : null} />
        ))}
      </div>
      <div style={{ display: "flex", gap: 14, padding: "8px 12px", borderTop: "1px solid var(--tasty-separator)",
        fontFamily: "var(--tasty-font-mono)", fontSize: 10.5, color: "var(--tasty-text-muted)" }}>
        <span>↑↓ navigate</span><span>↵ run</span><span>esc close</span>
      </div>
    </div>
  );
}

function ApprovalFrame() {
  return (
    <div style={{ width: 440, background: "var(--tasty-bg-panel)", border: "1px solid var(--tasty-border-strong)",
      borderRadius: "var(--tasty-radius)", overflow: "hidden", boxShadow: "0 20px 60px rgba(0,0,0,.55)" }}>
      <div style={{ display: "flex", alignItems: "center", gap: 8, padding: "12px 14px", borderBottom: "1px solid var(--tasty-separator)" }}>
        <span style={{ width: 7, height: 7, borderRadius: "50%", background: "var(--tasty-accent-agent)" }} />
        <span style={{ fontSize: 14, fontWeight: 600 }}>Approve agent action</span>
        <Tag variant="agent" style={{ marginLeft: "auto" }}>agent</Tag>
      </div>
      <div style={{ padding: 14, display: "flex", flexDirection: "column", gap: 10 }}>
        <p style={{ margin: 0, fontSize: 13, color: "var(--tasty-text-secondary)", lineHeight: 1.5 }}>
          The agent <b style={{ color: "var(--tasty-text-primary)" }}>ai-review</b> wants to run a command in <span className="ic">s_01HXK9</span>:
        </p>
        <pre style={{ margin: 0, background: "#000", border: "1px solid var(--tasty-separator)", borderRadius: "var(--tasty-radius)",
          padding: "10px 12px", fontFamily: "var(--tasty-font-mono)", fontSize: 12.5, color: "var(--tasty-color-green)", overflow: "auto" }}>
git push --force origin main</pre>
        <div style={{ display: "flex", gap: 6, flexWrap: "wrap" }}>
          <Tag variant="danger" dot>destructive</Tag><Tag>fs:write</Tag><Tag>net</Tag>
        </div>
      </div>
      <div style={{ display: "flex", justifyContent: "flex-end", gap: 8, padding: "10px 14px", borderTop: "1px solid var(--tasty-separator)" }}>
        <Button variant="ghost">Deny</Button>
        <Button variant="secondary">Allow once</Button>
        <Button variant="agent">Always allow</Button>
      </div>
    </div>
  );
}

function RenameFrame() {
  return (
    <div style={{ width: 360, background: "var(--tasty-bg-panel)", border: "1px solid var(--tasty-border-strong)",
      borderRadius: "var(--tasty-radius)", overflow: "hidden", boxShadow: "0 20px 60px rgba(0,0,0,.55)" }}>
      <div style={{ padding: "14px 14px 0" }}>
        <div style={{ fontSize: 14, fontWeight: 600, marginBottom: 2 }}>Rename workspace</div>
        <div style={{ fontSize: 12, color: "var(--tasty-text-muted)", marginBottom: 12 }}>Press <Kbd keys="↵" /> to confirm, <Kbd keys="Esc" /> to cancel.</div>
        <Input block autoFocus defaultValue="tasty-core" />
      </div>
      <div style={{ display: "flex", justifyContent: "flex-end", gap: 8, padding: 14 }}>
        <Button variant="ghost">Cancel</Button>
        <Button variant="primary">Rename</Button>
      </div>
    </div>
  );
}

function SettingsFrame() {
  const L1 = ["General", "Appearance", "Keybindings", "Plugins"];
  const L2 = ["Theme", "General", "Terminal"];
  return (
    <div style={{ width: 620, height: 380, display: "flex", flexDirection: "column", background: "var(--tasty-bg-panel)",
      border: "1px solid var(--tasty-border-strong)", borderRadius: "var(--tasty-radius)", overflow: "hidden", boxShadow: "0 20px 60px rgba(0,0,0,.55)" }}>
      {/* L1 top tabs */}
      <div style={{ display: "flex", alignItems: "center", height: 44, flex: "none", padding: "0 12px", gap: 2,
        borderBottom: "1px solid var(--tasty-separator)", background: "var(--tasty-bg-sidebar)" }}>
        <span style={{ fontSize: 14, fontWeight: 700 }}>Settings</span>
        <span style={{ width: 1, height: 20, background: "var(--tasty-separator)", margin: "0 14px 0 6px" }} />
        {L1.map((t, i) => (
          <span key={t} style={{ height: 43, padding: "0 13px", display: "inline-flex", alignItems: "center", fontSize: 13,
            color: i === 1 ? "var(--tasty-text-primary)" : "var(--tasty-text-muted)", fontWeight: i === 1 ? 600 : 400,
            borderBottom: i === 1 ? "2px solid var(--tasty-accent-primary)" : "2px solid transparent" }}>{t}</span>
        ))}
      </div>
      <div style={{ display: "flex", flex: 1, minHeight: 0 }}>
        {/* L2 sidebar */}
        <div style={{ width: 168, flex: "none", background: "var(--tasty-bg-sidebar)", borderRight: "1px solid var(--tasty-separator)" }}>
          <div style={{ padding: 8, borderBottom: "1px solid var(--tasty-separator)" }}><Input block icon={ic.search} placeholder="Filter…" /></div>
          <div style={{ padding: 6 }}>
            {L2.map((s, i) => (
              <div key={s} style={{ padding: "5px 8px", borderRadius: "var(--tasty-radius-sm)", fontSize: 13,
                color: i === 0 ? "var(--tasty-text-primary)" : "var(--tasty-text-muted)", background: i === 0 ? "var(--tasty-surface-active)" : "transparent" }}>{s}</div>
            ))}
          </div>
        </div>
        {/* content */}
        <div style={{ flex: 1, display: "flex", flexDirection: "column", minWidth: 0 }}>
          <div style={{ flex: 1, padding: 18, display: "flex", flexDirection: "column", gap: 12 }}>
            <div style={{ fontFamily: "var(--tasty-font-mono)", fontSize: 10, textTransform: "uppercase", letterSpacing: ".06em", color: "var(--tasty-text-muted)" }}>Theme preset</div>
            <div style={{ display: "grid", gridTemplateColumns: "1fr 1fr", gap: 10 }}>
              {[["Catppuccin Mocha", ["#11111b", "#1e1e2e", "#89b4fa", "#cba6f7", "#a6e3a1"], true],
                ["Catppuccin Latte", ["#dce0e8", "#eff1f5", "#1e66f5", "#8839ef", "#40a02b"], false]].map(([label, cols, on]) => (
                <div key={label} style={{ border: on ? "1px solid var(--tasty-accent-primary)" : "1px solid var(--tasty-border-default)",
                  borderRadius: "var(--tasty-radius)", overflow: "hidden", boxShadow: on ? "0 0 0 1px var(--tasty-accent-primary)" : "none" }}>
                  <div style={{ display: "flex", height: 34 }}>{cols.map((c, i) => <div key={i} style={{ flex: 1, background: c }} />)}</div>
                  <div style={{ padding: "6px 9px", background: "var(--tasty-bg-panel)", fontSize: 12 }}>{label}</div>
                </div>
              ))}
            </div>
            <p style={{ fontSize: 12, color: "var(--tasty-text-muted)", margin: 0, lineHeight: 1.5 }}>Selecting a preset resets all surface colors.</p>
          </div>
          <div style={{ display: "flex", justifyContent: "flex-end", gap: 8, padding: "10px 14px", borderTop: "1px solid var(--tasty-separator)" }}>
            <Button variant="ghost">Cancel</Button><Button variant="primary">Save</Button>
          </div>
        </div>
      </div>
    </div>
  );
}

// ── Settings › General › Overlay — L2 subtab + toast duration DragValue ──
function ToastDragValue({ state = "rest", v = "2.0 s" }) {
  const border = state === "editing" ? "1px solid var(--tasty-accent-primary)" : state === "hover" ? "1px solid var(--tasty-border-strong)" : "1px solid var(--tasty-border-default)";
  return (
    <span style={{ display: "inline-flex", alignItems: "center", height: 24, padding: "0 10px", fontFamily: "var(--tasty-font-mono)", fontSize: 13,
      background: state === "hover" ? "var(--tasty-surface-hover)" : "var(--tasty-bg-app)", border, borderRadius: "var(--tasty-radius-sm)",
      cursor: "ew-resize", color: "var(--tasty-text-primary)", userSelect: "none" }}>
      {state === "editing" ? <span style={{ background: "var(--tasty-surface-active)" }}>2.0</span> : v}{state === "editing" ? <span>&nbsp;s</span> : null}
    </span>
  );
}

function SettingsGeneralOverlayFrame() {
  const L1 = ["General", "Appearance", "Keybindings", "Plugins"];
  const L2 = ["General", "Notifications", "Accessibility", "Overlay"];
  const active = 3;
  return (
    <div style={{ width: 620, height: 380, display: "flex", flexDirection: "column", background: "var(--tasty-bg-panel)",
      border: "1px solid var(--tasty-border-strong)", borderRadius: "var(--tasty-radius)", overflow: "hidden", boxShadow: "0 20px 60px rgba(0,0,0,.55)" }}>
      <div style={{ display: "flex", alignItems: "center", height: 44, flex: "none", padding: "0 12px", gap: 2,
        borderBottom: "1px solid var(--tasty-separator)", background: "var(--tasty-bg-sidebar)" }}>
        <span style={{ fontSize: 14, fontWeight: 700 }}>Settings</span>
        <span style={{ width: 1, height: 20, background: "var(--tasty-separator)", margin: "0 14px 0 6px" }} />
        {L1.map((t, i) => (
          <span key={t} style={{ height: 43, padding: "0 13px", display: "inline-flex", alignItems: "center", fontSize: 13,
            color: i === 0 ? "var(--tasty-text-primary)" : "var(--tasty-text-muted)", fontWeight: i === 0 ? 600 : 400,
            borderBottom: i === 0 ? "2px solid var(--tasty-accent-primary)" : "2px solid transparent" }}>{t}</span>
        ))}
      </div>
      <div style={{ display: "flex", flex: 1, minHeight: 0 }}>
        <div style={{ width: 168, flex: "none", background: "var(--tasty-bg-sidebar)", borderRight: "1px solid var(--tasty-separator)" }}>
          <div style={{ padding: 8, borderBottom: "1px solid var(--tasty-separator)" }}><Input block icon={ic.search} placeholder="Filter…" /></div>
          <div style={{ padding: 6 }}>
            {L2.map((s, i) => (
              <div key={s} style={{ padding: "5px 8px", borderRadius: "var(--tasty-radius-sm)", fontSize: 13,
                color: i === active ? "var(--tasty-text-primary)" : "var(--tasty-text-muted)", background: i === active ? "var(--tasty-surface-active)" : "transparent" }}>{s}</div>
            ))}
          </div>
        </div>
        <div style={{ flex: 1, display: "flex", flexDirection: "column", minWidth: 0 }}>
          <div style={{ flex: 1, padding: 18, display: "flex", flexDirection: "column", gap: 12 }}>
            <div style={{ fontFamily: "var(--tasty-font-mono)", fontSize: 10, textTransform: "uppercase", letterSpacing: ".06em", color: "var(--tasty-text-muted)" }}>Toast</div>
            <div style={{ display: "grid", gridTemplateColumns: "180px 1fr", alignItems: "center" }}>
              <span style={{ fontSize: 13 }}>Toast duration</span>
              <span><ToastDragValue /></span>
            </div>
            <p style={{ fontSize: 12, color: "var(--tasty-text-muted)", margin: 0, lineHeight: 1.5 }}>How long a toast stays on screen before it auto-dismisses. Drag left–right to adjust, or click to type a value.</p>
          </div>
          <div style={{ display: "flex", justifyContent: "flex-end", gap: 8, padding: "10px 14px", borderTop: "1px solid var(--tasty-separator)" }}>
            <Button variant="ghost">Cancel</Button><Button variant="primary">Save</Button>
          </div>
        </div>
      </div>
    </div>
  );
}

// ── Settings › General › Remote transfer — 5th L2 subtab (dir + max_mb) ──
function SettingsRemoteTransferFrame() {
  const L1 = ["General", "Appearance", "Keybindings", "Plugins"];
  const L2 = ["General", "Notifications", "Accessibility", "Overlay", "Remote transfer"];
  const active = 4;
  return (
    <div style={{ width: 620, height: 380, display: "flex", flexDirection: "column", background: "var(--tasty-bg-panel)",
      border: "1px solid var(--tasty-border-strong)", borderRadius: "var(--tasty-radius)", overflow: "hidden", boxShadow: "0 20px 60px rgba(0,0,0,.55)" }}>
      <div style={{ display: "flex", alignItems: "center", height: 44, flex: "none", padding: "0 12px", gap: 2,
        borderBottom: "1px solid var(--tasty-separator)", background: "var(--tasty-bg-sidebar)" }}>
        <span style={{ fontSize: 14, fontWeight: 700 }}>Settings</span>
        <span style={{ width: 1, height: 20, background: "var(--tasty-separator)", margin: "0 14px 0 6px" }} />
        {L1.map((t, i) => (
          <span key={t} style={{ height: 43, padding: "0 13px", display: "inline-flex", alignItems: "center", fontSize: 13,
            color: i === 0 ? "var(--tasty-text-primary)" : "var(--tasty-text-muted)", fontWeight: i === 0 ? 600 : 400,
            borderBottom: i === 0 ? "2px solid var(--tasty-accent-primary)" : "2px solid transparent" }}>{t}</span>
        ))}
      </div>
      <div style={{ display: "flex", flex: 1, minHeight: 0 }}>
        <div style={{ width: 168, flex: "none", background: "var(--tasty-bg-sidebar)", borderRight: "1px solid var(--tasty-separator)" }}>
          <div style={{ padding: 8, borderBottom: "1px solid var(--tasty-separator)" }}><Input block icon={ic.search} placeholder="Filter…" /></div>
          <div style={{ padding: 6 }}>
            {L2.map((s, i) => (
              <div key={s} style={{ padding: "5px 8px", borderRadius: "var(--tasty-radius-sm)", fontSize: 13,
                color: i === active ? "var(--tasty-text-primary)" : "var(--tasty-text-muted)", background: i === active ? "var(--tasty-surface-active)" : "transparent" }}>{s}</div>
            ))}
          </div>
        </div>
        <div style={{ flex: 1, display: "flex", flexDirection: "column", minWidth: 0 }}>
          <div style={{ flex: 1, padding: 18, display: "flex", flexDirection: "column", gap: 10 }}>
            <div style={{ fontFamily: "var(--tasty-font-mono)", fontSize: 10, textTransform: "uppercase", letterSpacing: ".06em", color: "var(--tasty-text-muted)" }}>Received files</div>
            <div style={{ display: "grid", gridTemplateColumns: "150px 1fr", alignItems: "center", gap: 12, minHeight: "var(--tasty-settings-row-min-height)" }}>
              <span style={{ fontSize: 13 }}>Save folder</span>
              <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
                <Input block mono defaultValue="~/.tasty/transfers/" style={{ flex: 1, minWidth: 0 }} />
                <Button variant="secondary" size="sm" leadingIcon={ic.folder} style={{ flex: "none" }}>Browse…</Button>
              </div>
            </div>
            <p style={{ fontSize: 12, color: "var(--tasty-text-muted)", margin: 0, lineHeight: 1.5 }}>Where files received from a remote workspace are saved.</p>
            <div style={{ borderTop: "1px solid var(--tasty-separator)" }} />
            <div style={{ display: "grid", gridTemplateColumns: "150px 1fr", alignItems: "center", gap: 12, minHeight: "var(--tasty-settings-row-min-height)" }}>
              <span style={{ fontSize: 13 }}>Maximum size</span>
              <span style={{ display: "inline-flex", alignItems: "center", gap: 8 }}>
                <Input mono defaultValue="500" style={{ width: 88 }} />
                <span style={{ fontFamily: "var(--tasty-font-mono)", fontSize: 12, color: "var(--tasty-text-muted)" }}>MiB</span>
              </span>
            </div>
            <p style={{ fontSize: 12, color: "var(--tasty-text-muted)", margin: 0, lineHeight: 1.5 }}>Total the folder may hold. A transfer that would push it past this limit is rejected before it starts.</p>
          </div>
          <div style={{ display: "flex", justifyContent: "flex-end", gap: 8, padding: "10px 14px", borderTop: "1px solid var(--tasty-separator)" }}>
            <Button variant="ghost">Cancel</Button><Button variant="primary">Save</Button>
          </div>
        </div>
      </div>
    </div>
  );
}

// ── Remote transfer — progress popup (PopupDef, auto-closes on completion) ──
function TransferProgressFrame({ name = "sprint-42-demo.mp4", pct = 27, done = "34.6 MiB", total = "128.0 MiB", rate = "2.1 MiB/s" }) {
  return (
    <div style={{ width: "var(--tasty-transfer-popup-width)", background: "var(--tasty-bg-panel)", border: "1px solid var(--tasty-border-strong)",
      borderRadius: "var(--tasty-radius)", overflow: "hidden", boxShadow: "0 20px 60px rgba(0,0,0,.55)" }}>
      <div style={{ display: "flex", alignItems: "center", gap: 8, padding: "12px 14px", borderBottom: "1px solid var(--tasty-separator)" }}>
        <span style={{ display: "inline-flex", color: "var(--tasty-text-muted)" }}>{ic.download}</span>
        <span style={{ fontSize: 14, fontWeight: 600 }}>Receiving file</span>
        <span style={{ marginLeft: "auto", fontFamily: "var(--tasty-font-mono)", fontSize: 12, color: "var(--tasty-text-muted)" }}>{pct}%</span>
      </div>
      <div style={{ padding: 14, display: "flex", flexDirection: "column", gap: 10 }}>
        <div style={{ display: "flex", alignItems: "center", gap: 8, minWidth: 0 }}>
          <span style={{ display: "inline-flex", flex: "none", color: "var(--tasty-text-muted)" }}>{ic.file}</span>
          <span style={{ flex: 1, minWidth: 0, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap",
            fontFamily: "var(--tasty-font-mono)", fontSize: 13, color: "var(--tasty-text-primary)" }}>{name}</span>
        </div>
        <div style={{ height: "var(--tasty-progress-height)", background: "var(--tasty-progress-track-bg)", borderRadius: "var(--tasty-progress-radius)", overflow: "hidden" }}>
          <div style={{ width: pct + "%", height: "100%", background: "var(--tasty-progress-fill-bg)" }} />
        </div>
        <div style={{ display: "flex", justifyContent: "space-between", fontFamily: "var(--tasty-font-mono)", fontSize: 11.5, color: "var(--tasty-text-muted)" }}>
          <span>{done} / {total}</span><span>{rate}</span>
        </div>
      </div>
      <div style={{ display: "flex", justifyContent: "flex-end", padding: "10px 14px", borderTop: "1px solid var(--tasty-separator)" }}>
        <Button variant="ghost">Cancel</Button>
      </div>
    </div>
  );
}

// ── Remote transfer — error / rejected popup ──
function TransferErrorFrame({ retry = false, name = "sprint-42-demo.mp4", reason = "capacity exceeded — transfers folder is at its 500 MiB limit" }) {
  return (
    <div style={{ width: "var(--tasty-transfer-popup-width)", background: "var(--tasty-bg-panel)", border: "1px solid var(--tasty-border-strong)",
      borderRadius: "var(--tasty-radius)", overflow: "hidden", boxShadow: "0 20px 60px rgba(0,0,0,.55)" }}>
      <div style={{ display: "flex", alignItems: "center", gap: 8, padding: "12px 14px", borderBottom: "1px solid var(--tasty-separator)" }}>
        <span style={{ display: "inline-flex", color: "var(--tasty-accent-danger)" }}>{ic.warn}</span>
        <span style={{ fontSize: 14, fontWeight: 600 }}>Transfer failed</span>
      </div>
      <div style={{ padding: 14, display: "flex", flexDirection: "column", gap: 10 }}>
        <p style={{ margin: 0, fontSize: 13, color: "var(--tasty-text-secondary)", lineHeight: 1.5 }}>
          <b style={{ color: "var(--tasty-text-primary)", fontFamily: "var(--tasty-font-mono)", fontWeight: 600 }}>{name}</b> could not be received.
        </p>
        <div style={{ background: "var(--tasty-bg-app)", border: "1px solid var(--tasty-separator)", borderRadius: "var(--tasty-radius)",
          padding: "8px 10px", fontFamily: "var(--tasty-font-mono)", fontSize: 12, color: "var(--tasty-accent-danger)" }}>{reason}</div>
      </div>
      <div style={{ display: "flex", justifyContent: "flex-end", gap: 8, padding: "10px 14px", borderTop: "1px solid var(--tasty-separator)" }}>
        {retry ? <Button variant="ghost">Dismiss</Button> : null}
        {retry ? <Button variant="secondary">Retry</Button> : <Button variant="secondary">Dismiss</Button>}
      </div>
    </div>
  );
}

// ── Tools menu — 160px, anchored above the sidebar Tools button, no scrim ──
function ToolsMenuFrame() {
  const builtin = ["Command palette…", "Listening ports...", "Remote connections…", "Presets"];
  const plugin = ["Clipboard Viewer", "Git"];
  return (
    <div style={{ position: "relative", width: "100%", height: 320, borderRadius: "var(--tasty-radius)", overflow: "hidden",
      border: "1px solid var(--tasty-border-default)", background: "var(--tasty-bg-app)" }}>
      {/* faux sidebar with a Tools button at the bottom */}
      <div style={{ position: "absolute", top: 0, bottom: 0, left: 0, width: 150, background: "var(--tasty-bg-sidebar)",
        borderRight: "1px solid var(--tasty-separator)", display: "flex", flexDirection: "column", justifyContent: "flex-end", padding: 8 }}>
        <Button variant="ghost" size="sm" block leadingIcon={ic.tools} style={{ justifyContent: "flex-start", background: "var(--tasty-surface-active)" }}>Tools</Button>
      </div>
      {/* the menu, anchored above the button */}
      <div role="menu" aria-label="Tools" style={{ position: "absolute", left: 12, bottom: 52, width: 160,
        background: "var(--tasty-surface-raised)", border: "1px solid var(--tasty-border-strong)",
        borderRadius: "var(--tasty-radius)", padding: 6, boxShadow: "0 8px 28px rgba(0,0,0,.4)" }}>
        {builtin.map((l) => <MenuItem key={l} label={l} />)}
        <MenuItem separator />
        {plugin.map((l) => <MenuItem key={l} label={l} />)}
      </div>
    </div>
  );
}

// ── Listening ports — favorites section (design SoT: the popup's only WRITING
// control). Star = accent-warning gold when registered, muted outline when not;
// identity is (addr, port). The section is bounded — caption 22 + list capped at
// --tasty-port-favorites-max-height (112 = 5 rows) — and ignores the search box,
// the scope checkbox and the sort: it always reports the FULL system scan.
function PortStarG({ on }) {
  return (
    <span style={{ display: "inline-flex", alignItems: "center", justifyContent: "center", flex: "none",
      width: "var(--tasty-control-height-tree)", height: "var(--tasty-control-height-tree)", borderRadius: "var(--tasty-radius-sm)",
      color: on ? "var(--tasty-port-star-on)" : "var(--tasty-port-star-off)" }}>
      <Icon name={on ? "starFill" : "star"} size="var(--tasty-icon-size-sm)" />
    </span>
  );
}

function PortsFavoritesG({ favorites = "mixed" }) {
  const sets = {
    mixed: [["127.0.0.1:5173", "vite · 48990 · Project A", "LISTEN"],
            ["0.0.0.0:8080", "tasty-agent · 50321 · Project B", "LISTEN"],
            ["127.0.0.1:5432", "postgres · 1192", "LISTEN"],
            ["127.0.0.1:9999", "not running", "NONE"]],
    scrolling: [["127.0.0.1:3000", "node · 48213 · Project A", "LISTEN"],
            ["127.0.0.1:5173", "vite · 48990 · Project A", "LISTEN"],
            ["127.0.0.1:5432", "postgres · 1192", "LISTEN"],
            ["127.0.0.1:6379", "redis-server · 1456", "LISTEN"],
            ["0.0.0.0:8080", "tasty-agent · 50321 · Project B", "LISTEN"],
            ["127.0.0.1:9229", "node · 48213 · Project A", "CLOSE_WAIT"],
            ["127.0.0.1:9999", "not running", "NONE"]],
    empty: [],
  };
  const rows = sets[favorites] || sets.mixed;
  const cap = { fontFamily: "var(--tasty-font-mono)", fontSize: "var(--tasty-font-size-micro)", color: "var(--tasty-text-muted)" };
  return (
    <div style={{ flex: "none", background: "var(--tasty-port-favorites-bg)",
      borderBottom: "1px solid var(--tasty-port-favorites-border)" }}>
      <div style={{ display: "flex", alignItems: "center", height: "var(--tasty-control-height-tree)", padding: "0 14px" }}>
        <span style={{ ...cap, textTransform: "uppercase", letterSpacing: "var(--tasty-letter-spacing-caps)" }}>
          Favorites{rows.length > 0 && ` · ${rows.length}`}
        </span>
        <div style={{ flex: 1 }} />
        <span style={{ ...cap, color: "var(--tasty-text-placeholder)" }}>system-wide</span>
      </div>
      {rows.length === 0 ? (
        <div style={{ display: "flex", alignItems: "center", gap: 4, height: "var(--tasty-control-height-tree)", padding: "0 14px", color: "var(--tasty-text-muted)" }}>
          <span style={{ display: "inline-flex", flex: "none", opacity: 0.55 }}><Icon name="star" size="var(--tasty-icon-size-xs)" /></span>
          <span style={{ fontSize: 12 }}>No favorites yet</span>
          <span style={{ fontSize: 12, color: "var(--tasty-text-placeholder)" }}>— click a star in the list below to pin a port.</span>
        </div>
      ) : (
        <div className="tasty-scroll" style={{ maxHeight: "var(--tasty-port-favorites-max-height)", overflowY: "auto" }}>
          {rows.map(([addr, meta, state]) => (
            <div key={addr} style={{ display: "flex", alignItems: "center", height: "var(--tasty-port-favorites-row-height)", padding: "0 14px 0 0" }}>
              <span style={{ width: "var(--tasty-port-star-col-width)", display: "inline-flex", justifyContent: "center", flex: "none" }}>
                <PortStarG on />
              </span>
              <span style={{ fontFamily: "var(--tasty-font-mono)", fontSize: 12, color: "var(--tasty-text-primary)", flex: "none" }}>{addr}</span>
              <span style={{ flex: 1, minWidth: 12, paddingLeft: 12, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap", fontSize: 12, color: "var(--tasty-text-muted)" }}>{meta}</span>
              <span style={{ flex: "none", display: "inline-flex", justifyContent: "flex-end", minWidth: 112 }}>
                {state === "NONE"
                  ? <StatusDot status="idle" label="NONE" />
                  : <StatusDot status={state === "LISTEN" ? "running" : "waiting"} pulse={state === "LISTEN"} label={state} />}
              </span>
            </div>
          ))}
        </div>
      )}
    </div>
  );
}

// ── Listening ports — 660×520 table popup built on the Table component ──
function PortsFrame({ favorites = "mixed" }) {
  const Dash = () => <span style={{ color: "var(--tasty-text-muted)" }}>—</span>;
  const rows = [
    { port: 3000, proto: "tcp", addr: "127.0.0.1", proc: "node", pid: 48213, ws: "Project A", tab: "server", state: "LISTEN" },
    { port: 5173, proto: "tcp", addr: "127.0.0.1", proc: "vite", pid: 48990, ws: "Project A", tab: "dev", state: "LISTEN" },
    { port: 8080, proto: "tcp", addr: "0.0.0.0", proc: "tasty-agent", pid: 50321, ws: "Project B", tab: "agent", state: "LISTEN" },
    { port: 8443, proto: "tcp6", addr: "::", proc: "tasty-agent", pid: 50321, ws: "Project B", tab: "agent", state: "LISTEN" },
    { port: 9229, proto: "tcp", addr: "127.0.0.1", proc: "node", pid: 48213, ws: "Project A", tab: "server", state: "CLOSE_WAIT" },
  ];
  const columns = [
    { key: "fav", header: "", tight: true, width: "var(--tasty-port-star-col-width)",
      render: (_v, row) => <PortStarG on={row.port === 5173 || row.port === 8080} /> },
    { key: "port", header: "Port", align: "right", mono: true, sortable: true, width: 72 },
    { key: "proto", header: "Proto", mono: true, width: 64 },
    { key: "addr", header: "Address", mono: true, sortable: true },
    { key: "proc", header: "Process", strong: true, sortable: true,
      render: (v, row) => (<span style={{ display: "inline-flex", alignItems: "center", gap: 8 }}>{v}<Tag>{row.pid}</Tag></span>) },
    { key: "ws", header: "Workspace", width: 104, render: (v) => v || <Dash /> },
    { key: "state", header: "State", width: 132,
      render: (v) => <StatusDot status={v === "LISTEN" ? "running" : "waiting"} pulse={v === "LISTEN"} label={v} /> },
  ];
  return (
    <div style={{ width: "100%", maxWidth: 640, height: 460, display: "flex", flexDirection: "column", background: "var(--tasty-bg-panel)",
      border: "1px solid var(--tasty-border-strong)", borderRadius: "var(--tasty-radius)", overflow: "hidden", boxShadow: "0 20px 60px rgba(0,0,0,.55)" }}>
      <div style={{ display: "flex", alignItems: "center", gap: 8, padding: "10px 14px", flex: "none", borderBottom: "1px solid var(--tasty-separator)" }}>
        <span style={{ display: "inline-flex", color: "var(--tasty-text-muted)" }}>{ic.port}</span>
        <span style={{ fontSize: 14, fontWeight: 600 }}>Listening ports</span>
        <Tag variant="accent">5 listening</Tag>
        <div style={{ flex: 1 }} />
        <Input icon={ic.search} placeholder="Filter…" style={{ width: 160 }} />
        <IconButton aria-label="Refresh">{ic.refresh}</IconButton>
        <IconButton aria-label="Close">{ic.x}</IconButton>
      </div>
      <div style={{ display: "flex", alignItems: "center", padding: "8px 14px", flex: "none", borderBottom: "1px solid var(--tasty-separator)" }}>
        <Checkbox label="Show all (system-wide)" />
      </div>
      <PortsFavoritesG favorites={favorites} />
      <div style={{ overflow: "auto", flex: 1, minHeight: 0 }}>
        <Table columns={columns} rows={rows} rowKey="port" selectedKey={8080} />
      </div>
      <div style={{ display: "flex", alignItems: "center", gap: 12, padding: "8px 14px", flex: "none", borderTop: "1px solid var(--tasty-separator)" }}>
        <span style={{ fontFamily: "var(--tasty-font-mono)", fontSize: 11, color: "var(--tasty-text-muted)" }}>5 of 5 ports</span>
        <div style={{ flex: 1 }} />
        <Button variant="ghost">Copy address</Button>
        <Button variant="secondary">Close</Button>
      </div>
    </div>
  );
}

// ── Remote connections — 520×460, three top tabs, list view ──
function RemoteFrame({ tab = "profiles" }) {
  const Caption = ({ children }) => <span style={{ fontSize: 11, color: "var(--tasty-text-muted)", fontFamily: "var(--tasty-font-mono)" }}>{children}</span>;
  const ProfileRow = ({ name, label, type, target, passkey, detecting }) => (
    <div style={{ display: "flex", alignItems: "flex-start", gap: 8, padding: "10px 4px", borderBottom: "1px solid var(--tasty-separator)" }}>
      <div style={{ flex: 1, minWidth: 0, display: "flex", flexDirection: "column", gap: 2 }}>
        <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
          <span style={{ fontSize: 13, fontWeight: 600 }}>{name}{label && <span style={{ fontWeight: 400, color: "var(--tasty-text-muted)" }}>  ({label})</span>}</span>
          <Tag>{type}</Tag>
        </div>
        <span style={{ fontFamily: "var(--tasty-font-mono)", fontSize: 11, color: "var(--tasty-text-muted)" }}>{target}</span>
        <div style={{ display: "flex", alignItems: "center", gap: 10 }}>
          <Caption>passkey: {passkey || "—"}</Caption>
          {detecting && <span style={{ display: "inline-flex", alignItems: "center", gap: 4, fontSize: 11, color: "var(--tasty-text-muted)" }}><Spinner size={12} /> detecting…</span>}
        </div>
      </div>
      <div style={{ display: "flex", gap: 1, flex: "none" }}>
        <IconButton size="sm" aria-label="Re-detect">{ic.refresh}</IconButton>
        <IconButton size="sm" aria-label="Edit">{ic.edit}</IconButton>
        <IconButton size="sm" aria-label="Delete">{ic.x}</IconButton>
      </div>
    </div>
  );
  const AttachRow = ({ name, label, mode, target, tasty, port, inactive, missing }) => (
    <div style={{ display: "flex", alignItems: "flex-start", gap: 8, padding: "10px 4px", borderBottom: "1px solid var(--tasty-separator)" }}>
      <div style={{ flex: 1, minWidth: 0, display: "flex", flexDirection: "column", gap: 2 }}>
        <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
          <span style={{ fontSize: 13, fontWeight: 600, color: inactive ? "var(--tasty-text-disabled)" : "var(--tasty-text-primary)" }}>{name}{label && <span style={{ fontWeight: 400, color: "var(--tasty-text-muted)" }}>  ({label})</span>}</span>
          <Tag>{mode}</Tag>
          {inactive && <span style={{ display: "inline-flex", alignItems: "center", gap: 3, height: 16, padding: "0 6px", borderRadius: "var(--tasty-radius-sm)",
            fontFamily: "var(--tasty-font-mono)", fontSize: 10, fontWeight: 500, color: "var(--tasty-accent-warning)",
            border: "1px solid color-mix(in srgb, var(--tasty-accent-warning) 40%, transparent)",
            background: "color-mix(in srgb, var(--tasty-accent-warning) 12%, transparent)" }}>inactive</span>}
        </div>
        <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
          <span style={{ fontFamily: "var(--tasty-font-mono)", fontSize: 11, color: "var(--tasty-text-muted)" }}>{target}</span>
          {missing && <span style={{ fontFamily: "var(--tasty-font-mono)", fontSize: 10, color: "var(--tasty-accent-warning)" }}>profile missing</span>}
        </div>
        <div style={{ display: "flex", alignItems: "center", gap: 12 }}>
          <Caption>tasty: {tasty}</Caption><Caption>port: {port}</Caption>
        </div>
      </div>
      <div style={{ display: "flex", gap: 1, flex: "none" }}>
        <IconButton size="sm" aria-label="Edit">{ic.edit}</IconButton>
        <IconButton size="sm" aria-label="Delete">{ic.x}</IconButton>
      </div>
    </div>
  );
  const TabBtn = ({ children, on }) => (
    <span style={{ height: 35, padding: "0 12px", display: "inline-flex", alignItems: "center", fontSize: 13,
      color: on ? "var(--tasty-text-primary)" : "var(--tasty-text-muted)", fontWeight: on ? 600 : 400,
      borderBottom: on ? "2px solid var(--tasty-accent-primary)" : "2px solid transparent" }}>{children}</span>
  );
  return (
    <div style={{ width: 520, height: 440, display: "flex", flexDirection: "column", background: "var(--tasty-bg-panel)",
      border: "1px solid var(--tasty-border-strong)", borderRadius: "var(--tasty-radius)", overflow: "hidden", boxShadow: "0 20px 60px rgba(0,0,0,.55)" }}>
      <div style={{ display: "flex", alignItems: "center", gap: 8, padding: "10px 14px", flex: "none", borderBottom: "1px solid var(--tasty-separator)" }}>
        <span style={{ display: "inline-flex", color: "var(--tasty-text-muted)" }}>{ic.remote}</span>
        <span style={{ fontSize: 14, fontWeight: 600 }}>Remote connections</span>
        <div style={{ flex: 1 }} />
        <IconButton size="sm" aria-label="Close">{ic.x}</IconButton>
      </div>
      <div style={{ display: "flex", gap: 2, padding: "0 8px", flex: "none", borderBottom: "1px solid var(--tasty-separator)", background: "var(--tasty-bg-sidebar)" }}>
        <TabBtn on={tab === "profiles"}>Remote profiles</TabBtn><TabBtn on={tab === "attach"}>Attach</TabBtn><TabBtn on={tab === "passkeys"}>Passkeys</TabBtn>
      </div>
      {tab === "attach" ? (
        <>
          <div style={{ display: "flex", alignItems: "center", padding: "10px 14px 6px", flex: "none" }}>
            <Button variant="secondary" size="sm" leadingIcon={ic.plus}>Add attach</Button>
          </div>
          <div style={{ flex: 1, minHeight: 0, overflow: "auto", padding: "0 14px 8px" }}>
            <AttachRow name="gb10" label="us-east" mode="profile" target="→ prod-web" tasty="tasty" port="auto" />
            <AttachRow name="edge-direct" mode="inline" target="root@edge.example.com" tasty="/opt/tasty/bin/tasty" port="file-unix" />
            <AttachRow name="legacy-attach" mode="profile" target="→ legacy-box" tasty="tasty" port="subcommand" inactive />
          </div>
        </>
      ) : (
      <>
      <div style={{ display: "flex", alignItems: "center", justifyContent: "space-between", padding: "10px 14px 6px", flex: "none" }}>
        <Button variant="secondary" size="sm" leadingIcon={ic.plus}>Add profile</Button>
        <div style={{ position: "relative" }}>
          <Button variant="primary" size="sm" leadingIcon={ic.funnel}>Filter · 3/4</Button>
          <div role="dialog" aria-label="Filter by protocol" style={{ position: "absolute", top: "calc(100% + 4px)", right: 0, zIndex: 5, width: 236,
            background: "var(--tasty-surface-raised)", border: "1px solid var(--tasty-border-strong)", borderRadius: "var(--tasty-radius)",
            boxShadow: "0 8px 28px rgba(0,0,0,.4)", overflow: "hidden" }}>
            <div style={{ padding: "8px 12px", borderBottom: "1px solid var(--tasty-separator)", fontFamily: "var(--tasty-font-mono)",
              fontSize: 10, textTransform: "uppercase", letterSpacing: ".06em", color: "var(--tasty-text-muted)" }}>Filter by protocol</div>
            <div style={{ padding: "8px 12px", display: "flex", flexDirection: "column", gap: 8 }}>
              {[["ssh", true, false], ["smb", true, false], ["http", true, false], ["snb", false, true]].map(([p, on, unknown]) => (
                <div key={p} style={{ display: "flex", alignItems: "center", gap: 8 }}>
                  <Checkbox checked={on} readOnly label={<span style={{ fontFamily: "var(--tasty-font-mono)", fontSize: 12 }}>{p}</span>} />
                  {unknown && <span style={{ display: "inline-flex", alignItems: "center", gap: 3, height: 16, padding: "0 6px", borderRadius: "var(--tasty-radius-sm)",
                    fontFamily: "var(--tasty-font-mono)", fontSize: 10, fontWeight: 500, color: "var(--tasty-accent-warning)",
                    border: "1px solid color-mix(in srgb, var(--tasty-accent-warning) 40%, transparent)",
                    background: "color-mix(in srgb, var(--tasty-accent-warning) 12%, transparent)" }}>unknown</span>}
                </div>
              ))}
            </div>
            <div style={{ display: "flex", alignItems: "center", gap: 8, padding: "8px 12px", borderTop: "1px solid var(--tasty-separator)" }}>
              <span style={{ fontSize: 11, color: "var(--tasty-accent-primary)" }}>Select all</span>
              <span style={{ color: "var(--tasty-separator)" }}>·</span>
              <span style={{ fontSize: 11, color: "var(--tasty-accent-primary)" }}>Deselect all</span>
            </div>
            <div style={{ display: "flex", alignItems: "center", justifyContent: "space-between", padding: "8px 12px", borderTop: "1px solid var(--tasty-separator)" }}>
              <Button variant="ghost" size="sm">Reset</Button>
              <Button variant="primary" size="sm">Apply</Button>
            </div>
          </div>
        </div>
      </div>
      <div style={{ flex: 1, minHeight: 0, overflow: "auto", padding: "0 14px 8px" }}>
        <ProfileRow name="prod-web" label="us-east" type="ssh" target="deploy@10.0.4.12" passkey="ed25519-main" />
        <ProfileRow name="db-primary" type="ssh" target="postgres@db.internal:2222" passkey="" />
        <ProfileRow name="edge-cache" label="staging" type="ssh" target="root@edge.example.com" passkey="edge-pem" detecting />
        <ProfileRow name="media-nas" label="lab" type="smb" target="host=nas.local  share=media" passkey="nas-cred" />
      </div>
      </>
      )}
    </div>
  );
}

// ── Search bar — 360px headless popup floating top-right of the focused surface ──
function SearchBarFrame() {
  const Toggle = ({ label, active }) => (
    <IconButton size="sm" active={active} aria-label={label}>
      <span style={{ fontFamily: "var(--tasty-font-mono)", fontSize: 11, fontWeight: 600,
        color: active ? "var(--tasty-text-primary)" : "var(--tasty-text-muted)" }}>{label}</span>
    </IconButton>
  );
  return (
    <div style={{ position: "relative", width: "100%", height: 240, borderRadius: "var(--tasty-radius)", overflow: "hidden",
      border: "1px solid var(--tasty-border-default)", background: "#000" }}>
      {/* faux terminal text */}
      <div style={{ padding: "12px 14px", fontFamily: "var(--tasty-font-mono)", fontSize: 12.5, lineHeight: 1.7, color: "var(--tasty-color-neutral-1100, #cdd6f4)", opacity: 0.85 }}>
        <div><span style={{ color: "var(--tasty-color-green)" }}>~/tasty</span> $ cargo build --release</div>
        <div>   Compiling tasty-core v0.5.0</div>
        <div>   Compiling <mark style={{ background: "var(--tasty-accent-warning)", color: "#000" }}>tasty</mark>-ui-widgets v0.5.0</div>
        <div>   Compiling <mark style={{ background: "var(--tasty-accent-primary)", color: "#fff" }}>tasty</mark>-gallery v0.5.0</div>
        <div>    Finished release [optimized] target(s)</div>
      </div>
      {/* the search bar */}
      <div role="search" style={{ position: "absolute", top: 8, right: 8, width: 360, maxWidth: "calc(100% - 16px)",
        display: "flex", alignItems: "center", gap: 4, padding: 4, background: "var(--tasty-surface-raised)",
        border: "1px solid var(--tasty-border-strong)", borderRadius: "var(--tasty-radius)", boxShadow: "0 2px 6px rgba(0,0,0,.4)" }}>
        <span style={{ flex: 1, minWidth: 60, display: "flex" }}><Input block defaultValue="tasty" /></span>
        <span style={{ flex: "none", width: 40, textAlign: "center", fontSize: 12, color: "var(--tasty-text-muted)", fontVariantNumeric: "tabular-nums" }}>2/3</span>
        <IconButton size="sm" aria-label="Previous"><Icon name="chevronUp" size={14} /></IconButton>
        <IconButton size="sm" aria-label="Next"><Icon name="chevronDown" size={14} /></IconButton>
        <Toggle label="Aa" /><Toggle label=".*" /><Toggle label="ab" active />
        <span style={{ width: 1, alignSelf: "stretch", margin: "0 2px", background: "var(--tasty-separator)" }} />
        <IconButton size="sm" aria-label="Close">{ic.x}</IconButton>
      </div>
    </div>
  );
}

// ── Convert surface — small dialog ──
function ConvertFrame() {
  return (
    <div style={{ width: 400, background: "var(--tasty-bg-panel)", border: "1px solid var(--tasty-border-strong)",
      borderRadius: "var(--tasty-radius)", overflow: "hidden", boxShadow: "0 20px 60px rgba(0,0,0,.55)" }}>
      <div style={{ padding: "14px 14px 0" }}>
        <div style={{ fontSize: 14, fontWeight: 600, marginBottom: 12 }}>Convert surface</div>
        <div style={{ display: "flex", alignItems: "center", gap: 12, marginBottom: 12 }}>
          <div style={{ display: "flex", flexDirection: "column", gap: 4 }}>
            <span style={{ fontSize: 11, color: "var(--tasty-text-muted)" }}>From</span>
            <Tag>terminal</Tag>
          </div>
          <span style={{ color: "var(--tasty-text-muted)", marginTop: 16 }}>{ic.swap}</span>
          <div style={{ display: "flex", flexDirection: "column", gap: 4, flex: 1 }}>
            <span style={{ fontSize: 11, color: "var(--tasty-text-muted)" }}>To</span>
            <Select options={["markdown", "editor", "log viewer"]} style={{ width: "100%" }} />
          </div>
        </div>
        <p style={{ margin: 0, fontSize: 11, color: "var(--tasty-text-muted)", lineHeight: 1.5 }}>The running process keeps its scrollback; only the surface renderer changes.</p>
      </div>
      <div style={{ display: "flex", justifyContent: "flex-end", gap: 8, padding: 14 }}>
        <Button variant="ghost">Cancel</Button><Button variant="primary">Convert</Button>
      </div>
    </div>
  );
}

// ── File handler picker — "Open with…" list ──
function FileHandlerFrame() {
  const handlers = [
    { name: "Markdown preview", meta: "built-in", icon: ic.md, sel: true, dflt: true },
    { name: "Text editor", meta: "built-in", icon: ic.edit, sel: false },
    { name: "Terminal (less)", meta: "built-in", icon: ic.term, sel: false },
    { name: "Git diff", meta: "plugin · git-helper", icon: <Icon name="gitBranch" />, sel: false, agent: true },
  ];
  return (
    <div style={{ width: 420, background: "var(--tasty-bg-panel)", border: "1px solid var(--tasty-border-strong)",
      borderRadius: "var(--tasty-radius)", overflow: "hidden", boxShadow: "0 20px 60px rgba(0,0,0,.55)" }}>
      <div style={{ padding: "14px 14px 10px", borderBottom: "1px solid var(--tasty-separator)" }}>
        <div style={{ fontSize: 14, fontWeight: 600, marginBottom: 4 }}>Open file with…</div>
        <span style={{ fontFamily: "var(--tasty-font-mono)", fontSize: 11, color: "var(--tasty-text-muted)" }}>docs/architecture.md</span>
      </div>
      <div style={{ padding: 6 }}>
        {handlers.map((h) => (
          <div key={h.name} style={{ display: "flex", alignItems: "center", gap: 10, padding: "8px 10px", borderRadius: "var(--tasty-radius-sm)",
            background: h.sel ? "var(--tasty-surface-active)" : "transparent",
            boxShadow: h.sel ? "inset 2px 0 0 var(--tasty-accent-primary)" : "none" }}>
            <span style={{ display: "inline-flex", color: h.agent ? "var(--tasty-accent-agent)" : "var(--tasty-text-muted)" }}>{h.icon}</span>
            <div style={{ flex: 1, minWidth: 0 }}>
              <div style={{ fontSize: 13, color: "var(--tasty-text-primary)" }}>{h.name}</div>
              <div style={{ fontSize: 11, color: "var(--tasty-text-muted)" }}>{h.meta}</div>
            </div>
            {h.dflt && <Tag variant="accent">default</Tag>}
          </div>
        ))}
      </div>
      <div style={{ display: "flex", alignItems: "center", padding: "8px 14px", borderTop: "1px solid var(--tasty-separator)" }}>
        <Checkbox label="Always open .md with this" defaultChecked />
        <div style={{ flex: 1 }} />
        <div style={{ display: "flex", gap: 8 }}><Button variant="ghost">Cancel</Button><Button variant="primary">Open</Button></div>
      </div>
    </div>
  );
}

// ── Apply preset — scope + preset list ──
function PresetFrame() {
  const Seg = ({ children, on }) => (
    <button style={{ appearance: "none", cursor: "pointer", border: 0, padding: "0 14px", height: 26,
      borderLeft: on ? 0 : "1px solid var(--tasty-border-default)", fontSize: 12,
      color: on ? "var(--tasty-text-on-accent)" : "var(--tasty-text-secondary)",
      background: on ? "var(--tasty-accent-primary)" : "transparent" }}>{children}</button>
  );
  const presets = [
    { name: "Dev split", meta: "2 panes · editor + shell", sel: true },
    { name: "Logs grid", meta: "4 panes · tail -f" },
    { name: "Single shell", meta: "1 pane · zsh" },
  ];
  return (
    <div style={{ width: 440, background: "var(--tasty-bg-panel)", border: "1px solid var(--tasty-border-strong)",
      borderRadius: "var(--tasty-radius)", overflow: "hidden", boxShadow: "0 20px 60px rgba(0,0,0,.55)" }}>
      <div style={{ padding: "14px 14px 12px", borderBottom: "1px solid var(--tasty-separator)", display: "flex", alignItems: "center", gap: 12 }}>
        <span style={{ fontSize: 14, fontWeight: 600 }}>Apply preset</span>
        <div style={{ flex: 1 }} />
        <div style={{ display: "inline-flex", border: "1px solid var(--tasty-border-default)", borderRadius: "var(--tasty-radius)", overflow: "hidden" }}>
          <Seg on>Workspace</Seg><Seg>Tab</Seg><Seg>Pane</Seg>
        </div>
      </div>
      <div style={{ padding: 6 }}>
        {presets.map((p) => (
          <div key={p.name} style={{ display: "flex", alignItems: "center", gap: 10, padding: "9px 10px", borderRadius: "var(--tasty-radius-sm)",
            background: p.sel ? "var(--tasty-surface-active)" : "transparent", boxShadow: p.sel ? "inset 2px 0 0 var(--tasty-accent-primary)" : "none" }}>
            <span style={{ display: "inline-flex", color: "var(--tasty-text-muted)" }}>{ic.layers}</span>
            <div style={{ flex: 1 }}>
              <div style={{ fontSize: 13, color: "var(--tasty-text-primary)" }}>{p.name}</div>
              <div style={{ fontSize: 11, color: "var(--tasty-text-muted)", fontFamily: "var(--tasty-font-mono)" }}>{p.meta}</div>
            </div>
          </div>
        ))}
      </div>
      <div style={{ display: "flex", justifyContent: "flex-end", gap: 8, padding: 14, borderTop: "1px solid var(--tasty-separator)" }}>
        <Button variant="ghost">Cancel</Button><Button variant="primary">Apply to workspace</Button>
      </div>
    </div>
  );
}

// ── Markdown open — confirm preview vs raw ──
function MarkdownOpenFrame() {
  const Choice = ({ icon, title, sub, on }) => (
    <button style={{ appearance: "none", cursor: "pointer", textAlign: "left", flex: 1, display: "flex", flexDirection: "column", gap: 6,
      padding: 12, borderRadius: "var(--tasty-radius)", background: "var(--tasty-surface-raised)",
      border: on ? "1px solid var(--tasty-accent-primary)" : "1px solid var(--tasty-border-default)",
      boxShadow: on ? "0 0 0 1px var(--tasty-accent-primary)" : "none" }}>
      <span style={{ display: "inline-flex", color: on ? "var(--tasty-accent-primary)" : "var(--tasty-text-muted)" }}>{icon}</span>
      <span style={{ fontSize: 13, fontWeight: 600, color: "var(--tasty-text-primary)" }}>{title}</span>
      <span style={{ fontSize: 11, color: "var(--tasty-text-muted)", lineHeight: 1.5 }}>{sub}</span>
    </button>
  );
  return (
    <div style={{ width: 420, background: "var(--tasty-bg-panel)", border: "1px solid var(--tasty-border-strong)",
      borderRadius: "var(--tasty-radius)", overflow: "hidden", boxShadow: "0 20px 60px rgba(0,0,0,.55)" }}>
      <div style={{ padding: "14px 14px 0" }}>
        <div style={{ fontSize: 14, fontWeight: 600, marginBottom: 2 }}>Open markdown file</div>
        <div style={{ fontFamily: "var(--tasty-font-mono)", fontSize: 11, color: "var(--tasty-text-muted)", marginBottom: 12 }}>README.md</div>
        <div style={{ display: "flex", gap: 10 }}>
          <Choice icon={ic.md} title="Rendered preview" sub="Formatted view with headings and links." on />
          <Choice icon={<Icon name="scriptFile" />} title="Raw text" sub="Edit the source in the editor surface." />
        </div>
      </div>
      <div style={{ display: "flex", justifyContent: "flex-end", gap: 8, padding: 14 }}>
        <Button variant="ghost">Cancel</Button><Button variant="primary">Open preview</Button>
      </div>
    </div>
  );
}

// ── Switch-number overlay — transient keycaps over tabs / workspaces ──
// Shown while the (rebindable) tab/workspace switch modifier is held; the
// keycap REPLACES each item's leading indicator. Reuses the Kbd keycap; the
// current item gets an accent-filled keycap. No scrim, no reflow.
function NumCap({ n, active }) {
  return (
    <span aria-hidden style={{ display: "inline-flex", alignItems: "center", justifyContent: "center",
      width: "var(--tasty-switch-overlay-size)", height: "var(--tasty-switch-overlay-size)", flex: "none",
      borderRadius: "var(--tasty-radius-sm)", borderStyle: "solid", borderWidth: "var(--tasty-border-width)",
      borderBottomWidth: "var(--tasty-switch-overlay-shadow-depth)",
      borderColor: active ? "var(--tasty-switch-overlay-active-bg)" : "var(--tasty-switch-overlay-border)",
      background: active ? "var(--tasty-switch-overlay-active-bg)" : "var(--tasty-switch-overlay-bg)",
      color: active ? "var(--tasty-switch-overlay-active-fg)" : "var(--tasty-switch-overlay-fg)",
      fontFamily: "var(--tasty-font-mono)", fontSize: "var(--tasty-kbd-font-size)",
      fontWeight: "var(--tasty-font-weight-medium)", lineHeight: 1 }}>{n}</span>
  );
}

function HeldLabel({ keys, children }) {
  return (
    <div style={{ display: "flex", alignItems: "center", gap: 6, marginBottom: 8, fontSize: 11, color: "var(--tasty-text-muted)" }}>
      <Kbd keys={keys} /><span>held — {children}</span>
    </div>
  );
}

function TabStripMock({ held }) {
  const tabs = [
    { n: "1", label: "server", icon: ic.term },
    { n: "2", label: "dev", icon: ic.term },
    { n: "3", label: "agent", icon: ic.term, active: true },
    { n: "4", label: "README.md", icon: ic.file },
  ];
  return (
    <div style={{ display: "flex", alignItems: "stretch", background: "var(--tasty-bg-sidebar)", width: "fit-content",
      border: "1px solid var(--tasty-separator)", borderRadius: "var(--tasty-radius)", overflow: "hidden" }}>
      {tabs.map((t) => (
        <div key={t.n} style={{ display: "flex", alignItems: "center", gap: "var(--tasty-space-sm)", height: 28, padding: "0 12px",
          borderRight: "1px solid var(--tasty-separator)", whiteSpace: "nowrap", fontSize: 13,
          background: t.active ? "var(--tasty-bg-panel)" : "transparent",
          color: t.active ? "var(--tasty-text-primary)" : "var(--tasty-text-muted)",
          boxShadow: t.active ? "inset 0 -2px 0 var(--tasty-accent-primary)" : "none" }}>
          <span style={{ display: "inline-flex", width: 16, height: 16, alignItems: "center", justifyContent: "center" }}>
            {held ? <NumCap n={t.n} active={t.active} /> : t.icon}
          </span>
          <span>{t.label}</span>
        </div>
      ))}
    </div>
  );
}

function WsRowMock({ n, name, sub, status, active, held }) {
  return (
    <div style={{ display: "flex", alignItems: "flex-start", gap: "var(--tasty-space-sm)", padding: "var(--tasty-space-sm)",
      background: active ? "var(--tasty-surface-active)" : "transparent",
      boxShadow: active ? "inset 2px 0 0 var(--tasty-accent-primary)" : "none" }}>
      <span style={{ flex: "none", height: 18, display: "inline-flex", alignItems: "center" }}>
        {held ? <NumCap n={n} active={active} /> : <StatusDot status={status} pulse={status === "running" || status === "agent"} />}
      </span>
      <div style={{ flex: 1, minWidth: 0 }}>
        <div style={{ fontSize: 13, fontWeight: 500, color: active ? "var(--tasty-text-primary)" : "var(--tasty-text-secondary)",
          overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>{name}</div>
        {sub && <div style={{ fontSize: 11, color: "var(--tasty-text-muted)", marginTop: 1 }}>{sub}</div>}
      </div>
    </div>
  );
}

function SidebarMock({ held }) {
  const rows = [
    { n: "1", name: "tasty-core", sub: "main · 2 tabs", status: "running" },
    { n: "2", name: "ai-review", sub: "agent", status: "agent", active: true },
    { n: "3", name: "infra", sub: "idle", status: "idle" },
    { n: "4", name: "docs-site", sub: "idle", status: "idle" },
  ];
  return (
    <div style={{ width: 188, flex: "none", background: "var(--tasty-bg-sidebar)", border: "1px solid var(--tasty-separator)",
      borderRadius: "var(--tasty-radius)", overflow: "hidden" }}>
      <div style={{ fontFamily: "var(--tasty-font-mono)", fontSize: 10, textTransform: "uppercase", letterSpacing: ".06em",
        color: "var(--tasty-text-muted)", padding: "10px 8px 4px" }}>Workspaces</div>
      {rows.map((r, i) => (
        <React.Fragment key={r.n}>
          {i > 0 && <div style={{ height: 1, background: "var(--tasty-separator)", margin: "0 0 0 32px" }} />}
          <WsRowMock {...r} held={held} />
        </React.Fragment>
      ))}
    </div>
  );
}

function RailMock() {
  const items = [
    { n: "1", letter: "T", status: "running" },
    { n: "2", letter: "A", status: "agent", active: true },
    { n: "3", letter: "I", status: "idle" },
    { letter: "D", status: "idle", noNum: true },
  ];
  return (
    <div style={{ width: 44, flex: "none", background: "var(--tasty-bg-sidebar)", border: "1px solid var(--tasty-separator)",
      borderRadius: "var(--tasty-radius)", padding: "8px 0", display: "flex", flexDirection: "column", alignItems: "center", gap: 4 }}>
      {items.map((it, i) => (
        <div key={i} style={{ width: 28, height: 28, borderRadius: "var(--tasty-radius)", display: "flex", alignItems: "center", justifyContent: "center",
          color: "var(--tasty-text-secondary)",
          background: it.active ? "var(--tasty-surface-active)" : "transparent",
          boxShadow: it.active ? "inset 0 0 0 1px var(--tasty-accent-primary)" : "none" }}>
          {it.noNum
            ? <span style={{ fontFamily: "var(--tasty-font-mono)", fontSize: 13, fontWeight: 700 }}>{it.letter}</span>
            : <NumCap n={it.n} active={it.active} />}
        </div>
      ))}
    </div>
  );
}

// ── Category quick-switch — Alt+Shift held paints a keycap per CATEGORY ──
// A second switch AXIS layered on the folders feature. The two switch
// overlays are modifier-EXCLUSIVE, so they never paint at once:
//   Alt        = workspaces  (row status-dot → keycap)   [SidebarMock/RailMock]
//   Alt+Shift  = categories  (this overlay)
// Full sidebar: the keycap is RIGHT-aligned on the category HEADER row — the
// chevron is NOT replaced, because it is load-bearing for the auto-expand
// transition (a collapsed target rotates open on switch). Reserved `normal`
// (“Workspaces”) is category 1 and gets a slot like any other. Digits run
// 1–9 then 0 (10th); an 11th category onward gets no keycap. Workspace rows
// keep their status dots throughout — only headers change. Switching lands on
// that category's LAST-ACTIVE workspace (falls back to its first), shown with
// the ordinary active treatment (surface-active + 2px accent bar).
function CatSwitchSidebarMock({ held }) {
  const chev = (collapsed) => (
    <span style={{ display: "inline-flex", width: 12, justifyContent: "center", color: "var(--tasty-text-muted)" }}>
      <Icon name={collapsed ? "chevronRight" : "chevronDown"} size={13} />
    </span>
  );
  const Head = ({ n, label, collapsed, active }) => (
    <div style={{ display: "flex", alignItems: "center", gap: 4, padding: "4px 8px", marginTop: 8 }}>
      {chev(collapsed)}
      <span style={{ flex: 1, fontFamily: "var(--tasty-font-mono)", fontSize: 10, textTransform: "uppercase", letterSpacing: ".07em", color: "var(--tasty-text-muted)" }}>{label}</span>
      {held && <NumCap n={n} active={active} />}
    </div>
  );
  const Row = ({ name, status, active }) => (
    <div style={{ display: "flex", alignItems: "center", gap: "var(--tasty-space-sm)", padding: "7px 9px",
      background: active ? "var(--tasty-surface-active)" : "transparent", boxShadow: active ? "inset 2px 0 0 var(--tasty-accent-primary)" : "none" }}>
      <span style={{ flex: "none", height: 18, display: "inline-flex", alignItems: "center" }}><StatusDot status={status} pulse={status === "running" || status === "agent"} /></span>
      <span style={{ flex: 1, minWidth: 0, fontSize: 13, color: active ? "var(--tasty-text-primary)" : "var(--tasty-text-secondary)", overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>{name}</span>
    </div>
  );
  const rule = <div style={{ height: 1, background: "var(--tasty-separator)", margin: "0 0 0 32px" }} />;
  return (
    <div style={{ width: 188, flex: "none", background: "var(--tasty-bg-sidebar)", border: "1px solid var(--tasty-separator)",
      borderRadius: "var(--tasty-radius)", overflow: "hidden", paddingBottom: 8 }}>
      <Head n="1" label="Workspaces" active />
      <Row name="tasty-core" status="running" active />
      {rule}
      <Row name="scratch" status="idle" />
      <Head n="2" label="Services" />
      <Row name="api-gateway" status="agent" />
      {rule}
      <Row name="data-pipeline" status="running" />
      <Head n="3" label="Archived" collapsed />
    </div>
  );
}

// Collapsed rail: categories are `---` boundary buttons. Alt+Shift held puts
// the keycap centered ON the `---` slot (a collapsed/empty category still
// shows its `---`, so it still gets a keycap).
function CatSwitchRailMock({ held }) {
  const boundary = (n, active) => (
    <div style={{ height: 22, display: "flex", alignItems: "center", justifyContent: "center" }}>
      {held ? <NumCap n={n} active={active} /> : <div style={{ width: 24, height: 1, background: "var(--tasty-separator)" }} />}
    </div>
  );
  const av = (ch, active) => (
    <div style={{ width: 28, height: 28, borderRadius: "var(--tasty-radius)", display: "flex", alignItems: "center", justifyContent: "center",
      fontFamily: "var(--tasty-font-mono)", fontSize: 13, fontWeight: 700, color: "var(--tasty-text-secondary)",
      background: active ? "var(--tasty-surface-active)" : "transparent", boxShadow: active ? "inset 0 0 0 1px var(--tasty-accent-primary)" : "none" }}>{ch}</div>
  );
  return (
    <div style={{ width: 44, flex: "none", background: "var(--tasty-bg-sidebar)", border: "1px solid var(--tasty-separator)",
      borderRadius: "var(--tasty-radius)", padding: "8px 0", display: "flex", flexDirection: "column", alignItems: "center", gap: 4 }}>
      {boundary("1", true)}
      {av("T", true)}{av("S")}
      {boundary("2")}
      {av("A")}{av("D")}
      {boundary("3")}
    </div>
  );
}

// ── Banner — the 4th overlay family (floating, top-anchored, focus-less) ──
// Shared shell; sits at the top of a scope's content area, never over the tab bar.
function BannerShellG({ recessed, zIndex, children }) {
  return (
    <div style={{ width: "100%", position: "relative", zIndex,
      background: "var(--tasty-banner-bg)", color: "var(--tasty-banner-fg)",
      border: "1px solid var(--tasty-banner-border)", borderRadius: "var(--tasty-banner-radius)",
      boxShadow: "var(--tasty-banner-shadow)", opacity: recessed ? "var(--tasty-banner-recessed-opacity)" : 1 }}>
      {children}
    </div>
  );
}

// a faux scope: tab strip on top (must stay clear) + content; banner floats at content top
function BannerScope({ height = 240, tab = "vim", children }) {
  return (
    <div style={{ position: "relative", width: "100%", maxWidth: 600, height, borderRadius: "var(--tasty-radius)", overflow: "hidden",
      border: "1px solid var(--tasty-border-default)", background: "#000" }}>
      <div style={{ display: "flex", alignItems: "stretch", height: 28, background: "var(--tasty-bg-sidebar)", borderBottom: "1px solid var(--tasty-separator)" }}>
        {["server", "dev", tab].map((t, i) => (
          <div key={t} style={{ display: "flex", alignItems: "center", padding: "0 12px", fontSize: 13, borderRight: "1px solid var(--tasty-separator)",
            background: i === 2 ? "var(--tasty-bg-panel)" : "transparent", color: i === 2 ? "var(--tasty-text-primary)" : "var(--tasty-text-muted)",
            boxShadow: i === 2 ? "inset 0 -2px 0 var(--tasty-accent-primary)" : "none" }}>{t}</div>
        ))}
      </div>
      <div style={{ padding: "12px 14px", fontFamily: "var(--tasty-font-mono)", fontSize: 12, lineHeight: 1.7, color: "#cdd6f4", opacity: 0.4 }}>
        <div><span style={{ color: "var(--tasty-color-green)" }}>~/tasty</span> $ vim src/main.rs</div>
        <div>-- VISUAL --   1,1   Top</div>
      </div>
      <div style={{ position: "absolute", top: "calc(28px + var(--tasty-banner-margin))",
        left: "var(--tasty-banner-margin)", right: "var(--tasty-banner-margin)" }}>{children}</div>
    </div>
  );
}

// ── Banner "more" (⋯) menu — per-app opt-outs behind one trigger ──────────
// A row's label is fixed text + the interpolated program name; the fixed part
// never truncates, the app name (mono) shrinks first and ellipsises, so the menu
// stays inside --tasty-banner-more-menu-max-width in every locale.
function MoreLabel({ text, app, after }) {
  return (
    <span style={{ display: "flex", minWidth: 0, whiteSpace: "nowrap" }}>
      <span style={{ flex: "none" }}>{text}</span>
      <span style={{ minWidth: 0, overflow: "hidden", textOverflow: "ellipsis",
        fontFamily: "var(--tasty-banner-more-app-font)", color: "var(--tasty-banner-more-app-fg)" }} title={app}>{app}</span>
      {after && <span style={{ flex: "none" }}>{after}</span>}
    </span>
  );
}

// the menu itself — 2 rows, no scrim, anchored under the ⋯ trigger.
function BannerMoreMenuG({ app = "vim", hovered = -1, danger = false, style }) {
  const rows = [
    { icon: <Icon name="bell" />, text: "Turn off this notice for ", danger: false },
    { icon: <Icon name="mouse" />, text: "Disable mouse capture for ", danger },
  ];
  return (
    <div role="menu" aria-label="Banner options" style={{
      minWidth: "var(--tasty-banner-more-menu-min-width)", maxWidth: "var(--tasty-banner-more-menu-max-width)",
      background: "var(--tasty-banner-more-menu-bg)", border: "1px solid var(--tasty-banner-more-menu-border)",
      borderRadius: "var(--tasty-banner-more-menu-radius)", padding: "var(--tasty-banner-more-menu-padding)",
      boxShadow: "var(--tasty-banner-more-menu-shadow)", ...style }}>
      {rows.map((r, i) => (
        <MenuItem key={r.text} icon={r.icon} danger={r.danger} active={hovered === i}
          label={<MoreLabel text={r.text} app={app} />} />
      ))}
    </div>
  );
}

// the canonical mouse-capture banner — persistent (no TTL); ⋯ and × on hover.
// Fires once per tracking session, on a real user click while an app holds the
// mouse. Content = title + shift-bypass hint; the two per-app opt-outs live
// behind the ⋯ trigger, so the card body is never crowded by action buttons.
function MouseCaptureBannerG({ glyph = true, body, lines, app = "vim", more = true, open = false, hovered = -1, danger = false, forceHover = false }) {
  const [hover, setHover] = React.useState(false);
  const shown = hover || forceHover || open;
  const defBody = (
    <>This app is capturing the mouse. Hold <Kbd keys="Shift" />+drag to select text, <Kbd keys="Shift" />+Right-click for the tasty menu.</>
  );
  return (
    <BannerShellG>
      <div onMouseEnter={() => setHover(true)} onMouseLeave={() => setHover(false)}
        style={{ display: "flex", alignItems: "flex-start", gap: "var(--tasty-banner-gap)",
          padding: "var(--tasty-banner-padding-y) var(--tasty-banner-padding-x)" }}>
        {glyph && <span style={{ display: "inline-flex", flex: "none", marginTop: 1, color: "var(--tasty-banner-icon-fg)" }}>{ic.mouse}</span>}
        <div style={{ flex: 1, minWidth: 0, display: "flex", flexDirection: "column", gap: 2,
          paddingRight: more ? "var(--tasty-banner-more-reserve)" : 28 }}>
          <div style={{ fontSize: "var(--tasty-banner-title-font-size)", fontWeight: 600, color: "var(--tasty-banner-fg)", lineHeight: 1.4 }}>Mouse input captured</div>
          <div style={{ fontSize: "var(--tasty-banner-body-font-size)", color: "var(--tasty-text-muted)", lineHeight: 1.45 }}>{body || defBody}</div>
        </div>
        <span style={{ position: "absolute", top: "var(--tasty-banner-padding-y)", right: "var(--tasty-space-sm)",
          display: "flex", alignItems: "center", gap: "var(--tasty-banner-more-column-gap)",
          opacity: shown ? 1 : 0, transition: "opacity var(--tasty-banner-fade) var(--tasty-ease-ui)" }}>
          {more && (
            <span style={{ position: "relative", display: "inline-flex" }}>
              <IconButton size="sm" aria-label="More options" aria-expanded={open} active={open}><Icon name="more" /></IconButton>
              {open && (
                <div style={{ position: "absolute", top: "calc(100% + var(--tasty-banner-more-menu-offset))", right: 0, zIndex: 2 }}>
                  <BannerMoreMenuG app={app} hovered={hovered} danger={danger} />
                </div>
              )}
            </span>
          )}
          <IconButton size="sm" aria-label="Dismiss">{ic.x}</IconButton>
        </span>
      </div>
    </BannerShellG>
  );
}

// interactive: click ⋯ to open, outside-click / Esc to close, row click acts.
function BannerMoreDemoG() {
  const [open, setOpen] = React.useState(false);
  const [log, setLog] = React.useState(null);
  const wrap = React.useRef(null);
  React.useEffect(() => {
    if (!open) return undefined;
    const onDown = (e) => { if (wrap.current && !wrap.current.contains(e.target)) setOpen(false); };
    const onKey = (e) => { if (e.key === "Escape") setOpen(false); };
    document.addEventListener("mousedown", onDown);
    document.addEventListener("keydown", onKey);
    return () => { document.removeEventListener("mousedown", onDown); document.removeEventListener("keydown", onKey); };
  }, [open]);
  const act = (which) => { setOpen(false); setLog(which); };
  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 8, width: "100%", maxWidth: 600 }}>
      <div ref={wrap}>
        <BannerScope height={230}>
          {log !== "notice" ? (
            <BannerShellG>
              <div style={{ display: "flex", alignItems: "flex-start", gap: "var(--tasty-banner-gap)",
                padding: "var(--tasty-banner-padding-y) var(--tasty-banner-padding-x)" }}>
                <span style={{ display: "inline-flex", flex: "none", marginTop: 1, color: "var(--tasty-banner-icon-fg)" }}>{ic.mouse}</span>
                <div style={{ flex: 1, minWidth: 0, display: "flex", flexDirection: "column", gap: 2, paddingRight: "var(--tasty-banner-more-reserve)" }}>
                  <div style={{ fontSize: "var(--tasty-banner-title-font-size)", fontWeight: 600, color: "var(--tasty-banner-fg)", lineHeight: 1.4 }}>Mouse input captured</div>
                  <div style={{ fontSize: "var(--tasty-banner-body-font-size)", color: "var(--tasty-text-muted)", lineHeight: 1.45 }}>
                    This app is capturing the mouse. Hold <Kbd keys="Shift" />+drag to select text, <Kbd keys="Shift" />+Right-click for the tasty menu.
                  </div>
                </div>
                <span style={{ position: "absolute", top: "var(--tasty-banner-padding-y)", right: "var(--tasty-space-sm)",
                  display: "flex", alignItems: "center", gap: "var(--tasty-banner-more-column-gap)" }}>
                  <span style={{ position: "relative", display: "inline-flex" }}>
                    <IconButton size="sm" aria-label="More options" aria-expanded={open} active={open}
                      onClick={() => setOpen((o) => !o)}><Icon name="more" /></IconButton>
                    {open && (
                      <div style={{ position: "absolute", top: "calc(100% + var(--tasty-banner-more-menu-offset))", right: 0, zIndex: 2 }}>
                        <div role="menu" aria-label="Banner options" style={{
                          minWidth: "var(--tasty-banner-more-menu-min-width)", maxWidth: "var(--tasty-banner-more-menu-max-width)",
                          background: "var(--tasty-banner-more-menu-bg)", border: "1px solid var(--tasty-banner-more-menu-border)",
                          borderRadius: "var(--tasty-banner-more-menu-radius)", padding: "var(--tasty-banner-more-menu-padding)",
                          boxShadow: "var(--tasty-banner-more-menu-shadow)" }}>
                          <MenuItem icon={<Icon name="bell" />} onClick={() => act("notice")}
                            label={<MoreLabel text="Turn off this notice for " app="vim" />} />
                          <MenuItem icon={<Icon name="mouse" />} onClick={() => act("capture")}
                            label={<MoreLabel text="Disable mouse capture for " app="vim" />} />
                        </div>
                      </div>
                    )}
                  </span>
                  <IconButton size="sm" aria-label="Dismiss" onClick={() => setLog("dismissed")}>{ic.x}</IconButton>
                </span>
              </div>
            </BannerShellG>
          ) : null}
        </BannerScope>
      </div>
      <div style={{ fontSize: 11, color: "var(--tasty-text-muted)", minHeight: 16 }}>
        {log === "notice" && <>Added <code>vim</code> to <code>mouse_capture_banner_blacklist</code> — banner closed with the action.</>}
        {log === "capture" && <>Added <code>vim</code> to <code>mouse_capture_blacklist</code> — capture released; the banner stays until dismissed.</>}
        {log === "dismissed" && <>Dismissed for this tracking session — nothing written to Settings.</>}
        {!log && <>Click <b>⋯</b> to open · outside click or <span className="ic">Esc</span> closes.</>}
      </div>
    </div>
  );
}

// B4 — position context + hit-zone. The card consumes its own mouse; the surface
// body below it is pass-through (clicks reach the capturing app).
function MouseCaptureHitZone() {
  return (
    <div style={{ position: "relative", width: "100%", maxWidth: 560, height: 250, borderRadius: "var(--tasty-radius)", overflow: "hidden",
      border: "1px solid var(--tasty-border-default)", background: "#000" }}>
      {/* tab bar — banner must never cover this */}
      <div style={{ display: "flex", alignItems: "stretch", height: 28, background: "var(--tasty-bg-sidebar)", borderBottom: "1px solid var(--tasty-separator)" }}>
        {["server", "dev", "htop"].map((t, i) => (
          <div key={t} style={{ display: "flex", alignItems: "center", padding: "0 12px", fontSize: 13, borderRight: "1px solid var(--tasty-separator)",
            background: i === 2 ? "var(--tasty-bg-panel)" : "transparent", color: i === 2 ? "var(--tasty-text-primary)" : "var(--tasty-text-muted)",
            boxShadow: i === 2 ? "inset 0 -2px 0 var(--tasty-accent-primary)" : "none" }}>{t}</div>
        ))}
      </div>
      {/* surface body — pass-through */}
      <div style={{ position: "absolute", inset: "28px 0 0 0", display: "flex", alignItems: "flex-end", padding: 12 }}>
        <span style={{ fontFamily: "var(--tasty-font-mono)", fontSize: 11, color: "var(--tasty-text-disabled)" }}>↑ surface body — pass-through · clicks/drag reach the app</span>
      </div>
      {/* the banner card, with an explicit consume-zone outline */}
      <div style={{ position: "absolute", top: "calc(28px + var(--tasty-banner-margin))", left: "var(--tasty-banner-margin)", right: "var(--tasty-banner-margin)" }}>
        <div style={{ position: "relative" }}>
          <MouseCaptureBannerG />
          <span style={{ position: "absolute", top: -7, left: 8, fontFamily: "var(--tasty-font-mono)", fontSize: 9, letterSpacing: ".04em",
            background: "var(--tasty-bg-app)", color: "var(--tasty-accent-info)", padding: "0 4px", borderRadius: 2 }}>CARD RECT = mouse-consume + hover zone</span>
        </div>
      </div>
    </div>
  );
}

// blacklist editor (Settings › Terminal) — list rows + add input + empty state
function BlacklistRow({ pattern, hover }) {
  return (
    <div style={{ display: "flex", alignItems: "center", gap: 8, height: 28, padding: "0 4px 0 8px", borderRadius: "var(--tasty-radius-sm)",
      background: hover ? "var(--tasty-overlay-hover)" : "transparent" }}>
      <span style={{ flex: 1, fontFamily: "var(--tasty-font-mono)", fontSize: 12, color: "var(--tasty-text-primary)" }}>{pattern}</span>
      <IconButton size="sm" aria-label={"Remove " + pattern}>{ic.x}</IconButton>
    </div>
  );
}

function BlacklistEditorG({ empty }) {
  return (
    <div style={{ width: 320, display: "flex", flexDirection: "column", gap: 10, padding: 14,
      background: "var(--tasty-bg-panel)", border: "1px solid var(--tasty-border-default)", borderRadius: "var(--tasty-radius)" }}>
      <div style={{ fontFamily: "var(--tasty-font-mono)", fontSize: 10, textTransform: "uppercase", letterSpacing: ".06em", color: "var(--tasty-text-muted)" }}>Mouse capture</div>
      <div style={{ display: "flex", alignItems: "center", gap: 10 }}>
        <span style={{ flex: 1, fontSize: 13, color: "var(--tasty-text-secondary)" }}>Show mouse-capture hint</span>
        <Switch defaultChecked />
      </div>
      <div style={{ height: 1, background: "var(--tasty-separator)" }} />
      <div style={{ fontSize: 13, color: "var(--tasty-text-secondary)" }}>Disable capture for these programs</div>
      {empty ? (
        <div style={{ fontSize: 11, color: "var(--tasty-text-muted)", lineHeight: 1.5, padding: "4px 0" }}>No programs excluded — clicks are sent to capturing apps.</div>
      ) : (
        <div style={{ display: "flex", flexDirection: "column", gap: 1 }}>
          <BlacklistRow pattern="htop" />
          <BlacklistRow pattern="vim" hover />
          <BlacklistRow pattern="ht*" />
        </div>
      )}
      <div style={{ display: "flex", gap: 6 }}>
        <Input block placeholder="process name or pattern, e.g. htop or ht*" />
        <Button variant="secondary" size="sm" disabled={empty}>Add</Button>
      </div>
      <div style={{ fontSize: 11, color: "var(--tasty-accent-warning)", lineHeight: 1.5 }}>
        Case-insensitive substring or <b>*</b> wildcard on the process name. When a listed program is foreground, clicks/drags are handled locally; the wheel is still sent.
      </div>
    </div>
  );
}

// a TTL banner — live countdown that flips to X on hover (and pauses while hovered)
function TtlBannerG() {
  const [hover, setHover] = React.useState(false);
  const [n, setN] = React.useState(6);
  React.useEffect(() => {
    if (hover) return undefined;                       // pause while hovered
    const id = setTimeout(() => setN((x) => (x <= 1 ? 6 : x - 1)), 1000);
    return () => clearTimeout(id);
  }, [hover, n]);
  return (
    <BannerShellG>
      <div onMouseEnter={() => setHover(true)} onMouseLeave={() => setHover(false)}
        style={{ display: "flex", alignItems: "center", gap: "var(--tasty-banner-gap)",
          padding: "var(--tasty-banner-padding-y) var(--tasty-banner-padding-x)" }}>
        <span style={{ display: "inline-flex", flex: "none", color: "var(--tasty-accent-success)" }}>{ic.check}</span>
        <div style={{ flex: 1, fontSize: "var(--tasty-banner-title-font-size)", color: "var(--tasty-text-secondary)" }}>
          Preset <b style={{ color: "var(--tasty-text-primary)" }}>Dev split</b> applied to this tab.
        </div>
        <span style={{ width: 24, height: 24, flex: "none", display: "inline-flex", alignItems: "center", justifyContent: "center" }}>
          {hover
            ? <IconButton size="sm" aria-label="Dismiss">{ic.x}</IconButton>
            : <span style={{ fontFamily: "var(--tasty-banner-countdown-font)", fontSize: "var(--tasty-banner-countdown-font-size)",
                color: "var(--tasty-banner-countdown-fg)", fontVariantNumeric: "tabular-nums" }}>{n}</span>}
        </span>
      </div>
    </BannerShellG>
  );
}

// cross-scope stacking — a higher-scope banner sits in front; the lower one is dimmed behind
function StackDemoG() {
  const line = (txt) => (<span style={{ fontSize: "var(--tasty-banner-body-font-size)", color: "var(--tasty-text-muted)" }}>{txt}</span>);
  return (
    <div style={{ position: "relative", width: "100%", maxWidth: 520, height: 116 }}>
      {/* lower-tier (Pane) banner — taller, recessed behind */}
      <div style={{ position: "absolute", top: 14, left: 0, right: 0 }}>
        <BannerShellG recessed zIndex={1}>
          <div style={{ padding: "var(--tasty-banner-padding-y) var(--tasty-banner-padding-x)", height: 84, display: "flex", alignItems: "flex-end" }}>
            {line("Pane banner — recessed to 40% opacity behind the higher banner; only its overhang shows")}
          </div>
        </BannerShellG>
      </div>
      {/* higher-tier (Workspace) banner — on top, full opacity, higher z */}
      <div style={{ position: "absolute", top: 0, left: 0, right: 0 }}>
        <BannerShellG zIndex={2}>
          <div style={{ display: "flex", alignItems: "center", gap: 10, padding: "var(--tasty-banner-padding-y) var(--tasty-banner-padding-x)" }}>
            <span style={{ display: "inline-flex", flex: "none", color: "var(--tasty-accent-warning)" }}>{ic.warn}</span>
            <span style={{ flex: 1, fontSize: "var(--tasty-banner-title-font-size)", color: "var(--tasty-text-secondary)" }}>Workspace banner — in front, full opacity, higher z-index</span>
          </div>
        </BannerShellG>
      </div>
    </div>
  );
}

// ── Git worktree viewer — 960×640 read-only popup (status/log + diff) ──
// token-colored pill — mirrors the Tag visual (outlined, color-mix tint); `info`
// is the tone Tag variants don't ship (main / oid / refs / hunk = sky).
const GV_TONE = { info: "var(--tasty-accent-info)", success: "var(--tasty-accent-success)",
  warning: "var(--tasty-accent-warning)", danger: "var(--tasty-accent-danger)", primary: "var(--tasty-accent-primary)" };
function gvPill(txt, tone, dot, width) {
  const c = GV_TONE[tone];
  const base = { display: "inline-flex", alignItems: "center", justifyContent: width ? "center" : "flex-start", gap: 4,
    height: 16, padding: width ? 0 : "0 6px", width, flex: "none", borderRadius: "var(--tasty-radius-sm)",
    fontFamily: "var(--tasty-font-mono)", fontSize: 10, fontWeight: 500, lineHeight: 1, whiteSpace: "nowrap", border: "1px solid" };
  const dotEl = dot ? <span style={{ width: 5, height: 5, borderRadius: "var(--tasty-radius-pill)", background: "currentColor" }} /> : null;
  if (!c) return <span key={txt} style={{ ...base, color: "var(--tasty-text-secondary)", borderColor: "var(--tasty-border-default)", background: "var(--tasty-surface-raised)" }}>{txt}</span>;
  return <span key={txt} style={{ ...base, color: c, borderColor: `color-mix(in srgb, ${c} 40%, transparent)`, background: `color-mix(in srgb, ${c} 12%, transparent)` }}>{dotEl}{txt}</span>;
}
const gvStrip = { display: "flex", alignItems: "center", gap: 8, height: 26, flex: "none", padding: "0 12px",
  background: "var(--tasty-bg-sidebar)", borderBottom: "1px solid var(--tasty-separator)",
  fontFamily: "var(--tasty-font-mono)", fontSize: 10, textTransform: "uppercase", letterSpacing: ".06em", color: "var(--tasty-text-muted)" };

// ── Git worktree viewer — 960×640 read-only popup (rail · status/log · diff) ──
function GitViewerFrame({ diff }) {
  const mono = { fontFamily: "var(--tasty-font-mono)", fontSize: 12 };
  const WtRow = ({ name, oid, type, state, reason, active, dim }) => (
    <div style={{ display: "flex", flexDirection: "column", gap: 3, padding: "7px 10px", borderBottom: "1px solid var(--tasty-separator)",
      background: active ? "var(--tasty-surface-active)" : "transparent", boxShadow: active ? "inset 2px 0 0 var(--tasty-accent-primary)" : "none", opacity: dim ? 0.7 : 1 }}>
      <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
        <span style={{ ...mono, fontWeight: 600, flex: 1, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap",
          color: dim ? "var(--tasty-text-disabled)" : active ? "var(--tasty-text-primary)" : "var(--tasty-text-secondary)" }}>{name}</span>
        {type === "main" ? gvPill("main", "info") : gvPill("linked")}
      </div>
      <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
        {oid && <span style={{ ...mono, fontSize: 11, color: "var(--tasty-accent-info)" }}>{oid}</span>}
        <span style={{ flex: 1 }} />
        {state && gvPill(state, state === "current" ? "success" : state === "locked" ? "warning" : "danger", true)}
      </div>
    </div>
  );
  const StRow = ({ p, tone, dir, file, active }) => (
    <div style={{ display: "flex", alignItems: "center", gap: 8, height: 26, padding: "0 12px",
      background: active ? "var(--tasty-surface-active)" : "transparent", boxShadow: active ? "inset 2px 0 0 var(--tasty-accent-primary)" : "none" }}>
      {gvPill(p, tone, false, 18)}
      <span style={{ ...mono, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>
        <span style={{ color: "var(--tasty-text-muted)" }}>{dir}</span><span style={{ color: "var(--tasty-text-primary)" }}>{file}</span>
      </span>
    </div>
  );
  const Head = ({ children }) => <div style={gvStrip}>{children}</div>;
  const dl = (oldn, newn, kind, content) => {
    const hunk = kind === "@", add = kind === "+", del = kind === "-";
    const fg = hunk ? "var(--tasty-accent-info)" : add ? "var(--tasty-accent-success)" : del ? "var(--tasty-accent-danger)" : "var(--tasty-text-primary)";
    const bg = hunk ? "color-mix(in srgb, var(--tasty-accent-info) 9%, transparent)" : add ? "color-mix(in srgb, var(--tasty-accent-success) 10%, transparent)" : del ? "color-mix(in srgb, var(--tasty-accent-danger) 10%, transparent)" : "transparent";
    return (
      <div style={{ display: "flex", ...mono, fontSize: 11, lineHeight: 1.65, background: bg, color: fg }}>
        <span style={{ width: 30, textAlign: "right", color: "var(--tasty-text-disabled)", paddingRight: 6 }}>{hunk ? "" : oldn}</span>
        <span style={{ width: 30, textAlign: "right", color: "var(--tasty-text-disabled)", paddingRight: 8 }}>{hunk ? "" : newn}</span>
        <span style={{ width: 12, textAlign: "center", opacity: 0.8 }}>{hunk || kind === " " ? "" : kind}</span>
        <span style={{ whiteSpace: "pre" }}>{content}</span>
      </div>
    );
  };
  return (
    <div style={{ width: "100%", maxWidth: 640, height: 412, display: "flex", flexDirection: "column",
      background: "var(--tasty-bg-panel)", border: "1px solid var(--tasty-border-strong)", borderRadius: "var(--tasty-radius)", boxShadow: "var(--tasty-shadow-modal)", overflow: "hidden" }}>
      {/* header */}
      <div style={{ display: "flex", alignItems: "center", gap: 8, padding: "8px 12px", flex: "none", borderBottom: "1px solid var(--tasty-separator)" }}>
        <span style={{ display: "inline-flex", color: "var(--tasty-text-muted)" }}><Icon name="gitBranch" size={16} /></span>
        <span style={{ fontSize: 14, fontWeight: 600 }}>Git</span>
        <span style={{ flex: 1 }} />
        <Button variant="secondary" size="sm" leadingIcon={ic.refresh}>Refresh</Button>
        <IconButton size="sm" aria-label="Close">{ic.x}</IconButton>
      </div>
      {/* context strip */}
      <div style={{ display: "flex", alignItems: "center", gap: 8, height: 28, flex: "none", padding: "0 12px", background: "var(--tasty-bg-sidebar)", borderBottom: "1px solid var(--tasty-separator)" }}>
        <span style={{ ...mono, color: "var(--tasty-text-primary)" }}>tasty</span>
        <span style={{ color: "var(--tasty-text-disabled)" }}>·</span>
        <span style={{ ...mono, color: "var(--tasty-text-secondary)" }}>main</span>
        {gvPill("a1b2c3d", "info")}
        <span style={{ flex: 1 }} />
        <span style={{ ...mono, fontSize: 11, color: "var(--tasty-text-muted)" }}>~/work/tasty</span>
      </div>
      <div style={{ flex: 1, minHeight: 0, display: "flex" }}>
        {/* rail */}
        <div style={{ width: 200, flex: "none", borderRight: "1px solid var(--tasty-separator)", display: "flex", flexDirection: "column", overflow: "hidden" }}>
          <Head>Worktrees (4)</Head>
          <div style={{ flex: 1, overflow: "auto" }}>
            <WtRow name="tasty" oid="a1b2c3d" type="main" state="current" active />
            <WtRow name="feature-ui" oid="9f8e7d6" type="linked" state="locked" />
            <WtRow name="release-0.6" oid="3c4d5e6" type="linked" />
            <WtRow name="stale-wt" type="linked" state="invalid" dim />
          </div>
        </div>
        {/* right column */}
        <div style={{ flex: 1, minWidth: 0, display: "flex", flexDirection: "column" }}>
          <div style={{ flex: 1, minHeight: 0, borderBottom: "1px solid var(--tasty-separator)", display: "flex", flexDirection: "column", overflow: "hidden" }}>
            <Head>Changes (4)</Head>
            <div style={{ flex: 1, overflow: "auto", padding: "2px 0" }}>
              <StRow p="M" tone="warning" dir="crates/tasty-plugin-git-viewer/src/" file="view.rs" active={diff} />
              <StRow p="A" tone="success" dir="ui_kits/terminal/overlays/" file="git_viewer.jsx" />
              <StRow p="D" tone="danger" dir="crates/tasty-gallery/src/catalog/" file="legacy_git.rs" />
              <StRow p="?" tone={null} dir="scraps/" file="git-notes.txt" />
            </div>
          </div>
          <div style={{ flex: 1, minHeight: 0, display: "flex", flexDirection: "column", overflow: "hidden" }}>
            {diff ? (
              <>
                <div style={{ display: "flex", alignItems: "center", gap: 8, height: 30, flex: "none", padding: "0 8px 0 12px", background: "var(--tasty-bg-sidebar)", borderBottom: "1px solid var(--tasty-separator)" }}>
                  <Button variant="ghost" size="sm" leadingIcon={ic.back}>Back</Button>
                  <span style={{ ...mono, fontSize: 11, color: "var(--tasty-text-muted)", overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>crates/tasty-plugin-git-viewer/src/view.rs</span>
                </div>
                <div style={{ flex: 1, overflow: "auto", background: "var(--tasty-bg-app)", padding: "2px 0" }}>
                  {dl("", "", "@", "@@ -38,9 +38,12 @@ fn draw_worktree_rail()")}
                  {dl("38", "38", " ", "    for wt in worktrees {")}
                  {dl("39", "", "-", "        ui.label(wt.name);")}
                  {dl("40", "", "-", "        ui.colored_label(BLUE, wt.oid);")}
                  {dl("", "39", "+", "        let row = SelectableRow::new(wt.id);")}
                  {dl("", "40", "+", "        row.badge(wt.state_badge());")}
                  {dl("41", "41", " ", "    }")}
                </div>
              </>
            ) : (
              <>
                <Head>Commits (5)</Head>
                <div style={{ flex: 1, overflow: "auto", padding: "2px 0" }}>
                  {[["a1b2c3d", ["main", "origin/main"], "feat(git-viewer): worktree rail + diff well", "2h"],
                    ["9f8e7d6", [], "fix: align diff gutter line numbers", "5h"],
                    ["3c4d5e6", ["release/0.6"], "docs: read-only viewer note", "1d"]].map(([o, refs, s, t]) => (
                    <div key={o} style={{ display: "flex", alignItems: "center", gap: 8, height: 28, padding: "0 12px", whiteSpace: "nowrap", overflow: "hidden" }}>
                      <span style={{ ...mono, fontSize: 11, color: "var(--tasty-accent-info)" }}>{o}</span>
                      {refs.map((r) => gvPill(r, "info"))}
                      <span style={{ flex: 1, fontSize: 13, color: "var(--tasty-text-primary)", overflow: "hidden", textOverflow: "ellipsis" }}>{s}</span>
                      <span style={{ fontSize: 12, color: "var(--tasty-text-muted)" }}>zilhak</span>
                      <span style={{ ...mono, fontSize: 11, color: "var(--tasty-text-muted)" }}>{t} ago</span>
                    </div>
                  ))}
                </div>
              </>
            )}
          </div>
        </div>
      </div>
    </div>
  );
}

// ── Clipboard viewer — 480×360 read-only SNAPSHOT popup (single-column) ──
function ClipboardFrame({ state = "data" }) {
  const TEXT = `cargo build -p tasty-gallery --release
git switch wt-5/T8-code-review
tasty read screen --surface 3 --json
~/work/tasty/crates/tasty-gallery/src/catalog`;
  const center = (glyph, title, sub, danger) => (
    <div style={{ flex: 1, display: "flex", flexDirection: "column", alignItems: "center", justifyContent: "center", gap: 8, padding: 24, textAlign: "center" }}>
      <span style={{ display: "inline-flex", color: danger ? "var(--tasty-accent-danger)" : "var(--tasty-text-muted)", opacity: danger ? 0.9 : 0.5 }}>{glyph}</span>
      <span style={{ fontSize: 13, fontWeight: 600, color: danger ? "var(--tasty-accent-danger)" : "var(--tasty-text-secondary)" }}>{title}</span>
      {sub && <span style={{ fontSize: 12, color: "var(--tasty-text-muted)", maxWidth: 280, lineHeight: 1.5 }}>{sub}</span>}
    </div>
  );
  const clipGlyph = <Icon name="clipboard" size={28} />;
  const Frame = ({ children }) => (
    <div style={{ width: "100%", maxWidth: 460, height: 320, display: "flex", flexDirection: "column", background: "var(--tasty-bg-panel)",
      border: "1px solid var(--tasty-border-strong)", borderRadius: "var(--tasty-radius)", boxShadow: "var(--tasty-shadow-modal)", overflow: "hidden" }}>
      <div style={{ display: "flex", alignItems: "center", gap: 8, padding: "10px 14px", flex: "none", borderBottom: "1px solid var(--tasty-separator)" }}>
        <span style={{ display: "inline-flex", color: "var(--tasty-text-muted)" }}><Icon name="clipboard" size={16} /></span>
        <span style={{ fontSize: 14, fontWeight: 600 }}>Clipboard</span>
        <Tag>snapshot</Tag>
        <span style={{ flex: 1 }} />
        <IconButton size="sm" aria-label="Close">{ic.x}</IconButton>
      </div>
      {children}
    </div>
  );
  if (state === "empty") return <Frame>{center(clipGlyph, "Clipboard is empty", "Copy some text, an image, or files and reopen to see a snapshot here.")}</Frame>;
  if (state === "failed") return <Frame>{center(<Icon name="alertTriangle" size={28} />, "Couldn't read the clipboard", "The system clipboard handle could not be opened.", true)}</Frame>;
  const seg = (id, active, first, compact) => {
    const [label, glyph] = TYPE_DEF[id];
    const bare = compact && !active;
    return (
      <span key={id} title={label} style={{ display: "inline-flex", alignItems: "center", justifyContent: "center", gap: bare ? 0 : 4, height: 26,
        padding: bare ? "0 8px" : "0 10px", minWidth: bare ? 30 : 0,
        borderLeft: first ? "none" : "1px solid var(--tasty-border-default)", fontSize: 12, fontWeight: active ? 600 : 400,
        color: active ? "var(--tasty-text-on-accent)" : "var(--tasty-text-secondary)", background: active ? "var(--tasty-accent-primary)" : "transparent" }}>
        <Icon name={glyph} size={13} />{bare ? null : label}</span>
    );
  };
  const ids = CB_SETS[state] || CB_SETS.data;
  const cur = state === "html" || state === "htmlPretty" ? "html" : state === "other" ? "other" : "text";
  const compact = ids.length >= 5;
  const meta = cur === "html" ? "312 chars · 1 line" : cur === "other" ? "4 formats" : "284 chars · 6 lines · UTF-8";
  const mime = cur === "html" ? "text/html · 312 chars · 1 line" : cur === "other" ? "4 unrecognized formats" : "text/plain";
  const mono = { fontFamily: "var(--tasty-font-mono)", fontSize: 12, lineHeight: 1.7, color: "var(--tasty-text-primary)" };
  const metaMono = { fontFamily: "var(--tasty-font-mono)", fontSize: 11, color: "var(--tasty-text-muted)" };
  const well = { flex: 1, minHeight: 0, overflow: "auto", margin: "8px 14px 14px", borderRadius: "var(--tasty-radius)",
    border: "1px solid var(--tasty-separator)", background: "var(--tasty-bg-app)" };
  const body = cur === "other" ? (
    <div style={well}>
      <div style={{ display: "flex", flexDirection: "column" }}>
        {CB_FORMATS.map((f, i) => (
          <div key={f.name} style={{ display: "flex", flexDirection: "column", gap: 4, padding: "10px 14px",
            borderBottom: i < CB_FORMATS.length - 1 ? "1px solid var(--tasty-separator)" : "none" }}>
            <div style={{ display: "flex", alignItems: "baseline", gap: 8 }}>
              <span style={{ ...metaMono, color: "var(--tasty-text-secondary)", fontWeight: 600 }}>{f.name}</span>
              <span style={metaMono}>{f.size}</span>
            </div>
            <pre style={{ ...mono, margin: 0, whiteSpace: "pre-wrap", wordBreak: "break-all" }}>{f.content}</pre>
            {f.moreLines ? <span style={{ ...metaMono, fontStyle: "italic" }}>+{f.moreLines} more lines</span> : null}
          </div>
        ))}
      </div>
    </div>
  ) : (
    <div style={well}>
      <pre style={{ ...mono, margin: 0, padding: "10px 14px", whiteSpace: "pre-wrap", wordBreak: "break-word" }}>
        {cur === "html" ? (state === "htmlPretty" ? CB_HTML_PRETTY : CB_HTML_RAW) : TEXT}</pre>
    </div>
  );
  return (
    <Frame>
      {/* type bar */}
      <div style={{ display: "flex", alignItems: "center", gap: 12, padding: "8px 14px", flex: "none", background: "var(--tasty-bg-sidebar)", borderBottom: "1px solid var(--tasty-separator)" }}>
        {ids.length > 1 ? (
          <div style={{ display: "inline-flex", border: "1px solid var(--tasty-border-default)", borderRadius: "var(--tasty-radius)", overflow: "hidden" }}>
            {ids.map((id, i) => seg(id, id === cur, i === 0, compact))}
          </div>
        ) : (
          <span style={{ display: "inline-flex", alignItems: "center", gap: 8 }}>
            <span style={{ display: "inline-flex", color: "var(--tasty-text-muted)" }}><Icon name="textLeft" size={13} /></span>
            <Tag variant="accent">Text</Tag>
          </span>
        )}
        <span style={{ flex: 1 }} />
        {cur === "html"
          ? <Checkbox label="Pretty print" checked={state === "htmlPretty"} readOnly />
          : <span style={metaMono}>{meta}</span>}
      </div>
      {body}
      {/* footer */}
      <div style={{ display: "flex", alignItems: "center", gap: 12, padding: "8px 14px", flex: "none", borderTop: "1px solid var(--tasty-separator)" }}>
        <span style={metaMono}>{mime}</span>
        <span style={{ flex: 1 }} />
        <Button variant="secondary" size="sm">Close</Button>
      </div>
    </Frame>
  );
}
const TYPE_DEF = { text: ["Text", "textLeft"], image: ["Image", "image"], files: ["Files", "file"], html: ["HTML", "html"], other: ["Other", "layers"] };
const CB_SETS = { data: ["text"], multi: ["text", "image", "files"], html: ["text", "html"], htmlPretty: ["text", "html"],
  other: ["text", "other"], five: ["text", "image", "files", "html", "other"] };
const CB_HTML_RAW = `<div class="release"><h3>tasty 0.9.4</h3><ul><li><a href="https://github.com/zilhak/tasty/pull/812">#812</a> clipboard viewer: HTML type</li><li>#815 port scanner: refresh throttle</li></ul></div>`;
const CB_HTML_PRETTY = `<div class="release">
  <h3>
    tasty 0.9.4
  </h3>
  <ul>
    <li>
      <a href="https://github.com/zilhak/tasty/pull/812">
        #812
      </a>
      clipboard viewer: HTML type
    </li>
    <li>
      #815 port scanner: refresh throttle
    </li>
  </ul>
</div>`;
const CB_FORMATS = [
  { name: "RTF", size: "1.4 KB", content: "{\\rtf1\\ansi\\deff0{\\fonttbl{\\f0 D2Coding;}}\n\\f0\\fs20 cargo build -p tasty-gallery --release\\par", moreLines: 22 },
  { name: "Custom App Data", size: "860 B", content: "com.tasty.pane-ref v1\nsurface=3 pane=0 range=112:0-118:64" },
  { name: "text/csv", size: "204 B", content: "port,pid,process\n5173,48211,node", moreLines: 4 },
  { name: "application/x-vnd.oasis.opendocument.text", size: "12.8 KB", content: "PK\\x03\\x04\\x14\\x00 mimetypeapplication/vnd.oasis…" },
];

// ── Remote profile / passkey form — a route inside the 520×460 remote_tool popup ──
function RemoteFormFrame({ variant = "ssh", error }) {
  const passkey = variant.startsWith("passkey");
  const attach = variant.startsWith("attach");
  const attachInline = variant === "attach-inline";
  const FRow = ({ label, children, hint }) => (
    <>
      <div style={{ display: "grid", gridTemplateColumns: "var(--tasty-remote-label-col) 1fr", columnGap: 12, alignItems: "center" }}>
        <span style={{ textAlign: "right", fontSize: 13, color: "var(--tasty-text-muted)" }}>{label}</span>
        <div style={{ minWidth: 0 }}>{children}</div>
      </div>
      {hint && <div style={{ display: "grid", gridTemplateColumns: "var(--tasty-remote-label-col) 1fr", columnGap: 12 }}><span /><span style={{ fontSize: 11, color: "var(--tasty-text-muted)", lineHeight: 1.4 }}>{hint}</span></div>}
    </>
  );
  const peach = (txt) => (
    <span style={{ display: "inline-flex", alignItems: "center", height: 16, padding: "0 6px", borderRadius: "var(--tasty-radius-sm)",
      fontFamily: "var(--tasty-font-mono)", fontSize: 10, fontWeight: 500, color: "var(--tasty-accent-warning)", border: "1px solid var(--tasty-accent-warning)" }}>{txt}</span>
  );
  const seg = (label, active) => (
    <div style={{ display: "flex", alignItems: "center", height: "var(--tasty-control-height)", padding: "0 12px", borderRadius: "var(--tasty-radius)", fontSize: 13, cursor: "pointer",
      background: active ? "var(--tasty-surface-active)" : "var(--tasty-surface-raised)", color: active ? "var(--tasty-text-primary)" : "var(--tasty-text-secondary)",
      border: "1px solid " + (active ? "var(--tasty-border-strong)" : "var(--tasty-border-default)") }}>{label}</div>
  );
  return (
    <div style={{ width: "100%", maxWidth: 460, height: 408, display: "flex", flexDirection: "column",
      background: "var(--tasty-bg-panel)", border: "1px solid var(--tasty-border-strong)", borderRadius: "var(--tasty-radius)", boxShadow: "var(--tasty-shadow-modal)", overflow: "hidden" }}>
      {/* header */}
      <div style={{ display: "flex", alignItems: "center", gap: 8, padding: "10px 14px", flex: "none", borderBottom: "1px solid var(--tasty-separator)" }}>
        <span style={{ display: "inline-flex", color: "var(--tasty-text-muted)" }}>{ic.remote}</span>
        <span style={{ fontSize: 14, fontWeight: 600 }}>Remote connections</span>
        <span style={{ flex: 1 }} />
        <IconButton size="sm" aria-label="Close">{ic.x}</IconButton>
      </div>
      {/* tabs */}
      <div style={{ display: "flex", gap: 4, padding: "6px 10px 0", flex: "none", background: "var(--tasty-bg-sidebar)", borderBottom: "1px solid var(--tasty-separator)" }}>
        {[["Remote profiles", !passkey && !attach], ["Attach", attach], ["Passkeys", passkey]].map(([t, a]) => (
          <div key={t} style={{ padding: "6px 12px", fontSize: 13, cursor: "pointer", color: a ? "var(--tasty-text-primary)" : "var(--tasty-text-muted)",
            boxShadow: a ? "inset 0 -2px 0 var(--tasty-accent-primary)" : "none" }}>{t}</div>
        ))}
      </div>
      {/* body */}
      <div style={{ flex: 1, minHeight: 0, overflow: "auto", padding: "12px 16px", display: "flex", flexDirection: "column", gap: 8 }}>
        <div style={{ fontSize: 13, fontWeight: 600, marginBottom: 2 }}>{passkey ? "New passkey" : attach ? "New attach" : "New profile"}</div>
        {!passkey && !attach && (
          <>
            <FRow label="Type"><div style={{ display: "flex", gap: 4 }}><Input block defaultValue={variant === "generic" ? "smb" : "ssh"} /><Select options={["ssh", "smb", "http"]} style={{ width: 64 }} /></div></FRow>
            {variant === "generic" && error !== "type" && <div style={{ display: "grid", gridTemplateColumns: "var(--tasty-remote-label-col) 1fr", columnGap: 12 }}><span /><span>{peach("Unknown type")}</span></div>}
            <FRow label="Name"><Input block defaultValue={variant === "generic" ? "fileshare" : "prod-web"} /></FRow>
            {variant === "ssh" ? (
              <>
                <FRow label="Host"><Input block defaultValue="10.0.0.4" /></FRow>
                <FRow label="User"><Input block defaultValue="deploy" /></FRow>
                <FRow label="Port"><Input style={{ width: 96 }} defaultValue="22" /></FRow>
                <FRow label="Label"><Input block placeholder="optional" /></FRow>
                <FRow label="Shell" hint="Auto-detects the remote shell once when saved (runs an SSH probe)."><Select options={["auto", "bash", "zsh", "fish"]} block /></FRow>
                <FRow label="Passkey"><Select options={["(none)", "id_ed25519", "deploy-key"]} block /></FRow>
              </>
            ) : (
              <>
                <div style={{ fontFamily: "var(--tasty-font-mono)", fontSize: 10, textTransform: "uppercase", letterSpacing: ".06em", color: "var(--tasty-text-muted)", marginTop: 4 }}>Fields</div>
                {[["host", "fs.local"], ["share", "team"]].map(([k, v]) => (
                  <div key={k} style={{ display: "grid", gridTemplateColumns: "var(--tasty-remote-label-col) 1fr 28px", columnGap: 12, alignItems: "center" }}>
                    <Input defaultValue={k} style={{ width: "100%" }} /><Input defaultValue={v} block /><IconButton size="sm" aria-label="Remove field">{ic.x}</IconButton>
                  </div>
                ))}
                <div><Button variant="ghost" size="sm" leadingIcon={ic.plus}>Add field</Button></div>
                <FRow label="Passkey"><Select options={["(none)", "smb-cred"]} block /></FRow>
              </>
            )}
          </>
        )}
        {attach && (
          <>
            <FRow label="Name"><Input block defaultValue="gb10" /></FRow>
            <FRow label="Label"><Input block placeholder="optional" defaultValue={attachInline ? "" : "us-east"} /></FRow>
            <FRow label="Connection"><div style={{ display: "flex", gap: 6 }}>{seg("SSH profile", !attachInline)}{seg("Direct (inline)", attachInline)}</div></FRow>
            {attachInline ? (
              <>
                <FRow label="Host"><Input block defaultValue="edge.example.com" /></FRow>
                <FRow label="User"><Input block defaultValue="root" /></FRow>
                <FRow label="Port"><Input style={{ width: 96 }} defaultValue="22" /></FRow>
                <FRow label="Shell"><Select options={["auto", "bash", "zsh", "fish"]} block /></FRow>
                <FRow label="Passkey"><Select options={["(none)", "edge-pem"]} block /></FRow>
              </>
            ) : (
              <FRow label="SSH profile"><Select options={["(select a profile)", "prod-web (us-east)", "db-primary", "legacy-box"]} block /></FRow>
            )}
            <div style={{ fontFamily: "var(--tasty-font-mono)", fontSize: 10, textTransform: "uppercase", letterSpacing: ".06em", color: "var(--tasty-text-muted)", marginTop: 4 }}>Remote tasty</div>
            <FRow label="Executable"><Input block defaultValue={attachInline ? "/opt/tasty/bin/tasty" : "tasty"} /></FRow>
            <FRow label="Port mode"><Select options={["auto", "subcommand", "file-unix", "file-windows"]} block /></FRow>
            <FRow label="Port file" hint="Optional — an explicit path takes precedence over the port mode."><Input block placeholder="optional path" defaultValue={attachInline ? "/run/user/1000/tasty/port" : ""} /></FRow>
          </>
        )}
        {passkey && (
          <>
            <FRow label="Name"><Input block defaultValue="id_ed25519" /></FRow>
            <FRow label="Kind"><div style={{ display: "flex", gap: 6 }}>{seg("path", variant === "passkey-path")}{seg("inline", variant === "passkey-inline")}</div></FRow>
            {variant === "passkey-path"
              ? <FRow label="Value" hint="References a key file you own."><Input block defaultValue="~/.ssh/id_ed25519" /></FRow>
              : <FRow label="Value" hint="Pasted secret is materialized to a 0600-managed file."><textarea rows={3} placeholder="Paste secret / key contents" style={{ width: "100%", resize: "none", fontFamily: "var(--tasty-font-mono)", fontSize: 12, padding: 8, background: "var(--tasty-input-bg)", color: "var(--tasty-input-fg)", border: "1px solid var(--tasty-input-border)", borderRadius: "var(--tasty-radius)" }} /></FRow>}
            <div style={{ display: "grid", gridTemplateColumns: "var(--tasty-remote-label-col) 1fr", columnGap: 12 }}><span /><span style={{ fontSize: 11, color: "var(--tasty-text-muted)", lineHeight: 1.4 }}>Local-only. Profiles reference this passkey by name; the secret is never shared.</span></div>
          </>
        )}
        {error && <div style={{ fontSize: 11, color: "var(--tasty-accent-danger)", marginTop: 4 }}>{error === "type" ? "Type is required." : error === "host" ? "Host is required." : "Port must be 1–65535."}</div>}
      </div>
      {/* footer — full-width separator, buttons inset */}
      <div style={{ flex: "none", borderTop: "1px solid var(--tasty-separator)", padding: "12px 16px", display: "flex", justifyContent: "flex-end", gap: 8 }}>
        <Button variant="ghost" size="sm">Cancel</Button>
        <Button variant="primary" size="sm">Save</Button>
      </div>
    </div>
  );
}

// ── Scripts subsection (Settings › Misc › Scripts) — Lua script manager ──
function ScriptManagerFrame({ empty }) {
  const sIcon = <Icon name="scriptFile" size={16} />;
  const kbdIcon = <Icon name="keyboard" size={16} />;
  const changedBadge = (
    <span style={{ display: "inline-flex", alignItems: "center", gap: 3, height: 16, padding: "0 6px", borderRadius: "var(--tasty-radius-sm)",
      fontFamily: "var(--tasty-font-mono)", fontSize: 10, fontWeight: 500, color: "var(--tasty-accent-warning)",
      border: "1px solid color-mix(in srgb, var(--tasty-accent-warning) 40%, transparent)",
      background: "color-mix(in srgb, var(--tasty-accent-warning) 12%, transparent)" }}>changed</span>
  );
  const xIcon = <Icon name="close" size={12} />;
  const caretIcon = <Icon name="chevronDown" size={12} />;
  // Auto-run trigger chip (static mirror — whole chip is the remove affordance)
  const TriggerChip = ({ event }) => (
    <span style={{ display: "inline-flex", alignItems: "center", gap: 4, height: 16, padding: "0 4px", flex: "none",
      borderRadius: "var(--tasty-radius-sm)", border: "1px solid var(--tasty-border-default)", whiteSpace: "nowrap",
      fontFamily: "var(--tasty-font-mono)", fontSize: 10, lineHeight: 1, color: "var(--tasty-text-secondary)" }}>
      <span>{event}</span><span style={{ display: "inline-flex", color: "var(--tasty-text-muted)" }}>{xIcon}</span>
    </span>
  );
  // Auto-run row: caption + chips + dashed add control (menu of remaining events)
  const TriggerRow = ({ triggers }) => (
    <div style={{ display: "flex", alignItems: "center", flexWrap: "wrap", gap: 4, marginTop: 2 }}>
      <span style={{ flex: "none", fontSize: 11, color: "var(--tasty-text-muted)" }}>Auto-run:</span>
      {triggers.map((e) => <TriggerChip key={e} event={e} />)}
      <span style={{ display: "inline-flex", alignItems: "center", gap: 4, height: 16, padding: "0 4px", flex: "none",
        borderRadius: "var(--tasty-radius-sm)", border: "1px dashed var(--tasty-border-default)", whiteSpace: "nowrap",
        fontFamily: "var(--tasty-font-mono)", fontSize: 10, lineHeight: 1, color: "var(--tasty-text-muted)" }}>
        <span>Add trigger…</span><span style={{ display: "inline-flex" }}>{caretIcon}</span>
      </span>
    </div>
  );
  const Path = ({ dir, file }) => (
    <span style={{ display: "flex", minWidth: 0, fontFamily: "var(--tasty-font-mono)", fontSize: 12 }}>
      <span style={{ flex: "0 1 auto", overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap", minWidth: 0, color: "var(--tasty-text-muted)" }}>{dir}</span>
      <span style={{ flex: "none", color: "var(--tasty-text-secondary)" }}>{file}</span>
    </span>
  );
  const Row = ({ name, dir, file, shortcut, changed, triggers }) => (
    <div style={{ display: "flex", alignItems: "flex-start", gap: 12, padding: "8px 4px", borderBottom: "1px solid var(--tasty-separator)" }}>
      <span style={{ display: "inline-flex", flex: "none", marginTop: 2, color: "var(--tasty-text-muted)" }}>{sIcon}</span>
      <div style={{ flex: 1, minWidth: 0, display: "flex", flexDirection: "column", gap: 2 }}>
        <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
          <span style={{ fontSize: 13, fontWeight: 600, color: "var(--tasty-text-primary)" }}>{name}</span>
          {changed && changedBadge}
        </div>
        <Path dir={dir} file={file} />
        {changed && <span style={{ fontSize: 11, color: "var(--tasty-accent-warning)", lineHeight: 1.4 }}>File changed since registration — you'll be asked to confirm on next run.</span>}
        <TriggerRow triggers={triggers || []} />
      </div>
      <div style={{ display: "flex", alignItems: "center", gap: 6, flex: "none" }}>
        {shortcut ? <Kbd keys={shortcut} /> : <span style={{ fontSize: 12, color: "var(--tasty-text-disabled)", fontStyle: "italic" }}>Unbound</span>}
        <IconButton size="sm" aria-label="Bind shortcut">{kbdIcon}</IconButton>
        <IconButton size="sm" aria-label="Rename">{ic.edit}</IconButton>
        <IconButton size="sm" aria-label="Remove">{ic.x}</IconButton>
      </div>
    </div>
  );
  return (
    <div style={{ width: "100%", maxWidth: 560, display: "flex", flexDirection: "column", gap: 14, padding: 18,
      background: "var(--tasty-bg-panel)", border: "1px solid var(--tasty-border-strong)", borderRadius: "var(--tasty-radius)", boxShadow: "var(--tasty-shadow-modal)" }}>
      <div style={{ display: "flex", alignItems: "flex-start", gap: 12 }}>
        <div style={{ flex: 1 }}>
          <div style={{ fontSize: 16, fontWeight: 600, color: "var(--tasty-text-primary)" }}>Scripts</div>
          <p style={{ fontSize: 12, color: "var(--tasty-text-muted)", margin: "2px 0 0", lineHeight: 1.5 }}>Register and manage Lua scripts you can run with a shortcut. Binding a trigger is done in <b style={{ color: "var(--tasty-text-secondary)" }}>Keybindings</b>; each script is verified against the SHA recorded when it was added.</p>
        </div>
        <Button variant="secondary" size="sm" leadingIcon={ic.plus}>Add script</Button>
      </div>
      {empty ? (
        <div style={{ display: "flex", flexDirection: "column", alignItems: "center", justifyContent: "center", gap: 8, textAlign: "center", padding: "32px 0", color: "var(--tasty-text-muted)" }}>
          <Icon name="scriptFile" size={26} />
          <div style={{ fontSize: 14, color: "var(--tasty-text-secondary)" }}>No scripts registered</div>
          <p style={{ fontSize: 12, margin: 0, maxWidth: 300, lineHeight: 1.5 }}>Click <b style={{ color: "var(--tasty-text-secondary)" }}>Add script</b> to register a Lua script and bind it to a shortcut.</p>
        </div>
      ) : (
        <div style={{ display: "flex", flexDirection: "column" }}>
          <Row name="Reformat JSON" dir="~/.tasty/scripts/" file="reformat-json.lua" shortcut="Ctrl+Shift+J" triggers={["clipboard.copy.post"]} />
          <Row name="Tail & highlight errors" dir="~/.tasty/scripts/" file="tail-errors.lua" shortcut="" triggers={["pane.create.post", "session.start.post"]} />
          <Row name="Deploy staging" dir="~/work/ops/tasty/" file="deploy-staging.lua" shortcut="Ctrl+Alt+D" changed triggers={[]} />
        </div>
      )}
    </div>
  );
}

// ════════════════════════════════════════════════════════════════════════
//  Workspace categories (sidebar folders) — context menu, rail popup, dialogs
//  Static mirrors of ui_kits/terminal/{chrome.jsx, overlays/sidebar_context_menu.jsx}.
// ════════════════════════════════════════════════════════════════════════
const catIc = {
  plus: <Icon name="plus" />,
  edit: <Icon name="edit" />,
  trash: <Icon name="trash" />,
  folder: <Icon name="folder" />,
  move: <Icon name="move" />,
  chevD: <Icon name="chevronDown" size={14} />,
  chevR: <Icon name="chevronRight" size={13} />,
};
const catMenuPanel = { background: "var(--tasty-surface-raised)", border: "1px solid var(--tasty-border-strong)",
  borderRadius: "var(--tasty-radius)", padding: 6, boxShadow: "var(--tasty-shadow-popover)", minWidth: 176 };

// small labelled menu column (for the "target resolves to N shapes" layout)
function CatMenu({ caption, children }) {
  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 6 }}>
      <span style={{ fontFamily: "var(--tasty-font-mono)", fontSize: 10, color: "var(--tasty-text-muted)" }}>{caption}</span>
      <div role="menu" style={catMenuPanel}>{children}</div>
    </div>
  );
}

// Faux full sidebar with two category groups + one empty/collapsed category.
function CategorySidebarFrame({ collapsedRail }) {
  const Head = ({ label, collapsed, reserved }) => (
    <div style={{ display: "flex", alignItems: "center", gap: 4, padding: "4px 8px", marginTop: 8, cursor: "pointer" }}>
      <span style={{ display: "inline-flex", width: 12, justifyContent: "center", color: "var(--tasty-text-muted)",
        transform: collapsed ? "none" : "rotate(0deg)" }}>{collapsed ? catIc.chevR : catIc.chevD}</span>
      <span style={{ flex: 1, fontFamily: "var(--tasty-font-mono)", fontSize: 10, textTransform: "uppercase", letterSpacing: ".07em", color: "var(--tasty-text-muted)" }}>{label}</span>
    </div>
  );
  const Row = ({ name, sub, status, active }) => (
    <div style={{ display: "flex", alignItems: "flex-start", gap: 8, padding: "8px", cursor: "pointer",
      background: active ? "var(--tasty-surface-active)" : "transparent", boxShadow: active ? "inset 2px 0 0 var(--tasty-accent-primary)" : "none" }}>
      <span style={{ height: 18, display: "inline-flex", alignItems: "center", flex: "none" }}>
        <StatusDot status={status} pulse={status === "running" || status === "agent"} /></span>
      <div style={{ flex: 1, minWidth: 0 }}>
        <div style={{ fontSize: 13, fontWeight: 500, color: active ? "var(--tasty-text-primary)" : "var(--tasty-text-secondary)", overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>{name}</div>
        {sub && <div style={{ fontSize: 11, color: "var(--tasty-text-muted)", marginTop: 1 }}>{sub}</div>}
      </div>
    </div>
  );
  const List = ({ children }) => <div style={{ borderTop: "1px solid var(--tasty-separator)" }}>{children}</div>;
  return (
    <div style={{ width: 212, flex: "none", background: "var(--tasty-bg-sidebar)", border: "1px solid var(--tasty-separator)",
      borderRadius: "var(--tasty-radius)", overflow: "hidden", height: 380, display: "flex", flexDirection: "column" }}>
      <Head label="Workspaces" reserved /><List><Row name="agents-prod" status="running" active /><Row name="scratch" status="idle" /></List>
      <Head label="Services" /><List><Row name="infra" sub="terraform + k8s" status="idle" /><Row name="api-gateway" sub="agent" status="agent" /><Row name="data-pipeline" sub="spark · 4 nodes" status="running" /></List>
      <Head label="Archived" collapsed />
      <div style={{ marginTop: "auto", padding: 8, borderTop: "1px solid var(--tasty-separator)" }}>
        <Button variant="ghost" size="sm" block leadingIcon={catIc.plus} style={{ justifyContent: "flex-start" }}>New Workspace</Button>
      </div>
    </div>
  );
}

// Faux 52px rail: `---` category buttons + letter avatars, with the anchored popup.
function RailCategoryFrame() {
  const Sep = () => <div style={{ width: 24, height: 1, background: "var(--tasty-separator)", margin: "3px 0" }} />;
  const Av = ({ ch, active, dot }) => (
    <div style={{ position: "relative", width: 28, height: 28, borderRadius: "var(--tasty-radius)", display: "flex", alignItems: "center", justifyContent: "center",
      color: active ? "var(--tasty-text-primary)" : "var(--tasty-text-secondary)", background: active ? "var(--tasty-surface-active)" : "transparent",
      boxShadow: active ? "inset 0 0 0 1px var(--tasty-accent-primary)" : "none", fontFamily: "var(--tasty-font-mono)", fontSize: 13, fontWeight: 700 }}>
      {ch}{dot && <span style={{ position: "absolute", top: -1, right: -1, width: 6, height: 6, borderRadius: "50%", background: "var(--tasty-accent-primary)", boxShadow: "0 0 0 2px var(--tasty-bg-sidebar)" }} />}</div>
  );
  return (
    <div style={{ position: "relative", height: 380, display: "flex" }}>
      <div style={{ width: 52, flex: "none", background: "var(--tasty-bg-sidebar)", border: "1px solid var(--tasty-separator)", borderRadius: "var(--tasty-radius)",
        padding: "10px 0", display: "flex", flexDirection: "column", alignItems: "center", gap: 4 }}>
        <div style={{ width: 24, height: 24, borderRadius: 6, background: "var(--tasty-brand-melon-flesh)", marginBottom: 4 }} />
        <Sep /><Av ch="A" active /><Av ch="S" />
        <div title="Services" style={{ padding: "2px 0", cursor: "pointer" }}><div style={{ width: 24, height: 1, background: "var(--tasty-text-muted)" }} /></div>
        <Av ch="I" /><Av ch="A" dot /><Av ch="D" dot />
        <Sep />{/* Archived — empty, header-only */}
      </div>
      {/* anchored popup to the right of the Services `---` button */}
      <div role="menu" aria-label="Services" style={{ position: "absolute", left: 60, top: 118, ...catMenuPanel }}>
        <div style={{ display: "flex", alignItems: "baseline", gap: 8, padding: "4px 8px 8px", borderBottom: "1px solid var(--tasty-separator)", marginBottom: 4 }}>
          <span style={{ flex: 1, fontSize: 13, fontWeight: 600, color: "var(--tasty-text-primary)" }}>Services</span>
          <span style={{ fontFamily: "var(--tasty-font-mono)", fontSize: 10, color: "var(--tasty-text-muted)" }}>3</span>
        </div>
        <MenuItem label="Add workspace" icon={catIc.plus} />
        <MenuItem label="Collapse" icon={catIc.chevD} />
        <MenuItem separator />
        <MenuItem label="Rename category" icon={catIc.edit} />
        <MenuItem label="Delete category" icon={catIc.trash} danger />
      </div>
    </div>
  );
}

// Create / rename category — 360px single-field dialog (Rename pattern) + validation.
function CategoryEditFrame({ mode = "new", error }) {
  const errText = error === "empty" ? "Enter a category name." : error === "reserved" ? "\u201Cnormal\u201D is reserved." : error === "duplicate" ? "A category named \u201CServices\u201D already exists." : null;
  return (
    <div style={{ width: 360, background: "var(--tasty-bg-panel)", border: "1px solid var(--tasty-border-strong)",
      borderRadius: "var(--tasty-radius)", overflow: "hidden", boxShadow: "0 20px 60px rgba(0,0,0,.55)" }}>
      <div style={{ padding: "14px 14px 0" }}>
        <div style={{ fontSize: 14, fontWeight: 600, marginBottom: 2 }}>{mode === "new" ? "New category" : "Rename category"}</div>
        <div style={{ fontSize: 12, color: "var(--tasty-text-muted)", marginBottom: 12 }}>Press <Kbd keys="↵" /> to confirm, <Kbd keys="Esc" /> to cancel.</div>
        <Input block autoFocus defaultValue={mode === "new" ? "" : "Services"} placeholder="Category name"
          style={errText ? { borderColor: "var(--tasty-accent-danger)" } : undefined} />
        {errText && <div style={{ fontSize: 11, color: "var(--tasty-accent-danger)", marginTop: 6 }}>{errText}</div>}
      </div>
      <div style={{ display: "flex", justifyContent: "flex-end", gap: 8, padding: 14 }}>
        <Button variant="ghost">Cancel</Button>
        <Button variant="primary" disabled={!!error}>{mode === "new" ? "Create" : "Rename"}</Button>
      </div>
    </div>
  );
}

// Delete category — destructive confirm; the category's workspaces move to normal.
function CategoryDeleteFrame() {
  return (
    <div style={{ width: 380, background: "var(--tasty-bg-panel)", border: "1px solid var(--tasty-border-strong)",
      borderRadius: "var(--tasty-radius)", overflow: "hidden", boxShadow: "0 20px 60px rgba(0,0,0,.55)" }}>
      <div style={{ display: "flex", alignItems: "center", gap: 8, padding: "14px 14px 0" }}>
        <span style={{ display: "inline-flex", color: "var(--tasty-accent-danger)" }}>{catIc.trash}</span>
        <span style={{ fontSize: 14, fontWeight: 600 }}>Delete category?</span>
      </div>
      <p style={{ margin: "10px 14px 0", fontSize: 13, color: "var(--tasty-text-secondary)", lineHeight: 1.5 }}>
        Delete <b style={{ color: "var(--tasty-text-primary)" }}>Services</b>? Its <b>3 workspaces</b> aren't deleted — they move back to <b>Workspaces</b>.
      </p>
      <div style={{ display: "flex", justifyContent: "flex-end", gap: 8, padding: 14, marginTop: 4 }}>
        <Button variant="ghost">Cancel</Button>
        <Button variant="danger">Delete category</Button>
      </div>
    </div>
  );
}

// ── Modifier hint panel — floating, focus-less discoverability overlay ──
// Painted while a modifier is HELD; lists every chord that contains it,
// grouped by chord (size-ascending, then Ctrl→Cmd/Alt→Option→Shift), plus
// any special role the chord carries. Draggable + edge-resizable, an X for a
// this-session dismiss. NEVER takes focus — the muted drag strip (sidebar
// fill, not a titlebar) is the visual promise of that. Content below is the
// "Ctrl held" instance (cross-platform: the same on Win/Linux and macOS).
const mhIc = {
  hash: <Icon name="hash" />,
  mouse: <Icon name="mouse" />,
  folder: <Icon name="folder" />,
};

// chord shown as keycaps + "+" joiners — reuses the Kbd vocabulary as the header
function ChordHead({ keys }) {
  return (
    <div style={{ display: "flex", alignItems: "center", gap: 6, padding: "0 2px 6px",
      borderBottom: "var(--tasty-border-width) solid var(--tasty-separator)" }}>
      <Kbd keys={keys} />
    </div>
  );
}

// one keycap row inside a chord section — action label (elides) + its Kbd
function HintRow({ action, keys, plugin }) {
  return (
    <div style={{ display: "flex", alignItems: "center", gap: 8, minHeight: 24 }}>
      {plugin && <span aria-hidden style={{ width: "var(--tasty-status-dot-size)", height: "var(--tasty-status-dot-size)",
        borderRadius: "50%", flex: "none", background: "var(--tasty-accent-agent)" }} />}
      <span style={{ flex: 1, minWidth: 0, fontSize: 12, color: "var(--tasty-text-secondary)",
        overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>{action}</span>
      <span style={{ flex: "none" }}><Kbd keys={keys} /></span>
    </div>
  );
}

// special-role row — a full-width wash, leading glyph, no keycap. This is the
// "what does holding it DO" line (tab-switch numbers, mouse-capture bypass).
function RoleRow({ icon, children }) {
  return (
    <div style={{ display: "flex", alignItems: "flex-start", gap: 8, padding: "6px 8px", borderRadius: "var(--tasty-radius-sm)",
      background: "var(--tasty-modhint-role-bg)" }}>
      <span style={{ display: "inline-flex", flex: "none", marginTop: 1, color: "var(--tasty-modhint-role-fg)" }}>{icon}</span>
      <span style={{ flex: 1, fontSize: 12, lineHeight: "var(--tasty-line-height-ui)", color: "var(--tasty-text-secondary)" }}>{children}</span>
    </div>
  );
}

// empty-chord placeholder — the row for a chord that EXISTS but carries no
// binding and no role. Deliberately the QUIETEST row in the panel: muted text
// (--tasty-modhint-empty-fg → text-muted, one step below a keycap row), no
// keycap, no wash, no leading glyph. It is an ABSENCE signal, not a role-row,
// so it must never borrow the role-row's wash/glyph emphasis. Static & non-
// interactive (no hover, no focus, no click) — like every row in this overlay.
function EmptyRow({ label = "No shortcuts bound" }) {
  return (
    <div style={{ display: "flex", alignItems: "center", minHeight: 20, padding: "0 2px" }}>
      <span style={{ fontSize: 12, color: "var(--tasty-modhint-empty-fg)" }}>{label}</span>
    </div>
  );
}

// the panel body — the "Ctrl held" chord list. A chord with no binding and no
// role is NOT omitted: its ChordHead is shown with a single EmptyRow placeholder
// beneath, so holding an all-empty combo (e.g. Ctrl+Alt+Shift) still shows the
// panel with a "No shortcuts bound" line rather than nothing at all.
// (Reverses the 2026-07-02 decision "empty chord → omitted entirely, never 'none'".)
const MH_SECTIONS = [
  { keys: "Ctrl",
    role: <>Preview <b style={{ color: "var(--tasty-text-primary)" }}>tab-switch numbers</b> — <span style={{ fontFamily: "var(--tasty-font-mono)" }}>1</span>–<span style={{ fontFamily: "var(--tasty-font-mono)" }}>9</span>, <span style={{ fontFamily: "var(--tasty-font-mono)" }}>0</span> over each tab.</>, roleIcon: mhIc.hash,
    rows: [
      { action: "Command palette", keys: "Ctrl+K" },
      { action: "New tab", keys: "Ctrl+T" },
      { action: "Close tab", keys: "Ctrl+W" },
      { action: "Split vertical", keys: "Ctrl+D" },
      { action: "Settings", keys: "Ctrl+," },
    ] },
  { keys: "Ctrl+Alt",
    rows: [
      { action: "Apply workspace preset", keys: "Ctrl+Alt+1" },
      { action: "git-helper: Stage hunk", keys: "Ctrl+Alt+G", plugin: true },
      { action: "docker: Attach shell", keys: "Ctrl+Alt+D", plugin: true },
    ] },
  { keys: "Ctrl+Shift",
    rows: [
      { action: "New workspace", keys: "Ctrl+Shift+N" },
      { action: "Split horizontal", keys: "Ctrl+Shift+D" },
      { action: "Copy", keys: "Ctrl+Shift+C" },
      { action: "Paste", keys: "Ctrl+Shift+V" },
    ] },
];

// Alt+Shift held — the category quick-switch chord. Its own function (a numeric
// switch that jumps categories) rides in the special-role row, exactly like
// Ctrl = tab-switch numbers. Only present when the folders feature is on.
const MH_CAT_SECTIONS = [
  { keys: "Alt+Shift",
    role: <>Switch <b style={{ color: "var(--tasty-text-primary)" }}>category</b> — <span style={{ fontFamily: "var(--tasty-font-mono)" }}>1</span>–<span style={{ fontFamily: "var(--tasty-font-mono)" }}>9</span>, <span style={{ fontFamily: "var(--tasty-font-mono)" }}>0</span> over each category header; a collapsed target auto-expands.</>, roleIcon: mhIc.folder,
    rows: [] },
];

// Ctrl held, mixed — filled chords next to empty ones. Ctrl (role + rows) and
// Ctrl+Shift (rows) are bound; Ctrl+Alt and Ctrl+Alt+Shift are empty and draw
// the EmptyRow placeholder. Shows how a filled section and an absence section
// contrast within one list.
const MH_MIXED_SECTIONS = [
  { keys: "Ctrl",
    role: <>Preview <b style={{ color: "var(--tasty-text-primary)" }}>tab-switch numbers</b> — <span style={{ fontFamily: "var(--tasty-font-mono)" }}>1</span>–<span style={{ fontFamily: "var(--tasty-font-mono)" }}>9</span>, <span style={{ fontFamily: "var(--tasty-font-mono)" }}>0</span> over each tab.</>, roleIcon: mhIc.hash,
    rows: [
      { action: "Command palette", keys: "Ctrl+K" },
      { action: "New tab", keys: "Ctrl+T" },
      { action: "Close tab", keys: "Ctrl+W" },
    ] },
  { keys: "Ctrl+Alt", rows: [] },
  { keys: "Ctrl+Shift",
    rows: [
      { action: "New workspace", keys: "Ctrl+Shift+N" },
      { action: "Split horizontal", keys: "Ctrl+Shift+D" },
      { action: "git-helper: Stage hunk", keys: "Ctrl+Shift+G", plugin: true },
    ] },
  { keys: "Ctrl+Alt+Shift", rows: [] },
];

// Ctrl+Alt+Shift held — the all-empty case that motivated the empty-state: no
// superset chord carries a binding, so the panel is nothing but ChordHead +
// placeholder (previously it never appeared at all).
const MH_EMPTY_SECTIONS = [
  { keys: "Ctrl+Alt+Shift", rows: [] },
];

function ModifierHintPanelG({ style, held = "Ctrl", sections = MH_SECTIONS, showGrip = true }) {
  return (
    <div style={{ display: "flex", flexDirection: "column", flex: "none",
      width: "var(--tasty-modhint-width)", height: "var(--tasty-modhint-height)",
      background: "var(--tasty-modhint-bg)", border: "var(--tasty-border-width) solid var(--tasty-modhint-border)",
      borderRadius: "var(--tasty-modhint-radius)", boxShadow: "var(--tasty-modhint-shadow)", overflow: "hidden",
      position: "relative", ...style }}>
      {/* drag strip — muted (sidebar) fill, NOT a focus titlebar */}
      <div style={{ display: "flex", alignItems: "center", gap: 8, flex: "none", height: "var(--tasty-modhint-header-height)",
        padding: "0 4px 0 10px", cursor: "move", userSelect: "none",
        background: "var(--tasty-modhint-header-bg)", borderBottom: "var(--tasty-border-width) solid var(--tasty-separator)" }}>
        <Kbd keys={held} />
        <span style={{ fontSize: 11, color: "var(--tasty-text-muted)" }}>held</span>
        <span style={{ marginLeft: "auto" }}>
          <IconButton size="sm" aria-label="Hide for this hold">{ic.x}</IconButton>
        </span>
      </div>
      {/* scrolling chord list */}
      <div className="tasty-scroll" style={{ flex: 1, minHeight: 0, overflow: "auto", padding: "10px",
        display: "flex", flexDirection: "column", gap: "var(--tasty-modhint-section-gap)" }}>
        {sections.map((s) => {
          const isEmpty = !s.role && (!s.rows || s.rows.length === 0);
          return (
          <div key={s.keys} style={{ display: "flex", flexDirection: "column", gap: isEmpty ? 3 : 6 }}>
            <ChordHead keys={s.keys} />
            {s.role && <RoleRow icon={s.roleIcon}>{s.role}</RoleRow>}
            {(s.rows || []).map((r) => <HintRow key={r.keys} {...r}
              keys={r.keys.startsWith(s.keys + "+") ? r.keys.slice(s.keys.length + 1) : r.keys} />)}
            {isEmpty && <EmptyRow />}
          </div>
          );
        })}
      </div>
      {/* bottom-right resize grip */}
      {showGrip && (
        <span aria-hidden style={{ position: "absolute", right: 2, bottom: 2, width: 12, height: 12, cursor: "nwse-resize" }}>
          <svg viewBox="0 0 12 12" width="12" height="12" fill="none"
            stroke="var(--tasty-modhint-grip-fg)" strokeWidth="1" strokeLinecap="round">
            <path d="M11 5 5 11M11 9 9 11" />
          </svg>
        </span>
      )}
    </div>
  );
}

// ── Add remote workspace — 680×460 two-pane remote-workspace picker (NEW) ──
// Consumes the tasty-attach profiles (left) → lists a remote instance's workspaces
// (right) across 4 states. Same shell language as RemoteFrame (remote_tool).
function RaBadge({ tone, children, title }) {
  const c = tone === "attached" ? "var(--tasty-accent-attached)" : "var(--tasty-accent-warning)";
  const pct = tone === "attached" ? "45%" : "40%";
  const bg = tone === "attached" ? "14%" : "12%";
  return (
    <span title={title} style={{ display: "inline-flex", alignItems: "center", gap: tone === "attached" ? 0 : 4,
      height: 16, padding: "0 var(--tasty-space-sm)", borderRadius: "var(--tasty-radius-sm)", whiteSpace: "nowrap",
      fontFamily: "var(--tasty-font-mono)", fontSize: "var(--tasty-font-size-micro)", fontWeight: 500, lineHeight: 1, color: c,
      border: `1px solid color-mix(in srgb, ${c} ${pct}, transparent)`, background: `color-mix(in srgb, ${c} ${bg}, transparent)` }}>
      {tone !== "attached" && <span style={{ display: "inline-flex" }}>{ic.warn}</span>}{children}
    </span>
  );
}
function RaProfile({ name, label, target, selected, inactive }) {
  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 2, padding: "var(--tasty-space-sm) var(--tasty-space-md)", position: "relative",
      background: selected ? "var(--tasty-surface-active)" : "transparent",
      boxShadow: selected ? "inset 2px 0 0 var(--tasty-accent-primary)" : "none" }}>
      <div style={{ display: "flex", alignItems: "center", gap: 8, minWidth: 0 }}>
        <span style={{ fontSize: 13, fontWeight: 600, color: selected ? "var(--tasty-text-primary)" : "var(--tasty-text-secondary)" }}>
          {name}{label && <span style={{ fontWeight: 400, color: "var(--tasty-text-muted)" }}>  ({label})</span>}
        </span>
        {inactive && <RaBadge title="Inactive">inactive</RaBadge>}
      </div>
      <div style={{ fontFamily: "var(--tasty-font-mono)", fontSize: 11, color: "var(--tasty-text-muted)" }}>{target}</div>
    </div>
  );
}
function RaWs({ name, panes, busy, selected, attached }) {
  return (
    <div style={{ display: "flex", alignItems: "center", gap: 8, padding: "var(--tasty-space-sm) var(--tasty-space-md)", position: "relative",
      background: selected ? "var(--tasty-surface-active)" : "transparent",
      boxShadow: selected ? "inset 2px 0 0 var(--tasty-accent-primary)" : "none" }}>
      <span style={{ opacity: attached ? 0.5 : 1, display: "inline-flex" }}><StatusDot status={busy ? "running" : "idle"} pulse={busy} /></span>
      <span style={{ fontSize: 13, fontWeight: 500, color: attached ? "var(--tasty-text-disabled)" : selected ? "var(--tasty-text-primary)" : "var(--tasty-text-secondary)" }}>{name}</span>
      <span style={{ display: "inline-flex", alignItems: "center", gap: 4, fontSize: 11, color: "var(--tasty-text-muted)", opacity: attached ? 0.6 : 1 }}>{ic.split}{panes}</span>
      <div style={{ flex: 1 }} />
      {busy && !attached && <span style={{ fontSize: 11, color: "var(--tasty-text-muted)" }}>busy</span>}
      {attached && <RaBadge tone="attached" title="Already attached on another client">in use</RaBadge>}
    </div>
  );
}
function RaNewRow({ selected, hover, phase = "rest", error, sep = true }) {
  const creating = phase === "creating";
  const failed = phase === "failed";
  return (
    <div>
      <div style={{ display: "flex", alignItems: "center", gap: 8, padding: "var(--tasty-space-sm) var(--tasty-space-md)", position: "relative",
        background: selected ? "var(--tasty-surface-active)" : hover ? "var(--tasty-overlay-hover)" : "transparent",
        boxShadow: selected ? "inset 2px 0 0 var(--tasty-accent-primary)" : "none" }}>
        <span style={{ flex: "none", width: "var(--tasty-status-dot-size)", display: "inline-flex", alignItems: "center", justifyContent: "center",
          color: creating ? "var(--tasty-text-muted)" : failed ? "var(--tasty-accent-danger)" : "var(--tasty-accent-primary)" }}>
          {creating ? <Spinner size={14} /> : <Icon name={failed ? "alertTriangle" : "plus"} size={14} />}
        </span>
        <span style={{ fontSize: 13, fontWeight: 500, whiteSpace: "nowrap",
          color: creating ? "var(--tasty-text-muted)" : selected ? "var(--tasty-text-primary)" : "var(--tasty-accent-primary)" }}>
          {creating ? "Creating workspace…" : "New workspace"}
        </span>
        <div style={{ flex: 1 }} />
        {!creating && !failed && <span style={{ fontSize: 11, color: "var(--tasty-text-muted)" }}>on remote</span>}
      </div>
      {failed && (
        <div style={{ display: "flex", alignItems: "flex-start", gap: 8, padding: "var(--tasty-space-xs) var(--tasty-space-md) var(--tasty-space-sm)" }}>
          <span style={{ flex: "none", width: "var(--tasty-status-dot-size)" }} />
          <div style={{ flex: 1, minWidth: 0, display: "flex", flexDirection: "column", alignItems: "flex-start", gap: 4 }}>
            <span style={{ fontSize: 11, color: "var(--tasty-accent-danger)", lineHeight: 1.5,
              display: "-webkit-box", WebkitLineClamp: 3, WebkitBoxOrient: "vertical", overflow: "hidden" }}>
              {error || "Remote refused: workspace quota reached (8/8) on edge.example.com. Close a workspace there, or raise `limits.workspaces` in the remote's tasty.toml, then try again."}
            </span>
            <Button variant="secondary" size="sm" leadingIcon={ic.refresh}>Try again</Button>
          </div>
        </div>
      )}
      {sep && <div style={{ height: 1, margin: "var(--tasty-space-xs) 0", background: "var(--tasty-separator)" }} />}
    </div>
  );
}
// one real remote-ws row, for the new-workspace row-state specimens (alignment check)
function RaWsPeek() {
  return <RaWs name="agents-prod" panes={3} busy />;
}
function RemoteAttachFrame({ state = "loaded", newPhase = "rest", newSelected, emptyPlan = "B" }) {
  const caps = { fontFamily: "var(--tasty-font-mono)", fontSize: 10, textTransform: "uppercase", letterSpacing: ".06em", color: "var(--tasty-text-muted)" };
  const center = (kids) => (
    <div style={{ flex: 1, minHeight: 0, display: "flex", flexDirection: "column", alignItems: "center", justifyContent: "center",
      textAlign: "center", gap: "var(--tasty-space-sm)", padding: "var(--tasty-space-xl) var(--tasty-space-lg)" }}>{kids}</div>
  );
  const errSel = state === "error";
  return (
    <div style={{ width: 680, height: 460, display: "flex", flexDirection: "column", background: "var(--tasty-bg-panel)",
      border: "1px solid var(--tasty-border-strong)", borderRadius: "var(--tasty-radius)", overflow: "hidden", boxShadow: "var(--tasty-shadow-modal)" }}>
      {/* header */}
      <div style={{ display: "flex", alignItems: "center", gap: 8, padding: "10px 10px 10px 14px", flex: "none", borderBottom: "1px solid var(--tasty-separator)" }}>
        <span style={{ display: "inline-flex", color: "var(--tasty-text-muted)" }}>{ic.remote}</span>
        <span style={{ fontSize: 14, fontWeight: 600, color: "var(--tasty-text-primary)" }}>Add remote workspace</span>
        <div style={{ flex: 1 }} />
        <IconButton size="sm" aria-label="Close">{ic.x}</IconButton>
      </div>
      {/* body */}
      <div style={{ flex: 1, minHeight: 0, display: "flex" }}>
        {/* left */}
        <div style={{ width: 240, flex: "none", display: "flex", flexDirection: "column", borderRight: "1px solid var(--tasty-separator)", background: "var(--tasty-bg-sidebar)" }}>
          <div style={{ ...caps, padding: "10px 12px 4px" }}>Attach profiles</div>
          <div style={{ flex: 1, minHeight: 0, overflow: "hidden" }}>
            <RaProfile name="prod-web" label="us-east" target="deploy@10.0.4.12" selected={state === "loaded"} />
            <RaProfile name="gb10" target="→ prod-web" selected={state === "loading"} />
            <RaProfile name="edge-direct" target="root@edge.example.com" />
            <RaProfile name="media-nas" label="lab" target="→ nas.local" selected={state === "empty"} />
            <RaProfile name="legacy-attach" target="→ legacy-box" inactive selected={errSel} />
          </div>
        </div>
        {/* right */}
        <div style={{ flex: 1, minWidth: 0, display: "flex", flexDirection: "column" }}>
          {state === "initial" && center(<>
            <span style={{ display: "inline-flex", color: "var(--tasty-text-placeholder)", transform: "scale(1.4)" }}>{ic.remote}</span>
            <span style={{ fontSize: 13, color: "var(--tasty-text-muted)" }}>Select an attach profile</span>
            <span style={{ fontSize: 11, color: "var(--tasty-text-muted)", maxWidth: 300 }}>Pick a profile on the left to connect and list the remote instance's workspaces.</span>
          </>)}
          {state === "loading" && center(<>
            <Spinner size={22} />
            <span style={{ fontSize: 13, color: "var(--tasty-text-secondary)" }}>Connecting…</span>
            <span style={{ fontSize: 11, color: "var(--tasty-text-muted)", maxWidth: 300 }}>Establishing the SSH tunnel to <span style={{ fontFamily: "var(--tasty-font-mono)" }}>gb10</span> and listing workspaces. This can take a few seconds.</span>
          </>)}
          {state === "error" && center(<>
            <span style={{ display: "inline-flex", color: "var(--tasty-accent-danger)", transform: "scale(1.4)" }}>{ic.warn}</span>
            <span style={{ fontSize: 13, fontWeight: 600, color: "var(--tasty-text-primary)" }}>Can't connect</span>
            <span style={{ fontSize: 11, color: "var(--tasty-text-muted)", maxWidth: 300, lineHeight: 1.5 }}>SSH authentication failed — passkey “old-rsa” was rejected by legacy-box.</span>
            <span style={{ marginTop: 4 }}><Button variant="secondary" size="sm" leadingIcon={ic.refresh}>Retry</Button></span>
          </>)}
          {state === "loaded" && (<>
            <div style={{ ...caps, padding: "10px 12px 4px", display: "flex", alignItems: "center", gap: 8 }}>
              <span>Remote workspaces</span><span style={{ color: "var(--tasty-text-disabled)" }}>·</span>
              <span style={{ textTransform: "none", letterSpacing: 0, fontFamily: "var(--tasty-font-ui)", fontSize: 11 }}>prod-web</span>
            </div>
            <div style={{ flex: 1, minHeight: 0, overflow: "hidden" }}>
              <RaNewRow phase={newPhase} selected={newSelected || newPhase !== "rest"} />
              <RaWs name="agents-prod" panes={3} busy selected={!newSelected && newPhase === "rest"} />
              <RaWs name="api-gateway" panes={2} attached />
              <RaWs name="scratch" panes={1} />
            </div>
          </>)}
          {state === "empty" && emptyPlan === "B" && (<>
            <div style={{ ...caps, padding: "10px 12px 4px", display: "flex", alignItems: "center", gap: 8 }}>
              <span>Remote workspaces</span><span style={{ color: "var(--tasty-text-disabled)" }}>·</span>
              <span style={{ textTransform: "none", letterSpacing: 0, fontFamily: "var(--tasty-font-ui)", fontSize: 11 }}>media-nas</span>
            </div>
            <div style={{ flex: 1, minHeight: 0, overflow: "hidden" }}>
              <RaNewRow selected />
              <div style={{ display: "flex", gap: 8, padding: "var(--tasty-space-xs) var(--tasty-space-md)" }}>
                <span style={{ flex: "none", width: "var(--tasty-status-dot-size)" }} />
                <span style={{ fontSize: 11, color: "var(--tasty-text-muted)", lineHeight: 1.5 }}>
                  <span style={{ fontFamily: "var(--tasty-font-mono)" }}>media-nas</span> is reachable but has no workspaces yet.
                </span>
              </div>
            </div>
          </>)}
          {state === "empty" && emptyPlan === "A" && center(<>
            <span style={{ display: "inline-flex", color: "var(--tasty-text-placeholder)", transform: "scale(1.4)" }}><Icon name="paneEmpty" size={16} /></span>
            <span style={{ fontSize: 13, color: "var(--tasty-text-muted)" }}>No workspaces on this remote yet</span>
            <span style={{ fontSize: 11, color: "var(--tasty-text-muted)", maxWidth: 300, lineHeight: 1.5 }}><span style={{ fontFamily: "var(--tasty-font-mono)" }}>media-nas</span> is reachable. Create one there and mirror it here.</span>
            <span style={{ marginTop: 4 }}><Button variant="secondary" size="sm" leadingIcon={<Icon name="plus" size={14} />}>New workspace</Button></span>
          </>)}
        </div>
      </div>
      {/* footer */}
      <div style={{ display: "flex", alignItems: "center", justifyContent: "flex-end", gap: 8, flex: "none", padding: "10px 20px", borderTop: "1px solid var(--tasty-separator)" }}>
        <Button variant="ghost">Cancel</Button>
        <Button variant="primary" disabled={newPhase === "creating" || (state !== "loaded" && state !== "empty")}>
          {(newSelected || newPhase !== "rest" || state === "empty") ? "Create & connect" : "Connect"}
        </Button>
      </div>
    </div>
  );
}

// ── Native file picker — 640×480 modal "Open file" dialog ──────────────
// A select-and-confirm dialog (NOT the Explorer surface): browse a dir,
// pick a path, hand it back, close. One component, two modes — local vs.
// a remote attach host — differing only in the header host indicator and
// the breadcrumb root. Popup language: Scrim · content-drawn header ·
// --tasty-bg-panel frame, same as remote_tool / Add remote workspace.
function FpRow({ kind, name, size, mod, selected, focus, multi, checked, dim }) {
  const isFolder = kind === "folder";
  return (
    <div style={{ display: "flex", alignItems: "center", gap: 8, padding: "6px var(--tasty-space-md)", position: "relative", cursor: "default",
      background: selected ? "var(--tasty-surface-active)" : "transparent",
      boxShadow: selected ? "inset 2px 0 0 var(--tasty-accent-primary)" : "none",
      outline: focus ? "1px solid var(--tasty-accent-primary)" : "none", outlineOffset: -1,
      opacity: dim ? 0.5 : 1 }}>
      {multi && <span style={{ display: "inline-flex", flex: "none" }}><Checkbox checked={!!checked} /></span>}
      <span style={{ flex: "none", display: "inline-flex", color: isFolder ? "var(--tasty-accent-primary)" : "var(--tasty-text-muted)" }}>
        <Icon name={isFolder ? "folder" : "file"} size={16} />
      </span>
      <span style={{ flex: 1, minWidth: 0, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap",
        fontSize: 13, fontWeight: isFolder ? 500 : 400, color: selected ? "var(--tasty-text-primary)" : "var(--tasty-text-secondary)" }}>{name}</span>
      <span style={{ flex: "none", width: 68, textAlign: "right", fontFamily: "var(--tasty-font-mono)", fontSize: 11, color: "var(--tasty-text-muted)" }}>{size}</span>
      <span style={{ flex: "none", width: 108, textAlign: "right", fontFamily: "var(--tasty-font-mono)", fontSize: 11, color: "var(--tasty-text-muted)" }}>{mod}</span>
    </div>
  );
}
function FpCrumbs({ items }) {
  return (
    <div style={{ display: "flex", alignItems: "center", gap: 2, minWidth: 0, flexWrap: "nowrap", overflow: "hidden" }}>
      {items.map((it, i) => (
        <React.Fragment key={i}>
          {i > 0 && <span style={{ flex: "none", display: "inline-flex", color: "var(--tasty-text-disabled)" }}><Icon name="chevronRight" size={13} /></span>}
          <span style={{ flex: "none", maxWidth: 180, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap",
            fontSize: 12, fontFamily: it.root ? "var(--tasty-font-mono)" : "var(--tasty-font-ui)",
            cursor: it.current ? "default" : "pointer",
            color: it.current ? "var(--tasty-text-primary)" : "var(--tasty-accent-primary)",
            fontWeight: it.current ? 600 : 400 }}>{it.label}</span>
        </React.Fragment>
      ))}
    </div>
  );
}
// host indicator chip shown in the header on remote mode (indicator="badge")
function FpHostBadge({ host }) {
  return (
    <span style={{ display: "inline-flex", alignItems: "center", gap: 5, height: 22, padding: "0 8px", borderRadius: "var(--tasty-radius)",
      background: "color-mix(in srgb, var(--tasty-accent-info) 14%, transparent)", border: "1px solid color-mix(in srgb, var(--tasty-accent-info) 45%, transparent)",
      color: "var(--tasty-accent-info)", fontFamily: "var(--tasty-font-mono)", fontSize: 11, whiteSpace: "nowrap" }}>
      <span style={{ display: "inline-flex" }}><Icon name="remote" size={13} /></span>{host}
    </span>
  );
}
function FilePickerFrame({ state = "loaded", remote = false, indicator = "badge", multi = false, filterOpen = false }) {
  const host = "deploy@10.0.4.12";
  const crumbs = remote
    ? [{ label: host, root: true }, { label: "home" }, { label: "deploy" }, { label: "agents-prod", current: true }]
    : [{ label: "/", root: true }, { label: "Users" }, { label: "maya" }, { label: "projects", current: true }];
  const borderMode = remote && indicator === "border";
  const files = [
    { kind: "folder", name: "configs", size: "—", mod: "Jul 12 09:14" },
    { kind: "folder", name: "logs", size: "—", mod: "Jul 14 22:03" },
    { kind: "folder", name: "node_modules", size: "—", mod: "Jul 02 11:40" },
    { kind: "file", name: "README.md", size: "4.2 KB", mod: "Jul 15 08:21", pick: true },
    { kind: "file", name: "package.json", size: "1.1 KB", mod: "Jul 15 08:21", pick: true },
    { kind: "file", name: "pipeline.yaml", size: "3.8 KB", mod: "Jul 14 17:55", pick: true },
    { kind: "file", name: "deploy.sh", size: "902 B", mod: "Jul 11 14:02" },
    { kind: "file", name: ".env", size: "218 B", mod: "Jul 09 10:30" },
  ];
  const fileName = multi ? "README.md, package.json, pipeline.yaml" : "README.md";
  const canOpen = (state === "loaded" || state === "empty" ? state === "loaded" : false);
  const center = (kids) => (
    <div style={{ flex: 1, minHeight: 0, display: "flex", flexDirection: "column", alignItems: "center", justifyContent: "center",
      textAlign: "center", gap: "var(--tasty-space-sm)", padding: "var(--tasty-space-xl) var(--tasty-space-lg)" }}>{kids}</div>
  );
  const errText = state === "error-conn"
    ? { title: "Remote connection lost", body: <>The SSH tunnel to <span style={{ fontFamily: "var(--tasty-font-mono)" }}>{host}</span> dropped. Reconnect to resume browsing from the last folder.</> }
    : { title: "Permission denied", body: <>You don't have permission to read <span style={{ fontFamily: "var(--tasty-font-mono)" }}>/home/deploy/agents-prod</span>. Try a different folder or check access.</> };
  return (
    <div style={{ width: 640, height: 480, display: "flex", flexDirection: "column", background: "var(--tasty-bg-panel)",
      border: borderMode ? "1px solid var(--tasty-accent-info)" : "1px solid var(--tasty-border-strong)",
      borderRadius: "var(--tasty-radius)", overflow: "hidden", boxShadow: "var(--tasty-shadow-modal)" }}>
      {borderMode && <div style={{ height: 2, flex: "none", background: "var(--tasty-accent-info)" }} />}
      {/* header */}
      <div style={{ display: "flex", alignItems: "center", gap: 8, padding: "10px 10px 10px 14px", flex: "none", borderBottom: "1px solid var(--tasty-separator)" }}>
        <span style={{ display: "inline-flex", color: remote && indicator === "glyph" ? "var(--tasty-accent-info)" : "var(--tasty-text-muted)" }}>
          {remote && (indicator === "glyph" || indicator === "border") ? <Icon name="remote" /> : <Icon name="file" />}
        </span>
        <span style={{ fontSize: 14, fontWeight: 600, color: "var(--tasty-text-primary)" }}>Open file</span>
        {remote && indicator === "badge" && <FpHostBadge host={host} />}
        {remote && indicator === "glyph" && <span style={{ fontSize: 12, fontFamily: "var(--tasty-font-mono)", color: "var(--tasty-accent-info)" }}>{host}</span>}
        <div style={{ flex: 1 }} />
        <IconButton size="sm" aria-label="Close">{ic.x}</IconButton>
      </div>
      {/* path bar */}
      <div style={{ display: "flex", alignItems: "center", gap: 8, padding: "6px 8px 6px 14px", flex: "none", borderBottom: "1px solid var(--tasty-separator)", background: "var(--tasty-bg-sidebar)" }}>
        <FpCrumbs items={crumbs} />
        <div style={{ flex: 1 }} />
        <IconButton size="sm" aria-label="Refresh">{ic.refresh}</IconButton>
      </div>
      {/* list header */}
      {state === "loaded" && (
        <div style={{ display: "flex", alignItems: "center", gap: 8, padding: "5px var(--tasty-space-md)", flex: "none", borderBottom: "1px solid var(--tasty-separator)",
          fontFamily: "var(--tasty-font-mono)", fontSize: 10, textTransform: "uppercase", letterSpacing: ".06em", color: "var(--tasty-text-muted)" }}>
          {multi && <span style={{ width: 16, flex: "none" }} />}
          <span style={{ width: 16, flex: "none" }} /><span style={{ flex: 1 }}>Name</span>
          <span style={{ width: 68, textAlign: "right", flex: "none" }}>Size</span>
          <span style={{ width: 108, textAlign: "right", flex: "none" }}>Modified</span>
        </div>
      )}
      {/* body */}
      <div style={{ flex: 1, minHeight: 0, overflow: "hidden", display: "flex", flexDirection: "column" }}>
        {state === "loaded" && (
          <div style={{ flex: 1, minHeight: 0, overflow: "hidden" }}>
            {files.map((f, i) => (
              <FpRow key={f.name} {...f}
                multi={multi}
                checked={multi && f.pick && ["README.md", "package.json", "pipeline.yaml"].includes(f.name)}
                selected={!multi && f.name === "README.md"}
                focus={!multi && f.name === "pipeline.yaml"} />
            ))}
          </div>
        )}
        {state === "loading" && center(<>
          <Spinner size={22} />
          <span style={{ fontSize: 13, color: "var(--tasty-text-secondary)" }}>Loading folder…</span>
          <span style={{ fontSize: 11, color: "var(--tasty-text-muted)", maxWidth: 320 }}>{remote ? <>Reading <span style={{ fontFamily: "var(--tasty-font-mono)" }}>{host}</span> over SSH.</> : "Reading the directory contents."}</span>
        </>)}
        {(state === "error-perm" || state === "error-conn") && center(<>
          <span style={{ display: "inline-flex", color: "var(--tasty-accent-danger)", transform: "scale(1.4)" }}>{ic.warn}</span>
          <span style={{ fontSize: 13, fontWeight: 600, color: "var(--tasty-text-primary)" }}>{errText.title}</span>
          <span style={{ fontSize: 11, color: "var(--tasty-text-muted)", maxWidth: 340, lineHeight: 1.5 }}>{errText.body}</span>
          <span style={{ marginTop: 4 }}><Button variant="secondary" size="sm" leadingIcon={ic.refresh}>{state === "error-conn" ? "Reconnect" : "Retry"}</Button></span>
        </>)}
        {state === "empty" && center(<>
          <span style={{ display: "inline-flex", color: "var(--tasty-text-placeholder)", transform: "scale(1.4)" }}><Icon name="folderOpen" /></span>
          <span style={{ fontSize: 13, color: "var(--tasty-text-muted)" }}>This folder is empty</span>
        </>)}
      </div>
      {/* footer */}
      <div style={{ display: "flex", flexDirection: "column", gap: 8, flex: "none", padding: "10px 14px", borderTop: "1px solid var(--tasty-separator)" }}>
        <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
          <span style={{ fontSize: 12, color: "var(--tasty-text-muted)", flex: "none", width: 64 }}>File name</span>
          <span style={{ flex: 1, minWidth: 0, display: "flex" }}><Input block defaultValue={state === "loaded" ? fileName : ""} placeholder="No file selected" /></span>
          <span style={{ flex: "none", position: "relative" }}>
            <span style={{ display: "inline-flex", alignItems: "center", gap: 6, height: 30, padding: "0 8px", border: "1px solid var(--tasty-border-strong)", borderRadius: "var(--tasty-radius)",
              background: "var(--tasty-bg-panel)", fontSize: 12, color: "var(--tasty-text-secondary)", cursor: "pointer", whiteSpace: "nowrap" }}>
              All files <span style={{ display: "inline-flex", color: "var(--tasty-text-muted)" }}><Icon name="chevronDown" size={13} /></span>
            </span>
          </span>
        </div>
        <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
          {multi && <span style={{ fontSize: 11, color: "var(--tasty-text-muted)" }}>3 selected</span>}
          <div style={{ flex: 1 }} />
          <Button variant="ghost">Cancel</Button>
          <Button variant="primary" disabled={!canOpen}>Open</Button>
        </div>
      </div>
    </div>
  );
}

// ── shared across the 4 overlay sub-pages (dialogs / windows / popups / banners) ──
window.OverlaysShared = {
  ic, Backdrop,
  PaletteFrame, ApprovalFrame, RenameFrame, SettingsFrame, SettingsGeneralOverlayFrame, SettingsRemoteTransferFrame, TransferProgressFrame, TransferErrorFrame, ToastDragValue,
  ToolsMenuFrame, PortsFrame, PortsFavoritesG, PortStarG, RemoteFrame, SearchBarFrame,
  ConvertFrame, FileHandlerFrame, PresetFrame, MarkdownOpenFrame,
  NumCap, HeldLabel, TabStripMock, SidebarMock, RailMock, CatSwitchSidebarMock, CatSwitchRailMock,
  BannerShellG, BannerScope, MouseCaptureBannerG, MouseCaptureHitZone, BlacklistEditorG, TtlBannerG, StackDemoG,
  BannerMoreMenuG, BannerMoreDemoG, MoreLabel,
  GitViewerFrame, ClipboardFrame, RemoteFormFrame, ScriptManagerFrame,
  RemoteAttachFrame, RaNewRow, RaWsPeek,
  FilePickerFrame,
  ModifierHintPanelG,
  MH_CAT_SECTIONS,
  MH_MIXED_SECTIONS,
  MH_EMPTY_SECTIONS,
  catIc, CatMenu, CategorySidebarFrame, RailCategoryFrame, CategoryEditFrame, CategoryDeleteFrame,
};
