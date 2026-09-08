/**
 * Guide model: reading order, sections, and the translation rules.
 *
 * Korean is canonical and English is a translation of it. A page with no
 * translation is still published in the English tree with the Korean body and a
 * banner, so that tree is always complete. Each translation carries a
 * `<!-- source-hash: … -->` stamp of the Korean source it was made from; when the
 * source moves on, the page says so. (docs/dev-guide/site.md "번역 모델")
 */
import { createHash } from "node:crypto";
import { readFileSync } from "node:fs";
import { join } from "node:path";

export type Lang = "ko" | "en";

/** Sidebar sections in reading order. The empty key is the guide home. */
export const SECTIONS: { dir: string; ko: string; en: string }[] = [
  { dir: "", ko: "가이드", en: "Guide" },
  { dir: "getting-started", ko: "시작하기", en: "Getting started" },
  { dir: "using", ko: "사용하기", en: "Using Tasty" },
  { dir: "customize", ko: "맞춤 설정", en: "Customize" },
  { dir: "agents", ko: "AI 에이전트", en: "AI agents" },
  { dir: "remote", ko: "원격", en: "Remote" },
  { dir: "plugins", ko: "플러그인", en: "Plugins" },
  { dir: "help", ko: "도움말", en: "Help" },
];

/**
 * Reading order. The sidebar and prev/next follow this list, not file names — the
 * order is an editorial decision, so it is written down. A content file missing
 * from it is a build error, so a new page is always placed deliberately.
 */
export const ORDER: string[] = [
  "index",
  "getting-started/install",
  "getting-started/first-look",
  "using/workspaces",
  "using/panes-tabs-splits",
  "using/terminal",
  "using/files",
  "customize/keybindings",
  "customize/settings",
  "customize/themes",
  "customize/scripts",
  "agents/cli",
  "agents/claude-codex",
  "agents/tasks",
  "agents/hooks-notifications",
  "remote/attach",
  "plugins/index",
  "help/troubleshooting",
];

export const TRANSLATION_DIR = "en";
const STAMP_PREFIX = "<!-- source-hash:";
const CONTENT_DIR = "content";

/** Short content hash. Line endings are normalised so a CRLF checkout does not
 *  invalidate every stamp. */
export function contentHash(source: string): string {
  return createHash("sha256").update(source.replace(/\r\n/g, "\n")).digest("hex").slice(0, 12);
}

/** The stamp on a translation's first non-empty line, if any. */
export function readStamp(source: string): string | null {
  for (const line of source.split("\n")) {
    const t = line.trim();
    if (!t) continue;
    if (!t.startsWith(STAMP_PREFIX)) return null;
    return t.slice(STAMP_PREFIX.length).replace("-->", "").trim();
  }
  return null;
}

/** First `# heading` of a document. */
export function titleOf(source: string, fallback: string): string {
  for (const line of source.split("\n")) {
    const t = line.trim();
    if (t.startsWith("# ")) return t.slice(2).trim();
  }
  return fallback;
}

/** Sidebar label: the title without a trailing parenthetical or dash clause. */
export function navLabel(title: string, rel: string): string {
  let label = title;
  const paren = label.indexOf(" (");
  if (paren >= 2) label = label.slice(0, paren);
  const dash = label.indexOf(" — ");
  if (dash >= 0) label = label.slice(0, dash);
  label = label.trim();
  return label || rel;
}

export type Page = {
  /** Path under content/, without extension: `using/terminal`. */
  rel: string;
  section: number;
  title: string;
  navLabel: string;
  /** Hash of the Korean source; a translation is stamped with it. */
  sourceHash: string;
  translation: { title: string; navLabel: string; stamp: string | null } | null;
};

const read = (root: string, rel: string): string | null => {
  try {
    return readFileSync(join(root, CONTENT_DIR, `${rel}.md`), "utf8");
  } catch {
    return null;
  }
};

/** Builds the page list in reading order. `root` is the site directory. */
export function buildPages(root: string): Page[] {
  return ORDER.map((rel) => {
    const source = read(root, rel);
    if (source === null) throw new Error(`ORDER lists ${rel}, but content/${rel}.md is missing`);
    const dir = rel.includes("/") ? rel.slice(0, rel.lastIndexOf("/")) : "";
    const section = SECTIONS.findIndex((s) => s.dir === dir);
    if (section < 0) throw new Error(`content/${rel}.md sits in a directory with no sidebar section`);

    const title = titleOf(source, rel);
    const translated = read(root, `${TRANSLATION_DIR}/${rel}`);
    const tTitle = translated ? titleOf(translated, title) : null;

    return {
      rel,
      section,
      title,
      navLabel: navLabel(title, rel),
      sourceHash: contentHash(source),
      translation: translated
        ? { title: tTitle!, navLabel: navLabel(tTitle!, rel), stamp: readStamp(translated) }
        : null,
    };
  });
}

/** How a page stands in one language. */
export type PageState = "canonical" | "translated" | "stale" | "untranslated";

export function stateOf(page: Page, lang: Lang): PageState {
  if (lang === "ko") return "canonical";
  if (!page.translation) return "untranslated";
  return page.translation.stamp === page.sourceHash ? "translated" : "stale";
}

/**
 * The collection entry id that holds the body to render. Falls back to the Korean
 * source so the English tree is complete even without a translation.
 *
 * The glob loader folds `<dir>/index.md` into `<dir>`; only the tree root keeps
 * the name `index`.
 */
export function entryIdFor(page: Page, lang: Lang): string {
  const path =
    lang === "en" && page.translation ? `${TRANSLATION_DIR}/${page.rel}` : page.rel;
  return path === "index" ? path : path.replace(/\/index$/, "");
}

export const titleIn = (p: Page, lang: Lang) =>
  lang === "en" && p.translation ? p.translation.title : p.title;
export const navLabelIn = (p: Page, lang: Lang) =>
  lang === "en" && p.translation ? p.translation.navLabel : p.navLabel;

/** URL prefix of a language tree. English is the default (`/guide/`). */
export const prefixOf = (lang: Lang) => (lang === "en" ? "guide" : "ko/guide");
