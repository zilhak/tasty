//! Markdown -> HTML for the docs tree.
//!
//! Everything the site needs beyond stock CommonMark is done by rewriting the
//! event stream before handing it to the HTML renderer: heading slugs + anchors,
//! `.md` link rewriting, table wrappers, and syntect-classed code blocks.

use std::collections::HashMap;
use std::path::{Component, Path, PathBuf};

use pulldown_cmark::{CodeBlockKind, CowStr, Event, HeadingLevel, Options, Parser, Tag, TagEnd};
use syntect::html::{ClassStyle, ClassedHTMLGenerator};
use syntect::parsing::SyntaxSet;
use syntect::util::LinesWithEndings;

const CLASS_STYLE: ClassStyle = ClassStyle::SpacedPrefixed { prefix: "hl-" };

pub struct Highlighter {
    syntaxes: SyntaxSet,
}

impl Highlighter {
    pub fn new() -> Self {
        Self {
            syntaxes: SyntaxSet::load_defaults_newlines(),
        }
    }

    /// Highlight `code` as `lang`, falling back to plain text for unknown tokens.
    fn highlight(&self, code: &str, lang: &str) -> String {
        // The docs use a few aliases syntect does not know by token.
        let token = match lang {
            "sh" | "shell" | "zsh" | "console" => "bash",
            "jsonc" => "json",
            "ps1" => "powershell",
            "" | "text" | "txt" | "mermaid" => return html_escape(code),
            other => other,
        };
        let syntax = self
            .syntaxes
            .find_syntax_by_token(token)
            .unwrap_or_else(|| self.syntaxes.find_syntax_plain_text());
        let mut generator =
            ClassedHTMLGenerator::new_with_class_style(syntax, &self.syntaxes, CLASS_STYLE);
        for line in LinesWithEndings::from(code) {
            if generator
                .parse_html_for_line_which_includes_newline(line)
                .is_err()
            {
                return html_escape(code);
            }
        }
        generator.finalize()
    }
}

#[derive(Debug, Clone)]
pub struct TocItem {
    pub level: u8,
    pub id: String,
    pub text: String,
}

pub struct Rendered {
    pub html: String,
    pub toc: Vec<TocItem>,
    /// All heading text joined — feeds the search index.
    pub headings: String,
    /// Links that pointed at a file which does not exist on disk.
    pub broken_links: Vec<String>,
}

/// Where the document being rendered lives, so relative links can be resolved.
pub struct LinkCtx<'a> {
    /// Directory of the document, relative to the repo root (e.g. `docs/features/terminal`).
    pub dir: PathBuf,
    /// Repo root on disk, used to check link targets actually exist.
    pub repo_root: &'a Path,
    /// `https://github.com/<owner>/<repo>/blob/<ref>` — for links that leave `docs/`.
    pub blob_base: &'a str,
    pub copy_label: &'a str,
    pub copied_label: &'a str,
}

pub fn render(src: &str, ctx: &LinkCtx<'_>, hl: &Highlighter) -> Rendered {
    let mut options = Options::empty();
    options.insert(Options::ENABLE_TABLES);
    options.insert(Options::ENABLE_STRIKETHROUGH);
    options.insert(Options::ENABLE_TASKLISTS);
    options.insert(Options::ENABLE_FOOTNOTES);
    options.insert(Options::ENABLE_SMART_PUNCTUATION);

    let events: Vec<Event> = Parser::new_ext(src, options).collect();

    let mut out: Vec<Event> = Vec::with_capacity(events.len() + 64);
    let mut toc: Vec<TocItem> = Vec::new();
    let mut headings = String::new();
    let mut broken_links: Vec<String> = Vec::new();
    let mut slugs: HashMap<String, usize> = HashMap::new();

    let mut i = 0usize;
    while i < events.len() {
        match &events[i] {
            // ---------------------------------------------------- headings
            Event::Start(Tag::Heading {
                level,
                classes,
                attrs,
                ..
            }) => {
                let level = *level;
                let (inner, end) = slice_until(&events, i + 1, |e| {
                    matches!(e, Event::End(TagEnd::Heading(_)))
                });
                let text = plain_text(inner);
                let id = unique_slug(&text, &mut slugs);

                if matches!(level, HeadingLevel::H2 | HeadingLevel::H3) {
                    toc.push(TocItem {
                        level: heading_number(level),
                        id: id.clone(),
                        text: text.clone(),
                    });
                }
                if !text.is_empty() {
                    if !headings.is_empty() {
                        headings.push(' ');
                    }
                    headings.push_str(&text);
                }

                out.push(Event::Start(Tag::Heading {
                    level,
                    id: Some(CowStr::from(id.clone())),
                    classes: classes.clone(),
                    attrs: attrs.clone(),
                }));
                out.extend(rewrite_inline(inner, ctx, &mut broken_links));
                if level != HeadingLevel::H1 {
                    out.push(Event::Html(CowStr::from(format!(
                        "<a class=\"heading-anchor\" href=\"#{id}\" aria-hidden=\"true\">#</a>"
                    ))));
                }
                out.push(Event::End(TagEnd::Heading(level)));
                i = end + 1;
            }

            // -------------------------------------------------- code blocks
            Event::Start(Tag::CodeBlock(kind)) => {
                let lang = match kind {
                    CodeBlockKind::Fenced(info) => {
                        info.split_whitespace().next().unwrap_or("").to_string()
                    }
                    CodeBlockKind::Indented => String::new(),
                };
                let (inner, end) = slice_until(&events, i + 1, |e| {
                    matches!(e, Event::End(TagEnd::CodeBlock))
                });
                let mut code = String::new();
                for event in inner {
                    if let Event::Text(t) = event {
                        code.push_str(t);
                    }
                }
                let body = hl.highlight(&code, &lang);
                let badge = if lang.is_empty() {
                    String::new()
                } else {
                    format!(
                        "<span class=\"code-block__lang\">{}</span>",
                        html_escape(&lang)
                    )
                };
                out.push(Event::Html(CowStr::from(format!(
                    "<div class=\"code-block\" data-copy-label=\"{copy}\" \
                     data-copied-label=\"{copied}\">{badge}<pre><code>{body}</code></pre></div>",
                    copy = html_escape(ctx.copy_label),
                    copied = html_escape(ctx.copied_label),
                ))));
                i = end + 1;
            }

            // ------------------------------------------------------- tables
            Event::Start(Tag::Table(alignment)) => {
                out.push(Event::Html(CowStr::from("<div class=\"table-wrap\">")));
                out.push(Event::Start(Tag::Table(alignment.clone())));
                i += 1;
            }
            Event::End(TagEnd::Table) => {
                out.push(Event::End(TagEnd::Table));
                out.push(Event::Html(CowStr::from("</div>")));
                i += 1;
            }

            // -------------------------------------------------------- links
            event => {
                out.push(rewrite_one(event.clone(), ctx, &mut broken_links));
                i += 1;
            }
        }
    }

    let mut html = String::with_capacity(src.len() * 2);
    pulldown_cmark::html::push_html(&mut html, out.into_iter());

    Rendered {
        html,
        toc,
        headings,
        broken_links,
    }
}

// ------------------------------------------------------------------ helpers

fn heading_number(level: HeadingLevel) -> u8 {
    match level {
        HeadingLevel::H1 => 1,
        HeadingLevel::H2 => 2,
        HeadingLevel::H3 => 3,
        HeadingLevel::H4 => 4,
        HeadingLevel::H5 => 5,
        HeadingLevel::H6 => 6,
    }
}

/// Returns the events from `start` up to (not including) the first match, plus that index.
fn slice_until<'e>(
    events: &'e [Event<'e>],
    start: usize,
    is_end: impl Fn(&Event<'e>) -> bool,
) -> (&'e [Event<'e>], usize) {
    let mut j = start;
    while j < events.len() && !is_end(&events[j]) {
        j += 1;
    }
    (&events[start..j.min(events.len())], j)
}

fn plain_text(events: &[Event<'_>]) -> String {
    let mut s = String::new();
    for event in events {
        match event {
            Event::Text(t) | Event::Code(t) => s.push_str(t),
            Event::SoftBreak | Event::HardBreak => s.push(' '),
            _ => {}
        }
    }
    s.trim().to_string()
}

fn rewrite_inline<'e>(
    events: &[Event<'e>],
    ctx: &LinkCtx<'_>,
    broken: &mut Vec<String>,
) -> Vec<Event<'e>> {
    events
        .iter()
        .map(|e| rewrite_one(e.clone(), ctx, broken))
        .collect()
}

fn rewrite_one<'e>(event: Event<'e>, ctx: &LinkCtx<'_>, broken: &mut Vec<String>) -> Event<'e> {
    match event {
        Event::Start(Tag::Link {
            link_type,
            dest_url,
            title,
            id,
        }) => {
            let dest = rewrite_link(&dest_url, ctx, broken);
            Event::Start(Tag::Link {
                link_type,
                dest_url: CowStr::from(dest),
                title,
                id,
            })
        }
        Event::Start(Tag::Image {
            link_type,
            dest_url,
            title,
            id,
        }) => {
            let dest = rewrite_link(&dest_url, ctx, broken);
            Event::Start(Tag::Image {
                link_type,
                dest_url: CowStr::from(dest),
                title,
                id,
            })
        }
        other => other,
    }
}

/// Rewrite one link destination.
///
/// * external / anchor-only / mailto -> untouched
/// * `*.md` inside `docs/` -> same relative path with an `.html` extension
/// * anything else that resolves inside the repo -> GitHub blob URL
fn rewrite_link(dest: &str, ctx: &LinkCtx<'_>, broken: &mut Vec<String>) -> String {
    if dest.is_empty()
        || dest.starts_with('#')
        || dest.contains("://")
        || dest.starts_with("mailto:")
        || dest.starts_with("data:")
        || dest.starts_with('/')
    {
        return dest.to_string();
    }

    let (path_part, anchor) = match dest.split_once('#') {
        Some((p, a)) => (p, Some(a)),
        None => (dest, None),
    };
    if path_part.is_empty() {
        return dest.to_string();
    }

    let resolved = normalize(&ctx.dir.join(path_part));
    let escapes_repo = resolved.components().next() == Some(Component::ParentDir);
    let exists = ctx.repo_root.join(&resolved).exists();

    let inside_docs = !escapes_repo && resolved.starts_with("docs");
    let is_markdown = path_part.ends_with(".md");

    if inside_docs && is_markdown {
        if !exists {
            broken.push(dest.to_string());
        }
        let mut rewritten = path_part.trim_end_matches(".md").to_string();
        rewritten.push_str(".html");
        if let Some(a) = anchor {
            rewritten.push('#');
            rewritten.push_str(a);
        }
        return rewritten;
    }

    // Leaves the docs tree (source files, CLAUDE.md, LICENSES/, ...) -> point at GitHub.
    if !escapes_repo && !exists {
        broken.push(dest.to_string());
    }
    let mut url = format!(
        "{}/{}",
        ctx.blob_base,
        resolved.to_string_lossy().replace('\\', "/")
    );
    if let Some(a) = anchor {
        url.push('#');
        url.push_str(a);
    }
    url
}

/// Lexical `..` / `.` collapse. Keeps leading `..` so callers can detect escapes.
fn normalize(path: &Path) -> PathBuf {
    let mut out: Vec<Component> = Vec::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => match out.last() {
                Some(Component::Normal(_)) => {
                    out.pop();
                }
                _ => out.push(Component::ParentDir),
            },
            other => out.push(other),
        }
    }
    out.iter().collect()
}

/// GitHub-compatible heading slug: lowercase, drop punctuation, spaces to dashes.
/// Non-ASCII (Hangul) is kept as-is — the docs are Korean and their anchors must work.
pub fn slugify(text: &str) -> String {
    let mut slug = String::with_capacity(text.len());
    for ch in text.chars() {
        if ch.is_alphanumeric() {
            for lower in ch.to_lowercase() {
                slug.push(lower);
            }
        } else if ch == ' ' || ch == '-' || ch == '_' {
            slug.push('-');
        }
        // everything else (punctuation, emoji) is dropped, as GitHub does
    }
    let slug = slug.trim_matches('-').to_string();
    if slug.is_empty() {
        "section".to_string()
    } else {
        slug
    }
}

fn unique_slug(text: &str, seen: &mut HashMap<String, usize>) -> String {
    let base = slugify(text);
    let count = seen.entry(base.clone()).or_insert(0);
    let slug = if *count == 0 {
        base.clone()
    } else {
        format!("{base}-{count}")
    };
    *count += 1;
    slug
}

pub fn html_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(ch),
        }
    }
    out
}

/// Escape for a JSON string literal.
pub fn json_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 8);
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out
}
