import type { Lang } from "./guide";

/**
 * The design-system section's own table of contents and copy.
 *
 * The documents this section publishes come from the design system itself and
 * are written once, in English — a swatch of `--tasty-accent-primary` says the
 * same thing in either language. So the pages are bilingual in their chrome
 * and their framing, and identical in what they embed.
 */
export interface DesignPage {
  /** Route under the section, with its trailing slash. "" is the section root. */
  slug: string;
  label: { en: string; ko: string };
  desc: { en: string; ko: string };
}

export interface DesignGroup {
  group: { en: string; ko: string };
  pages: DesignPage[];
}

export const DESIGN_NAV: DesignGroup[] = [
  {
    group: { en: "Guidelines", ko: "가이드라인" },
    pages: [
      {
        slug: "",
        label: { en: "Foundations", ko: "기초" },
        desc: { en: "brand · color · type · spacing", ko: "브랜드 · 색 · 타이포 · 간격" },
      },
      {
        slug: "tokens/",
        label: { en: "Token system", ko: "토큰 체계" },
        desc: { en: "primitive → semantic → component", ko: "원시 → 의미 → 컴포넌트" },
      },
    ],
  },
];

/** Route of a design page in one language. */
export const designPath = (slug: string, lang: Lang) =>
  `${lang === "ko" ? "ko/design/" : "design/"}${slug}`;

export const pick = <T>(v: { en: T; ko: T }, lang: Lang) => (lang === "ko" ? v.ko : v.en);

/**
 * The gallery — the design system's live component catalogue.
 *
 * These pages are the gallery's own application, chrome included, so they are
 * published once rather than per language and carry their own navigation.
 * `key` is the identifier each specimen file already declares for itself.
 */
export interface GalleryPage {
  key: string;
  /** Route under `design/gallery/`; "" is the gallery root. */
  slug: string;
  label: string;
  desc: string;
}

export const GALLERY_GROUPS: { group: string; pages: GalleryPage[] }[] = [
  {
    group: "Foundations",
    pages: [
      { key: "foundations", slug: "", label: "Foundations", desc: "tokens" },
      { key: "icons", slug: "icons/", label: "Icons", desc: "glyphs" },
    ],
  },
  {
    group: "Components",
    pages: [{ key: "components", slug: "components/", label: "Components", desc: "primitives" }],
  },
  {
    group: "Overlays",
    pages: [
      { key: "overlays-dialogs", slug: "overlays-dialogs/", label: "Dialogs", desc: "modal confirms" },
      { key: "overlays-windows", slug: "overlays-windows/", label: "Windows", desc: "big surfaces" },
      { key: "overlays-popups", slug: "overlays-popups/", label: "Popups & menus", desc: "anchored" },
      { key: "overlays-banners", slug: "overlays-banners/", label: "Banners", desc: "floating" },
      { key: "overlays-tutorial", slug: "overlays-tutorial/", label: "Tutorial", desc: "marker · guide" },
    ],
  },
  {
    group: "Layouts",
    pages: [{ key: "layouts", slug: "layouts/", label: "Layouts", desc: "shells" }],
  },
  {
    group: "Surfaces",
    pages: [{ key: "dag", slug: "dag/", label: "Task DAG", desc: "graph surface" }],
  },
  {
    group: "Chrome",
    pages: [{ key: "loading", slug: "loading/", label: "Startup", desc: "boot screen" }],
  },
  {
    group: "Plugins",
    pages: [{ key: "plugins", slug: "plugins/", label: "Plugin surfaces", desc: "surfaces" }],
  },
];

export const GALLERY_PAGES: GalleryPage[] = GALLERY_GROUPS.flatMap((g) => g.pages);

/** Route of a gallery page. The gallery is published once, not per language. */
export const galleryPath = (slug: string) => `design/gallery/${slug}`;

/** The gallery nav as plain data, so it can cross into the React island. */
export const galleryNav = (url: (p: string) => string) =>
  GALLERY_GROUPS.map((g) => ({
    group: g.group,
    pages: g.pages.map((p) => ({ key: p.key, label: p.label, desc: p.desc, href: url(galleryPath(p.slug)) })),
  }));
