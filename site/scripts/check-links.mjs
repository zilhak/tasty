/**
 * Checks every internal link and `#anchor` in the built site.
 *
 * The previous generator did this while rendering (`--strict`). Doing it on the
 * output instead is stricter: it sees the final HTML, so a link broken by a
 * layout or a component is caught too, not just one written in markdown.
 *
 *   node scripts/check-links.mjs [dist]
 */
import { readdirSync, statSync, readFileSync, existsSync } from "node:fs";
import { join, resolve, dirname, relative } from "node:path";
import { BASE } from "../src/lib/base.mjs";

// Imported, not repeated: a base changed in one place and forgotten here would
// make this pass on links the deployed site cannot serve.
const base = BASE.replace(/\/$/, "");

const dist = resolve(process.argv[2] ?? "dist");
if (!existsSync(dist)) {
  console.error(`no build at ${dist} — run npm run build first`);
  process.exit(2);
}

const htmlFiles = [];
(function walk(dir) {
  for (const name of readdirSync(dir)) {
    const p = join(dir, name);
    if (statSync(p).isDirectory()) walk(p);
    else if (name.endsWith(".html")) htmlFiles.push(p);
  }
})(dist);

/** id="…" of every element, so `#anchor` targets can be verified. */
const anchorsOf = (html) =>
  new Set([...html.matchAll(/\sid="([^"]+)"/g)].map((m) => m[1]));

const anchors = new Map(htmlFiles.map((f) => [f, anchorsOf(readFileSync(f, "utf8"))]));

/** Where a site path lands on disk, following the directory build format. */
const targetOf = (path) => {
  const clean = path.replace(/[?#].*$/, "");
  // A site-absolute link carries the base; the build output does not.
  if (base && !(clean === base || clean.startsWith(`${base}/`))) return null;
  const p = join(dist, clean.slice(base.length));
  if (existsSync(p) && statSync(p).isDirectory()) return join(p, "index.html");
  if (existsSync(p)) return p;
  if (existsSync(`${p}.html`)) return `${p}.html`;
  return null;
};

let broken = 0;
let checked = 0;
for (const file of htmlFiles) {
  const html = readFileSync(file, "utf8");
  const here = relative(dist, file);
  for (const m of html.matchAll(/(?:href|src)="([^"]+)"/g)) {
    const raw = m[1];
    if (/^(https?:|mailto:|data:|#|\/\/)/.test(raw)) {
      // A bare fragment must exist on this very page.
      if (raw.startsWith("#") && raw.length > 1) {
        checked++;
        if (!anchors.get(file).has(decodeURIComponent(raw.slice(1)))) {
          broken++;
          console.error(`  ${here}: ${raw}`);
        }
      }
      continue;
    }
    checked++;
    const abs = raw.startsWith("/")
      ? raw
      : `${base}/${relative(dist, resolve(dirname(file), raw))}`;
    const target = targetOf(abs);
    if (!target) {
      broken++;
      console.error(`  ${here}: ${raw}`);
      continue;
    }
    const hash = raw.includes("#") ? decodeURIComponent(raw.slice(raw.indexOf("#") + 1)) : null;
    if (hash && !anchors.get(target)?.has(hash)) {
      broken++;
      console.error(`  ${here}: ${raw} (no #${hash} in target)`);
    }
  }
}

console.log(`${htmlFiles.length} pages, ${checked} links checked, ${broken} broken`);
process.exit(broken > 0 ? 1 : 0);
