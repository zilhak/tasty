// Tasty Gallery — Layouts › Pane tab strip: tab-cell status cluster, scroll arrows,
// and the move-source cue for a target scrolled out of view (2026-09-29).
// Mounted by layouts.jsx as <window.GalleryTabStrip />.
// Requests: html-script-marker-tab-slot · tab-scroll-arrow-shape · move-source-hidden-target.
(() => {
const { Spec, Stage, Meta, Note, Do, Dont } = window.Gallery;
const { Icon, StatusDot } = window.TastyDesignSystem_41fd3f;

const cap = (t) => <div style={{ fontFamily: "var(--tasty-font-mono)", fontSize: "var(--tasty-font-size-micro)", color: "var(--tasty-text-muted)", marginBottom: "var(--tasty-space-xs)" }}>{t}</div>;
const THEMES = [["Mocha", null], ["Latte", "latte"]];
const Themed = ({ attr, children, style }) => (
  <div {...(attr ? { "data-theme": attr } : {})} style={{ display: "flex", flexDirection: "column", gap: "var(--tasty-space-md)", padding: "var(--tasty-space-md)",
    background: "var(--tasty-bg-app)", border: "var(--tasty-border-width) solid var(--tasty-border-default)", borderRadius: "var(--tasty-radius)", ...style }}>{children}</div>
);

// ── tab cell: [icon][label…] · right cluster [html marker][move][busy][close] ──
function Marker({ kind, hover }) {
  const lock = kind === "blocked";
  return (
    <span title={lock ? "Scripts blocked. Click to show the notice again." : "Scripts allowed for this document until Tasty restarts"}
      style={{ flex: "none", width: "var(--tasty-html-script-marker-hit)", height: "var(--tasty-html-script-marker-hit)", display: "inline-flex", alignItems: "center", justifyContent: "center",
        borderRadius: "var(--tasty-radius-sm)", cursor: lock ? "pointer" : "default",
        background: lock && hover ? "var(--tasty-html-script-marker-hover-bg)" : "transparent",
        color: lock ? "var(--tasty-html-script-marker-fg)" : "var(--tasty-html-script-marker-allowed-fg)" }}>
      <Icon name={lock ? "lock" : "scriptFile"} size="var(--tasty-html-script-marker-size)" />
    </span>
  );
}
function TabCellS({ label = "report.html", active, hover, marker, markerHover, move, busy, width = "var(--tasty-tab-width)" }) {
  const showX = active || hover;
  const fg = active ? "var(--tasty-tab-fg-active)" : hover ? "var(--tasty-tab-fg-hover)" : "var(--tasty-tab-fg)";
  return (
    <div style={{ position: "relative", width, height: "var(--tasty-tab-height)", flex: "none", display: "flex", alignItems: "center", gap: "var(--tasty-tab-gap)",
      padding: "0 var(--tasty-space-xs) 0 var(--tasty-tab-padding-x)", color: fg,
      background: active ? "var(--tasty-tab-bg-active)" : hover ? "var(--tasty-surface-hover)" : "var(--tasty-tab-bg)",
      borderRight: "var(--tasty-border-width) solid var(--tasty-tab-separator)" }}>
      {active && <span style={{ position: "absolute", top: 0, left: 0, right: 0, height: "var(--tasty-tab-indicator-width)", background: "var(--tasty-tab-indicator)" }} />}
      <Icon name="html" size="var(--tasty-tab-icon-size)" />
      <span style={{ flex: 1, minWidth: 0, fontSize: "var(--tasty-font-size-caption)", overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>{label}</span>
      {/* right cluster — fixed slots, never shrink; the label gives way first */}
      <span style={{ flex: "none", display: "inline-flex", alignItems: "center", gap: "var(--tasty-tab-status-gap)" }}>
        {marker && <Marker kind={marker} hover={markerHover} />}
        {move && <span style={{ display: "inline-flex", color: "var(--tasty-move-source-glyph)" }}><Icon name="move" size="var(--tasty-move-source-glyph-size)" /></span>}
        {busy && <span style={{ display: "inline-flex", ["--tasty-status-dot-size"]: "var(--tasty-tab-dot-size)" }}><StatusDot status="running" /></span>}
        <span style={{ width: "var(--tasty-tab-close-size)", height: "var(--tasty-tab-close-size)", display: "inline-flex", alignItems: "center", justifyContent: "center",
          borderRadius: "var(--tasty-tab-close-radius)", color: "var(--tasty-text-muted)", visibility: showX ? "visible" : "hidden" }}><Icon name="close" size="var(--tasty-icon-size-xs)" /></span>
      </span>
    </div>
  );
}

// ── scroll arrow cell ──
function Arrow({ dir, disabled, hover, move }) {
  const color = disabled ? "var(--tasty-tab-scroll-arrow-fg-disabled)" : move ? "var(--tasty-tab-scroll-arrow-move-fg)" : "var(--tasty-tab-scroll-arrow-fg)";
  return (
    <span aria-disabled={disabled || undefined} title={move && !disabled ? "Move source is further " + dir : undefined}
      style={{ flex: "none", width: "var(--tasty-tab-scroll-arrow-width)", height: "var(--tasty-tab-height)", display: "inline-flex", alignItems: "center", justifyContent: "center",
        background: hover && !disabled ? "var(--tasty-tab-scroll-arrow-hover-bg)" : "transparent", color, cursor: disabled ? "default" : "pointer" }}>
      <Icon name={dir === "left" ? "chevronLeft" : "chevronRight"} size="var(--tasty-tab-scroll-arrow-glyph-size)" />
    </span>
  );
}
function Strip({ focused = true, at = "start", hoverSide, moveSide, tabs }) {
  return (
    <div style={{ display: "flex", alignItems: "stretch", width: "var(--tasty-size-560)", maxWidth: "100%", overflow: "hidden",
      background: focused ? "var(--tasty-surface-raised)" : "var(--tasty-bg-sidebar)", borderBottom: "var(--tasty-border-width) solid var(--tasty-separator)" }}>
      <Arrow dir="left" disabled={at === "start"} hover={hoverSide === "left"} move={moveSide === "left"} />
      <div style={{ flex: 1, minWidth: 0, display: "flex", overflow: "hidden" }}>
        {(tabs || ["server", "dev", "vim", "logs", "tests"]).map((t, i) => <TabCellS key={t} label={t} active={i === 1} />)}
      </div>
      <Arrow dir="right" disabled={at === "end"} hover={hoverSide === "right"} move={moveSide === "right"} />
    </div>
  );
}

function GalleryTabStrip() {
  return (
    <>
      <Spec title="Tab cell — the right-hand status cluster (2026-09-29)"
        when={<>Everything a tab reports sits in <b>one fixed cluster at the right end</b> of the cell, never glued to the label text. Order, left → right: <b>HTML script marker</b> · <b>move glyph</b> · <b>busy dot</b> · <b>close</b>. Items are <span className="tok">--tasty-tab-status-gap</span> (4) apart; the label keeps <span className="tok">--tasty-tab-gap</span> (8) to the cluster. The cluster never shrinks: the <b>label ellipsises first</b>, so markers stay visible in a narrow tab. Close keeps its slot (hidden until active / hover) so nothing shifts on hover. The <b>lock</b> marker has a 16 × 16 hit cell (<span className="tok">--tasty-html-script-marker-hit</span>) with its own hover fill; hovering it is still hovering the tab, so close shows too. The <b>scriptFile</b> marker is tooltip-only, no hover fill.</>}>
        <Stage variant="solo" style={{ padding: "var(--tasty-space-lg)", background: "var(--tasty-bg-app)", gap: "var(--tasty-space-lg)", flexWrap: "wrap", alignItems: "flex-start" }}>
          {THEMES.map(([label, attr]) => (
            <Themed key={label} attr={attr}>
              <span style={{ fontSize: "var(--tasty-font-size-caption)", color: "var(--tasty-text-muted)" }}>{label}</span>
              {[["marker only · inactive", { marker: "blocked" }], ["marker only · active", { marker: "blocked", active: true }], ["allowed · active", { marker: "allowed", active: true }],
                ["marker + busy", { marker: "blocked", busy: true }], ["marker + move", { marker: "blocked", move: true }], ["marker + move + busy · hover", { marker: "blocked", move: true, busy: true, hover: true }],
                ["hover on the lock", { marker: "blocked", hover: true, markerHover: true }], ["narrow tab — label ellipsises", { marker: "blocked", move: true, busy: true, active: true, label: "quarterly-report-final.html", width: "var(--tasty-size-120)" }]].map(([c, p]) => (
                <div key={c}>{cap(c)}<div style={{ display: "flex", background: "var(--tasty-bg-sidebar)" }}><TabCellS {...p} /></div></div>
              ))}
            </Themed>
          ))}
        </Stage>
        <Meta
          specs={[["order", "label … | marker · move · busy · close"], ["cluster gap", "4 · tab-status-gap"], ["label → cluster", "8 · tab-gap"], ["squeeze", "label ellipsises; cluster fixed"], ["lock hit", "16 × 16, hover = overlay-hover"], ["lock click", "re-shows the banner only (no tab switch, no focus change) · mouse only"], ["scriptFile", "tooltip only"]]}
          tokens={[{ tok: "--tasty-tab-status-gap", use: "→ space-xs 4" }, { tok: "--tasty-html-script-marker-hit", use: "→ size-16" }, { tok: "--tasty-html-script-marker-hover-bg", use: "→ overlay-hover" }, { tok: "--tasty-html-script-marker-fg", use: "lock", color: "var(--tasty-html-script-marker-fg)" }]} />
      </Spec>

      <Spec title="Scroll arrows — chevron icon, square cell, no own fill (2026-09-29)"
        when={<>The strip's scroll arrows are <b>icons</b>: <b>chevronLeft / chevronRight</b> at <span className="tok">--tasty-tab-scroll-arrow-glyph-size</span> (12). The <code>&lt;</code> <code>&gt;</code> characters go: a text character is not an icon. The cell is square, <span className="tok">--tasty-tab-scroll-arrow-width</span> = strip height 24 (was 20; the tab viewport loses 4 per side, accepted). The cell has <b>no fill of its own</b>; it sits on the strip ground (focused pane surface-raised, unfocused bg-sidebar). Hover lays <span className="tok">--tasty-tab-scroll-arrow-hover-bg</span> on the enabled side only. The same shape and roles apply to <b>every</b> horizontal tab bar with arrows, including <code>horizontal_tab_bar_with_arrows</code> (its 0.4 tint retires; the end it has reached is disabled). Contrast is judged on <b>flat colour</b> like every other token ratio; rasteriser AA is not part of the target.</>}>
        <Stage variant="solo" style={{ padding: "var(--tasty-space-lg)", background: "var(--tasty-bg-app)", gap: "var(--tasty-space-lg)", flexWrap: "wrap", alignItems: "flex-start" }}>
          {THEMES.map(([label, attr]) => (
            <Themed key={label} attr={attr}>
              <span style={{ fontSize: "var(--tasty-font-size-caption)", color: "var(--tasty-text-muted)" }}>{label}</span>
              {[["focused · left end", { at: "start" }], ["focused · middle · hover right", { at: "mid", hoverSide: "right" }], ["focused · right end", { at: "end" }], ["unfocused · middle", { focused: false, at: "mid" }], ["unfocused · left end · hover left (no fill: disabled)", { focused: false, at: "start", hoverSide: "left" }]].map(([c, p]) => (
                <div key={c}>{cap(c)}<Strip {...p} /></div>
              ))}
            </Themed>
          ))}
        </Stage>
        <Meta
          specs={[["glyph", "chevronLeft / chevronRight · 12"], ["cell", "24 × 24, no fill"], ["hover", "overlay-hover, enabled side only"], ["disabled", "the end reached · no hover · no response"], ["scope", "pane strip + horizontal_tab_bar_with_arrows"], ["3:1 basis", "flat colour (Mocha 5.65 · Latte 3.65)"]]}
          tokens={[{ tok: "--tasty-tab-scroll-arrow-width", use: "→ control-height-tab 24" }, { tok: "--tasty-tab-scroll-arrow-glyph-size", use: "→ icon-size-xs 12" }, { tok: "--tasty-tab-scroll-arrow-fg", use: "→ text-muted", color: "var(--tasty-tab-scroll-arrow-fg)" }, { tok: "--tasty-tab-scroll-arrow-fg-disabled", use: "→ text-disabled", color: "var(--tasty-tab-scroll-arrow-fg-disabled)" }]} />
        <Dont><b>Don't</b> draw arrows with a font glyph. A proportional <code>&lt;</code> at 11px renders thinner than the icon stroke and depends on the UI font.</Dont>
      </Spec>

      <Spec title="Move source scrolled out of view — the arrow on that side turns pink (2026-09-29)"
        when={<>When the pending move target (a tab, or a surface inside a tab) is in the <b>active workspace</b> but its tab cell is scrolled out of the strip, the <b>scroll arrow on that side</b> takes <span className="tok">--tasty-tab-scroll-arrow-move-fg</span> (= move glyph pink). Tab targets and surface targets use the same cue. A cell counts as visible only when it is <b>fully</b> inside the viewport; until then the arrow carries the cue. The cue is static and behaves like the arrow: hover is the arrow's hover, a click is the arrow's normal one-step scroll (no jump). It ends when the cell comes fully into view; the ring or glyph in the cell takes over. That side can never be disabled while the target is beyond it. A rect too small for the ring (under 2 × ring width) gets <b>no substitute</b>: it cannot happen at the minimum pane size. Pane maximise does not exist yet; request it separately when it does.</>}>
        <Stage variant="solo" style={{ padding: "var(--tasty-space-lg)", background: "var(--tasty-bg-app)", gap: "var(--tasty-space-lg)", flexWrap: "wrap", alignItems: "flex-start" }}>
          {THEMES.map(([label, attr]) => (
            <Themed key={label} attr={attr}>
              <span style={{ fontSize: "var(--tasty-font-size-caption)", color: "var(--tasty-text-muted)" }}>{label}</span>
              {[["target out on the right", { at: "start", moveSide: "right" }], ["target out on the left", { at: "end", moveSide: "left" }], ["target out on the right · hover", { at: "mid", moveSide: "right", hoverSide: "right" }], ["unfocused pane · target out on the left", { focused: false, at: "mid", moveSide: "left" }]].map(([c, p]) => (
                <div key={c}>{cap(c)}<Strip {...p} /></div>
              ))}
            </Themed>
          ))}
        </Stage>
        <Meta
          specs={[["where", "the scroll arrow on the target's side"], ["colour", "accent-move (same as the glyph)"], ["tab · surface target", "same cue"], ["visible", "cell fully inside the viewport"], ["input", "none of its own — the arrow's normal step scroll"], ["too-small rect", "no substitute"], ["maximise", "out of scope — new request later"]]}
          tokens={[{ tok: "--tasty-tab-scroll-arrow-move-fg", use: "→ move-source-glyph", color: "var(--tasty-tab-scroll-arrow-move-fg)" }]} />
        <Note>Nothing new appears in the strip. The arrow already means "more tabs this way"; the pink says the move source is one of them.</Note>
      </Spec>
    </>
  );
}

window.GalleryTabStrip = GalleryTabStrip;
})();
