import React from "react";
import { WorkflowLayout } from "../components/diagrams/WorkflowLayout.jsx";
import { renderToStaticMarkup } from "react-dom/server";
import { WindowAnatomy } from "../components/diagrams/WindowAnatomy.jsx";
import { SettingsAnatomy } from "../components/diagrams/SettingsAnatomy.jsx";

/** Replace marked code blocks with diagrams from the vendored kit.
 * The ASCII drawings remain in Markdown for readers on GitHub. */
const LABELS = {
  ko: { titlebar: "타이틀바", sidebar: "사이드바", tabstrip: "탭 스트립", work: "작업 영역", statusbar: "상태바", workspaces: "워크스페이스" },
  en: { titlebar: "Title bar", sidebar: "Sidebar", tabstrip: "Tab strip", work: "Work area", statusbar: "Status bar", workspaces: "Workspaces" },
};

const DIAGRAMS = {
  "workflow-layout": (lang) => <WorkflowLayout lang={lang} />,
  "background-task": (lang) => <WorkflowLayout lang={lang} background />,
  "window-anatomy": (lang) => <WindowAnatomy labels={LABELS[lang] ?? LABELS.en} />,
  "settings-window": () => <SettingsAnatomy />,
};

export const isDiagram = (name) => Object.hasOwn(DIAGRAMS, name);

/** Return static diagram HTML, or null for an unknown name. The Markdown
 * processor runs outside Vite, so Astro config supplies the deployment base. */
export function renderDiagram(name, lang, base = "") {
  const make = DIAGRAMS[name];
  if (!make) return null;
  const html = renderToStaticMarkup(make(lang));
  const prefix = base.replace(/\/$/, "");
  return prefix ? html.replace(/(\s(?:src|href)=")\//g, `$1${prefix}/`) : html;
}
