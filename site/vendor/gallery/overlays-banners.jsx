// Tasty Gallery — Overlays · Banners (the 4th overlay family)
// One of the four Overlays sub-pages. Frame components are shared from
// overlays-shared.jsx (window.OverlaysShared); this file holds only the
// page's specimens + nav. See the other overlays-*.jsx for the rest.
const { Section, Spec, Stage, Meta, Note, Do, Dont, GIcon } = window.Gallery;
const { IconButton, Kbd, Tag } = window.TastyDesignSystem_41fd3f;
const { BannerScope, MouseCaptureBannerG, MouseCaptureHitZone, BlacklistEditorG, BannerShellG, TtlBannerG, StackDemoG, BannerMoreMenuG, BannerMoreDemoG, ic } = window.OverlaysShared;

const NAV = [
  { id: "banner", label: "Banner" },
  { id: "mousecapture", label: "Mouse capture" },
  { id: "more", label: "Banner more menu" },
  { id: "blacklist", label: "Capture blacklist" },
];

function Page() {
  return (
    <>
<Section id="banner" title="Banner — the floating top notice">
        <Spec title="Banner shell — the 4th overlay family"
          when={<>After Modal / Popup / Toast, <b>Banner</b> is the fourth overlay concept. It floats at the <b>top of a scope's content area</b> (View / Workspace / Pane / Tab / Surface) and <b>never covers the tab bar</b> — full width minus <b>8px</b> side margins, <b>8px</b> top margin, <b>no bottom margin</b>, <b>8px</b> radius, variable height. Unlike a Toast it <b>consumes its own mouse</b> and can carry <b>actions</b>; unlike a Popup it takes <b>no keyboard focus</b> and has no titlebar/drag. Fired only by <b>direct user action</b> — never from agent/IPC. The example below is the first use: a TUI mouse-capture notice.</>}>
          <Stage variant="solo center" style={{ padding: 20, background: "var(--tasty-bg-app)" }}>
            <BannerScope height={250}><MouseCaptureBannerG /></BannerScope>
          </Stage>
          <Meta
            specs={[["family", "Modal · Popup · Toast · Banner"], ["position", "top of content, below the tab bar"], ["width", "100% − 16px (8px each side)"], ["margin", "8 top / 8 sides · 0 bottom"], ["radius", <>8px (<span className="tok">--tasty-banner-radius</span>)</>], ["height", "variable — per banner"], ["fires on", "user action only (never IPC/agent)"]]}
            tokens={[{ tok: "--tasty-banner-bg", use: "surface0 fill", color: "var(--tasty-banner-bg)" }, { tok: "--tasty-banner-border", use: "1px edge", color: "var(--tasty-banner-border)" }, { tok: "--tasty-banner-shadow", use: "floating lift" }, { tok: "--tasty-banner-margin", use: "8px top/sides" }]} />
          <Do><b>Do</b> reach for a Banner when there's an <b>action</b> to attach (Popup = independent feature, Banner = info + action, Toast = info only). If the message is short and action-less, use a <b>Toast</b> instead.</Do>
          <Note>There is <b>no</b> Info/Warning/Error kind — each banner's <b>id is its kind</b>, and it styles its own leading glyph/severity. These tokens are only the shared shell.</Note>
        </Spec>
        <Spec title="Dismiss & TTL countdown — top-right, one slot"
          when={<>The top-right corner is an <b>affordance column</b> holding <b>one</b> dismiss slot (a banner that carries a ⋯ overflow trigger adds it to the left of that slot — see <i>Banner more menu</i>). A plain banner's <b>×</b> is <b>hidden until you hover</b> the banner. A <b>TTL</b> banner shows a <b>seconds countdown</b> there instead, flipping to the × on hover; at <b>0</b> it auto-dismisses. The countdown <b>pauses</b> while the banner is hovered or its scope is backgrounded, then resumes. Appear/dismiss is a <b>120ms alpha fade</b> — the banner never moves.</>}>
          <Stage variant="solo center" style={{ flexDirection: "column", gap: 16, padding: 20, background: "var(--tasty-bg-app)" }}>
            <div style={{ width: "100%", maxWidth: 480 }}>
              <div style={{ fontSize: 11, color: "var(--tasty-text-muted)", marginBottom: 8 }}>no TTL — hover the banner to reveal ×</div>
              <BannerShellG><div style={{ display: "flex", alignItems: "center", gap: 12, padding: "var(--tasty-banner-padding-y) var(--tasty-banner-padding-x)" }}>
                <span style={{ flex: 1, fontSize: 13, color: "var(--tasty-text-secondary)" }}>Plain banner — × appears on hover (top-right)</span>
                <IconButton size="sm" aria-label="Dismiss">{ic.x}</IconButton></div></BannerShellG>
            </div>
            <div style={{ width: "100%", maxWidth: 480 }}>
              <div style={{ fontSize: 11, color: "var(--tasty-text-muted)", marginBottom: 8 }}>TTL 6s — live countdown · hover to pause + show ×</div>
              <TtlBannerG />
            </div>
          </Stage>
          <Meta
            specs={[["slot", "top-right · one affordance"], ["plain", "× hidden → reveal on hover"], ["TTL", "seconds count → × on hover"], ["expiry", "0 → auto-dismiss"], ["pause", "hover · backgrounded scope"], ["motion", "120ms alpha fade, no move"]]}
            tokens={[{ tok: "--tasty-banner-countdown-fg", use: "seconds", color: "var(--tasty-banner-countdown-fg)" }, { tok: "--tasty-banner-countdown-font", use: "mono digits" }, { tok: "--tasty-banner-fade", use: "120ms appear/dismiss" }]} />
        </Spec>
        <Spec title="Queue & stacking — one per scope, rest wait"
          when={<>A scope shows <b>one</b> banner at a time; others <b>queue</b> (max <b>5</b>). Re-firing the <b>shown</b> id resets its countdown; a <b>queued</b> duplicate id is ignored; a full queue drops new banners. When banners from <b>different scopes</b> coexist, the higher scope (<b>View &gt; Workspace &gt; Pane &gt; Tab &gt; Surface</b>) sits in front at a higher z-index, and the lower one is <b>dimmed to ~40% opacity</b> behind it — only its overhang shows.</>}>
          <Stage variant="solo center" style={{ padding: 24, background: "var(--tasty-bg-app)" }}>
            <StackDemoG />
          </Stage>
          <Meta
            specs={[["per scope", "1 shown + up to 5 queued"], ["dup (shown)", "resets the countdown"], ["dup (queued)", "ignored"], ["queue full", "new banner dropped"], ["z-order", "View > Workspace > Pane > Tab > Surface"], ["recessed", "lower scope → 40% opacity"]]}
            tokens={[{ tok: "--tasty-banner-recessed-opacity", use: "dimmed lower banner", color: "var(--tasty-banner-bg)" }, { tok: "--tasty-banner-bg", use: "both shells", color: "var(--tasty-banner-bg)" }]} />
          <Note>The interactive manager (spawn / queue / background-pause / dismiss-pops-next) lives in the kit specimen <span className="ic">ui_kits/terminal/overlays/banner.html</span>.</Note>
        </Spec>
      </Section>

      <Section id="mousecapture" title="Mouse capture — the first banner instance">
        <Spec title="Anatomy & states"
          when={<>The canonical use: a TUI app turns on mouse tracking (DECSET 1000/1002/1003), so drag-to-select and right-click stop working. The banner explains <b>why</b> and the <b>bypass</b>. It is <b>persistent</b> (no TTL — it doesn't time out), fires <b>once per tracking session</b> and only on a <b>real user click</b> (never from agent/IPC). The top-right × is hidden until you hover; dismissing it suppresses the banner for that session. Leading glyph = a mouse (open decision resolved: keep it, in <span className="tok">--tasty-banner-icon-fg</span> tone). Card height is <b>variable</b> — body wraps to 1–3 lines depending on locale.</>}>
          <Stage variant="solo center" style={{ flexDirection: "column", gap: 16, padding: 20, background: "var(--tasty-bg-app)" }}>
            <div style={{ width: "100%", maxWidth: 480 }}>
              <div style={{ fontSize: 11, color: "var(--tasty-text-muted)", marginBottom: 8 }}>default · hover the card to reveal ×</div>
              <MouseCaptureBannerG />
            </div>
            <div style={{ width: "100%", maxWidth: 480 }}>
              <div style={{ fontSize: 11, color: "var(--tasty-text-muted)", marginBottom: 8 }}>one-line body (short locale)</div>
              <MouseCaptureBannerG body="Mouse captured — hold Shift to select." />
            </div>
            <div style={{ width: "100%", maxWidth: 480 }}>
              <div style={{ fontSize: 11, color: "var(--tasty-text-muted)", marginBottom: 8 }}>recessed — dimmed behind a higher-scope banner</div>
              <div style={{ opacity: "var(--tasty-banner-recessed-opacity)" }}><MouseCaptureBannerG /></div>
            </div>
          </Stage>
          <Meta
            specs={[["title", <>"Mouse input captured" · 13/600 <span className="tok">--tasty-banner-fg</span></>], ["body", <>caption · <span className="tok">--tasty-text-muted</span> · 1–3 lines</>], ["glyph", <>mouse · <span className="tok">--tasty-banner-icon-fg</span></>], ["affordance", "⋯ + × on hover (no TTL / countdown)"], ["fires", "once per tracking session · user click only"], ["dismiss", "suppresses for the session"]]}
            tokens={[{ tok: "--tasty-banner-bg", use: "card fill", color: "var(--tasty-banner-bg)" }, { tok: "--tasty-banner-icon-fg", use: "mouse glyph", color: "var(--tasty-banner-icon-fg)" }, { tok: "--tasty-banner-recessed-opacity", use: "dimmed variant" }]} />
          <Note>The bypass keys (<span className="ic">Shift+drag</span> / <span className="ic">Shift+Right-click</span>) are the message — keep them in the body. <b>No inline action buttons</b>: the body stays text-only, and the two per-app opt-outs live behind the <b>⋯</b> trigger next to the × (see <i>Banner more menu</i> below).</Note>
        </Spec>
        <Spec title="Position & hit-zone — card consumes, body passes through"
          when={<>The banner floats at the <b>top of the surface content area, below the tab bar</b> (never over it), full width minus <b>8px</b> side margins. Crucially, the <b>card rect</b> — not the whole scope — is the mouse-consume + hover zone. Clicks on the <b>surface body below the card pass through to the capturing app</b>, so mouse reporting keeps working everywhere except the card itself.</>}>
          <Stage variant="solo center" style={{ padding: 20, background: "var(--tasty-bg-app)" }}>
            <MouseCaptureHitZone />
          </Stage>
          <Meta
            specs={[["anchor", "surface content top, under tab bar"], ["margins", "8 top / 8 sides · 0 bottom"], ["consume zone", "the drawn card rect only"], ["below card", "pass-through to the app"], ["focus", "never steals keyboard focus"], ["inactive surface", "first click only focuses the surface"]]}
            tokens={[{ tok: "--tasty-banner-margin", use: "8px gap", color: "var(--tasty-banner-bg)" }, { tok: "--tasty-accent-info", use: "consume-zone label", color: "var(--tasty-accent-info)" }]} />
        </Spec>
      </Section>

      <Section id="more" title="Banner more menu — per-app opt-outs behind ⋯">
        <Spec title="⋯ trigger — second slot in the affordance column"
          when={<>The mouse-capture banner's two per-app opt-outs are <b>not</b> inline buttons — they sit behind a <b>⋯ trigger</b> in the banner's top-right affordance column, <b>left of the ×</b> with a <b>4px</b> gap. Both share the <b>same reveal rule</b>: hidden until the banner is hovered (this banner has no TTL, so no countdown competes for the slot), and the ⋯ additionally <b>stays lit while its menu is open</b>. The card's width and height are unchanged — only the body's right reserve grows from 28 to <b>56px</b>, so a 1-line body still fits on one line.</>}>
          <Stage variant="solo center" style={{ flexDirection: "column", gap: 16, padding: 20, background: "var(--tasty-bg-app)" }}>
            <div style={{ width: "100%", maxWidth: 480 }}>
              <div style={{ fontSize: 11, color: "var(--tasty-text-muted)", marginBottom: 8 }}>idle — no affordance drawn</div>
              <MouseCaptureBannerG />
            </div>
            <div style={{ width: "100%", maxWidth: 480 }}>
              <div style={{ fontSize: 11, color: "var(--tasty-text-muted)", marginBottom: 8 }}>banner hovered — ⋯ then ×</div>
              <MouseCaptureBannerG forceHover />
            </div>
            <div style={{ width: "100%", maxWidth: 480 }}>
              <div style={{ fontSize: 11, color: "var(--tasty-text-muted)", marginBottom: 8 }}>menu open — trigger stays lit (active)</div>
              <MouseCaptureBannerG open />
            </div>
          </Stage>
          <Meta
            specs={[["trigger", <>IconButton <b>ghost / sm</b> · 24px · <span className="ic">more</span> glyph</>], ["placement", "affordance column, left of ×"], ["gap", "4px between ⋯ and ×"], ["reveal", "banner hover — same rule as ×"], ["open state", "active (accent) while menu is open"], ["body reserve", "28 → 56px right padding"], ["card size", "unchanged"]]}
            tokens={[{ tok: "--tasty-banner-more-column-gap", use: "4px ⋯ ↔ ×" }, { tok: "--tasty-banner-more-reserve", use: "56px body right padding" }, { tok: "--tasty-overlay-active", use: "open-trigger wash", color: "var(--tasty-overlay-active)" }, { tok: "--tasty-accent-primary", use: "open-trigger glyph", color: "var(--tasty-accent-primary)" }]} />
          <Note>New glyph <span className="ic">more</span> (<code>icons/more.svg</code> — three dots, horizontal, same 2px round-cap language as <code>M12 17h.01</code> style glyphs). A unicode <b>⋯</b> was rejected: the system renders every affordance through the <span className="ic">Icon</span> component so it inherits <code>currentColor</code> and the 16px sm size.</Note>
          <Dont><b>Don't</b> keep the trigger permanently visible. A persistent banner with a permanently drawn ⋯ reads as a second dismiss control and adds chrome to a notice that is already at the top of live output — hover is where every other banner affordance lives.</Dont>
        </Spec>
        <Spec title="The menu — 2 rows, anchored under the trigger"
          when={<>Clicking ⋯ opens a headless menu <b>4px below the trigger</b>, <b>right-aligned</b> to it (the trigger sits at the card's right edge, so left-aligning would overflow), at a higher z-order than the banner. It <b>flips above</b> the trigger when the viewport can't fit it below. Same anchored-popup contract as the Tools menu: <b>no scrim</b>, outside click · <Kbd keys="Esc" /> dismiss, ↑↓ move, <Kbd keys="↵" /> select. Selecting a row <b>acts immediately and closes</b> the menu — to run the other one, reopen ⋯.</>}>
          <Stage variant="solo center" style={{ padding: 20, background: "var(--tasty-bg-app)" }}>
            <BannerMoreDemoG />
          </Stage>
          <Meta
            specs={[["rows", "2 × 28px MenuItem, leading icon"], ["anchor", "4px below ⋯, right-aligned"], ["overflow", "flip above the trigger"], ["width", "min 200 · max 288 (elastic)"], ["scrim", "none"], ["dismiss", <>outside click · <span className="ic">Esc</span> · row select</>], ["keyboard", "↑↓ · ↵ · Esc"]]}
            tokens={[{ tok: "--tasty-banner-more-menu-bg", use: "menu fill", color: "var(--tasty-banner-more-menu-bg)" }, { tok: "--tasty-banner-more-menu-border", use: "edge", color: "var(--tasty-banner-more-menu-border)" }, { tok: "--tasty-banner-more-menu-shadow", use: "popover lift" }, { tok: "--tasty-banner-more-menu-offset", use: "4px below trigger" }, { tok: "--tasty-banner-more-menu-min-width", use: "200px floor" }, { tok: "--tasty-banner-more-menu-max-width", use: "288px ceiling" }]} />
          <Note>Both rows write a <b>permanent</b> Settings entry, so the copy says <b>off / disable</b>, never “pause”: <code>Turn off this notice for {"{app}"}</code> → <code>mouse_capture_banner_blacklist</code> (capture stays delegated, only the notice is suppressed) · <code>Disable mouse capture for {"{app}"}</code> → <code>mouse_capture_blacklist</code> (tasty takes the mouse back). “Turn off this notice” also <b>closes the banner it was opened from</b> — suppressing the notice and leaving it on screen would contradict itself. “Disable mouse capture” leaves the banner up (capture is already released; the user dismisses when done reading). Icons: <span className="ic">bell</span> for the notice row, <span className="ic">mouse</span> for the capture row.</Note>
          <Note>While the menu is open the banner is treated as <b>hover-equivalent</b> — a TTL banner would keep its countdown paused, and the affordance column stays drawn even if the pointer has left the card.</Note>
        </Spec>
        <Spec title="Elastic width — the interpolated program name"
          when={<>Every row interpolates a program name of arbitrary length, and word order differs by locale (en: name last; ko/ja: name first). So a row's label is <b>two parts</b>: the <b>fixed text</b> (never truncates) and the <b>program name</b> in <b>mono / text-primary</b>, which is the part that <b>shrinks and ellipsises</b>. The menu grows with the content between <b>200px</b> and <b>288px</b> — the fixed Tools-menu 160px is too narrow for these strings — and the full name is available as the row's tooltip.</>}>
          <Stage variant="solo center" style={{ padding: 20, background: "var(--tasty-bg-app)", gap: 18, alignItems: "flex-start", flexWrap: "wrap" }}>
            <div style={{ display: "flex", flexDirection: "column", gap: 6 }}>
              <div style={{ fontSize: 11, color: "var(--tasty-text-muted)" }}>short name — at the 200px floor</div>
              <BannerMoreMenuG app="vim" />
            </div>
            <div style={{ display: "flex", flexDirection: "column", gap: 6 }}>
              <div style={{ fontSize: 11, color: "var(--tasty-text-muted)" }}>medium — grows to fit</div>
              <BannerMoreMenuG app="python3.11" hovered={0} />
            </div>
            <div style={{ display: "flex", flexDirection: "column", gap: 6 }}>
              <div style={{ fontSize: 11, color: "var(--tasty-text-muted)" }}>long — capped at 288px, name ellipsises</div>
              <BannerMoreMenuG app="some-very-long-tool-name" />
            </div>
            <div style={{ display: "flex", flexDirection: "column", gap: 6 }}>
              <div style={{ fontSize: 11, color: "var(--tasty-text-muted)" }}>rejected — danger tone on the capture row</div>
              <div style={{ opacity: 0.75 }}><BannerMoreMenuG app="vim" danger /></div>
            </div>
          </Stage>
          <Meta
            specs={[["label", "fixed text + app name (2 spans)"], ["truncates", "the app name only"], ["app name", <>mono · <span className="tok">--tasty-text-primary</span></>], ["width", "content-sized, 200 ≤ w ≤ 288"], ["wrap", "never — 1 line per row"], ["tooltip", "full program name on the row"], ["tone", "both rows neutral (no danger)"]]}
            tokens={[{ tok: "--tasty-banner-more-app-font", use: "mono program name" }, { tok: "--tasty-banner-more-app-fg", use: "program name tone", color: "var(--tasty-banner-more-app-fg)" }, { tok: "--tasty-menu-item-height", use: "28px rows" }, { tok: "--tasty-accent-danger", use: "rejected variant only", color: "var(--tasty-accent-danger)" }]} />
          <Do><b>Do</b> keep the program name as the emphasised, mono part of the row — it is the one thing the user must verify before writing a permanent per-app setting, and mono marks it as a process name rather than prose.</Do>
          <Dont><b>Don't</b> paint the “Disable mouse capture” row in <b>danger</b> tone (last card above, shown for the record). Nothing is destroyed and nothing is lost: the setting is reversible from Settings › Terminal, and it <i>returns</i> the mouse to tasty. Danger in this system means delete — spending it here would make the two rows look like different classes of action when they are the same class at two strengths.</Dont>
        </Spec>
      </Section>

      <Section id="blacklist" title="Mouse-capture blacklist (Settings › Terminal)">
        <Spec title="Per-program capture opt-out — list editor"
          when={<>The companion setting: a list of process-name patterns where mouse capture is <b>disabled</b>, so clicks/drags are handled locally (select / tasty menu) instead of being sent to the app. Promoted from the old newline textarea to a <b>list editor</b> (rows + Add) — resolved open decision. Grouped with the <b>Show mouse-capture hint</b> switch since both govern the same feature. Default is an <b>empty list</b>. Remove uses <span className="ic">×</span> (it un-lists a program, it doesn't delete anything).</>}>
          <Stage variant="solo center" style={{ padding: 20, background: "var(--tasty-bg-app)", gap: 18, flexWrap: "wrap" }}>
            <div style={{ display: "flex", flexDirection: "column", gap: 6 }}>
              <div style={{ fontSize: 11, color: "var(--tasty-text-muted)" }}>filled · one row hovered</div>
              <BlacklistEditorG />
            </div>
            <div style={{ display: "flex", flexDirection: "column", gap: 6 }}>
              <div style={{ fontSize: 11, color: "var(--tasty-text-muted)" }}>empty (default) · Add disabled</div>
              <BlacklistEditorG empty />
            </div>
          </Stage>
          <Meta
            specs={[["lives in", "Settings › Terminal"], ["group", "hint switch + blacklist (1 section)"], ["row", "28px · pattern (mono) + × remove"], ["add", "Input + Add button (disabled when empty)"], ["match", "case-insensitive substring or * wildcard"], ["default", "empty list"]]}
            tokens={[{ tok: "--tasty-overlay-hover", use: "row hover (8%)" }, { tok: "--tasty-accent-warning", use: "match-rule notice", color: "var(--tasty-accent-warning)" }, { tok: "--tasty-input-bg", use: "add field", color: "var(--tasty-input-bg)" }]} />
          <Note>The wheel is always forwarded to the program even for blacklisted apps — only clicks/drags are intercepted. A blacklisted foreground app also suppresses the capture banner on that surface.</Note>
        </Spec>
      </Section>
    </>
  );
}

window.Gallery.mount(
  "overlays-banners",
  NAV,
  {
    title: "Banners",
    intro: "The 4th overlay family (Modal · Popup · Toast · Banner). A floating, top-anchored, focus-less notice pinned to the top of a scope’s content area — never over the tab bar. Unlike a Toast it consumes its own mouse and can carry actions; unlike a Popup it takes no keyboard focus.",
    howto: false,
  },
  <Page />
);
