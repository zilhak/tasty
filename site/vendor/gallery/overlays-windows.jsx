// Tasty Gallery — Overlays · Windows (large modal surfaces)
// One of the four Overlays sub-pages. Frame components are shared from
// overlays-shared.jsx (window.OverlaysShared); this file holds only the
// page's specimens + nav. See the other overlays-*.jsx for the rest.
const { Section, Spec, Stage, Meta, Note, Do, Dont, GIcon } = window.Gallery;
const { Kbd, IconButton, Input, Button } = window.TastyDesignSystem_41fd3f;
const { Backdrop, PaletteFrame, PortsFrame, PortsFavoritesG, PortStarG, RemoteFrame, SettingsFrame, SettingsGeneralOverlayFrame, SettingsRemoteTransferFrame, ToastDragValue, GitViewerFrame, ClipboardFrame, RemoteFormFrame, RemoteAttachFrame, RaNewRow, RaWsPeek, FilePickerFrame, ScriptManagerFrame, ic } = window.OverlaysShared;

const NAV = [
  { id: "palette", label: "Command palette" },
  { id: "ports", label: "Listening ports" },
  { id: "remote", label: "Remote connections" },
  { id: "remoteattach", label: "Add remote workspace" },
  { id: "filepicker", label: "File picker" },
  { id: "preseteditor", label: "Preset editor" },
  { id: "settings", label: "Settings window" },
  { id: "scripts", label: "Misc · Scripts" },
  { id: "gitviewer", label: "Git viewer" },
  { id: "clipboard", label: "Clipboard viewer" },
  { id: "moveresize", label: "Move & resize" },
];

// ── Move & resize — interaction spec helpers ──────────────────
// A faux popup: mantle titlebar (drag handle) over a surface0 body.
function FauxPopup({ title = "Listening ports", w = 240, h = 150, titlebar = true, region = false, grab = true, children, style }) {
  return (
    <div style={{ width: w, background: "var(--tasty-surface-raised)", border: "1px solid var(--tasty-border-strong)",
      borderRadius: "var(--tasty-radius)", boxShadow: "var(--tasty-shadow-modal)", overflow: "hidden", ...style }}>
      {titlebar && (
        <div style={{ position: "relative", display: "flex", alignItems: "center", justifyContent: "center", height: "var(--tasty-titlebar-height)",
          background: "var(--tasty-bg-app)", borderBottom: "1px solid var(--tasty-separator)", cursor: grab ? "grab" : "default" }}>
          <span style={{ fontSize: 12, color: "var(--tasty-titlebar-fg)" }}>{title}</span>
          <span style={{ position: "absolute", right: 4, top: "50%", transform: "translateY(-50%)", cursor: "default" }}>
            <IconButton size="sm" aria-label="Close" className="faux-close">{ic.x}</IconButton>
          </span>
        </div>
      )}
      {region && (
        <div style={{ display: "flex", alignItems: "center", gap: 6, height: "var(--tasty-titlebar-height)", padding: "0 6px",
          background: "var(--tasty-bg-app)", borderBottom: "1px solid var(--tasty-separator)" }}>
          <span style={{ display: "inline-flex", alignItems: "center", gap: 6, height: "100%", paddingRight: 8, cursor: "grab", color: "var(--tasty-text-secondary)" }}>
            <span style={{ display: "inline-flex", color: "var(--tasty-text-muted)" }}>{ic.port}</span><span style={{ fontSize: 12 }}>Ports</span>
          </span>
          <div style={{ flex: 1, cursor: "text" }}><Input block icon={ic.search} placeholder="filter…" /></div>
          <IconButton size="sm" aria-label="Close">{ic.x}</IconButton>
        </div>
      )}
      <div style={{ height: h, padding: 10, fontFamily: "var(--tasty-font-mono)", fontSize: 11, color: "var(--tasty-text-muted)", lineHeight: 1.7 }}>{children}</div>
    </div>
  );
}

// the 8-direction resize band overlaid on a popup. Real CSS cursors, no visual grip.
function ResizeMap() {
  const band = 12, corner = 16;
  const zone = (style, cursor, label) => (
    <div className="rz-zone" style={{ position: "absolute", cursor, ...style }} title={label}>
      {label && <span style={{ position: "absolute", inset: 0, display: "flex", alignItems: "center", justifyContent: "center",
        fontSize: 9, fontFamily: "var(--tasty-font-mono)", color: "var(--tasty-accent-info)" }}>{label}</span>}
    </div>
  );
  return (
    <div style={{ position: "relative", width: 280, height: 180 }}>
      <FauxPopup w={280} h={104} title="remote_tool — resizable" style={{ position: "absolute", inset: 0 }}>
        <div>min 420×320 · drag any edge or corner</div>
        <div style={{ color: "var(--tasty-text-disabled)" }}>no visual grip — cursor only</div>
      </FauxPopup>
      {/* edges */}
      {zone({ top: 0, left: corner, right: corner, height: band }, "ns-resize", "N")}
      {zone({ bottom: 0, left: corner, right: corner, height: band }, "ns-resize", "S")}
      {zone({ left: 0, top: corner, bottom: corner, width: band }, "ew-resize", "W")}
      {zone({ right: 0, top: corner, bottom: corner, width: band }, "ew-resize", "E")}
      {/* corners (take priority over edges) */}
      {zone({ top: 0, left: 0, width: corner, height: corner }, "nwse-resize", "NW")}
      {zone({ top: 0, right: 0, width: corner, height: corner }, "nesw-resize", "NE")}
      {zone({ bottom: 0, left: 0, width: corner, height: corner }, "nesw-resize", "SW")}
      {zone({ bottom: 0, right: 0, width: corner, height: corner }, "nwse-resize", "SE")}
    </div>
  );
}

function CursorRow({ area, cursor, glyph }) {
  return (
    <div style={{ display: "flex", alignItems: "center", gap: 10, padding: "5px 8px", borderRadius: "var(--tasty-radius-sm)",
      fontSize: 12, color: "var(--tasty-text-secondary)", cursor }}>
      <span style={{ width: 26, textAlign: "center", fontSize: 13 }}>{glyph}</span>
      <span style={{ flex: 1 }}>{area}</span>
      <span style={{ fontFamily: "var(--tasty-font-mono)", fontSize: 10, color: "var(--tasty-text-muted)" }}>{cursor}</span>
    </div>
  );
}

function CursorMatrix() {
  return (
    <div style={{ width: 320, display: "flex", flexDirection: "column", gap: 1, padding: 8,
      background: "var(--tasty-bg-panel)", border: "1px solid var(--tasty-border-default)", borderRadius: "var(--tasty-radius)" }}>
      <CursorRow area="Content (inside, not a handle)" cursor="default" glyph="↖" />
      <CursorRow area="Drag handle — hover" cursor="grab" glyph="✋" />
      <CursorRow area="Dragging" cursor="grabbing" glyph="✊" />
      <CursorRow area="Corner NW / SE" cursor="nwse-resize" glyph="⤡" />
      <CursorRow area="Corner NE / SW" cursor="nesw-resize" glyph="⤢" />
      <CursorRow area="Edge E / W" cursor="ew-resize" glyph="↔" />
      <CursorRow area="Edge N / S" cursor="ns-resize" glyph="↕" />
      <CursorRow area="Close button — hover" cursor="pointer" glyph="✕" />
    </div>
  );
}

// scope clamp — popup pinned to a scope rect, dragged into the corner & stopped
function ClampDemo() {
  return (
    <div style={{ position: "relative", width: 300, height: 200, background: "var(--tasty-bg-app)",
      border: "1px dashed var(--tasty-border-strong)", borderRadius: "var(--tasty-radius)", overflow: "hidden" }}>
      <span style={{ position: "absolute", top: 4, left: 6, fontFamily: "var(--tasty-font-mono)", fontSize: 9, color: "var(--tasty-text-disabled)" }}>scope rect</span>
      <div style={{ position: "absolute", right: 0, bottom: 0 }}>
        <FauxPopup w={188} h={70} title="popup" grab>
          <div>clamped — no part leaves the scope</div>
        </FauxPopup>
      </div>
    </div>
  );
}

// z-order — two overlapping popups; only the top one's edges respond
function ZOrderDemo() {
  return (
    <div style={{ position: "relative", width: 320, height: 200 }}>
      <div style={{ position: "absolute", left: 0, top: 0, opacity: 0.55 }}>
        <FauxPopup w={190} h={66} title="behind — inert" grab={false}>
          <div style={{ color: "var(--tasty-text-disabled)" }}>handles don't respond</div>
        </FauxPopup>
      </div>
      <div style={{ position: "absolute", right: 0, bottom: 0 }}>
        <FauxPopup w={200} h={70} title="front — active" grab>
          <div>top popup owns the cursor; click brings to front</div>
        </FauxPopup>
      </div>
    </div>
  );
}

function Page() {
  return (
    <>
<Section id="palette" title="Command palette">
        <Spec title="Command palette — ⌘K"
          when={<>Top-anchored launcher. A search Input header, a scrollable MenuItem list with the first row pre-highlighted, and a monospace hint footer. The fastest path to any command — every action should be reachable here.</>}>
          <Stage variant="solo center"><Backdrop height={400}><PaletteFrame /></Backdrop></Stage>
          <Meta
            specs={[["width", "480–540px"], ["anchor", "top, ~40–90px"], ["header", <>Input + <span className="tok">--tasty-separator</span></>], ["rows", <>28px MenuItem · first active</>], ["footer", "mono hints ↑↓ ↵ esc"]]}
            tokens={[{ tok: "--tasty-surface-raised", use: "frame", color: "var(--tasty-surface-raised)" }, { tok: "--tasty-surface-active", use: "highlight", color: "var(--tasty-surface-active)" }, { tok: "--tasty-font-mono", use: "hints" }]} />
        </Spec>
      </Section>

<Section id="ports" title="Listening ports">
        <Spec title="Listening ports — the Table popup"
          when={<>Tools › Listening ports. A <b>660×520</b> modal built directly on the <b>Table</b> component — the canonical example of a data-table surface. Header carries a live filter + refresh; a <b>Show all</b> checkbox toggles between Tasty-owned and system-wide ports; the scan state shows a <b>Spinner</b>. Rows are sortable; the State column renders a StatusDot.</>}>
          <Stage variant="solo center" style={{ padding: 20, background: "var(--tasty-bg-app)" }}><PortsFrame /></Stage>
          <Meta
            specs={[["frame", "660 × 520 (canonical)"], ["built on", <span className="ic">Table</span>], ["header", "filter Input + refresh"], ["scan", <>Spinner \u2192 \u201cCollecting\u2026\u201d</>], ["footer", "count · Copy address · Close"]]}
            tokens={[{ tok: "--tasty-bg-panel", use: "frame", color: "var(--tasty-bg-panel)" }, { tok: "--tasty-surface-active", use: "selected row", color: "var(--tasty-surface-active)" }, { tok: "--tasty-accent-success", use: "LISTEN dot", color: "var(--tasty-accent-success)" }, { tok: "--tasty-font-mono", use: "port/addr/pid" }]} />
          <Note>The 660px width is <b>design-canonical</b> — the 7-column table (Port · Proto · Address · Process · Workspace · Tab · State) needs it, plus a <b>tight 28px leading star column</b>. Don't narrow the frame; let columns truncate instead.</Note>
        </Spec>
        <Spec title="Favorites — star toggle, pinned section, NONE badge"
          when={<>Port favorites are keyed by <b>(addr, port)</b> — never the PID, which changes on every restart. A <b>tight 28px leading column</b> in the table carries the star (outline = unregistered, <b>gold starFill</b> = registered, same pair the Explorer sidebar uses); clicking it registers or unregisters <b>immediately</b>, with no confirm step. This is the only writing control in an otherwise read-only popup. A <b>Favorites</b> section sits between the filter row and the table and lists every favorite <b>regardless of the search box, the scope checkbox and the sort</b> — and it always judges LISTEN/NONE against the <b>full system scan</b>, so a favorite that Tasty doesn't own still reads LISTEN. A favorite whose port is not in the scan gets the <b>NONE</b> badge: StatusDot <span className="ic">idle</span>, muted, no pulse — apart from LISTEN (green + pulse) and other states (yellow). Stars can be toggled from <b>either</b> region.</>}>
          <Stage variant="solo center" style={{ padding: 20, background: "var(--tasty-bg-app)", flexDirection: "column", gap: 18 }}>
            <div style={{ display: "flex", gap: 18, flexWrap: "wrap", justifyContent: "center", alignItems: "flex-start" }}>
              {[["mixed — LISTEN + NONE", "4 favorites, one stopped", "mixed"],
                ["empty — caption persists", "one muted how-to line", "empty"],
                ["7 favorites — section scrolls", "list capped at 112px (5 rows)", "scrolling"]].map(([label, sub, v]) => (
                <div key={v} style={{ display: "flex", flexDirection: "column", gap: 6, width: 300 }}>
                  <div style={{ fontSize: 11, color: "var(--tasty-text-muted)" }}>{label}</div>
                  <div style={{ border: "1px solid var(--tasty-border-default)", borderRadius: "var(--tasty-radius)", overflow: "hidden" }}>
                    <PortsFavoritesG favorites={v} />
                  </div>
                  <div style={{ fontSize: 11, color: "var(--tasty-text-placeholder)" }}>{sub}</div>
                </div>
              ))}
            </div>
            <div style={{ display: "flex", alignItems: "center", gap: 14 }}>
              <span style={{ fontSize: 11, color: "var(--tasty-text-muted)" }}>star states</span>
              <span style={{ display: "inline-flex", alignItems: "center", gap: 6 }}><PortStarG /><span style={{ fontSize: 12, color: "var(--tasty-text-muted)" }}>unregistered</span></span>
              <span style={{ display: "inline-flex", alignItems: "center", gap: 6 }}><PortStarG on /><span style={{ fontSize: 12, color: "var(--tasty-text-muted)" }}>registered</span></span>
            </div>
          </Stage>
          <Meta
            specs={[["identity", <span className="ic">(addr, port)</span>], ["star", "22×22 hit · tight 28px column"], ["section", "caption 22 + list ≤ 112 (5 rows)"], ["row height", "22 (tree density)"], ["scope", "always system-wide"], ["frame", "unchanged — 660 × 520"]]}
            tokens={[{ tok: "--tasty-port-star-on", use: "registered star", color: "var(--tasty-port-star-on)" }, { tok: "--tasty-port-star-off", use: "outline star", color: "var(--tasty-port-star-off)" }, { tok: "--tasty-port-favorites-bg", use: "section tone", color: "var(--tasty-port-favorites-bg)" }, { tok: "--tasty-port-state-none-dot", use: "NONE dot", color: "var(--tasty-port-state-none-dot)" }, { tok: "--tasty-port-favorites-max-height", use: "scroll cap" }]} />
          <Do><b>Do</b> keep the favorites rows as <b>summary</b> rows (addr:port · process · state), not the 7-column grid — a stopped port has no process/workspace/tab data to show, and the summary row never needs the table's horizontal scroll. The star column width is shared, so stars still line up across both regions.</Do>
          <Note>New strings: <span className="ic">ports.favorites</span> ("Favorites") · <span className="ic">ports.favorites_empty</span> ("No favorites yet") · <span className="ic">ports.favorites_empty_hint</span> · <span className="ic">ports.state_none</span> ("NONE") · <span className="ic">ports.favorites_scope</span> ("system-wide"). Leave 20–40% growth room for ko/ja/de — the caption row and the empty line are single-line by design.</Note>
        </Spec>
      </Section>

<Section id="remote" title="Remote connections">
        <Spec title="Remote connections — profiles + passkeys"
          when={<>Tools › Remote connections. A <b>520×460</b> modal with <b>three top tabs</b>: a type-agnostic <b>remote-profile</b> store (ssh / smb / http / anything), an <b>Attach</b> store (tasty-attach targets), and a separate <b>Passkey</b> credential store. Each tab routes list → form → confirm-delete off a shared header. SSH profiles get a dedicated form; everything else uses a generic key-value editor. The Profiles add-bar carries a <b>protocol filter</b> (right) — a dropdown of the protocols present, checkbox-toggled with Select all / Deselect all / Reset, committed on <b>Apply</b>. Secrets live only in passkeys, referenced by name.</>}>
          <Stage variant="solo center" style={{ padding: 20, background: "var(--tasty-bg-app)" }}><RemoteFrame /></Stage>
          <Meta
            specs={[["frame", "520 × 460"], ["tabs", "Remote profiles · Attach · Passkeys"], ["routes", "list → form → confirm"], ["filter", "protocol dropdown (Profiles only)"], ["dismiss", <>×/Close/<span className="ic">Esc</span> only</>]]}
            tokens={[{ tok: "--tasty-bg-panel", use: "frame", color: "var(--tasty-bg-panel)" }, { tok: "--tasty-bg-sidebar", use: "tab bar", color: "var(--tasty-bg-sidebar)" }, { tok: "--tasty-accent-primary", use: "active tab / filter on", color: "var(--tasty-accent-primary)" }, { tok: "--tasty-accent-warning", use: "unknown-type / dangling-ref", color: "var(--tasty-accent-warning)" }]} />
          <Do><b>Do</b> keep secrets in the Passkey store and reference them by name. A profile never holds a secret inline.</Do>
          <Note>The protocol filter is <b>Profiles-only</b> (Passkeys has no filter) and <b>session-only</b> — never persisted; Tasty restarts with every protocol selected. The button reads <span className="ic">Filter</span> when off and turns accent with a <span className="ic">selected/total</span> count when a filter is applied.</Note>
        </Spec>
        <Spec title="Attach tab — tasty-attach targets"
          when={<>The <b>middle tab</b>. An <b>Attach</b> holds everything needed to attach to a <b>remote tasty instance</b> — it either <b>references</b> an ssh profile (<span className="ic">→ prod-web</span>) or carries <b>inline</b> ssh info, plus a <b>remote tasty</b> executable path and a <b>port discovery</b> mode. Splitting this off the ssh profile keeps ssh profiles pure connection info. Rows show name + (label), the reference/host summary, remote-tasty + port-mode captions, and an <b>inactive</b> badge when the referenced profile or inline shell isn't reachable.</>}>
          <Stage variant="solo center" style={{ padding: 20, background: "var(--tasty-bg-app)" }}><RemoteFrame tab="attach" /></Stage>
          <Meta
            specs={[["rows", "name+label · target · tasty/port"], ["mode", "profile (→ ref) · inline"], ["target", <><span className="ic">→ profile</span> or <span className="ic">user@host</span></>], ["inactive", "peach badge (unreachable)"], ["add-bar", "Add attach (no protocol filter)"]]}
            tokens={[{ tok: "--tasty-accent-warning", use: "inactive / profile-missing", color: "var(--tasty-accent-warning)" }, { tok: "--tasty-text-disabled", use: "inactive name", color: "var(--tasty-text-disabled)" }, { tok: "--tasty-separator", use: "row dividers" }]} />
          <Do><b>Do</b> keep <span className="ic">remote_tasty</span> and port discovery on the Attach, not the ssh profile — an ssh profile is reusable connection info; how to find the remote tasty binary is attach-specific.</Do>
        </Spec>
        <Spec title="Attach form — reference vs. inline"
          when={<>The add/edit route on the Attach tab, on the shared [<span className="tok">--tasty-remote-label-col</span> · 1fr] grid. A <b>Connection</b> segmented toggle switches between <b>SSH profile</b> (an <span className="ic">ssh_ref</span> dropdown of existing ssh profiles) and <b>Direct (inline)</b> (the ssh profile fieldset — host / user / port / shell / passkey). Both share a <b>Remote tasty</b> group: <b>Executable</b> (default <span className="ic">tasty</span>), <b>Port mode</b> (<span className="ic">auto / subcommand / file-unix / file-windows</span>), and an optional <b>Port file</b> that overrides the mode.</>}>
          <Stage variant="solo center" style={{ padding: 20, background: "var(--tasty-bg-app)", gap: 16, flexWrap: "wrap", alignItems: "flex-start" }}>
            <div style={{ display: "flex", flexDirection: "column", gap: 6 }}><div style={{ fontSize: 11, color: "var(--tasty-text-muted)" }}>reference an ssh profile</div><RemoteFormFrame variant="attach-ref" /></div>
            <div style={{ display: "flex", flexDirection: "column", gap: 6 }}><div style={{ fontSize: 11, color: "var(--tasty-text-muted)" }}>inline ssh info</div><RemoteFormFrame variant="attach-inline" /></div>
          </Stage>
          <Meta
            specs={[["toggle", "SSH profile ↔ Direct (inline)"], ["ref", "ssh_ref dropdown of ssh profiles"], ["inline", "host · user · port · shell · passkey"], ["remote tasty", "Executable (def. tasty)"], ["port", "auto / subcommand / file-unix / file-windows"], ["port file", "optional — overrides port mode"]]}
            tokens={[{ tok: "--tasty-remote-label-col", use: "shared 112 label column" }, { tok: "--tasty-surface-active", use: "selected connection segment", color: "var(--tasty-surface-active)" }, { tok: "--tasty-accent-primary", use: "active tab / segment", color: "var(--tasty-accent-primary)" }, { tok: "--tasty-text-muted", use: "labels / hints", color: "var(--tasty-text-muted)" }]} />
        </Spec>
        <Spec title="Profile form — SSH (the [112px · 1fr] row grid)"
          when={<>The add/edit route inside the popup. Every row is the same grid: a <b>fixed 112px right-aligned label</b> + a <b>1fr</b> control (columnGap 12, rowGap 8) so labels never collapse/truncate and all controls share one left edge. SSH gets the dedicated fields (Type · Name · Host · User · Port · Label · Shell + hint · Passkey) — <b>remote_tasty moved to the Attach tab</b>. Body scrolls; the footer is pinned with a <b>full-width</b> separator and right-aligned <b>Cancel (ghost) / Save (primary)</b>. Right: the same form with a <b>validation error</b>.</>}>
          <Stage variant="solo center" style={{ padding: 20, background: "var(--tasty-bg-app)", gap: 16, flexWrap: "wrap", alignItems: "flex-start" }}>
            <div style={{ display: "flex", flexDirection: "column", gap: 6 }}><div style={{ fontSize: 11, color: "var(--tasty-text-muted)" }}>SSH · add mode</div><RemoteFormFrame variant="ssh" /></div>
            <div style={{ display: "flex", flexDirection: "column", gap: 6 }}><div style={{ fontSize: 11, color: "var(--tasty-text-muted)" }}>validation error</div><RemoteFormFrame variant="ssh" error="host" /></div>
          </Stage>
          <Meta
            specs={[["frame", "520 × 460 · resizable · headless"], ["row", <>[<span className="tok">--tasty-remote-label-col</span> 112 · 1fr] · gap 12/8</>], ["label", "right-aligned · text-muted · 13"], ["body/footer", "flex:1 scroll · pinned footer"], ["footer sep", "full-width 1px · buttons inset 16/12"], ["error", "accent-danger caption (11)"]]}
            tokens={[{ tok: "--tasty-remote-label-col", use: "112px label column" }, { tok: "--tasty-text-muted", use: "labels / hints", color: "var(--tasty-text-muted)" }, { tok: "--tasty-separator", use: "footer + tab divider" }, { tok: "--tasty-accent-primary", use: "active tab · Save", color: "var(--tasty-accent-primary)" }, { tok: "--tasty-accent-danger", use: "validation error", color: "var(--tasty-accent-danger)" }]} />
          <Do><b>Do</b> keep the label column fixed at 112px (not 1fr-negotiated) — egui's auto grid collapsed it and truncated labels, so the form uses a manual two-column row.</Do>
        </Spec>
        <Spec title="Generic & Passkey forms · badges"
          when={<>A non-ssh Type swaps the SSH grid for a <b>generic key-value editor</b> (Type · Name · FIELDS rows `[112 · 1fr · 28]` + Add field · Passkey), with a peach <b>Unknown type</b> badge when the type isn't registered (still saves). The <b>Passkey</b> tab's form has Name · Kind (<span className="ic">path</span> / <span className="ic">inline</span> segmented) · Value — singleline path vs. 3-row multiline secret — plus the local-only note.</>}>
          <Stage variant="solo center" style={{ padding: 20, background: "var(--tasty-bg-app)", gap: 16, flexWrap: "wrap", alignItems: "flex-start" }}>
            <div style={{ display: "flex", flexDirection: "column", gap: 6 }}><div style={{ fontSize: 11, color: "var(--tasty-text-muted)" }}>generic · unknown type</div><RemoteFormFrame variant="generic" /></div>
            <div style={{ display: "flex", flexDirection: "column", gap: 6 }}><div style={{ fontSize: 11, color: "var(--tasty-text-muted)" }}>passkey · kind = path</div><RemoteFormFrame variant="passkey-path" /></div>
            <div style={{ display: "flex", flexDirection: "column", gap: 6 }}><div style={{ fontSize: 11, color: "var(--tasty-text-muted)" }}>passkey · kind = inline</div><RemoteFormFrame variant="passkey-inline" /></div>
          </Stage>
          <Meta
            specs={[["generic", "Type · Name · FIELDS · Passkey"], ["field row", "[112 key · 1fr value · 28 ✕]"], ["unknown type", "peach badge (saves anyway)"], ["passkey kind", "path = singleline · inline = 3-row"], ["secret", "name-reference only · note always shown"], ["footer", "ghost Cancel / primary Save (both forms)"]]}
            tokens={[{ tok: "--tasty-accent-warning", use: "unknown-type / dangling badge", color: "var(--tasty-accent-warning)" }, { tok: "--tasty-surface-active", use: "selected kind segment", color: "var(--tasty-surface-active)" }, { tok: "--tasty-input-bg", use: "inline secret field", color: "var(--tasty-input-bg)" }, { tok: "--tasty-remote-label-col", use: "shared 112 column" }]} />
          <Note>Other states (brief §5/§ST): <b>dangling passkey</b> reuses the same peach badge ("passkey missing"); <b>detecting</b> shows "detecting…" / "detection failed (disabled)" + Re-detect after a shell=auto save. Secrets live only in passkeys — a profile holds a <b>name reference</b>, never an inline secret.</Note>
        </Spec>
      </Section>

      <Section id="remoteattach" title="Add remote workspace">
        <Spec title="Two-pane remote-workspace picker — 4 states"
          when={<>Sidebar workspaces-header / new-workspace(<span className="ic">+</span>) right click → <b>Add remote workspace</b>. A <b>680×460</b> modal in the <b>remote_tool</b> language (Scrim · content-drawn header · <span className="tok">--tasty-bg-panel</span> frame). It <b>consumes</b> the <b>tasty-attach</b> profiles registered on remote_tool's Attach tab — it never edits them. <b>Left (240px)</b>: the attach-profile list, single select, with an <b>inactive</b> badge on profiles whose last detection failed. <b>Right</b>: the selected profile's remote workspaces. <b>Footer</b>: <b>Connect</b> (primary, enabled <i>only</i> when a selectable remote workspace is chosen) · <b>Cancel</b> (ghost, always enabled — closes, aborting any in-flight connect).</>}>
          <Stage variant="solo center" style={{ padding: 20, background: "var(--tasty-bg-app)" }}><RemoteAttachFrame state="loaded" /></Stage>
          <Meta
            specs={[["frame", "680 × 460"], ["left", "240px attach-profile list (single select)"], ["right", "remote workspaces — 4 states"], ["ws row", "dot · name · panes · busy / in-use"], ["connect", "enabled only with a selectable ws"], ["cancel", "always enabled · aborts connect"], ["dismiss", <>×/Cancel/<span className="ic">Esc</span></>]]}
            tokens={[{ tok: "--tasty-bg-panel", use: "frame", color: "var(--tasty-bg-panel)" }, { tok: "--tasty-bg-sidebar", use: "left pane", color: "var(--tasty-bg-sidebar)" }, { tok: "--tasty-surface-active", use: "selected row", color: "var(--tasty-surface-active)" }, { tok: "--tasty-accent-primary", use: "selected bar · Connect", color: "var(--tasty-accent-primary)" }, { tok: "--tasty-accent-attached", use: "in-use (claimed elsewhere)", color: "var(--tasty-accent-attached)" }, { tok: "--tasty-accent-success", use: "busy dot", color: "var(--tasty-accent-success)" }]} />
          <Do><b>Do</b> block selection of a remote workspace already <b>attached on another client</b> (lavender <span className="ic">in use</span> badge, dot dimmed) — mirror it there, not twice. This is the same <span className="tok">--tasty-accent-attached</span> axis as the sidebar attached ring.</Do>
          <Note>The right pane runs four states off the left selection: <b>initial</b> (nothing picked — placeholder), <b>connecting</b> (Spinner — SSH tunnel + list can take seconds), <b>error</b> (danger glyph + reason + <b>Retry</b>), and <b>loaded</b> (the workspace list, or a muted “no workspaces” empty when the remote is reachable but idle).</Note>
        </Spec>
        <Spec title="Right-pane states — initial · connecting · error"
          when={<>The three non-list states, driven by the left selection. Each is a centered column (glyph → title → one muted line), so the pane never looks broken while a connect is pending or failed.</>}>
          <Stage variant="solo center" style={{ padding: 20, background: "var(--tasty-bg-app)", gap: 16, flexWrap: "wrap", alignItems: "flex-start" }}>
            <div style={{ display: "flex", flexDirection: "column", gap: 6 }}><div style={{ fontSize: 11, color: "var(--tasty-text-muted)" }}>initial — nothing picked</div><RemoteAttachFrame state="initial" /></div>
            <div style={{ display: "flex", flexDirection: "column", gap: 6 }}><div style={{ fontSize: 11, color: "var(--tasty-text-muted)" }}>connecting</div><RemoteAttachFrame state="loading" /></div>
            <div style={{ display: "flex", flexDirection: "column", gap: 6 }}><div style={{ fontSize: 11, color: "var(--tasty-text-muted)" }}>error — retry</div><RemoteAttachFrame state="error" /></div>
          </Stage>
          <Meta
            specs={[["initial", "remote glyph + prompt · Connect disabled"], ["connecting", "Spinner + “Connecting…”"], ["error", "danger glyph + reason + Retry"], ["empty", "muted “no workspaces” (reachable, idle)"]]}
            tokens={[{ tok: "--tasty-text-placeholder", use: "initial glyph", color: "var(--tasty-text-placeholder)" }, { tok: "--tasty-accent-danger", use: "error glyph", color: "var(--tasty-accent-danger)" }, { tok: "--tasty-spinner-track", use: "connecting spinner" }]} />
        </Spec>
        <Spec title="“+ New workspace” row — the escape from a dead-end remote"
          when={<>The <b>first row</b> of the loaded list opens the other path: instead of mirroring a workspace the remote already has, <b>create one there</b> and mirror that. It is deliberately a <b>row in the list</b>, not a button or a second tab — so it inherits the list's <b>select-then-confirm</b> contract: clicking selects, the footer confirms (§6-5). No name or cwd is ever asked for; the remote's defaults are used, which the row's tooltip states outright.</>}>
          <Stage variant="solo center" style={{ padding: 20, background: "var(--tasty-bg-app)" }}><RemoteAttachFrame state="loaded" newSelected /></Stage>
          <Meta
            specs={[["position", "first row of the scroll area, under the caps header"], ["height", "34 — identical to a ws row (8/12 padding, 13px/500)"], ["glyph", <><span className="ic">plus</span> 14px inside the <b>status-dot slot</b> → same name-column left edge</>], ["right slot", "muted “on remote” instead of panes/busy"], ["separator", <>1px <span className="tok">--tasty-separator</span>, 4 above / 4 below</>], ["sticky", "no — scrolls with the list"], ["footer", "label becomes Create & connect while selected"]]}
            tokens={[{ tok: "--tasty-accent-primary", use: "glyph + label + select bar", color: "var(--tasty-accent-primary)" }, { tok: "--tasty-overlay-hover", use: "hover", color: "var(--tasty-overlay-hover)" }, { tok: "--tasty-surface-active", use: "selected", color: "var(--tasty-surface-active)" }, { tok: "--tasty-separator", use: "group divider", color: "var(--tasty-separator)" }, { tok: "--tasty-accent-danger", use: "create failed", color: "var(--tasty-accent-danger)" }]} />
          <Do><b>Do</b> separate it from the real workspaces on <b>three channels at once</b> — glyph, accent label, and the divider. Colour alone would fail both colour-blind users and the 4.5:1 rule; the <span className="ic">plus</span> glyph and the divider carry the meaning without it. When the row is <b>selected</b> the label drops the accent for <span className="tok">--tasty-text-primary</span> (accent over <span className="tok">--tasty-surface-active</span> measures 3.17:1) — the glyph and the 2px bar keep the accent, so nothing is lost.</Do>
          <Dont><b>Don't</b> give it a status dot, pane count, or a busy/in-use badge — none of them exist for a workspace that hasn't been created. The dot slot is <b>reused</b> (not padded out) so the name column still lines up.</Dont>
        </Spec>

        <Spec title="New-workspace row — rest · hover · selected · creating · failed"
          when={<>All five row states at pane width (440px), each with the first real ws row beneath so the shared alignment line is visible. <b>Creating</b> swaps the glyph for a 14px <b>Spinner</b> and the label for “Creating workspace…” (§6-3: inline, not a full-pane takeover — the user keeps the list they were reading, and the roundtrip is seconds, not minutes). <b>Failed</b> keeps the row and hangs the remote's own message under it with a <b>Try again</b> button (§6-4), clamped to 3 lines with the full text in <span className="tok">title</span>.</>}>
          <Stage variant="solo center" style={{ padding: 20, background: "var(--tasty-bg-app)", gap: 16, flexWrap: "wrap", alignItems: "flex-start" }}>
            {[["rest", {}], ["hover", { hover: true }], ["selected", { selected: true }], ["creating (list dims, Connect disabled)", { selected: true, phase: "creating" }], ["create failed — inline, list kept", { selected: true, phase: "failed" }]].map(([label, p]) => (
              <div key={label} style={{ display: "flex", flexDirection: "column", gap: 6, width: 440 }}>
                <div style={{ fontSize: 11, color: "var(--tasty-text-muted)" }}>{label}</div>
                <div style={{ background: "var(--tasty-bg-panel)", border: "1px solid var(--tasty-border-strong)", borderRadius: "var(--tasty-radius)", overflow: "hidden" }}>
                  <RaNewRow {...p} />
                  <div style={{ opacity: p.phase === "creating" ? 0.5 : 1 }}>
                    <RaWsPeek />
                  </div>
                </div>
              </div>
            ))}
          </Stage>
          <Meta
            specs={[["rest", "transparent · accent glyph + label"], ["hover", <span className="tok">--tasty-overlay-hover</span>], ["selected", <>+ 2px inset accent bar · label switches to <span className="tok">--tasty-text-primary</span> (accent on <span className="tok">--tasty-surface-active</span> is only 3.17:1) — identical to a ws row</>], ["creating", "Spinner 14 in the glyph slot · label muted · list dimmed, not replaced"], ["failed", "danger glyph · message clamp 3 lines · Try again (secondary sm)"], ["motion", "state swaps are instant (0ms) — the Spinner is the one allowed exception"]]}
            tokens={[{ tok: "--tasty-accent-primary", use: "rest / selected label", color: "var(--tasty-accent-primary)" }, { tok: "--tasty-text-muted", use: "creating label", color: "var(--tasty-text-muted)" }, { tok: "--tasty-accent-danger", use: "failed glyph + message", color: "var(--tasty-accent-danger)" }]} />
          <Note><b>i18n.</b> “New workspace” grows to “Neuer Arbeitsbereich” / “새 워킬스페이스”. The label is <span className="tok">flex: 0 1 auto; min-width: 0</span> with single-line ellipsis and the trailing “on remote” caption is the first thing to be pushed out — the row never wraps and never grows past 34px.</Note>
        </Spec>

        <Spec title="Empty remote — plan A (center-state + CTA) vs plan B (list path)"
          when={<>§6-1, the decision that sets the implementation's render branch. Before this change a reachable-but-empty remote ended at a center-state with nothing but <b>Cancel</b>. <b>Plan A</b> keeps that center-state and adds a CTA button. <b>Plan B</b> takes the <b>list</b> path even when empty: caps header + the one “+ New workspace” row, with a muted line explaining why nothing follows it.</>}>
          <Stage variant="solo center" style={{ padding: 20, background: "var(--tasty-bg-app)", gap: 16, flexWrap: "wrap", alignItems: "flex-start" }}>
            <div style={{ display: "flex", flexDirection: "column", gap: 6 }}><div style={{ fontSize: 11, color: "var(--tasty-text-muted)" }}>plan A — center-state + CTA</div><RemoteAttachFrame state="empty" emptyPlan="A" /></div>
            <div style={{ display: "flex", flexDirection: "column", gap: 6 }}><div style={{ fontSize: 11, color: "var(--tasty-accent-primary)" }}>plan B — list path (recommended)</div><RemoteAttachFrame state="empty" emptyPlan="B" /></div>
          </Stage>
          <Meta
            specs={[["recommendation", "plan B"], ["why", "one render path, one affordance, one confirm route"], ["plan B copy", "“‹host› is reachable but has no workspaces yet.”"], ["footer", "Create & connect, enabled (the row is pre-selected)"], ["plan A cost", "a second way to trigger the same action, confirmed differently"]]}
            tokens={[{ tok: "--tasty-text-muted", use: "explanatory line", color: "var(--tasty-text-muted)" }, { tok: "--tasty-accent-primary", use: "row label", color: "var(--tasty-accent-primary)" }]} />
          <Note><b>Recommended: plan B.</b> Plan A's CTA is a <i>button</i>, so it fires immediately — the same action would then be confirmed two different ways depending on whether the remote happened to be empty, and the implementation would carry two render branches plus two handlers. Plan B has one list, one row, one confirm route, and the empty case degrades to “the list has exactly one row.” In plan B the row is <b>pre-selected</b> on an empty remote, so <b>Create &amp; connect</b> is live the moment the pane loads — the dead end is gone without adding a control.</Note>
        </Spec>

        <Spec title="Decisions — the eight open questions, resolved"
          when={<>What was chosen and why. This is the implementation spec.</>}>
          <Note>
            <b>1 · empty state</b> — <b>plan B</b> (list path, one pre-selected row). One render branch, one confirm route.<br />
            <b>2 · distinction</b> — <span className="ic">plus</span> glyph in the dot slot <b>+</b> accent label <b>+</b> 1px separator below (4/4 margins). Weight stays 500, size stays 13 — the row must read as a peer of the rows below it, not as a header.<br />
            <b>3 · creating</b> — <b>inline in the row</b> (glyph → Spinner, label → “Creating workspace…”), list below dimmed to 50% and inert. A full-pane <i>connecting</i> takeover would throw away the list for a 1–3s roundtrip.<br />
            <b>4 · failure</b> — <b>inline under the row</b>, message clamped to 3 lines (full string in <span className="tok">title</span>) + <b>Try again</b>. The connect-error center-state is right for “we never got a list”; here we <i>have</i> the list and the user's next move is usually to pick an existing workspace instead — don't hide it.<br />
            <b>5 · confirm (core)</b> — <b>select, then footer</b>. The user chose a placement <i>inside a list</i>; a single row that fires on click while its neighbours only select is the inconsistency, and it also loses the reversible “I clicked it, now what?” moment before a remote-mutating action. Cost accepted: the row carries a selected state.<br />
            <b>6 · height &amp; sticky</b> — <b>34px, not sticky</b>. Same box as a ws row; sticky would stack a second frozen band under the caps header for a list that rarely exceeds ~8 rows.<br />
            <b>7 · footer label</b> — <b>“Create &amp; connect”</b> while the new row is selected, “Connect” otherwise. Because §6-5 chose footer confirmation, the button must say which of the two things it will do.<br />
            <b>8 · tooltip</b> — <b>yes</b>, on the row: “Creates a workspace on the remote with its default name and cwd — you won't be asked for a name — then mirrors it here.” Not asking for a name is the surprising part, so it gets said where the click happens.
          </Note>
          <Note><b>Interaction contract.</b> Arrow keys traverse the new row as row 1; <span className="ic">Enter</span> confirms the selection (same as the footer). Changing the left-hand profile resets the selection and the row's phase to rest. During creation the left profile list and the right list are inert; <b>Cancel</b> stays live — it closes the popup and abandons the in-flight request, and a workspace already created on the remote is <b>not</b> rolled back (a flash message on close says so; no extra confirm). On success the popup closes straight into attach — no interstitial “created” step. The caps header keeps its wording: <span className="ic">REMOTE WORKSPACES · ‹profile›</span> still describes the group, and the new row's own label says it is a creation.</Note>
          <Note><b>New icons: none.</b> <span className="ic">plus</span>, <span className="ic">alertTriangle</span> and <span className="ic">refresh</span> already exist in <span className="tok">icons/</span>. <b>New tokens: none</b> — the row is built from <span className="tok">--tasty-accent-primary</span>, <span className="tok">--tasty-overlay-hover</span>, <span className="tok">--tasty-surface-active</span>, <span className="tok">--tasty-separator</span>, <span className="tok">--tasty-accent-danger</span> and the existing spacing steps.</Note>
        </Spec>
      </Section>

      <Section id="filepicker" title="File picker">
        <Spec title="Native file picker — local & remote, one component"
          when={<>Tasty's own <b>Open file</b> dialog, replacing the OS-native picker so it can browse a <b>remote attach host</b> over the <b>same SSH mechanism</b> as attach — the OS picker only ever sees the local disk. It's a <b>select-and-confirm</b> dialog (pick a path, hand it back, close), <b>not</b> the Explorer surface (that's a free-roam tab). A <b>640×480</b> modal in the <b>remote_tool</b> popup language: Scrim · content-drawn header · <span className="tok">--tasty-bg-panel</span> frame. <b>Local and remote are the same component</b> — they differ only in the header host indicator and the breadcrumb root. Rows reuse the <span className="ic">folder</span>/<span className="ic">file</span> list language; folder <b>double-click</b> descends, file <b>click</b> fills the name field, file <b>double-click</b> = Open.</>}>
          <Stage variant="solo center" style={{ padding: 20, background: "var(--tasty-bg-app)", gap: 16, flexWrap: "wrap", alignItems: "flex-start" }}>
            <div style={{ display: "flex", flexDirection: "column", gap: 6 }}><div style={{ fontSize: 11, color: "var(--tasty-text-muted)" }}>local</div><FilePickerFrame /></div>
            <div style={{ display: "flex", flexDirection: "column", gap: 6 }}><div style={{ fontSize: 11, color: "var(--tasty-text-muted)" }}>remote (host badge — recommended)</div><FilePickerFrame remote indicator="badge" /></div>
          </Stage>
          <Meta
            specs={[["frame", "640 × 480 · PopupDef"], ["header", "glyph · title · host indicator · ✕"], ["path bar", "breadcrumb + refresh · bg-sidebar"], ["row", "checkbox? · icon · name · size · modified"], ["footer", "name field · type filter · Cancel / Open"], ["open", "enabled only with a selection"], ["dismiss", <>×/Cancel/<span className="ic">Esc</span> · Open = <span className="ic">Enter</span></>]]}
            tokens={[{ tok: "--tasty-bg-panel", use: "frame", color: "var(--tasty-bg-panel)" }, { tok: "--tasty-bg-sidebar", use: "path / list-header bar", color: "var(--tasty-bg-sidebar)" }, { tok: "--tasty-surface-active", use: "selected row", color: "var(--tasty-surface-active)" }, { tok: "--tasty-accent-primary", use: "selected bar · folder glyph · Open · crumb link", color: "var(--tasty-accent-primary)" }, { tok: "--tasty-accent-info", use: "remote host indicator", color: "var(--tasty-accent-info)" }, { tok: "--tasty-accent-danger", use: "error glyph", color: "var(--tasty-accent-danger)" }]} />
          <Do><b>Do</b> keep local and remote a single component with a single set of states — the target only changes the <b>host indicator</b> and the <b>breadcrumb root</b> (<span className="tok">--tasty-font-mono</span> <span className="ic">user@host</span> vs. <span className="ic">/</span>), never the layout.</Do>
          <Note>Out of scope this pass (deferred, not designed): a favorites/recent-locations rail and per-host history. Multi-select is spec'd below but ships behind single-select by default.</Note>
        </Spec>

        <Spec title="Remote indicator — three candidates"
          when={<>§6.1 open decision: how to signal "this dialog is looking at a remote host, not your local disk," clearly but not loudly. Three candidates on the same <span className="tok">--tasty-accent-info</span> axis. <b>Badge</b> (recommended): a mono <span className="ic">user@host</span> chip beside the title — explicit about <i>which</i> host, reuses the attach/remote visual language. <b>Glyph + host</b>: swap the header icon to <span className="ic">remote</span> and print the host inline — lighter, no chip. <b>Border</b>: tint the whole frame edge + a 2px top strip — unmissable but the heaviest; use only if remote/local confusion is a real hazard.</>}>
          <Stage variant="solo center" style={{ padding: 20, background: "var(--tasty-bg-app)", gap: 16, flexWrap: "wrap", alignItems: "flex-start" }}>
            <div style={{ display: "flex", flexDirection: "column", gap: 6 }}><div style={{ fontSize: 11, color: "var(--tasty-text-muted)" }}>A · host badge</div><FilePickerFrame remote indicator="badge" /></div>
            <div style={{ display: "flex", flexDirection: "column", gap: 6 }}><div style={{ fontSize: 11, color: "var(--tasty-text-muted)" }}>B · glyph + host</div><FilePickerFrame remote indicator="glyph" /></div>
            <div style={{ display: "flex", flexDirection: "column", gap: 6 }}><div style={{ fontSize: 11, color: "var(--tasty-text-muted)" }}>C · frame border</div><FilePickerFrame remote indicator="border" /></div>
          </Stage>
          <Meta
            specs={[["A badge", "mono host chip · info-tinted · explicit"], ["B glyph", "remote icon + inline host · lightest"], ["C border", "info edge + 2px top strip · loudest"], ["shared axis", "--tasty-accent-info (never danger)"], ["local", "no indicator · file glyph · / root"]]}
            tokens={[{ tok: "--tasty-accent-info", use: "all three indicators", color: "var(--tasty-accent-info)" }, { tok: "--tasty-font-mono", use: "host string", color: "var(--tasty-text-secondary)" }]} />
          <Dont><b>Don't</b> use a danger/warning tone for the remote indicator — remote is a normal, expected mode, not an error. Reserve peach/red for the connection-lost state below.</Dont>
        </Spec>

        <Spec title="States — loading · empty · permission · connection lost · multi-select"
          when={<>The body region swaps between list and status states without changing the frame. <b>Loading</b>: Spinner while the directory reads (remote adds "over SSH"). <b>Empty</b>: muted <span className="ic">folderOpen</span> + "This folder is empty." <b>Permission denied</b> (local) and <b>Remote connection lost</b> share the layout — danger glyph · title · one reason line · action — but differ in copy and action (<b>Retry</b> vs. <b>Reconnect</b>, which resumes from the last folder). <b>Multi-select</b> (§6.2): a checkbox column, a comma-joined name field, and an <span className="ic">N selected</span> count — spec'd for the future, single-select is the default.</>}>
          <Stage variant="solo center" style={{ padding: 20, background: "var(--tasty-bg-app)", gap: 16, flexWrap: "wrap", alignItems: "flex-start" }}>
            <div style={{ display: "flex", flexDirection: "column", gap: 6 }}><div style={{ fontSize: 11, color: "var(--tasty-text-muted)" }}>loading (remote)</div><FilePickerFrame remote state="loading" /></div>
            <div style={{ display: "flex", flexDirection: "column", gap: 6 }}><div style={{ fontSize: 11, color: "var(--tasty-text-muted)" }}>empty folder</div><FilePickerFrame state="empty" /></div>
            <div style={{ display: "flex", flexDirection: "column", gap: 6 }}><div style={{ fontSize: 11, color: "var(--tasty-text-muted)" }}>permission denied (local)</div><FilePickerFrame state="error-perm" /></div>
            <div style={{ display: "flex", flexDirection: "column", gap: 6 }}><div style={{ fontSize: 11, color: "var(--tasty-text-muted)" }}>connection lost (remote)</div><FilePickerFrame remote state="error-conn" /></div>
            <div style={{ display: "flex", flexDirection: "column", gap: 6 }}><div style={{ fontSize: 11, color: "var(--tasty-text-muted)" }}>multi-select (remote)</div><FilePickerFrame remote multi /></div>
          </Stage>
          <Meta
            specs={[["loading", "Spinner · reads dir (remote: over SSH)"], ["empty", "folderOpen glyph + muted line"], ["error", "danger glyph · title · reason · action"], ["perm vs. conn", "Retry vs. Reconnect (resumes)"], ["multi", "checkbox col · joined names · N selected"], ["Open", "disabled while loading / error / empty"]]}
            tokens={[{ tok: "--tasty-accent-danger", use: "error glyph", color: "var(--tasty-accent-danger)" }, { tok: "--tasty-text-placeholder", use: "empty glyph", color: "var(--tasty-text-placeholder)" }, { tok: "--tasty-spinner-track", use: "loading spinner" }]} />
          <Note>Row focus ring (keyboard nav) is shown on <span className="ic">pipeline.yaml</span> in the loaded frames — 1px <span className="tok">--tasty-accent-primary</span> outline, distinct from the filled selection background so focus and selection never merge visually.</Note>
        </Spec>
      </Section>

      {window.PresetEditor && <window.PresetEditor.Section />}

<Section id="settings" title="Settings window">
        <Spec title="Settings — two-tier navigation"
          when={<>The largest dialog. <b>L1</b> = a small, fixed set of top tabs (General / Appearance / Keybindings / Plugins). <b>L2</b> = a growable, filterable left sidebar of sections (plugins contribute here). This split keeps top-level navigation stable while the content tree scales. Reuse it for any settings-like surface.</>}>
          <Stage variant="solo center"><Backdrop height={420}><SettingsFrame /></Backdrop></Stage>
          <Meta
            specs={[["frame", "1100 × 700 (canonical)"], ["L1 tabs", "44px bar, accent underline"], ["L2 sidebar", "200px, filter + list"], ["footer", "Cancel / Save, right"]]}
            tokens={[{ tok: "--tasty-bg-sidebar", use: "L1 + L2", color: "var(--tasty-bg-sidebar)" }, { tok: "--tasty-bg-panel", use: "content", color: "var(--tasty-bg-panel)" }, { tok: "--tasty-accent-primary", use: "active tab", color: "var(--tasty-accent-primary)" }, { tok: "--tasty-surface-active", use: "active section", color: "var(--tasty-surface-active)" }]} />
          <Note>L1 stays small forever; growth happens only in L2. Plugins are their <b>own</b> L1 tab — never crammed into another group's sidebar.</Note>
        </Spec>

        <Spec title="General › Overlay — toast duration"
          when={<>The <b>General</b> L1 tab gains a fourth L2 section, <b>Overlay</b> (after General / Notifications / Accessibility) — the umbrella term for Toast / Banner / Modifier-hint / Marker overlays. It ships with <b>one row</b>: <b>Toast duration</b>, a DragValue that controls how long a toast stays before auto-dismissing (today hardcoded at 2000ms). Exposed in <b>seconds</b> (matches the user's mental model), stored as ms. Same Grid (label + control) and hint-text pattern as the other General sections — no new interaction invented.</>}>
          <Stage variant="solo center" style={{ gap: 24, flexWrap: "wrap" }}>
            <Backdrop height={420}><SettingsGeneralOverlayFrame /></Backdrop>
            <div style={{ display: "flex", flexDirection: "column", gap: 10, alignSelf: "center" }}>
              {[["rest", "rest"], ["hover", "hover — ew-resize cursor"], ["editing", "editing — click to type"]].map(([s, l]) => (
                <div key={s} style={{ display: "flex", alignItems: "center", gap: 10 }}>
                  <ToastDragValue state={s} />
                  <span style={{ fontSize: 11, color: "var(--tasty-text-muted)" }}>{l}</span>
                </div>
              ))}
            </div>
          </Stage>
          <Meta
            specs={[["L2 position", "4th — after Accessibility"], ["label", "Toast duration"], ["unit", "seconds — mono “2.0 s”"], ["range / step", "1.0–10.0 s, step 0.5"], ["default", "2.0 s (= DEFAULT_LIFETIME 2000ms)"], ["control", "DragValue — drag or click-to-type"], ["hint", "12px muted line below the grid"]]}
            tokens={[{ tok: "--tasty-surface-active", use: "active L2 row / edit selection", color: "var(--tasty-surface-active)" }, { tok: "--tasty-border-default", use: "DragValue border", color: "var(--tasty-border-default)" }, { tok: "--tasty-accent-primary", use: "editing border", color: "var(--tasty-accent-primary)" }, { tok: "--tasty-font-mono", use: "value text" }]} />
          <Note>Scope is <b>this one row only</b> — the tab name is the umbrella so future overlay settings (banner, marker…) can land here without inventing a new section. Existing General / Notifications / Accessibility content is untouched. Store the value in ms internally; only the display is in seconds.</Note>
        </Spec>
        <Spec title="Settings · General › Remote transfer — 5th L2 subtab"
          when={<>The <b>General</b> L1 tab gains a fifth L2 section, <b>Remote transfer</b> (after Overlay) — the umbrella for the remote (mirror) file-transfer channel, following the Overlay-subtab precedent: a topical L2 with room to grow rather than rows crammed into General. Two rows edit <code>RemoteTransferSettings</code>: <b>Save folder</b> — a mono path Input + a <b>Browse…</b> secondary button (opens the native folder picker, same pairing as the Scripts file row) — and <b>Maximum size</b> — a numeric Input with a mono <b>MiB</b> unit suffix. Each row keeps the settings-row grid (150px label · control), a muted description line beneath, and a separator between rows.</>}>
          <Stage variant="solo center"><Backdrop height={420}><SettingsRemoteTransferFrame /></Backdrop></Stage>
          <Meta
            specs={[["L2 position", "5th — after Overlay"], ["rows", "Save folder · Maximum size"], ["folder row", "mono path Input + Browse… (secondary, folder icon)"], ["size row", "numeric Input · 88px + mono “MiB” suffix"], ["defaults", "~/.tasty/transfers/ · 500 MiB"], ["row grid", "150px label · control · desc below"]]}
            tokens={[{ tok: "--tasty-settings-row-min-height", use: "row height" }, { tok: "--tasty-surface-active", use: "active L2 row", color: "var(--tasty-surface-active)" }, { tok: "--tasty-separator", use: "row separator", color: "var(--tasty-separator)" }, { tok: "--tasty-text-muted", use: "descriptions + unit", color: "var(--tasty-text-muted)" }]} />
          <Note>The unit is a static mono <b>MiB</b> suffix outside the field — not typed, not a Tag — mirroring how the Toast DragValue carries its “s” unit. Exceeding <b>Maximum size</b> rejects new transfers before they start; the rejection surfaces as the <b>Transfer failed</b> popup (Overlays › Dialogs › Remote transfer).</Note>
        </Spec>
      </Section>

      <Section id="scripts" title="Misc · Scripts (Lua script manager)">
        <Spec title="Scripts subsection — list, states & empty"
          when={<>Settings › <b>Misc</b> › <b>Scripts</b>. A subsection (not a separate popup) that manages user <b>Lua scripts</b> run by a shortcut (ADR-0031). Each <b>ScriptRow</b>: the display name, the absolute path (<b>middle-elided</b> — dir tail truncates, filename always shown), a bound-shortcut <span className="ic">Kbd</span> badge (or italic <b>Unbound</b>), a peach <b>changed</b> badge + help line when the file's SHA no longer matches the one recorded at registration (TOFU re-confirm on next run), and an <b>Auto-run</b> row: mono <b>trigger chips</b> for the host-lifecycle events the script fires on (click a chip to remove it) plus a dashed <b>Add trigger…</b> control offering the remaining events. Row actions: bind shortcut (→ Keybindings), rename (inline), remove (inline confirm).</>}>
          <Stage variant="solo center" style={{ padding: 20, background: "var(--tasty-bg-app)", gap: 16, flexWrap: "wrap", alignItems: "flex-start" }}>
            <div style={{ display: "flex", flexDirection: "column", gap: 6 }}><div style={{ fontSize: 11, color: "var(--tasty-text-muted)" }}>registered scripts</div><ScriptManagerFrame /></div>
            <div style={{ display: "flex", flexDirection: "column", gap: 6 }}><div style={{ fontSize: 11, color: "var(--tasty-text-muted)" }}>empty state</div><ScriptManagerFrame empty /></div>
          </Stage>
          <Meta
            specs={[["home", "Settings › Misc › Scripts (subsection)"], ["row", "name · path · shortcut · actions · auto-run"], ["shortcut", <><span className="ic">Kbd</span> badge or italic Unbound</>], ["changed", "peach badge + help line (SHA mismatch)"], ["auto-run", "trigger chips + Add trigger… (13 lifecycle events)"], ["chip", "mono event · click removes · hover 12% overlay"], ["actions", "bind · rename · remove"], ["empty", "glyph + Add script prompt"]]}
            tokens={[{ tok: "--tasty-accent-warning", use: "changed badge + help", color: "var(--tasty-accent-warning)" }, { tok: "--tasty-border-default", use: "chip + add-control border", color: "var(--tasty-border-default)" }, { tok: "--tasty-overlay-active", use: "chip hover / menu open", color: "var(--tasty-overlay-active)" }, { tok: "--tasty-text-disabled", use: "Unbound", color: "var(--tasty-text-disabled)" }, { tok: "--tasty-font-mono", use: "path · trigger chips" }]} />
          <Note>Two independent run paths: a <b>manual shortcut</b> (bound in the Keybindings tab, shown as the <span className="ic">Kbd</span> badge) and <b>auto-run triggers</b> (edited inline here — the script fires when the host emits a bound lifecycle event). A script can have either, both, or neither. The <span className="ic">changed</span> state is informational — the script still runs, but re-confirms once (TOFU) because the on-disk file drifted from the registered hash.</Note>
        </Spec>
      </Section>

      <Section id="gitviewer" title="Git viewer">
        <Spec title="Git worktree viewer — read-only popup"
          when={<>Tools → Git. A <b>960×640</b> read-only popup that surveys every worktree. A header (<span className="ic">Git</span> + Refresh) sits over a <b>context strip</b> (current worktree · branch · HEAD oid · repo path). A left <b>worktree rail</b> (fixed 232px) lists main + linked worktrees as <b>two-line rows</b> — name + type badge, then oid + state badge — so nothing overflows the rail; the right column splits 50/50 into <b>Changes</b> (status) over <b>Commits</b> (log). Every badge is a <b>token-tinted pill</b> (the Tag visual), not colored text. <b>No writes</b> — only Refresh, selecting a worktree (re-binds the panes), selecting a changed file, and the diff Back button.</>}>
          <Stage variant="solo center" style={{ padding: 20, background: "var(--tasty-bg-app)" }}><GitViewerFrame /></Stage>
          <Meta
            specs={[["frame", "960 × 640 · dismiss on outside click"], ["header", "Git + Refresh · context strip (wt · branch · oid · path)"], ["rail", "232px — 2-line rows: name+type / oid+state"], ["right", "50/50 split — Changes over Commits"], ["badges", "Tag-style pills: main · linked · current · locked · invalid"], ["status", "M·A·D·R·?·U fixed-width pill prefix"], ["read-only", "Refresh · select · Back only"]]}
            tokens={[{ tok: "--tasty-accent-info", use: "oid · main · refs · R", color: "var(--tasty-accent-info)" }, { tok: "--tasty-accent-success", use: "current · A", color: "var(--tasty-accent-success)" }, { tok: "--tasty-accent-warning", use: "locked · M", color: "var(--tasty-accent-warning)" }, { tok: "--tasty-accent-danger", use: "invalid · D · U", color: "var(--tasty-accent-danger)" }, { tok: "--tasty-surface-active", use: "selected row", color: "var(--tasty-surface-active)" }, { tok: "--tasty-bg-sidebar", use: "section / context strips", color: "var(--tasty-bg-sidebar)" }]} />
          <Do><b>Do</b> distinguish <b>"current"</b> worktree (green badge — where cwd lives) from the <b>selected</b> row (surface-active fill — what you're viewing). They can differ: you can inspect a non-current worktree without switching anything.</Do>
        </Spec>
        <Spec title="Diff variant — file row → unified diff"
          when={<>Clicking a file in Changes swaps the bottom pane (log → diff): a <b>Back</b> toolbar + file path, then unified hunks with an old/new line-number gutter. Hunk header in <span className="tok">--tasty-accent-info</span>, <code>+</code> in success, <code>-</code> in danger, context primary. Back returns to the log.</>}>
          <Stage variant="solo center" style={{ padding: 20, background: "var(--tasty-bg-app)" }}><GitViewerFrame diff /></Stage>
          <Meta
            specs={[["trigger", "select a Changes row → diff"], ["toolbar", "Back + file path (muted/mono)"], ["gutter", "old / new line numbers (mono)"], ["hunk header", <span className="tok">--tasty-accent-info</span>], ["empty diff", "Back + 'No changes.'"], ["states", "non-repo · error · loading · already-open · 0 worktrees"]]}
            tokens={[{ tok: "--tasty-accent-info", use: "hunk header", color: "var(--tasty-accent-info)" }, { tok: "--tasty-accent-success", use: "added line", color: "var(--tasty-accent-success)" }, { tok: "--tasty-accent-danger", use: "deleted line", color: "var(--tasty-accent-danger)" }, { tok: "--tasty-text-disabled", use: "line-number gutter", color: "var(--tasty-text-disabled)" }]} />
          <Note>Empty/edge states reuse <span className="ic">--tasty-text-muted</span> one-liners: "No git repository…", "No changes.", "No commits.", "No worktrees.", "Git viewer is already open." Errors are a single <span className="ic">--tasty-accent-danger</span> line above the (still shown) panes.</Note>
        </Spec>
      </Section>

      <Section id="clipboard" title="Clipboard viewer">
        <Spec title="Clipboard viewer — read-only snapshot popup"
          when={<>Tools → Clipboard Viewer (or a toggle shortcut). A <b>480×360</b> screen-centered popup that shows what's on the system clipboard <b>right now</b> — a one-time snapshot, <b>not</b> history. <b>Single column</b>: a header (clipboard icon + <span className="ic">snapshot</span> tag), a <b>type bar</b>, then a recessed mono <b>code well</b>. The type bar treats a <b>single type as first-class</b> — just a <span className="ic">Text</span> badge + live metadata (<span className="ic">284 chars · 6 lines · UTF-8</span>); two-plus types expand it into a <b>segmented switch</b> (Text / Image / Files / HTML / Other). <b>No rail.</b> At <b>5 segments</b> the switch compacts to <b>icon-only</b> (the active segment keeps its label; tooltips carry the names) so it survives 20–40% longer translations inside 480px. <b>HTML</b> is shown as <b>source text only — never rendered</b>; a <span className="ic">Pretty print</span> checkbox takes over the type bar's meta slot (metadata moves beside the mime in the footer) and re-indents the same source instantly (0ms). <b>Other</b> buckets every format that isn't text/image/files/html — one row per format: mono name + size, then its textualized content, separated by 1px rules, all of it scrolling in the same well (long content clamps with a muted <span className="ic">+N more lines</span>). <b>Read-only</b> — no copy/paste/edit/delete.</>}>
          <Stage variant="solo center" style={{ padding: 20, background: "var(--tasty-bg-app)", gap: 16, flexWrap: "wrap" }}>
            <div style={{ display: "flex", flexDirection: "column", gap: 6 }}><div style={{ fontSize: 11, color: "var(--tasty-text-muted)" }}>single type (Text)</div><ClipboardFrame /></div>
            <div style={{ display: "flex", flexDirection: "column", gap: 6 }}><div style={{ fontSize: 11, color: "var(--tasty-text-muted)" }}>multi-type · segmented</div><ClipboardFrame state="multi" /></div>
            <div style={{ display: "flex", flexDirection: "column", gap: 6 }}><div style={{ fontSize: 11, color: "var(--tasty-text-muted)" }}>HTML · source (Pretty print off)</div><ClipboardFrame state="html" /></div>
            <div style={{ display: "flex", flexDirection: "column", gap: 6 }}><div style={{ fontSize: 11, color: "var(--tasty-text-muted)" }}>HTML · Pretty print on</div><ClipboardFrame state="htmlPretty" /></div>
            <div style={{ display: "flex", flexDirection: "column", gap: 6 }}><div style={{ fontSize: 11, color: "var(--tasty-text-muted)" }}>Other · format buckets</div><ClipboardFrame state="other" /></div>
            <div style={{ display: "flex", flexDirection: "column", gap: 6 }}><div style={{ fontSize: 11, color: "var(--tasty-text-muted)" }}>5 types · compact icon-only segments</div><ClipboardFrame state="five" /></div>
            <div style={{ display: "flex", flexDirection: "column", gap: 6 }}><div style={{ fontSize: 11, color: "var(--tasty-text-muted)" }}>empty</div><ClipboardFrame state="empty" /></div>
            <div style={{ display: "flex", flexDirection: "column", gap: 6 }}><div style={{ fontSize: 11, color: "var(--tasty-text-muted)" }}>read failed</div><ClipboardFrame state="failed" /></div>
          </Stage>
          <Meta
            specs={[["frame", "480 × 360 · screen-center · dismiss outside"], ["layout", "single column — no rail"], ["type bar", "1 type = badge + meta · 2–4 = segmented · ≥5 = icon-only"], ["segment", "h 26 · pad 0 10 (icon-only 0 8, min-w 30)"], ["metadata", "chars · lines · encoding (mono 11, right) — moves to footer on HTML"], ["HTML body", "source only, never rendered · Pretty print = re-indent, 0ms"], ["Other body", "per format: mono 11 name + size / mono 12 content · 1px rules"], ["body", "recessed code well — bg-app · mono 12 · scroll"], ["empty / fail", "centered glyph + muted / danger one-liner"]]}
            tokens={[{ tok: "--tasty-bg-panel", use: "frame", color: "var(--tasty-bg-panel)" }, { tok: "--tasty-bg-app", use: "code well", color: "var(--tasty-bg-app)" }, { tok: "--tasty-bg-sidebar", use: "type bar", color: "var(--tasty-bg-sidebar)" }, { tok: "--tasty-accent-primary", use: "selected type segment", color: "var(--tasty-accent-primary)" }, { tok: "--tasty-accent-danger", use: "read-failed", color: "var(--tasty-accent-danger)" }]} />
          <Dont><b>Don't</b> add any mutate action (copy / re-copy / clear) or a master-detail type rail — this is a one-column snapshot inspector. Snapshot is taken once at open; no live polling, no history. <b>Don't</b> render the HTML payload (no live preview, no styled output) and <b>don't</b> let the type bar scroll horizontally or truncate labels with ellipsis — compact to icon-only instead.</Dont>
        </Spec>
      </Section>

      <Section id="moveresize" title="Move & resize — popup interaction spec">
        <Spec title="Drag handles — TitleBar · Region · None"
          when={<>How a popup moves. A <b>titlebar</b> popup drags from the <b>whole bar</b> (the close button is excluded). A <b>headless</b> popup can declare a <b>Region</b> handle — a header band (currently the left half: icon + title) — while the header's own widgets (search, close) keep priority. <b>None</b> means the popup can't be moved. Affordance is the <b>cursor</b> only (grab → grabbing); no painted grip. Hover the bars below to feel the cursors.</>}>
          <Stage variant="solo center" style={{ padding: 24, background: "var(--tasty-bg-app)", gap: 20, flexWrap: "wrap", alignItems: "flex-start" }}>
            <div style={{ display: "flex", flexDirection: "column", gap: 6 }}>
              <div style={{ fontSize: 11, color: "var(--tasty-text-muted)" }}>TitleBar — whole bar drags</div>
              <FauxPopup title="Listening ports"><div>title centered · close excluded from the handle</div></FauxPopup>
            </div>
            <div style={{ display: "flex", flexDirection: "column", gap: 6 }}>
              <div style={{ fontSize: 11, color: "var(--tasty-text-muted)" }}>Region — header band drags, widgets win</div>
              <FauxPopup titlebar={false} region w={260}><div>left band = grab · search/close = their own cursor</div></FauxPopup>
            </div>
            <div style={{ display: "flex", flexDirection: "column", gap: 6 }}>
              <div style={{ fontSize: 11, color: "var(--tasty-text-muted)" }}>None — not movable</div>
              <FauxPopup title="apply preset" grab={false}><div>no handle · grab cursor never appears</div></FauxPopup>
            </div>
          </Stage>
          <Meta
            specs={[["TitleBar", "whole bar drags, close excluded"], ["Region", "header left band (headless popups)"], ["None", "immovable"], ["affordance", "cursor only — no painted grip"], ["priority", "widget > close > resize edge > drag handle > content"], ["start gate", "after content render — widget pointer wins"]]}
            tokens={[{ tok: "--tasty-bg-app", use: "titlebar fill (mantle)", color: "var(--tasty-bg-app)" }, { tok: "--tasty-surface-raised", use: "popup body", color: "var(--tasty-surface-raised)" }, { tok: "--tasty-titlebar-height", use: "bar / band height" }]} />
          <Dont><b>Don't</b> tint or outline the drag band over a widget — the widget always wins the press, so a visual grip there only invites mis-clicks. Let the cursor carry the affordance.</Dont>
        </Spec>
        <Spec title="8-direction resize — handle map & cursors"
          when={<>Opt-in (<span className="ic">resizable</span> popups — today: <b>Listening ports</b> & <b>Remote connections</b>). A <b>band just inside the border</b> holds 8 hit zones: 4 edges + 4 corners (corners take priority). There's <b>no visual grip</b> — the cursor is the only affordance. Dragging an edge moves <b>only that edge</b> (the opposite side stays fixed); a corner moves two. Hover the outlined zones to see each cursor.</>}>
          <Stage variant="solo center" style={{ padding: 28, background: "var(--tasty-bg-app)", gap: 28, flexWrap: "wrap", alignItems: "center" }}>
            <ResizeMap />
            <CursorMatrix />
          </Stage>
          <Meta
            specs={[["zones", "N · S · E · W + NW · NE · SW · SE"], ["band", "inside the border (token-width)"], ["corner", "wins over edge at the crossing"], ["edge drag", "that edge moves, opposite fixed"], ["grip", "none — cursor only"], ["persist", "user size sticks until the popup closes"]]}
            tokens={[{ tok: "--tasty-accent-info", use: "zone labels (demo only)", color: "var(--tasty-accent-info)" }, { tok: "--tasty-border-strong", use: "popup edge", color: "var(--tasty-border-strong)" }]} />
          <Note>Once you resize, the size <b>sticks</b> (the sizer won't overwrite it); closing the popup resets it to its default size on next open.</Note>
        </Spec>
        <Spec title="Constraints & z-order — quiet limits, top-most wins"
          when={<>Limits are enforced <b>silently</b>. A popup can't shrink past its <b>min size</b> (or its default size if none is set), and <b>no part</b> may leave the scope rect (Window / Workspace / Pane / Tab / Surface) — drag or resize just stops at the boundary, and a shrinking scope <b>auto-repositions</b> the popup back inside. The scope edge is the effective max (there's no separate max size). When popups overlap, only the <b>top-most</b> one's handles/cursor respond; pressing a popup brings it to front.</>}>
          <Stage variant="solo center" style={{ padding: 24, background: "var(--tasty-bg-app)", gap: 28, flexWrap: "wrap", alignItems: "center" }}>
            <div style={{ display: "flex", flexDirection: "column", gap: 6 }}>
              <div style={{ fontSize: 11, color: "var(--tasty-text-muted)" }}>scope clamp — stops at the boundary</div>
              <ClampDemo />
            </div>
            <div style={{ display: "flex", flexDirection: "column", gap: 6 }}>
              <div style={{ fontSize: 11, color: "var(--tasty-text-muted)" }}>z-order — only the front popup responds</div>
              <ZOrderDemo />
            </div>
          </Stage>
          <Meta
            specs={[["min size", "min_size, else default_size"], ["max", "scope rect is the cap (no max_size)"], ["clamp", "no part leaves the scope"], ["reposition", "shrinking scope pulls it back in"], ["limit signal", "none — quiet stop (no warning color)"], ["z-order", "top-most only · press = bring to front"]]}
            tokens={[{ tok: "--tasty-border-strong", use: "scope rect (demo)", color: "var(--tasty-border-strong)" }, { tok: "--tasty-shadow-modal", use: "popup lift" }]} />
          <Dont><b>Don't</b> flash a warning color (peach/red) when a popup hits min size or the scope edge — the limit is communicated by simply <b>not moving</b>. Reserve danger tones for actual errors.</Dont>
        </Spec>
      </Section>
      <style>{`.rz-zone:hover{background:color-mix(in srgb,var(--tasty-accent-info) 14%,transparent);} .faux-close:hover{color:var(--tasty-titlebar-close-hover-fg);background:var(--tasty-titlebar-close-hover-bg);}`}</style>
    </>
  );
}

window.Gallery.mount(
  "overlays-windows",
  NAV,
  {
    title: "Windows",
    intro: "Large modal surfaces — launchers, data tables, tabbed and two-tier windows. Bigger frames carrying their own internal navigation; the canonical dimensions are spelled out so they stay consistent across the app.",
    howto: false,
  },
  <Page />
);
