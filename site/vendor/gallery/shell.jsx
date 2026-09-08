// ============================================================
// Tasty Gallery — shared shell + specimen primitives.
// Every category page loads this, then calls Gallery.mount(...).
// Exposes a small vocabulary so each specimen reads the same:
//   <Spec> = one catalogued thing
//     · title + "when to use" note   (usage)
//     · <Stage>  the live, interactive demo
//     · <Meta>   layout spec (dimensions) + tokens it consumes
// The "Specs" toggle reveals 4px-grid overlays + dimension rails.
// ============================================================
const { useState, useEffect } = React;

// Catalog is grouped so the left nav stays legible as it grows (e.g. the four
// Overlays sub-pages, and a future "Screens" group for per-implementation designs).
const PAGE_GROUPS = [
  { group: "Foundations", pages: [
    { key: "foundations", label: "Foundations", href: "index.html", desc: "tokens" },
    { key: "icons",       label: "Icons",       href: "icons.html",  desc: "glyphs" },
  ] },
  { group: "Components", pages: [
    { key: "components",  label: "Components",  href: "components.html", desc: "primitives" },
  ] },
  { group: "Overlays", pages: [
    { key: "overlays-dialogs", label: "Dialogs",        href: "overlays-dialogs.html", desc: "modal confirms" },
    { key: "overlays-windows", label: "Windows",        href: "overlays-windows.html", desc: "big surfaces" },
    { key: "overlays-popups",  label: "Popups & menus", href: "overlays-popups.html",  desc: "anchored" },
    { key: "overlays-banners", label: "Banners",        href: "overlays-banners.html", desc: "floating" },
    { key: "overlays-tutorial", label: "Tutorial",       href: "overlays-tutorial.html", desc: "marker · guide" },
  ] },
  { group: "Layouts", pages: [
    { key: "layouts",     label: "Layouts",     href: "layouts.html", desc: "shells" },
  ] },
  { group: "Surfaces", pages: [
    { key: "dag",         label: "Task DAG",    href: "dag.html", desc: "graph surface" },
  ] },
  { group: "Chrome", pages: [
    { key: "loading",     label: "Startup",     href: "loading.html", desc: "boot screen" },
  ] },
  { group: "Plugins", pages: [
    { key: "plugins",     label: "Plugin surfaces", href: "plugins.html", desc: "surfaces" },
  ] },
];
const PAGES = PAGE_GROUPS.flatMap((g) => g.pages);

const THEME_KEY = "tasty-gallery-theme";
const SPECS_KEY = "tasty-gallery-specs";

function readTheme() { try { return localStorage.getItem(THEME_KEY) || "mocha"; } catch { return "mocha"; } }
function readSpecs() { try { return localStorage.getItem(SPECS_KEY) === "1"; } catch { return false; } }

function applyTheme(t) {
  document.documentElement.setAttribute("data-theme", t === "latte" ? "latte" : "mocha");
  if (t !== "latte") document.documentElement.removeAttribute("data-theme");
  if (t === "latte") document.documentElement.setAttribute("data-theme", "latte");
}
function applySpecs(on) {
  if (on) document.body.setAttribute("data-specs", "");
  else document.body.removeAttribute("data-specs");
}

// ── Top bar segmented controls ──────────────────────────────
function ThemeToggle({ theme, onChange }) {
  return (
    <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
      <span className="g-seglabel">Theme</span>
      <div className="g-seg" role="group" aria-label="Theme">
        <button aria-pressed={theme === "mocha"} onClick={() => onChange("mocha")}>
          <span className="swatch" style={{ background: "#1e1e2e" }} />Mocha
        </button>
        <button aria-pressed={theme === "latte"} onClick={() => onChange("latte")}>
          <span className="swatch" style={{ background: "#eff1f5" }} />Latte
        </button>
      </div>
    </div>
  );
}

function SpecsToggle({ on, onChange }) {
  return (
    <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
      <span className="g-seglabel">Specs</span>
      <div className="g-seg" role="group" aria-label="Spec overlay">
        <button aria-pressed={!on} onClick={() => onChange(false)}>Off</button>
        <button aria-pressed={on} onClick={() => onChange(true)}>
          <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"
            strokeLinecap="round" strokeLinejoin="round"><path d="M3 3h18v18H3z" /><path d="M3 9h18M3 15h18M9 3v18M15 3v18" /></svg>
          On
        </button>
      </div>
    </div>
  );
}

// ── The shell ───────────────────────────────────────────────
function Shell({ active, nav, head, children }) {
  const [theme, setTheme] = useState(readTheme());
  const [specs, setSpecs] = useState(readSpecs());

  useEffect(() => { applyTheme(theme); try { localStorage.setItem(THEME_KEY, theme); } catch {} }, [theme]);
  useEffect(() => { applySpecs(specs); try { localStorage.setItem(SPECS_KEY, specs ? "1" : "0"); } catch {} }, [specs]);

  const activeGroup = PAGE_GROUPS.find((g) => g.pages.some((p) => p.key === active));
  const activePage = (activeGroup && activeGroup.pages.find((p) => p.key === active)) || PAGES[0];

  return (
    <div className="g-app">
      <div className="g-brand">
        <img src="../assets/icons/icon_256.png" alt="" />
        <span className="word">tasty<span className="dot">.</span></span>
        <span className="tagline">gallery</span>
      </div>

      <div className="g-top">
        <div className="g-crumb">{activeGroup && activeGroup.group !== activePage.label && <span className="crumb-group">{activeGroup.group} · </span>}<b>{activePage.label}</b></div>
        <div className="spacer" />
        <SpecsToggle on={specs} onChange={setSpecs} />
        <ThemeToggle theme={theme} onChange={setTheme} />
      </div>

      <nav className="g-nav">
        {PAGE_GROUPS.map((grp) => (
          <div className="g-nav-group" key={grp.group}>
            <div className="g-nav-h">{grp.group}</div>
            {grp.pages.map((p) => (
              <a key={p.key} className={"g-nav-link" + (p.key === active ? " active" : "")} href={p.href}>
                <span className="lbl">{p.label}</span>
                <span className="desc">{p.desc}</span>
              </a>
            ))}
          </div>
        ))}
        {nav && nav.length > 0 && (
          <div className="g-nav-group">
            <div className="g-nav-h">On this page</div>
            {nav.map((n) => (
              <a key={n.id} className="g-anchor" href={"#" + n.id}>{n.label}</a>
            ))}
          </div>
        )}
      </nav>

      <main className="g-main">
        <div className="g-page">
          <header className="g-pagehead">
            <h1>{head.title}</h1>
            <p>{head.intro}</p>
            {head.howto && <HowTo />}
          </header>
          {children}
        </div>
      </main>
    </div>
  );
}

function HowTo() {
  return (
    <div className="g-howto">
      <div>
        <div className="k">Usage</div>
        <div className="v">Every specimen leads with <b>when to use it</b> — not just what it looks like.</div>
      </div>
      <div>
        <div className="k">Tokens used</div>
        <div className="v">Each demo lists the exact tokens it consumes and <b>why</b>. No orphan values.</div>
      </div>
      <div>
        <div className="k">Specs toggle</div>
        <div className="v">Flip <b>Specs → On</b> (top right) to overlay the 4px grid and dimension rails.</div>
      </div>
    </div>
  );
}

// ── Composable specimen primitives ──────────────────────────
function Section({ id, title, children }) {
  return (
    <section className="g-section" id={id}>
      <h2>{title}</h2>
      {children}
    </section>
  );
}

function Spec({ id, title, badges, when, children }) {
  return (
    <div className="spec" id={id}>
      <div className="spec-head">
        <h3>{title}{badges}</h3>
        {when && <p className="when">{when}</p>}
      </div>
      {children}
    </div>
  );
}

// Lazy: a live demo only mounts once it scrolls near the viewport, so a page
// with 20+ specimens renders ~3-4 stages at a time instead of all at once.
// Robust by design: a synchronous rect-check on mount handles above-the-fold
// (and environments where IntersectionObserver never fires); a capture-phase
// scroll listener reveals the rest (the gallery scrolls inside .g-main, not
// window, so we listen on the capture phase to catch any scroller).
const LAZY_MARGIN = 700;
function useNearViewport(ref, eager) {
  const [shown, setShown] = useState(eager);
  useEffect(() => {
    if (shown) return;
    const el = ref.current;
    if (!el) return;
    const near = () => {
      const r = el.getBoundingClientRect();
      const vh = window.innerHeight || document.documentElement.clientHeight;
      return r.top < vh + LAZY_MARGIN && r.bottom > -LAZY_MARGIN;
    };
    if (near()) { setShown(true); return; }
    let io = null;
    if (typeof IntersectionObserver !== "undefined") {
      io = new IntersectionObserver((es) => {
        if (es.some((e) => e.isIntersecting)) { setShown(true); cleanup(); }
      }, { rootMargin: LAZY_MARGIN + "px 0px" });
      io.observe(el);
    }
    let last = 0;
    const onScroll = () => {
      const now = Date.now();
      if (now - last < 120) return;
      last = now;
      if (near()) { setShown(true); cleanup(); }
    };
    const cleanup = () => {
      if (io) io.disconnect();
      window.removeEventListener("scroll", onScroll, true);
      window.removeEventListener("resize", onScroll);
    };
    window.addEventListener("scroll", onScroll, true);
    window.addEventListener("resize", onScroll);
    return cleanup;
  }, [shown]);
  return shown;
}

function Stage({ variant = "", grid = false, children, style, eager = false, minHeight = 220 }) {
  const ref = React.useRef(null);
  const shown = useNearViewport(ref, eager);
  return (
    <div ref={ref} className={"stage " + variant} style={shown ? style : { ...style, minHeight }}>
      {shown ? (<>{grid && <div className="gridover" />}{children}</>) : null}
    </div>
  );
}

function Cluster({ label, children }) {
  return (
    <div className="cluster">
      {label && <span className="lbl">{label}</span>}
      <div className="row">{children}</div>
    </div>
  );
}

// specs: [[label, valueNode], …]   tokens: [{tok, use, color}]
function Meta({ specs, tokens }) {
  const cols = (specs ? 1 : 0) + (tokens ? 1 : 0);
  return (
    <div className="meta" style={cols === 1 ? { gridTemplateColumns: "1fr" } : undefined}>
      {specs && (
        <div>
          <div className="mh">Layout spec</div>
          <dl className="dl">
            {specs.map(([k, v], i) => (<React.Fragment key={i}><dt>{k}</dt><dd>{v}</dd></React.Fragment>))}
          </dl>
        </div>
      )}
      {tokens && (
        <div>
          <div className="mh">Tokens used</div>
          <div className="chips">
            {tokens.map((t, i) => (
              <span className="chip" key={i}>
                {t.color && <span className="sw" style={{ background: t.color }} />}
                <b>{t.tok}</b>{t.use && <span className="use">— {t.use}</span>}
              </span>
            ))}
          </div>
        </div>
      )}
    </div>
  );
}

function Note({ children }) { return <p className="note">{children}</p>; }
function Do({ children }) { return <p className="do">{children}</p>; }
function Dont({ children }) { return <p className="dont">{children}</p>; }

// inline icon (lucide-style 2px) — shared by pages
function GIcon({ d, size = 16, fill }) {
  return (
    <svg width={size} height={size} viewBox="0 0 24 24" fill={fill || "none"}
      stroke={fill ? "none" : "currentColor"} strokeWidth="2" strokeLinecap="round"
      strokeLinejoin="round" style={{ display: "block" }}>{d}</svg>
  );
}

function mount(active, nav, head, content) {
  ReactDOM.createRoot(document.getElementById("root")).render(
    <Shell active={active} nav={nav} head={head}>{content}</Shell>
  );
}

window.Gallery = { mount, Section, Spec, Stage, Cluster, Meta, Note, Do, Dont, GIcon, PAGES, PAGE_GROUPS };
