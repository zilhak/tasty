// Tasty Gallery — Overlays · Windows › Settings › Terminal › Performance: when saved values apply (2026-10-21).
// Three fixed rows; only the notice copy, role and place change.
(() => {
const { Section, Spec, Stage, Meta, Note, Do, Dont } = window.Gallery;
const { Input: PfInput, Switch: PfSwitch, HelpHint: PfHelp, Icon: PfIcon } = window.TastyDesignSystem_41fd3f;

const PF = {
  en: { note: "Saved changes apply to terminals you open afterward. Terminals that are already open don't change right away.",
    poll: "Targeted PTY polling", pollHint: "Checks a terminal for output only when it signals new output, instead of checking every terminal.",
    swap: "Scrollback disk swap", swapHint: "Keeps scrollback past the in-memory limit in a temporary file instead of dropping the oldest lines.",
    lines: "Disk scrollback lines", linesHint: "Lines kept on disk per terminal while disk swap is on. -1 keeps every line. Separate from Scrollback lines.",
    unit: "lines", unlimited: "no limit" },
  ko: { note: "저장한 값은 이후 새로 여는 터미널부터 적용됩니다. 이미 열린 터미널은 바로 바뀌지 않습니다.",
    poll: "선택적 PTY 폴링", pollHint: "모든 터미널을 확인하지 않고, 새 출력이 있다고 알린 터미널만 확인합니다.",
    swap: "스크롤백 디스크 스왑", swapHint: "메모리 보관량을 넘은 스크롤백을 버리지 않고 임시 파일에 보관합니다.",
    lines: "디스크 보관 줄 수", linesHint: "디스크 스왑이 켜져 있을 때 터미널마다 디스크에 보관하는 줄 수입니다. -1이면 모두 보관합니다. 스크롤백 줄 수와는 별도입니다.",
    unit: "줄", unlimited: "무제한" },
  ja: { note: "保存した値は、このあと新しく開くターミナルから適用されます。すでに開いているターミナルはすぐには変わりません。",
    poll: "選択的 PTY ポーリング", pollHint: "すべてのターミナルを確認せず、新しい出力を知らせたターミナルだけを確認します。",
    swap: "スクロールバックのディスク退避", swapHint: "メモリの保持量を超えたスクロールバックを捨てずに一時ファイルへ保存します。",
    lines: "ディスク保持行数", linesHint: "ディスク退避がオンのとき、ターミナルごとにディスクへ保持する行数です。-1 ですべて保持します。スクロールバック行数とは別です。",
    unit: "行", unlimited: "無制限" },
};

// the page-level note: info glyph + secondary ink, above the three rows (replaces the warning-yellow restart line)
function PfApplyNote({ L }) {
  return (
    <div style={{ display: "flex", alignItems: "flex-start", gap: "var(--tasty-settings-apply-note-gap)", maxWidth: "var(--tasty-measure-md)" }}>
      <span style={{ display: "inline-flex", flex: "none", height: "calc(var(--tasty-font-size-caption) * var(--tasty-line-height-ui))", alignItems: "center", color: "var(--tasty-settings-apply-note-glyph)" }}><PfIcon name="info" size="var(--tasty-icon-size-sm)" /></span>
      <p style={{ margin: 0, fontSize: "var(--tasty-font-size-caption)", lineHeight: "var(--tasty-line-height-ui)", color: "var(--tasty-settings-apply-note-fg)", textWrap: "pretty" }}>{L.note}</p>
    </div>
  );
}
function PfRow({ label, hint, children }) {
  return (
    <div style={{ display: "flex", alignItems: "center", flexWrap: "wrap", gap: "var(--tasty-space-xs) var(--tasty-settings-label-gap)", minHeight: "var(--tasty-settings-row-min-height)" }}>
      <span style={{ width: "var(--tasty-settings-label-width)", flex: "none", fontSize: "var(--tasty-font-size-body)", color: "var(--tasty-text-secondary)", lineHeight: "var(--tasty-line-height-ui)", display: "inline-flex", alignItems: "center", gap: "var(--tasty-help-hint-gap)" }}>
        <span>{label}:</span><PfHelp label={hint} placement="bottom" />
      </span>
      <span style={{ display: "inline-flex", alignItems: "center", gap: "var(--tasty-space-sm)", minWidth: 0 }}>{children}</span>
    </div>
  );
}
function PfPage({ lang = "en", poll = true, swap = true, lines = "10000", w }) {
  const L = PF[lang];
  return (
    <div style={{ display: "flex", flexDirection: "column", gap: "var(--tasty-space-md)", padding: "var(--tasty-space-lg)", background: "var(--tasty-bg-panel)", border: "var(--tasty-border-width) solid var(--tasty-border-frame)", borderRadius: "var(--tasty-radius)", width: w || "var(--tasty-size-560)", maxWidth: "100%", boxSizing: "border-box" }}>
      <PfApplyNote L={L} />
      <div style={{ display: "flex", flexDirection: "column", gap: "var(--tasty-space-sm)" }}>
        <PfRow label={L.poll} hint={L.pollHint}><PfSwitch checked={poll} onChange={() => {}} /></PfRow>
        <PfRow label={L.swap} hint={L.swapHint}><PfSwitch checked={swap} onChange={() => {}} /></PfRow>
        <PfRow label={L.lines} hint={L.linesHint}>
          <span style={{ display: "inline-flex", width: "var(--tasty-field-width-color)" }}><PfInput block mono value={lines} onChange={() => {}} disabled={!swap} addon={L.unit} /></span>
          {lines === "-1" && <span style={{ fontSize: "var(--tasty-font-size-caption)", color: swap ? "var(--tasty-text-secondary)" : "var(--tasty-state-disabled-fg)" }}>{L.unlimited}</span>}
        </PfRow>
      </div>
    </div>
  );
}
const PfPair = ({ children }) => (
  <>{[["Mocha", null], ["Latte", "latte"]].map(([label, t]) => (
    <div key={label} {...(t ? { "data-theme": t } : {})} style={{ display: "flex", flexDirection: "column", gap: "var(--tasty-space-sm)", padding: "var(--tasty-space-md)", background: "var(--tasty-bg-app)", borderRadius: "var(--tasty-radius)", minWidth: 0, flex: "1 1 var(--tasty-size-560)", maxWidth: "100%" }}>
      <span style={{ fontSize: "var(--tasty-font-size-caption)", color: "var(--tasty-text-muted)" }}>{label}</span>{children}
    </div>
  ))}</>
);
const PfCase = ({ label, children }) => (
  <div style={{ display: "flex", flexDirection: "column", gap: "var(--tasty-space-xs)", minWidth: 0 }}>
    <span style={{ fontSize: "var(--tasty-font-size-micro)", color: "var(--tasty-text-muted)" }}>{label}</span>{children}
  </div>
);

function PerfSection() {
  const st = { padding: "var(--tasty-space-lg)", background: "var(--tasty-bg-app)", gap: "var(--tasty-space-lg)", flexWrap: "wrap", alignItems: "flex-start" };
  return (
    <Section id="perfapply" title="Terminal › Performance — when saved values apply (2026-10-21)">
      <Spec title="Apply note + the three rows"
        when={<>The old top line said every change needs a restart, in small <code>accent-warning</code> text. Saved values actually reach the <b>terminals opened afterward</b>; terminals already open do not switch over at save time. That is a fact about timing, not a warning, so the line becomes a neutral <b>note</b>: info glyph + <code>text-secondary</code> caption, at the top of the page above the rows, one sentence pair for all three rows. It does not promise that open terminals never change (a terminal that later reconnects may pick the new polling value), and it names no internal mechanism. The rows keep their order, controls, defaults and the existing HelpHint pattern; their descriptions say what the setting does and do not repeat the timing. <b>Disk scrollback lines</b> keeps default <b>10000</b>, <b>-1 = no limit</b>, stays separate from Scrollback lines (Terminal › General), and is disabled while disk swap is off. A larger valid value from the config file is shown as is and is only written back when the user commits an edit.</>}>
        <Stage variant="solo" style={st}>
          <PfPair>
            <PfCase label="default · swap on · 10000"><PfPage /></PfCase>
            <PfCase label="swap off · lines disabled"><PfPage swap={false} /></PfCase>
            <PfCase label="-1 · no limit"><PfPage lines="-1" /></PfCase>
            <PfCase label="larger config value, shown as read"><PfPage lines="2500000" /></PfCase>
          </PfPair>
        </Stage>
        <Stage variant="solo" style={st}>
          <PfCase label="ko · 560"><PfPage lang="ko" /></PfCase>
          <PfCase label="ja · 560"><PfPage lang="ja" /></PfCase>
          <PfCase label="ja · narrow 360 — note wraps, rows stack label over control"><PfPage lang="ja" w="var(--tasty-size-360)" /></PfCase>
          <PfCase label="ko · narrow 360 · swap off"><PfPage lang="ko" w="var(--tasty-size-360)" swap={false} /></PfCase>
        </Stage>
        <Meta
          specs={[["page", "tab body padding space-lg · note → rows gap space-md · rows gap space-sm"], ["note", "top of the page, above the rows · info glyph icon-size-sm settings-apply-note-glyph (text-muted) · caption settings-apply-note-fg (text-secondary), line-height-ui · gap settings-apply-note-gap 8 · max-width measure-md · wraps, never ellipsises"], ["contrast", "text-secondary on bg-panel: Mocha ≈ 8.0 · Latte ≈ 5.1 (≥ 4.5) · glyph text-muted ≥ 3"], ["rows", "SettingsRow: label settings-label-width + HelpHint (?) · gap settings-label-gap 16 · min-height settings-row-min-height · wrap below the label when narrow"], ["Targeted PTY polling", "Switch · behaviour, default unchanged"], ["Scrollback disk swap", "Switch · behaviour, default unchanged"], ["Disk scrollback lines", "Input mono field-width-color 110 + unit addon · default 10000 · -1 shows “no limit” after the field · disabled (state-disabled) while swap is off · large file values shown verbatim, not clamped on read"], ["not added", "no restart button · no per-terminal status · no popup · no new setting"]]}
          tokens={[{ tok: "--tasty-settings-apply-note-fg", use: "new · → text-secondary", color: "var(--tasty-settings-apply-note-fg)" }, { tok: "--tasty-settings-apply-note-glyph", use: "new · → text-muted", color: "var(--tasty-settings-apply-note-glyph)" }, { tok: "--tasty-settings-apply-note-gap", use: "new · → space-sm" }]} />
        <Note>i18n — retire <code>settings.performance.restart_notice</code>; add <code>settings.performance.apply_notice</code>: “Saved changes apply to terminals you open afterward. Terminals that are already open don't change right away.” · “저장한 값은 이후 새로 여는 터미널부터 적용됩니다. 이미 열린 터미널은 바로 바뀌지 않습니다.” · “保存した値は、このあと新しく開くターミナルから適用されます。すでに開いているターミナルはすぐには変わりません。” A new key, because the meaning changes. Row labels and HelpHint descriptions shown here are the specimen's reading of the three settings: keep the product's existing label / description strings, and drop any “requires restart” wording from them. “no limit” / “무제한” / “無制限” reuses the existing unlimited label if the product has one.</Note>
        <Do>Say when values apply, once, as a neutral note at the top of the page.</Do>
        <Dont>Say a restart is required, or that open terminals never change; don't repeat the timing on every row.</Dont>
      </Spec>
    </Section>
  );
}

window.OverlaysWindowsPerf = { PerfSection, PfPage };
})();
