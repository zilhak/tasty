import React from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { WindowAnatomy } from "../components/diagrams/WindowAnatomy.jsx";
import { SettingsAnatomy } from "../components/diagrams/SettingsAnatomy.jsx";

/**
 * Diagrams the guide can name from a fenced block.
 *
 * The markdown keeps the ASCII drawing inside the fence, because the guide is
 * also read on GitHub, where none of this runs. On the site the fence is
 * replaced by the components the app itself is built from — so the picture
 * cannot drift from the product, and it cannot be misaligned by a language
 * whose characters are two columns wide.
 */
const LABELS = {
  ko: { titlebar: "타이틀바", sidebar: "사이드바", tabstrip: "탭 스트립", work: "작업 영역", statusbar: "상태바", workspaces: "워크스페이스" },
  en: { titlebar: "Title bar", sidebar: "Sidebar", tabstrip: "Tab strip", work: "Work area", statusbar: "Status bar", workspaces: "Workspaces" },
};

const DIAGRAMS = {
  "window-anatomy": (lang) => <WindowAnatomy labels={LABELS[lang] ?? LABELS.en} />,
  "settings-window": () => <SettingsAnatomy />,
};

export const isDiagram = (name) => Object.hasOwn(DIAGRAMS, name);

/**
 * Static HTML for one diagram, or null when the name is not one of ours.
 *
 * The kit resolves the deployment base from Vite's environment, but this render
 * happens inside the markdown processor — which Astro loads from its config,
 * outside that environment, where the base always reads as "/". So the base is
 * passed in and applied here, at the seam. It still has exactly one source:
 * `astro.config.mjs` hands the same value to Astro and to this.
 */
export function renderDiagram(name, lang, base = "") {
  const make = DIAGRAMS[name];
  if (!make) return null;
  const html = renderToStaticMarkup(make(lang));
  const prefix = base.replace(/\/$/, "");
  return prefix ? html.replace(/(\s(?:src|href)=")\//g, `$1${prefix}/`) : html;
}
