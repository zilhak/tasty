// Tasty Gallery — Overlays · Windows (large modal surfaces)
// One of the five Overlays sub-pages (dialogs · windows · popups · banners · tutorial). Frame components are shared from
// overlays-shared.jsx (window.OverlaysShared); this file holds only the
// page's specimens + nav. See the other overlays-*.jsx for the rest.
const { Section, Spec, Stage, Meta, Note, Do, Dont, GIcon } = window.Gallery;
const { Kbd, IconButton, Input, Button } = window.TastyDesignSystem_41fd3f;
// Distinct names — the sibling gallery scripts already bind Icon/Checkbox/
// Select/Tag at top level, and a second binding of the same name resolves to
// undefined here (invalid element type).
const WIcon = window.TastyDesignSystem_41fd3f.Icon;
const WCheckbox = window.TastyDesignSystem_41fd3f.Checkbox;
const WSelect = window.TastyDesignSystem_41fd3f.Select;
const WTag = window.TastyDesignSystem_41fd3f.Tag;
const { Backdrop, PaletteFrame, LocalSshSection, PortsFrame, PortsFavoritesG, PortStarG, RemoteFrame, SettingsFrame, SettingsGeneralOverlayFrame, SettingsRemoteTransferFrame, ToastDragValue, GitViewerFrame, ClipboardFrame, RemoteFormFrame, RemoteAttachFrame, RaNewRow, RaWsPeek, FilePickerFrame, ScriptManagerFrame, ic } = window.OverlaysShared;

const NAV = [
  { id: "palette", label: "Command palette" },
  { id: "ports", label: "Listening ports" },
  { id: "remote", label: "Remote connections" },
  { id: "remoteattach", label: "Add remote workspace" },
  { id: "filepicker", label: "File picker" },
  { id: "preseteditor", label: "Preset editor" },
  { id: "settings", label: "Settings window" },
  { id: "permissions", label: "General › Permissions (macOS)" },
  { id: "kbimportexport", label: "Keybindings · Import / Export" },
  { id: "pluginswindow", label: "Plugins window · avatar" },
  { id: "scripts", label: "Misc · Scripts" },
  { id: "gitviewer", label: "Git viewer" },
  { id: "clipboard", label: "Clipboard viewer" },
  { id: "moveresize", label: "Move & resize" },
];

// ── Plugin avatar — the canonical identity mark (Plugins window) ──────
// ONE component, TWO sizes. Values are component tokens (--tasty-plugin-avatar-*);
// this specimen is the design SoT the real widget is transcribed from.
function PluginAvatarG({ initial, size = "sm" }) {
  const lg = size === "lg";
  const s = lg ? "lg" : "sm";
  return (
    <span style={{ width: `var(--tasty-plugin-avatar-size-${s})`, height: `var(--tasty-plugin-avatar-size-${s})`,
      flex: "none", borderRadius: "var(--tasty-plugin-avatar-radius)", display: "inline-flex",
      alignItems: "center", justifyContent: "center", background: "var(--tasty-plugin-avatar-bg)",
      border: "var(--tasty-plugin-avatar-border-width) solid var(--tasty-plugin-avatar-border)",
      fontFamily: "var(--tasty-font-mono)", fontWeight: "var(--tasty-plugin-avatar-initial-weight)",
      color: "var(--tasty-plugin-avatar-fg)", lineHeight: 1,
      fontSize: `var(--tasty-plugin-avatar-initial-font-size-${s})` }}>{initial}</span>
  );
}

// A Plugins-window list row at each row state — the mark does not change.
function PluginRowG({ name, meta, state = "rest", disabled }) {
  const bg = state === "selected" ? "var(--tasty-surface-active)" : state === "hover" ? "var(--tasty-overlay-hover)" : "transparent";
  return (
    <div style={{ display: "flex", alignItems: "center", gap: "var(--tasty-space-sm)", width: 260,
      padding: "var(--tasty-space-sm)", borderRadius: "var(--tasty-radius)", background: bg,
      boxShadow: state === "selected" ? "inset var(--tasty-selection-edge-width) 0 0 var(--tasty-accent-primary)" : "none",
      opacity: disabled ? "var(--tasty-state-dim-opacity)" : 1 /* disabled PLUGIN = dimmed item, still selectable */ }}>
      <PluginAvatarG initial={name.charAt(0).toUpperCase()} />
      <div style={{ flex: 1, minWidth: 0 }}>
        <div style={{ fontSize: "var(--tasty-font-size-body)", color: "var(--tasty-text-primary)" }}>{name}</div>
        <div style={{ marginTop: 2, fontFamily: "var(--tasty-font-mono)", fontSize: "var(--tasty-font-size-micro)", color: "var(--tasty-text-muted)" }}>{meta}</div>
      </div>
    </div>
  );
}

// ── Keybindings › Import / Export — specimen parts ────────────────
const ieMono = { fontFamily: "var(--tasty-font-mono)", fontSize: 12 };
const ieCaps = { fontFamily: "var(--tasty-font-mono)", fontSize: 10, textTransform: "uppercase",
  letterSpacing: "var(--tasty-letter-spacing-caps)" };

// L2 tail with the SEPARATED row (new axis on the settings L2 row model).
function IeL2Tail() {
  const row = (label, active) => (
    <div key={label} style={{ display: "flex", alignItems: "center", padding: "4px 8px", borderRadius: "var(--tasty-radius-sm)", fontSize: 13,
      color: active ? "var(--tasty-text-primary)" : "var(--tasty-text-muted)",
      background: active ? "var(--tasty-surface-active)" : "transparent" }}>{label}</div>
  );
  return (
    <div style={{ width: "var(--tasty-settings-sidebar-width)", padding: 8, background: "var(--tasty-bg-sidebar)",
      border: "1px solid var(--tasty-separator)", borderRadius: "var(--tasty-radius)" }}>
      {["Explorer", "Scripts", "Preset", "Plugins"].map((l) => row(l, false))}
      <div style={{ height: 1, background: "var(--tasty-separator)", margin: "8px" }} />
      {row("Import / Export", true)}
    </div>
  );
}

function IeEntry() {
  const card = (glyph, title, desc, btn) => (
    <div style={{ display: "flex", alignItems: "flex-start", gap: 12, padding: "12px 14px", borderRadius: "var(--tasty-radius)",
      background: "var(--tasty-surface-raised)", border: "1px solid var(--tasty-border-default)" }}>
      <span style={{ display: "inline-flex", flex: "none", marginTop: 2, color: "var(--tasty-text-muted)" }}><WIcon name={glyph} size={16} /></span>
      <div style={{ flex: 1, minWidth: 0 }}>
        <div style={{ fontSize: 13, color: "var(--tasty-text-primary)" }}>{title}</div>
        <p style={{ margin: "2px 0 0", fontSize: 12, color: "var(--tasty-text-muted)", lineHeight: "var(--tasty-line-height-ui)" }}>{desc}</p>
      </div>
      <span style={{ flex: "none" }}>{btn}</span>
    </div>
  );
  return (
    <div style={{ width: "100%", maxWidth: 620, display: "flex", flexDirection: "column", gap: 12 }}>
      {card("download", "Export", "Writes every binding — general, quick switch, script bindings and plugin overrides — to one file.",
        <Button variant="secondary" size="sm">Export…</Button>)}
      {card("file", "Import", "Reads a keybinding file and shows the changes against your current bindings before anything is written.",
        <Button variant="primary" size="sm">Import…</Button>)}
    </div>
  );
}

// Diff grid — select column + group header axis.
function IeGrid() {
  const head = { ...ieCaps, color: "var(--tasty-text-muted)", padding: "0 12px 8px", borderBottom: "1px solid var(--tasty-separator)" };
  const cell = { padding: "8px 12px", borderBottom: "1px solid var(--tasty-separator)", fontSize: 13, display: "flex", alignItems: "center" };
  const group = (label, counts, open = true) => (
    <div style={{ gridColumn: "1 / -1", display: "flex", alignItems: "center", gap: 8, padding: "8px 12px",
      background: "var(--tasty-surface-raised)", borderBottom: "1px solid var(--tasty-separator)" }}>
      <WCheckbox defaultChecked={open} aria-label={"Select all in " + label} />
      <span style={{ display: "inline-flex", color: "var(--tasty-text-muted)" }}><WIcon name={open ? "chevronDown" : "chevronRight"} size={14} /></span>
      <span style={{ ...ieCaps, color: "var(--tasty-text-secondary)" }}>{label}</span>
      <span style={{ ...ieMono, fontSize: 11, color: "var(--tasty-text-muted)" }}>{counts}</span>
    </div>
  );
  const row = (action, cur, next, extra) => (
    <React.Fragment key={action}>
      <div style={{ ...cell, justifyContent: "center" }}><WCheckbox defaultChecked aria-label={"Apply " + action} /></div>
      <div style={{ ...cell, flexDirection: "column", alignItems: "flex-start", gap: 2, color: "var(--tasty-text-secondary)" }}>
        <span>{action}</span>{extra}
      </div>
      <div style={{ ...cell, ...ieMono, color: "var(--tasty-text-muted)" }}>{cur}</div>
      <div style={{ ...cell, ...ieMono, color: cur === next ? "var(--tasty-text-muted)" : "var(--tasty-accent-primary)" }}>{next}</div>
    </React.Fragment>
  );
  return (
    <div style={{ width: "100%", maxWidth: 620, display: "grid", gridTemplateColumns: "32px minmax(0,1.6fr) 1fr 1fr", alignItems: "stretch" }}>
      <div style={head} /><div style={{ ...head, textAlign: "left" }}>Action</div><div style={head}>Current</div><div style={head}>Imported</div>
      {group("General bindings", "3 changed · 61 total")}
      {row("Command palette", "Ctrl+K", "Ctrl+Shift+P")}
      {row("Split vertical", "Ctrl+D", "Ctrl+Alt+D")}
      {group("Quick switch (axis summary)", "2 changed · 3 axes")}
      {row("Tab axis", "Alt+1…0", "Ctrl+1…0", <span style={{ fontSize: 10, color: "var(--tasty-text-muted)" }}>10 slots follow this axis</span>)}
      {group("Plugin overrides", "1 changed · 2 total")}
      {row("review staged", "Ctrl+Alt+R", "Ctrl+Shift+R",
        <span style={{ ...ieMono, fontSize: 10, color: "var(--tasty-text-muted)", display: "flex", alignItems: "center", gap: 5 }}>
          <span style={{ width: "var(--tasty-status-dot-size)", height: "var(--tasty-status-dot-size)", borderRadius: "50%",
            background: "var(--tasty-accent-agent)" }} />git-helper</span>)}
      {group("Script bindings", "1 changed · 2 total", false)}
    </div>
  );
}

// Migration card — `state`: "pending" | "resolved".
function IeMigrateG({ state = "pending" }) {
  const done = state === "resolved";
  const tone = done ? "var(--tasty-accent-success)" : "var(--tasty-accent-warning)";
  const row = (action, from, widget, trail, sub, subTone) => (
    <div style={{ display: "flex", flexDirection: "column", gap: 4, padding: "8px 0", borderTop: "1px solid var(--tasty-separator)" }}>
      <div style={{ display: "flex", alignItems: "center", gap: 12, minHeight: 28, flexWrap: "wrap" }}>
        <span style={{ width: 200, flex: "none", fontSize: 13, color: "var(--tasty-text-secondary)" }}>{action}</span>
        <span style={{ ...ieMono, width: 110, flex: "none", color: "var(--tasty-text-muted)" }}>{from}</span>
        <span style={{ display: "inline-flex", color: "var(--tasty-text-muted)" }}><WIcon name="chevronRight" size={14} /></span>
        {widget}{trail}
      </div>
      {sub && <div style={{ paddingLeft: 200, fontSize: 11, color: subTone || "var(--tasty-text-muted)" }}>{sub}</div>}
    </div>
  );
  const slot = (label, tone2) => (
    <span style={{ minWidth: 140, height: 24, display: "inline-flex", alignItems: "center", padding: "0 8px", ...ieMono,
      background: "var(--tasty-surface-raised)", color: tone2 === "empty" ? "var(--tasty-text-disabled)" : "var(--tasty-text-primary)",
      border: "1px solid " + (tone2 === "conflict" ? "var(--tasty-accent-danger)" : "var(--tasty-border-default)"),
      borderRadius: "var(--tasty-radius)" }}>{label}</span>
  );
  return (
    <div style={{ width: "100%", maxWidth: 620, borderRadius: "var(--tasty-radius)", padding: "12px 14px",
      background: "color-mix(in srgb, " + tone + " 11%, transparent)",
      border: "1px solid color-mix(in srgb, " + tone + " 36%, transparent)" }}>
      <div style={{ display: "flex", alignItems: "center", gap: 8, color: tone, fontSize: 13, fontWeight: 600 }}>
        <WIcon name={done ? "check" : "alertTriangle"} size={16} />
        <span>{done ? "Option bindings resolved" : "Option bindings need a replacement"}</span>
        <span style={{ marginLeft: "auto", ...ieMono, fontSize: 11, color: tone }}>{done ? "4 of 4 resolved" : "2 of 4 unresolved"}</span>
      </div>
      <p style={{ margin: "4px 0 0", fontSize: 12, color: "var(--tasty-text-secondary)", lineHeight: "var(--tasty-line-height-ui)" }}>
        {done ? <>Every option-bearing binding has a replacement or is left unbound. <b>Apply</b> is enabled.</>
          : <><span style={ieMono}>option</span> never matches on this OS — these bindings would look bound and do nothing. <b>Apply</b> stays disabled until none are left.</>}
      </p>
      <div style={{ marginTop: 8 }}>
        {row("Screenshot to clipboard", "Option+Shift+4", slot("Ctrl+Shift+4"),
          <span style={{ display: "inline-flex", color: "var(--tasty-accent-success)" }}><WIcon name="check" size={14} /></span>)}
        {row("Category axis modifier", "Option",
          <WSelect options={done ? ["Ctrl+Alt"] : ["Select a modifier"]} style={{ width: "var(--tasty-field-width-md)" }} />,
          done ? <span style={{ display: "inline-flex", color: "var(--tasty-accent-success)" }}><WIcon name="check" size={14} /></span>
            : <span style={{ fontSize: 11, color: "var(--tasty-accent-warning)" }}>Not set</span>,
          "10 slots on this axis change with it")}
        {!done && row("Toggle vi mode", "Option+V", slot("Ctrl+Shift+C", "conflict"), null,
          "Also bound to Copy — the shortcut-conflict popup opens on Apply.", "var(--tasty-accent-danger)")}
        {row("Jump to error", "Option+E", slot(done ? "Unbound" : "Not set", done ? null : "empty"),
          done ? <WTag>Unbound — counts as resolved</WTag> : <Button variant="ghost" size="sm">Leave unbound</Button>)}
      </div>
    </div>
  );
}

function IeBackBarG({ unresolved }) {
  return (
    <div style={{ width: "100%", maxWidth: 620, display: "flex", alignItems: "center", gap: 8, height: 40, padding: "0 12px",
      background: "var(--tasty-bg-sidebar)", border: "1px solid var(--tasty-separator)", borderRadius: "var(--tasty-radius)" }}>
      <IconButton size="sm" aria-label="Back">{ic.back}</IconButton>
      <span style={{ fontSize: 13, color: "var(--tasty-text-primary)" }}>Import keybindings</span>
      <span style={{ marginLeft: "auto", display: "flex", alignItems: "center", gap: 8 }}>
        <Button variant="ghost" size="sm">{unresolved ? "Show all 73" : "Changed only"}</Button>
        {unresolved > 0 && <span style={{ ...ieMono, fontSize: 11, color: "var(--tasty-accent-warning)" }}>{unresolved} unresolved</span>}
        <Button variant="primary" size="sm" disabled={unresolved > 0}>Apply</Button>
      </span>
    </div>
  );
}

function IeNotices() {
  return (
    <div style={{ width: "100%", maxWidth: 620, display: "flex", flexDirection: "column", gap: 12 }}>
      <div style={{ display: "flex", alignItems: "flex-start", gap: 8, fontSize: 12, color: "var(--tasty-text-muted)" }}>
        <span style={{ display: "inline-flex", flex: "none", marginTop: 1 }}><WIcon name="helpCircle" size={14} /></span>
        <span>2 plugin overrides were dropped — those plugins aren't installed here (<span style={ieMono}>k8s-lens, s3-browser</span>).</span>
      </div>
      <div style={{ fontSize: 12, color: "var(--tasty-text-muted)" }}>
        <span style={ieMono}>tasty-keybindings-2026-09-09.toml</span> — <b style={{ color: "var(--tasty-text-secondary)" }}>7</b> of 73 bindings change.
        {" "}No <span style={ieMono}>option</span> bindings to migrate.
      </div>
      <div style={{ borderRadius: "var(--tasty-radius)", padding: "12px 14px",
        background: "color-mix(in srgb, var(--tasty-accent-danger) 12%, transparent)",
        border: "1px solid color-mix(in srgb, var(--tasty-accent-danger) 35%, transparent)" }}>
        <div style={{ display: "flex", alignItems: "center", gap: 8, color: "var(--tasty-accent-danger)", fontSize: 13, fontWeight: 600 }}>
          <WIcon name="alertCircle" size={16} /><span>This file can't be read as keybindings</span>
        </div>
        <p style={{ margin: "4px 0 8px", fontSize: 12, color: "var(--tasty-text-secondary)", lineHeight: "var(--tasty-line-height-ui)" }}>
          <span style={ieMono}>~/Downloads/settings.json</span> — expected a keybinding export (TOML, a <span style={ieMono}>[keybindings]</span> table);
          parsing stopped at line 1. Nothing was changed.
        </p>
        <Button variant="secondary" size="sm">Choose another file</Button>
      </div>
    </div>
  );
}

// ── Import / Export — the six values the spec left open ───────────
// Shared notice block recipe: tone + glyph + title + lines. Same geometry as
// the parse-failure block, so failure / warning / info differ only in tone.
function IeBlockG({ tone, glyph, title, count, children, action }) {
  return (
    <div style={{ width: "100%", maxWidth: "var(--tasty-settings-content-max-width)", borderRadius: "var(--tasty-radius)",
      padding: "var(--tasty-kb-ie-notice-inset)", background: "color-mix(in srgb, " + tone + " 12%, transparent)",
      border: "1px solid color-mix(in srgb, " + tone + " 35%, transparent)" }}>
      <div style={{ display: "flex", alignItems: "center", gap: 8, color: tone, fontSize: 13, fontWeight: 600 }}>
        <WIcon name={glyph} size={16} /><span>{title}</span>
        {count && <span style={{ marginLeft: "auto", ...ieMono, fontSize: 11, color: tone }}>{count}</span>}
      </div>
      <div style={{ margin: "4px 0 0", fontSize: 12, color: "var(--tasty-text-secondary)", lineHeight: "var(--tasty-line-height-ui)" }}>{children}</div>
      {action && <div style={{ marginTop: 8, display: "flex", gap: 8 }}>{action}</div>}
    </div>
  );
}

// §1 — export failure. Success is a toast (nothing to do); failure replaces the
// Export row's description with an inline block, because the retry lives there.
function IeExportFailG({ reason = "readonly" }) {
  // §7 of the open-values round: the middle clause is a FIXED set, and the OS
  // string is never spliced into the sentence — it gets its own muted mono line.
  const clause = reason === "readonly" ? "the folder is read-only."
    : reason === "denied" ? "you don't have permission to write there."
    : reason === "space" ? "the disk is full."
    : "the write didn't finish.";
  return (
    <div style={{ width: "100%", maxWidth: "var(--tasty-settings-content-max-width)", display: "flex", flexDirection: "column", gap: 8,
      padding: "var(--tasty-kb-ie-notice-inset)", borderRadius: "var(--tasty-radius)",
      background: "var(--tasty-surface-raised)", border: "1px solid var(--tasty-border-default)" }}>
      <div style={{ display: "flex", alignItems: "flex-start", gap: 12 }}>
        <span style={{ display: "inline-flex", flex: "none", marginTop: 2, color: "var(--tasty-text-muted)" }}><WIcon name="download" size={16} /></span>
        <div style={{ flex: 1, minWidth: 0 }}>
          <div style={{ fontSize: 13, color: "var(--tasty-text-primary)" }}>Export</div>
          <p style={{ margin: "2px 0 0", fontSize: 12, color: "var(--tasty-text-muted)", lineHeight: "var(--tasty-line-height-ui)" }}>
            Writes every binding — general, quick switch, script bindings and plugin overrides — to one file.
          </p>
        </div>
        <span style={{ flex: "none" }}><Button variant="secondary" size="sm" disabled>Export…</Button></span>
      </div>
      <IeBlockG tone="var(--tasty-accent-danger)" glyph="alertCircle" title="The export wasn't written"
        action={<><Button variant="secondary" size="sm">Try again</Button><Button variant="ghost" size="sm">Choose another location…</Button></>}>
        <span style={ieMono}>~/tasty/tasty-keybindings-2026-09-14.toml</span> — {clause} Nothing was written.
        {reason === "other" && (
          <div style={{ ...ieMono, marginTop: 4, fontSize: 11, color: "var(--tasty-text-muted)", overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}
            title="os error 28: No space left on device (os reported)">os error 28: No space left on device</div>
        )}
      </IeBlockG>
    </div>
  );
}

// §2 — bundle warnings other than "plugin not installed". One warning block,
// one line per notice, count in the header. The dropped-override info line
// stays separate (muted, no tone) — it is a fact, not a warning.
function IeBundleNoticesG({ one = false }) {
  const line = (txt) => (
    <div style={{ display: "flex", alignItems: "flex-start", gap: 6, marginTop: 4 }}>
      <span style={{ flex: "none", color: "var(--tasty-text-muted)" }}>·</span><span>{txt}</span>
    </div>
  );
  return (
    <div style={{ width: "100%", maxWidth: "var(--tasty-settings-content-max-width)", display: "flex", flexDirection: "column", gap: 12 }}>
      <div style={{ display: "flex", alignItems: "flex-start", gap: 8, fontSize: 12, color: "var(--tasty-text-muted)" }}>
        <span style={{ display: "inline-flex", flex: "none", marginTop: 1 }}><WIcon name="helpCircle" size={14} /></span>
        <span>2 plugin overrides were dropped — those plugins aren't installed here (<span style={ieMono}>k8s-lens, s3-browser</span>).</span>
      </div>
      <IeBlockG tone="var(--tasty-accent-warning)" glyph="alertTriangle" title="Read with warnings" count={one ? "1 notice" : "4 notices"}
        action={one ? null : <Button variant="ghost" size="sm">Show 1 more</Button>}>
        {one
          ? line(<>1 unknown action was skipped (<span style={ieMono}>tab.pin</span>).</>)
          : <>
            {line(<>Written by a newer schema (<span style={ieMono}>v3</span>, this build reads <span style={ieMono}>v2</span>) — unreadable parts were skipped.</>)}
            {line(<>2 unknown actions were skipped (<span style={ieMono}>pane.zoom_cycle, tab.pin</span>).</>)}
            {line(<>The group <span style={ieMono}>[keybindings.image]</span> is empty — nothing to import from it.</>)}
          </>}
      </IeBlockG>
    </div>
  );
}

// §3 — parsing failure with and without a line number.
function IeParseFailG({ line = true }) {
  return (
    <IeBlockG tone="var(--tasty-accent-danger)" glyph="alertCircle" title="This file can't be read as keybindings"
      action={<Button variant="secondary" size="sm">Choose another file</Button>}>
      <span style={ieMono}>~/Downloads/settings.json</span> — expected a keybinding export (TOML, a <span style={ieMono}>[keybindings]</span> table);
      {line ? <> parsing stopped at line 1.</> : <> the file isn't TOML.</>} Nothing was changed.
    </IeBlockG>
  );
}

// §4 — several conflicts. Count first, in the card; the rows keep their own
// inline reason, so the summary never repeats the list.
function IeConflictSummaryG() {
  const row = (action, from, to, reason) => (
    <div style={{ display: "flex", flexDirection: "column", gap: 4, padding: "8px 0", borderTop: "1px solid var(--tasty-separator)" }}>
      <div style={{ display: "flex", alignItems: "center", gap: 12, minHeight: 28, flexWrap: "wrap" }}>
        <span style={{ width: "var(--tasty-kb-ie-action-column-width)", flex: "none", fontSize: 13, color: "var(--tasty-text-secondary)",
          overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>{action}</span>
        <span style={{ ...ieMono, width: "var(--tasty-kb-ie-from-column-width)", flex: "none", color: "var(--tasty-text-muted)" }}>{from}</span>
        <span style={{ display: "inline-flex", color: "var(--tasty-text-muted)" }}><WIcon name="chevronRight" size={14} /></span>
        <span style={{ minWidth: "var(--tasty-kb-ie-slot-min-width)", height: "var(--tasty-kb-ie-slot-height)", display: "inline-flex", alignItems: "center",
          padding: "0 8px", ...ieMono, background: "var(--tasty-surface-raised)", color: "var(--tasty-text-primary)",
          border: "1px solid var(--tasty-accent-danger)", borderRadius: "var(--tasty-radius)" }}>{to}</span>
      </div>
      <div style={{ paddingLeft: "var(--tasty-kb-ie-action-column-width)", display: "flex", alignItems: "center", gap: 6, fontSize: 11, color: "var(--tasty-accent-danger)" }}>
        <WIcon name="alertTriangle" size={14} /><span>{reason}</span>
      </div>
    </div>
  );
  return (
    <div style={{ width: "100%", maxWidth: "var(--tasty-settings-content-max-width)", borderRadius: "var(--tasty-radius)", padding: "var(--tasty-kb-ie-notice-inset)",
      background: "color-mix(in srgb, var(--tasty-accent-warning) 11%, transparent)",
      border: "1px solid color-mix(in srgb, var(--tasty-accent-warning) 36%, transparent)" }}>
      <div style={{ display: "flex", alignItems: "center", gap: 8, color: "var(--tasty-accent-warning)", fontSize: 13, fontWeight: 600 }}>
        <WIcon name="alertTriangle" size={16} /><span>Option bindings need a replacement</span>
        <span style={{ marginLeft: "auto", ...ieMono, fontSize: 11 }}>3 of 4 unresolved</span>
      </div>
      <p style={{ margin: "4px 0 0", fontSize: 12, color: "var(--tasty-text-secondary)", lineHeight: "var(--tasty-line-height-ui)" }}>
        <span style={{ color: "var(--tasty-accent-danger)", fontWeight: 600 }}>3 conflicts</span> — those shortcuts are already bound.
        The shortcut-conflict popup opens on Apply.
      </p>
      <div style={{ marginTop: 8 }}>
        {row("Toggle vi mode", "Option+V", "Ctrl+Shift+C", "Also bound to Copy")}
        {row("Jump to error", "Option+E", "Ctrl+Shift+K", "Also bound to Clear scrollback")}
        {row("Screenshot to clipboard", "Option+Shift+4", "Ctrl+Shift+P", "Also bound to Command palette")}
      </div>
    </div>
  );
}

// §5 — the axis-modifier Select before anything is chosen.
function IeModifierSelectG({ chosen = false }) {
  return (
    <div style={{ display: "flex", alignItems: "center", gap: 12, minHeight: 28 }}>
      <span style={{ width: "var(--tasty-kb-ie-from-column-width)", flex: "none", ...ieMono, color: "var(--tasty-text-muted)" }}>Option</span>
      <span style={{ display: "inline-flex", color: "var(--tasty-text-muted)" }}><WIcon name="chevronRight" size={14} /></span>
      <WSelect options={chosen ? ["Ctrl+Alt"] : ["Select a modifier"]} style={{ width: "var(--tasty-field-width-md)",
        color: chosen ? "var(--tasty-text-primary)" : "var(--tasty-text-placeholder)" }} />
      {chosen
        ? <span style={{ display: "inline-flex", color: "var(--tasty-accent-success)" }}><WIcon name="check" size={14} /></span>
        : <span style={{ fontSize: 11, color: "var(--tasty-accent-warning)" }}>Not set</span>}
    </div>
  );
}

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
        <Spec title="Process column — a minimum, not a fixed width"
          when={<>The <b>Process</b> column's 200 is a <b>floor</b>: the column never shrinks below it and takes the table's spare width as the popup grows, because the process line (<code>node /usr/local/bin/vite --host</code>) is the cell most likely to be cut. The name is now public — <span className="tok">--tasty-port-process-col-min-width</span> — so the gallery's fixed 200 and the product's min 200 stop reading as two different numbers that happen to match. <b>No column hides</b> at any width; narrow tables ellipsise.</>}>
          <Stage variant="solo center" style={{ padding: 20, background: "var(--tasty-bg-app)", flexDirection: "column", gap: 14, alignItems: "flex-start" }}>
            {[["default — Process takes the spare width", 660], ["narrow — Process holds its 200 floor and ellipsises", 460]].map(([label, tw]) => (
              <div key={label} style={{ display: "flex", flexDirection: "column", gap: 6 }}>
                <div style={{ fontSize: 11, color: "var(--tasty-text-muted)" }}>{label}</div>
                <div style={{ width: tw, border: "1px solid var(--tasty-border-strong)", borderRadius: "var(--tasty-radius)", overflow: "hidden", background: "var(--tasty-bg-panel)" }}>
                  <div style={{ display: "flex", height: 24, alignItems: "center", fontSize: 11, color: "var(--tasty-text-secondary)", background: "var(--tasty-bg-sidebar)", borderBottom: "1px solid var(--tasty-separator)" }}>
                    <span style={{ flex: "none", width: "var(--tasty-port-star-col-width)" }} />
                    <span style={{ flex: "none", width: 64, paddingRight: 8, textAlign: "right" }}>Port</span>
                    <span style={{ flex: "none", width: 72, paddingLeft: 10 }}>Proto</span>
                    <span style={{ flex: 1, minWidth: "var(--tasty-port-process-col-min-width)", paddingLeft: 10 }}>Process</span>
                    <span style={{ flex: "none", width: 64, paddingRight: 10, textAlign: "right" }}>PID</span>
                  </div>
                  {[["3000", "tcp", "node /usr/local/bin/vite --host --strictPort", "41822"], ["5432", "tcp", "postgres: checkpointer", "913"]].map((r) => (
                    <div key={r[0]} style={{ display: "flex", height: 26, alignItems: "center", fontFamily: "var(--tasty-font-mono)", fontSize: 11, color: "var(--tasty-text-secondary)", borderBottom: "1px solid var(--tasty-separator)" }}>
                      <span style={{ flex: "none", width: "var(--tasty-port-star-col-width)", display: "inline-flex", justifyContent: "center", color: "var(--tasty-port-star-off)" }}><WIcon name="star" size={12} /></span>
                      <span style={{ flex: "none", width: 64, paddingRight: 8, textAlign: "right" }}>{r[0]}</span>
                      <span style={{ flex: "none", width: 72, paddingLeft: 10 }}>{r[1]}</span>
                      <span style={{ flex: 1, minWidth: "var(--tasty-port-process-col-min-width)", paddingLeft: 10, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>{r[2]}</span>
                      <span style={{ flex: "none", width: 64, paddingRight: 10, textAlign: "right" }}>{r[3]}</span>
                    </div>
                  ))}
                </div>
              </div>
            ))}
          </Stage>
          <Meta
            specs={[["role", "minimum width (flex-grow, never shrink)"], ["value", "200 — unchanged"], ["token", <span className="tok">--tasty-port-process-col-min-width</span>], ["zoom", "scales with the UI scale, like every width token"], ["hiding", "none — no column disappears"]]}
            tokens={[{ tok: "--tasty-port-process-col-min-width", use: "Process floor" }, { tok: "--tasty-port-star-col-width", use: "leading star column" }]} />
          <Note>The gallery specimen keeps a fixed stage width, so it pins the same 200 — same token, same number, one role.</Note>
        </Spec>
      </Section>

<Section id="remote" title="Remote connections">
        <Spec title="Remote connections — profiles + passkeys"
          when={<>Tools › Remote connections. A <b>520×460</b> modal with <b>three top tabs</b>: a type-agnostic <b>remote-profile</b> store (ssh / smb / http / anything), an <b>Attach</b> store (tasty-attach targets), and a separate <b>Passkey</b> credential store. Each tab routes list → form → confirm-delete off a shared header. SSH profiles get a dedicated form; everything else uses a generic key-value editor. The Profiles add-bar carries a <b>protocol filter</b> (right) — a dropdown of the protocols present, checkbox-toggled with Select all / Deselect all / Reset, committed on <b>Apply</b>. Secrets live only in passkeys, referenced by name.</>}>
          <Stage variant="solo center" style={{ padding: 20, background: "var(--tasty-bg-app)" }}><RemoteFrame /></Stage>
          <Meta
            specs={[["frame", "520 × 460"], ["tabs", "Remote profiles · Attach · Passkeys"], ["routes", "list → form → confirm"], ["filter", <>protocol dropdown (Profiles only) · menu <span className="tok">--tasty-remote-filter-menu-width</span> = 240, measured as the <b>border box</b></>], ["dismiss", <>×/Close/<span className="ic">Esc</span> only</>]]}
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
        <Spec title="Segmented active is an accent fill — tab strips keep the underline"
          when={<>Two components were drifting into one another. A <b>tab strip</b> (the window's Profiles / Attach / Passkeys bar) marks the current tab with a <b>2px accent underline</b> and a weight change, no fill. A <b>segmented control</b> (the attach form's <code>profile</code> / <code>inline</code> switch, the clipboard type segment, the apply-preset scope) marks the active segment with an <b>accent-primary fill</b> and <span className="tok">--tasty-text-on-accent</span> ink. <span className="tok">--tasty-surface-active</span> is the <b>row-selection</b> fill and is not used by either — that was the gallery's bug, now fixed.</>}>
          <Stage variant="solo center" style={{ padding: 20, background: "var(--tasty-bg-app)", gap: 18, flexWrap: "wrap", alignItems: "flex-start" }}>
            <div style={{ display: "flex", flexDirection: "column", gap: 6 }}>
              <div style={{ fontSize: 11, color: "var(--tasty-text-muted)" }}>tab strip — underline</div>
              <RemoteFrame tab="attach" ssh="none" />
            </div>
            <div style={{ display: "flex", flexDirection: "column", gap: 6 }}>
              <div style={{ fontSize: 11, color: "var(--tasty-text-muted)" }}>segmented — accent fill</div>
              <RemoteFormFrame variant="attach" />
            </div>
          </Stage>
          <Meta
            specs={[["tab strip", <>2px <span className="tok">--tasty-accent-primary</span> underline · weight 600</>], ["segmented", <>fill <span className="tok">--tasty-accent-primary</span> · ink <span className="tok">--tasty-text-on-accent</span></>], ["inactive segment", <span className="tok">--tasty-surface-raised</span>], ["scope", "every remote segment + clipboard type + preset scope"], ["not used", <><span className="tok">--tasty-surface-active</span> (row selection)</>]]}
            tokens={[{ tok: "--tasty-accent-primary", use: "active segment / underline", color: "var(--tasty-accent-primary)" }, { tok: "--tasty-text-on-accent", use: "active segment ink", color: "var(--tasty-text-on-accent)" }, { tok: "--tasty-surface-raised", use: "inactive segment", color: "var(--tasty-surface-raised)" }]} />
          <Note>Rule of thumb: an <b>underline</b> switches a <b>view</b>, a <b>fill</b> switches a <b>value</b>. The remote tab bar navigates; the attach switch sets a field.</Note>
        </Spec>

        <Spec title="From ssh config — a second, subordinate list"
          when={<>Hosts parsed out of <code>~/.ssh/config</code> sit <b>below the profiles, in the same scroll</b> — it is the same question (“which machine?”), and a separate tab would hide the answer. Everything about the section says <b>one tier down</b>: a section header instead of a card, the <b>source path in mono</b> so the origin is unambiguous, <b>two lines</b> per row instead of the profile row's three, secondary ink on the alias, and <b>no per-row icon buttons</b> — one ghost <b>Add profile</b>, which is the import action that already exists. An already-imported host shows a muted <b>in profiles</b> Tag and no action, so importing twice is not offered.</>}>
          <Stage variant="solo center" style={{ padding: 20, background: "var(--tasty-bg-app)", gap: 18, flexWrap: "wrap", alignItems: "flex-start" }}>
            <div style={{ display: "flex", flexDirection: "column", gap: 6 }}>
              <div style={{ fontSize: 11, color: "var(--tasty-text-muted)" }}>3 hosts · one already a profile · long alias</div>
              <RemoteFrame />
            </div>
            <div style={{ display: "flex", flexDirection: "column", gap: 10 }}>
              {[["no hosts", "empty"], ["unreadable config", "unreadable"]].map(([label, st]) => (
                <div key={st} style={{ display: "flex", flexDirection: "column", gap: 6, width: 300 }}>
                  <div style={{ fontSize: 11, color: "var(--tasty-text-muted)" }}>{label}</div>
                  <div style={{ padding: "8px 12px", background: "var(--tasty-bg-panel)", border: "1px solid var(--tasty-border-strong)", borderRadius: "var(--tasty-radius)" }}>
                    <LocalSshSection state={st} />
                  </div>
                </div>
              ))}
            </div>
          </Stage>
          <Meta
            specs={[["place", "below the profile list, same scroll"], ["header", <>11px uppercase label · <span className="tok">--tasty-font-mono</span> source path · count</>], ["separator", <>1px <span className="tok">--tasty-border-frame</span> above the section</>], ["row", "alias (13, secondary) + user@host:port (mono 11, muted)"], ["action", "ghost Add profile — the existing import, no new behaviour"], ["already imported", <>Tag <b>in profiles</b>, no action</>], ["empty / failure", "one muted line each, no error tone"], ["long alias", "ellipsises; the target line never wraps"]]}
            tokens={[{ tok: "--tasty-border-frame", use: "section rule", color: "var(--tasty-border-frame)" }, { tok: "--tasty-text-secondary", use: "alias + section label", color: "var(--tasty-text-secondary)" }, { tok: "--tasty-text-muted", use: "target · source path · states", color: "var(--tasty-text-muted)" }]} />
          <Dont><b>Don't</b> give these rows the profile row's edit / delete / re-detect buttons. The file is the user's own; tasty reads it and never writes it.</Dont>
          <Note>A missing or unreadable <code>~/.ssh/config</code> is <b>not an error</b> — one muted line, no warning tone, section header stays so the origin is still explained.</Note>
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

        <Spec title="Save mode — one confirm, in the footer"
          when={<>The same picker, asked to <b>produce</b> a path instead of read one (Settings → Export, and every future save target — <code>rfd</code> blocks the calling thread, so no OS dialog). <b>There is exactly one confirm control, and it is the footer's primary button</b> — the <b>File name</b> row the picker already owns becomes editable and the button reads <b>Save</b>. No second row is appended below the view, because a second button would point at a second target and the screen could hold two answers at once.<br /><br /><b>One target.</b> Selecting a list row does not confirm anything — it <b>writes that row's name into the input</b>, and the input is the only thing the button reads. Typing after a pick keeps the row selected only while the name still matches; edit a character and the selection clears. So "the picked file" and "the typed name" are never two different paths.<br /><br /><b>Overwrite is inline, not a second dialog.</b> When the typed name already exists in this folder, a warning line appears above the buttons and the primary <b>relabels to Overwrite</b> — the decision stays on the button the user is already reaching for. A modal confirm on top of a modal picker would be a second scrim for a reversible write.</>}>
          <Stage variant="solo center" style={{ padding: 20, background: "var(--tasty-bg-app)", gap: 16, flexWrap: "wrap", alignItems: "flex-start" }}>
            <div style={{ display: "flex", flexDirection: "column", gap: 6 }}><div style={{ fontSize: 11, color: "var(--tasty-text-muted)" }}>save — typed name (new file)</div><FilePickerFrame mode="save" /></div>
            <div style={{ display: "flex", flexDirection: "column", gap: 6 }}><div style={{ fontSize: 11, color: "var(--tasty-text-muted)" }}>save — existing file picked → Overwrite</div><FilePickerFrame mode="save" save="picked" /></div>
            <div style={{ display: "flex", flexDirection: "column", gap: 6 }}><div style={{ fontSize: 11, color: "var(--tasty-text-muted)" }}>save — name edited after the pick → selection cleared</div><FilePickerFrame mode="save" save="edited" /></div>
            <div style={{ display: "flex", flexDirection: "column", gap: 6 }}><div style={{ fontSize: 11, color: "var(--tasty-text-muted)" }}>deep path — middle-elided breadcrumb</div><FilePickerFrame mode="save" deep /></div>
          </Stage>
          <Meta
            specs={[["title", "Save file (open mode: Open file)"], ["confirm", "footer primary only — one control"], ["labels", "Save · Overwrite when the name exists"], ["input", "editable · placeholder “Type a file name”"], ["list pick", "writes the name into the input → Overwrite"], ["selection", "clears as soon as the name diverges → Save"], ["overwrite", "11px warning line above the buttons"], ["disabled", "Save disabled while the name is empty"], ["breadcrumb", "root + … + last two segments"], ["footer", "never shrinks — the input absorbs it"]]}
            tokens={[{ tok: "--tasty-accent-warning", use: "overwrite line", color: "var(--tasty-accent-warning)" }, { tok: "--tasty-input-bg", use: "name field", color: "var(--tasty-input-bg)" }, { tok: "--tasty-text-placeholder", use: "empty name", color: "var(--tasty-text-placeholder)" }, { tok: "--tasty-separator", use: "footer rule", color: "var(--tasty-separator)" }]} />
          <Note><b>Long-path shrink rule (fixes a defect in open mode too).</b> Overflow is absorbed <b>in the path bar</b>, never by the footer: the breadcrumb is the only flexible child (<code>flex:1; min-width:0</code>) and elides in the <b>middle</b> — root + <span className="ic">…</span> + the last two segments, since the current folder and its parent are what orient you; the <span className="ic">…</span> lists the hidden ancestors on click. Footer label, filter chip and both buttons are <code>flex:none</code>; only the name input shrinks. No horizontal scroll, and Cancel / Open / Save can never be clipped.</Note>
          <Dont><b>Don't</b> append a save row under the list. Two confirm controls in one dialog means two targets — pick <span className="ic">package.json</span>, then type a different name, and the screen holds two answers with no rule for which wins.</Dont>
        </Spec>

        <Spec title="Gestures &amp; folder targets — the six open branches"
          when={<>The save-mode decision left six branches undecided, and because <b>save and open are the same view</b>, each answer had to hold in both modes. One gesture table now covers them: <b>single click selects, double click descends</b> — in both modes, for both kinds. A <b>folder is never a save target</b>: the footer button still reads the input only, so a selected folder can't change what gets written; a muted line says so where the reading happens. In save mode a <b>file's double-click stops at select</b> — it writes the name, which <i>is</i> the overwrite state, and the warning line has to be readable before the write. Only open mode confirms on a file's double-click.</>}>
          <Stage variant="solo center" style={{ padding: 20, background: "var(--tasty-bg-app)", gap: 16, flexWrap: "wrap", alignItems: "flex-start" }}>
            <div style={{ display: "flex", flexDirection: "column", gap: 6 }}><div style={{ fontSize: 11, color: "var(--tasty-text-muted)" }}>save — folder selected · not a save target</div><FilePickerFrame mode="save" folderSel /></div>
            <div style={{ display: "flex", flexDirection: "column", gap: 6 }}><div style={{ fontSize: 11, color: "var(--tasty-text-muted)" }}>open — folder selected · Open enters it</div><FilePickerFrame folderSel /></div>
            <div style={{ display: "flex", flexDirection: "column", gap: 6 }}><div style={{ fontSize: 11, color: "var(--tasty-text-muted)" }}>… menu open — the hidden ancestors</div><FilePickerFrame mode="save" deep crumbMenu /></div>
          </Stage>
          <Meta
            specs={[["single click", "selects the row — file or folder, both modes"], ["double click folder", "descends — both modes"], ["double click file", "open: confirms · save: selects only"], ["folder + Save", "never a target — muted footer line, button unchanged"], ["folder + Open", "enters it (Open is the keyboard route to descend)"], ["elision", "overflow-driven — one ancestor at a time, no depth threshold"], ["… tooltip", "Show 3 hidden folders (singular: 1 hidden folder)"], ["… menu", "content-measured, 180–320 band, path order"]]}
            tokens={[{ tok: "--tasty-fp-crumb-max-width", use: "180 — one crumb's cap (NEW)" }, { tok: "--tasty-fp-crumb-menu-min-width", use: "180 — … menu floor (NEW)" }, { tok: "--tasty-fp-crumb-menu-max-width", use: "320 — … menu ceiling (NEW)" }, { tok: "--tasty-text-muted", use: "folder-target line", color: "var(--tasty-text-muted)" }, { tok: "--tasty-surface-raised", use: "… menu fill", color: "var(--tasty-surface-raised)" }]} />
          <Note><b>Elision is measured, not counted.</b> The path bar elides only when the crumb row doesn't fit, and it drops <b>one ancestor at a time</b> from the middle — a six-segment path that fits stays whole. The floor is root + <span className="ic">…</span> + the current folder; the parent is the first tail segment to go. Separately, a single crumb longer than <span className="tok">--tasty-fp-crumb-max-width</span> ellipsises inside itself — a different axis from the path's middle elision, which is why it has its own token.</Note>
          <Note><b>Open mode changes too</b> (same view, same table): folder single-click now <i>selects</i> instead of doing nothing, and <b>Open</b> with a folder selected descends — which is also what <span className="ic">Enter</span> does, so the keyboard route to "go into this folder" exists without a second control. File behaviour in open mode is unchanged.</Note>
        </Spec>

        <Spec title="Path bar — what gives way when the folded path still doesn't fit"
          when={<>Folding the middle (<code>root › … › parent › current</code>) is not always enough: at <b>400px</b> with 53-character folder names the current folder used to be <b>clipped without an ellipsis</b>. The bar now allocates the width it actually has — <b>the path bar minus the Refresh button and its gap</b>, never the popup width — in a fixed priority order: <b>current folder → parent → root → the … menu</b>. Five steps, each taken only when the one before it has hit its floor.</>}>
          <Stage variant="solo" style={{ padding: 20, background: "var(--tasty-bg-app)", flexDirection: "column", gap: 14, alignItems: "flex-start" }}>
            <div style={{ display: "flex", gap: 18, flexWrap: "wrap", alignItems: "flex-start" }}>
              <div style={{ display: "flex", flexDirection: "column", gap: 6 }}>
                <div style={{ fontSize: 11, color: "var(--tasty-text-muted)" }}>640×480 · two 53-char segments — parent shrinks to its 64 floor</div>
                <FilePickerFrame pathKind="longtwo" />
              </div>
              <div style={{ display: "flex", flexDirection: "column", gap: 6 }}>
                <div style={{ fontSize: 11, color: "var(--tasty-text-muted)" }}>400×360 · single-crumb fallback — … › current</div>
                <FilePickerFrame pathKind="longtwo" w={400} h={360} single />
              </div>
            </div>
            <div style={{ display: "flex", gap: 18, flexWrap: "wrap", alignItems: "flex-start" }}>
              <div style={{ display: "flex", flexDirection: "column", gap: 6 }}>
                <div style={{ fontSize: 11, color: "var(--tasty-text-muted)" }}>long UNC root, short current — the root gives way before the current folder</div>
                <FilePickerFrame pathKind="longroot" w={440} h={300} />
              </div>
              <div style={{ display: "flex", flexDirection: "column", gap: 6 }}>
                <div style={{ fontSize: 11, color: "var(--tasty-text-muted)" }}>save / overwrite · same rule, footer untouched</div>
                <FilePickerFrame pathKind="longtwo" mode="save" save="picked" w={400} h={360} single />
              </div>
            </div>
          </Stage>
          <Meta
            specs={[["measure", "path-bar width − trailing buttons − gap (not the popup width)"], ["1 fold", "ancestors → … menu, one per step (approved 09-14)"], ["2 parent", <>180 → 64 (<span className="tok">--tasty-fp-crumb-min-width</span>)</>], ["3 current", <>180 → 96 (<span className="tok">--tasty-fp-crumb-current-min-width</span>)</>], ["4 parent folds", "root › … › current"], ["5 root folds", "… › current — the design floor"], ["grow back", <>floor + 8 (<span className="tok">--tasty-fp-bar-hysteresis</span>)</>], ["current ellipsis", "at the FRONT (…-bbbb) — the tail names the folder"], ["ancestor ellipsis", "at the tail"], ["picker floor", <><span className="tok">--tasty-fp-popup-min-width</span> 320 — the owning surface always wins</>]]}
            tokens={[{ tok: "--tasty-fp-crumb-max-width", use: "cap (approved, unchanged)" }, { tok: "--tasty-fp-crumb-min-width", use: "ancestor floor" }, { tok: "--tasty-fp-crumb-current-min-width", use: "current-folder floor" }, { tok: "--tasty-fp-bar-hysteresis", use: "grow-back margin" }, { tok: "--tasty-fp-popup-min-width", use: "popup floor" }]} />
          <Do><b>Do</b> keep the <b>… menu</b> in place through every step: hidden ancestors enter it in <b>path order</b>, so step 4 and 5 just prepend the parent and the root. Its hit area never exceeds its painted box.</Do>
          <Dont><b>Don't</b> buy width from the footer, the file-name input, the Refresh button, or the font size. The path bar takes what is left over after those, and folds.</Dont>
          <Note>Same table at <b>ui_scale 0.85 / 1 / 1.2</b>: floors and caps are width tokens, so they scale with everything else and the order is unchanged. Windows drives, UNC roots and remote <code>user@host</code> roots are ordinary root crumbs — long ones fold at step 5, they get no exemption.</Note>
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
        <Spec title="Numbers in settings — one shape: mono Input + a static suffix"
          when={<>Three different numeric controls had appeared: a mono text Input with a static unit (remote transfer <b>Maximum size</b> · MiB), a <b>drag</b> number (plugin <b>Default zoom</b> · %), and a proposed stepper. The settled shape is the <b>first</b>, everywhere: a <b>mono text Input</b>, keyboard entry only, a <b>static muted suffix</b> outside the field, and <b>clamp on commit</b> (blur / <span className="ic">↵</span>) — not while typing, so you can type <code>150</code> in a 25–200 field without the second keystroke fighting you. Out of range shows the danger border + one inline line naming the range. A drag surface inside a scrolling settings pane steals the scroll and hides its own range; a stepper needs two more hit targets for a field people set once.</>}>
          <Stage variant="solo" style={{ padding: 20, background: "var(--tasty-bg-app)", flexDirection: "column", gap: 16, alignItems: "flex-start" }}>
            {[["default", "150", false], ["out of range — clamp on commit", "420", true], ["disabled", "100", false, true]].map(([label, val, bad, dis]) => (
              <div key={label} style={{ display: "flex", flexDirection: "column", gap: 6, width: 420 }}>
                <div style={{ fontSize: 11, color: "var(--tasty-text-muted)" }}>{label}</div>
                <div style={{ display: "flex", alignItems: "center", gap: 10 }}>
                  <span style={{ flex: 1, fontSize: 13, color: dis ? "var(--tasty-text-disabled)" : "var(--tasty-text-secondary)" }}>Default zoom</span>
                  <span style={{ flex: "none", width: "var(--tasty-field-width-xs)" }}>
                    <Input block defaultValue={val} disabled={dis} aria-invalid={bad || undefined}
                      style={{ fontFamily: "var(--tasty-font-mono)", textAlign: "right", ...(bad ? { borderColor: "var(--tasty-accent-danger)" } : null) }} />
                  </span>
                  <span style={{ flex: "none", width: 28, fontSize: 12, color: "var(--tasty-text-muted)" }}>%</span>
                </div>
                {bad && <div style={{ fontSize: 11, color: "var(--tasty-accent-danger)", paddingLeft: 2 }}>Between 25 and 200. Commits as 200.</div>}
              </div>
            ))}
          </Stage>
          <Meta
            specs={[["control", "existing Input — no new component"], ["font", <span className="tok">--tasty-font-mono</span>], ["align", "right — digits line up down a settings column"], ["width", <>90 (<span className="tok">--tasty-field-width-xs</span>) — <b>88 is dropped</b></>], ["suffix", "static text outside the field, muted"], ["clamp", "on commit (blur / ↵), never mid-typing"], ["out of range", "danger border + one inline line with the range"], ["scope", "every numeric field in Settings and in plugin settings"]]}
            tokens={[{ tok: "--tasty-field-width-xs", use: "numeric field width", }, { tok: "--tasty-accent-danger", use: "out-of-range edge + line", color: "var(--tasty-accent-danger)" }, { tok: "--tasty-text-muted", use: "unit suffix", color: "var(--tasty-text-muted)" }]} />
          <Note><b>Maximum size keeps 90.</b> The 88 in the earlier mock was a drawing accident, not a role — the field-width set stays 90 / 110 / 160 / 180 / 200 with no new member. Two-pixel gain, one more name to maintain: not worth it.</Note>
          <Dont><b>Don't</b> put a drag-to-change number inside a scrollable settings pane. The gesture collides with the scroll and the range stays invisible until you overshoot it.</Dont>
        </Spec>

        <Spec title="Hook Handlers — origin decides whether a row can be removed"
          when={<>The registry <b>re-seeds host and plugin defaults on every start</b>, so a remove button on those rows promises something the system undoes. Rows now carry their <b>origin</b> as a Tag — <b>host</b> / <b>you</b> / the <b>plugin</b> in mauve — and only <b>user</b> rows get the trash affordance. Host and plugin rows show a <b>lock glyph</b> in the same slot (not a disabled button: nothing is pending) with the tooltip “Provided by host — can't be removed”. <b>IpcSequence</b> stays a <b>one-line mono summary</b> with an <b>Edit</b> button that opens the sequence editor; editing a multi-step sequence inline inside a settings row has nowhere to put the steps.</>}>
          <Stage variant="solo center" style={{ padding: 20, background: "var(--tasty-bg-app)" }}>
            <div style={{ width: 520, background: "var(--tasty-bg-panel)", border: "1px solid var(--tasty-border-strong)", borderRadius: "var(--tasty-radius)", overflow: "hidden" }}>
              <div style={{ padding: "8px 12px", borderBottom: "1px solid var(--tasty-separator)", fontSize: 11, letterSpacing: ".06em", textTransform: "uppercase", color: "var(--tasty-text-secondary)" }}>Hook handlers</div>
              {[{ ev: "on_open", act: "open_markdown_preview", origin: "host" },
                { ev: "on_open", act: "run: code -g {path}:{line}", origin: "you" },
                { ev: "on_paste", act: "imgview.stash", origin: "dev.imgview" },
                { ev: "on_exit", act: "ipc: focus → save → close", origin: "you", seq: true }].map((r, i) => {
                const user = r.origin === "you";
                const plugin = r.origin !== "host" && !user;
                return (
                  <div key={i} style={{ display: "flex", alignItems: "center", gap: 10, padding: "8px 12px", borderBottom: "1px solid var(--tasty-separator)" }}>
                    <span style={{ flex: "none", width: 88, fontFamily: "var(--tasty-font-mono)", fontSize: 12, color: "var(--tasty-text-secondary)" }}>{r.ev}</span>
                    <span style={{ flex: 1, minWidth: 0, fontFamily: "var(--tasty-font-mono)", fontSize: 12, color: "var(--tasty-text-primary)", overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>{r.act}</span>
                    <span style={{ flex: "none", color: plugin ? "var(--tasty-accent-agent)" : undefined }}><WTag>{r.origin}</WTag></span>
                    {r.seq && <Button variant="ghost" size="sm">Edit</Button>}
                    <span style={{ flex: "none", width: 24, display: "inline-flex", justifyContent: "center" }}>
                      {user
                        ? <IconButton size="sm" aria-label="Remove"><WIcon name="trash" size={13} /></IconButton>
                        : <span title="Provided by host — can't be removed" style={{ display: "inline-flex", color: "var(--tasty-glyph-dim)" }}><WIcon name="lock" size={13} /></span>}
                    </span>
                  </div>
                );
              })}
            </div>
          </Stage>
          <Meta
            specs={[["origin", <>Tag: <b>host</b> · <b>you</b> · plugin id (<span className="tok">--tasty-accent-agent</span>)</>], ["remove", "user rows only"], ["not removable", <>lock glyph, <span className="tok">--tasty-glyph-dim</span>, with tooltip</>], ["not a disabled button", "nothing is pending — an affordance would lie"], ["IpcSequence", "mono one-line summary, steps joined by →"], ["sequence editing", "Edit → the sequence editor (no inline edit)"], ["registry", "unchanged — defaults re-seed on start"]]}
            tokens={[{ tok: "--tasty-glyph-dim", use: "lock glyph", color: "var(--tasty-glyph-dim)" }, { tok: "--tasty-accent-agent", use: "plugin origin", color: "var(--tasty-accent-agent)" }, { tok: "--tasty-font-mono", use: "event · action · sequence" }]} />
          <Note>The design's earlier “remove on every row” is dropped: the registry policy wins, and the lock is the honest reading of it.</Note>
        </Spec>
        <Spec title="Appearance › colour rows — the Default hex is read-only, not disabled (2026-09-29)"
          when={<>With <b>Default</b> checked, a colour row has no override and its hex field cannot be edited. The field still carries the <b>base value in use</b>, the only text value on the row, so it is <b>read-only</b>, not disabled: the same neutral box as a disabled Input, with the value in <span className="tok">--tasty-input-readonly-fg</span> (text-secondary) instead of the disabled ink. The value can be selected and copied; the field takes focus (1px focus edge, no ring). Unchecking Default starts the override and the field becomes a normal Input. Applies to the Tasty colour rows, the terminal surface background row and the Colors group.</>}>
          <Stage variant="solo" style={{ padding: "var(--tasty-space-lg)", background: "var(--tasty-bg-app)", gap: "var(--tasty-space-lg)", flexWrap: "wrap", alignItems: "flex-start" }}>
            {[["Mocha", null], ["Latte", "latte"]].map(([label, theme]) => (
              <div key={label} {...(theme ? { "data-theme": theme } : {})} style={{ width: "var(--tasty-size-360)", display: "flex", flexDirection: "column", gap: "var(--tasty-label-detail-gap)", padding: "var(--tasty-space-md)", background: "var(--tasty-bg-panel)", border: "var(--tasty-border-width) solid var(--tasty-border-default)", borderRadius: "var(--tasty-radius)" }}>
                <span style={{ fontSize: "var(--tasty-font-size-caption)", color: "var(--tasty-text-muted)", marginBottom: "var(--tasty-space-xs)" }}>{label}</span>
                {[["accent", "#89b4fa", null], ["surface_bg", "#1e1e2e", "#181825"], ["selection", "#45475a", null]].map(([field, base, ov]) => (
                  <div key={field} style={{ display: "flex", alignItems: "center", gap: "var(--tasty-space-sm)", minHeight: "var(--tasty-control-height)" }}>
                    <span style={{ width: "var(--tasty-status-dot-size)", height: "var(--tasty-status-dot-size)", borderRadius: "var(--tasty-radius-pill)", flex: "none", background: ov ? "var(--tasty-accent-primary)" : "transparent" }} />
                    <span style={{ flex: 1, fontFamily: "var(--tasty-font-mono)", fontSize: "var(--tasty-font-size-term-sm)", color: ov ? "var(--tasty-text-primary)" : "var(--tasty-text-muted)" }}>{field}</span>
                    <span style={{ flex: "none", display: "flex", width: "var(--tasty-field-width-xs)" }}><Input block mono readOnly={!ov} defaultValue={ov || base} /></span>
                    <span style={{ width: "var(--tasty-swatch-size)", height: "var(--tasty-swatch-size)", borderRadius: "var(--tasty-swatch-radius)", flex: "none", background: ov || base, opacity: ov ? 1 : "var(--tasty-state-dim-opacity)", border: "var(--tasty-border-width) solid var(--tasty-border-strong)" }} />
                    <WCheckbox label="Default" checked={!ov} />
                  </div>
                ))}
              </div>
            ))}
          </Stage>
          <Meta
            specs={[["Default hex", "Input readOnly"], ["box", "state-disabled fill + border (same as disabled)"], ["value ink", "text-secondary"], ["select · copy", "allowed; focusable"], ["override", "normal Input"], ["disabled", "reserved for an unavailable control"]]}
            tokens={[{ tok: "--tasty-input-readonly-bg", use: "→ state-disabled-fill", color: "var(--tasty-input-readonly-bg)" }, { tok: "--tasty-input-readonly-border", use: "→ state-disabled-border", color: "var(--tasty-input-readonly-border)" }, { tok: "--tasty-input-readonly-fg", use: "→ text-secondary", color: "var(--tasty-input-readonly-fg)" }]} />
        </Spec>
      </Section>

      <Section id="permissions" title="Settings › General › Permissions (macOS) — 2026-09-28">
        <Spec title="Status table, one action per row, one request button"
          when={<>macOS builds only, the last L2 under General. A three-column table: <b>permission</b> · <b>status</b> · <b>row action</b>. The four states are told apart by <b>glyph + word</b>, with colour as a third channel: <b>Granted</b> check / success, <b>Not granted</b> alertCircle / warning, <b>Unknown</b> helpCircle / muted, <b>Cannot check automatically</b> eyeOff / muted. Unknown (inference failed) and Cannot check (deliberately not looked at) share the muted ink but never the glyph, and neither can be misread as granted. The Full Disk Access shortcut moves <b>into its own row</b> as Secondary / Sm <b>[Open System Settings]</b>. What needs explaining per row now lives in HelpHints, so the two notes under the table are short. <b>[Request all permissions]</b> stays Primary / Md under the table. While requesting it is disabled and the line below becomes a spinner + <i>requesting</i> copy; the button carries no spinner. The debug-only Accessibility row carries a <b>debug</b> Tag.</>}>
          <Stage variant="solo" style={{ padding: "var(--tasty-space-lg)", background: "var(--tasty-bg-app)", gap: "var(--tasty-space-lg)", flexWrap: "wrap", alignItems: "flex-start" }}>
            {[["A · nothing granted — Mocha", "none", false, null], ["A · nothing granted — Latte", "none", false, "latte"], ["B · all granted", "all", false, null], ["C · FDA unknown, screen granted", "fdaUnknown", false, null], ["D · requesting", "requesting", false, null], ["E · debug build (4 rows)", "none", true, null], ["E · debug build — Latte", "all", true, "latte"]].map(([cap, sc, dbg, theme]) => (
              <div key={cap} {...(theme ? { "data-theme": theme } : {})} style={{ width: "var(--tasty-size-560)", display: "flex", flexDirection: "column", gap: "var(--tasty-space-xs)" }}>
                <span style={{ fontSize: "var(--tasty-font-size-caption)", color: "var(--tasty-text-muted)" }}>{cap}</span>
                <div style={{ padding: "var(--tasty-space-lg)", background: "var(--tasty-bg-panel)", border: "var(--tasty-border-width) solid var(--tasty-border-frame)", borderRadius: "var(--tasty-radius)" }}>
                  <window.TastyKit.MacPermissionsPane scenario={sc} debug={dbg} />
                </div>
              </div>
            ))}
          </Stage>
          <Meta
            specs={[["columns", "label (1fr) · status · row action"], ["row", "min 32 (settings row) · 1px border-default rule"], ["status", "glyph 14 + word · gap 4"], ["FDA action", "Secondary / Sm, in the FDA row"], ["primary", "Request all permissions · Primary / Md"], ["requesting", "button disabled · note line → spinner + copy"], ["notes", "caption 12 · text-muted · wrap at measure-xl"], ["narrow", "status + action wrap under each other, right-aligned; label never truncates"], ["debug row", "Tag \"debug\""]]}
            tokens={[{ tok: "--tasty-perm-granted-fg", use: "check", color: "var(--tasty-perm-granted-fg)" }, { tok: "--tasty-perm-missing-fg", use: "alertCircle", color: "var(--tasty-perm-missing-fg)" }, { tok: "--tasty-perm-unknown-fg", use: "helpCircle", color: "var(--tasty-perm-unknown-fg)" }, { tok: "--tasty-perm-unobservable-fg", use: "eyeOff", color: "var(--tasty-perm-unobservable-fg)" }, { tok: "--tasty-perm-row-height", use: "→ settings row 32" }]} />
          <Note><b>Copy changed</b> (en final, in the specimen): FDA button → "Open System Settings"; <i>detection_note</i> and <i>request_note</i> shortened, their per-row parts moved to the FDA and Folder access HelpHints; <i>requesting</i> shortened to two clauses. Status words unchanged.</Note>
          <Dont><b>Don't</b> paint Unknown or Cannot check as a blank or a dash. An empty status cell reads as "fine".</Dont>
        </Spec>
      </Section>

      <Section id="pluginswindow" title="Plugins window · plugin avatar">
        <Spec title="Plugin identity mark — one component, two sizes"
          when={<>A plugin manifest carries <b>no image</b>, so its identity in the Plugins window is a <b>square initial mark</b>: the first letter of the plugin name, mono, bold, in a tinted square at <span className="tok">--tasty-radius</span>. There are exactly <b>two sizes</b> — <b>sm (32)</b> on Installed / Attention list rows and <b>lg (46)</b> on a detail identity block and the <b>Add plugin</b> manifest preview (the old one-off 42px preview copy is gone; it was the same thing at different numbers). The tint is mixed into <span className="tok">--tasty-surface-raised</span>, a <b>fixed bed</b> — not the row background — so the mark is pixel-identical on a rest, hover and selected row. The initial's size is a <b>token per size</b>, not a ratio of the box: at lg it is <b>16px</b>, a sanctioned <b>mark</b> exception to the 14px UI cap (same class as the brand wordmark) because it is one glyph standing in for an icon, never running copy.</>}>
          <Stage variant="tight" grid>
            <div style={{ display: "flex", gap: 32, alignItems: "flex-start", flexWrap: "wrap", padding: 14, background: "var(--tasty-bg-panel)" }}>
              <div style={{ display: "flex", gap: 18, alignItems: "flex-end" }}>
                {[["sm", "32 · list row"], ["lg", "46 · detail / preview"]].map(([s, l]) => (
                  <div key={s} style={{ display: "flex", flexDirection: "column", alignItems: "center", gap: 8 }}>
                    <PluginAvatarG initial="G" size={s} />
                    <span style={{ fontSize: 11, color: "var(--tasty-text-muted)" }}>{l}</span>
                  </div>
                ))}
              </div>
              <div style={{ display: "flex", flexDirection: "column", gap: 4 }}>
                <PluginRowG name="git-helper" meta="tasty-labs · v1.4.2" state="rest" />
                <PluginRowG name="ai-review" meta="tasty-labs · v0.9.0" state="hover" />
                <PluginRowG name="docker" meta="community · v2.1.0" state="selected" />
                <PluginRowG name="vim-mode" meta="ophen · v3.2.1 · disabled" state="rest" disabled />
              </div>
            </div>
          </Stage>
          <Meta
            specs={[["sizes", "sm 32 · lg 46 — the whole roster"], ["content", "name initial, uppercase, mono bold"], ["initial size", "sm 14 · lg 16 (mark exception)"], ["tint bed", "surface-raised — fixed, not the row bg"], ["mix", "bg 18% · border 38% of accent-primary"], ["row states", "mark unchanged — only the row bg moves"], ["disabled row", "whole row at state-disabled-opacity"]]}
            tokens={[{ tok: "--tasty-plugin-avatar-size-sm", use: "32 — list row" }, { tok: "--tasty-plugin-avatar-size-lg", use: "46 — detail / manifest preview" }, { tok: "--tasty-plugin-avatar-bg", use: "tinted square", color: "var(--tasty-plugin-avatar-bg)" }, { tok: "--tasty-plugin-avatar-border", use: "1px edge", color: "var(--tasty-plugin-avatar-border)" }, { tok: "--tasty-plugin-avatar-fg", use: "the initial", color: "var(--tasty-plugin-avatar-fg)" }, { tok: "--tasty-plugin-avatar-initial-font-size-sm", use: "14 at sm" }, { tok: "--tasty-plugin-avatar-initial-font-size-lg", use: "16 at lg — mark exception to the UI cap" }, { tok: "--tasty-plugin-avatar-initial-weight", use: "bold" }, { tok: "--tasty-plugin-avatar-border-width", use: "= --tasty-border-width" }]} />
          <Note><b>The mark carries identity, not classification.</b> An earlier draft coloured it by a plugin <i>category</i>, but a manifest has no category field — every real install fell back to one colour, so the hue said nothing while implying a taxonomy. Colour is now fixed at <span className="tok">--tasty-plugin-avatar-fg</span> for every plugin; state (running / error / needs attention) is already carried by the row's status dot and callouts, which is where a reader looks for it. If a classification axis is ever wanted here, it needs a manifest field first — and then it is a new decision, not this token.</Note>
          <Dont><b>Don't</b> mix the tint into the row background to "blend" on a selected row — the mark would then shift colour with row state and stop being a stable identity. And don't re-derive the initial's size from the box (<code>round(size × 0.42)</code>): that produced 19px at lg, off the type scale and over the UI cap with no decision behind it.</Dont>
        </Spec>
      </Section>

      <Section id="kbimportexport" title="Keybindings · Import / Export">
        <Spec title="L2 placement & entry screen"
          when={<>A <b>single</b> L2 section named <b>Import / Export</b>, pinned <b>last</b> under Keybindings (after Plugins). It moves a configuration rather than editing bindings, so it gets a <b>separator above it</b> — a <b>NEW axis on the settings L2 row model</b> (today a row carries only a label + <code>is_plugin</code>); the separator is suppressed while the sidebar filter is active, since filtering breaks the adjacency it describes. The section's list position is <b>not a list</b>: two action rows, each with a one-line description and its own trailing button. <b>Import is primary, Export secondary</b> — import is the path with consequences and the one people come here for; export is a single fire-and-forget write.<br /><br />While we were here we also corrected the <b>Keybindings L2 roster</b> to the eleven real sections (<b>General · Workspace · Pane · Tab · Surface · Clipboard · Zoom · Explorer · Scripts · Preset · Plugins</b>) plus the new twelfth: the design carried a phantom <b>Image</b> section and was missing <b>Explorer</b> and <b>Scripts</b>.</>}>
          <Stage variant="tight" grid>
            <div style={{ display: "flex", gap: 24, alignItems: "flex-start", flexWrap: "wrap", padding: 14, background: "var(--tasty-bg-panel)" }}>
              <IeL2Tail />
              <IeEntry />
            </div>
          </Stage>
          <Meta
            specs={[["L2 items", "one — “Import / Export”"], ["position", "last, after Plugins"], ["separator", "1px above the row (new axis)"], ["filtering", "separator hidden while filtering"], ["entry", "2 action rows, not a ListCtrl"], ["weights", "Import primary · Export secondary"], ["export feedback", "the window's own toast, carrying the path"], ["file picker", "popup on the window's PopupManager"]]}
            tokens={[{ tok: "--tasty-separator", use: "L2 separator + row rules" }, { tok: "--tasty-surface-raised", use: "action row bed", color: "var(--tasty-surface-raised)" }, { tok: "--tasty-border-default", use: "action row edge", color: "var(--tasty-border-default)" }, { tok: "--tasty-settings-sidebar-width", use: "200 — L2 column" }]} />
          <Note><b>Why a popup for the file picker</b> and not another drill-down step: the drill-down is already spoken for by the preview (list ⇄ detail), and the same picker serves both directions — Export needs a save target, Import an open target. The settings window already runs a PopupManager for the shortcut-conflict confirm, so this adds a case, not a mechanism.</Note>
          <Dont><b>Don't</b> give export its own result screen. There is nothing to do after a write, so the confirmation is a <b>toast with the resolved path</b> and the user stays where they were.</Dont>
        </Spec>

        <Spec title="Import preview — group headers, select column, long-table handling"
          when={<>The detail view keeps the Preset skeleton (back bar with <b>Apply</b> in the right slot, over the diff grid) and grows two axes. <b>Group headers</b> (NEW on this table): one row spanning all columns, carrying a select-all box, the group name in mono micro caps, a <b>changed / total</b> count and a collapse chevron — it does <b>not</b> repeat Action / Current / Imported. Four groups: general combos, quick switch, script bindings, plugin overrides. A <b>select column</b> (32px, leading) makes apply <b>per row</b>; the group box is the group's select-all. The third column header is the fixed word <b>Imported</b> — the file name lives in the intro line, where it can be long without breaking the grid.<br /><br />Length is handled by <b>both</b> levers: the table opens <b>changed-only</b> (a <b>Show all 73</b> toggle in the back bar) and every group collapses. At minimum 73 rows — 100+ once slots are expanded — neither alone is enough. <b>Quick switch is summarised one row per axis</b>, not per slot: slots store raw keys and the combo is composed for display, so the axis row's value is the composed range (<span className="tok">Ctrl+1…0</span>) with the slot count as a sub-line. That keeps 29 slot rows out of the table and matches where the edit actually happens.</>}>
          <Stage variant="tight" grid>
            <div style={{ display: "flex", flexDirection: "column", gap: 12, alignItems: "flex-start", padding: 14, background: "var(--tasty-bg-panel)" }}>
              <IeBackBarG unresolved={0} />
              <IeGrid />
            </div>
          </Stage>
          <Meta
            specs={[["columns", "32px select · 1.6fr action · 1fr current · 1fr imported"], ["group header", "spans all columns, on surface-raised"], ["group content", "select-all · name (mono 10 caps) · counts · chevron"], ["groups", "general · quick switch · scripts · plugin overrides"], ["default view", "changed only"], ["toggle", "“Show all {n}” in the back bar"], ["quick switch", "1 row per axis (3), slot count as sub-line"], ["plugin row", "command name + agent-dot plugin name, two lines"], ["changed cell", "accent-primary, colour only (no bold)"]]}
            tokens={[{ tok: "--tasty-surface-raised", use: "group header bed", color: "var(--tasty-surface-raised)" }, { tok: "--tasty-accent-primary", use: "changed imported value", color: "var(--tasty-accent-primary)" }, { tok: "--tasty-accent-agent", use: "plugin dot", color: "var(--tasty-accent-agent)" }, { tok: "--tasty-separator", use: "cell rules" }, { tok: "--tasty-letter-spacing-caps", use: "header + group name tracking" }]} />
          <Note><b>Apply writes the draft, Save commits it</b> — the same two-stage contract as Preset, and the intro line says so. The two never share a row: Apply sits in the back bar, Save in the window footer.</Note>
        </Spec>

        <Spec title="Option migration — pending · resolved · conflict · unbound · not needed"
          when={<>A binding containing <b>option</b> never matches on non-macOS: it looks bound and does nothing. So a mac-made configuration cannot be applied here until every option-bearing binding is resolved, and the card that collects them <b>gates Apply</b>. It sits <b>above</b> the diff table, with the dropped-plugin notice under it — everything that changes the meaning of the table reads before the table.<br /><br />Two widget kinds share one row shape, because the code allows nothing else: a <b>combo slot is recorded</b> (min 140×24, mono, surface-raised — the existing binding-capture button), while a <b>quick-switch axis modifier can only be picked</b> from a Select (capture ignores modifier-only input; non-macOS offers 7 combos). Row height, label column and the <span className="tok">→</span> gutter are identical either way, so the list reads as one thing; the widget shape is the only tell, which is honest — one takes a keystroke, the other a choice. Changing an axis modifier recomposes every slot on that axis, so the row says so.<br /><br />States: <b>not set</b> (empty slot, “Not set”, warning-toned) · <b>set</b> (value + success check) · <b>conflict</b> (danger border + inline reason; the existing shortcut-conflict popup still opens on Apply) · <b>unbound</b> (“Leave unbound” — a deliberate discard that <b>counts as resolved</b>). When the target is macOS or the file has no option bindings the card is <b>absent</b> and the intro line says “No option bindings to migrate” — no empty card, no placeholder.</>}>
          <Stage variant="tight" grid>
            <div style={{ display: "flex", flexDirection: "column", gap: 14, alignItems: "flex-start", padding: 14, background: "var(--tasty-bg-panel)" }}>
              <IeBackBarG unresolved={2} />
              <IeMigrateG state="pending" />
              <IeMigrateG state="resolved" />
              <IeNotices />
            </div>
          </Stage>
          <Meta
            specs={[["position", "above the diff table; notice between"], ["gate", "Apply disabled while any row is unresolved"], ["counter", "“{n} of {m} unresolved” in the card header + back bar"], ["widget A", "record slot — min 140 × 24, mono"], ["widget B", "modifier Select — 7 combos (non-macOS)"], ["label column", "288px — ja longest label measures 255px"], ["axis fan-out", "sub-line: “10 slots change with it”"], ["unbound", "counts as resolved, shown as a Tag"], ["not needed", "card absent + one intro sentence"], ["failure", "inline block in the detail area"]]}
            tokens={[{ tok: "--tasty-accent-warning", use: "pending card + “Not set”", color: "var(--tasty-accent-warning)" }, { tok: "--tasty-accent-success", use: "resolved card + set check", color: "var(--tasty-accent-success)" }, { tok: "--tasty-accent-danger", use: "conflict border + parse failure", color: "var(--tasty-accent-danger)" }, { tok: "--tasty-surface-raised", use: "record slot bed", color: "var(--tasty-surface-raised)" }, { tok: "--tasty-text-disabled", use: "empty slot label", color: "var(--tasty-text-disabled)" }, { tok: "--tasty-size-24", use: "record slot height" }]} />
          <Note><b>Two disabled Applies, two reasons.</b> Preset shows a disabled button relabelled <b>Applied</b> (nothing left to do). Here the label stays <b>Apply</b> and the reason is carried next to it as <b>“{"{n}"} unresolved”</b> plus the card counter — a disabled button whose cause is off-screen is a dead end, and relabelling would claim the import already happened. Same disabled treatment, different message.</Note>
          <Dont><b>Don't</b> make “dropped plugin overrides” a warning callout. Nothing is wrong and there is no action — a warning triangle on an unactionable fact trains people to ignore triangles. It is one muted info line naming the plugins.</Dont>
        </Spec>

        <Spec title="The six open values — failure, notices, conflicts, placeholder, tokens"
          when={<>Six things the first pass left blank. The rule behind all of them: <b>a result with nothing to do is a toast; a result with something to do is inline, where the thing to do lives.</b><br /><br /><b>1 · Export failure</b> — success stays a toast carrying the resolved path. Failure is an inline danger block <b>inside the Export row</b> (the row that started it), with <b>Try again</b> and <b>Choose another location…</b>; the row's button goes disabled while the block is up, so there is one live retry, not two.<br /><br /><b>2 · Other bundle warnings</b> — the dropped-override <b>info line</b> stays exactly as it is (muted, no tone, no glyph weight): nothing is wrong and there is nothing to do. Everything that <i>is</i> a warning — newer schema tag, unknown actions, empty groups — collects in <b>one warning block</b>, one line per notice, count in the header, in that fixed order. Three lines show; the rest fold behind <b>Show {"{n}"} more</b>. Never a block per notice.<br /><br /><b>3 · Parsing failure with no line number</b> — the line clause is <b>replaced, not dropped</b>: the middle sentence always says why the file failed. With a position: “parsing stopped at line 1.” Without: “the file isn't TOML.” The first and last sentences never change, so the two read as one message.<br /><br /><b>4 · Several conflicts</b> — <b>count first</b>, in the card intro: “<b>3 conflicts</b> — those shortcuts are already bound.” The per-row inline reason stays on every row (it names <i>which</i> binding), so the summary never repeats the list and nothing needs collapsing. The summary line appears from <b>2</b> up; at 1 the row line alone carries it.<br /><br /><b>5 · Modifier Select placeholder</b> — <b>Select a modifier</b>, sentence case, UI font (not mono — it is not a key), <span className="tok">--tasty-text-placeholder</span>. It is a sentinel first option that leaves the list once a real combo is chosen, and the row keeps its warning-toned <b>Not set</b> trailer. Not an em-dashed pseudo-value like “— pick a modifier —”, which reads as a choice.<br /><br /><b>6 · Tokens — opened.</b> The raw <span className="tok">--tasty-size-*</span> reads in the spec jsx were a tier violation, so they now have names (below). The off-grid <b>14px</b> card inset was a slip: it snaps to <span className="tok">--tasty-space-md</span> (12). Drop the quoted constants and the raw-dimension ratchet and read the tokens.</>}>
          <Stage variant="tight" grid>
            <div style={{ display: "flex", flexDirection: "column", gap: 14, alignItems: "flex-start", padding: 14, background: "var(--tasty-bg-panel)" }}>
              <IeExportFailG />
              <IeBundleNoticesG />
              <IeParseFailG />
              <IeParseFailG line={false} />
              <IeConflictSummaryG />
              <div style={{ display: "flex", flexDirection: "column", gap: 8, padding: "var(--tasty-kb-ie-notice-inset)", borderRadius: "var(--tasty-radius)",
                background: "var(--tasty-surface-raised)", border: "1px solid var(--tasty-border-default)" }}>
                <IeModifierSelectG />
                <IeModifierSelectG chosen />
              </div>
            </div>
          </Stage>
          <Meta
            specs={[["export success", "toast with the resolved path"], ["export failure", "inline danger block in the Export row"], ["failure actions", "Try again · Choose another location…"], ["info line", "dropped overrides — muted, unchanged"], ["warning block", "one block, 1 line per notice, count in header"], ["notice order", "schema → unknown actions → empty groups"], ["fold", "3 lines, then “Show {n} more”"], ["parse copy", "line clause replaced by “the file isn't TOML.”"], ["conflicts", "count-first line from 2 up; row lines always"], ["placeholder", "“Select a modifier” · text-placeholder"]]}
            tokens={[{ tok: "--tasty-settings-content-max-width", use: "620 — content column cap (NEW)" }, { tok: "--tasty-kb-ie-select-column-width", use: "32 — diff select column (NEW)" }, { tok: "--tasty-kb-ie-action-column-width", use: "288 — action label column + sub-line indent (NEW)" }, { tok: "--tasty-kb-ie-from-column-width", use: "120 — original shortcut column (NEW)" }, { tok: "--tasty-kb-ie-slot-height", use: "24 — replacement slot chip (NEW)" }, { tok: "--tasty-kb-ie-slot-min-width", use: "140 — slot chip min width (NEW)" }, { tok: "--tasty-kb-ie-notice-inset", use: "12 — notice / card inset, both axes (NEW, was off-grid 14)" }, { tok: "--tasty-accent-warning", use: "warning block + Not set", color: "var(--tasty-accent-warning)" }, { tok: "--tasty-accent-danger", use: "failure blocks + conflict count", color: "var(--tasty-accent-danger)" }]} />
          <Note><b>Why the notices are one block and the info line is not in it.</b> Tone is the sorting key, not topic: a reader scans for "is anything wrong". Merging an unactionable fact into a warning-toned block makes the whole block unactionable-looking; splitting the four warnings into four blocks makes a wall where one thing is needed — read it, then go look at the table.</Note>
          <Dont><b>Don't</b> toast an export failure. A toast auto-dismisses and carries no retry, so the one case where the user must act is the one case that disappears on its own.</Dont>
        </Spec>

        <Spec title="Open values — unknown failure reason, counts of one, and the 620 cap"
          when={<>Three blanks left by the first round. <b>An export failure with no known cause</b> keeps the block exactly as decided — only the middle clause changes, from a fixed set (<span className="ic">read-only</span> · <span className="ic">permission</span> · <span className="ic">disk full</span> · catch-all <b>the write didn't finish.</b>), and the OS string is never spliced into the sentence: it gets its own muted mono line below, one line, ellipsised, full text in <span className="tok">title</span>. <b>Counts of one</b> get singular forms (<span className="ic">1 notice</span>, <span className="ic">1 unknown action was skipped</span>) — English only; ko/ja don't inflect.</>}>
          <Stage variant="tight" grid>
            <div style={{ display: "flex", flexDirection: "column", gap: 14, alignItems: "flex-start", padding: 14, background: "var(--tasty-bg-panel)" }}>
              <IeExportFailG reason="other" />
              <IeBundleNoticesG one />
            </div>
          </Stage>
          <Meta
            specs={[["clause set", "read-only · permission · disk full · the write didn't finish."], ["OS text", "own line, mono 11, muted — never inside the sentence"], ["first / last sentence", "never change (same as parse failure)"], ["singular", "1 notice · 1 unknown action was skipped (…)"], ["fold link", "absent at 1 notice — nothing to fold"], ["620 cap", "the settings scroll column, every non-full-bleed subtab"], ["prose", "keeps --tasty-measure-md (reading measure, narrower)"]]}
            tokens={[{ tok: "--tasty-settings-content-max-width", use: "620 — now set once on the settings content column" }, { tok: "--tasty-text-muted", use: "OS reason line", color: "var(--tasty-text-muted)" }, { tok: "--tasty-accent-danger", use: "failure block", color: "var(--tasty-accent-danger)" }]} />
          <Note><b>Where the 620 cap lives.</b> On the <b>settings content column itself</b> — the scrolling pane inside the Settings window — so every non-full-bleed L2 subtab inherits it, UI-kit subtabs included, and no block carries its own width. <b>Full-bleed subtabs are exempt</b>: they replace the column with their own layout (diff tables, editors). Body prose keeps <span className="tok">--tasty-measure-md</span> — a line-length measure, a different axis from the column cap and narrower than it.</Note>
          <Dont><b>Don't</b> build the OS message into the sentence (“… — os error 28: No space left on device.”). It breaks the sentence's grammar in three languages and buries the two parts the user can act on: which path, and that nothing was written.</Dont>
        </Spec>
      </Section>

      <Section id="scripts" title="Misc · Scripts (Lua script manager)">
        <Spec title="Scripts subsection — list, states & empty"
          when={<>Settings › <b>Misc</b> › <b>Scripts</b>. A subsection (not a separate popup) that manages user <b>Lua scripts</b> run by a shortcut. Each <b>ScriptRow</b>: the display name, the absolute path (<b>middle-elided</b> — dir tail truncates, filename always shown), a bound-shortcut <span className="ic">Kbd</span> badge (or italic <b>Unbound</b>), a peach <b>changed</b> badge + help line when the file's SHA no longer matches the one recorded at registration (TOFU re-confirm on next run), and an <b>Auto-run</b> row: mono <b>trigger chips</b> for the host-lifecycle events the script fires on (click a chip to remove it) plus a dashed <b>Add trigger…</b> control offering the remaining events. Row actions: bind shortcut (→ Keybindings), rename (inline), remove (inline confirm).</>}>
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
        <Spec title="Diff toolbar — a container height, not a button height"
          when={<>The diff toolbar is <b>32px</b> tall and holds 28px controls with 2px of air above and below. It now has its own name — <span className="tok">--tasty-git-toolbar-height</span> — rather than borrowing <span className="tok">--tasty-control-height</span>: the two are different roles and, as it happens, different numbers (32 vs 28), so aliasing them would have been wrong in value as well as in meaning. The value does not change.</>}>
          <Stage variant="solo center" style={{ padding: 20, background: "var(--tasty-bg-app)" }}>
            <div style={{ width: 460, background: "var(--tasty-bg-panel)", border: "1px solid var(--tasty-border-strong)", borderRadius: "var(--tasty-radius)", overflow: "hidden" }}>
              <div style={{ height: "var(--tasty-git-toolbar-height)", display: "flex", alignItems: "center", gap: 6, padding: "0 8px",
                background: "var(--tasty-bg-sidebar)", borderBottom: "1px solid var(--tasty-separator)" }}>
                <Button variant="ghost" size="sm">Unified</Button>
                <Button variant="ghost" size="sm">Split</Button>
                <div style={{ flex: 1 }} />
                <span style={{ fontFamily: "var(--tasty-font-mono)", fontSize: 11, color: "var(--tasty-text-muted)" }}>+128 −44</span>
              </div>
              <div style={{ padding: "10px 12px", fontFamily: "var(--tasty-font-mono)", fontSize: 11, color: "var(--tasty-text-muted)", lineHeight: 1.7 }}>
                <div>@@ -12,7 +12,9 @@ fn draw_path_bar()</div>
                <div style={{ color: "var(--tasty-accent-success)" }}>+    let avail = bar_w - buttons_w - gap;</div>
                <div style={{ color: "var(--tasty-accent-danger)" }}>-    let avail = popup_w;</div>
              </div>
            </div>
          </Stage>
          <Meta
            specs={[["height", "32 — unchanged"], ["token", <span className="tok">--tasty-git-toolbar-height</span>], ["holds", "28px controls + 2px air"], ["not", <><span className="tok">--tasty-control-height</span> (28, a control's own box)</>]]}
            tokens={[{ tok: "--tasty-git-toolbar-height", use: "toolbar container" }, { tok: "--tasty-control-height", use: "the buttons inside it" }]} />
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
