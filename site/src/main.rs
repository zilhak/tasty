//! Static site generator for the Tasty GitHub Pages site.
//!
//! Input : the user guide in `site/content/` (Korean, canonical), its
//!         `site/content/en/` translations, and landing copy in `src/landing.rs`.
//!         The engineering docs in `docs/` are *not* published — they are for
//!         contributors and agents working on the code, and live on GitHub.
//! Output: a fully static `_site/` — no runtime dependency, every link relative
//!         so it works under a project-pages base path (`/tasty/`) or a custom domain.
//!
//!   cargo run --manifest-path site/Cargo.toml -- [--out DIR] [--strict]
//!
//! Translation model (docs/dev-guide/site.md): `site/content/en/<rel>` mirrors
//! `site/content/<rel>` path for path. English is the default tree (`/guide/`),
//! Korean sits under `/ko/guide/`. A page with no translation is still published
//! under `/guide/` with the Korean body and a banner, so the English tree is
//! always complete. Each translation carries a `<!-- source-hash: … -->` stamp of
//! the Korean source it was made from; when the source moves on, the page shows
//! a stale banner.

mod landing;
mod md;
mod shell;

use std::collections::{HashMap, HashSet};
use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use md::{Highlighter, LinkCtx, html_escape, json_escape};
use sha2::{Digest, Sha256};
use shell::{EN, KO, REPO, Shell, Strings};
use walkdir::WalkDir;

/// Guide source tree, relative to the repo root.
const CONTENT_DIR: &str = "site/content";

/// Sidebar sections, in reading order: (directory, Korean label, English label).
/// The empty key is the guide home (`index.md`).
const SECTIONS: &[(&str, &str, &str)] = &[
    ("", "가이드", "Guide"),
    ("getting-started", "시작하기", "Getting started"),
    ("using", "사용하기", "Using Tasty"),
    ("customize", "맞춤 설정", "Customize"),
    ("agents", "AI 에이전트", "AI agents"),
    ("remote", "원격", "Remote"),
    ("plugins", "플러그인", "Plugins"),
    ("help", "도움말", "Help"),
];

/// Reading order of the guide. The sidebar and prev/next links follow this list,
/// not the file names; a content file missing from it is a build error so a new
/// page is always placed deliberately.
const ORDER: &[&str] = &[
    "index.md",
    "getting-started/install.md",
    "getting-started/first-look.md",
    "using/workspaces.md",
    "using/panes-tabs-splits.md",
    "using/terminal.md",
    "using/files.md",
    "customize/keybindings.md",
    "customize/settings.md",
    "customize/themes.md",
    "customize/scripts.md",
    "agents/cli.md",
    "agents/claude-codex.md",
    "agents/tasks.md",
    "agents/hooks-notifications.md",
    "remote/attach.md",
    "plugins/index.md",
    "help/troubleshooting.md",
];

/// Directory under `site/content/` that holds translations rather than canonical pages.
const TRANSLATION_DIR: &str = "en";
const STAMP_PREFIX: &str = "<!-- source-hash:";

static VERSION_CELL: OnceLock<String> = OnceLock::new();

/// Crate version of the app itself, read from the workspace root `Cargo.toml`.
pub fn version() -> &'static str {
    VERSION_CELL.get_or_init(|| {
        let manifest = repo_root().join("Cargo.toml");
        fs::read_to_string(manifest)
            .ok()
            .and_then(|text| {
                text.lines()
                    .skip_while(|l| l.trim() != "[package]")
                    .find_map(|l| l.strip_prefix("version = "))
                    .map(|v| v.trim().trim_matches('"').to_string())
            })
            .unwrap_or_else(|| "0.0.0".to_string())
    })
}

pub fn repo_root() -> &'static Path {
    static CELL: OnceLock<PathBuf> = OnceLock::new();
    CELL.get_or_init(|| {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("site/ always has a parent")
            .to_path_buf()
    })
}

/// One site language: where its pages live on disk and where they land in the output.
struct Lang {
    strings: &'static Strings,
    /// Source directory relative to the repo root (`site/content` or `site/content/en`).
    src_dir: &'static str,
    /// Output prefix relative to the site root (`guide/` or `ko/guide/`).
    out_prefix: &'static str,
    search_index: &'static str,
}

const LANG_KO: Lang = Lang {
    strings: &KO,
    src_dir: "site/content",
    out_prefix: "ko/guide/",
    search_index: "assets/search-index.ko.json",
};
const LANG_EN: Lang = Lang {
    strings: &EN,
    src_dir: "site/content/en",
    out_prefix: "guide/",
    search_index: "assets/search-index.json",
};

struct Translation {
    title: String,
    nav_label: String,
    /// Hash of the Korean source this translation was made from, if stamped.
    source_hash: Option<String>,
}

struct Page {
    /// Path relative to `site/content/`, e.g. `using/terminal.md`.
    rel: PathBuf,
    title: String,
    nav_label: String,
    section: usize,
    /// Short content hash of the Korean source; translations are stamped with it.
    source_hash: String,
    translation: Option<Translation>,
}

impl Page {
    fn rel_str(&self) -> String {
        self.rel.to_string_lossy().replace('\\', "/")
    }

    fn html_rel(&self) -> String {
        format!("{}.html", self.rel_str().trim_end_matches(".md"))
    }

    fn url(&self, lang: &Lang) -> String {
        format!("{}{}", lang.out_prefix, self.html_rel())
    }

    fn title_in(&self, lang: &Lang) -> &str {
        match (&self.translation, lang.strings.lang) {
            (Some(t), "en") => &t.title,
            _ => &self.title,
        }
    }

    fn nav_label_in(&self, lang: &Lang) -> &str {
        match (&self.translation, lang.strings.lang) {
            (Some(t), "en") => &t.nav_label,
            _ => &self.nav_label,
        }
    }
}

#[derive(Default)]
struct Report {
    broken_links: usize,
    translated: usize,
    stale: Vec<String>,
    untranslated: Vec<String>,
}

fn main() -> Result<(), Box<dyn Error>> {
    let mut args = std::env::args().skip(1);
    let mut out_dir = repo_root().join("_site");
    let mut strict = false;
    let mut stamp: Vec<PathBuf> = Vec::new();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--out" | "-o" => {
                out_dir = PathBuf::from(args.next().ok_or("--out needs a directory")?);
            }
            // Turn broken relative links and unordered pages into a build failure (CI).
            "--strict" => strict = true,
            // Stamp a translation with the hash of its current Korean source.
            "--stamp" => stamp.push(PathBuf::from(args.next().ok_or("--stamp needs a file")?)),
            other => return Err(format!("unknown argument: {other}").into()),
        }
    }
    if !stamp.is_empty() {
        for path in &stamp {
            stamp_translation(path)?;
        }
        return Ok(());
    }

    let content_root = repo_root().join(CONTENT_DIR);
    if !content_root.is_dir() {
        return Err(format!("{CONTENT_DIR}/ not found at {}", content_root.display()).into());
    }

    if out_dir.exists() {
        fs::remove_dir_all(&out_dir)?;
    }
    fs::create_dir_all(&out_dir)?;
    let site_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut static_src = fs::read_to_string(site_dir.join("static/style.css"))?;
    static_src.push_str(&fs::read_to_string(site_dir.join("static/site.js"))?);
    if ASSET_TAG.set(content_hash(&static_src)).is_err() {
        eprintln!("warning: asset tag was already set");
    }
    let release = load_release(site_dir);
    match &release {
        Some(r) => println!("release: {} ({} assets)", r.tag, r.assets.len()),
        None => println!(
            "release: none (site/release.json missing) — downloads link to the releases page"
        ),
    }
    if RELEASE.set(release).is_err() {
        eprintln!("warning: release was already set");
    }

    let (pages, unordered) = collect_pages(&content_root)?;
    println!("collected {} guide pages", pages.len());
    if strict && unordered > 0 {
        return Err(format!("{unordered} pages missing from ORDER (--strict)").into());
    }

    let highlighter = Highlighter::new();
    let mut report = Report::default();
    let mut written = 0usize;

    for lang in [&LANG_KO, &LANG_EN] {
        let mut index_entries: Vec<String> = Vec::with_capacity(pages.len());
        // Fragment links are checked once the whole tree is rendered: a `#anchor`
        // copied into a translation only exists if the translated heading slugs match.
        let mut anchors_by_page: HashMap<PathBuf, HashSet<String>> = HashMap::new();
        let mut fragments: Vec<(PathBuf, md::Fragment)> = Vec::new();

        for (i, page) in pages.iter().enumerate() {
            let (html, rendered) = render_page(lang, page, &pages, i, &highlighter, &mut report)?;
            let headings = rendered.headings;
            let key = PathBuf::from(CONTENT_DIR).join(&page.rel);
            fragments.extend(rendered.fragments.into_iter().map(|f| (key.clone(), f)));
            anchors_by_page.insert(key, rendered.anchors);

            let out_path = out_dir.join(page.url(lang));
            if let Some(parent) = out_path.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::write(&out_path, html)?;
            written += 1;

            index_entries.push(format!(
                r#"{{"t":"{t}","u":"{u}","c":"{c}","h":"{h}"}}"#,
                t = json_escape(page.title_in(lang)),
                u = json_escape(&page.url(lang)),
                c = json_escape(&crumb(page, lang)),
                h = json_escape(&truncate(&headings, 600)),
            ));
        }

        for (from, fragment) in fragments {
            let target = fragment.target.as_ref().unwrap_or(&from);
            let found = anchors_by_page
                .get(target)
                .is_some_and(|anchors| anchors.contains(&fragment.anchor));
            if !found {
                report.broken_links += 1;
                eprintln!(
                    "  broken anchor in {}/{}: {}#{}",
                    lang.src_dir,
                    from.strip_prefix(CONTENT_DIR).unwrap_or(&from).display(),
                    target.strip_prefix(CONTENT_DIR).unwrap_or(target).display(),
                    fragment.anchor
                );
            }
        }

        let index_path = out_dir.join(lang.search_index);
        if let Some(parent) = index_path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(index_path, format!("[{}]", index_entries.join(",")))?;
    }

    // ------------------------------------------------------------- landings
    fs::write(
        out_dir.join("index.html"),
        landing::render(LANG_EN.strings, "", LANG_EN.out_prefix),
    )?;
    fs::create_dir_all(out_dir.join("ko"))?;
    fs::write(
        out_dir.join("ko/index.html"),
        landing::render(LANG_KO.strings, "../", LANG_KO.out_prefix),
    )?;
    written += 2;

    // --------------------------------------------------------------- assets
    let assets = out_dir.join("assets");
    fs::create_dir_all(&assets)?;
    let site_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    fs::copy(site_dir.join("static/style.css"), assets.join("style.css"))?;
    fs::copy(site_dir.join("static/site.js"), assets.join("site.js"))?;
    fs::copy(
        repo_root().join("assets/icons/tasty-melon.svg"),
        assets.join("tasty-melon.svg"),
    )?;

    // GitHub Pages otherwise runs Jekyll over the output and drops `_`-prefixed paths.
    fs::write(out_dir.join(".nojekyll"), "")?;

    // --------------------------------------------------------------- report
    println!(
        "wrote {written} pages to {} ({} broken links)",
        out_dir.display(),
        report.broken_links
    );
    println!(
        "translations: {}/{} pages, {} stale, {} untranslated",
        report.translated,
        pages.len(),
        report.stale.len(),
        report.untranslated.len()
    );
    for rel in &report.stale {
        println!("  stale: {CONTENT_DIR}/{TRANSLATION_DIR}/{rel}");
    }
    for rel in &report.untranslated {
        println!("  untranslated: {CONTENT_DIR}/{rel}");
    }

    if strict && report.broken_links > 0 {
        return Err(format!("{} broken links (--strict)", report.broken_links).into());
    }
    Ok(())
}

/// Rewrites (or inserts) the `<!-- source-hash: … -->` line of one translation so
/// it matches the Korean source as it is right now. Run after updating a translation.
fn stamp_translation(path: &Path) -> Result<(), Box<dyn Error>> {
    let abs = if path.is_absolute() {
        path.to_path_buf()
    } else {
        repo_root().join(path)
    };
    let rel = abs
        .strip_prefix(repo_root().join(CONTENT_DIR).join(TRANSLATION_DIR))
        .map_err(|_| {
            format!(
                "{} is not under {CONTENT_DIR}/{TRANSLATION_DIR}/",
                abs.display()
            )
        })?;
    let source_path = repo_root().join(CONTENT_DIR).join(rel);
    let source = fs::read_to_string(&source_path)
        .map_err(|e| format!("no Korean source at {}: {e}", source_path.display()))?;
    let hash = content_hash(&source);

    let translation = fs::read_to_string(&abs)?;
    let stamp_line = format!("{STAMP_PREFIX} {hash} -->");
    let body = match read_stamp(&translation) {
        Some(_) => {
            // Replace the existing first non-empty line.
            let mut lines = translation.lines();
            let mut out = Vec::new();
            let mut replaced = false;
            for line in lines.by_ref() {
                if !replaced && !line.trim().is_empty() {
                    out.push(stamp_line.as_str());
                    replaced = true;
                } else {
                    out.push(line);
                }
            }
            let mut joined = out.join("\n");
            joined.push('\n');
            joined
        }
        None => format!("{stamp_line}\n{translation}"),
    };
    fs::write(&abs, body)?;
    println!(
        "stamped {CONTENT_DIR}/{TRANSLATION_DIR}/{} -> {hash}",
        rel.display()
    );
    Ok(())
}

// ------------------------------------------------------------------- pages

/// Walks the guide tree and returns its pages in `ORDER`, plus how many files were
/// not listed there (they are appended at the end so nothing silently vanishes).
fn collect_pages(content_root: &Path) -> Result<(Vec<Page>, usize), Box<dyn Error>> {
    let translation_root = content_root.join(TRANSLATION_DIR);
    let mut keyed: Vec<(usize, Page)> = Vec::new();
    let mut unordered = 0usize;

    for entry in WalkDir::new(content_root).sort_by_file_name() {
        let entry = entry?;
        if !entry.file_type().is_file() {
            continue;
        }
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("md") {
            continue;
        }
        if path.starts_with(&translation_root) {
            continue;
        }
        let rel = path.strip_prefix(content_root)?.to_path_buf();
        let rel_str = rel.to_string_lossy().replace('\\', "/");

        let top = match rel_str.split_once('/') {
            Some((dir, _)) => dir,
            None => "",
        };
        let Some(section) = SECTIONS.iter().position(|(k, _, _)| *k == top) else {
            eprintln!("  skipping {CONTENT_DIR}/{rel_str}: directory is not a sidebar section");
            continue;
        };
        let order = match ORDER.iter().position(|o| *o == rel_str) {
            Some(i) => i,
            None => {
                eprintln!("  {CONTENT_DIR}/{rel_str} is not listed in ORDER — appended last");
                unordered += 1;
                ORDER.len() + keyed.len()
            }
        };

        let source = fs::read_to_string(path)?;
        let title =
            first_heading(&source).unwrap_or_else(|| rel_str.trim_end_matches(".md").to_string());
        let translation = load_translation(&translation_root.join(&rel), &rel_str);

        keyed.push((
            order,
            Page {
                nav_label: nav_label(&title, &rel_str),
                source_hash: content_hash(&source),
                rel,
                title,
                section,
                translation,
            },
        ));
    }

    for listed in ORDER {
        if !content_root.join(listed).is_file() {
            eprintln!("  ORDER lists {CONTENT_DIR}/{listed} but the file does not exist");
        }
    }

    // Translations with no Korean counterpart are orphans — say so, loudly.
    if translation_root.is_dir() {
        for entry in WalkDir::new(&translation_root) {
            let entry = entry?;
            let path = entry.path();
            if !entry.file_type().is_file()
                || path.extension().and_then(|e| e.to_str()) != Some("md")
            {
                continue;
            }
            let rel = path.strip_prefix(&translation_root)?;
            if !content_root.join(rel).exists() {
                eprintln!(
                    "  orphan translation {CONTENT_DIR}/{TRANSLATION_DIR}/{} (no {CONTENT_DIR}/{} to mirror)",
                    rel.display(),
                    rel.display()
                );
            }
        }
    }

    keyed.sort_by_key(|(order, _)| *order);
    Ok((keyed.into_iter().map(|(_, p)| p).collect(), unordered))
}

fn load_translation(path: &Path, rel_str: &str) -> Option<Translation> {
    let source = fs::read_to_string(path).ok()?;
    let title =
        first_heading(&source).unwrap_or_else(|| rel_str.trim_end_matches(".md").to_string());
    Some(Translation {
        nav_label: nav_label(&title, rel_str),
        title,
        source_hash: read_stamp(&source),
    })
}

/// Translations open with `<!-- source-hash: abcdef012345 -->` on their first line.
fn read_stamp(source: &str) -> Option<String> {
    let first = source.lines().find(|l| !l.trim().is_empty())?;
    let rest = first.trim().strip_prefix(STAMP_PREFIX)?;
    let hash = rest.trim().trim_end_matches("-->").trim();
    if hash.is_empty() {
        None
    } else {
        Some(hash.to_string())
    }
}

/// Short, stable content hash — what translations are stamped with.
/// Short hash of the static assets, appended as `?v=` to their URLs so a
/// redeploy is never served with a browser-cached stylesheet (GitHub Pages
/// caches assets for ten minutes; a page fetched with an old `style.css`
/// renders the new markup unstyled).
static ASSET_TAG: std::sync::OnceLock<String> = std::sync::OnceLock::new();

/// The latest GitHub release, resolved by the Pages workflow into
/// `site/release.json` (`gh release view --json tagName,assets`) before the
/// generator runs. Absent in a plain local build — the landing then links the
/// downloads to the releases page instead of to the assets.
pub struct Release {
    pub tag: String,
    /// (asset file name, browser download URL)
    pub assets: Vec<(String, String)>,
}

static RELEASE: std::sync::OnceLock<Option<Release>> = std::sync::OnceLock::new();

pub fn release() -> Option<&'static Release> {
    RELEASE.get().and_then(Option::as_ref)
}

fn load_release(site_dir: &Path) -> Option<Release> {
    let raw = fs::read_to_string(site_dir.join("release.json")).ok()?;
    let json: serde_json::Value = match serde_json::from_str(&raw) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("warning: site/release.json is not valid JSON: {e}");
            return None;
        }
    };
    let tag = json.get("tagName")?.as_str()?.to_string();
    let assets = json
        .get("assets")?
        .as_array()?
        .iter()
        .filter_map(|a| {
            Some((
                a.get("name")?.as_str()?.to_string(),
                a.get("url")?.as_str()?.to_string(),
            ))
        })
        .collect();
    Some(Release { tag, assets })
}

pub fn asset_tag() -> &'static str {
    ASSET_TAG.get().map(String::as_str).unwrap_or("dev")
}

pub fn content_hash(source: &str) -> String {
    // Normalise line endings so a CRLF checkout does not invalidate every stamp.
    let normalised = source.replace("\r\n", "\n");
    let digest = Sha256::digest(normalised.as_bytes());
    digest.iter().take(6).map(|b| format!("{b:02x}")).collect()
}

/// Docs titles are mostly `한국어 (English)` or `ADR-0042: …`; the sidebar only
/// has room for the leading part.
/// Sidebar label: the page title without a trailing parenthetical or dash clause.
fn nav_label(title: &str, rel: &str) -> String {
    let mut label = title.to_string();
    if let Some(idx) = label.find(" (") {
        // Keep it only if what precedes the paren is substantial on its own.
        if idx >= 2 {
            label.truncate(idx);
        }
    }
    if let Some(idx) = label.find(" — ") {
        label.truncate(idx);
    }
    let label = label.trim().to_string();
    if label.is_empty() {
        rel.trim_end_matches(".md").to_string()
    } else {
        label
    }
}

fn first_heading(source: &str) -> Option<String> {
    source
        .lines()
        .find(|l| l.starts_with("# "))
        .map(|l| l[2..].trim().to_string())
}

fn first_paragraph(source: &str) -> Option<String> {
    let mut lines = source.lines().skip_while(|l| !l.starts_with("# "));
    lines.next()?;
    let mut buf = String::new();
    for line in lines {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            if buf.is_empty() {
                continue;
            }
            break;
        }
        if trimmed.starts_with('#') || trimmed.starts_with('|') || trimmed.starts_with("```") {
            if buf.is_empty() {
                continue;
            }
            break;
        }
        if !buf.is_empty() {
            buf.push(' ');
        }
        buf.push_str(trimmed);
        if buf.chars().count() > 180 {
            break;
        }
    }
    let cleaned = strip_markdown(&buf);
    if cleaned.is_empty() {
        None
    } else {
        Some(truncate(&cleaned, 180))
    }
}

fn strip_markdown(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(ch) = chars.next() {
        match ch {
            '[' => {}
            ']' => {
                // drop the following (link target)
                if chars.peek() == Some(&'(') {
                    for c in chars.by_ref() {
                        if c == ')' {
                            break;
                        }
                    }
                }
            }
            '`' | '*' | '_' | '>' => {}
            c => out.push(c),
        }
    }
    out.trim().to_string()
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let mut out: String = s.chars().take(max).collect();
    out.push('…');
    out
}

fn header_slot(page: &Page) -> &'static str {
    if page.rel_str() == "getting-started/install.md" {
        "install"
    } else {
        "guide"
    }
}

fn section_label(index: usize, lang: &Lang) -> &'static str {
    let (_, ko, en) = SECTIONS[index];
    if lang.strings.lang == "en" { en } else { ko }
}

fn crumb(page: &Page, lang: &Lang) -> String {
    format!("{} · {}", section_label(page.section, lang), page.rel_str())
}

// -------------------------------------------------------------- docs page

/// Renders one page in one language. Returns the full HTML and the rendered body
/// (heading text for the search index, anchors and fragment links for checking).
fn render_page(
    lang: &Lang,
    page: &Page,
    pages: &[Page],
    idx: usize,
    highlighter: &Highlighter,
    report: &mut Report,
) -> Result<(String, md::Rendered), Box<dyn Error>> {
    let s = lang.strings;
    let translated = lang.strings.lang == "en" && page.translation.is_some();

    // English pages fall back to the Korean body when there is no translation.
    let src_path = if translated {
        repo_root().join(lang.src_dir).join(&page.rel)
    } else {
        repo_root().join(CONTENT_DIR).join(&page.rel)
    };
    let source = fs::read_to_string(&src_path)?;

    // Links are always resolved as if the page sat at its Korean path. Inside the
    // content tree the two languages mirror each other so relative links stay valid
    // on output, and links that leave it (`../../../docs/...`) keep pointing at the
    // right file even though `en/` is one level deeper. Translators copy links verbatim.
    let page_dir = PathBuf::from(CONTENT_DIR).join(page.rel.parent().unwrap_or(Path::new("")));
    let ctx = LinkCtx {
        dir: page_dir,
        content_root: Path::new(CONTENT_DIR),
        repo_root: repo_root(),
        blob_base: &format!("{REPO}/blob/main"),
        copy_label: s.copy,
        copied_label: s.copied,
    };
    let rendered = md::render(&source, &ctx, highlighter);

    if !rendered.broken_links.is_empty() {
        report.broken_links += rendered.broken_links.len();
        eprintln!(
            "  broken links in {}/{}: {}",
            if translated {
                lang.src_dir
            } else {
                CONTENT_DIR
            },
            page.rel_str(),
            rendered.broken_links.join(", ")
        );
    }

    // Translation-state banner (English tree only).
    let mut banner = String::new();
    if lang.strings.lang == "en" {
        match &page.translation {
            None => {
                if idx == 0 || !report.untranslated.contains(&page.rel_str()) {
                    report.untranslated.push(page.rel_str());
                }
                banner = translation_banner("untranslated", s.untranslated, s.translate_cta, page);
            }
            Some(t) => {
                report.translated += 1;
                if t.source_hash.as_deref() != Some(page.source_hash.as_str()) {
                    report.stale.push(page.rel_str());
                    banner = translation_banner("stale", s.stale, s.translate_cta, page);
                }
            }
        }
    }

    let url = page.url(lang);
    let depth = url.matches('/').count();
    let root = "../".repeat(depth);

    let body = docs_body(lang, page, pages, idx, &rendered, &root, &banner);
    let html = shell::document(&Shell {
        strings: s,
        title: page.title_in(lang).to_string(),
        description: first_paragraph(&source).unwrap_or_else(|| s.footer_blurb.to_string()),
        root,
        body,
        active: header_slot(page),
        ko_href: page.url(&LANG_KO),
        en_href: page.url(&LANG_EN),
        docs_prefix: lang.out_prefix,
        search_index: lang.search_index,
    });

    Ok((html, rendered))
}

fn translation_banner(kind: &str, text: &str, cta: &str, page: &Page) -> String {
    format!(
        "<div class=\"note note--{kind}\" role=\"note\">{text} \
         <a href=\"{repo}/blob/main/{content}/{rel}\">{cta}</a></div>",
        text = html_escape(text),
        repo = REPO,
        content = CONTENT_DIR,
        rel = page.rel_str(),
        cta = html_escape(cta),
    )
}

fn docs_body(
    lang: &Lang,
    page: &Page,
    pages: &[Page],
    idx: usize,
    rendered: &md::Rendered,
    root: &str,
    banner: &str,
) -> String {
    let s = lang.strings;
    let sidebar = sidebar(lang, pages, idx, root);
    let toc = toc(s, &rendered.toc);
    let prev = pages.get(idx.wrapping_sub(1)).filter(|_| idx > 0);
    let next = pages.get(idx + 1);

    let mut page_nav = String::new();
    if prev.is_some() || next.is_some() {
        page_nav.push_str("<nav class=\"page-nav\">");
        if let Some(p) = prev {
            page_nav.push_str(&format!(
                "<a class=\"prev\" href=\"{root}{url}\"><span class=\"dir\">← {label}</span>{title}</a>",
                url = p.url(lang),
                label = html_escape(s.prev),
                title = html_escape(p.nav_label_in(lang)),
            ));
        }
        if let Some(n) = next {
            page_nav.push_str(&format!(
                "<a class=\"next\" href=\"{root}{url}\"><span class=\"dir\">{label} →</span>{title}</a>",
                url = n.url(lang),
                label = html_escape(s.next),
                title = html_escape(n.nav_label_in(lang)),
            ));
        }
        page_nav.push_str("</nav>");
    }

    // "View on GitHub" points at whichever file the reader is actually looking at.
    let source_rel = if lang.strings.lang == "en" && page.translation.is_some() {
        format!("{}/{}", TRANSLATION_DIR, page.rel_str())
    } else {
        page.rel_str()
    };

    format!(
        r##"<div class="layout">
  <aside class="sidebar" aria-label="{docs}">{sidebar}</aside>
  <main class="content" id="content">
    <nav class="breadcrumb" aria-label="breadcrumb">
      <a href="{root}{prefix}index.html">{docs}</a>
      <span class="sep">/</span>
      <span>{category}</span>
    </nav>
    {banner}
    <article class="prose">{article}</article>
    {page_nav}
    <a class="edit-link" href="{repo}/blob/main/{content}/{source_rel}">{edit}</a>
  </main>
  <nav class="toc" aria-label="{toc_title}">{toc}</nav>
</div>
"##,
        docs = html_escape(s.dir_docs),
        sidebar = sidebar,
        root = root,
        prefix = lang.out_prefix,
        category = html_escape(section_label(page.section, lang)),
        banner = banner,
        article = rendered.html,
        page_nav = page_nav,
        repo = REPO,
        content = CONTENT_DIR,
        source_rel = source_rel,
        edit = html_escape(s.edit_page),
        toc_title = html_escape(s.toc_title),
        toc = toc,
    )
}

fn sidebar(lang: &Lang, pages: &[Page], current: usize, root: &str) -> String {
    let current_section = pages[current].section;
    let mut html = String::new();

    for si in 0..SECTIONS.len() {
        let items: Vec<(usize, &Page)> = pages
            .iter()
            .enumerate()
            .filter(|(_, p)| p.section == si)
            .collect();
        if items.is_empty() {
            continue;
        }
        // The guide is short enough to show whole: every section stays open, the
        // current one is only marked so the stylesheet can emphasise it.
        let current_attr = if si == current_section {
            " data-current"
        } else {
            ""
        };
        html.push_str(&format!(
            "<details class=\"nav-section\" open{current_attr}><summary>{label}</summary><ul class=\"nav-list\">",
            label = html_escape(section_label(si, lang)),
        ));
        for (pi, page) in items {
            let aria = if pi == current {
                " aria-current=\"page\""
            } else {
                ""
            };
            html.push_str(&format!(
                "<li><a href=\"{root}{url}\"{aria} style=\"--indent:0\">{label}</a></li>",
                url = page.url(lang),
                label = html_escape(page.nav_label_in(lang)),
            ));
        }
        html.push_str("</ul></details>");
    }
    html
}

fn toc(s: &Strings, items: &[md::TocItem]) -> String {
    if items.len() < 2 {
        return String::new();
    }
    let mut html = format!(
        "<div class=\"toc__title\">{}</div><ul>",
        html_escape(s.toc_title)
    );
    for item in items {
        html.push_str(&format!(
            "<li><a class=\"lvl-{lvl}\" href=\"#{id}\">{text}</a></li>",
            lvl = item.level,
            id = item.id,
            text = html_escape(&item.text),
        ));
    }
    html.push_str("</ul>");
    html
}
