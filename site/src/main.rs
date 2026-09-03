//! Static site generator for the Tasty GitHub Pages site.
//!
//! Input : the repo's `docs/` tree (Korean) + landing copy in `src/landing.rs`.
//! Output: a fully static `_site/` — no runtime dependency, every link relative
//!         so it works under a project-pages base path (`/tasty/`) or a custom domain.
//!
//!   cargo run --manifest-path site/Cargo.toml -- [--out DIR]

mod landing;
mod md;
mod shell;

use std::collections::BTreeMap;
use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use md::{Highlighter, LinkCtx, html_escape, json_escape};
use shell::{EN, KO, REPO, Shell};
use walkdir::WalkDir;

/// Sidebar sections, in order. The empty key collects the loose `docs/*.md` pages.
const CATEGORIES: &[(&str, &str)] = &[
    ("", "시작하기"),
    ("concepts", "개념"),
    ("features", "기능"),
    ("plugins", "번들 플러그인"),
    ("design", "설계"),
    ("reference", "레퍼런스"),
    ("dev-guide", "개발 가이드"),
    ("architecture", "아키텍처"),
    ("ai-verification", "AI 자체 검증"),
    ("adr", "근거 (ADR)"),
];

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

struct Page {
    /// Path relative to `docs/`, e.g. `features/terminal/index.md`.
    rel: PathBuf,
    /// Site-root-relative output URL, e.g. `docs/features/terminal/index.html`.
    url: String,
    title: String,
    nav_label: String,
    category: usize,
    /// Sidebar indent level inside its category.
    indent: usize,
}

fn main() -> Result<(), Box<dyn Error>> {
    let mut args = std::env::args().skip(1);
    let mut out_dir = repo_root().join("_site");
    let mut strict = false;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--out" | "-o" => {
                out_dir = PathBuf::from(args.next().ok_or("--out needs a directory")?);
            }
            // Turn broken relative links in docs/ into a build failure (used by CI).
            "--strict" => strict = true,
            other => return Err(format!("unknown argument: {other}").into()),
        }
    }

    let docs_root = repo_root().join("docs");
    if !docs_root.is_dir() {
        return Err(format!("docs/ not found at {}", docs_root.display()).into());
    }

    if out_dir.exists() {
        fs::remove_dir_all(&out_dir)?;
    }
    fs::create_dir_all(&out_dir)?;

    let pages = collect_pages(&docs_root)?;
    println!("collected {} docs pages", pages.len());

    let highlighter = Highlighter::new();
    let mut index_entries: Vec<String> = Vec::with_capacity(pages.len());
    let mut broken_total = 0usize;

    for (i, page) in pages.iter().enumerate() {
        let src = docs_root.join(&page.rel);
        let source = fs::read_to_string(&src)?;

        let doc_dir = PathBuf::from("docs").join(page.rel.parent().unwrap_or(Path::new("")));
        let ctx = LinkCtx {
            dir: doc_dir,
            repo_root: repo_root(),
            blob_base: &format!("{REPO}/blob/main"),
            copy_label: KO.copy,
            copied_label: KO.copied,
        };
        let rendered = md::render(&source, &ctx, &highlighter);

        // Templates are written against the path they get copied *to*
        // (`features/<f>/index.md`), so their relative links never resolve in place.
        let is_template = page
            .rel
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n.starts_with('_'));

        if !rendered.broken_links.is_empty() && !is_template {
            broken_total += rendered.broken_links.len();
            eprintln!(
                "  broken links in docs/{}: {}",
                page.rel.display(),
                rendered.broken_links.join(", ")
            );
        }

        let depth = page.url.matches('/').count();
        let root = "../".repeat(depth);

        let body = docs_body(page, &pages, i, &rendered, &root);
        let html = shell::document(&Shell {
            strings: &KO,
            title: page.title.clone(),
            description: first_paragraph(&source).unwrap_or_else(|| KO.footer_blurb.to_string()),
            root: root.clone(),
            body,
            active: header_slot(page),
            ko_href: "ko/index.html".to_string(),
            en_href: "index.html".to_string(),
        });

        let out_path = out_dir.join(&page.url);
        if let Some(parent) = out_path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&out_path, html)?;

        index_entries.push(format!(
            r#"{{"t":"{t}","u":"{u}","c":"{c}","h":"{h}"}}"#,
            t = json_escape(&page.title),
            u = json_escape(&page.url),
            c = json_escape(&crumb(page)),
            h = json_escape(&truncate(&rendered.headings, 600)),
        ));
    }

    // ------------------------------------------------------------- landings
    fs::write(out_dir.join("index.html"), landing::render(&EN, ""))?;
    fs::create_dir_all(out_dir.join("ko"))?;
    fs::write(out_dir.join("ko/index.html"), landing::render(&KO, "../"))?;

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
    fs::write(
        assets.join("search-index.json"),
        format!("[{}]", index_entries.join(",")),
    )?;

    // GitHub Pages otherwise runs Jekyll over the output and drops `_`-prefixed paths.
    fs::write(out_dir.join(".nojekyll"), "")?;

    println!(
        "wrote {} pages to {} ({broken_total} broken links)",
        pages.len() + 2,
        out_dir.display(),
    );
    if strict && broken_total > 0 {
        return Err(format!("{broken_total} broken links (--strict)").into());
    }
    Ok(())
}

// ------------------------------------------------------------------- pages

fn collect_pages(docs_root: &Path) -> Result<Vec<Page>, Box<dyn Error>> {
    let mut by_category: BTreeMap<usize, Vec<(Vec<String>, Page)>> = BTreeMap::new();

    for entry in WalkDir::new(docs_root).sort_by_file_name() {
        let entry = entry?;
        if !entry.file_type().is_file() {
            continue;
        }
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("md") {
            continue;
        }
        let rel = path.strip_prefix(docs_root)?.to_path_buf();
        let rel_str = rel.to_string_lossy().replace('\\', "/");

        let comps: Vec<String> = rel_str.split('/').map(|s| s.to_string()).collect();
        let top = if comps.len() > 1 {
            comps[0].as_str()
        } else {
            ""
        };
        let Some(category) = CATEGORIES.iter().position(|(k, _)| *k == top) else {
            eprintln!("  skipping uncategorised docs/{rel_str}");
            continue;
        };

        let source = fs::read_to_string(path)?;
        let title =
            first_heading(&source).unwrap_or_else(|| rel_str.trim_end_matches(".md").to_string());

        let after_category = if top.is_empty() {
            &comps[..]
        } else {
            &comps[1..]
        };
        let is_index = after_category
            .last()
            .map(|s| s == "index.md")
            .unwrap_or(false);
        let indent = after_category.len().saturating_sub(1);

        // Sort key: `index.md` collapses to "" so a directory sorts before its children.
        let mut key: Vec<String> = after_category.to_vec();
        if let (Some(last), true) = (key.last_mut(), is_index) {
            *last = String::new();
        }

        let url = format!("docs/{}", rel_str.trim_end_matches(".md")) + ".html";
        let page = Page {
            nav_label: nav_label(&title, &rel_str),
            rel,
            url,
            title,
            category,
            indent,
        };
        by_category.entry(category).or_default().push((key, page));
    }

    let mut pages = Vec::new();
    for (_, mut group) in by_category {
        group.sort_by(|a, b| a.0.cmp(&b.0));
        pages.extend(group.into_iter().map(|(_, p)| p));
    }
    Ok(pages)
}

/// Docs titles are mostly `한국어 (English)` or `ADR-0042: …`; the sidebar only
/// has room for the leading part.
fn nav_label(title: &str, rel: &str) -> String {
    let mut label = title.to_string();
    if let Some(rest) = label.strip_prefix("ADR-") {
        label = rest.replacen(':', " ·", 1);
    } else if let Some(idx) = label.find(" (") {
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
    let rel = page.rel.to_string_lossy();
    if rel == "installation.md" {
        "install"
    } else if rel.starts_with("features/") {
        "features"
    } else {
        "docs"
    }
}

fn crumb(page: &Page) -> String {
    let category = CATEGORIES[page.category].1;
    let rel = page.rel.to_string_lossy().replace('\\', "/");
    format!("{category} · {rel}")
}

// -------------------------------------------------------------- docs page

fn docs_body(
    page: &Page,
    pages: &[Page],
    idx: usize,
    rendered: &md::Rendered,
    root: &str,
) -> String {
    let sidebar = sidebar(pages, idx, root);
    let toc = toc(&rendered.toc);
    let prev = pages.get(idx.wrapping_sub(1)).filter(|_| idx > 0);
    let next = pages.get(idx + 1);

    let mut page_nav = String::new();
    if prev.is_some() || next.is_some() {
        page_nav.push_str("<nav class=\"page-nav\">");
        if let Some(p) = prev {
            page_nav.push_str(&format!(
                "<a class=\"prev\" href=\"{root}{url}\"><span class=\"dir\">← {label}</span>{title}</a>",
                url = p.url,
                label = html_escape(KO.prev),
                title = html_escape(&p.nav_label),
            ));
        }
        if let Some(n) = next {
            page_nav.push_str(&format!(
                "<a class=\"next\" href=\"{root}{url}\"><span class=\"dir\">{label} →</span>{title}</a>",
                url = n.url,
                label = html_escape(KO.next),
                title = html_escape(&n.nav_label),
            ));
        }
        page_nav.push_str("</nav>");
    }

    format!(
        r##"<div class="layout">
  <aside class="sidebar" aria-label="{docs}">{sidebar}</aside>
  <main class="content" id="content">
    <nav class="breadcrumb" aria-label="breadcrumb">
      <a href="{root}docs/index.html">{docs}</a>
      <span class="sep">/</span>
      <span>{category}</span>
    </nav>
    <article class="prose">{article}</article>
    {page_nav}
    <a class="edit-link" href="{repo}/blob/main/docs/{rel}">{edit}</a>
  </main>
  <nav class="toc" aria-label="{toc_title}">{toc}</nav>
</div>
"##,
        docs = html_escape(KO.dir_docs),
        sidebar = sidebar,
        root = root,
        category = html_escape(CATEGORIES[page.category].1),
        article = rendered.html,
        page_nav = page_nav,
        repo = REPO,
        rel = page.rel.to_string_lossy().replace('\\', "/"),
        edit = html_escape(KO.edit_page),
        toc_title = html_escape(KO.toc_title),
        toc = toc,
    )
}

fn sidebar(pages: &[Page], current: usize, root: &str) -> String {
    let current_category = pages[current].category;
    let mut html = String::new();

    for (ci, (_, label)) in CATEGORIES.iter().enumerate() {
        let items: Vec<(usize, &Page)> = pages
            .iter()
            .enumerate()
            .filter(|(_, p)| p.category == ci)
            .collect();
        if items.is_empty() {
            continue;
        }
        let open = if ci == current_category { " open" } else { "" };
        html.push_str(&format!(
            "<details class=\"nav-section\"{open}><summary>{label}\
             <span class=\"nav-section__count\">{count}</span></summary><ul class=\"nav-list\">",
            label = html_escape(label),
            count = items.len(),
        ));
        for (pi, page) in items {
            let aria = if pi == current {
                " aria-current=\"page\""
            } else {
                ""
            };
            html.push_str(&format!(
                "<li><a href=\"{root}{url}\"{aria} style=\"--indent:{indent}\">{label}</a></li>",
                url = page.url,
                indent = page.indent,
                label = html_escape(&page.nav_label),
            ));
        }
        html.push_str("</ul></details>");
    }
    html
}

fn toc(items: &[md::TocItem]) -> String {
    if items.len() < 2 {
        return String::new();
    }
    let mut html = format!(
        "<div class=\"toc__title\">{}</div><ul>",
        html_escape(KO.toc_title)
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
