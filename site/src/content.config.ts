import { defineCollection } from "astro:content";
import { glob } from "astro/loaders";

/**
 * The user guide. Korean (`content/**`) is canonical; `content/en/**` mirrors it
 * path for path. Both live in one collection — the `en/` prefix on an entry id is
 * what separates a translation from its source (src/lib/guide.ts).
 */
const guide = defineCollection({
  loader: glob({ pattern: "**/*.md", base: "./content" }),
});

export const collections = { guide };
