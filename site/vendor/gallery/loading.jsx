// Tasty Gallery — Chrome · Startup (boot loading screen)
// The pre-app full-window loading surface (egui render_loading, mirrors
// render_shell_setup): app-bg fill + centered brand lockup → spinner → phase
// slot. App chrome, NOT terminal content, so the spinner may spin (reduced-
// motion → static). Reuses components/feedback/Spinner.jsx (size bumped to 32)
// and the guidelines/brand-logo.html lockup verbatim.
const { Section, Spec, Stage, Meta, Note } = window.Gallery;
const { Spinner } = window.TastyDesignSystem_41fd3f;

const NAV = [
  { id: "default", label: "Default 1280×720" },
  { id: "minimal", label: "Minimal 640×480" },
  { id: "phases", label: "Phase variants" },
  { id: "spinner", label: "Spinner spec" },
  { id: "latte", label: "Latte variant" },
];

// Brand lockup — inherited verbatim from guidelines/brand-logo.html (branding
// exception to the 14px UI cap, like --tasty-font-size-brand-wordmark).
function Lockup() {
  return (
    <div style={{ display: "flex", alignItems: "center" }}>
      <img src="../assets/icons/icon_256.png" alt="tasty"
        style={{ width: 64, height: 64, imageRendering: "-webkit-optimize-contrast" }} />
      <span style={{ fontFamily: "var(--tasty-font-mono)", fontWeight: 700, fontSize: 38, letterSpacing: "-1px", marginLeft: -10, color: "var(--tasty-text-primary)" }}>
        tasty<span style={{ color: "var(--tasty-brand-melon-flesh)" }}>.</span>
      </span>
    </div>
  );
}

// A scaled true-size boot viewport. w/h are logical px; z the display scale.
function BootFrame({ w, h, z = 1, phase, showPhase = true, theme }) {
  const inner = {
    width: w, height: h, transform: z === 1 ? undefined : `scale(${z})`, transformOrigin: "top left",
  };
  const boot = {
    position: "absolute", inset: 0, background: "var(--tasty-bg-app)",
    display: "flex", flexDirection: "column", alignItems: "center", justifyContent: "center",
  };
  const wrap = {
    display: "inline-block", border: "var(--tasty-border-width) solid var(--tasty-border-strong)",
    borderRadius: "var(--tasty-radius-8)", overflow: "hidden",
  };
  const vp = { position: "relative", overflow: "hidden", width: w * z, height: h * z };
  const content = (
    <div style={inner}>
      <div style={boot}>
        <Lockup />
        <span style={{ marginTop: "var(--tasty-space-xl)", color: "var(--tasty-accent-primary)" }}>
          <Spinner size={32} stroke={3} label="Starting tasty" />
        </span>
        <div style={{ marginTop: "var(--tasty-space-lg)", height: "var(--tasty-size-16)", lineHeight: "var(--tasty-size-16)",
          fontFamily: "var(--tasty-font-ui)", fontSize: "var(--tasty-font-size-body)",
          color: showPhase ? "var(--tasty-text-muted)" : "transparent" }}>
          {showPhase ? phase : "·"}
        </div>
      </div>
    </div>
  );
  return (
    <div style={wrap}>
      <div style={vp} {...(theme ? { "data-theme": theme } : {})}>{content}</div>
    </div>
  );
}

function Page() {
  return (
    <>
      <Section id="default" title="Default — 1280 × 720 (logical)">
        <Spec title="Boot screen — full client area, centered stack"
          when={<>Shown from the very first presented GPU frame until <b>Ready</b> (GpuInit → WaitingPlugins → RestoringLayout). App-background fill, no chrome (sidebar/tabs/status don't exist yet), non-interactive, short + variable, frame-drop tolerant. Frame shown at 60%; token dimensions are real values.</>}>
          <Stage variant="solo" style={{ padding: "var(--tasty-space-lg)", background: "var(--tasty-bg-app)" }} eager>
            <BootFrame w={1280} h={720} z={0.6} phase="Loading plugins…" />
          </Stage>
          <Meta
            specs={[
              ["surface", "client area = --tasty-bg-app solid (= GPU clear color)"],
              ["stack", "lockup → (space-xl 24) → spinner → (space-lg 16) → phase slot"],
              ["lockup", "64px mark + tasty. mono 38px (brand-logo.html verbatim) · melon dot"],
              ["spinner", "32px · stroke 3 · accent-primary · track 0.22 · 900ms linear"],
              ["phase slot", "fixed --tasty-size-16 · font-size-body · text-muted"],
            ]}
            tokens={[
              { tok: "--tasty-bg-app", use: "full surface", color: "var(--tasty-bg-app)" },
              { tok: "--tasty-accent-primary", use: "spinner arc", color: "var(--tasty-accent-primary)" },
              { tok: "--tasty-brand-melon-flesh", use: "wordmark dot", color: "var(--tasty-brand-melon-flesh)" },
              { tok: "--tasty-spinner-duration", use: "900ms rotation" },
              { tok: "--tasty-space-xl / -lg", use: "stack gaps" },
            ]} />
          <Note><b>Non-interactive · frame-drop tolerant:</b> single constant-velocity arc, angle = f(time), so a 50–300ms atomic boot task (GPU init) that stalls the frame loop won't strobe or jump.</Note>
        </Spec>
      </Section>

      <Section id="minimal" title="Minimal window — 640 × 480 (true size)">
        <Spec title="Same stack, no responsive shrink"
          when={<>Minimum window size. Stack elements, gaps, and spinner size are <b>identical</b> to the default — centered placement still has ample margin, so implementation needs one centered layout regardless of window size.</>}>
          <Stage variant="solo" style={{ padding: "var(--tasty-space-lg)", background: "var(--tasty-bg-app)" }} eager>
            <BootFrame w={640} h={480} z={1} phase="Restoring layout…" />
          </Stage>
        </Spec>
      </Section>

      <Section id="phases" title="Phase variants (3) + no-text">
        <Spec title="Fixed-height phase slot — swap/omit never shifts the stack"
          when={<>Phase strings map 1:1 to the boot state machine and are confirmed in English → i18n keys verbatim (en/ko/ja). The slot always occupies --tasty-size-16 whether filled or empty, so the lockup + spinner never move; phases not all appearing (e.g. fresh install with no layout restore) is fine.</>}>
          <Stage variant="solo" style={{ display: "grid", gridTemplateColumns: "repeat(2, auto)", gap: "var(--tasty-space-lg)", justifyContent: "start", padding: "var(--tasty-space-lg)", background: "var(--tasty-bg-app)" }} eager>
            <BootFrame w={640} h={480} z={0.5} phase="Initializing graphics…" />
            <BootFrame w={640} h={480} z={0.5} phase="Loading plugins…" />
            <BootFrame w={640} h={480} z={0.5} phase="Restoring layout…" />
            <BootFrame w={640} h={480} z={0.5} showPhase={false} />
          </Stage>
          <Meta
            specs={[
              ["1 · GpuInit", "Initializing graphics…"],
              ["2 · WaitingPlugins", "Loading plugins…"],
              ["3 · RestoringLayout", "Restoring layout…"],
              ["no-text (§2-4)", "wordmark + spinner only; slot reserved, empty"],
            ]}
            tokens={[{ tok: "--tasty-size-16", use: "fixed phase-slot height" }, { tok: "--tasty-text-muted", use: "phase color", color: "var(--tasty-text-muted)" }]} />
        </Spec>
      </Section>

      <Section id="spinner" title="Spinner spec">
        <Spec title="Spinner.jsx reused — size 16 → 32 for boot hero"
          when={<>Same visual vocabulary as <code>components/feedback/Spinner.jsx</code> (thin arc on a faint track). Only the size is bumped one step to a boot-hero 32px; no new spinner invented. reduced-motion freezes the arc (Spinner's built-in static fallback).</>}>
          <Stage variant="solo" style={{ display: "flex", gap: "var(--tasty-space-xl)", padding: "var(--tasty-space-xl)", background: "var(--tasty-bg-app)", alignItems: "center" }} eager>
            <span style={{ color: "var(--tasty-accent-primary)" }}><Spinner size={32} stroke={3} label="boot spinner" /></span>
            <span style={{ color: "var(--tasty-accent-primary)", display: "inline-flex" }}>
              <span className="tasty-spinner--dots" style={{ ["--tasty-spinner-size"]: "32px", width: 32, height: 32, display: "inline-block" }}><svg /></span>
            </span>
            <span style={{ fontSize: "var(--tasty-font-size-caption)", color: "var(--tasty-text-muted)", fontFamily: "var(--tasty-font-mono)" }}>animated · reduced-motion (3-dot fallback)</span>
          </Stage>
          <Meta
            specs={[
              ["size", "32px (catalog 16 → hero) · viewBox 24"],
              ["stroke", "3px · arc round cap"],
              ["arc / track", "90° accent-primary / full-circle currentColor @0.22"],
              ["rotation", "--tasty-spinner-duration 900ms linear ∞ (constant velocity)"],
              ["reduced-motion", "arc frozen / Spinner 3-dot fallback"],
            ]}
            tokens={[
              { tok: "--tasty-spinner-indicator", use: "= accent-primary", color: "var(--tasty-accent-primary)" },
              { tok: "--tasty-spinner-track", use: "0.22 track" },
              { tok: "--tasty-spinner-duration", use: "900ms" },
            ]} />
        </Spec>
      </Section>

      <Section id="latte" title="Latte variant (theme-following)">
        <Spec title="Follows the saved theme — no dark flash for light users"
          when={<>Settings are loaded by boot time, so the loading screen adopts the user's theme. Mocha is the default (above); this Latte frame is the same tokens resolved in the light theme — a light user never sees a dark flash.</>}>
          <Stage variant="solo" style={{ padding: "var(--tasty-space-lg)", background: "var(--tasty-bg-app)" }} eager>
            <BootFrame w={640} h={480} z={0.7} phase="Loading plugins…" theme="latte" />
          </Stage>
          <Note>Same token names, light values. The <code>--tasty-bg-app</code> fill doubles as the GPU clear color, so it must be read from the resolved theme (not hard-coded dark) on the implementing side.</Note>
        </Spec>
      </Section>
    </>
  );
}

Gallery.mount("loading", NAV, {
  title: "Chrome · Startup",
  intro: "The boot loading screen shown from the first GPU frame to Ready — app-background fill, centered brand lockup + spinner + phase slot. App chrome (spinner may animate), non-interactive, frame-drop tolerant. Mirrors egui render_loading.",
  howto: true,
}, <Page />);
