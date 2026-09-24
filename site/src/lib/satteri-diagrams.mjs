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
 * Keep the ASCII drawing for GitHub readers. On the site, replace it with
 * a diagram made from the vendored design kit.
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
