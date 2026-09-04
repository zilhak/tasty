//! The HTML document shell: `<head>`, header bar, footer, and the page chrome
//! shared by the landing pages and every docs page.

use crate::md::html_escape;

pub const REPO: &str = "https://github.com/zilhak/tasty";

/// User-facing strings, one set per site language.
pub struct Strings {
    pub lang: &'static str,
    pub dir_docs: &'static str,
    pub nav_guide: &'static str,
    pub nav_install: &'static str,
    pub nav_download: &'static str,
    pub nav_changelog: &'static str,
    pub search_placeholder: &'static str,
    pub search_empty: &'static str,
    pub copy: &'static str,
    pub copied: &'static str,
    pub toc_title: &'static str,
    pub prev: &'static str,
    pub next: &'static str,
    pub edit_page: &'static str,
    pub skip_to_content: &'static str,
    pub toggle_theme: &'static str,
    pub toggle_nav: &'static str,
    pub footer_blurb: &'static str,
    pub footer_docs: &'static str,
    pub footer_project: &'static str,
    pub footer_resources: &'static str,
    pub footer_releases: &'static str,
    pub footer_issues: &'static str,
    pub footer_license: &'static str,
    pub footer_third_party: &'static str,
    pub footer_keybindings: &'static str,
    pub footer_plugins: &'static str,
    pub footer_dev_docs: &'static str,
    pub footer_note: &'static str,
    /// Translation-state banners (shown on the English docs tree only).
    pub untranslated: &'static str,
    pub stale: &'static str,
    pub translate_cta: &'static str,
}

pub const KO: Strings = Strings {
    lang: "ko",
    dir_docs: "가이드",
    nav_guide: "가이드",
    nav_install: "설치",
    nav_download: "다운로드",
    nav_changelog: "변경 이력",
    search_placeholder: "가이드 검색  /",
    search_empty: "검색 결과가 없습니다",
    copy: "복사",
    copied: "복사됨",
    toc_title: "이 페이지",
    prev: "이전",
    next: "다음",
    edit_page: "GitHub 에서 이 문서 보기",
    skip_to_content: "본문으로 건너뛰기",
    toggle_theme: "테마 전환",
    toggle_nav: "탐색 열기",
    footer_blurb: "AI 코딩 에이전트를 위해 만든 크로스 플랫폼 GPU 가속 터미널.",
    footer_docs: "가이드",
    footer_project: "프로젝트",
    footer_resources: "자료",
    footer_releases: "릴리스",
    footer_issues: "이슈",
    footer_license: "라이선스",
    footer_third_party: "서드파티 라이선스",
    footer_keybindings: "단축키",
    footer_plugins: "플러그인",
    footer_dev_docs: "개발 문서 (GitHub)",
    footer_note: "MIT 라이선스",
    untranslated: "이 문서는 아직 번역되지 않았습니다. 한국어 원문을 표시합니다.",
    stale: "이 번역은 한국어 원문보다 오래됐습니다. 내용이 다를 수 있습니다.",
    translate_cta: "원문 보기",
};

pub const EN: Strings = Strings {
    lang: "en",
    dir_docs: "Guide",
    nav_guide: "Guide",
    nav_install: "Install",
    nav_download: "Download",
    nav_changelog: "Changelog",
    search_placeholder: "Search the guide  /",
    search_empty: "No results",
    copy: "copy",
    copied: "copied",
    toc_title: "On this page",
    prev: "Previous",
    next: "Next",
    edit_page: "View this page on GitHub",
    skip_to_content: "Skip to content",
    toggle_theme: "Toggle theme",
    toggle_nav: "Open navigation",
    footer_blurb: "A cross-platform, GPU-accelerated terminal built for AI coding agents.",
    footer_docs: "Guide",
    footer_project: "Project",
    footer_resources: "Resources",
    footer_releases: "Releases",
    footer_issues: "Issues",
    footer_license: "License",
    footer_third_party: "Third-party licenses",
    footer_keybindings: "Keyboard shortcuts",
    footer_plugins: "Plugins",
    footer_dev_docs: "Developer docs (GitHub)",
    footer_note: "MIT licensed",
    untranslated: "This page has not been translated yet — showing the Korean original.",
    stale: "This translation is older than the Korean source and may be out of date.",
    translate_cta: "View the source",
};

pub struct Shell<'a> {
    pub strings: &'a Strings,
    /// `<title>` text, without the site suffix.
    pub title: String,
    pub description: String,
    /// Relative prefix back to the site root, e.g. `"../../"` (empty at the root).
    pub root: String,
    /// Rendered `<body>` inner HTML.
    pub body: String,
    /// Header nav item to mark as current: `"guide"`, `"install"`, or `""`.
    pub active: &'a str,
    /// Where the KO / EN switch should point, relative to the site root.
    pub ko_href: String,
    pub en_href: String,
    /// Guide tree this page belongs to, relative to the site root (`guide/` or `ko/guide/`).
    pub docs_prefix: &'a str,
    /// Search index for that tree, relative to the site root.
    pub search_index: &'a str,
}

pub fn document(shell: &Shell<'_>) -> String {
    let s = shell.strings;
    let root = &shell.root;
    let full_title = if shell.title.is_empty() {
        "Tasty".to_string()
    } else {
        format!("{} · Tasty", shell.title)
    };

    format!(
        r##"<!DOCTYPE html>
<html lang="{lang}" data-theme-default="dark">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>{title}</title>
<meta name="description" content="{desc}">
<meta name="color-scheme" content="dark light">
<meta property="og:title" content="{title}">
<meta property="og:description" content="{desc}">
<meta property="og:type" content="website">
<link rel="icon" href="{root}assets/tasty-melon.svg" type="image/svg+xml">
<link rel="stylesheet" href="{root}assets/style.css?v={asset_tag}">
<script>
/* Applied before first paint so the theme never flashes. */
(function () {{
  try {{
    var saved = localStorage.getItem("tasty-theme");
    if (saved === "light" || saved === "dark") {{
      document.documentElement.setAttribute("data-theme", saved);
      return;
    }}
  }} catch (e) {{}}
  if (window.matchMedia("(prefers-color-scheme: light)").matches) {{
    document.documentElement.setAttribute("data-theme", "light");
  }}
}})();
</script>
</head>
<body>
<a class="visually-hidden" href="#content">{skip}</a>
{header}
{body}
{footer}
<script src="{root}assets/site.js?v={asset_tag}" defer></script>
</body>
</html>
"##,
        lang = s.lang,
        title = html_escape(&full_title),
        desc = html_escape(&shell.description),
        asset_tag = crate::asset_tag(),
        root = root,
        skip = html_escape(s.skip_to_content),
        header = header(shell),
        body = shell.body,
        footer = footer(shell),
    )
}

fn header(shell: &Shell<'_>) -> String {
    let s = shell.strings;
    let root = &shell.root;
    let mark = |name: &str| {
        if shell.active == name {
            " aria-current=\"page\""
        } else {
            ""
        }
    };
    let landing_href = if s.lang == "ko" {
        format!("{root}ko/")
    } else {
        root.clone()
    };

    format!(
        r##"<header class="site-header">
  <button class="icon-btn menu-btn" type="button" aria-label="{toggle_nav}" aria-expanded="false">
    {icon_menu}
  </button>
  <a class="brand" href="{landing}">
    <img class="brand__mark" src="{root}assets/tasty-melon.svg" alt="" width="24" height="24">
    <span>Tasty</span>
    <span class="brand__version">v{version}</span>
  </a>
  <nav class="site-nav" aria-label="{nav_guide}">
    <a href="{root}{docs}index.html"{a_guide}>{nav_guide}</a>
    <a href="{root}{docs}getting-started/install.html"{a_inst}>{nav_install}</a>
    <a href="{repo}/releases/latest">{nav_download}</a>
    <a href="{repo}/blob/main/CHANGELOG.md">{nav_changelog}</a>
  </nav>
  <div class="header-tools">
    <div class="search">
      <label class="visually-hidden" for="site-search">{search_ph}</label>
      <span class="search__icon" aria-hidden="true">{icon_search}</span>
      <input id="site-search" type="search" autocomplete="off" spellcheck="false"
             placeholder="{search_ph}"
             data-index="{root}{search_index}?v={asset_tag}"
             data-base="{root}"
             data-empty-label="{search_empty}">
      <div class="search__results" role="listbox"></div>
    </div>
    <div class="lang-switch">
      <a href="{root}{en_href}"{a_en}>EN</a>
      <a href="{root}{ko_href}"{a_ko}>KO</a>
    </div>
    <button class="icon-btn theme-toggle" type="button" aria-label="{toggle_theme}">
      {icon_moon}{icon_sun}
    </button>
    <a class="icon-btn" href="{repo}" aria-label="GitHub">{icon_github}</a>
  </div>
</header>
"##,
        toggle_nav = html_escape(s.toggle_nav),
        icon_menu = ICON_MENU,
        icon_search = ICON_SEARCH,
        landing = landing_href,
        root = root,
        docs = shell.docs_prefix,
        search_index = shell.search_index,
        asset_tag = crate::asset_tag(),
        version = crate::version(),
        nav_guide = html_escape(s.nav_guide),
        nav_install = html_escape(s.nav_install),
        nav_download = html_escape(s.nav_download),
        nav_changelog = html_escape(s.nav_changelog),
        a_guide = mark("guide"),
        a_inst = mark("install"),
        repo = REPO,
        search_ph = html_escape(s.search_placeholder),
        search_empty = html_escape(s.search_empty),
        en_href = shell.en_href,
        ko_href = shell.ko_href,
        a_en = if s.lang == "en" {
            " aria-current=\"true\""
        } else {
            ""
        },
        a_ko = if s.lang == "ko" {
            " aria-current=\"true\""
        } else {
            ""
        },
        toggle_theme = html_escape(s.toggle_theme),
        icon_moon = ICON_MOON,
        icon_sun = ICON_SUN,
        icon_github = ICON_GITHUB,
    )
}

fn footer(shell: &Shell<'_>) -> String {
    let s = shell.strings;
    let root = &shell.root;
    format!(
        r##"<footer class="site-footer">
  <div class="site-footer__inner">
    <div>
      <a class="brand" href="{root}">
        <img class="brand__mark" src="{root}assets/tasty-melon.svg" alt="" width="24" height="24">
        <span>Tasty</span>
      </a>
      <p class="site-footer__blurb">{blurb}</p>
    </div>
    <div>
      <h4>{h_docs}</h4>
      <ul>
        <li><a href="{root}{docs}index.html">{nav_guide}</a></li>
        <li><a href="{root}{docs}getting-started/install.html">{nav_install}</a></li>
        <li><a href="{root}{docs}customize/keybindings.html">{keybindings}</a></li>
        <li><a href="{root}{docs}plugins/index.html">{plugins}</a></li>
      </ul>
    </div>
    <div>
      <h4>{h_project}</h4>
      <ul>
        <li><a href="{repo}">GitHub</a></li>
        <li><a href="{repo}/releases/latest">{releases}</a></li>
        <li><a href="{repo}/issues">{issues}</a></li>
        <li><a href="{repo}/blob/main/CHANGELOG.md">{changelog}</a></li>
      </ul>
    </div>
    <div>
      <h4>{h_res}</h4>
      <ul>
        <li><a href="{repo}/blob/main/docs/index.md">{dev_docs}</a></li>
        <li><a href="{repo}/blob/main/LICENSES">{license}</a></li>
        <li><a href="{repo}/blob/main/THIRD_PARTY_LICENSES.md">{third}</a></li>
      </ul>
    </div>
  </div>
  <div class="site-footer__legal">
    <span>© 2026 Tasty · {note}</span>
    <span>v{version}</span>
  </div>
</footer>
"##,
        root = root,
        docs = shell.docs_prefix,
        blurb = html_escape(s.footer_blurb),
        h_docs = html_escape(s.footer_docs),
        nav_guide = html_escape(s.nav_guide),
        nav_install = html_escape(s.nav_install),
        keybindings = html_escape(s.footer_keybindings),
        plugins = html_escape(s.footer_plugins),
        h_project = html_escape(s.footer_project),
        repo = REPO,
        releases = html_escape(s.footer_releases),
        issues = html_escape(s.footer_issues),
        changelog = html_escape(s.nav_changelog),
        h_res = html_escape(s.footer_resources),
        dev_docs = html_escape(s.footer_dev_docs),
        license = html_escape(s.footer_license),
        third = html_escape(s.footer_third_party),
        note = html_escape(s.footer_note),
        version = crate::version(),
    )
}

// Icons are inline so the pages need no icon font or sprite fetch.
pub const ICON_MENU: &str = r#"<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" aria-hidden="true"><path d="M4 7h16M4 12h16M4 17h16"/></svg>"#;
pub const ICON_MOON: &str = r#"<svg class="icon-moon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M21 12.8A9 9 0 1 1 11.2 3a7 7 0 0 0 9.8 9.8z"/></svg>"#;
pub const ICON_SUN: &str = r#"<svg class="icon-sun" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" aria-hidden="true"><circle cx="12" cy="12" r="4"/><path d="M12 2v2M12 20v2M4.9 4.9l1.4 1.4M17.7 17.7l1.4 1.4M2 12h2M20 12h2M4.9 19.1l1.4-1.4M17.7 6.3l1.4-1.4"/></svg>"#;
pub const ICON_SEARCH: &str = r#"<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" aria-hidden="true"><circle cx="11" cy="11" r="7"/><path d="M16.5 16.5 21 21"/></svg>"#;
pub const ICON_GITHUB: &str = r#"<svg viewBox="0 0 24 24" fill="currentColor" aria-hidden="true"><path d="M12 .5C5.6.5.5 5.6.5 12c0 5.1 3.3 9.4 7.9 10.9.6.1.8-.2.8-.6v-2c-3.2.7-3.9-1.5-3.9-1.5-.5-1.3-1.3-1.7-1.3-1.7-1.1-.7.1-.7.1-.7 1.2.1 1.8 1.2 1.8 1.2 1 1.8 2.7 1.3 3.4 1 .1-.8.4-1.3.7-1.6-2.6-.3-5.3-1.3-5.3-5.8 0-1.3.5-2.3 1.2-3.2-.1-.3-.5-1.5.1-3.1 0 0 1-.3 3.3 1.2a11.4 11.4 0 0 1 6 0C17.6 4.7 18.6 5 18.6 5c.6 1.6.2 2.8.1 3.1.8.9 1.2 1.9 1.2 3.2 0 4.5-2.7 5.5-5.3 5.8.4.4.8 1.1.8 2.2v3.3c0 .4.2.7.8.6 4.6-1.5 7.9-5.8 7.9-10.9C23.5 5.6 18.4.5 12 .5z"/></svg>"#;
