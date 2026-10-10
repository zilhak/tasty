// Tasty Gallery — Windows, batch 12 (2026-10-10): settings rows at narrow widths · Plugins Custom record slots ·
// Win / Super as option. Loaded before overlays-windows.jsx, which places these sections in its Page.
const WDS = window.TastyDesignSystem_41fd3f;
const { Section: WSection, Spec: WSpec, Stage: WStage, Meta: WMeta, Note: WNote } = window.Gallery;
const { Select: WSelect, Input: WInput, Kbd: WKbd, Icon: WIcon } = WDS;

const wRec = { height: "var(--tasty-kb-record-height)", minWidth: "var(--tasty-kb-record-width)", display: "inline-flex", alignItems: "center", justifyContent: "center", boxSizing: "border-box", border: "var(--tasty-border-width) solid var(--tasty-kb-record-border)", borderRadius: "var(--tasty-radius)", background: "var(--tasty-surface-raised)", fontFamily: "var(--tasty-font-mono)", fontSize: "var(--tasty-font-size-caption)", color: "var(--tasty-text-primary)", padding: "0 var(--tasty-space-sm)" };
const wAdd = { ...wRec, minWidth: 0, width: "var(--tasty-kb-record-add-width)", padding: 0, background: "transparent", color: "var(--tasty-text-muted)" };
const wLbl = { fontSize: "var(--tasty-font-size-caption)", color: "var(--tasty-text-muted)" };
const wBox = "var(--tasty-border-width) dashed var(--tasty-border-strong)";

// One settings Row. stacked = label on its own line, control at the row start (box outlines = implementer boundaries).
function WRow({ label, stacked, boxes, children }) {
  const cell = boxes ? { outline: wBox, outlineOffset: -1 } : null;
  return (
    <div style={{ display: "flex", flexDirection: stacked ? "column" : "row", alignItems: stacked ? "stretch" : "center", gap: stacked ? "var(--tasty-settings-row-stack-gap)" : "var(--tasty-settings-label-gap)", minHeight: "var(--tasty-settings-row-min-height)" }}>
      <span style={{ ...cell, flex: "none", width: stacked ? "auto" : "var(--tasty-settings-label-width)", minHeight: stacked ? 0 : "var(--tasty-settings-row-min-height)", display: "flex", alignItems: "center", fontSize: 13, color: "var(--tasty-text-secondary)", lineHeight: "var(--tasty-line-height-ui)", overflowWrap: "anywhere" }}>{label}</span>
      <span style={{ ...cell, display: "flex", alignItems: "center", flexWrap: "wrap", gap: "var(--tasty-space-xs)", minWidth: 0 }}>{children}</span>
    </div>
  );
}
// A settings content column at a given window width (sidebar 200 fixed; content inset space-lg each side).
function WSettingsCol({ win, title, stacked, children }) {
  const w = win - 200 - 2 * 16;
  return (
    <div style={{ display: "flex", flexDirection: "column", gap: "var(--tasty-space-xs)" }}>
      <span style={wLbl}>{title} — window {win} · content {w}</span>
      <div style={{ width: w, display: "flex", flexDirection: "column", gap: "var(--tasty-settings-row-gap)", padding: "var(--tasty-space-md)", background: "var(--tasty-bg-panel)", border: "var(--tasty-border-width) solid var(--tasty-border-default)", borderRadius: "var(--tasty-radius)", boxSizing: "content-box" }}>
        {children(stacked)}
      </div>
    </div>
  );
}
const wRows = (st) => (
  <>
    <WRow label="Next workspace" stacked={st} boxes><span style={wRec}>Ctrl+Alt+Down</span><span style={wAdd}><WIcon name="plus" size="var(--tasty-icon-size-sm)" /></span></WRow>
    <WRow label="Quick switch modifier" stacked={st} boxes><WSelect options={[{ value: "a", label: "Ctrl+Alt" }]} value="a" style={{ width: "var(--tasty-field-width-md)" }} /></WRow>
    <WRow label="Run “format-buffer.lua”" stacked={st} boxes><span style={{ ...wRec, color: "var(--tasty-kb-record-empty-fg)", fontFamily: "var(--tasty-font-ui)" }}>None</span></WRow>
    <WRow label="Maximum size" stacked={st} boxes><span style={{ width: "var(--tasty-field-width-xs)", flex: "none" }}><WInput block defaultValue="512" style={{ fontFamily: "var(--tasty-font-mono)", textAlign: "right" }} /></span><span style={{ fontSize: "var(--tasty-font-size-caption)", color: "var(--tasty-text-muted)" }}>MB</span></WRow>
  </>
);

function SettingsNarrowSection() {
  return (
    <WSection id="settingsnarrow" title="Settings rows at narrow widths — stacked Row (2026-10-10 b12)">
      <WSpec title="Row — side by side · stacked when the control doesn't fit"
        when={<>Below the supported minimum (a tiling WM ignoring 1100 × 700) a one-piece control can be wider than what is left beside the label. Each <b>Row decides on its own</b>: when its control's natural width does not fit in <i>content − label column − gap</i>, the label takes the <b>full row width</b> and the control moves under it at the <b>row start x</b> (not the control column), <span className="tok">--tasty-settings-row-stack-gap</span> 4 below. Rows whose control fits stay side by side, so a subtab can mix both. To stop flicker while resizing, a stacked row goes back only when the control fits with <span className="tok">--tasty-settings-row-stack-hysteresis</span> 16 to spare. Inside a stacked control the existing wraps still apply (second shortcut buttons wrap from the control x, units drop under the field). Dashed outlines are the label / control boxes.</>}>
        <WStage variant="solo" style={{ padding: 20, background: "var(--tasty-bg-app)", gap: 16, flexWrap: "wrap", alignItems: "flex-start" }}>
          <WSettingsCol win={700} title="side by side (fits)" stacked={false}>{wRows}</WSettingsCol>
          <WSettingsCol win={520} title="stacked" stacked>{wRows}</WSettingsCol>
          <WSettingsCol win={420} title="stacked" stacked>{wRows}</WSettingsCol>
        </WStage>
        <WMeta
          specs={[["switch", "per Row · control natural width > content − label column − settings-label-gap"], ["stacked", "label full width · control at row start · gap settings-row-stack-gap 4"], ["back", "only when it fits with settings-row-stack-hysteresis 16 spare"], ["row height", "min settings-row-min-height 32 applies to each line"], ["captions", "unchanged — measure-md or the remaining width, whichever is smaller"], ["inside", "existing wraps (buttons from control x · unit under the field)"]]}
          tokens={[{ tok: "--tasty-settings-row-stack-gap", use: "→ space-xs 4" }, { tok: "--tasty-settings-row-stack-hysteresis", use: "→ space-lg 16" }, { tok: "--tasty-settings-label-width", use: "150 (side by side only)" }, { tok: "--tasty-settings-label-gap", use: "16" }]} />
      </WSpec>
    </WSection>
  );
}

function KbPluginsRecordSpec() {
  const T = window.TastyKit;
  return (
    <WSpec title="Keybindings › Plugins — Custom = record slots (2026-10-10 b12)"
      when={<>Custom now records, using the same record slot as the other subtabs (one slot per key, then +, or a None slot), with two local changes so the line keeps <b>one control height</b>: slots here are <span className="tok">--tasty-kb-plugin-record-height</span> 28, like the mode Select and Reset. Slots are one group (space-xs 4 between them); the group's ends keep the line's 8. The slot column is no longer a fixed 200 box: one key is 140 + 4 + 32 = 176, more keys push Reset right. When the line runs out of width it <b>wraps as one flow</b>: the next line starts at the <b>mode Select x</b> (the same control x where other subtabs wrap), and Reset stays the <b>last item</b>. Every key is shown; the single-button sample is retired. The Esc hint sits under the subtab as on the other subtabs.</>}>
      <WStage variant="solo" style={{ padding: 20, background: "var(--tasty-bg-panel)", gap: "var(--tasty-space-xl)", flexDirection: "column", alignItems: "flex-start" }}>
        {T && T.KbPluginsSubtab && <>
          <div style={{ display: "flex", flexDirection: "column", gap: "var(--tasty-space-sm)", width: 620 }}>
            <span style={wLbl}>recording — two keys + a third being recorded · a Custom command with no key shows the None slot</span>
            <T.KbPluginsSubtab saved={{}} seedDrafts={{ "clipboard-viewer/open": { mode: "custom", keys: "ctrl+shift+h,ctrl+alt+v", recording: true }, "clipboard-viewer/clear": { mode: "custom", keys: "" } }} />
          </div>
          <div style={{ display: "flex", flexDirection: "column", gap: "var(--tasty-space-sm)", width: 460 }}>
            <span style={wLbl}>narrow (460) — three keys wrap from the mode x · Reset last</span>
            <T.KbPluginsSubtab saved={{}} seedDrafts={{ "clipboard-viewer/open": { mode: "custom", keys: "ctrl+shift+h,ctrl+alt+v,ctrl+f5" } }} />
          </div>
        </>}
      </WStage>
      <WMeta
        specs={[["slot", "kb_record_slot · min kb-record-width 140 × kb-plugin-record-height 28 · + kb-record-add-width 32 · None slot 140"], ["gaps", "space-xs 4 between slots · kb-plugin-control-gap 8 around the group"], ["width", "grows with keys · Reset pushed right"], ["wrap", "one flow · next line at the mode Select x · row gap 4 · Reset last"], ["recording", "“Press key combination...” takes the + slot's place, grows to its text"], ["unparsable config text", "slot shows the raw text with the danger border + the error caption"]]}
        tokens={[{ tok: "--tasty-kb-plugin-record-height", use: "→ kb-plugin-control-height 28" }, { tok: "--tasty-kb-record-width", use: "140" }, { tok: "--tasty-kb-record-add-width", use: "32" }, { tok: "--tasty-kb-record-border", use: "rest", color: "var(--tasty-kb-record-border)" }, { tok: "--tasty-kb-record-border-hover", use: "hover", color: "var(--tasty-kb-record-border-hover)" }]} />
    </WSpec>
  );
}

const wOS = [["macOS", "⌃⌥K", "Option"], ["Windows", "Ctrl+Win+K", "Win"], ["Linux", "Super+Shift+1", "Super"]];
function OptionKeySection() {
  return (
    <WSection id="optionkey" title="Win · Super as option — names and notices (2026-10-10 b12)">
      <WSpec title="Key names · OS-reserved caption · text-entry hint"
        when={<>The <code>option</code> token now also means <b>Win</b> on Windows and <b>Super</b> on Linux. They are shown as <b>words</b>, like Ctrl and Alt — no logo glyph (the Windows logo is a trademark and Linux keyboards print different marks). Linux uses <b>Super</b> on every desktop, KDE included, so one config reads the same everywhere. The display-style dropdown stays macOS-only (there is nothing to choose between on the other two). A shortcut <b>typed as text</b> (config file, bundle import, plugin shortcut field) that matches a short known list of OS-owned combos gets a warning caption — the list is a hint, not a guarantee, and the copy says “may”. A text field that receives an OS key name (<code>cmd+k</code>, <code>super+k</code>, <code>win+k</code>, <code>meta+k</code>) gets a dedicated caption saying how to write it.</>}>
        <WStage variant="solo" style={{ padding: 20, background: "var(--tasty-bg-panel)", gap: 24, flexWrap: "wrap", alignItems: "flex-start" }}>
          <div style={{ display: "flex", flexDirection: "column", gap: 8 }}>
            <span style={wLbl}>shortcut display per OS</span>
            {wOS.map(([os, combo, word]) => (
              <div key={os} style={{ display: "grid", gridTemplateColumns: "72px 140px 1fr", alignItems: "center", gap: 8 }}>
                <span style={{ fontSize: 12, color: "var(--tasty-text-secondary)" }}>{os}</span>
                <span style={wRec}>{combo}</span>
                <WKbd keys={word} />
              </div>
            ))}
          </div>
          <div style={{ display: "flex", flexDirection: "column", gap: 4, width: 300 }}>
            <span style={wLbl}>plugin shortcut field — OS-reserved (Windows)</span>
            <WInput block mono defaultValue="option+l" />
            <span style={{ display: "flex", alignItems: "flex-start", gap: "var(--tasty-space-xs)", fontSize: "var(--tasty-font-size-caption)", lineHeight: "var(--tasty-line-height-ui)", color: "var(--tasty-kb-os-reserved-fg)" }}><span style={{ display: "inline-flex", flex: "none", marginTop: 1 }}><WIcon name="alertTriangle" size="var(--tasty-icon-size-xs)" /></span>Windows may use Win+L itself, so Tasty might not receive it.</span>
          </div>
          <div style={{ display: "flex", flexDirection: "column", gap: 4, width: 300 }}>
            <span style={wLbl}>plugin shortcut field — OS key name typed</span>
            <WInput block mono defaultValue="cmd+k" invalid />
            <span style={{ fontSize: "var(--tasty-font-size-caption)", lineHeight: "var(--tasty-line-height-ui)", color: "var(--tasty-kb-plugin-error-fg)" }}>Write Win, Super and Option as <code style={{ fontFamily: "var(--tasty-ui-code-font)", fontSize: "1em", background: "var(--tasty-ui-code-bg)", color: "var(--tasty-ui-code-fg)", padding: "0 var(--tasty-ui-code-padding-x)", borderRadius: "var(--tasty-ui-code-radius)" }}>option</code>, and Cmd as <code style={{ fontFamily: "var(--tasty-ui-code-font)", fontSize: "1em", background: "var(--tasty-ui-code-bg)", color: "var(--tasty-ui-code-fg)", padding: "0 var(--tasty-ui-code-padding-x)", borderRadius: "var(--tasty-ui-code-radius)" }}>alt</code>.</span>
          </div>
        </WStage>
        <WMeta
          specs={[["names", "macOS Option / ⌥ (display setting) · Windows Win · Linux Super (all desktops) — words, no icon asset"], ["display dropdown", "macOS only"], ["reserved list (hint)", "Windows: Win alone, Win+L · D · E · R · I · Tab · Linux: Super alone"], ["reserved caption", "warning tone · under the field / one line in the import warning block · never blocks saving"], ["recorded", "no caption (the OS key never reaches the recorder)"], ["OS key name", "error caption replacing the generic “Unrecognized key” · code runs for the tokens"], ["hold helper", "no option binding on Win / Linux → holding Win / Super shows nothing (agreed)"]]}
          tokens={[{ tok: "--tasty-kb-os-reserved-fg", use: "→ accent-warning", color: "var(--tasty-kb-os-reserved-fg)" }, { tok: "--tasty-kb-plugin-error-fg", use: "OS key name", color: "var(--tasty-kb-plugin-error-fg)" }]} />
        <WNote>i18n — <code>keys.os_reserved.windows</code> “Windows may use {"{combo}"} itself, so Tasty might not receive it.” / “Windows가 {"{combo}"}를 먼저 쓸 수 있어 Tasty에 전달되지 않을 수 있습니다.” / “{"{combo}"} は Windows が使うことがあり、Tasty に届かない場合があります。” · <code>keys.os_reserved.linux</code> “Your desktop may use {"{combo}"} itself, so Tasty might not receive it.” / “데스크톱이 {"{combo}"}를 먼저 쓸 수 있어 Tasty에 전달되지 않을 수 있습니다.” / “{"{combo}"} はデスクトップが使うことがあり、Tasty に届かない場合があります。” · <code>keys.os_key_name</code> “Write Win, Super and Option as `option`, and Cmd as `alt`.” / “Win·Super·Option 은 `option`, Cmd 는 `alt` 로 적으세요.” / “Win・Super・Option は `option`、Cmd は `alt` と書いてください。” (backticks = code runs). The option-migration card and its gallery sample are removed (feature deleted).</WNote>
      </WSpec>
    </WSection>
  );
}

window.OverlaysWindowsB12 = { SettingsNarrowSection, KbPluginsRecordSpec, OptionKeySection };
