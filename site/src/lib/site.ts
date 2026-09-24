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

/** Join a site path to the configured deployment base. */
export const url = (path: string) =>
  `${import.meta.env.BASE_URL.replace(/\/$/, "")}/${path.replace(/^\//, "")}`;
