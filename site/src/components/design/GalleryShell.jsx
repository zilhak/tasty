import React from "react";
import { HowTo, SpecsToggle, ThemeToggle, readSpecs, applySpecs } from "../../gallery/shell.jsx";

/**
 * The gallery's chrome, as this site publishes it.
 *
 * The gallery is a whole application, not a page: a left rail of categories, a
 * top bar with the theme and spec toggles, and a scrolling body of specimens.
 * So it keeps its own shell here rather than being poured into the site's
 * documentation layout — the class names below are the ones `gallery.css`
 * styles. What changes is the navigation, which points at this site's routes,
 * and the theme, which is the site's rather than the gallery's own.
 */

/* The gallery's toggle drove a key of its own. Here the page theme is the
   site's, so the same control writes the key the rest of the site reads and
   the choice survives a walk back into the guide. */
const KEY = "tasty-theme";

/* Where the site is served from — a project page mounts it under the repository
   name. `astro.config.mjs` `base` is the only place that path is written. */
const BASE = `${import.meta.env.BASE_URL.replace(/\/$/, "")}/`;

function read() {
  try {
    const v = localStorage.getItem(KEY);
    if (v === "latte" || v === "light") return "latte";
    if (v) return "mocha";
  } catch (e) { /* private mode */ }
  return matchMedia("(prefers-color-scheme: light)").matches ? "latte" : "mocha";
}

function apply(theme) {
  const root = document.documentElement;
  if (theme === "latte") root.setAttribute("data-theme", "latte");
  else root.removeAttribute("data-theme");
  try { localStorage.setItem(KEY, theme); } catch (e) { /* private mode */ }
}

export function GalleryShell({ groups, active, head, brandHref, backLabel, children }) {
  const [theme, setTheme] = React.useState("mocha");
  const [specs, setSpecs] = React.useState(false);

  // Read after mount: the pre-paint script has already put the right theme on
  // the document, and reading it during render would disagree with the server.
  React.useEffect(() => { setTheme(read()); setSpecs(readSpecs()); }, []);
  React.useEffect(() => { apply(theme); }, [theme]);
  React.useEffect(() => {
    applySpecs(specs);
    try { localStorage.setItem("tasty-gallery-specs", specs ? "1" : "0"); } catch (e) { /* private mode */ }
  }, [specs]);

  const group = groups.find((g) => g.pages.some((p) => p.key === active));
  const page = (group && group.pages.find((p) => p.key === active)) || groups[0].pages[0];

  return (
    <div className="g-app">
      <div className="g-brand">
        <a href={brandHref} title={backLabel}>
          <img src={`${BASE}tasty-icon.png`} alt="" />
          <span className="word">tasty<span className="dot">.</span></span>
        </a>
        <span className="tagline">gallery</span>
      </div>

      <div className="g-top">
        <div className="g-crumb">
          {group && group.group !== page.label && <span className="crumb-group">{group.group} · </span>}
          <b>{page.label}</b>
        </div>
        <div className="spacer" />
        <a className="g-back" href={brandHref}>← {backLabel}</a>
        <SpecsToggle on={specs} onChange={setSpecs} />
        <ThemeToggle theme={theme} onChange={setTheme} />
      </div>

      <nav className="g-nav">
        {groups.map((grp) => (
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
        {head.nav && head.nav.length > 0 && (
          <div className="g-nav-group">
            <div className="g-nav-h">On this page</div>
            {head.nav.map((n) => (
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
