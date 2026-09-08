import { readFileSync } from "node:fs";

/**
 * The latest release, as CI hands it over.
 *
 * `pages.yml` writes `site/release.json` with `gh release view --json
 * tagName,assets` before the build. A local build has no such file, and that is
 * not an error — the download buttons then point at the releases page, which is
 * correct at any moment and merely one click longer.
 */
export type Platform = "macos" | "windows" | "linux";

export interface Download {
  platform: Platform;
  /** Shown on the button: "Apple silicon · .dmg". */
  label: string;
  /** Direct asset URL, or undefined when the release has no asset for it. */
  href?: string;
}

/** Which asset belongs to which platform, by file-name suffix. */
const ASSETS: ReadonlyArray<readonly [Platform, string, string]> = [
  ["macos", "-macos-arm64.dmg", "Apple silicon · .dmg"],
  ["windows", "-windows-x64.msi", "x64 · .msi"],
  ["linux", "-x86_64.AppImage", "x86_64 · AppImage"],
];

export const RELEASES_URL = "https://github.com/zilhak/tasty/releases/latest";

interface Manifest {
  tag?: string;
  assets: Map<string, string>;
}

function load(siteDir: string): Manifest {
  let raw: string;
  try {
    raw = readFileSync(`${siteDir}/release.json`, "utf8");
  } catch {
    return { assets: new Map() };
  }
  try {
    const json = JSON.parse(raw) as { tagName?: string; assets?: { name?: string; url?: string }[] };
    const assets = new Map<string, string>();
    for (const a of json.assets ?? []) if (a.name && a.url) assets.set(a.name, a.url);
    return { tag: json.tagName, assets };
  } catch (e) {
    console.warn(`site/release.json is not valid JSON, ignoring it: ${e}`);
    return { assets: new Map() };
  }
}

export function downloads(siteDir: string): { tag?: string; items: Download[] } {
  const { tag, assets } = load(siteDir);
  const items = ASSETS.map(([platform, suffix, label]) => {
    const hit = [...assets].find(([name]) => name.endsWith(suffix));
    return { platform, label, href: hit?.[1] };
  });
  return { tag, items };
}
