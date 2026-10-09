// Settings › Keybindings › Plugins subtab (2026-10-08, batch 8).
// Box boundaries for the implementer (all component tokens, --tasty-kb-plugin-*):
//   picker row:  [title column 150 "Plugin:"] gap 16 [Select 200]
//   list gap 12
//   command:     padding-y 4 · 1px separator between commands (none after the last)
//                [title column 150, wraps] gap 16 [ control line (min-h 32, every control 28):
//                  mode Select 160 · gap 8 · slot 200 (inherit-source Select | key Input | "(Unassigned)") · gap 8 · Reset ]
//                                                [ caption (gap 4): "Inherited (Ctrl+C)" | parse error ]
// The title column is the settings Row label column (settings-label-width), so the picker Select and every
// mode Select start at the same x. Reset is a ghost md button: same 28 height as the line, disabled when
// there is no override. A draft (unsaved) change shows a 6px accent dot after the title. Save/Cancel = footer.
(() => {
const { Select, Input, Button, Kbd } = window.TastyDesignSystem_41fd3f;

const KBP_SOURCES = [
  { value: "clipboard.copy", label: "clipboard.copy" },
  { value: "clipboard.paste", label: "clipboard.paste" },
  { value: "clipboard.cut", label: "clipboard.cut" },
  { value: "select_all", label: "select_all" },
];
// resolved host binding per inherit source (null = the host action has no key → "None")
const KBP_RESOLVED = { "clipboard.copy": "Ctrl+Shift+C", "clipboard.paste": "Ctrl+Shift+V", "clipboard.cut": null, "select_all": "Ctrl+Shift+A" };
const KBP_MODES = [{ value: "inherit", label: "Inherit" }, { value: "custom", label: "Custom" }, { value: "none", label: "None" }];

const KBP_PLUGINS = [
  { id: "clipboard-viewer", name: "Clipboard Viewer", commands: [
    { id: "open", title: "Open clipboard viewer", manifest: { mode: "custom", keys: "ctrl+shift+h" } },
    { id: "paste-plain", title: "Paste last entry as plain text", manifest: { mode: "inherit", source: "clipboard.paste" } },
    { id: "clear", title: "Clear history", manifest: { mode: "none" } },
  ] },
  { id: "git-helper", name: "Git Helper", commands: [
    { id: "stage", title: "Stage hunk", manifest: { mode: "custom", keys: "ctrl+alt+g" } },
    { id: "copy-sha", title: "Copy commit SHA", manifest: { mode: "inherit", source: "clipboard.copy" } },
  ] },
];

const KBP_MODS = ["ctrl", "shift", "alt", "cmd", "super", "option"];
// one chord = modifiers + one key; a list = chords joined by commas; spaces ignored
function kbpParse(text) {
  const parts = text.replace(/\s+/g, "").split(",").filter(Boolean);
  for (const p of parts) {
    const seg = p.toLowerCase().split("+");
    const key = seg.pop();
    if (!key || seg.some((m) => !KBP_MODS.includes(m)) || KBP_MODS.includes(key)) return p;
    if (!/^([a-z0-9]|f([1-9]|1[0-9]|2[0-4])|enter|tab|space|esc|escape|backspace|delete|home|end|pageup|pagedown|up|down|left|right|[-=,.;'/\[\]\\`])$/.test(key)) return p;
  }
  return null; // null = valid
}

const kbpS = {
  title: { width: "var(--tasty-kb-plugin-title-width)", flex: "none", minHeight: "var(--tasty-kb-plugin-row-min-height)",
    display: "flex", alignItems: "center", gap: "var(--tasty-space-sm)", fontSize: 13, color: "var(--tasty-text-secondary)",
    lineHeight: "var(--tasty-line-height-ui)", overflowWrap: "anywhere" },
  line: { display: "flex", alignItems: "center", gap: "var(--tasty-kb-plugin-control-gap)", minHeight: "var(--tasty-kb-plugin-row-min-height)" },
  caption: { fontSize: "var(--tasty-font-size-caption)", lineHeight: "var(--tasty-line-height-ui)" },
};

function KbpCommandRow({ cmd, value, draft, last, recordAlt, onChange, onReset }) {
  const v = draft ?? cmd.manifest; // effective (draft override or manifest default)
  const overridden = draft != null;
  const dirty = JSON.stringify(draft) !== JSON.stringify(value);
  const err = v.mode === "custom" && v.keys ? kbpParse(v.keys) : null;
  const setMode = (mode) => {
    if (mode === "custom") onChange({ mode, keys: (draft && draft.keys) || (cmd.manifest.mode === "custom" ? cmd.manifest.keys : "") });
    else if (mode === "inherit") onChange({ mode, source: cmd.manifest.source || KBP_SOURCES[0].value });
    else onChange({ mode });
  };
  const resolved = v.mode === "inherit" ? KBP_RESOLVED[v.source] : null;
  return (
    <div style={{ display: "flex", alignItems: "flex-start", gap: "var(--tasty-kb-plugin-title-gap)", padding: "var(--tasty-kb-plugin-row-padding-y) 0",
      borderBottom: last ? "none" : "var(--tasty-border-width) solid var(--tasty-kb-plugin-separator)" }}>
      <div style={kbpS.title}>
        <span>{cmd.title}</span>
        {dirty && <span title="Changed — not saved yet" style={{ flex: "none", width: "var(--tasty-kb-plugin-draft-dot-size)", height: "var(--tasty-kb-plugin-draft-dot-size)",
          borderRadius: "50%", background: "var(--tasty-kb-plugin-draft-dot)" }}></span>}
      </div>
      <div style={{ flex: 1, minWidth: 0, display: "flex", flexDirection: "column", gap: "var(--tasty-kb-plugin-caption-gap)" }}>
        <div style={kbpS.line}>
          <Select options={KBP_MODES} value={v.mode} onChange={(e) => setMode(e.target.value)} style={{ width: "var(--tasty-kb-plugin-mode-width)" }} />
          {v.mode === "inherit" && <Select options={KBP_SOURCES} value={v.source} onChange={(e) => onChange({ mode: "inherit", source: e.target.value })} style={{ width: "var(--tasty-kb-plugin-slot-width)" }} />}
          {v.mode === "custom" && !recordAlt && <span style={{ width: "var(--tasty-kb-plugin-slot-width)", flex: "none", display: "flex" }}>
            <Input block mono invalid={!!err} value={v.keys} placeholder="ctrl+f5" onChange={(e) => onChange({ mode: "custom", keys: e.target.value })} /></span>}
          {v.mode === "custom" && recordAlt && <span style={{ width: "var(--tasty-kb-plugin-slot-width)", flex: "none", display: "flex" }}>
            <Button variant="secondary" style={{ width: "100%" }} title="Click, then press the shortcut">{v.keys ? <Kbd keys={v.keys.split(",")[0].replace(/\b\w/g, (c) => c.toUpperCase())} /> : "Record shortcut"}</Button></span>}
          {v.mode === "none" && <span style={{ width: "var(--tasty-kb-plugin-slot-width)", flex: "none", height: "var(--tasty-kb-plugin-control-height)", display: "flex", alignItems: "center",
            fontSize: 13, color: "var(--tasty-kb-plugin-none-fg)" }}>(Unassigned)</span>}
          <Button variant="ghost" disabled={!overridden} title="Clear the override and use the manifest default." onClick={onReset}>Reset</Button>
        </div>
        {v.mode === "inherit" && <span style={{ ...kbpS.caption, color: "var(--tasty-kb-plugin-caption-fg)" }}>{resolved ? `Inherited (${resolved})` : "None"}</span>}
        {err && <span style={{ ...kbpS.caption, color: "var(--tasty-kb-plugin-error-fg)" }}>Unrecognized key: <span style={{ fontFamily: "var(--tasty-font-mono)" }}>{err}</span></span>}
      </div>
    </div>
  );
}

// saved: { [pluginId/cmdId]: override } (what Save last wrote) · drafts start equal to saved.
function KbPluginsSubtab({ plugins = KBP_PLUGINS, saved = { "clipboard-viewer/clear": { mode: "custom", keys: "ctrl+alt+x" } }, seedDrafts = null, initial = null, recordAlt = false }) {
  const [pid, setPid] = React.useState(initial || (plugins[0] && plugins[0].id));
  const [drafts, setDrafts] = React.useState(() => ({ ...saved, ...(seedDrafts || {}) }));
  if (!plugins.length)
    return <p style={{ margin: 0, fontSize: "var(--tasty-font-size-caption)", color: "var(--tasty-text-muted)" }}>No plugins have registered shortcuts.</p>;
  const sorted = plugins.slice().sort((a, b) => a.name.localeCompare(b.name));
  const plugin = sorted.find((p) => p.id === pid) || sorted[0];
  const set = (k, val) => setDrafts((d) => { const n = { ...d }; if (val == null) delete n[k]; else n[k] = val; return n; });
  return (
    <div style={{ display: "flex", flexDirection: "column", gap: "var(--tasty-kb-plugin-list-gap)" }}>
      <div style={{ display: "flex", alignItems: "center", gap: "var(--tasty-kb-plugin-title-gap)", minHeight: "var(--tasty-kb-plugin-row-min-height)" }}>
        <span style={{ ...kbpS.title, minHeight: 0 }}>Plugin:</span>
        <Select options={sorted.map((p) => ({ value: p.id, label: p.name }))} value={plugin.id} onChange={(e) => setPid(e.target.value)} style={{ width: "var(--tasty-kb-plugin-picker-width)" }} />
      </div>
      <div>
        {plugin.commands.map((c, i) => {
          const k = plugin.id + "/" + c.id;
          return <KbpCommandRow key={k} cmd={c} value={saved[k] ?? null} draft={drafts[k] ?? null} last={i === plugin.commands.length - 1}
            recordAlt={recordAlt} onChange={(val) => set(k, val)} onReset={() => set(k, null)} />;
        })}
      </div>
    </div>
  );
}

window.TastyKit = Object.assign(window.TastyKit || {}, { KbPluginsSubtab, KBP_PLUGINS });
})();
