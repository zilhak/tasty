// Tasty Gallery — Foundations. Tokens shown ONLY in the context that
// consumes them: each swatch/value carries a "used for" statement, and
// the spacing section shows real gaps/padding on the 4px grid, never
// abstract bars.
const { Section, Spec, Stage, Cluster, Meta, Note, Do, Dont } = window.Gallery;
const { Button, Tag, StatusDot, Input, Badge, Toast } = window.TastyDesignSystem_41fd3f;

const NAV = [
  { id: "elevation", label: "Color — elevation" },
  { id: "text", label: "Color — text" },
  { id: "accents", label: "Color — accent roles" },
  { id: "terminal", label: "Color — terminal / ANSI" },
  { id: "type", label: "Type" },
  { id: "spacing", label: "Spacing" },
  { id: "shape", label: "Radius · border · motion" },
  { id: "uiscale", label: "UI scale" },
];

// ── a labelled surface tile (token + what uses it) ──
function Tile({ bg, name, role, dark }) {
  return (
    <div style={{ background: bg, border: "1px solid var(--tasty-border-default)", borderRadius: "var(--tasty-radius)",
      padding: "11px 13px", minWidth: 168 }}>
      <div style={{ fontFamily: "var(--tasty-font-mono)", fontSize: 11.5, color: "var(--tasty-text-primary)" }}>{name}</div>
      <div style={{ fontSize: 11.5, color: "var(--tasty-text-muted)", marginTop: 3 }}>{role}</div>
    </div>
  );
}

function ElevationStack() {
  // a true nested composition: chrome → panel → card → row
  return (
    <div style={{ background: "var(--tasty-bg-app)", border: "1px solid var(--tasty-border-default)",
      borderRadius: "var(--tasty-radius)", padding: 14, width: 360 }}>
      <Lbl tok="--tasty-bg-app">window chrome / deepest</Lbl>
      <div style={{ background: "var(--tasty-bg-sidebar)", border: "1px solid var(--tasty-separator)",
        borderRadius: "var(--tasty-radius)", padding: 12, marginTop: 8 }}>
        <Lbl tok="--tasty-bg-sidebar">sidebars, side panels</Lbl>
        <div style={{ background: "var(--tasty-bg-panel)", border: "1px solid var(--tasty-separator)",
          borderRadius: "var(--tasty-radius)", padding: 12, marginTop: 8 }}>
          <Lbl tok="--tasty-bg-panel">primary content panels</Lbl>
          <div style={{ background: "var(--tasty-surface-raised)", border: "1px solid var(--tasty-border-default)",
            borderRadius: "var(--tasty-radius)", padding: 8, marginTop: 8 }}>
            <Lbl tok="--tasty-surface-raised">cards, inputs, menus</Lbl>
            <div style={{ marginTop: 6, display: "flex", flexDirection: "column", gap: 2 }}>
              <div style={{ background: "var(--tasty-surface-hover)", borderRadius: "var(--tasty-radius-sm)", padding: "5px 8px",
                fontFamily: "var(--tasty-font-mono)", fontSize: 11, color: "var(--tasty-text-secondary)" }}>--tasty-surface-hover · hovered row</div>
              <div style={{ background: "var(--tasty-surface-active)", borderRadius: "var(--tasty-radius-sm)", padding: "5px 8px",
                fontFamily: "var(--tasty-font-mono)", fontSize: 11, color: "var(--tasty-text-primary)" }}>--tasty-surface-active · selected row</div>
            </div>
          </div>
        </div>
      </div>
    </div>
  );
}
function Lbl({ tok, children }) {
  return (
    <div style={{ display: "flex", alignItems: "baseline", gap: 8 }}>
      <span style={{ fontFamily: "var(--tasty-font-mono)", fontSize: 11.5, color: "var(--tasty-text-primary)" }}>{tok}</span>
      <span style={{ fontSize: 11, color: "var(--tasty-text-muted)" }}>{children}</span>
    </div>
  );
}

// ── accent role row: name + live usage ──
function AccentRow({ tok, color, role, demo }) {
  return (
    <div style={{ display: "grid", gridTemplateColumns: "16px 150px 1fr", alignItems: "center", gap: 13,
      padding: "9px 0", borderBottom: "1px solid var(--tasty-separator)" }}>
      <span style={{ width: 16, height: 16, borderRadius: "var(--tasty-radius-sm)", background: color,
        border: "1px solid var(--tasty-border-strong)" }} />
      <span style={{ fontFamily: "var(--tasty-font-mono)", fontSize: 12, color: "var(--tasty-text-primary)" }}>{tok}</span>
      <div style={{ display: "flex", alignItems: "center", gap: 12 }}>
        <span style={{ fontSize: 12.5, color: "var(--tasty-text-secondary)", width: 168, flex: "none" }}>{role}</span>
        <span style={{ display: "inline-flex", gap: 8, alignItems: "center" }}>{demo}</span>
      </div>
    </div>
  );
}

// ── spacing demo: real usage on the 4px grid ──
function SpaceUse({ token, px, usedFor, children }) {
  return (
    <div style={{ borderBottom: "1px solid var(--tasty-separator)", padding: "16px 0" }}>
      <div style={{ display: "flex", alignItems: "baseline", gap: 10, marginBottom: 10 }}>
        <span style={{ fontFamily: "var(--tasty-font-mono)", fontSize: 12.5, color: "var(--tasty-text-primary)" }}>{token}</span>
        <span style={{ fontFamily: "var(--tasty-font-mono)", fontSize: 11, color: "var(--tasty-accent-info)" }}>{px}</span>
        <span style={{ fontSize: 12, color: "var(--tasty-text-muted)" }}>used for {usedFor}</span>
      </div>
      <div style={{ position: "relative" }}>
        <div className="gridover" style={{ borderRadius: 2 }} />
        {children}
      </div>
    </div>
  );
}

function typeScaleRow(size, tok, weight, role, sample) {
  return (
    <div key={tok} style={{ display: "grid", gridTemplateColumns: "150px 1fr", alignItems: "baseline", gap: 16,
      padding: "9px 0", borderBottom: "1px solid var(--tasty-separator)" }}>
      <div>
        <div style={{ fontFamily: "var(--tasty-font-mono)", fontSize: 11.5, color: "var(--tasty-text-primary)" }}>{tok}</div>
        <div style={{ fontFamily: "var(--tasty-font-mono)", fontSize: 10.5, color: "var(--tasty-text-muted)", marginTop: 2 }}>{size} · {weight}</div>
      </div>
      <div>
        <div style={{ fontSize: size, fontWeight: weight, color: "var(--tasty-text-primary)", lineHeight: 1.3 }}>{sample}</div>
        <div style={{ fontSize: 11.5, color: "var(--tasty-text-muted)", marginTop: 3 }}>{role}</div>
      </div>
    </div>
  );
}

// ── terminal / ANSI palette ──
// The 16 SGR colors a terminal cell paints with, plus the four state-fill
// tokens. These are TERMINAL CONTENT colors, distinct from the UI accent roles.
const ANSI_NORMAL = [
  ["--tasty-ansi-black", "30"], ["--tasty-ansi-red", "31"], ["--tasty-ansi-green", "32"], ["--tasty-ansi-yellow", "33"],
  ["--tasty-ansi-blue", "34"], ["--tasty-ansi-magenta", "35"], ["--tasty-ansi-cyan", "36"], ["--tasty-ansi-white", "37"],
];
const ANSI_BRIGHT = [
  ["--tasty-ansi-bright-black", "90"], ["--tasty-ansi-bright-red", "91"], ["--tasty-ansi-bright-green", "92"], ["--tasty-ansi-bright-yellow", "93"],
  ["--tasty-ansi-bright-blue", "94"], ["--tasty-ansi-bright-magenta", "95"], ["--tasty-ansi-bright-cyan", "96"], ["--tasty-ansi-bright-white", "97"],
];

function AnsiRow({ tok, sgr }) {
  return (
    <div style={{ display: "grid", gridTemplateColumns: "16px 1fr auto", alignItems: "center", gap: 10,
      padding: "7px 2px", borderBottom: "1px solid var(--tasty-separator)" }}>
      <span style={{ width: 16, height: 16, borderRadius: "var(--tasty-radius-sm)", background: `var(${tok})`,
        border: "1px solid var(--tasty-border-strong)" }} />
      <span style={{ fontFamily: "var(--tasty-font-mono)", fontSize: 11.5, color: "var(--tasty-text-primary)",
        overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>{tok.replace("--tasty-", "")}</span>
      <span style={{ fontFamily: "var(--tasty-font-mono)", fontSize: 10.5, color: "var(--tasty-text-muted)" }}>SGR {sgr}</span>
    </div>
  );
}
function AnsiCol({ heading, rows }) {
  return (
    <div>
      <div style={{ fontFamily: "var(--tasty-font-mono)", fontSize: 10, textTransform: "uppercase",
        letterSpacing: ".06em", color: "var(--tasty-text-muted)", marginBottom: 4 }}>{heading}</div>
      {rows.map(([tok, sgr]) => <AnsiRow key={tok} tok={tok} sgr={sgr} />)}
    </div>
  );
}
// state fill rendered onto a faux terminal line
function Hi({ tok, children }) {
  return <span style={{ background: `var(${tok})`, color: "var(--tasty-text-primary)", borderRadius: 2, padding: "0 1px" }}>{children}</span>;
}
function SemRow({ tok, role }) {
  return (
    <div style={{ display: "grid", gridTemplateColumns: "16px 1fr", alignItems: "center", gap: 10, padding: "6px 0",
      borderBottom: "1px solid var(--tasty-separator)" }}>
      <span style={{ width: 16, height: 16, borderRadius: "var(--tasty-radius-sm)", background: `var(${tok})`,
        border: "1px solid var(--tasty-border-strong)" }} />
      <div style={{ display: "flex", alignItems: "baseline", gap: 10, flexWrap: "wrap" }}>
        <span style={{ fontFamily: "var(--tasty-font-mono)", fontSize: 11.5, color: "var(--tasty-text-primary)" }}>{tok.replace("--tasty-", "")}</span>
        <span style={{ fontSize: 11.5, color: "var(--tasty-text-muted)" }}>{role}</span>
      </div>
    </div>
  );
}

function Foundations() {
  return (
    <>
      {/* ELEVATION */}
      <Section id="elevation" title="Color — elevation (surface ramp)">
        <Spec title="Depth reads through surface tint, never shadow"
          when={<>Tasty has <b>no shadow system</b> (modals aside). Pick the next step up the ramp whenever you nest a container. A card on a panel is <b>--tasty-surface-raised</b> on <b>--tasty-bg-panel</b> — one step, not a drop shadow.</>}>
          <Stage variant="solo center">
            <ElevationStack />
          </Stage>
          <Meta
            specs={[["ramp order", <>crust→mantle→base→surface0→1→2</>], ["border", <>1px <span className="tok">--tasty-border-default</span></>], ["shadow", "none (UI); modals only"]]}
            tokens={[
              { tok: "--tasty-bg-app", use: "window", color: "var(--tasty-bg-app)" },
              { tok: "--tasty-bg-sidebar", use: "rails", color: "var(--tasty-bg-sidebar)" },
              { tok: "--tasty-bg-panel", use: "content", color: "var(--tasty-bg-panel)" },
              { tok: "--tasty-surface-raised", use: "cards", color: "var(--tasty-surface-raised)" },
              { tok: "--tasty-surface-hover", use: "hover row", color: "var(--tasty-surface-hover)" },
              { tok: "--tasty-surface-active", use: "selected", color: "var(--tasty-surface-active)" },
            ]} />
          <Do><b>Do</b> tint up exactly one ramp step each time you nest a container.</Do>
          <Dont><b>Don't</b> add a <code>box-shadow</code> to cards or panels — only the modal scrim layer uses shadow.</Dont>
        </Spec>
      </Section>

      {/* TEXT */}
      <Section id="text" title="Color — text">
        <Spec title="Hierarchy by text color, on any surface"
          when={<>Four text tints carry hierarchy. <b>Headings</b> aren't bigger — they're <b>--tasty-text-primary</b> at weight 600 (see Type). Meta, captions, and IDs drop to <b>--tasty-text-muted</b>.</>}>
          <Stage variant="solo column" style={{ gap: 10 }}>
            <div style={{ fontSize: 13, fontWeight: 600, color: "var(--tasty-text-primary)" }}>--tasty-text-primary — headings, active labels, terminal foreground</div>
            <div style={{ fontSize: 13, color: "var(--tasty-text-secondary)" }}>--tasty-text-secondary — body labels, descriptions, settings rows</div>
            <div style={{ fontSize: 13, color: "var(--tasty-text-muted)" }}>--tasty-text-muted — meta, captions, hints, monospace IDs</div>
            <div style={{ fontSize: 13, color: "var(--tasty-text-disabled)" }}>--tasty-text-disabled — disabled controls and rows</div>
            <Input placeholder="--tasty-text-placeholder — empty field hint" style={{ width: 320 }} />
          </Stage>
          <Meta tokens={[
            { tok: "--tasty-text-primary", use: "headings, active", color: "var(--tasty-text-primary)" },
            { tok: "--tasty-text-secondary", use: "body, labels", color: "var(--tasty-text-secondary)" },
            { tok: "--tasty-text-muted", use: "meta, IDs", color: "var(--tasty-text-muted)" },
            { tok: "--tasty-text-disabled", use: "disabled", color: "var(--tasty-text-disabled)" },
            { tok: "--tasty-text-placeholder", use: "empty inputs", color: "var(--tasty-text-placeholder)" },
          ]} />
        </Spec>
      </Section>

      {/* ACCENTS */}
      <Section id="accents" title="Color — accent roles">
        <Spec title="Accents map to roles, not decoration"
          when={<>Each accent owns one meaning. The standout rule: <b>mauve = AI agent</b>. Anything agent-driven is tinted mauve so user-vs-agent stays visible at a glance. Don't reach for an accent for looks — reach for the role.</>}>
          <Stage variant="solo column" style={{ gap: 0, padding: "8px 26px" }}>
            <AccentRow tok="--tasty-accent-primary" color="var(--tasty-accent-primary)" role="primary actions, focus ring"
              demo={<Button variant="primary" size="sm">Save</Button>} />
            <AccentRow tok="--tasty-accent-info" color="var(--tasty-accent-info)" role="informational, neutral status"
              demo={<Tag variant="accent" dot>info</Tag>} />
            <AccentRow tok="--tasty-accent-success" color="var(--tasty-accent-success)" role="running / ok / success"
              demo={<StatusDot status="running" pulse label="running" />} />
            <AccentRow tok="--tasty-accent-warning" color="var(--tasty-accent-warning)" role="warnings, validation"
              demo={<Tag variant="warning" dot>warning</Tag>} />
            <AccentRow tok="--tasty-accent-attention" color="var(--tasty-accent-attention)" role="needs-attention notice (plugins, occupied) — peach, NOT yellow"
              demo={<Tag variant="attention" dot>attention</Tag>} />
            <AccentRow tok="--tasty-accent-danger" color="var(--tasty-accent-danger)" role="destructive, errors"
              demo={<Button variant="danger" size="sm">Force detach</Button>} />
            <AccentRow tok="--tasty-accent-agent" color="var(--tasty-accent-agent)" role="AI-agent surfaces & actions"
              demo={<><StatusDot status="agent" pulse /><Tag variant="agent">agent</Tag></>} />
          </Stage>
          <Meta
            specs={[["fill text", <span className="tok">--tasty-text-on-accent</span>], ["focus ring", <>2px <span className="tok">--tasty-accent-primary</span></>], ["agent rule", "mauve = always agent"]]}
            tokens={[
              { tok: "--tasty-accent-primary", use: "blue", color: "var(--tasty-accent-primary)" },
              { tok: "--tasty-accent-info", use: "sky", color: "var(--tasty-accent-info)" },
              { tok: "--tasty-accent-success", use: "green", color: "var(--tasty-accent-success)" },
              { tok: "--tasty-accent-warning", use: "yellow", color: "var(--tasty-accent-warning)" },
              { tok: "--tasty-accent-attention", use: "peach", color: "var(--tasty-accent-attention)" },
              { tok: "--tasty-accent-danger", use: "red", color: "var(--tasty-accent-danger)" },
              { tok: "--tasty-accent-agent", use: "mauve", color: "var(--tasty-accent-agent)" },
            ]} />
        </Spec>
      </Section>

      {/* TERMINAL / ANSI */}
      <Section id="terminal" title="Color — terminal / ANSI palette">
        <Spec title="The colors a terminal cell paints with — not UI chrome"
          when={<>These are the <b>terminal content</b> colors the renderer writes into cells, kept deliberately separate from the accent roles above. An app's <span className="ic">SGR 32</span> green is <b>--tasty-ansi-green</b> — it is <b>not</b> <span className="tok">--tasty-accent-success</span>, even though both are green. Never wire terminal output to a UI accent, or a theme tweak to one will silently repaint the other. The 16 ANSI colors derive from the same Catppuccin palette; the four state fills are what Tasty itself paints onto cells (selection, vi cursor, search).</>}>
          <Stage variant="solo column" style={{ gap: 18, padding: "10px 26px" }}>
            {/* ANSI 16 — normal + bright, paired by hue */}
            <div style={{ display: "grid", gridTemplateColumns: "1fr 1fr", gap: "var(--tasty-space-xl)" }}>
              <AnsiCol heading="normal · SGR 30–37" rows={ANSI_NORMAL} />
              <AnsiCol heading="bright · SGR 90–97" rows={ANSI_BRIGHT} />
            </div>

            {/* terminal state fills, shown on a faux terminal line */}
            <div>
              <div style={{ fontFamily: "var(--tasty-font-mono)", fontSize: 10, textTransform: "uppercase",
                letterSpacing: ".06em", color: "var(--tasty-text-muted)", marginBottom: 8 }}>terminal state fills — painted onto cells</div>
              <div style={{ background: "#000", border: "1px solid var(--tasty-separator)", borderRadius: "var(--tasty-radius)",
                padding: "12px 14px", fontFamily: "var(--tasty-font-mono)", fontSize: 13, lineHeight: 1.8,
                color: "var(--tasty-color-neutral-1100, #cdd6f4)" }}>
                <div><span style={{ color: "var(--tasty-ansi-green)" }}>~/tasty</span> $ vi notes.md</div>
                <div>
                  The <Hi tok="--tasty-selection-bg">selected words</Hi> wrap the <Hi tok="--tasty-search-match-bg">match</Hi> and
                  the <Hi tok="--tasty-search-match-active-bg">active match</Hi> sit&nbsp;
                  <span style={{ background: "var(--tasty-vi-cursor-bg)", color: "#000", borderRadius: 1 }}>h</span>ere
                </div>
              </div>
              <div style={{ marginTop: 10 }}>
                <SemRow tok="--tasty-selection-bg" role="mouse / keyboard selection region" />
                <SemRow tok="--tasty-vi-cursor-bg" role="vi-mode block cursor" />
                <SemRow tok="--tasty-search-match-bg" role="search matches in scrollback" />
                <SemRow tok="--tasty-search-match-active-bg" role="the current / active match" />
              </div>
            </div>
          </Stage>
          <Meta
            specs={[["set", "16 ANSI (8 normal + 8 bright)"], ["source", "derived from the Catppuccin palette"], ["scope", "terminal cells — not UI chrome"], ["state fills", "selection · vi cursor · search"]]}
            tokens={[
              { tok: "--tasty-ansi-red", use: "SGR 31 red", color: "var(--tasty-ansi-red)" },
              { tok: "--tasty-ansi-green", use: "SGR 32 green", color: "var(--tasty-ansi-green)" },
              { tok: "--tasty-ansi-bright-blue", use: "SGR 94 br.blue", color: "var(--tasty-ansi-bright-blue)" },
              { tok: "--tasty-selection-bg", use: "selection fill", color: "var(--tasty-selection-bg)" },
              { tok: "--tasty-search-match-active-bg", use: "active match", color: "var(--tasty-search-match-active-bg)" },
            ]} />
          <Dont><b>Don't</b> use an ANSI color for UI chrome or an accent for terminal output — <span className="ic">ansi-green</span> and <span className="ic">accent-success</span> are different roles that only happen to share a hue.</Dont>
        </Spec>
      </Section>

      {/* TYPE */}
      <Section id="type" title="Type">
        <Spec title="Two families, hard 14px cap, hierarchy by weight"
          when={<>UI chrome uses the <b>native system sans</b>, capped at 14px — body is 13px, captions 11px. Code and terminal use <b>D2Coding</b> (the brand mono, with ligatures). Headings are body-size at weight 600; never scale UI text up for emphasis.</>}>
          <Stage variant="solo column" style={{ gap: 0 }}>
            {typeScaleRow(13, "--tasty-font-size-heading", 600, "section heading — same size as body, weight 600", "New workspace")}
            {typeScaleRow(13, "--tasty-font-size-body", 400, "default UI text, labels, menu rows", "Held by another client. Force detach to take over.")}
            {typeScaleRow(11, "--tasty-font-size-caption", 400, "meta, badges, status-bar, hints", "bash · 80×24 · s_01HX")}
            <div style={{ display: "grid", gridTemplateColumns: "150px 1fr", alignItems: "baseline", gap: 16, padding: "12px 0 0" }}>
              <div style={{ fontFamily: "var(--tasty-font-mono)", fontSize: 11.5, color: "var(--tasty-text-primary)" }}>--tasty-font-mono</div>
              <div>
                <div style={{ fontFamily: "var(--tasty-font-mono)", fontSize: 14, color: "var(--tasty-text-primary)" }}>D2Coding — git diff --stat  →  !=  ==&gt;  </div>
                <div style={{ fontSize: 11.5, color: "var(--tasty-text-muted)", marginTop: 3 }}>terminal cells, code, IDs, shortcuts — ligatures on</div>
              </div>
            </div>
          </Stage>
          <Meta
            specs={[["UI cap", <span className="tok">--tasty-font-size-max</span>], ["heading", "body size, weight 600"], ["mono", <span className="tok">--tasty-font-mono</span>]]}
            tokens={[
              { tok: "--tasty-font-ui", use: "chrome (system sans)" },
              { tok: "--tasty-font-mono", use: "D2Coding — code/term" },
              { tok: "--tasty-font-size-body", use: "13px default" },
              { tok: "--tasty-font-size-caption", use: "11px meta" },
              { tok: "--tasty-font-weight-semibold", use: "600 = heading" },
            ]} />
          <Note>Two <b>sanctioned exceptions</b> to the 14px cap, both flagged in the tokens: <span className="tok">--tasty-font-size-prose-h1</span> (20px — rendered markdown <i>content</i>, not chrome; since the egui_commonmark adoption this is the <b>Heading anchor</b> at the top of the library's interpolated heading ladder, not a per-level size) and <span className="tok">--tasty-font-size-brand-wordmark</span> (17px — the mono <b>“tasty.”</b> sidebar wordmark, a <i>branding</i> mark). Nothing else in UI chrome exceeds 14px. Markdown <b>content</b> typography is otherwise <b>library-driven</b> (size ladder + leading fixed by egui_commonmark) — see the Markdown viewer specimen.</Note>
        </Spec>
      </Section>

      {/* SPACING — the one that mattered */}
      <Section id="spacing" title="Spacing — the 4px grid, in use">
        <Spec title="Five steps, each with a job"
          when={<>The whole system snaps to a <b>4px grid</b> — there are only five spacing tokens. Below, each one is shown <b>doing its job</b>, not as an abstract bar. Turn on <b>Specs</b> to see the 4px rhythm under each example.</>}>
          <Stage variant="solo column" style={{ gap: 0, padding: "4px 26px 14px" }}>
            <SpaceUse token="--tasty-space-xs" px="4px" usedFor="icon↔label gaps, chip insets, tight inner padding">
              <div style={{ display: "inline-flex", alignItems: "center", gap: "var(--tasty-space-xs)", background: "var(--tasty-surface-raised)",
                border: "1px solid var(--tasty-border-default)", borderRadius: "var(--tasty-radius)", padding: "var(--tasty-space-xs) var(--tasty-space-sm)" }}>
                <span style={{ width: 7, height: 7, borderRadius: "50%", background: "var(--tasty-accent-success)" }} />
                <span style={{ fontSize: 12, color: "var(--tasty-text-secondary)" }}>running</span>
              </div>
            </SpaceUse>
            <SpaceUse token="--tasty-space-sm" px="8px" usedFor="gaps between sibling controls, list-row padding">
              <div style={{ display: "inline-flex", gap: "var(--tasty-space-sm)" }}>
                <Button size="sm" variant="secondary">Cancel</Button>
                <Button size="sm" variant="primary">Save</Button>
              </div>
            </SpaceUse>
            <SpaceUse token="--tasty-space-md" px="12px" usedFor="card / panel padding, toolbar insets">
              <div style={{ background: "var(--tasty-surface-raised)", border: "1px solid var(--tasty-border-default)",
                borderRadius: "var(--tasty-radius)", padding: "var(--tasty-space-md)", fontSize: 12.5, color: "var(--tasty-text-secondary)", display: "inline-block" }}>
                Card padded with --tasty-space-md on all sides
              </div>
            </SpaceUse>
            <SpaceUse token="--tasty-space-lg" px="16px" usedFor="section gaps inside dialogs, form row rhythm">
              <div style={{ display: "flex", flexDirection: "column", gap: "var(--tasty-space-lg)" }}>
                <span style={{ fontSize: 12.5, color: "var(--tasty-text-secondary)" }}>Setting group A</span>
                <span style={{ fontSize: 12.5, color: "var(--tasty-text-secondary)" }}>Setting group B</span>
              </div>
            </SpaceUse>
            <SpaceUse token="--tasty-space-xl" px="24px" usedFor="major gaps between distinct regions / columns">
              <div style={{ display: "flex", gap: "var(--tasty-space-xl)" }}>
                <div style={{ background: "var(--tasty-surface-raised)", borderRadius: "var(--tasty-radius)", padding: "6px 12px", fontSize: 12, color: "var(--tasty-text-muted)" }}>region</div>
                <div style={{ background: "var(--tasty-surface-raised)", borderRadius: "var(--tasty-radius)", padding: "6px 12px", fontSize: 12, color: "var(--tasty-text-muted)" }}>region</div>
              </div>
            </SpaceUse>
          </Stage>
          <Meta
            specs={[["grid", "4 / 8 / 12 / 16 / 24"], ["rule", "multiples of 4px only"], ["heights", "snap to grid too"]]}
            tokens={[
              { tok: "--tasty-space-xs", use: "4 · icon gaps" },
              { tok: "--tasty-space-sm", use: "8 · control gaps" },
              { tok: "--tasty-space-md", use: "12 · card pad" },
              { tok: "--tasty-space-lg", use: "16 · sections" },
              { tok: "--tasty-space-xl", use: "24 · regions" },
            ]} />
          <Dont><b>Don't</b> use values off the grid (6px, 10px, 14px). If something needs in-between, you're missing a layout, not a token.</Dont>
        </Spec>
      </Section>

      {/* RADIUS / BORDER / MOTION */}
      <Section id="shape" title="Radius · border · motion">
        <Spec title="Crisp and rectilinear — it's a terminal"
          when={<>Radius is <b>always 4px</b> (2px for tiny inner chips). Borders are <b>always 1px</b>. Motion is short UI feedback only — and <b>terminal content never animates (0ms, always)</b>.</>}>
          <Stage variant="solo" style={{ gap: 26 }}>
            <Cluster label="--tasty-radius · 4px (default)">
              <div style={{ width: 88, height: 48, background: "var(--tasty-surface-raised)", border: "1px solid var(--tasty-border-default)", borderRadius: "var(--tasty-radius)" }} />
            </Cluster>
            <Cluster label="--tasty-radius-sm · 2px (inner)">
              <div style={{ width: 88, height: 48, background: "var(--tasty-surface-raised)", border: "1px solid var(--tasty-border-default)", borderRadius: "var(--tasty-radius-sm)" }} />
            </Cluster>
            <Cluster label="--tasty-radius-pill · chips/dots">
              <Badge variant="danger">99+</Badge>
            </Cluster>
            <Cluster label="fixed heights">
              <span style={{ fontFamily: "var(--tasty-font-mono)", fontSize: 11.5, color: "var(--tasty-text-muted)", lineHeight: 1.7 }}>
                tree 22 · control 28<br />tab 24 ×150
              </span>
            </Cluster>
          </Stage>
          <Meta
            specs={[["radius", <>4px (<span className="tok">--tasty-radius</span>)</>], ["border", <>1px (<span className="tok">--tasty-border-width</span>)</>], ["UI motion", <>90–120ms <span className="tok">--tasty-ease-ui</span></>], ["terminal motion", "0ms — always"]]}
            tokens={[
              { tok: "--tasty-radius", use: "4px everywhere" },
              { tok: "--tasty-radius-sm", use: "2px inner" },
              { tok: "--tasty-motion-ui", use: "120ms feedback" },
              { tok: "--tasty-motion-term", use: "0ms — terminal" },
              { tok: "--tasty-focus-ring-width", use: "2px ring" },
            ]} />
        </Spec>
      </Section>

      {/* UI SCALE */}
      <Section id="uiscale" title="UI scale — sidebar zoom">
        <Spec title="One multiplier scales the sidebar; everything else stays fixed"
          when={<>Sidebar scale is a <b>single multiplier</b> exposed in <span className="ic">Settings › Appearance › Display</span>. Three named stops feed one active value — components read <b>only</b> <span className="tok">--tasty-ui-scale</span>, never the stops directly. It zooms the <b>sidebar</b> alone (wordmark, workspace list, footer); the <b>title bar, tabs, panes, and dialogs are untouched</b>, and the terminal keeps its own glyph size (a separate font-size axis). Bigger nav targets without disturbing the work area.</>}>
          <Stage variant="solo" grid style={{ gap: 22, alignItems: "flex-end" }}>
            {[["sm", "0.8×", "Small"], ["md", "1.0×", "Medium"], ["lg", "1.2×", "Large"]].map(([key, mult, label]) => (
              <Cluster key={key} label={`${label} · ${mult}`}>
                <div style={{ zoom: `var(--tasty-ui-scale-${key})`, display: "inline-flex", alignItems: "center", gap: "var(--tasty-space-sm)",
                  background: "var(--tasty-bg-sidebar)", border: "1px solid var(--tasty-separator)", borderRadius: "var(--tasty-radius)", padding: "6px 9px" }}>
                  <StatusDot status="idle" />
                  <span style={{ fontSize: 13, color: "var(--tasty-text-primary)" }}>agents-prod</span>
                  <Badge variant="primary">2</Badge>
                </div>
              </Cluster>
            ))}
          </Stage>
          <Meta
            specs={[["stops", <>0.8 / 1.0 / 1.2 (<span className="tok">--tasty-ui-scale-sm/md/lg</span>)</>], ["active value", <span className="tok">--tasty-ui-scale</span>], ["consumed by", "sidebar root zoom"], ["excluded", "title bar, tabs, panes, dialogs"], ["control", "Appearance › Display"]]}
            tokens={[
              { tok: "--tasty-ui-scale-sm", use: "0.8 · compact" },
              { tok: "--tasty-ui-scale-md", use: "1.0 · default" },
              { tok: "--tasty-ui-scale-lg", use: "1.2 · roomy" },
              { tok: "--tasty-ui-scale", use: "active — the only one read" },
            ]} />
          <Dont><b>Don't</b> read a named stop (<span className="ic">--tasty-ui-scale-lg</span>) in a component, and <b>don't</b> add per-component size tokens (no "sidebar = medium"). One multiplier, read in one place, the 4px grid intact.</Dont>
          <Note>Implementation: the sidebar root sets <span className="ic">zoom: var(--tasty-ui-scale)</span> with <span className="ic">height: 100%</span> so it fills top-to-bottom at any scale. Nothing else reads <span className="tok">--tasty-ui-scale</span> — the title bar, tab strip, panes, and modals render at fixed size, so terminal columns and tab hit-targets never shift.</Note>
        </Spec>
      </Section>
    </>
  );
}

window.Gallery.mount(
  "foundations",
  NAV,
  {
    title: "Foundations",
    intro: "Tasty's tokens — but never in a vacuum. Each value is shown inside the thing it builds, with a plain statement of what it's for. A token with no usage is just a number; here every one earns its place.",
    howto: true,
  },
  <Foundations />
);
