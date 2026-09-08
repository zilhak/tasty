import type { Page, Lang } from "./guide";
import { prefixOf } from "./guide";

/**
 * Route slug for a page. `index` is a section's own landing, so it drops out of
 * the path: `index` -> ``, `plugins/index` -> `plugins`.
 */
export const slugOf = (rel: string) =>
  rel === "index" ? "" : rel.replace(/\/index$/, "");

/** Site-relative path of a page in one language, with a trailing slash. */
export const pathOf = (page: Page, lang: Lang) => {
  const slug = slugOf(page.rel);
  return slug ? `${prefixOf(lang)}/${slug}/` : `${prefixOf(lang)}/`;
};
