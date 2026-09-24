import { fileURLToPath } from "node:url";
import { posix, relative, resolve, sep } from "node:path";

/**
 * Rewrite Markdown links to guide routes or repository files on GitHub.
 * Keep guide links relative so untranslated bodies work under both languages.
 * Compute paths between routes: using/terminal.md becomes using/terminal/,
 * while plugins/index.md becomes plugins/.
 */
const BLOB_BASE = "https://github.com/zilhak/tasty/blob/main";

const toPosix = (p) => (sep === "/" ? p : p.split(sep).join("/"));

/** Route of a content file, relative to the guide root and without the `en/` tree prefix. */
function routeOf(absFile, contentRoot) {
  let rel = toPosix(relative(contentRoot, absFile)).replace(/\.md$/, "");
  if (rel.startsWith("en/")) rel = rel.slice(3);
  else if (rel === "en") rel = "";
  return rel.replace(/(^|\/)index$/, "$1").replace(/\/$/, "");
}

export function createGuideLinkPlugin(contentRoot, repoRoot) {
  return {
    name: "guide-md-links",
    element: {
      filter: ["a"],
      visit(node, ctx) {
        const href = node.properties?.href;
        if (typeof href !== "string" || !ctx.fileURL) return;
        if (!href || /^[a-z][a-z0-9+.-]*:/i.test(href) || href.startsWith("/") || href.startsWith("#")) return;

        const hashAt = href.indexOf("#");
        const path = hashAt === -1 ? href : href.slice(0, hashAt);
        const hash = hashAt === -1 ? "" : href.slice(hashAt);
        if (!path.endsWith(".md")) return;

        const self = fileURLToPath(ctx.fileURL);
        const target = resolve(self, "..", path);

        // Anything the content tree doesn't serve is a file, not a page.
        const inTree = target === contentRoot || target.startsWith(contentRoot + sep);
        if (!inTree) {
          ctx.setProperty(node, "href", `${BLOB_BASE}/${toPosix(relative(repoRoot, target))}${hash}`);
          return;
        }

        const from = routeOf(self, contentRoot);
        const to = routeOf(target, contentRoot);
        const rel = posix.relative(from, to);
        ctx.setProperty(node, "href", (rel === "" ? "./" : `${rel}/`) + hash);
      },
    },
  };
}
