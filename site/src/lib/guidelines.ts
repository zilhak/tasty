import { readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";

/**
 * The design guidelines, read straight out of `site/vendor/guidelines/`.
 *
 * Each document opens with a `@dsCard` comment carrying the metadata the
 * design system publishes it under — the group it belongs to, its name and
 * subtitle, and the viewport it is drawn for. That comment is the manifest;
 * there is no second list to keep in step.
 */
export interface Card {
  slug: string;
  group: string;
  name: string;
  subtitle: string;
  width: number;
  height: number;
}

/** Group order on the page — broad identity first, then the token layers. */
const GROUPS = ["Brand", "Colors", "Type", "Spacing", "Tokens"];

const attr = (meta: string, key: string) =>
  (meta.match(new RegExp(`${key}="([^"]*)"`)) ?? ["", ""])[1];

export function cards(siteDir: string): Card[] {
  const dir = join(siteDir, "vendor/guidelines");
  const out: Card[] = [];
  for (const file of readdirSync(dir).sort()) {
    if (!file.endsWith(".html")) continue;
    const first = readFileSync(join(dir, file), "utf8").split("\n", 1)[0];
    const meta = first.match(/@dsCard\s+(.*?)-->/)?.[1];
    if (!meta) continue;
    const [width, height] = attr(meta, "viewport").split("x").map(Number);
    out.push({
      slug: file.replace(/\.html$/, ""),
      group: attr(meta, "group"),
      name: attr(meta, "name"),
      subtitle: attr(meta, "subtitle"),
      width: width || 700,
      height: height || 200,
    });
  }
  const rank = (c: Card) => {
    const i = GROUPS.indexOf(c.group);
    return i === -1 ? GROUPS.length : i;
  };
  return out.sort((a, b) => rank(a) - rank(b));
}

/** The cards of one group, in file order within it. */
export function byGroup(all: Card[]): { group: string; cards: Card[] }[] {
  const out: { group: string; cards: Card[] }[] = [];
  for (const c of all) {
    const last = out[out.length - 1];
    if (last && last.group === c.group) last.cards.push(c);
    else out.push({ group: c.group, cards: [c] });
  }
  return out;
}
