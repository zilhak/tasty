/**
 * Stamps a translation with the hash of the Korean source it was made from.
 *
 *   node scripts/stamp.mjs content/en/using/terminal.md
 *
 * Run it right after actually updating a translation. Stamping without updating
 * only hides the stale banner — the content stays out of date.
 */
import { readFileSync, writeFileSync } from "node:fs";
import { relative, resolve } from "node:path";
import { contentHash } from "../src/lib/guide.ts";

const root = resolve(import.meta.dirname, "..");
const targets = process.argv.slice(2);
if (targets.length === 0) {
  console.error("usage: node scripts/stamp.mjs <content/en/…md> [more…]");
  process.exit(2);
}

const PREFIX = "<!-- source-hash:";

for (const arg of targets) {
  const abs = resolve(root, arg);
  const rel = relative(resolve(root, "content/en"), abs);
  if (rel.startsWith("..")) {
    console.error(`${arg} is not under content/en/`);
    process.exit(1);
  }
  const sourcePath = resolve(root, "content", rel);
  let source;
  try {
    source = readFileSync(sourcePath, "utf8");
  } catch {
    console.error(`no Korean source at content/${rel}`);
    process.exit(1);
  }
  const hash = contentHash(source);
  const stampLine = `${PREFIX} ${hash} -->`;

  const body = readFileSync(abs, "utf8");
  const lines = body.split("\n");
  const first = lines.findIndex((l) => l.trim() !== "");
  const hasStamp = first >= 0 && lines[first].trim().startsWith(PREFIX);
  const out = hasStamp
    ? [...lines.slice(0, first), stampLine, ...lines.slice(first + 1)].join("\n")
    : `${stampLine}\n${body}`;
  writeFileSync(abs, out);
  console.log(`stamped content/en/${rel} -> ${hash}`);
}
