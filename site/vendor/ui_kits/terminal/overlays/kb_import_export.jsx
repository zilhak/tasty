// Tasty UI kit — Settings › Keybindings › Import / Export.
// Answers design-request/keybinding-import-export.md. Borrows the Preset
// drill-down skeleton (list-position entry → detail with a back bar whose RIGHT
// slot holds Apply, over a diff grid) and adds three things Preset does not have:
//   • GROUP HEADERS in the diff grid (4 groups: general combos / quick-switch /
//     script bindings / plugin overrides) — a NEW axis on that table.
//   • A per-row SELECT column (partial apply — user decision §6-2).
//   • An OPTION MIGRATION card that GATES Apply: `option` never matches on
//     non-macOS, so every option-bearing binding must be resolved first.
// Apply writes the draft; the window footer's Save commits it (same 2-stage
// contract as Preset). Rendered full-bleed — it owns its own scroll.
const { Button, IconButton, Input, Select, Tag, Checkbox, DrillDown } = window.TastyDesignSystem_41fd3f;
const { Icon } = window.TastyKit;

const IE_FILE = "tasty-keybindings-2026-09-09.toml";

const IE_GROUPS = [
  { id: "general", label: "General bindings", rows: [
    { action: "Copy", cur: "Ctrl+Shift+C", next: "Ctrl+Shift+C" },
    { action: "Paste", cur: "Ctrl+Shift+V", next: "Ctrl+Shift+V" },
    { action: "Command palette", cur: "Ctrl+K", next: "Ctrl+Shift+P" },
    { action: "Split vertical", cur: "Ctrl+D", next: "Ctrl+Alt+D" },
    { action: "Screenshot to clipboard", cur: "None", next: "Ctrl+Shift+4" },
    { action: "Find", cur: "Ctrl+F", next: "Ctrl+F" },
  ] },
  // §6-5: ONE ROW PER AXIS, not per slot. Slots store raw keys; the combo is
  // composed at display time, so an axis row's value is the composed range.
  { id: "quickswitch", label: "Quick switch (axis summary)", rows: [
    { action: "Tab axis", cur: "Alt+1…0", next: "Ctrl+1…0", note: "10 slots follow this axis" },
    { action: "Workspace axis", cur: "Alt+Shift+1…9", next: "Alt+Shift+1…9", note: "9 slots" },
    { action: "Category axis", cur: "Option+1…0", next: "— unresolved —", note: "10 slots · needs migration", blocked: true },
  ] },
  { id: "scripts", label: "Script bindings", rows: [
    { action: "deploy-staging.lua", cur: "Ctrl+Alt+1", next: "Ctrl+Alt+1" },
    { action: "rotate-logs.lua", cur: "None", next: "Ctrl+Alt+2" },
  ] },
  { id: "plugins", label: "Plugin overrides", rows: [
    { action: "open panel", plugin: "git-helper", cur: "Ctrl+Alt+G", next: "Ctrl+Alt+G" },
    { action: "review staged", plugin: "ai-review", cur: "Ctrl+Alt+R", next: "Ctrl+Shift+R" },
  ] },
];

// Option-bearing bindings that cannot match on this OS. Two widget kinds:
// "record" (a capture slot) and "modifier" (a Select — capture ignores
// modifier-only input, so an axis modifier can only be PICKED).
const MODIFIER_COMBOS = ["Ctrl", "Alt", "Shift", "Ctrl+Alt", "Ctrl+Shift", "Alt+Shift", "Ctrl+Alt+Shift"];
// Sentinel first option — a modifier Select must be able to read "not chosen
// yet"; without it the first real combo would look like an answer.
const IE_PICK = "— pick a modifier —";
const IE_MIGRATE = [
  { id: "m1", action: "Screenshot to clipboard", from: "Option+Shift+4", kind: "record", value: "Ctrl+Shift+4" },
  { id: "m2", action: "Category axis modifier", from: "Option", kind: "modifier", value: "", fanout: "10 slots on this axis change with it" },
  { id: "m3", action: "Toggle vi mode", from: "Option+V", kind: "record", value: "Ctrl+Shift+C", conflict: "Also bound to Copy" },
  { id: "m4", action: "Jump to error", from: "Option+E", kind: "record", value: "" },
];

const IE_DISCARDED = ["k8s-lens", "s3-browser"];

const mono = { fontFamily: "var(--tasty-font-mono)", fontSize: 12 };
const microCaps = { fontFamily: "var(--tasty-font-mono)", fontSize: 10, textTransform: "uppercase",
  letterSpacing: "var(--tasty-letter-spacing-caps)" };

// ── Diff grid — 4 columns (select · action · current · imported) ─────────
function IeDiffTable({ groups, changedOnly, collapsed, onToggleGroup, sel, onSel }) {
  const head = { ...microCaps, color: "var(--tasty-text-muted)",
    padding: "0 var(--tasty-space-md) var(--tasty-space-sm)", borderBottom: "var(--tasty-border-width) solid var(--tasty-separator)" };
  const cell = { padding: "var(--tasty-space-sm) var(--tasty-space-md)", borderBottom: "var(--tasty-border-width) solid var(--tasty-separator)",
    fontSize: 13, display: "flex", alignItems: "center" };
  return (
    <div style={{ display: "grid", gridTemplateColumns: "var(--tasty-size-32) minmax(0,1.6fr) 1fr 1fr", alignItems: "stretch" }}>
      <div style={{ ...head }} />
      <div style={{ ...head, textAlign: "left" }}>Action</div>
      <div style={{ ...head }}>Current</div>
      <div style={{ ...head }}>Imported</div>
      {groups.map((g) => {
        const rows = changedOnly ? g.rows.filter((r) => r.cur !== r.next) : g.rows;
        const isOpen = !collapsed[g.id];
        const on = rows.length > 0 && rows.every((r) => sel[g.id + r.action] !== false);
        return (
          <React.Fragment key={g.id}>
            {/* GROUP HEADER — a NEW axis on this table. One row spanning all four
                columns: select-all box, group name, counts, collapse chevron.
                It does NOT repeat the column headers. */}
            <div style={{ gridColumn: "1 / -1", display: "flex", alignItems: "center", gap: "var(--tasty-space-sm)",
              padding: "var(--tasty-space-sm) var(--tasty-space-md)", background: "var(--tasty-surface-raised)",
              borderBottom: "var(--tasty-border-width) solid var(--tasty-separator)" }}>
              <Checkbox checked={on} onChange={() => onSel(g, !on)} aria-label={"Select all in " + g.label} />
              <button type="button" onClick={() => onToggleGroup(g.id)} style={{ border: 0, background: "transparent", cursor: "pointer",
                display: "flex", alignItems: "center", gap: 6, padding: 0 }}>
                <span style={{ display: "inline-flex", color: "var(--tasty-text-muted)" }}>
                  <Icon name={isOpen ? "chevronDown" : "chevronRight"} size={14} />
                </span>
                <span style={{ ...microCaps, color: "var(--tasty-text-secondary)" }}>{g.label}</span>
              </button>
              <span style={{ ...mono, fontSize: 11, color: "var(--tasty-text-muted)" }}>
                {g.rows.filter((r) => r.cur !== r.next).length} changed · {g.rows.length} total
              </span>
            </div>
            {isOpen && rows.map((r) => {
              const changed = r.cur !== r.next;
              const key = g.id + r.action;
              return (
                <React.Fragment key={key}>
                  <div style={{ ...cell, justifyContent: "center" }}>
                    <Checkbox checked={sel[key] !== false} onChange={() => onSel(g, sel[key] === false, r)}
                      aria-label={"Apply " + r.action} />
                  </div>
                  <div style={{ ...cell, color: "var(--tasty-text-secondary)", flexDirection: "column", alignItems: "flex-start", gap: 2 }}>
                    <span>{r.action}</span>
                    {r.plugin && (
                      <span style={{ ...mono, fontSize: 10, color: "var(--tasty-text-muted)", display: "flex", alignItems: "center", gap: 5 }}>
                        <span style={{ width: "var(--tasty-status-dot-size)", height: "var(--tasty-status-dot-size)", borderRadius: "50%",
                          background: "var(--tasty-accent-agent)", flex: "none" }} />{r.plugin}
                      </span>
                    )}
                    {r.note && <span style={{ fontSize: 10, color: "var(--tasty-text-muted)" }}>{r.note}</span>}
                  </div>
                  <div style={{ ...cell, ...mono, color: "var(--tasty-text-muted)" }}>{r.cur}</div>
                  <div style={{ ...cell, ...mono,
                    color: r.blocked ? "var(--tasty-accent-warning)" : changed ? "var(--tasty-accent-primary)" : "var(--tasty-text-muted)" }}>{r.next}</div>
                </React.Fragment>
              );
            })}
          </React.Fragment>
        );
      })}
    </div>
  );
}

// ── Migration row — record slot OR modifier select, one shared row shape ──
function IeMigrateRow({ row, onSet }) {
  const state = row.conflict ? "conflict" : row.unbound ? "unbound" : row.value ? "set" : "unset";
  const tone = state === "conflict" ? "var(--tasty-accent-danger)"
    : state === "unset" ? "var(--tasty-accent-warning)" : "var(--tasty-accent-success)";
  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 4, padding: "var(--tasty-space-sm) 0",
      borderTop: "var(--tasty-border-width) solid var(--tasty-separator)" }}>
      <div style={{ display: "flex", alignItems: "center", gap: "var(--tasty-space-md)", minHeight: "var(--tasty-size-28)", flexWrap: "wrap" }}>
        {/* label column fixed at 288 — the ja longest action label measures 255px */}
        <span style={{ width: "var(--tasty-size-288)", flex: "none", fontSize: 13, color: "var(--tasty-text-secondary)",
          overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>{row.action}</span>
        <span style={{ ...mono, width: "var(--tasty-size-120)", flex: "none", color: "var(--tasty-text-muted)" }}>{row.from}</span>
        <span style={{ display: "inline-flex", flex: "none", color: "var(--tasty-text-muted)" }}><Icon name="chevronRight" size={14} /></span>
        {/* widget kind 2 — an axis modifier can only be PICKED (capture ignores
            modifier-only input); kind 1 — a combo is RECORDED. */}
        {row.kind === "modifier" ? (
          <Select options={row.value ? MODIFIER_COMBOS : [IE_PICK, ...MODIFIER_COMBOS]} value={row.value || IE_PICK}
            onChange={(e) => onSet(row.id, e.target.value === IE_PICK ? "" : e.target.value)}
            style={{ width: "var(--tasty-field-width-md)" }} />
        ) : (
          <button type="button" onClick={() => onSet(row.id, row.value ? "" : "Ctrl+Shift+9")} style={{ minWidth: 140,
            height: "var(--tasty-size-24)", padding: "0 var(--tasty-space-sm)", cursor: "pointer", ...mono,
            background: "var(--tasty-surface-raised)", color: row.value ? "var(--tasty-text-primary)" : "var(--tasty-text-disabled)",
            border: "var(--tasty-border-width) solid " + (state === "conflict" ? "var(--tasty-accent-danger)" : "var(--tasty-border-default)"),
            borderRadius: "var(--tasty-radius)", textAlign: "left" }}>
            {row.unbound ? "Unbound" : row.value || "Not set"}
          </button>
        )}
        {state === "set" && <span style={{ display: "inline-flex", color: tone }}><Icon name="check" size={14} /></span>}
        {state === "unbound" && <Tag>Unbound — counts as resolved</Tag>}
        {state === "unset" && (
          <>
            <span style={{ fontSize: 11, color: "var(--tasty-accent-warning)" }}>Not set</span>
            <Button variant="ghost" size="sm" onClick={() => onSet(row.id, "__unbound")}>Leave unbound</Button>
          </>
        )}
      </div>
      {row.conflict && (
        <div style={{ display: "flex", alignItems: "center", gap: 6, paddingLeft: "var(--tasty-size-288)", fontSize: 11, color: "var(--tasty-accent-danger)" }}>
          <Icon name="alertTriangle" size={14} /><span>{row.conflict} — the shortcut-conflict popup opens on Apply.</span>
        </div>
      )}
      {row.fanout && !row.conflict && (
        <div style={{ paddingLeft: "var(--tasty-size-288)", fontSize: 11, color: "var(--tasty-text-muted)" }}>{row.fanout}</div>
      )}
    </div>
  );
}

function IeMigrateCard({ rows, onSet }) {
  const left = rows.filter((r) => !r.value && !r.unbound).length;
  const done = left === 0;
  const tone = done ? "var(--tasty-accent-success)" : "var(--tasty-accent-warning)";
  return (
    <div style={{ borderRadius: "var(--tasty-radius)", padding: "var(--tasty-space-md) var(--tasty-size-14)",
      background: "color-mix(in srgb, " + tone + " 11%, transparent)",
      border: "var(--tasty-border-width) solid color-mix(in srgb, " + tone + " 36%, transparent)" }}>
      <div style={{ display: "flex", alignItems: "center", gap: "var(--tasty-space-sm)", color: tone, fontSize: 13, fontWeight: 600 }}>
        <Icon name={done ? "check" : "alertTriangle"} size={16} />
        <span>{done ? "Option bindings resolved" : "Option bindings need a replacement"}</span>
        <span style={{ marginLeft: "auto", ...mono, fontSize: 11, color: tone }}>
          {done ? rows.length + " of " + rows.length + " resolved" : left + " of " + rows.length + " unresolved"}
        </span>
      </div>
      <p style={{ margin: "4px 0 0", fontSize: 12, color: "var(--tasty-text-secondary)", lineHeight: "var(--tasty-line-height-ui)",
        maxWidth: "var(--tasty-measure-lg)" }}>
        {done
          ? <>Every option-bearing binding now has a replacement or is left unbound. <b>Apply</b> is enabled.</>
          : <><span style={mono}>option</span> never matches on this OS — these bindings would look bound and do nothing.
            Give each one a replacement, or leave it unbound. <b>Apply</b> stays disabled until none are left.</>}
      </p>
      <div style={{ marginTop: "var(--tasty-space-sm)" }}>
        {rows.map((r) => <IeMigrateRow key={r.id} row={r} onSet={onSet} />)}
      </div>
    </div>
  );
}

// ── Entry (list position) — two actions, not a list ──────────────────────
function IeActionRow({ glyph, title, desc, action }) {
  return (
    <div style={{ display: "flex", alignItems: "flex-start", gap: "var(--tasty-space-md)",
      padding: "var(--tasty-space-md) var(--tasty-size-14)", borderRadius: "var(--tasty-radius)",
      background: "var(--tasty-surface-raised)", border: "var(--tasty-border-width) solid var(--tasty-border-default)" }}>
      <span style={{ display: "inline-flex", flex: "none", marginTop: 2, color: "var(--tasty-text-muted)" }}><Icon name={glyph} size={16} /></span>
      <div style={{ flex: 1, minWidth: 0 }}>
        <div style={{ fontSize: 13, color: "var(--tasty-text-primary)" }}>{title}</div>
        <p style={{ margin: "2px 0 0", fontSize: 12, color: "var(--tasty-text-muted)", lineHeight: "var(--tasty-line-height-ui)",
          maxWidth: "var(--tasty-measure-md)" }}>{desc}</p>
      </div>
      <span style={{ flex: "none" }}>{action}</span>
    </div>
  );
}

// A file picker opens as a POPUP on the settings window's own PopupManager
// (it already runs one for the shortcut-conflict confirm) — not a DrillDown
// step, because the drill-down is already spoken for by the preview, and the
// same picker serves both Export (save target) and Import (open).
function IePickerPopup({ mode, onClose, onPick }) {
  return (
    <div style={{ position: "absolute", inset: 0, display: "flex", alignItems: "center", justifyContent: "center",
      background: "var(--tasty-scrim-bg)", zIndex: 3 }}>
      <div style={{ width: 420, background: "var(--tasty-bg-panel)", borderRadius: "var(--tasty-radius)",
        border: "var(--tasty-border-width) solid var(--tasty-border-strong)", boxShadow: "var(--tasty-shadow-modal)", overflow: "hidden" }}>
        <div style={{ display: "flex", alignItems: "center", height: "var(--tasty-titlebar-height)", padding: "0 var(--tasty-space-sm) 0 var(--tasty-space-md)",
          background: "var(--tasty-bg-app)", borderBottom: "var(--tasty-border-width) solid var(--tasty-separator)" }}>
          <span style={{ fontSize: 12, color: "var(--tasty-titlebar-fg)" }}>{mode === "export" ? "Export to…" : "Open keybinding file"}</span>
          <span style={{ marginLeft: "auto" }}><IconButton size="sm" aria-label="Close" onClick={onClose}><Icon name="close" size={16} /></IconButton></span>
        </div>
        <div style={{ padding: "var(--tasty-space-md)", display: "flex", flexDirection: "column", gap: "var(--tasty-space-sm)" }}>
          <p style={{ margin: 0, fontSize: 12, color: "var(--tasty-text-muted)", lineHeight: "var(--tasty-line-height-ui)" }}>
            The in-app file picker view mounts here — its own design is unchanged; only this placement is new.
          </p>
          <Input block value={"~/tasty/" + IE_FILE} readOnly />
          <div style={{ display: "flex", justifyContent: "flex-end", gap: 8 }}>
            <Button variant="ghost" size="sm" onClick={onClose}>Cancel</Button>
            <Button variant="primary" size="sm" onClick={onPick}>{mode === "export" ? "Save" : "Open"}</Button>
          </div>
        </div>
      </div>
    </div>
  );
}

// `variant` drives the specimen states: "ready" (entry), "pending" (migration
// unresolved), "resolved", "nomigration", "failed".
function KbImportExportSubtab({ variant = "ready", onFlash }) {
  const [view, setView] = React.useState(variant === "ready" ? "list" : "detail");
  const [picker, setPicker] = React.useState(null);
  const [changedOnly, setChangedOnly] = React.useState(true);
  const [collapsed, setCollapsed] = React.useState({});
  const [sel, setSel] = React.useState({});
  const [failed, setFailed] = React.useState(variant === "failed");
  const [rows, setRows] = React.useState(
    variant === "resolved" ? IE_MIGRATE.map((r) => ({ ...r, value: r.value || "Ctrl+Shift+8", conflict: null }))
      : variant === "nomigration" ? [] : IE_MIGRATE);

  const setOne = (id, v) => setRows((rs) => rs.map((r) => r.id === id
    ? (v === "__unbound" ? { ...r, unbound: true, value: "", conflict: null } : { ...r, value: v, unbound: false, conflict: null })
    : r));
  const onSel = (g, next, row) => setSel((s) => {
    const c = { ...s };
    (row ? [row] : g.rows).forEach((r) => { c[g.id + r.action] = next; });
    return c;
  });

  const left = rows.filter((r) => !r.value && !r.unbound).length;
  const total = IE_GROUPS.reduce((n, g) => n + g.rows.length, 0);
  const changed = IE_GROUPS.reduce((n, g) => n + g.rows.filter((r) => r.cur !== r.next).length, 0);
  const picked = IE_GROUPS.reduce((n, g) => n + g.rows.filter((r) => sel[g.id + r.action] !== false).length, 0);

  const detail = failed ? (
    // §6-6 — a broken file is a fact about the thing you were looking at, so it
    // is told INLINE in the detail area, not as a toast or a popup.
    <div style={{ padding: "var(--tasty-space-lg)" }}>
      <div style={{ borderRadius: "var(--tasty-radius)", padding: "var(--tasty-space-md) var(--tasty-size-14)",
        background: "color-mix(in srgb, var(--tasty-accent-danger) 12%, transparent)",
        border: "var(--tasty-border-width) solid color-mix(in srgb, var(--tasty-accent-danger) 35%, transparent)" }}>
        <div style={{ display: "flex", alignItems: "center", gap: "var(--tasty-space-sm)", color: "var(--tasty-accent-danger)", fontSize: 13, fontWeight: 600 }}>
          <Icon name="alertCircle" size={16} /><span>This file can't be read as keybindings</span>
        </div>
        <p style={{ margin: "4px 0 0", fontSize: 12, color: "var(--tasty-text-secondary)", maxWidth: "var(--tasty-measure-md)", lineHeight: "var(--tasty-line-height-ui)" }}>
          <span style={mono}>~/Downloads/settings.json</span> — expected a keybinding export (TOML, a <span style={mono}>[keybindings]</span> table);
          parsing stopped at line 1. Nothing was changed.
        </p>
        <div style={{ marginTop: "var(--tasty-space-sm)" }}>
          <Button variant="secondary" size="sm" onClick={() => { setFailed(false); setPicker("import"); }}>Choose another file</Button>
        </div>
      </div>
    </div>
  ) : (
    <div className="tasty-scroll" style={{ flex: 1, minHeight: 0, overflow: "auto", padding: "var(--tasty-space-lg)",
      display: "flex", flexDirection: "column", gap: "var(--tasty-space-md)" }}>
      <p style={{ margin: 0, fontSize: 12, color: "var(--tasty-text-muted)", lineHeight: "var(--tasty-line-height-ui)", maxWidth: "var(--tasty-measure-lg)" }}>
        <span style={mono}>{IE_FILE}</span> — <b style={{ color: "var(--tasty-text-secondary)" }}>{changed}</b> of {total} bindings change,
        {" "}<b style={{ color: "var(--tasty-text-secondary)" }}>{picked}</b> selected. <b>Apply</b> writes the selected rows into the draft;
        nothing is saved until you press <b style={{ color: "var(--tasty-text-secondary)" }}>Save</b>.
        {rows.length === 0 && <> No <span style={mono}>option</span> bindings to migrate.</>}
      </p>
      {rows.length > 0 && <IeMigrateCard rows={rows} onSet={setOne} />}
      {/* Discarded plugin overrides — INFORMATION, not a warning: nothing is
          wrong and there is nothing to do. Above the table because it explains
          what the table does NOT contain. */}
      <div style={{ display: "flex", alignItems: "flex-start", gap: "var(--tasty-space-sm)", fontSize: 12, color: "var(--tasty-text-muted)" }}>
        <span style={{ display: "inline-flex", flex: "none", marginTop: 1 }}><Icon name="helpCircle" size={14} /></span>
        <span>{IE_DISCARDED.length} plugin overrides were dropped — those plugins aren't installed here
          (<span style={mono}>{IE_DISCARDED.join(", ")}</span>).</span>
      </div>
      <IeDiffTable groups={IE_GROUPS} changedOnly={changedOnly} collapsed={collapsed}
        onToggleGroup={(id) => setCollapsed((c) => ({ ...c, [id]: !c[id] }))} sel={sel} onSel={onSel} />
    </div>
  );

  return (
    <div style={{ position: "relative", flex: 1, minHeight: 0, display: "flex", flexDirection: "column" }}>
      <DrillDown
        view={view}
        title="Import keybindings"
        onBack={() => setView("list")}
        actions={!failed && (
          <div style={{ display: "flex", alignItems: "center", gap: "var(--tasty-space-sm)" }}>
            <Button variant="ghost" size="sm" onClick={() => setChangedOnly((v) => !v)}>
              {changedOnly ? "Show all " + total : "Changed only"}
            </Button>
            {left > 0 && <span style={{ ...mono, fontSize: 11, color: "var(--tasty-accent-warning)" }}>{left} unresolved</span>}
            <Button variant="primary" size="sm" disabled={left > 0 || picked === 0}
              onClick={() => onFlash && onFlash("Applied to draft — press Save to commit")}>Apply</Button>
          </div>
        )}
        detail={detail}
      >
        <div style={{ padding: "var(--tasty-space-md) var(--tasty-space-lg)", display: "flex", flexDirection: "column", gap: "var(--tasty-space-md)" }}>
          <p style={{ margin: 0, fontSize: 12, color: "var(--tasty-text-muted)", lineHeight: "var(--tasty-line-height-ui)", maxWidth: "var(--tasty-measure-md)" }}>
            Move your whole keybinding configuration between machines. Importing never applies straight away —
            you see what changes first.
          </p>
          <IeActionRow glyph="download" title="Export"
            desc="Writes every binding — general, quick switch, script bindings and plugin overrides — to one file."
            action={<Button variant="secondary" size="sm" onClick={() => setPicker("export")}>Export…</Button>} />
          <IeActionRow glyph="file" title="Import"
            desc="Reads a keybinding file and shows the changes against your current bindings before anything is written."
            action={<Button variant="primary" size="sm" onClick={() => setPicker("import")}>Import…</Button>} />
        </div>
      </DrillDown>
      {picker && (
        <IePickerPopup mode={picker} onClose={() => setPicker(null)} onPick={() => {
          const m = picker; setPicker(null);
          // §6-7 — export feedback is the window's own TOAST carrying the
          // resolved path; there is no result screen to return to.
          if (m === "export") onFlash && onFlash("Exported to ~/tasty/" + IE_FILE);
          else { setFailed(false); setView("detail"); }
        }} />
      )}
    </div>
  );
}

window.TastyKit = Object.assign(window.TastyKit || {}, {
  KbImportExportSubtab, IeDiffTable, IeMigrateCard, IeActionRow, IePickerPopup,
  IE_GROUPS, IE_MIGRATE, IE_FILE, IE_DISCARDED, MODIFIER_COMBOS,
});
