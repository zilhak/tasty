/**
 * Search index: one row per page, the shape src/scripts/site.js expects.
 * `h` is the page's plain text, trimmed — enough to match on without shipping
 * the whole guide to the browser.
 */
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { SECTIONS, entryIdFor, titleIn } from "./guide";
import type { Page, Lang } from "./guide";
import { pathOf } from "./urls";

/** Markdown stripped down to searchable words. */
function plain(markdown: string): string {
  return markdown
    .replace(/^<!--[\s\S]*?-->/, "")
    .replace(/```[\s\S]*?```/g, " ")
    .replace(/`[^`]*`/g, " ")
    .replace(/!\[[^\]]*\]\([^)]*\)/g, " ")
    .replace(/\[([^\]]*)\]\([^)]*\)/g, "$1")
    .replace(/<!--[\s\S]*?-->/g, " ")
    .replace(/[#>*_|-]+/g, " ")
    .replace(/\s+/g, " ")
    .trim();
}

export function searchIndex(root: string, pages: Page[], lang: Lang) {
  return pages.map((page) => {
    const id = entryIdFor(page, lang);
    const file = join(root, "content", `${id === "index" ? "index" : id}.md`);
    let text = "";
    try {
      text = plain(readFileSync(file, "utf8"));
    } catch {
      // `<dir>` ids come from `<dir>/index.md`.
      try {
        text = plain(readFileSync(join(root, "content", `${id}/index.md`), "utf8"));
      } catch {
        text = "";
      }
    }
    return {
      t: titleIn(page, lang),
      u: pathOf(page, lang),
      c: SECTIONS[page.section][lang],
      h: text.slice(0, 600),
    };
  });
}
