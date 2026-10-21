// 2026-10-21 follow-up: several sources, user-only Remove, patch on existing id.
// Tasty Gallery — Overlays · Windows › Settings › Handler › File Detectors · File Handlers (2026-10-20).
// Static states of ui_kits/terminal/overlays/file_handlers.jsx
// (the kit panes, pinned through their initial-state props).
(() => {
const { Section, Spec, Stage, Meta, Note, Do, Dont } = window.Gallery;

const FhPair = ({ children }) => (
  <>{[["Mocha", null], ["Latte", "latte"]].map(([label, t]) => (
    <div key={label} {...(t ? { "data-theme": t } : {})} style={{ display: "flex", flexDirection: "column", gap: "var(--tasty-space-sm)", padding: "var(--tasty-space-md)", background: "var(--tasty-bg-app)", borderRadius: "var(--tasty-radius)", minWidth: 0, flex: "1 1 var(--tasty-size-560)", maxWidth: "100%" }}>
      <span style={{ fontSize: "var(--tasty-font-size-caption)", color: "var(--tasty-text-muted)" }}>{label}</span>{children}
    </div>
  ))}</>
);
// settings content column: same panel ground + the width other Handler subtabs get
const FhPanel = ({ children, narrow }) => (
  <div style={{ display: "flex", flexDirection: "column", gap: "var(--tasty-space-md)", padding: "var(--tasty-space-lg)", background: "var(--tasty-bg-panel)",
    border: "var(--tasty-border-width) solid var(--tasty-border-frame)", borderRadius: "var(--tasty-radius)", width: narrow ? "var(--tasty-size-360)" : "100%", maxWidth: "100%", boxSizing: "border-box" }}>{children}</div>
);
const st = { padding: "var(--tasty-space-lg)", background: "var(--tasty-bg-app)", gap: "var(--tasty-space-lg)", flexWrap: "wrap", alignItems: "flex-start" };
const Pane = ({ k, narrow, ...p }) => {
  const C = k === "det" ? window.TastyKit.FileDetectorsPane : window.TastyKit.FileHandlersPane;
  return <FhPair><FhPanel narrow={narrow}><C {...p} /></FhPanel></FhPair>;
};

const ROW_SPECS = [["row", "2 lines · padding space-sm space-xs · gap space-xs · bottom 1px separator"], ["line 1", "min-height button-height-sm · gap space-sm · id mono body 13 semibold (ellipsis, full id in tooltip) · origin Tag · [prio N micro] · [pending Tag] · spacer · Switch · trailing slot"],
  ["origin Tag(s)", "host / user neutral · plugin:<id> agent variant · detectors: one Tag per source, order host → plugin → user, gap space-sm; a rule-less user patch shows “user override” · collapse → one neutral “N sources” Tag (tooltip = full list) once the Tags would ellipsize the id"], ["trailing slot", "any user contribution → Remove (ghost sm) · pending → Undo (ghost sm), same slot · no user contribution → lock glyph icon-size-sm, glyph-dim, tooltip “Provided by host, plugin:logkit”"],
  ["line 2", "summary mono term-sm, one line, ellipsis, full value in tooltip · detectors: every source's rules in merge order “ext md, markdown · ext mdx” · handlers “<detector> → <action>”"],
  ["switched off", "id text-secondary · summary text-muted (still editable)"], ["sort", "detectors: registry order · handlers: priority ↑ then id"]];
const CARD_SPECS = [["card", "file-handler-form-bg · 1px file-handler-form-border · radius · padding file-handler-form-padding 12 · row gap file-handler-form-row-gap 8"],
  ["title", "mono micro caps, file-handler-form-label-fg"], ["field row", "label column settings-label-width 150 (wraps, body 13) · gap settings-label-gap 16 · control column flex ≥ field-width-lg; wraps under the label when the card is narrower"],
  ["control column", "control (control-height 28) + one line under it, gap space-xs — priority error > info > caption"], ["info line", "info glyph icon-size-sm + caption 12, file-handler-form-info-fg, role status; same box as the error line; live while typing (not only after Add)"], ["widths", "text Inputs block (fill) · Detector Select file-handler-select-width 200 · Action Select file-handler-kind-width 160 · Priority Input file-handler-priority-width 90"],
  ["buttons", "right-aligned, gap space-sm: Cancel ghost sm · Add primary sm"], ["error", "alertCircle icon-size-sm + caption 12, file-handler-form-error-fg; replaces that field's caption; Input invalid. Cross-field error (no rule) sits above the button row"]];
const TOKENS = [
  { tok: "--tasty-file-handler-form-bg", use: "→ surface-raised", color: "var(--tasty-file-handler-form-bg)" },
  { tok: "--tasty-file-handler-form-border", use: "→ border-default", color: "var(--tasty-file-handler-form-border)" },
  { tok: "--tasty-file-handler-form-label-fg", use: "→ text-secondary-raised", color: "var(--tasty-file-handler-form-label-fg)" },
  { tok: "--tasty-file-handler-form-caption-fg", use: "→ text-secondary-raised", color: "var(--tasty-file-handler-form-caption-fg)" },
  { tok: "--tasty-file-handler-form-error-fg", use: "→ accent-danger", color: "var(--tasty-file-handler-form-error-fg)" },
  { tok: "--tasty-file-handler-priority-width", use: "→ field-width-xs 90" },
  { tok: "--tasty-file-handler-select-width", use: "→ field-width-lg 200" },
  { tok: "--tasty-file-handler-kind-width", use: "→ field-width-md 160" },
  { tok: "--tasty-file-handler-prio-fg", use: "→ text-muted", color: "var(--tasty-file-handler-prio-fg)" },
  { tok: "--tasty-file-handler-summary-fg", use: "→ text-secondary", color: "var(--tasty-file-handler-summary-fg)" },
  { tok: "--tasty-file-handler-empty-fg", use: "→ text-muted", color: "var(--tasty-file-handler-empty-fg)" },
  { tok: "--tasty-tag-disabled-bg", use: "added / removed on save Tag", color: "var(--tasty-tag-disabled-bg)" },
  { tok: "--tasty-file-handler-form-info-fg", use: "new · → text-secondary-raised", color: "var(--tasty-file-handler-form-info-fg)" },
  { tok: "--tasty-file-handler-summary-removed-fg", use: "new · → text-muted", color: "var(--tasty-file-handler-summary-removed-fg)" },
];

function FileHandlerSection() {
  return (
    <Section id="filehandlers" title="Handler › File Detectors · File Handlers — list + add card (2026-10-20)">
      <Spec title="Registered rows — two lines, no table"
        when={<>Both subtabs list every registered entry as a <b>two-line row</b> (the Hook Handlers row). Line 1 carries identity and state: <b>id</b>, <b>origin</b> Tag, <b>prio</b> (handlers), Switch and the trailing slot. Line 2 is one mono summary: the detector's <b>rules</b>, or the handler's <b>detector → action</b>. Seven columns do not fit the settings content width, and a table would truncate every column; two lines truncate only the id and the summary, both with the full value in a tooltip. Only <b>user</b> rows have Remove; host and plugin rows show the padlock.</>}>
        <Stage variant="solo" style={st}><Pane k="det" /></Stage>
        <Stage variant="solo" style={st}><Pane k="hdl" /></Stage>
        <Meta specs={ROW_SPECS} tokens={TOKENS.slice(8, 10)} />
      </Spec>

      <Spec title="Add card — open, empty"
        when={<>“Add detector” / “Add handler” (secondary sm, plus glyph, right of the intro) opens the card between the intro and the list, and is disabled while it is open. Add stays <b>disabled until the required fields have a value</b> (Hook Handlers rule): detector = Id; handler = Short name + Detector + the action's required field. The <b>Saved as user/…</b> caption shows the stored id as you type.</>}>
        <Stage variant="solo" style={st}><Pane k="det" adding /></Stage>
        <Stage variant="solo" style={st}><Pane k="hdl" adding /></Stage>
        <Meta specs={CARD_SPECS} tokens={TOKENS.slice(0, 8)} />
      </Spec>

      <Spec title="Add card — typing, validation errors"
        when={<>Format errors are checked <b>when Add is pressed</b> (Add was enabled: the required fields are filled). The error replaces the field's caption and stays until the field is valid again; Add stays clickable. Detector: <b>id characters</b> (lowercase, digits, -, ≤ 64) and the cross-field <b>no extension and no glob</b> line above the buttons. Handler: <b>short name characters</b> (≤ 32), <b>priority not a whole number</b>, IPC <b>method not dotted</b>. There is <b>no duplicate error</b> any more: an existing id is a patch (below). The detector Id caption is the character rule; the stored id equals the input, so there is no “Saved as” line. Handlers keep “Saved as user/&lt;short&gt;”.</>}>
        <Stage variant="solo" style={st}><Pane k="det" adding attempted draft={{ id: "Docker_File" }} /></Stage>
        <Stage variant="solo" style={st}><Pane k="hdl" adding attempted draft={{ short: "docker.system", det: "dockerfile", prio: "1.5", surface: "text_view" }} /></Stage>
        <Note>Strings: <code>file_detector.id.hint</code> “Lowercase letters, digits and -. Up to 64.” · <code>file_detector.error.id_chars</code> “Use lowercase letters, digits and - only, up to 64.” · <code>file_detector.error.reserved</code> “Ids starting with $ are reserved. You can only change ones that already exist.” · <code>.error.no_rules</code> “Add at least one extension or a path glob.” · <code>file_handler.error.short_chars</code> “Use lowercase letters, digits and - only, up to 32.” · <code>.error.priority_int</code> “Enter a whole number.” · <code>.error.method</code> “Use a dotted name, e.g. com.example.foo.open.” Removed: <code>.error.duplicate</code>.</Note>
      </Spec>

      <Spec title="Handler add card — action kinds"
        when={<>The Action Select swaps the rows below it. <b>Open surface</b>: Surface kind (required) + Param key (empty = <code>file</code>). <b>IPC</b>: Method (required). <b>System</b>: no rows; the Action caption says what it does.</>}>
        <Stage variant="solo" style={st}><Pane k="hdl" adding draft={{ short: "notes-view", det: "markdown", kind: "surface", surface: "markdown_view" }} /></Stage>
        <Stage variant="solo" style={st}><Pane k="hdl" adding draft={{ short: "notes-ipc", det: "markdown", prio: "40", kind: "ipc", method: "com.example.notes.open" }} /></Stage>
        <Stage variant="solo" style={st}><Pane k="hdl" adding draft={{ short: "notes-os", det: "markdown", kind: "system" }} /></Stage>
      </Spec>

      <Spec title="Several sources on one detector (2026-10-21)"
        when={<>A detector id merges contributions <b>Host → Plugin → User</b>. Line 1 shows <b>one origin Tag per source</b> in that order; line 2 lists every source's rules in the same order. A user patch with <b>no rules</b> (e.g. the user only switched a plugin detector off) adds a <b>user override</b> Tag. Any user contribution puts <b>Remove</b> in the trailing slot even when other sources exist; rows with no user contribution keep the padlock. When the Tags would push the id into an ellipsis (narrow column, long German strings) they collapse into one <b>N sources</b> Tag whose tooltip lists them; the id keeps its room.</>}>
        <Stage variant="solo" style={st}><Pane k="det" /></Stage>
        <Stage variant="solo" style={st}><Pane k="det" narrow /></Stage>
        <Note>Rows: <code>markdown</code> host + user rule · <code>log</code> host + plugin:logkit + user rule · <code>notebook</code> plugin:md-plus + user override (switched off, no user rule). Strings: <code>file_detector.origin.user_override</code> “user override” · <code>.origin.sources</code> “{"{n}"} sources”.</Note>
      </Spec>

      <Spec title="Remove a user contribution — the row stays (2026-10-21)"
        when={<>On a row with other sources, Remove takes out <b>only the user part</b>. The row is <b>not struck</b>: only the <b>user Tag</b> is struck (Tag disabled), the user rule pieces in line 2 are struck in <code>summary-removed-fg</code>, and the pending Tag says what goes: <b>your rules removed on save</b>, or <b>your override removed on save</b> for a rule-less patch. The Switch is disabled and shows the state the row will have <b>after Save</b> (the plugin default for <code>notebook</code>). Undo restores the row. A row with <b>only</b> a user contribution keeps the old idiom: whole id struck, <b>removed on save</b>.</>}>
        <Stage variant="solo" style={st}><Pane k="det" unpatch /></Stage>
        <Stage variant="solo" style={st}><Pane k="det" unpatch pending /></Stage>
        <Note>Strings: <code>file_detector.pending.yours_removed</code> “your rules removed on save” · <code>.pending.override_removed</code> “your override removed on save” · <code>.pending.removed</code> “removed on save”.</Note>
      </Spec>

      <Spec title="Add with an existing id — patch, not an error (2026-10-21)"
        when={<>Typing an id that is already registered shows an <b>info line</b> under the field at once (before Add), and the primary button reads <b>Update detector</b>. No rule changes for Add being enabled. With no user contribution yet: “<b>{"{id}"} already exists. Your rules and settings are added to it on save.</b>” With an earlier user contribution: “<b>You already changed {"{id}"}. This replaces your earlier rules and settings on save.</b>” A registered reserved id (<code>$directory</code>) is patched the same way. A <b>new</b> reserved id (<code>$foo</code>) passes the character check but fails with the reserved error on Add. After Update the card closes and the <b>existing row</b> gets a user Tag and <b>your rules added on save</b> / <b>your rules replaced on save</b>; no new row appears. Undo restores the previous contribution.</>}>
        <Stage variant="solo" style={st}><Pane k="det" adding draft={{ id: "image", ext: "heic" }} /></Stage>
        <Stage variant="solo" style={st}><Pane k="det" adding draft={{ id: "markdown", ext: "mdx, mdown" }} /></Stage>
        <Stage variant="solo" style={st}><Pane k="det" adding draft={{ id: "$directory", glob: "**/.git" }} /></Stage>
        <Stage variant="solo" style={st}><Pane k="det" adding attempted draft={{ id: "$foo", ext: "foo" }} /></Stage>
        <Stage variant="solo" style={st}><Pane k="det" patched /></Stage>
        <Note>Strings: <code>file_detector.info.patch</code> · <code>.info.replace</code> · <code>.add.update</code> “Update detector” · <code>.pending.yours_added</code> “your rules added on save” · <code>.pending.yours_replaced</code> “your rules replaced on save”.</Note>
      </Spec>

      <Spec title="Handler — same short name replaces (2026-10-21)"
        when={<>A short name whose <code>user/&lt;short&gt;</code> already exists shows the info line “<b>user/{"{short}"} already exists. This replaces it on save.</b>” in place of the Saved as caption, and the button reads <b>Update handler</b>. After Update the row shows the new values with <b>replaced on save</b> + Undo (back to the old values). Host / plugin handlers never collide: their ids have no <code>user/</code> prefix.</>}>
        <Stage variant="solo" style={st}><Pane k="hdl" adding draft={{ short: "dockerfile-system", det: "dockerfile", kind: "system" }} /></Stage>
        <Note>Strings: <code>file_handler.info.replace</code> · <code>.add.update</code> “Update handler” · <code>.pending.replaced</code> “replaced on save”.</Note>
      </Spec>

      <Spec title="Pending — added on save · removed on save"
        when={<>Nothing is written until the Settings <b>Save</b>; Cancel drops it. Add <b>closes the card</b> and puts the entry <b>in the list at its sort position</b> with a disabled Tag <b>added on save</b>; Undo (same slot as Remove) takes it out. Remove keeps the row, strikes the id, shows <b>removed on save</b>, disables its Switch and turns Remove into Undo. Same pair as File Extension Mapping and Hook Handlers.</>}>
        <Stage variant="solo" style={st}><Pane k="det" pending /></Stage>
        <Stage variant="solo" style={st}><Pane k="hdl" pending /></Stage>
      </Spec>

      <Spec title="Empty list"
        when={<>One row-height line in caption <b>text-muted</b> where the rows would be, with the same bottom separator. The header count reads (0); the Add button is unchanged.</>}>
        <Stage variant="solo" style={st}><Pane k="det" empty /></Stage>
        <Stage variant="solo" style={st}><Pane k="hdl" empty /></Stage>
      </Spec>

      <Spec title="Long labels · narrow column"
        when={<>Labels wrap inside the 150 column (German). When the card is narrower than label + gap + field-width-lg the control drops under its label. Long ids and summaries ellipsize on one line.</>}>
        <Stage variant="solo" style={st}><Pane k="hdl" lang="de" adding draft={{ short: "notizen", det: "markdown", kind: "surface" }} /></Stage>
        <Stage variant="solo" style={st}><Pane k="hdl" narrow adding draft={{ short: "notes-view", det: "markdown", kind: "ipc", method: "com.example.notes.open" }} /></Stage>
        <Stage variant="solo" style={st}><Pane k="det" lang="de" narrow unpatch /></Stage>
        <Do>Keep the summary on one line and put the full value in the tooltip, so every row has the same height.</Do>
        <Dont>Shrink the label column below settings-label-width to fit long labels; wrap them instead.</Dont>
      </Spec>
    </Section>
  );
}

window.OverlaysWindowsFileHandlers = { FileHandlerSection };
})();
