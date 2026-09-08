import { renderDiagram, isDiagram } from "./diagrams.jsx";

/**
 * Replaces a marked code block with the diagram it names.
 *
 * In the source:
 *
 *     <!-- tasty-diagram: window-anatomy -->
 *     ```
 *     ┌────────────┐
 *     ...
 *     ```
 *
 * The drawing stays because the guide is read on GitHub too, where none of this
 * runs; there the comment is invisible and the fence is the picture. On the site
 * the fence becomes the components the app is built from, so the picture cannot
 * drift from the product — and cannot be misaligned by a script whose glyphs are
 * two columns wide, which is what the drawings had been.
 *
 * The marker is a comment rather than a fence language because the highlighter
 * runs first and rewrites an unrecognised language to `plaintext`, taking the
 * name with it. A comment reaches this plugin intact as a raw node.
 */
const MARKER = /^<!--\s*tasty-diagram:\s*([a-z0-9-]+)\s*-->$/;

export function createDiagramPlugin(langOf, base = "") {
  return {
    name: "guide-diagrams",
    raw(node, ctx) {
      const marker = MARKER.exec(String(node.value ?? "").trim());
      if (!marker) return;
      const name = marker[1];

      const parent = ctx.parent(node);
      const index = ctx.indexOf(node);
      const target = (parent?.children ?? [])
        .slice(index + 1)
        .find((n) => n.type === "element" || (n.type === "raw" && String(n.value).trim()));

      if (!isDiagram(name) || !target) {
        ctx.report({
          message: !target
            ? `tasty-diagram: \`${name}\` has no block after it`
            : `tasty-diagram: no diagram named \`${name}\``,
          node,
          severity: "warning",
        });
        return;
      }

      ctx.replaceNode(target, { type: "raw", value: renderDiagram(name, langOf(ctx.fileURL), base) });
      ctx.removeNode(node);
    },
  };
}
