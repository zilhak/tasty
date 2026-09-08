import { readFileSync } from "node:fs";
import { join } from "node:path";

export const REPO = "https://github.com/zilhak/tasty";

/** Crate version of the app itself, from the workspace root Cargo.toml. */
export function appVersion(root: string): string {
  const manifest = readFileSync(join(root, "..", "Cargo.toml"), "utf8");
  let inPackage = false;
  for (const line of manifest.split("\n")) {
    const t = line.trim();
    if (t.startsWith("[")) inPackage = t === "[package]";
    else if (inPackage && t.startsWith("version = ")) return t.slice(10).trim().replace(/"/g, "");
  }
  return "0.0.0";
}

/**
 * Site-absolute URL.
 *
 * The configured `base` is the only place the deployment path is written, and
 * this is the only place it is joined to anything — so a project page
 * (`/tasty/`) and a custom domain (`/`) differ by one line of config and
 * nothing else. Astro reports the base without a trailing slash, so the join
 * normalises rather than concatenating.
 */
export const url = (path: string) =>
  `${import.meta.env.BASE_URL.replace(/\/$/, "")}/${path.replace(/^\//, "")}`;
