//! Markdown → sanitized HTML document generation for the **webview** surface (ADR-0028,
//! Stage B — replaces the former `egui_commonmark` mesh renderer).
//!
//! [`render_document`] is the single entry point: it turns the markdown source into a
//! complete, self-contained HTML5 document that the plugin hands to the host via
//! `webview.set_url` (the host's `sync_webviews` auto-detects a scheme-less string as raw
//! HTML and calls the native WebView's `load_html`, see `src/view/main/redraw.rs`).
//!
//! Pipeline:
//! 1. `pulldown-cmark` parses the source and walks the event stream, rewriting every link
//!    `href` destination to the internal `#tasty-nav:link:<percent-encoded-dest>` fragment
//!    scheme ([`rewrite_link_dest`]) before generating HTML with `pulldown_cmark::html::push_html`.
//!    This is **not** cosmetic — a plain `href` pointing at a local file would let the native
//!    WebView actually navigate there (WebKitGTK/WKWebView/WebView2 only block *remote*
//!    http(s) navigation at the host level, see `src/host_api/webview/linux.rs`), replacing our
//!    rendered document with the raw file and breaking the viewer. Routing every non-anchor
//!    link through a same-document URL fragment sidesteps this: WebKitGTK's `decide-policy`
//!    still fires for fragment-only navigation (so the host still captures and forwards the
//!    attempt via Stage A's `webview.navigation_attempt`), but the navigation itself never
//!    reloads the document (verified live — no `load-changed`/`load-failed` cycle), so the
//!    surface never flips to the host's loading/error chrome just because a link was clicked.
//!    The plugin's address-bar script (baked into the document, see [`nav_script`]) uses the
//!    identical `location.hash = 'tasty-nav:addr:' + encodeURIComponent(path)` trick.
//! 2. The generated HTML is sanitized through [`sanitize_html`] (`ammonia`) with a minimal
//!    allowlist sized for GFM output only — `<script>`, inline event handlers (`onerror=`, …),
//!    and `javascript:` scheme URLs are all stripped. [`classify_link`] additionally treats a
//!    `javascript:` destination as unresolvable (`None`) so a malicious link can't survive
//!    even as an inert internal-nav fragment.
//! 3. [`theme_css`] maps the host `Theme` tokens onto CSS custom properties — the CSS-token
//!    equivalent of the former `egui::Visuals` mapping, but without the two library
//!    limitations that motivated this rewrite (`egui_commonmark` couldn't set a per-level
//!    heading ladder or override body line-height; real CSS does both trivially).
//! 4. Relative image `src`/link resolution against the markdown file's directory is handled by
//!    a `<base href="file://<base_dir>/">` tag rather than per-`src` rewriting — simpler than
//!    the old `image_uri_prefix` prefixing and, unlike it, resolves absolute local paths
//!    correctly too (`<base>` only affects genuinely relative references).
//! 5. A fenced ` ```mermaid ` block survives the pipeline above as plain
//!    `<code class="language-mermaid">` — [`rewrite_code_block_event`]/[`sanitize_fence_lang`]
//!    normalize the fence language into that exact class shape, and `sanitize_html`'s allowlist
//!    lets `class` through on `code`. [`mermaid_script`] (trusted, plugin-authored — appended
//!    after sanitization, like [`nav_script`], so it's never subject to the user-content
//!    allowlist) then vendors `mermaid.js` inline and calls `mermaid.run` against that selector,
//!    but only when the document actually has one (see call site in [`render_document`]).

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use pulldown_cmark::{
    BlockQuoteKind, CodeBlockKind, CowStr, Event, HeadingLevel, LinkType, Options, Parser, Tag,
    TagEnd,
};
use tasty_plugin_sdk::Translator;
use tasty_type_appearance::theme::Theme;

/// Marker preceding the encoded payload in every internal-nav URL fragment
/// (`#tasty-nav:link:<enc>` / `#tasty-nav:addr:<enc>`). Shared by the generator
/// ([`rewrite_link_dest`], [`nav_script`]) and the consumer (`main.rs`'s
/// `on_webview_navigation_attempt` handler via [`parse_nav_fragment`]).
pub const NAV_FRAGMENT_MARKER: &str = "tasty-nav:";

/// Outcome of clicking a markdown link or submitting the address bar, raised so the plugin
/// shell performs the side effect (host `file_handler.dispatch` for files / OS open for URLs).
///
/// - `File`: a filesystem path already made absolute against the md dir's `base_dir`.
/// - `External`: a URL/scheme handed to the OS (`http(s)`, `mailto:`, `data:`, …).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LinkClick {
    File(PathBuf),
    External(String),
}

/// A decoded `#tasty-nav:` fragment — which kind of trusted-side interaction it came from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NavIntent {
    /// A content link (`<a>`) was clicked — `dest` is the original, un-rewritten href.
    Link(String),
    /// The address bar's Go action (click or Enter) fired — `path` is the input's raw value.
    Addr(String),
}

/// Parser options mirroring GFM: tables, task lists, strikethrough, footnotes, definition
/// lists, alert blockquotes (`> [!NOTE]` etc — [`Options::ENABLE_GFM`] is the *only* flag
/// `scan_blockquote_tag` gates on in pulldown-cmark 0.12's `firstpass.rs`, so it touches
/// nothing else already enabled here). Shared by [`unsafe_content_html`] and any pre-scan.
///
/// - `ENABLE_YAML_STYLE_METADATA_BLOCKS`/`ENABLE_PLUSES_DELIMITED_METADATA_BLOCKS` — hides
///   leading `---`/`+++` frontmatter (Jekyll/Hugo/Obsidian/Zettlr convention) from the
///   rendered body instead of letting it fall through to CommonMark's thematic-break/setext-
///   heading misparse (`---\nkey: value\n---` → a bogus `<h2>key: value</h2>`). This is a
///   pure hide, not a metadata panel — pulldown-cmark's HTML writer already treats
///   `Tag::MetadataBlock` as non-writing (`html.rs`'s `in_non_writing_block`), so no extra
///   event handling is needed here. Only recognized when the block is the very first thing
///   in the source (CommonMark frontmatter rule) — a `---` later in the document still
///   parses as an ordinary thematic break.
/// - `ENABLE_SMART_PUNCTUATION` — always-on typographic substitution (curly quotes, en/em
///   dash, ellipsis). No settings toggle: this viewer is a general document viewer, not a
///   spec-literal CommonMark renderer, so the nicer default (matching Obsidian et al.)
///   outweighs staying byte-identical to GitHub's rendering.
fn parser_options() -> Options {
    Options::ENABLE_TABLES
        | Options::ENABLE_TASKLISTS
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_FOOTNOTES
        | Options::ENABLE_DEFINITION_LIST
        | Options::ENABLE_YAML_STYLE_METADATA_BLOCKS
        | Options::ENABLE_PLUSES_DELIMITED_METADATA_BLOCKS
        | Options::ENABLE_SMART_PUNCTUATION
        | Options::ENABLE_GFM
        | Options::ENABLE_MATH
}

/// Everything [`render_document`] needs. A struct (rather than a long parameter list) since
/// several fields are independently optional/derived — mirrors the former `draw()` args.
pub struct DocumentInput<'a> {
    pub theme: &'a Theme,
    pub tr: &'a Translator,
    pub file_path: &'a str,
    pub source: &'a str,
    pub load_error: Option<&'a str>,
    pub base_dir: Option<&'a Path>,
    /// Newest-first recent paths (already capped upstream by `recent.query`) — baked into the
    /// address bar's `<datalist>` at generation time (no async JS fetch / native message
    /// bridge exists for a webview surface, see module doc).
    pub recent: &'a [String],
}

/// Build the complete, self-contained HTML5 document for the markdown webview surface.
pub fn render_document(input: DocumentInput) -> String {
    let DocumentInput {
        theme,
        tr,
        file_path,
        source,
        load_error,
        base_dir,
        recent,
    } = input;

    let base_tag = base_dir
        .map(|dir| format!(r#"<base href="{}">"#, attr_escape(&file_dir_uri(dir))))
        .unwrap_or_default();

    let (body_html, headings) = if let Some(err) = load_error {
        (
            format!(
                r#"<div class="tasty-state tasty-state-error"><div class="tasty-state-title">{}</div><pre class="tasty-state-detail">{}</pre></div>"#,
                html_escape(tr.t("markdown.state.failed")),
                html_escape(err)
            ),
            Vec::new(),
        )
    } else if source.trim().is_empty() {
        (
            format!(
                r#"<div class="tasty-state">{}</div>"#,
                html_escape(tr.t("markdown.state.empty"))
            ),
            Vec::new(),
        )
    } else {
        (
            sanitize_html(&unsafe_content_html_in_dir(source, tr, base_dir)),
            collect_headings(source),
        )
    };

    // heading 이 하나도 없으면 TOC 영역 자체를 렌더하지 않는다(빈 nav 로 깨지지 않게).
    let toc_html = if headings.is_empty() {
        String::new()
    } else {
        toc_nav_html(tr, &headings)
    };

    // 3.5MB 번들이라 mermaid 블록이 실제로 있는 문서에서만 삽입한다 — 대다수 문서는
    // mermaid 를 쓰지 않으므로 매번 inline 하면 순수 낭비다.
    let mermaid = if body_html.contains("language-mermaid") {
        mermaid_script(theme.is_light)
    } else {
        String::new()
    };

    // 마찬가지로 펜스드 코드블록이 하나도 없는 문서(대다수)에서는 삽입하지 않는다. mermaid
    // 전용 문서에서도 이 substring 검사는 걸리지만(mermaid 블록도 `class="language-mermaid"`
    // 라는 같은 모양의 class 를 가짐) — 벤더링한 번들엔 "mermaid" 라는 언어가 없으므로
    // `highlight_script` 는 아무것도 하이라이팅하지 않고 조용히 끝난다(125KB 낭비는 있지만
    // mermaid 를 조건에서 정교하게 제외하는 별도 스캐너를 두는 것보다 mermaid_script 와 같은
    // 수준의 단순한 substring 검사를 유지하는 쪽을 택했다).
    let highlight = if body_html.contains("class=\"language-") {
        highlight_script()
    } else {
        String::new()
    };

    // Same "only inline when there's something to act on" convention as mermaid/highlight above
    // — a document with no code blocks at all (most non-technical documents) skips this too.
    // `<pre><code` matches both the labeled fenced-block shape (`<pre><code class="language-…">`)
    // and the unlabeled/indented shape (`<pre><code>`, no class — pulldown-cmark's own
    // `lang.is_empty()` branch), since the substring check doesn't care what (if anything)
    // follows `<code`.
    let copy_buttons = if body_html.contains("<pre><code") {
        copy_button_script(tr)
    } else {
        String::new()
    };

    // Same convention again — most documents have no images at all, so skip the attach script
    // entirely when there's nothing for it to watch.
    let image_errors = if body_html.contains("<img") {
        image_error_script(tr)
    } else {
        String::new()
    };

    // Same convention again — the KaTeX bundle+fonts are ~1MB embedded, so most documents (no
    // `$...$`/`$$...$$` math) skip it entirely. `class="math math-` matches both
    // `math math-inline` and `math math-display` (pulldown-cmark's `ENABLE_MATH` HTML writer's
    // only two possible class values), so one substring check covers both.
    let math = if body_html.contains("class=\"math math-") {
        katex_script()
    } else {
        String::new()
    };

    format!(
        r#"<!doctype html><html><head><meta charset="utf-8">{base_tag}<style>{css}</style></head><body>{addr_bar}{find_bar}{toc_html}<div id="tasty-md-body">{body_html}</div><script>{script}</script><script>{find_script}</script>{highlight}{mermaid}{copy_buttons}{image_errors}{math}</body></html>"#,
        base_tag = base_tag,
        css = theme_css(theme),
        addr_bar = addr_bar_html(tr, file_path, recent),
        find_bar = find_bar_html(tr),
        toc_html = toc_html,
        body_html = body_html,
        script = nav_script(file_path),
        find_script = find_in_page_script(tr),
        highlight = highlight,
        mermaid = mermaid,
        copy_buttons = copy_buttons,
        image_errors = image_errors,
        math = math,
    )
}

/// `file://<dir>/` URI for a `<base href>` tag — every relative `href`/`src` in the document
/// resolves against it. Absolute local paths (`/abs/img.png`) and already-schemed remote URLs
/// are untouched by a `<base>` tag (only genuinely relative references are affected), so unlike
/// the former `image_uri_prefix` this needs no separate absolute-path special case.
fn file_dir_uri(dir: &Path) -> String {
    let normalized = dir.to_string_lossy().replace('\\', "/");
    let with_slash = if normalized.ends_with('/') {
        normalized
    } else {
        format!("{normalized}/")
    };
    let path_part = with_slash.strip_prefix('/').unwrap_or(&with_slash);
    format!("file:///{}", percent_encode_path(path_part))
}

/// Parse the URL host WebKitGTK reports via `webview.navigation_attempt` (Stage A) — everything
/// up to and including the last occurrence of [`NAV_FRAGMENT_MARKER`] is the document's own
/// location (irrelevant — may be `about:blank` or, when a `<base href>` is set, that base URI);
/// only the payload after the marker matters. Returns `None` if the URL carries no internal-nav
/// fragment at all (host chrome shouldn't normally forward anything else, but a defensive `None`
/// keeps this robust against unrelated navigation attempts).
pub fn parse_nav_fragment(url: &str) -> Option<NavIntent> {
    let idx = url.rfind(NAV_FRAGMENT_MARKER)?;
    let payload = &url[idx + NAV_FRAGMENT_MARKER.len()..];
    if let Some(enc) = payload.strip_prefix("link:") {
        return Some(NavIntent::Link(percent_decode(enc)));
    }
    if let Some(enc) = payload.strip_prefix("addr:") {
        return Some(NavIntent::Addr(percent_decode(enc)));
    }
    None
}

/// Classify a link/address-bar destination for host-side dispatch. Empty/anchor-only/
/// `javascript:` destinations are `None` (ignored — the last of these closes the sanitization
/// gap: `javascript:` has no `://` so it would otherwise fall through to the `File` branch).
/// `mailto:`/`data:`/any `scheme://` destination is `External`; everything else is resolved as
/// a filesystem path against `base_dir`.
pub fn classify_link(dest: &str, base_dir: Option<&Path>) -> Option<LinkClick> {
    let dest = dest.trim();
    if dest.is_empty() || dest.starts_with('#') {
        return None;
    }
    if dest.to_ascii_lowercase().starts_with("javascript:") {
        return None;
    }
    if dest.contains("://") || dest.starts_with("mailto:") || dest.starts_with("data:") {
        return Some(LinkClick::External(dest.to_string()));
    }
    let path = Path::new(dest);
    let joined = if path.is_absolute() {
        path.to_path_buf()
    } else {
        base_dir?.join(path)
    };
    // cwd 기준 절대화 후 lexical 정규화로 `..` 를 붕괴 (Unix `absolute` 은 `..` 보존).
    let abs = std::path::absolute(&joined).unwrap_or(joined);
    let abs = lexically_normalize(&abs);
    let abs = PathBuf::from(strip_verbatim_prefix(&abs.to_string_lossy()));
    Some(LinkClick::File(abs))
}

/// Strip a Windows verbatim (`\\?\`) prefix so resolved paths read cleanly cross-platform.
fn strip_verbatim_prefix(s: &str) -> String {
    s.strip_prefix(r"\\?\")
        .map(|rest| {
            rest.strip_prefix(r"UNC\")
                .map(|u| format!(r"\\{u}"))
                .unwrap_or_else(|| rest.to_string())
        })
        .unwrap_or_else(|| s.to_string())
}

/// Collapse `.` / `..` segments purely lexically (no filesystem access).
///
/// `std::path::absolute` preserves `..` on Unix for symlink safety (`/a/b/../c` isn't
/// folded to `/a/c` because `b` might be a symlink), so it can't normalize link paths
/// on its own. This folds `..` identically on every platform — for markdown link
/// display/dedup a lexical collapse matches intent and keeps dedup keys stable.
///
/// Trade-off: when a collapsed segment is a symlink the lexical result can diverge from
/// the OS's real resolution; for markdown link use this is intentional. Never climbs
/// above the root/prefix. (Local copy — the plugin doesn't depend on `tasty-utils`,
/// mirroring `strip_verbatim_prefix` above.)
fn lexically_normalize(path: &Path) -> PathBuf {
    use std::path::Component;
    let mut out = PathBuf::new();
    for comp in path.components() {
        match comp {
            Component::CurDir => {}
            Component::ParentDir => match out.components().next_back() {
                Some(Component::Normal(_)) => {
                    out.pop();
                }
                Some(Component::RootDir | Component::Prefix(_)) => {}
                _ => out.push(".."),
            },
            other => out.push(other.as_os_str()),
        }
    }
    out
}

// ── heading ids + TOC ───────────────────────────────────────────────────────────

/// One heading captured in document order — its plain text (for slug derivation and TOC label)
/// and the slug ultimately assigned as its `id` (see [`collect_headings`]/[`assign_heading_ids`]).
#[derive(Clone, Debug)]
struct HeadingInfo {
    level: HeadingLevel,
    /// Markup-stripped text — exactly what a reader sees as the heading's own text, no
    /// code/link/emphasis/image-alt syntax (module design decision: no explicit `{#id}`
    /// syntax, auto slug only).
    text: String,
    /// GitHub-compatible slug, deduped against every earlier heading in the same document.
    slug: String,
}

/// Pass 1 of the two-pass heading-id pipeline: walks the event stream purely to capture each
/// heading's plain text and turn it into a per-document-unique slug. pulldown-cmark's flat event
/// stream doesn't expose a heading's full text until its `TagEnd::Heading` arrives (nested
/// emphasis/link/code events land as separate stream items in between), so this can't be done in
/// a single `map()` like [`rewrite_link_event`]/[`rewrite_code_block_event`] — it needs to
/// buffer. [`assign_heading_ids`] later re-parses the same `source` and pairs this pass's output
/// back onto each `Tag::Heading` event by document order.
///
/// `Event::Text`/`Event::Code` between a heading's `Start`/`End` are concatenated (covers plain
/// text, inline code, and — since pulldown-cmark emits image alt text as `Event::Text` between
/// `Tag::Image`'s start/end — image alt text too); every other event (the `Start`/`End` tags of
/// emphasis/strong/link/image themselves) is ignored, which is exactly "strip the markup, keep
/// the text" (task requirement).
fn collect_headings(source: &str) -> Vec<HeadingInfo> {
    let mut headings = Vec::new();
    let mut current: Option<(HeadingLevel, String)> = None;
    let mut slugger = Slugger::default();
    for event in Parser::new_ext(source, parser_options()) {
        match event {
            Event::Start(Tag::Heading { level, .. }) => current = Some((level, String::new())),
            Event::End(TagEnd::Heading(_)) => {
                if let Some((level, text)) = current.take() {
                    let slug = slugger.slug(&text);
                    headings.push(HeadingInfo { level, text, slug });
                }
            }
            Event::Text(t) | Event::Code(t) => {
                if let Some((_, text)) = current.as_mut() {
                    text.push_str(&t);
                }
            }
            _ => {}
        }
    }
    headings
}

/// GitHub-compatible heading slug allocator — one instance per document so the dedup counters
/// are shared across every heading (`-1`/`-2` suffixes on the 2nd/3rd occurrence of the same
/// text, matching GitHub's own heading-anchor behavior).
#[derive(Default)]
struct Slugger {
    seen: HashMap<String, u32>,
}

impl Slugger {
    /// Lowercase; Unicode letters/digits/`-`/`_` kept (so non-ASCII text like Korean passes
    /// through untouched — `char::is_alphanumeric` is Unicode-aware, not ASCII-only), runs of
    /// whitespace collapsed to a single `-`, everything else (ASCII punctuation, markup residue,
    /// emoji, …) dropped, leading/trailing `-` trimmed. This is a closer-to-intent variant of
    /// GitHub's own algorithm rather than a byte-exact port (GitHub doesn't collapse/trim) —
    /// deemed an acceptable, more robust deviation, since "GFM 호환" here means "sane anchors
    /// GitHub users would recognize", not byte-identical output. Falls back to `"heading"` when
    /// the input slugifies to nothing at all (an all-punctuation/all-emoji heading) so the `id`
    /// is never empty.
    fn slug(&mut self, text: &str) -> String {
        let mut base = String::with_capacity(text.len());
        for c in text.chars() {
            if c.is_whitespace() {
                if !base.ends_with('-') && !base.is_empty() {
                    base.push('-');
                }
            } else if c.is_alphanumeric() || c == '-' || c == '_' {
                base.extend(c.to_lowercase());
            }
            // else: drop (punctuation/symbols/markup residue).
        }
        let base = base.trim_matches('-');
        let base = if base.is_empty() {
            "heading".to_string()
        } else {
            base.to_string()
        };

        let count = self.seen.entry(base.clone()).or_insert(0);
        let slug = if *count == 0 {
            base
        } else {
            format!("{base}-{count}")
        };
        *count += 1;
        slug
    }
}

/// Pass 2: rewrites each `Tag::Heading`'s `id` field to the slug [`collect_headings`] computed
/// for it, matched purely by document order (both passes parse the identical `source`, so
/// pulldown-cmark yields headings in the same order both times — index-pairing is safe). The
/// HTML writer honors `Tag::Heading::id` unconditionally whenever it's `Some` (`html.rs`) — this
/// needs no `Options::ENABLE_HEADING_ATTRIBUTES` (that option only governs the *parser*
/// recognizing an explicit `{#id}` in the source text, which this module intentionally never
/// enables — design decision: auto slugs only, no explicit-id syntax).
fn assign_heading_ids<'a>(
    events: impl Iterator<Item = Event<'a>> + 'a,
    headings: &[HeadingInfo],
) -> impl Iterator<Item = Event<'a>> + 'a {
    let mut slugs = headings
        .iter()
        .map(|h| h.slug.clone())
        .collect::<Vec<_>>()
        .into_iter();
    events.map(move |event| match event {
        Event::Start(Tag::Heading {
            level,
            id,
            classes,
            attrs,
        }) => {
            let id = slugs.next().map(Into::into).or(id);
            Event::Start(Tag::Heading {
                level,
                id,
                classes,
                attrs,
            })
        }
        other => other,
    })
}

/// Builds the collapsible in-document TOC `<nav>` (design decision: inline top-of-document, not
/// a sticky side panel — see module/task doc). Nested indentation is per-level via
/// `tasty-toc-l<N>` classes ([`theme_css`]'s TOC rules). Caller skips this entirely when there
/// are no headings (see call site in [`render_document`]) rather than rendering an empty shell.
fn toc_nav_html(tr: &Translator, headings: &[HeadingInfo]) -> String {
    let items: String = headings
        .iter()
        .map(|h| {
            format!(
                r##"<li class="tasty-toc-l{level}"><a href="#{slug}">{text}</a></li>"##,
                level = h.level as u8,
                slug = attr_escape(&h.slug),
                text = html_escape(&h.text),
            )
        })
        .collect();
    format!(
        r#"<nav id="tasty-toc" aria-label="{aria}"><button id="tasty-toc-toggle" type="button" aria-expanded="true">{label}</button><ul id="tasty-toc-list">{items}</ul></nav>"#,
        aria = attr_escape(tr.t("markdown.toc.aria_label")),
        label = html_escape(tr.t("markdown.toc.label")),
        items = items,
    )
}

// ── HTML generation ───────────────────────────────────────────────────────────

/// Parse `source` and generate (unsanitized) HTML, rewriting every link destination to the
/// internal nav-fragment scheme first (module doc — never a plain `href` to a local/external
/// target). An image alone in its own paragraph gets promoted to a captioned `<figure>`
/// ([`figurize_solo_image_paragraphs`]) — every other image (mixed into running text, wrapped in
/// a link, alt-less) passes through untouched, same as before that pass existed. The `<base
/// href>` tag resolves relative `src`. Headings get a GitHub-compatible `id` via the
/// [`collect_headings`]/[`assign_heading_ids`] two-pass pipeline (module doc "heading ids + TOC"
/// section) — this recomputes the heading list itself (a cheap, HTML-free text-only walk) rather
/// than taking it as a parameter, so this function's signature — and every existing call
/// site/test — stays unchanged; [`render_document`] computes its own separate copy for the TOC
/// (same deterministic result, since both walk the identical `source`).
///
/// Pass ordering: [`figurize_solo_image_paragraphs`] runs before [`autolink_bare_urls`] — it's a
/// structural (block-level) promotion, decided purely from paragraph shape, so it runs first and
/// leaves every other inline-text rewrite (autolinking, nav-fragment rewriting) to work on
/// whatever text/image events actually survive that decision, exactly as if that pass didn't
/// exist for paragraphs it declines to touch. [`resolve_wikilinks`] runs next, right before
/// [`autolink_bare_urls`] — both are the same shape of pass (scan merged `Event::Text` runs for a
/// library-unknown syntax, synthesize `Tag::Link` events with a plain raw `dest_url`), and each
/// already excludes text inside a link/code-block via its own `link_depth`/`code_block_depth`
/// tracking, so their relative order can't cause either to double-process the other's output
/// regardless of which runs first — they're placed adjacently here only because they're
/// conceptually paired, not because ordering is load-bearing. [`autolink_bare_urls`] itself runs
/// *before* [`rewrite_link_event`], and synthesizes its new `Tag::Link` events with a plain raw
/// `dest_url` (e.g. `https://example.com`) — the exact same shape an explicit `[text](url)` link
/// has at this point in the pipeline (wikilink events have this shape too, see
/// [`wikilink_events`]). This way `rewrite_link_event`, run last over the *whole* (now
/// autolink/wikilink-expanded) event stream, is the single place that ever produces the
/// `#tasty-nav:` fragment scheme; neither pass needs its own copy of that rewrite.
/// `rewrite_code_block_event`/`rewrite_footnote_event` run first since neither touches
/// `Text`/`Link`/`Image` events, so their relative order doesn't matter. [`rewrite_callout_events`]
/// runs right after — it *does* consume `Text` events, but only the ones immediately inside a
/// blockquote's first paragraph that match a `[!type]` tag line, which no other pass here ever
/// produces or depends on, so it's still safe before [`figurize_solo_image_paragraphs`]/
/// [`resolve_wikilinks`]/[`autolink_bare_urls`]/[`rewrite_link_event`]. [`assign_heading_ids`]
/// runs last over the fully-rewritten stream — none of the other passes touch `Tag::Heading`, so
/// its position doesn't matter either.
#[cfg(test)]
fn unsafe_content_html(source: &str, tr: &Translator) -> String {
    unsafe_content_html_in_dir(source, tr, None)
}

/// Same as [`unsafe_content_html`], additionally resolving `[[wikilink]]` targets against
/// `base_dir` ([`resolve_wikilinks`]). Split out under its own name so the large existing block of
/// regression tests that don't exercise wikilink resolution keep calling the stable 2-arg form
/// unchanged; [`render_document`] — the only real production caller — uses this one, with its own
/// `base_dir` threaded straight through.
fn unsafe_content_html_in_dir(source: &str, tr: &Translator, base_dir: Option<&Path>) -> String {
    let headings = collect_headings(source);
    let footnote_ref_totals = footnote_reference_totals(source);
    let mut footnote_state = FootnoteState::default();
    let events: Vec<Event> = Parser::new_ext(source, parser_options())
        .map(rewrite_code_block_event)
        .map(|event| rewrite_footnote_event(event, tr, &footnote_ref_totals, &mut footnote_state))
        .collect();
    let events = rewrite_callout_events(events, tr);
    let events = figurize_solo_image_paragraphs(events);
    let events = resolve_wikilinks(events, base_dir);
    let events = autolink_bare_urls(events)
        .into_iter()
        .map(rewrite_link_event);
    let events = assign_heading_ids(events, &headings);
    let mut html = String::new();
    pulldown_cmark::html::push_html(&mut html, events);
    html
}

// ── image captions (solo-image paragraphs → <figure>/<figcaption>) ─────────────

/// Promotes a paragraph that consists of **nothing but** a single image to
/// `<figure><img.../><figcaption>{alt}</figcaption></figure>` — the alt text becomes a visible
/// caption instead of living only in the invisible `alt` attribute. Policy: this happens
/// automatically whenever such a paragraph has non-empty alt text, no opt-in syntax required —
/// alt-less images (`![](img.png)`) are unaffected (nothing to caption).
///
/// `Tag::Image` is an *inline* element — pulldown-cmark's own HTML writer only ever emits it
/// inside a `<p>...</p>` (or another inline context). Wrapping just the `Tag::Image` span itself
/// in `<figure>` (a block element) would leave `<p><figure>...` in the output — invalid nesting
/// that browsers "fix" by closing the `<p>` early in unpredictable ways. So this promotes the
/// *whole paragraph* instead, and only when the image is truly alone in it: any other inline
/// content in the same paragraph (more text, another image, a link wrapping the image, …)
/// disqualifies the paragraph and it passes through completely unchanged (original
/// `<p><img.../></p>`) — safer to skip the caption than to ever risk invalid HTML.
///
/// Buffers each `Tag::Paragraph`'s events, mirroring [`collect_headings`]'s "buffer until the
/// matching end tag, since pulldown-cmark's flat stream doesn't expose a container's full content
/// until it closes" approach. Paragraphs can't nest (CommonMark block grammar has no paragraph-
/// inside-paragraph production), so a single non-recursive buffer-until-`TagEnd::Paragraph` is
/// safe here — unlike a genuinely nestable container (blockquote/list), there's no risk of a
/// second `Tag::Paragraph` opening before this one closes.
fn figurize_solo_image_paragraphs(events: Vec<Event<'_>>) -> Vec<Event<'_>> {
    let mut out = Vec::with_capacity(events.len());
    let mut iter = events.into_iter();
    while let Some(event) = iter.next() {
        match event {
            Event::Start(Tag::Paragraph) => {
                let mut buf = Vec::new();
                for inner in iter.by_ref() {
                    if matches!(inner, Event::End(TagEnd::Paragraph)) {
                        break;
                    }
                    buf.push(inner);
                }
                out.extend(figurize_paragraph_buffer(buf));
            }
            other => out.push(other),
        }
    }
    out
}

/// Re-wraps a buffered paragraph's interior events (stripped of its `Tag::Paragraph` start/end by
/// [`figurize_solo_image_paragraphs`]) back into an ordinary `<p>...</p>` — used by
/// [`figurize_paragraph_buffer`] on every path that declines to promote.
fn wrap_as_paragraph(buf: Vec<Event<'_>>) -> Vec<Event<'_>> {
    let mut out = Vec::with_capacity(buf.len() + 2);
    out.push(Event::Start(Tag::Paragraph));
    out.extend(buf);
    out.push(Event::End(TagEnd::Paragraph));
    out
}

/// One buffered paragraph's events → either the `<figure>` promotion or the original paragraph
/// passed through unchanged.
fn figurize_paragraph_buffer(buf: Vec<Event<'_>>) -> Vec<Event<'_>> {
    // Find at most one top-level `Tag::Image` span, matching its end via nest-counting — the
    // same generic Start/End balancing algorithm pulldown-cmark's own `html.rs::raw_text()` uses
    // to build the `alt` attribute, so this locates exactly the span that writer will consume.
    let mut image_span: Option<(usize, usize)> = None;
    let mut i = 0;
    while i < buf.len() {
        if matches!(buf[i], Event::Start(Tag::Image { .. })) {
            if image_span.is_some() {
                return wrap_as_paragraph(buf); // more than one image in this paragraph — bail.
            }
            let mut nest = 0i32;
            let mut end_idx = None;
            let mut j = i + 1;
            while j < buf.len() {
                match &buf[j] {
                    Event::Start(_) => nest += 1,
                    Event::End(TagEnd::Image) if nest == 0 => {
                        end_idx = Some(j);
                        break;
                    }
                    Event::End(_) => nest -= 1,
                    _ => {}
                }
                j += 1;
            }
            let Some(end_idx) = end_idx else {
                return wrap_as_paragraph(buf); // unmatched — defensive, shouldn't happen.
            };
            image_span = Some((i, end_idx));
            i = end_idx;
        }
        i += 1;
    }
    let Some((start_idx, end_idx)) = image_span else {
        return wrap_as_paragraph(buf); // no image at all — an ordinary text paragraph.
    };

    // Everything outside the image span must be whitespace-only text — any other inline content
    // (more text, a link wrapping the image, a second image, …) disqualifies promotion.
    let outside_is_blank = buf.iter().enumerate().all(|(idx, ev)| {
        (start_idx..=end_idx).contains(&idx) || matches!(ev, Event::Text(t) if t.trim().is_empty())
    });
    if !outside_is_blank {
        return wrap_as_paragraph(buf);
    }

    // Alt text: markup-stripped concatenation of Text/Code inside the image span — same "keep
    // the text, drop the markup" collection [`collect_headings`] uses for heading text.
    let mut alt = String::new();
    for ev in &buf[start_idx + 1..end_idx] {
        match ev {
            Event::Text(t) | Event::Code(t) => alt.push_str(t),
            _ => {}
        }
    }
    if alt.trim().is_empty() {
        return wrap_as_paragraph(buf); // no alt text — nothing to caption, leave it as-is.
    }

    // The image span itself (`Tag::Image` start/inner/end) is kept byte-for-byte as pulldown-cmark
    // produced it and handed to the HTML writer unchanged, so `<img src=".." alt=".." title="..">`
    // is still built by the library's own `raw_text()` exactly as it always was — only the
    // surrounding structure changes. `Event::Html` is this file's established "trusted,
    // plugin-authored raw markup" escape hatch (mirrors `wrap_static_callout`'s
    // `<blockquote class=".." data-label="..">` injection) for the `<figure>`/`<figcaption>` tags
    // themselves; the caption text rides in as a plain `Event::Text`, so `push_html` HTML-escapes
    // it exactly like any other body text (no separate escaping call needed here).
    let mut buf = buf;
    let image_events: Vec<Event<'_>> = buf.drain(start_idx..=end_idx).collect();
    let mut out = Vec::with_capacity(image_events.len() + 4);
    out.push(Event::Html("<figure>".into()));
    out.extend(image_events);
    out.push(Event::Html("<figcaption>".into()));
    out.push(Event::Text(alt.into()));
    out.push(Event::Html("</figcaption></figure>".into()));
    out
}

// ── Bare `http(s)://` autolinking ───────────────────────────────────────────────

/// Schemes this pass recognizes for bare-URL autolinking. `www.`-prefixed (schemeless) hosts
/// and email addresses are out of scope for now (conductor-scoped — a separate TODO if needed).
/// find 히트 하이라이트 배경의 알파. 대응 토큰이 없어 값에 이름만 둔다.
/// (`alert_css` 의 `BG_ALPHA` 와 값 공간은 같고 역할이 다르다.)
const FIND_HIT_BG_ALPHA: u8 = 90;

/// diff 추가/삭제 줄 배경의 알파 — `gamma_multiply(0.12)` 과 같은 비율을
/// 알파 공간으로 옮긴 값이다. 대응 토큰 없음.
const DIFF_LINE_BG_ALPHA: u8 = 31;

const AUTOLINK_SCHEMES: &[&str] = &["https://", "http://"];

/// Trailing characters stripped one at a time from the end of a matched URL run — mirrors GFM's
/// extended-autolink "trailing punctuation" rule so a URL at the end of a sentence/quote doesn't
/// swallow the punctuation with it (`https://example.com.` → link stops before the `.`). `)` is
/// handled separately below via paren-balance, not blanket-stripped, since a balanced trailing
/// `)` (e.g. a wiki URL) is legitimately part of the URL.
const TRAILING_PUNCTUATION: &[char] = &['.', ',', ';', ':', '!', '?', '\'', '"', '*', '_', '~'];

/// Splits bare `http(s)://` URLs found in plain text into `Tag::Link` spans.
///
/// Must be **stateful**, not a plain per-event `map()` like [`rewrite_link_event`]/
/// [`rewrite_code_block_event`]: whether a given `Event::Text` is eligible depends on events
/// *around* it, not just its own content —
/// - text already inside an explicit `Tag::Link` (`[https://x](https://x)`) must be left alone
///   (no nested/duplicate link), tracked via `link_depth`;
/// - text inside a `Tag::CodeBlock` (fenced or indented) must be left alone, tracked via
///   `code_block_depth`. Inline code doesn't need separate tracking — pulldown-cmark never
///   represents inline-code content as `Event::Text` in the first place (it's the distinct
///   `Event::Code` variant), so it's already excluded by construction.
///
/// It must also **buffer across event boundaries**: probing pulldown-cmark 0.12.2 directly
/// showed a single visual URL can arrive as *multiple* `Event::Text` events, because a lone
/// `*`/`_` inside the URL that doesn't pair up into real emphasis still gets tokenized as its
/// own one-character `Event::Text` (e.g. `.../foo*bar` → `Text("...foo")`, `Text("*")`,
/// `Text("bar...")`; `.../Rust_(lang)` splits the same way around the first `_`). This pass
/// therefore merges every consecutive run of eligible `Event::Text` events into one buffer
/// before scanning it for URLs — a non-text event (real markup, `SoftBreak`, …) always flushes
/// the buffer first, so genuine markup boundaries are never stitched across.
fn autolink_bare_urls(events: Vec<Event<'_>>) -> Vec<Event<'_>> {
    let mut out = Vec::with_capacity(events.len());
    let mut link_depth: u32 = 0;
    let mut code_block_depth: u32 = 0;
    let mut run = String::new();

    for event in events {
        match event {
            Event::Text(ref text) if link_depth == 0 && code_block_depth == 0 => {
                run.push_str(text);
            }
            other => {
                if !run.is_empty() {
                    out.extend(split_bare_urls(std::mem::take(&mut run)));
                }
                match &other {
                    Event::Start(Tag::Link { .. }) => link_depth += 1,
                    Event::End(TagEnd::Link) => link_depth = link_depth.saturating_sub(1),
                    Event::Start(Tag::CodeBlock(_)) => code_block_depth += 1,
                    Event::End(TagEnd::CodeBlock) => {
                        code_block_depth = code_block_depth.saturating_sub(1)
                    }
                    _ => {}
                }
                out.push(other);
            }
        }
    }
    if !run.is_empty() {
        out.extend(split_bare_urls(run));
    }
    out
}

/// Scan one merged plain-text run for bare URLs, emitting alternating `Event::Text` (plain) and
/// `Event::Start(Tag::Link)`/`Event::Text`/`Event::End(TagEnd::Link)` (matched URL) events.
/// `dest_url`/inner text are both the raw matched URL — [`rewrite_link_event`] rewrites the
/// destination into the nav-fragment scheme afterward (see [`unsafe_content_html`] doc).
///
/// Any HTML entity in the source markdown (`&amp;` etc) has already been decoded to a literal
/// character by pulldown-cmark's parser by the time it reaches an `Event::Text` — `push_html`
/// re-escapes it on the way out identically for autolinked and plain text, so no separate
/// entity handling is needed here.
fn split_bare_urls(text: String) -> Vec<Event<'static>> {
    let mut out = Vec::new();
    let mut plain_start = 0usize;
    let mut search_from = 0usize;

    while let Some(scheme_start) = find_scheme_start(&text[search_from..]).map(|i| i + search_from)
    {
        let url_end = scan_url_end(&text, scheme_start);
        if url_end <= scheme_start {
            // Defensive: a recognized scheme with nothing usable after it (shouldn't happen,
            // the scheme string itself is always non-whitespace). Skip past it and keep scanning.
            search_from = scheme_start + 1;
            continue;
        }

        if scheme_start > plain_start {
            out.push(Event::Text(
                text[plain_start..scheme_start].to_string().into(),
            ));
        }
        let url = text[scheme_start..url_end].to_string();
        out.push(Event::Start(Tag::Link {
            link_type: LinkType::Autolink,
            dest_url: CowStr::from(url.clone()),
            title: CowStr::from(""),
            id: CowStr::from(""),
        }));
        out.push(Event::Text(CowStr::from(url)));
        out.push(Event::End(TagEnd::Link));

        plain_start = url_end;
        search_from = url_end;
    }

    if plain_start < text.len() {
        out.push(Event::Text(text[plain_start..].to_string().into()));
    } else if out.is_empty() {
        // No URL found at all — return the run untouched as a single Text event rather than
        // an empty Vec, so callers never lose content.
        out.push(Event::Text(text.into()));
    }
    out
}

/// Byte offset (relative to `text`) of the earliest recognized [`AUTOLINK_SCHEMES`] occurrence,
/// or `None` if the text contains none. `https://` can never spuriously contain `http://` as a
/// substring (the 5th character differs, `s` vs `:`), so checking both independently and taking
/// the minimum can't double-count a single occurrence.
fn find_scheme_start(text: &str) -> Option<usize> {
    AUTOLINK_SCHEMES
        .iter()
        .filter_map(|scheme| text.find(scheme))
        .min()
}

/// Given confirmed scheme text starting at `scheme_start`, find the byte offset (absolute, in
/// `text`) where the URL run ends: the first ASCII whitespace or CommonMark autolink delimiter
/// (`<`/`>`), then trailing punctuation trimmed back per [`TRAILING_PUNCTUATION`] and paren
/// balance (only parens *within the matched run* count — an enclosing sentence's own `(`/`)`
/// around the whole URL, e.g. `(https://example.com)`, is irrelevant to this balance check).
fn scan_url_end(text: &str, scheme_start: usize) -> usize {
    let rest = &text[scheme_start..];
    let mut end = rest
        .char_indices()
        .find(|(_, c)| c.is_whitespace() || *c == '<' || *c == '>')
        .map(|(i, _)| i)
        .unwrap_or(rest.len());

    loop {
        let candidate = &rest[..end];
        let Some(last) = candidate.chars().last() else {
            break;
        };
        if TRAILING_PUNCTUATION.contains(&last) {
            end -= last.len_utf8();
            continue;
        }
        if last == ')' {
            let opens = candidate.matches('(').count();
            let closes = candidate.matches(')').count();
            if closes > opens {
                end -= 1; // ')' is a single ASCII byte
                continue;
            }
        }
        break;
    }
    scheme_start + end
}

// ── Obsidian-style wikilinks (`[[문서명]]` / `[[문서명|표시텍스트]]`) ────────────

/// Fixed, deliberately narrow scope (see `docs/plugins/markdown/screens/markdown.md` "위키링크"
/// section for the full rationale): resolution only ever looks for `<name>.md` in the exact same
/// directory as the current file (`base_dir`) — no vault-wide recursive search, no
/// case-insensitive matching, no alias handling. `[[name#heading]]` and `![[embed]]` are not
/// recognized as wikilinks at all (they fail [`parse_wikilink_body`] and pass through as literal
/// text, same as any other malformed wikilink body).
struct Wikilink {
    /// The `.md`-less document name as written inside `[[...]]`, already validated to contain
    /// neither `/`, `\`, nor `..` (see [`parse_wikilink_body`]).
    name: String,
    /// Link text: the `|`-delimited display text if present and non-blank, otherwise `name`.
    display: String,
}

/// Parse a `[[...]]` body (the text between the delimiters, not including them) into a
/// [`Wikilink`], or `None` if it isn't a valid wikilink — in which case the caller must leave the
/// original `[[...]]` text completely untouched (silent pass-through, no error): a nested `[[`,
/// an empty name, or a name containing `/`/`\`/`..` (not a single-segment same-directory
/// reference — path-traversal prevention) all fall through here. This intentionally does NOT
/// recognize `#heading` anchors — a name containing `#` is still accepted verbatim as a literal
/// (nonexistent) filename component, matching the explicit out-of-scope decision to not implement
/// `[[name#heading]]`.
fn parse_wikilink_body(body: &str) -> Option<Wikilink> {
    if body.contains("[[") {
        return None;
    }
    let (name_part, display_part) = match body.split_once('|') {
        Some((n, d)) => (n, Some(d)),
        None => (body, None),
    };
    let name = name_part.trim();
    if name.is_empty() || name.contains(['/', '\\']) || name.contains("..") {
        return None;
    }
    let display = display_part
        .map(str::trim)
        .filter(|d| !d.is_empty())
        .unwrap_or(name);
    Some(Wikilink {
        name: name.to_string(),
        display: display.to_string(),
    })
}

/// Turn one resolved [`Wikilink`] into its event sequence: a normal `Tag::Link` (destination is
/// the plain relative `<name>.md` — the exact same shape a hand-written `[text](name.md)` link
/// would have at this point in the pipeline), so [`rewrite_link_event`] rewrites it into the nav
/// fragment scheme afterward exactly like any other link — no separate resolution machinery.
///
/// When the target isn't found under `base_dir` (including when `base_dir` itself is `None` —
/// same "unresolvable" treatment [`classify_link`] already gives a relative destination with no
/// base directory), the link is still emitted (clicking it falls through to the existing
/// nonexistent-file handling), just wrapped in a `.tasty-wikilink-missing` span for the visual
/// distinction — `span`/`class` are already sanitizer-whitelisted (opened for KaTeX math spans),
/// so this needs no new [`sanitize_html`] allowance.
fn wikilink_events(link: Wikilink, base_dir: Option<&Path>) -> Vec<Event<'static>> {
    let dest = format!("{}.md", link.name);
    let exists = base_dir.is_some_and(|dir| dir.join(&dest).exists());

    let link_events = [
        Event::Start(Tag::Link {
            link_type: LinkType::Shortcut,
            dest_url: CowStr::from(dest),
            title: CowStr::from(""),
            id: CowStr::from(""),
        }),
        Event::Text(link.display.into()),
        Event::End(TagEnd::Link),
    ];

    if exists {
        link_events.to_vec()
    } else {
        let mut out = vec![Event::Html(CowStr::from(
            r#"<span class="tasty-wikilink-missing">"#,
        ))];
        out.extend(link_events);
        out.push(Event::Html(CowStr::from("</span>")));
        out
    }
}

/// Scan one merged plain-text run for `[[name]]`/`[[name|display]]` wikilinks, emitting
/// alternating `Event::Text` (plain) and wikilink-link events ([`wikilink_events`]). A `[[` with
/// no matching `]]` anywhere later in the run stops the scan (the rest is left as plain text) —
/// mirrors [`split_bare_urls`]'s structure exactly, substituting the delimiter/validation logic.
fn split_wikilinks(text: String, base_dir: Option<&Path>) -> Vec<Event<'static>> {
    let mut out = Vec::new();
    let mut plain_start = 0usize;
    let mut search_from = 0usize;

    while let Some(rel_open) = text[search_from..].find("[[") {
        let open = search_from + rel_open;
        let Some(rel_close) = text[open + 2..].find("]]") else {
            break;
        };
        let close = open + 2 + rel_close;
        let body = &text[open + 2..close];

        let Some(link) = parse_wikilink_body(body) else {
            // Not a valid wikilink body — leave this `[[` untouched and resume right after it,
            // so a `]]` later on the line can still pair with a *subsequent* `[[`.
            search_from = open + 2;
            continue;
        };

        if open > plain_start {
            out.push(Event::Text(text[plain_start..open].to_string().into()));
        }
        out.extend(wikilink_events(link, base_dir));
        plain_start = close + 2;
        search_from = plain_start;
    }

    if plain_start < text.len() {
        out.push(Event::Text(text[plain_start..].to_string().into()));
    } else if out.is_empty() {
        out.push(Event::Text(text.into()));
    }
    out
}

/// Rewrite `[[...]]` wikilink text into link events, resolved against `base_dir`. Mirrors
/// [`autolink_bare_urls`]'s architecture exactly (same doc comment reasoning applies here
/// verbatim): pulldown-cmark has no notion of this syntax, so `[[...]]` only ever reaches this
/// pass as literal `Event::Text`, buffered across event boundaries and flushed via
/// [`split_wikilinks`]; the same `link_depth`/`code_block_depth` tracking excludes text already
/// inside an explicit link or a code block. `Event::FootnoteReference` is a distinct event
/// variant — never `Event::Text` — so `[^name]` footnote syntax structurally cannot reach this
/// scanner regardless of pass ordering; `wikilink_does_not_collide_with_footnote_reference` below
/// confirms this empirically against the real parser output rather than resting on that
/// structural argument alone.
fn resolve_wikilinks<'a>(events: Vec<Event<'a>>, base_dir: Option<&Path>) -> Vec<Event<'a>> {
    let mut out = Vec::with_capacity(events.len());
    let mut link_depth: u32 = 0;
    let mut code_block_depth: u32 = 0;
    let mut run = String::new();

    for event in events {
        match event {
            Event::Text(ref text) if link_depth == 0 && code_block_depth == 0 => {
                run.push_str(text);
            }
            other => {
                if !run.is_empty() {
                    out.extend(split_wikilinks(std::mem::take(&mut run), base_dir));
                }
                match &other {
                    Event::Start(Tag::Link { .. }) => link_depth += 1,
                    Event::End(TagEnd::Link) => link_depth = link_depth.saturating_sub(1),
                    Event::Start(Tag::CodeBlock(_)) => code_block_depth += 1,
                    Event::End(TagEnd::CodeBlock) => {
                        code_block_depth = code_block_depth.saturating_sub(1)
                    }
                    _ => {}
                }
                out.push(other);
            }
        }
    }
    if !run.is_empty() {
        out.extend(split_wikilinks(run, base_dir));
    }
    out
}

fn rewrite_link_event(event: Event<'_>) -> Event<'_> {
    match event {
        Event::Start(Tag::Link {
            link_type,
            dest_url,
            title,
            id,
        }) => Event::Start(Tag::Link {
            link_type,
            dest_url: rewrite_link_dest(&dest_url).into(),
            title,
            id,
        }),
        other => other,
    }
}

/// Rewrite a raw markdown link destination into `#tasty-nav:link:<enc>`, unless it's an
/// anchor-only/empty destination (native same-page scroll is fine — no interception needed).
fn rewrite_link_dest(dest: &str) -> String {
    let trimmed = dest.trim();
    if trimmed.is_empty() || trimmed.starts_with('#') {
        return dest.to_string();
    }
    format!(
        "#{NAV_FRAGMENT_MARKER}link:{}",
        percent_encode_fragment(trimmed)
    )
}

/// Normalize a fenced code block's info-string language token before `push_html` turns it
/// into `<pre><code class="language-<lang>">` (pulldown-cmark 0.12 emits the class on `code`,
/// not `pre` — checked against its `html.rs` source). This is defense-in-depth on top of
/// `sanitize_html`'s allowlist (which would let *any* value through as long as the `class`
/// attribute itself is allowed): [`mermaid_script`]'s `querySelector` keys off this exact
/// `language-<lang>` class, so the value must be predictable — arbitrary characters from user
/// markdown (`​```rust"><script>…`) are stripped down to a plain identifier rather than passed
/// through as-is (they can't break out of the attribute either way, since pulldown-cmark
/// HTML-escapes the info string, but a predictable value matters for that consumer, not just
/// for safety).
fn rewrite_code_block_event(event: Event<'_>) -> Event<'_> {
    match event {
        Event::Start(Tag::CodeBlock(CodeBlockKind::Fenced(info))) => Event::Start(Tag::CodeBlock(
            CodeBlockKind::Fenced(sanitize_fence_lang(&info).into()),
        )),
        other => other,
    }
}

/// Keep only `[A-Za-z0-9_+-]` from the fence info string's first (language) token, capped at
/// 32 chars. Empty input/output means no class is emitted at all (matches pulldown-cmark's own
/// `lang.is_empty()` branch in `CodeBlockKind::Fenced` handling).
fn sanitize_fence_lang(info: &str) -> String {
    info.split(' ')
        .next()
        .unwrap_or("")
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '+' | '_'))
        .take(32)
        .collect()
}

// ── callouts (GFM `> [!NOTE]` alerts + Obsidian-style `> [!type]+ Title`) ──────

/// One callout type — either one of the 5 GitHub-style alert kinds pulldown-cmark's *parser*
/// itself recognizes with [`Options::ENABLE_GFM`] on, or one of Obsidian's extended types that
/// the parser never sees as a distinct AST shape (module doc below explains why). Both flavors
/// render through the exact same class/label/icon/accent machinery — this table (and
/// [`rewrite_callout_events`]) is the single unified path, not two parallel ones.
struct CalloutKind {
    /// `Some` only for the 5 fixed literal tags pulldown-cmark's own `scanners.rs::
    /// scan_blockquote_tag` recognizes — matched against the real `Tag::BlockQuote(Some(kind))`
    /// AST event, never against rendered HTML text (see [`rewrite_callout_events`] doc for why
    /// that distinction matters). `None` for Obsidian-only extended types, which the parser
    /// always leaves as a plain `Tag::BlockQuote(None)` no matter how they're written — those
    /// are recognized instead from the buffered blockquote's first line of text
    /// ([`parse_callout_tag_line`]).
    gfm_kind: Option<BlockQuoteKind>,
    /// Lowercase Obsidian tag text this entry answers to (e.g. `"note"`, `"info"`) — matched
    /// case-insensitively against the `[!type]` token via [`find_callout_kind`]. For the 5 GFM
    /// kinds this is simply their lowercase name, so a bare `[!note]` and an Obsidian-flavored
    /// `[!note]+ Title` resolve to the same entry either way.
    type_key: &'static str,
    /// The literal class this kind renders as (mirrors pulldown-cmark's own `html.rs` naming
    /// for the 5 GFM kinds, kept identical so existing CSS/snapshots don't need to change).
    class: &'static str,
    /// `Translator` key for the default header label (used whenever no custom title follows the
    /// tag) — must exist in `lang/{en,ko,ja}.toml`.
    label_key: &'static str,
    /// Inner markup of a [`tasty_icons`] glyph (`Icon::body` — no wrapping `<svg>`, no color
    /// baked in). Fed to [`alert_icon_data_uri`].
    icon_body: &'static str,
    /// The glyph's own `Icon::filled` — `true` colors it via `fill`, `false` via `stroke`
    /// (mirrors how `tasty_icons`' `stroke_icon!`/`fill_icon!` macros built it).
    icon_filled: bool,
    /// This kind's `Theme` accent accessor. No dedicated callout design-token set exists yet, so
    /// every entry reuses one of the handful of existing generic semantic accents
    /// (`accent_primary`/`accent_info`/`accent_success`/`accent_warning`/`accent_attention`/
    /// `accent_danger`/`accent_agent`) rather than inventing new color tokens — with 15 types
    /// sharing 7 accents, several intentionally double up (distinguished by icon + label text,
    /// not uniquely by color).
    accent: fn(&Theme) -> tasty_type_appearance::color::HexColor,
}

/// The 5 GFM kinds (unchanged from before Obsidian support existed — same class/label/icon/
/// accent) plus Obsidian's own built-in types confirmed against the official callout
/// documentation (obsidian.md/help/callouts), minus the 3 Obsidian aliases (`important`,
/// `caution`, `attention`) that collide with a GFM kind's own name — those keywords keep
/// resolving to the pre-existing GFM entry instead of being redefined, since "GFM 5종 기존
/// 유지" is a hard requirement and [`find_callout_kind`] checks canonical `type_key`s (this
/// table) before consulting [`CALLOUT_ALIASES`]. Non-colliding Obsidian aliases (`hint`,
/// `summary`/`tldr`, `check`/`done`, `help`/`faq`, `fail`/`missing`, `error`, `cite`) live in
/// [`CALLOUT_ALIASES`] instead of duplicating entries here.
const CALLOUT_KINDS: &[CalloutKind] = &[
    CalloutKind {
        gfm_kind: Some(BlockQuoteKind::Note),
        type_key: "note",
        class: "markdown-alert-note",
        label_key: "markdown.alert.note",
        icon_body: tasty_icons::ALERT_CIRCLE.body,
        icon_filled: tasty_icons::ALERT_CIRCLE.filled,
        accent: Theme::accent_primary,
    },
    CalloutKind {
        gfm_kind: Some(BlockQuoteKind::Tip),
        type_key: "tip",
        class: "markdown-alert-tip",
        label_key: "markdown.alert.tip",
        icon_body: tasty_icons::STAR_FILL.body,
        icon_filled: tasty_icons::STAR_FILL.filled,
        accent: Theme::accent_success,
    },
    CalloutKind {
        gfm_kind: Some(BlockQuoteKind::Important),
        type_key: "important",
        class: "markdown-alert-important",
        label_key: "markdown.alert.important",
        icon_body: tasty_icons::BELL.body,
        icon_filled: tasty_icons::BELL.filled,
        accent: Theme::accent_agent,
    },
    CalloutKind {
        gfm_kind: Some(BlockQuoteKind::Warning),
        type_key: "warning",
        class: "markdown-alert-warning",
        label_key: "markdown.alert.warning",
        icon_body: tasty_icons::ALERT_TRIANGLE.body,
        icon_filled: tasty_icons::ALERT_TRIANGLE.filled,
        accent: Theme::accent_warning,
    },
    CalloutKind {
        gfm_kind: Some(BlockQuoteKind::Caution),
        type_key: "caution",
        class: "markdown-alert-caution",
        label_key: "markdown.alert.caution",
        icon_body: tasty_icons::CLOSE.body,
        icon_filled: tasty_icons::CLOSE.filled,
        accent: Theme::accent_danger,
    },
    CalloutKind {
        gfm_kind: None,
        type_key: "abstract",
        class: "markdown-alert-abstract",
        label_key: "markdown.alert.abstract",
        icon_body: tasty_icons::LIST.body,
        icon_filled: tasty_icons::LIST.filled,
        accent: Theme::accent_primary,
    },
    CalloutKind {
        gfm_kind: None,
        type_key: "info",
        class: "markdown-alert-info",
        label_key: "markdown.alert.info",
        icon_body: tasty_icons::ALERT_CIRCLE.body,
        icon_filled: tasty_icons::ALERT_CIRCLE.filled,
        accent: Theme::accent_info,
    },
    CalloutKind {
        gfm_kind: None,
        type_key: "todo",
        class: "markdown-alert-todo",
        label_key: "markdown.alert.todo",
        icon_body: tasty_icons::CHECK.body,
        icon_filled: tasty_icons::CHECK.filled,
        accent: Theme::accent_info,
    },
    CalloutKind {
        gfm_kind: None,
        type_key: "success",
        class: "markdown-alert-success",
        label_key: "markdown.alert.success",
        icon_body: tasty_icons::CHECK.body,
        icon_filled: tasty_icons::CHECK.filled,
        accent: Theme::accent_success,
    },
    CalloutKind {
        gfm_kind: None,
        type_key: "question",
        class: "markdown-alert-question",
        label_key: "markdown.alert.question",
        icon_body: tasty_icons::HELP_CIRCLE.body,
        icon_filled: tasty_icons::HELP_CIRCLE.filled,
        accent: Theme::accent_attention,
    },
    CalloutKind {
        gfm_kind: None,
        type_key: "failure",
        class: "markdown-alert-failure",
        label_key: "markdown.alert.failure",
        icon_body: tasty_icons::ALERT_TRIANGLE.body,
        icon_filled: tasty_icons::ALERT_TRIANGLE.filled,
        accent: Theme::accent_danger,
    },
    CalloutKind {
        gfm_kind: None,
        type_key: "danger",
        class: "markdown-alert-danger",
        label_key: "markdown.alert.danger",
        icon_body: tasty_icons::CLOSE.body,
        icon_filled: tasty_icons::CLOSE.filled,
        accent: Theme::accent_danger,
    },
    CalloutKind {
        gfm_kind: None,
        type_key: "bug",
        class: "markdown-alert-bug",
        label_key: "markdown.alert.bug",
        icon_body: tasty_icons::CLOSE.body,
        icon_filled: tasty_icons::CLOSE.filled,
        accent: Theme::accent_agent,
    },
    CalloutKind {
        gfm_kind: None,
        type_key: "example",
        class: "markdown-alert-example",
        label_key: "markdown.alert.example",
        icon_body: tasty_icons::SCRIPT.body,
        icon_filled: tasty_icons::SCRIPT.filled,
        accent: Theme::accent_agent,
    },
    CalloutKind {
        gfm_kind: None,
        type_key: "quote",
        class: "markdown-alert-quote",
        label_key: "markdown.alert.quote",
        icon_body: tasty_icons::TEXT_LEFT.body,
        icon_filled: tasty_icons::TEXT_LEFT.filled,
        accent: Theme::accent_primary,
    },
];

/// `(alias, canonical type_key)` pairs for Obsidian's documented type aliases, excluding the 3
/// that would collide with a GFM kind's own canonical name (`important`, `caution`, `attention`
/// — see [`CALLOUT_KINDS`] doc). [`find_callout_kind`] only consults this after an exact
/// [`CALLOUT_KINDS`] `type_key` match fails, so a collision here could never actually shadow a
/// GFM entry even if one were added by mistake — kept excluded anyway for clarity.
const CALLOUT_ALIASES: &[(&str, &str)] = &[
    ("summary", "abstract"),
    ("tldr", "abstract"),
    ("hint", "tip"),
    ("check", "success"),
    ("done", "success"),
    ("help", "question"),
    ("faq", "question"),
    ("fail", "failure"),
    ("missing", "failure"),
    ("error", "danger"),
    ("cite", "quote"),
];

/// Resolves a lowercase `[!type]` token (already lowercased by [`parse_callout_tag_line`]) to
/// its [`CalloutKind`], checking canonical [`CALLOUT_KINDS`] names first and [`CALLOUT_ALIASES`]
/// second. Types not present in either (e.g. some `[!made-up-type]`) return `None` — per scope,
/// this plugin only recognizes the Obsidian types the official docs confirm, nothing invented.
fn find_callout_kind(type_key: &str) -> Option<&'static CalloutKind> {
    if let Some(found) = CALLOUT_KINDS.iter().find(|k| k.type_key == type_key) {
        return Some(found);
    }
    let canonical = CALLOUT_ALIASES
        .iter()
        .find(|(alias, _)| *alias == type_key)?
        .1;
    CALLOUT_KINDS.iter().find(|k| k.type_key == canonical)
}

/// One parsed `[!type]([+-])?( title)?` tag line — the shape [`parse_callout_tag_line`] extracts
/// from a plain blockquote's first line of text.
struct ParsedCalloutTag {
    /// Lowercased `[!type]` token, looked up via [`find_callout_kind`].
    type_key: String,
    /// `Some(true)` = `+` (initially expanded), `Some(false)` = `-` (initially collapsed),
    /// `None` = no fold marker at all (no `<details>` — matches scope: "마커 없음 = 접기 UI
    /// 자체 없음").
    fold: Option<bool>,
    /// Text following the tag (and fold marker, if any) on the same line, trimmed — `None` if
    /// empty (fold and title are independent per Obsidian's own docs: a title can appear with or
    /// without a fold marker).
    title: Option<String>,
}

/// Hand-rolled (no `regex` dependency in this crate) parse of `^\[!(\w[\w-]*)\]([+-])?(.*)$`
/// against a blockquote's first line of raw text. Returns `None` for anything that doesn't match
/// that exact shape — including a bare `[!NOTE]` line, which never reaches this function at all
/// (pulldown-cmark's own GFM scanner already consumes those into `Tag::BlockQuote(Some(kind))`
/// before [`rewrite_callout_events`] ever sees plain-blockquote text; see its module doc).
fn parse_callout_tag_line(text: &str) -> Option<ParsedCalloutTag> {
    let rest = text.strip_prefix("[!")?;
    let close = rest.find(']')?;
    let type_token = &rest[..close];
    if type_token.is_empty()
        || !type_token
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return None;
    }
    let mut after = &rest[close + 1..];
    let fold = match after.chars().next() {
        Some('+') => {
            after = &after[1..];
            Some(true)
        }
        Some('-') => {
            after = &after[1..];
            Some(false)
        }
        _ => None,
    };
    let title = after.trim();
    Some(ParsedCalloutTag {
        type_key: type_token.to_ascii_lowercase(),
        fold,
        title: (!title.is_empty()).then(|| title.to_string()),
    })
}

/// Unified callout preprocessing: buffers every blockquote (`Start(Tag::BlockQuote(_))` through
/// its matching `End(TagEnd::BlockQuote)`, nest-counted so a callout containing another callout
/// buffers only its own span) and decides, in one place, whether it's a genuine GFM alert, an
/// Obsidian-style callout, or an ordinary quote — replacing the old GFM-only
/// `rewrite_alert_blockquote_event` entirely rather than running alongside it. Two structurally
/// different inputs both flow through here (see [`CalloutKind::gfm_kind`] doc):
///
/// - `Tag::BlockQuote(Some(kind))` — pulldown-cmark's parser already recognized a bare `[!TYPE]`
///   line (nothing else on it) as one of the 5 GFM kinds. Grammar-guaranteed: never has a fold
///   marker or custom title, so this always renders the fixed non-foldable shape — byte-
///   identical to the pre-Obsidian-support output, which is what keeps every existing GFM alert
///   test passing unchanged.
/// - `Tag::BlockQuote(None)` — anything else, including every Obsidian-flavored line (`[!info]`,
///   `[!note]+ Title`, `[!warning]- `, …): the GFM scanner's `scan_blank_line` requirement makes
///   it reject *any* trailing content after `]`, so these never reach the parser as a distinct
///   AST shape no matter which of the 5 names they use. [`rewrite_callout_buffer`] parses the
///   buffered blockquote's own first line of text instead.
///
/// This intentionally still keys off the real parser events for the GFM-recognized half (not a
/// post-pass over the assembled HTML string) for the same reason the old function did: raw HTML
/// blocks/inline (`Event::Html`/`Event::InlineHtml`) pass through byte-for-byte independent of
/// `Tag::BlockQuote`, so a naive string search over rendered output could be spoofed into
/// labeling attacker content as a trusted alert. Matching the AST event closes that gap; the
/// Obsidian-only half necessarily reads buffered *text* (there's no AST shape to key off), but
/// only ever from inside a genuine `Tag::BlockQuote(None)` span — raw HTML still never enters
/// that buffer, so it still can never manufacture a `data-label`/callout header this way either.
fn rewrite_callout_events<'a>(events: Vec<Event<'a>>, tr: &Translator) -> Vec<Event<'a>> {
    let mut out = Vec::with_capacity(events.len());
    let mut iter = events.into_iter();
    while let Some(event) = iter.next() {
        match event {
            Event::Start(Tag::BlockQuote(kind)) => {
                let mut buf = Vec::new();
                let mut nest = 0i32;
                for inner in iter.by_ref() {
                    match &inner {
                        Event::Start(Tag::BlockQuote(_)) => {
                            nest += 1;
                            buf.push(inner);
                        }
                        Event::End(TagEnd::BlockQuote(_)) => {
                            if nest == 0 {
                                break;
                            }
                            nest -= 1;
                            buf.push(inner);
                        }
                        _ => buf.push(inner),
                    }
                }
                out.extend(rewrite_callout_buffer(kind, buf, tr));
            }
            other => out.push(other),
        }
    }
    out
}

/// One buffered blockquote's interior events (stripped of its own `Start`/`End` by
/// [`rewrite_callout_events`]) → either a rendered callout or the original blockquote passed
/// through unchanged. See [`rewrite_callout_events`] doc for the two input shapes this handles.
fn rewrite_callout_buffer<'a>(
    gfm_kind: Option<BlockQuoteKind>,
    buf: Vec<Event<'a>>,
    tr: &Translator,
) -> Vec<Event<'a>> {
    // Nested callouts get their own detection too, recursively — bounded by actual source
    // nesting depth, same as any other recursive-descent pass over this event stream. Perfect
    // styling for deep nesting isn't required (scope), only that nothing crashes or gets lost —
    // recursing first and then treating the (already-rewritten) result as opaque body content
    // for the outer wrapper satisfies that with no special-casing.
    let buf = rewrite_callout_events(buf, tr);

    if let Some(kind) = gfm_kind {
        let Some(callout) = CALLOUT_KINDS.iter().find(|k| k.gfm_kind == Some(kind)) else {
            // All 5 GFM `BlockQuoteKind` variants are covered in `CALLOUT_KINDS`; unreachable in
            // practice, but fall back to a plain blockquote rather than panicking if
            // pulldown-cmark ever adds a 6th kind before this table does.
            return wrap_plain_blockquote(Some(kind), buf);
        };
        return wrap_static_callout(callout.class, tr.t(callout.label_key), buf);
    }

    // Plain blockquote — an Obsidian-style tag can only be recognized when the first paragraph's
    // opening run is plain, untouched `Text` (mirrors `figurize_solo_image_paragraphs`'s "give up
    // rather than guess" stance for anything more structurally complex, e.g. a tag line starting
    // with inline emphasis). pulldown-cmark's inline scanner does *not* coalesce a `[...]` run
    // into one `Text` event even when it isn't a link — `[!note]+ Title` tokenizes as
    // `Text("["), Text("!note"), Text("]"), Text("+ Title")` (verified against the real 0.12.2
    // event stream), so every leading `Text` run up to the first non-`Text` event (`SoftBreak` or
    // `End(Paragraph)`) has to be concatenated before the tag-line grammar can be matched at all.
    let Some(Event::Start(Tag::Paragraph)) = buf.first() else {
        return wrap_plain_blockquote(None, buf);
    };
    let mut first_line = String::new();
    let mut text_run_end = 1;
    while let Some(Event::Text(t)) = buf.get(text_run_end) {
        first_line.push_str(t);
        text_run_end += 1;
    }
    if text_run_end == 1 {
        return wrap_plain_blockquote(None, buf); // no leading text at all — nothing to parse.
    }
    let Some(parsed) = parse_callout_tag_line(&first_line) else {
        return wrap_plain_blockquote(None, buf);
    };
    let Some(callout) = find_callout_kind(&parsed.type_key) else {
        return wrap_plain_blockquote(None, buf);
    };

    let label = parsed
        .title
        .clone()
        .unwrap_or_else(|| tr.t(callout.label_key).to_string());

    // Every `Text` run making up the tag line is consumed (regex is `^...$`-anchored over their
    // concatenation) — what follows is either the rest of that same first paragraph (a
    // fold-marker-only line immediately continued on the next physical line, still one
    // CommonMark paragraph) or the paragraph's own close, in which case the tag line *was* the
    // entire first paragraph and that now-empty paragraph is dropped rather than emitted as
    // `<p></p>`.
    let mut rest: Vec<Event<'a>> = buf[text_run_end..].to_vec();
    let body: Vec<Event<'a>> = if matches!(rest.first(), Some(Event::End(TagEnd::Paragraph))) {
        rest.remove(0);
        rest
    } else {
        let mut body = vec![Event::Start(Tag::Paragraph)];
        body.append(&mut rest);
        body
    };

    match parsed.fold {
        None => wrap_static_callout(callout.class, &label, body),
        Some(open) => wrap_foldable_callout(callout.class, &label, open, body),
    }
}

/// Re-wraps buffered interior events back into an ordinary, untouched
/// `<blockquote>...</blockquote>` — used by every [`rewrite_callout_buffer`] path that declines
/// to treat the blockquote as a callout.
fn wrap_plain_blockquote(kind: Option<BlockQuoteKind>, buf: Vec<Event<'_>>) -> Vec<Event<'_>> {
    let mut out = Vec::with_capacity(buf.len() + 2);
    out.push(Event::Start(Tag::BlockQuote(kind)));
    out.extend(buf);
    out.push(Event::End(TagEnd::BlockQuote(kind)));
    out
}

/// The non-foldable callout shape — used both for a grammar-guaranteed bare GFM `[!TYPE]` tag
/// and for an Obsidian tag with a custom title but no fold marker (`[!type] Title`). Identical
/// `<blockquote class=".." data-label="..">` shape the pre-Obsidian-support GFM alert renderer
/// used, so [`sanitize_html`]'s existing `blockquote` allowlist needs no changes for this path.
/// CSS can't branch on UI language ([`theme_css`]'s `content: attr(data-label)` just echoes
/// whatever lands in the DOM), so the label — default *or* custom title — is resolved here, at
/// document generation time.
fn wrap_static_callout<'a>(class: &str, label: &str, body: Vec<Event<'a>>) -> Vec<Event<'a>> {
    let mut out = Vec::with_capacity(body.len() + 2);
    out.push(Event::Html(
        format!(
            r#"<blockquote class="{class}" data-label="{}">"#,
            attr_escape(label)
        )
        .into(),
    ));
    out.extend(body);
    out.push(Event::Html("</blockquote>".into()));
    out
}

/// The foldable callout shape (`+`/`-` marker present) — `<details>`/`<summary>` instead of
/// `<blockquote>`, per scope: "접기(+/-/마커없음): `<details>/<summary>` 매핑". `open` maps
/// `+` (initially expanded) and is omitted for `-` (initially collapsed) — native
/// `<details>`/`<summary>` behavior handles the actual expand/collapse toggle, no script needed.
/// The label rides in as real `Event::Text` inside `<summary>` (auto-escaped by `push_html`,
/// unlike the sibling `data-label` attribute trick [`wrap_static_callout`] uses, which needs
/// `<summary>` itself to carry no data — see [`theme_css`]'s `details[class^="markdown-alert-"]
/// >summary::before` rule for how the icon still renders without an `attr()`-readable label).
fn wrap_foldable_callout<'a>(
    class: &str,
    label: &str,
    open: bool,
    body: Vec<Event<'a>>,
) -> Vec<Event<'a>> {
    let mut out = Vec::with_capacity(body.len() + 4);
    out.push(Event::Html(
        format!(
            r#"<details class="{class}"{}>"#,
            if open { " open" } else { "" }
        )
        .into(),
    ));
    out.push(Event::Html("<summary>".into()));
    out.push(Event::Text(label.to_string().into()));
    out.push(Event::Html("</summary>".into()));
    out.extend(body);
    out.push(Event::Html("</details>".into()));
    out
}

// ── footnote backlinks + a11y (`[^name]` / `[^name]: ...`) ─────────────────────

/// Per-render mutable state for [`rewrite_footnote_event`], threaded through the whole event
/// stream via a single `.map()` closure capture (mirrors how [`pulldown_cmark::html`]'s own
/// writer keeps a `numbers: HashMap<CowStr, usize>` internally — we need our own copy since we
/// intercept these events *before* that writer ever sees them).
#[derive(Default)]
struct FootnoteState {
    /// `name -> display number`, assigned the first time a name is seen (whichever comes
    /// first in event order — a reference or the definition itself; footnote definitions can
    /// legally appear *before* their first reference, so neither event kind can assume it's
    /// first). Mirrors pulldown-cmark's own `self.numbers.len() + 1` / `or_insert` numbering
    /// exactly, so the visible `[1]`/`[2]` markers match what the un-rewritten library would
    /// have shown.
    numbers: std::collections::HashMap<String, usize>,
    /// `name -> how many `FootnoteReference` events for this name have been rewritten so far`
    /// — drives the `fnref-<name>`/`fnref-<name>-2`/... suffix so multiple references to the
    /// same footnote get distinct, individually-targetable ids.
    seen_ref_counts: std::collections::HashMap<String, usize>,
    /// The name of the `FootnoteDefinition` currently open, if any (`TagEnd::FootnoteDefinition`
    /// carries no name of its own — checked against pulldown-cmark 0.12's `html.rs` — so the
    /// name has to be stashed here on `Start` and consumed on `End`). Definitions never nest, so
    /// a single slot is enough.
    open_definition: Option<String>,
}

fn footnote_number(state: &mut FootnoteState, name: &str) -> usize {
    let next = state.numbers.len() + 1;
    *state.numbers.entry(name.to_string()).or_insert(next)
}

/// Total reference count per footnote name, computed via a full separate parse pass over `source`
/// *before* the real rewriting pass runs. Needed because a `FootnoteDefinition`'s closing tag has
/// to know how many backlinks to emit, but (as [`FootnoteState::open_definition`] documents) a
/// definition can appear *before* some of its references in the event stream — by the time the
/// single forward-only rewriting pass reaches `TagEnd::FootnoteDefinition`, later references
/// simply haven't happened yet. Re-parsing is cheap relative to this plugin's existing
/// whole-document-re-render-per-keystroke/theme-change architecture (module doc), so a second
/// pass here is not a new order-of-magnitude cost.
fn footnote_reference_totals(source: &str) -> std::collections::HashMap<String, usize> {
    let mut totals = std::collections::HashMap::new();
    for event in Parser::new_ext(source, parser_options()) {
        if let Event::FootnoteReference(name) = event {
            *totals.entry(name.to_string()).or_insert(0) += 1;
        }
    }
    totals
}

/// Rewrites `Event::FootnoteReference` and the `Tag::FootnoteDefinition` start/end pair, same
/// "intercept the real AST event, emit finished markup via `Event::Html`" shape as
/// [`rewrite_callout_events`] — chosen for the same reason: matching against fully
/// rendered HTML text can't distinguish a genuine footnote from a raw-HTML block that merely
/// looks like one, but matching the AST event can (pulldown-cmark passes raw HTML through as
/// `Event::Html`/`Event::InlineHtml`, never `FootnoteReference`/`Tag::FootnoteDefinition`).
///
/// pulldown-cmark's own default markup (`html.rs`) is a starting point but has two gaps this
/// closes: no id on the reference itself (so multiple references to one footnote all point at
/// the same `href`, and a definition has no way to link back to *which* reference), and no
/// backlink or `aria-label` at all. An **undefined** reference (`[^missing]` with no matching
/// `[^missing]: ...`) never reaches this function as a `FootnoteReference` event in the first
/// place — pulldown-cmark's parser only recognizes the construct when a matching definition
/// exists; otherwise it falls back to plain `[`/`^missing`/`]` text (verified by dumping the
/// event stream for an unmatched reference), so no special-case handling is needed here for that
/// case — it degrades to literal text with zero risk of a panic.
fn rewrite_footnote_event<'a>(
    event: Event<'a>,
    tr: &Translator,
    ref_totals: &std::collections::HashMap<String, usize>,
    state: &mut FootnoteState,
) -> Event<'a> {
    match event {
        Event::FootnoteReference(name) => {
            let name = name.to_string();
            let number = footnote_number(state, &name);
            let occurrence = {
                let count = state.seen_ref_counts.entry(name.clone()).or_insert(0);
                *count += 1;
                *count
            };
            let safe = percent_encode_fragment(&name);
            let ref_id = if occurrence == 1 {
                format!("fnref-{safe}")
            } else {
                format!("fnref-{safe}-{occurrence}")
            };
            let aria = attr_escape(&tr.t_fmt("markdown.footnote.ref_aria", &number.to_string()));
            Event::Html(
                format!(
                    r##"<sup class="footnote-reference" id="{ref_id}"><a href="#fndef-{safe}" aria-label="{aria}">{number}</a></sup>"##
                )
                .into(),
            )
        }
        Event::Start(Tag::FootnoteDefinition(name)) => {
            let name = name.to_string();
            let number = footnote_number(state, &name);
            let safe = percent_encode_fragment(&name);
            state.open_definition = Some(name);
            Event::Html(
                format!(
                    r#"<div class="footnote-definition" id="fndef-{safe}"><sup class="footnote-definition-label">{number}</sup>"#
                )
                .into(),
            )
        }
        Event::End(TagEnd::FootnoteDefinition) => {
            let name = state
                .open_definition
                .take()
                .unwrap_or_else(|| String::from("unknown"));
            let number = footnote_number(state, &name);
            let safe = percent_encode_fragment(&name);
            let total = *ref_totals.get(&name).unwrap_or(&0);
            let mut backlinks = String::new();
            for occurrence in 1..=total {
                let ref_id = if occurrence == 1 {
                    format!("fnref-{safe}")
                } else {
                    format!("fnref-{safe}-{occurrence}")
                };
                let aria = if total > 1 {
                    tr.t("markdown.footnote.backlink_aria_nth")
                        .replace("{0}", &number.to_string())
                        .replace("{1}", &occurrence.to_string())
                } else {
                    tr.t_fmt("markdown.footnote.backlink_aria", &number.to_string())
                };
                backlinks.push_str(&format!(
                    r##"<a href="#{ref_id}" aria-label="{}">↩</a>"##,
                    attr_escape(&aria)
                ));
            }
            Event::Html(format!("{backlinks}</div>\n").into())
        }
        other => other,
    }
}

/// Minimal GFM-sized sanitize allowlist. Strips `<script>`, every inline event handler
/// (`onerror=`, `onclick=`, …), and any `javascript:`-scheme attribute value — the sanitizer
/// itself is the primary XSS defense (independent of the `classify_link` guard, which only
/// covers destinations this plugin later dispatches).
///
/// `class` is allowed only on the tags pulldown-cmark actually emits it on: `code`
/// (fenced-block language, already normalized to a plain identifier by
/// [`rewrite_code_block_event`] — [`mermaid_script`] depends on that exact `language-<lang>`
/// shape surviving sanitize), `sup`/`div` (fixed literal footnote classes the library itself
/// writes — `footnote-reference`/`footnote-definition`/`footnote-definition-label`), and
/// `blockquote`/`details` (one of [`CALLOUT_KINDS`]' fixed literal `markdown-alert-<type>`
/// classes — `details` additionally allows `open`, the fold-state attribute
/// [`wrap_foldable_callout`] sets). ammonia does not validate `class` *values* — a raw HTML
/// block/inline in the source (passed through byte-for-byte by pulldown-cmark, independent of
/// `Tag::BlockQuote`) can already carry any of these class strings verbatim, so a document author
/// *can* make an arbitrary blockquote/details pick up the callout CSS's background/border/icon
/// purely via `class`, same residual risk the `code`/`sup`/`div` allowances already accept — none
/// of these carry executable content, so that's fine here. What the sanitizer's `class` allowlist
/// does *not* by itself make possible is a forged `data-label` matching one of the real
/// translated callout headers: that attribute is only ever set by [`rewrite_callout_events`] from
/// a genuine `Tag::BlockQuote` event/buffered blockquote text, never by matching rendered HTML
/// text, so raw-HTML blockquotes always reach this allowlist with `data-label` absent (see that
/// function's doc for why the distinction matters). A foldable callout's label rides in as plain
/// `<summary>` text instead of a `data-label` attribute (no sanitizer change needed for that —
/// `summary` carries no attributes at all). `data-label`'s value is `attr_escape`d before
/// injection, same escaping every other attribute value in this module gets.
fn sanitize_html(unsafe_html: &str) -> String {
    use ammonia::Builder;
    use std::collections::{HashMap, HashSet};

    let tags: HashSet<&str> = [
        "p",
        "br",
        "hr",
        "h1",
        "h2",
        "h3",
        "h4",
        "h5",
        "h6",
        "strong",
        "em",
        "del",
        "s",
        "code",
        "pre",
        "blockquote",
        "ul",
        "ol",
        "li",
        "table",
        "thead",
        "tbody",
        "tr",
        "th",
        "td",
        "a",
        "img",
        "input",
        "sup",
        "sub",
        "dl",
        "dt",
        "dd",
        "div",
        // figurize_solo_image_paragraphs()의 <figure>/<figcaption> 래핑 — 둘 다 별도
        // attribute 없이 bare tag 로만 쓴다(tag_attributes 등록 불필요).
        "figure",
        "figcaption",
        // wrap_foldable_callout() 의 접기 콜아웃(+/- 마커) — details 는 class+open, summary 는
        // 별도 attribute 없이 label 텍스트만 담는다(tag_attributes 등록 불필요).
        "details",
        "summary",
        // pulldown-cmark 의 ENABLE_MATH 가 InlineMath/DisplayMath 이벤트를 기본으로
        // `<span class="math math-inline|math-display">{escaped latex}</span>` 로 쓴다(자체
        // Rust 쪽 이벤트 rewrite 불필요 — 라이브러리 기본 동작 그대로 통과시킨다). 아래
        // `span`+`class` 화이트리스트 근거는 `tag_attributes` 주석 참조.
        "span",
    ]
    .into_iter()
    .collect();

    let mut tag_attributes: HashMap<&str, HashSet<&str>> = HashMap::new();
    // `aria-label` scoped to `a` only (not `generic_attributes`, which every tag would then
    // get) — the two consumers are [`rewrite_footnote_event`]'s reference/backlink anchors,
    // both `attr_escape`d before injection.
    tag_attributes.insert(
        "a",
        ["href", "title", "id", "aria-label"].into_iter().collect(),
    );
    tag_attributes.insert("img", ["src", "alt", "title"].into_iter().collect());
    tag_attributes.insert(
        "input",
        ["type", "checked", "disabled"].into_iter().collect(),
    );
    tag_attributes.insert("th", ["align"].into_iter().collect());
    tag_attributes.insert("td", ["align"].into_iter().collect());
    // 펜스드 코드블록 언어(`<code class="language-rust">`) — 값은 이미 event 단계에서
    // `[A-Za-z0-9_+-]` 로 정규화됨(sanitize_fence_lang).
    tag_attributes.insert("code", ["class"].into_iter().collect());
    // footnote 마크업의 고정 리터럴 class(라이브러리 자체가 씀, 사용자 입력 아님).
    tag_attributes.insert("sup", ["class"].into_iter().collect());
    tag_attributes.insert("div", ["class"].into_iter().collect());
    // callout blockquote — 고정 literal class(markdown-alert-<type>) + rewrite_callout_events 가
    // 진짜 AST 이벤트/파싱된 태그 라인에서만 심는 localized data-label(attr_escape 済み).
    tag_attributes.insert("blockquote", ["class", "data-label"].into_iter().collect());
    // 접기 콜아웃(details) — 고정 literal class + fold 상태(open, 마커 유무로만 결정되는
    // 불리언, 사용자 입력 그대로 반영되지 않음).
    tag_attributes.insert("details", ["class", "open"].into_iter().collect());
    // math span(`math math-inline`/`math math-display`) — ammonia 는 class *값* 단위
    // 화이트리스트를 지원하지 않는다(태그·속성 단위만) — `div`+`class` 가 이미 이 crate 에서
    // 같은 방식으로 열려 있다(`.tasty-state`/`.tasty-state-error` 등, 위 참조). class 값
    // 자체는 스크립트 실행도 URL 도 아니라 사용자가 raw HTML 로 `<span
    // class="math math-inline">직접 쓴 LaTeX</span>` 를 흉내내도 결과는 "자기 콘텐츠가
    // KaTeX 로 렌더된다" 뿐 — 별도의 신뢰 HTML 사후조립(sanitizer 우회) 경로 없이 이 방식을
    // 택했다(스코프상으로도 이 방식이 pulldown-cmark 기본 동작을 그대로 쓰므로 커스텀 이벤트
    // rewrite 코드가 전혀 필요 없어 더 작다).
    tag_attributes.insert("span", ["class"].into_iter().collect());

    Builder::default()
        .tags(tags)
        .tag_attributes(tag_attributes)
        .generic_attributes(["id"].into_iter().collect())
        // 내부 nav fragment(`#tasty-nav:...`)는 scheme 이 없어 이 allowlist 와 무관하게
        // 항상 통과한다 — 여기서 막는 건 `javascript:`/기타 스킴을 가진 `href`/`src` 뿐.
        .url_schemes(["http", "https", "mailto"].into_iter().collect())
        .clean(unsafe_html)
        .to_string()
}

// ── CSS (theme → custom properties) ───────────────────────────────────────────

/// Map the host `Theme` tokens onto CSS custom properties + the base stylesheet. The CSS-side
/// equivalent of the former `apply_theme` (`egui::Visuals` mapping) — see module doc for why
/// this rewrite finally allows a real per-level heading ladder and a tuned body line-height,
/// both library limitations of the retired `egui_commonmark` renderer.
fn theme_css(theme: &Theme) -> String {
    let [h1, h2, h3, h4, h5, h6] = heading_sizes_px(theme);
    let body = theme.font_size_body.value();
    format!(
        r#":root{{
--md-fg:{fg};
--md-strong:{strong};
--md-link:{link};
--md-code-bg:{code_bg};
--md-code-border:{code_border};
--md-border:{border};
--md-quote-bar:{quote_bar};
--md-rule:{rule};
--md-zebra:{zebra};
--md-bg:{bg};
--md-radius:{radius}px;
--md-border-w:{border_w}px;
--md-space-xs:{space_xs}px;
--md-space-sm:{space_sm}px;
--md-space-md:{space_md}px;
--md-font-body:{body}px;
--md-h1:{h1}px;--md-h2:{h2}px;--md-h3:{h3}px;--md-h4:{h4}px;--md-h5:{h5}px;--md-h6:{h6}px;
}}
html,body{{height:100%;margin:0;padding:0;}}
body{{background:var(--md-bg);color:var(--md-fg);font-family:-apple-system,BlinkMacSystemFont,"Segoe UI",Helvetica,Arial,sans-serif;font-size:var(--md-font-body);line-height:1.6;}}
#tasty-addr-bar{{position:sticky;top:0;display:flex;align-items:center;gap:var(--md-space-sm);height:40px;padding:0 var(--md-space-sm);box-sizing:border-box;background:{bg_sidebar};border-bottom:var(--md-border-w) solid {separator};}}
#tasty-addr-input{{flex:1;height:24px;border:var(--md-border-w) solid var(--md-border);border-radius:var(--md-radius);padding:0 var(--md-space-xs);background:var(--md-bg);color:var(--md-fg);font-size:var(--md-font-body);}}
#tasty-addr-go{{height:24px;padding:0 var(--md-space-sm);border:var(--md-border-w) solid var(--md-border);border-radius:var(--md-radius);background:var(--md-code-bg);color:var(--md-fg);cursor:pointer;}}
#tasty-find-bar{{position:fixed;top:calc(40px + var(--md-space-xs));right:var(--md-space-sm);z-index:20;display:flex;align-items:center;gap:var(--md-space-xs);height:28px;padding:0 var(--md-space-xs);background:{bg_sidebar};border:var(--md-border-w) solid {separator};border-radius:var(--md-radius);box-shadow:0 2px 8px rgba(0,0,0,0.25);}}
#tasty-find-bar[hidden]{{display:none;}}
#tasty-find-input{{width:140px;height:22px;border:var(--md-border-w) solid var(--md-border);border-radius:var(--md-radius);padding:0 var(--md-space-xs);background:var(--md-bg);color:var(--md-fg);font-size:var(--md-font-body);}}
#tasty-find-count{{min-width:40px;text-align:center;font-size:calc(var(--md-font-body) * 0.85);color:{muted};}}
#tasty-find-count.tasty-find-nomatch{{color:{danger};}}
.tasty-find-btn{{height:22px;width:22px;flex-shrink:0;display:inline-flex;align-items:center;justify-content:center;border:var(--md-border-w) solid var(--md-border);border-radius:var(--md-radius);background:var(--md-code-bg);color:var(--md-fg);font-size:10px;line-height:1;padding:0;cursor:pointer;}}
.tasty-find-btn:disabled{{opacity:0.45;cursor:default;}}
mark.tasty-find-hit{{background:{find_match_bg};color:inherit;border-radius:2px;}}
mark.tasty-find-hit.tasty-find-current{{background:{find_current_bg};color:{find_current_fg};}}
#tasty-md-body{{padding:var(--md-space-sm) var(--md-space-md);}}
h1,h2,h3,h4,h5,h6{{color:var(--md-strong);font-weight:600;margin:1em 0 0.5em;scroll-margin-top:calc(40px + var(--md-space-sm));}}
h1{{font-size:var(--md-h1);}}h2{{font-size:var(--md-h2);}}h3{{font-size:var(--md-h3);}}
h4{{font-size:var(--md-h4);}}h5{{font-size:var(--md-h5);}}h6{{font-size:var(--md-h6);}}
#tasty-toc{{margin:var(--md-space-sm) var(--md-space-md) 0;padding:var(--md-space-sm) var(--md-space-md);border:var(--md-border-w) solid var(--md-border);border-radius:var(--md-radius);background:var(--md-code-bg);}}
#tasty-toc-toggle{{all:unset;cursor:pointer;display:inline-flex;align-items:center;gap:var(--md-space-xs);font-weight:600;font-size:var(--md-font-body);color:var(--md-strong);}}
#tasty-toc-toggle::before{{content:"\25be";display:inline-block;}}
#tasty-toc.tasty-toc-collapsed #tasty-toc-toggle::before{{content:"\25b8";}}
#tasty-toc-list{{list-style:none;margin:var(--md-space-xs) 0 0;padding:0;max-height:280px;overflow-y:auto;}}
#tasty-toc.tasty-toc-collapsed #tasty-toc-list{{display:none;}}
#tasty-toc-list a{{display:block;padding:var(--md-space-xs) 0;color:var(--md-link);text-decoration:none;font-size:var(--md-font-body);}}
#tasty-toc-list a:hover{{text-decoration:underline;}}
.tasty-toc-l1 a{{padding-left:0;}}
.tasty-toc-l2 a{{padding-left:var(--md-space-sm);}}
.tasty-toc-l3 a{{padding-left:calc(var(--md-space-sm) * 2);}}
.tasty-toc-l4 a{{padding-left:calc(var(--md-space-sm) * 3);}}
.tasty-toc-l5 a{{padding-left:calc(var(--md-space-sm) * 4);}}
.tasty-toc-l6 a{{padding-left:calc(var(--md-space-sm) * 5);}}
a{{color:var(--md-link);}}
.tasty-wikilink-missing a{{color:{danger};text-decoration:underline dotted;}}
strong{{color:var(--md-strong);font-weight:600;}}
code{{background:var(--md-code-bg);border-radius:var(--md-radius);padding:0.1em 0.35em;font-family:ui-monospace,SFMono-Regular,Menlo,Consolas,monospace;}}
pre{{position:relative;background:var(--md-code-bg);border:var(--md-border-w) solid var(--md-code-border);border-radius:var(--md-radius);padding:var(--md-space-sm);overflow:auto;}}
pre code{{background:none;padding:0;}}
.tasty-copy-btn{{position:absolute;top:var(--md-space-xs);right:var(--md-space-xs);height:22px;padding:0 var(--md-space-xs);border:var(--md-border-w) solid var(--md-border);border-radius:var(--md-radius);background:var(--md-bg);color:var(--md-fg);font-size:calc(var(--md-font-body) * 0.8);line-height:1;cursor:pointer;opacity:0;transition:opacity 0.15s ease;}}
pre:hover .tasty-copy-btn,.tasty-copy-btn:focus-visible{{opacity:1;}}
@media (hover:none){{.tasty-copy-btn{{opacity:1;}}}}
.tasty-copy-btn[data-state="copied"]{{border-color:{success};color:{success};}}
.tasty-copy-btn[data-state="failed"]{{border-color:{danger};color:{danger};}}
table{{border-collapse:collapse;}}
th,td{{border:var(--md-border-w) solid var(--md-border);padding:var(--md-space-xs) var(--md-space-sm);text-align:left;}}
tr:nth-child(even){{background:var(--md-zebra);}}
blockquote{{border-left:calc(var(--md-border-w) * 3) solid var(--md-quote-bar);margin:0.5em 0;padding:0.1em var(--md-space-md);opacity:0.9;}}
blockquote[class^="markdown-alert-"]{{opacity:1;border-radius:var(--md-radius);padding:var(--md-space-sm) var(--md-space-md);}}
blockquote[class^="markdown-alert-"]::before{{content:attr(data-label);display:block;font-weight:600;margin-bottom:var(--md-space-xs);padding-left:22px;background-repeat:no-repeat;background-position:left center;background-size:16px 16px;}}
details[class^="markdown-alert-"]{{border-radius:var(--md-radius);padding:var(--md-space-sm) var(--md-space-md);border-left:calc(var(--md-border-w) * 3) solid;}}
details[class^="markdown-alert-"]>summary{{cursor:pointer;font-weight:600;}}
details[class^="markdown-alert-"]>summary::before{{content:"";display:inline-block;width:16px;height:16px;margin-right:6px;vertical-align:middle;background-repeat:no-repeat;background-position:center;background-size:16px 16px;}}
details[class^="markdown-alert-"][open]>summary{{margin-bottom:var(--md-space-xs);}}
{alert_rules}
{hljs_rules}
hr{{border:none;border-top:var(--md-border-w) solid var(--md-rule);margin:var(--md-space-md) 0;}}
img{{max-width:100%;}}
.tasty-img-error{{display:inline-flex;align-items:center;gap:var(--md-space-xs);flex-wrap:wrap;max-width:100%;box-sizing:border-box;padding:var(--md-space-xs) var(--md-space-sm);border:var(--md-border-w) dashed {danger};border-radius:var(--md-radius);background:var(--md-code-bg);}}
.tasty-img-error-icon{{flex:0 0 auto;width:16px;height:16px;background-image:url("{img_error_icon}");background-repeat:no-repeat;background-size:16px 16px;}}
.tasty-img-error-label{{color:{danger};font-weight:600;}}
.tasty-img-error-path{{color:{muted};word-break:break-all;font-family:ui-monospace,SFMono-Regular,Menlo,Consolas,monospace;}}
figure{{margin:var(--md-space-sm) 0;text-align:center;}}
figcaption{{margin-top:var(--md-space-xs);font-size:var(--md-font-body);color:{muted};}}
ul,ol{{padding-left:1.5em;}}
li input[type=checkbox]{{margin-right:0.4em;}}
.tasty-state{{padding:var(--md-space-md);color:{muted};}}
.tasty-state-title{{font-size:var(--md-font-body);font-weight:600;color:{danger};}}
.tasty-state-detail{{color:{muted};white-space:pre-wrap;}}
"#,
        fg = theme.text_secondary().to_hex(),
        strong = theme.text_primary().to_hex(),
        link = theme.accent_primary().to_hex(),
        code_bg = theme.surface_raised().to_hex(),
        code_border = theme.separator.to_hex(),
        border = theme.md_table_border().to_hex(),
        quote_bar = theme.border_strong().to_hex(),
        rule = theme.separator.to_hex(),
        zebra = theme.md_table_row_bg_zebra().to_hex(),
        // webview 렌더 경로엔 focus 신호가 없다 — surfaces.markdown.focused_bg 대신
        // bg_app(=crust)을 문서의 유일한 배경으로 쓴다.
        bg = theme.bg_app().to_hex(),
        radius = theme.corner_radius.value(),
        border_w = theme.border_width.value(),
        space_xs = theme.spacing_xs.value(),
        space_sm = theme.spacing_sm.value(),
        space_md = theme.spacing_md.value(),
        body = body,
        h1 = h1,
        h2 = h2,
        h3 = h3,
        h4 = h4,
        h5 = h5,
        h6 = h6,
        bg_sidebar = theme.bg_sidebar().to_hex(),
        separator = theme.separator.to_hex(),
        muted = theme.text_muted().to_hex(),
        danger = theme.accent_danger().to_hex(),
        success = theme.accent_success().to_hex(),
        img_error_icon = alert_icon_data_uri(
            tasty_icons::IMAGE.body,
            false,
            &theme.accent_danger().to_hex()
        ),
        alert_rules = alert_css(theme),
        hljs_rules = hljs_css(theme),
        find_match_bg = theme
            .accent_warning()
            .with_alpha(FIND_HIT_BG_ALPHA)
            .to_hex(),
        find_current_bg = theme.accent_primary().to_hex(),
        find_current_fg = theme.text_on_accent().to_hex(),
    )
}

/// Per-[`CalloutKind`] CSS: border/background from its accent color, plus the icon half of the
/// shared header rule — for the `<blockquote>` shape, `blockquote[class^="markdown-alert-"]
/// ::before` above (the label-text half — `content: attr(data-label)` — is already covered there
/// since it doesn't vary per kind); for the `<details>`/`<summary>` foldable shape, the
/// `.{class}>summary::before` selector below layers the same icon onto
/// `details[class^="markdown-alert-"]>summary::before`'s shared sizing rule (that shape's label
/// is real `<summary>` text, not `attr()`-read, so only the icon needs a per-kind rule there — see
/// [`wrap_foldable_callout`]). No dedicated "callout" design token set exists yet, so background
/// is derived the same way `drop_overlay.rs`/`Theme::preset_split_zone_bg` already do: the same
/// accent color at low alpha, not a separate token.
fn alert_css(theme: &Theme) -> String {
    /// ~12% opacity — same ratio `drop_overlay.rs` uses for `accent_primary().with_alpha(31)`.
    const BG_ALPHA: u8 = 31;
    let mut rules = String::new();
    for kind in CALLOUT_KINDS {
        let color = (kind.accent)(theme);
        let icon_uri = alert_icon_data_uri(kind.icon_body, kind.icon_filled, &color.to_hex());
        rules.push_str(&format!(
            ".{class}{{border-left-color:{hex};background:{bg};}}.{class}::before,.{class}>summary::before{{color:{hex};background-image:url(\"{icon_uri}\");}}\n",
            class = kind.class,
            hex = color.to_hex(),
            bg = color.with_alpha(BG_ALPHA).to_hex(),
        ));
    }
    rules
}

/// Bakes [`CalloutKind::icon_body`] into a complete, `color_hex`-colored `<svg>` and encodes it as
/// a `data:image/svg+xml,` URI ready for a CSS `background-image`. The `tasty_icons` source is
/// fixed to `stroke="white"`/`fill="white"` (crate doc: "색을 글리프에 박지 않는다" — consumers
/// tint it themselves); the `egui` consumer does that post-hoc on the GPU texture
/// (`Icon::image`'s `tint`), but a CSS background image has no equivalent hook, so this bakes a
/// separately-colored copy of the markup directly instead.
fn alert_icon_data_uri(icon_body: &str, filled: bool, color_hex: &str) -> String {
    let (fill, stroke) = if filled {
        (color_hex, color_hex)
    } else {
        ("none", color_hex)
    };
    let svg = format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="{fill}" stroke="{stroke}" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">{icon_body}</svg>"#,
    );
    format!("data:image/svg+xml,{}", percent_encode_fragment(&svg))
}

/// `highlight.js`'s emitted `hljs-*` token classes, colored from this plugin's own Catppuccin-
/// style hue fields (`Theme::mauve`/`blue`/`green`/... — the same named-hue vocabulary every other
/// accent in this codebase derives from) instead of a vendored highlight.js theme (`github.css`
/// etc). That keeps highlighted code following whichever theme (mocha/latte/a user's custom
/// theme) is active, the same way every other rule in [`theme_css`] does, rather than always
/// rendering GitHub's fixed palette regardless of the user's actual theme choice. The class
/// grouping (which scopes share a color) mirrors highlight.js's own shipped themes — only the
/// colors themselves are swapped for `Theme` tokens, the scope-to-class grouping is highlight.js's
/// standard convention, not invented here. Always emitted (like [`alert_css`]'s rules) — inert on
/// documents with no code blocks since nothing has an `hljs-*` class to match.
fn hljs_css(theme: &Theme) -> String {
    format!(
        r#"code.hljs{{background:none;}}
.hljs-comment,.hljs-quote{{color:{comment};font-style:italic;}}
.hljs-meta{{color:{comment};}}
.hljs-keyword,.hljs-selector-tag,.hljs-subst,.hljs-operator{{color:{keyword};}}
.hljs-number,.hljs-literal{{color:{number};}}
.hljs-string,.hljs-doctag,.hljs-regexp,.hljs-link{{color:{string};}}
.hljs-title,.hljs-section,.hljs-selector-id,.hljs-title.function_{{color:{title};}}
.hljs-type,.hljs-class .hljs-title{{color:{type_};}}
.hljs-tag,.hljs-name,.hljs-attribute,.hljs-attr{{color:{tag};}}
.hljs-variable,.hljs-template-variable,.hljs-symbol,.hljs-bullet{{color:{variable};}}
.hljs-built_in,.hljs-builtin-name{{color:{builtin};}}
.hljs-deletion{{background:{deletion_bg};}}
.hljs-addition{{background:{addition_bg};}}
.hljs-emphasis{{font-style:italic;}}
.hljs-strong{{font-weight:700;}}
"#,
        comment = theme.text_muted().to_hex(),
        keyword = theme.mauve.to_hex(),
        number = theme.peach.to_hex(),
        string = theme.green.to_hex(),
        title = theme.blue.to_hex(),
        type_ = theme.yellow.to_hex(),
        tag = theme.teal.to_hex(),
        variable = theme.lavender.to_hex(),
        builtin = theme.red.to_hex(),
        deletion_bg = theme
            .accent_danger()
            .with_alpha(DIFF_LINE_BG_ALPHA)
            .to_hex(),
        addition_bg = theme
            .accent_success()
            .with_alpha(DIFF_LINE_BG_ALPHA)
            .to_hex(),
    )
}

/// Per-level heading pixel sizes, linearly interpolated between `font_size_prose_h1` (h1) and
/// `font_size_body` (h6) — real CSS, unlike the retired `egui_commonmark` renderer, can set
/// each level independently (module doc "library exceptions" this rewrite resolves).
fn heading_sizes_px(theme: &Theme) -> [f32; 6] {
    let h1 = theme.font_size_prose_h1.value();
    let body = theme.font_size_body.value();
    let step = (h1 - body) / 5.0;
    [
        h1,
        h1 - step,
        h1 - 2.0 * step,
        h1 - 3.0 * step,
        h1 - 4.0 * step,
        body,
    ]
}

// ── address bar (HTML/CSS/minimal trusted JS — design decision: Option 1) ────

/// The address bar markup: a path input (native `<datalist>` for recent-path autocomplete —
/// no custom dropdown JS needed) + a Go button. Baked with the *current* path/recent list at
/// document-generation time (there's no live JS↔plugin message channel for a webview surface —
/// see module doc — so unlike the old `PathField` there's no reactive fetch-on-focus).
fn addr_bar_html(tr: &Translator, file_path: &str, recent: &[String]) -> String {
    let options: String = recent
        .iter()
        .map(|p| format!(r#"<option value="{}"></option>"#, attr_escape(p)))
        .collect();
    format!(
        r#"<div id="tasty-addr-bar"><input id="tasty-addr-input" list="tasty-addr-recent" value="{value}" placeholder="{placeholder}"><datalist id="tasty-addr-recent">{options}</datalist><button id="tasty-addr-go" title="{go_tooltip}">&#8594;</button></div>"#,
        value = attr_escape(file_path),
        placeholder = attr_escape(tr.t("markdown.addr.placeholder")),
        options = options,
        go_tooltip = attr_escape(tr.t("markdown.addr.go")),
    )
}

/// Trusted, plugin-authored script (never sanitized — it never touches user markdown content).
/// Two responsibilities:
/// 1. Builds the `#tasty-nav:addr:<enc>` fragment from the address bar's current input value on
///    Enter/Go-click; module doc explains why a fragment assignment (rather than a real
///    navigation) is the only safe way to signal the host.
/// 2. Best-effort scroll-position preservation across `webview.set_url` reloads (idle-watch
///    auto-reload and `markdown.reload` both replace the whole document via `load_html` — there
///    is no in-place DOM patch, so the native WebView's own scroll position is always reset to
///    0 on reload without this). Keyed by `file_path` (baked in at generation time) so switching
///    files via the address bar doesn't restore the wrong document's position.
///    `sessionStorage` is same-origin scoped; whether a webview engine's `load_html` preserves
///    origin identity (and therefore `sessionStorage`) across reloads of the *same* surface is
///    platform-dependent and not verified across all three backends (WebKitGTK/WKWebView/
///    WebView2) — this degrades gracefully to "no restore" if it doesn't persist, never to an
///    error.
fn nav_script(file_path: &str) -> String {
    format!(
        r#"(function(){{
var i=document.getElementById('tasty-addr-input');
var g=document.getElementById('tasty-addr-go');
function go(){{
var v=i.value.trim();
if(!v)return;
location.hash='tasty-nav:addr:'+encodeURIComponent(v);
}}
if(g)g.addEventListener('click',go);
if(i)i.addEventListener('keydown',function(e){{if(e.key==='Enter')go();}});
var toc=document.getElementById('tasty-toc');
var tocToggle=document.getElementById('tasty-toc-toggle');
if(toc&&tocToggle){{
tocToggle.addEventListener('click',function(){{
var collapsed=toc.classList.toggle('tasty-toc-collapsed');
tocToggle.setAttribute('aria-expanded',collapsed?'false':'true');
}});
}}
var scrollKey='tasty-md-scroll:'+{file_path_json};
try{{
var saved=sessionStorage.getItem(scrollKey);
if(saved)window.scrollTo(0,parseInt(saved,10)||0);
}}catch(e){{}}
var saveTimer=null;
window.addEventListener('scroll',function(){{
if(saveTimer)clearTimeout(saveTimer);
saveTimer=setTimeout(function(){{
try{{sessionStorage.setItem(scrollKey,String(window.scrollY));}}catch(e){{}}
}},150);
}});
}})();"#,
        file_path_json = serde_json::to_string(file_path).unwrap_or_else(|_| "\"\"".to_string()),
    )
}

// ── find-in-page (trusted JS, TreeWalker-based text-node highlight) ─────────────

/// The find bar markup: query input + match counter + prev/next + close, `hidden` by default
/// (toggled by [`find_in_page_script`]). Floats top-right of the document (`position:fixed` in
/// [`theme_css`]) rather than sitting in normal flow like [`addr_bar_html`], mirroring the
/// gallery's `search_bar` specimen (`crates/tasty-gallery/src/catalog/components/search_bar.rs`)
/// — a sticky, non-modal find bar anchored to the top-right of the focused surface.
///
/// No case/regex/whole-word toggles (unlike the gallery specimen / terminal `search_bar.rs`) —
/// out of scope here: this is a literal substring find-on-page, not the terminal's full
/// `SearchOptions` search.
fn find_bar_html(tr: &Translator) -> String {
    format!(
        r#"<div id="tasty-find-bar" role="search" hidden><input id="tasty-find-input" type="text" placeholder="{placeholder}" autocomplete="off" spellcheck="false"><span id="tasty-find-count">0/0</span><button type="button" id="tasty-find-prev" class="tasty-find-btn" title="{prev}" aria-label="{prev}">&#9650;</button><button type="button" id="tasty-find-next" class="tasty-find-btn" title="{next}" aria-label="{next}">&#9660;</button><button type="button" id="tasty-find-close" class="tasty-find-btn" title="{close}" aria-label="{close}">&times;</button></div>"#,
        placeholder = attr_escape(tr.t("markdown.find.placeholder")),
        prev = attr_escape(tr.t("markdown.find.prev_tooltip")),
        next = attr_escape(tr.t("markdown.find.next_tooltip")),
        close = attr_escape(tr.t("markdown.find.close_tooltip")),
    )
}

/// Trusted, plugin-authored script (never sanitized — [`nav_script`]'s doc comment explains why
/// this category of script is safe: it never touches user markdown content, only the DOM
/// structure this same trusted pipeline already built).
///
/// Implements the TODO's recommended "trust JS" direction over a native find API
/// (`WebKitFindController`/`WKWebView.find`/WebView2 `Find`): none of those three engines' native
/// find surfaces agree on a feature set (regex isn't supported by any of them; whole-word varies),
/// and getting a live match-count back to the host would need a new bidirectional signal per
/// backend (WebKitGTK's is async-signal-based, WKWebView's is a completion handler, WebView2's is
/// an event) for a feature that's markdown-plugin-local anyway — a DOM `TreeWalker` walk is a few
/// dozen lines and needs zero host API surface.
///
/// Algorithm: on each search, first [`clearHighlights`](https://developer.mozilla.org — see
/// inline) unwraps every previously-inserted `<mark>` back into its original text (via
/// `Node.normalize()`, merging the split text nodes back together) — this is the "restore DOM on
/// search end/change" requirement, run before every new search rather than only on close, so
/// stale highlights never accumulate across keystrokes. Then a `TreeWalker` over `#tasty-md-body`
/// (`NodeFilter.SHOW_TEXT`) visits every text node, **rejecting** (not just skipping — `TreeWalker`
/// still descends into a rejected node's siblings) any node whose ancestor chain (up to
/// `#tasty-md-body`) hits a `<pre>`/`<code>` — the scope-exclusion policy decision: code blocks
/// are excluded from find-in-page (matching a common find-in-page convention — code samples often
/// contain the search term as noise, e.g. searching a prose word that also happens to appear in a
/// fenced shell command). The find bar itself lives outside `#tasty-md-body` (see
/// [`find_bar_html`]'s placement in `render_document`), so it's naturally excluded without an
/// explicit check.
///
/// The query is escaped as a regex literal (`escapeRegExp`) before being compiled with the `gi`
/// flags — this is **not** a regex-search feature (out of scope, see [`find_bar_html`]'s doc), it
/// only reuses `RegExp` as the engine for case-insensitive multi-match-per-node scanning.
///
/// IME: `compositionstart`/`compositionend` bracket the input's own `input` event — while
/// composing, `input` events fire per-keystroke of the *in-progress* (not-yet-committed)
/// composition and must not trigger a search (matching `docs/ai-verification/ime-testing.md`'s
/// general IME-safety principle); the debounced search only (re)fires on `compositionend` or on
/// a post-composition plain `input` event.
fn find_in_page_script(tr: &Translator) -> String {
    let json_or_empty = |s: &str| serde_json::to_string(s).unwrap_or_else(|_| "\"\"".to_string());
    format!(
        r#"(function(){{
var bar=document.getElementById('tasty-find-bar');
var input=document.getElementById('tasty-find-input');
var countEl=document.getElementById('tasty-find-count');
var prevBtn=document.getElementById('tasty-find-prev');
var nextBtn=document.getElementById('tasty-find-next');
var closeBtn=document.getElementById('tasty-find-close');
var body=document.getElementById('tasty-md-body');
if(!bar||!input||!countEl||!prevBtn||!nextBtn||!closeBtn||!body)return;
var MATCH_COUNT={match_count};
var matches=[];
var current=-1;
var composing=false;
var debounceTimer=null;
function clearHighlights(){{
matches.forEach(function(m){{
var parent=m.parentNode;
if(!parent)return;
parent.replaceChild(document.createTextNode(m.textContent),m);
parent.normalize();
}});
matches=[];
current=-1;
}}
function escapeRegExp(s){{return s.replace(/[.*+?^${{}}()|[\]\\]/g,'\\$&');}}
function collectTextNodes(){{
var nodes=[];
var walker=document.createTreeWalker(body,NodeFilter.SHOW_TEXT,{{
acceptNode:function(node){{
if(!node.nodeValue||!node.nodeValue.trim())return NodeFilter.FILTER_REJECT;
var el=node.parentElement;
while(el&&el!==body){{
if(el.tagName==='PRE'||el.tagName==='CODE')return NodeFilter.FILTER_REJECT;
el=el.parentElement;
}}
return NodeFilter.FILTER_ACCEPT;
}}
}});
var n;
while((n=walker.nextNode()))nodes.push(n);
return nodes;
}}
function updateCount(){{
var total=matches.length;
if(total===0){{
countEl.textContent='0/0';
}}else{{
countEl.textContent=MATCH_COUNT.replace('{{current}}',String(current+1)).replace('{{total}}',String(total));
}}
countEl.classList.toggle('tasty-find-nomatch',input.value.length>0&&total===0);
prevBtn.disabled=total===0;
nextBtn.disabled=total===0;
}}
function applyCurrent(){{
matches.forEach(function(m,i){{
if(i===current)m.classList.add('tasty-find-current');
else m.classList.remove('tasty-find-current');
}});
if(current>=0)matches[current].scrollIntoView({{block:'center'}});
}}
function runSearch(){{
clearHighlights();
var query=input.value;
if(!query){{updateCount();return;}}
var re=new RegExp(escapeRegExp(query),'gi');
collectTextNodes().forEach(function(node){{
var text=node.nodeValue;
re.lastIndex=0;
var m;
var frag=null;
var lastIndex=0;
while((m=re.exec(text))){{
if(!frag)frag=document.createDocumentFragment();
if(m.index>lastIndex)frag.appendChild(document.createTextNode(text.slice(lastIndex,m.index)));
var mark=document.createElement('mark');
mark.className='tasty-find-hit';
mark.textContent=m[0];
frag.appendChild(mark);
matches.push(mark);
lastIndex=m.index+m[0].length;
}}
if(frag){{
if(lastIndex<text.length)frag.appendChild(document.createTextNode(text.slice(lastIndex)));
node.parentNode.replaceChild(frag,node);
}}
}});
current=matches.length?0:-1;
applyCurrent();
updateCount();
}}
function scheduleSearch(){{
if(composing)return;
if(debounceTimer)clearTimeout(debounceTimer);
debounceTimer=setTimeout(runSearch,150);
}}
function next(){{if(!matches.length)return;current=(current+1)%matches.length;applyCurrent();updateCount();}}
function prev(){{if(!matches.length)return;current=(current-1+matches.length)%matches.length;applyCurrent();updateCount();}}
function openBar(){{
bar.hidden=false;
input.focus();
input.select();
}}
function closeBar(){{
if(bar.hidden)return;
bar.hidden=true;
clearHighlights();
input.value='';
updateCount();
}}
input.addEventListener('compositionstart',function(){{composing=true;}});
input.addEventListener('compositionend',function(){{composing=false;scheduleSearch();}});
input.addEventListener('input',scheduleSearch);
input.addEventListener('keydown',function(e){{
if(e.key==='Escape'){{e.preventDefault();closeBar();}}
else if(e.key==='Enter'){{e.preventDefault();if(e.shiftKey)prev();else next();}}
}});
prevBtn.addEventListener('click',prev);
nextBtn.addEventListener('click',next);
closeBtn.addEventListener('click',closeBar);
document.addEventListener('keydown',function(e){{
if((e.ctrlKey||e.metaKey)&&!e.altKey&&(e.key==='f'||e.key==='F')){{
e.preventDefault();
openBar();
}}else if(e.key==='Escape'&&!bar.hidden){{
closeBar();
}}
}});
}})();"#,
        match_count = json_or_empty(tr.t("markdown.find.match_count")),
    )
}

// ── mermaid ────────────────────────────────────────────────────────────────────

/// Vendored `mermaid.js` UMD-equivalent bundle (see `assets/NOTICE.md` for version/license/
/// source). Fetched once at packaging time — never over the network at runtime, matching
/// Tasty's offline-first principle.
const MERMAID_JS_RAW: &str = include_str!("../assets/mermaid.min.js");

/// Neutralize every `</script` occurrence in `s` — **case-insensitively** — by inserting a
/// backslash between `<` and `/` (`<\/script`, trailing letters' original case preserved).
/// Necessary because the bundle is embedded verbatim inside an HTML `<script>` element: the
/// HTML tokenizer's raw-text end-tag scan compares the tag name case-insensitively (`</SCRIPT>`
/// or `</Script>` closes a `<script>` element exactly like `</script>` does), so a literal
/// occurrence inside the minified source in *any* case (e.g. in a string constant) would
/// truncate the script and break every diagram on the page — a plain case-sensitive
/// `str::replace("</script", ..)` only defuses the all-lowercase form. `\/` is a valid escape
/// for `/` in JS string/regex literals, so the substitution is semantics-preserving regardless
/// of case.
///
/// Byte-level (not char-level) so multi-byte UTF-8 in the input is never touched except by
/// copying it through unchanged: `to_ascii_lowercase` only folds ASCII bytes and never changes a
/// string's byte length, so a match against the lowercased copy at byte offset `i` guarantees
/// `bytes[i..i+8]` is in-bounds and — since `</script` is pure ASCII — falls on UTF-8
/// char-boundary-safe split points in the original.
fn escape_script_close(s: &str) -> String {
    let bytes = s.as_bytes();
    let lower = s.to_ascii_lowercase();
    let lower_bytes = lower.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if lower_bytes[i..].starts_with(b"</script") {
            out.extend_from_slice(b"<\\/");
            out.extend_from_slice(&bytes[i + 2..i + 8]);
            i += 8;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).unwrap_or_default()
}

/// [`MERMAID_JS_RAW`] with [`escape_script_close`] applied (memoized — the source is ~3.5MB and
/// this runs once per process, not per render). The vendored build has zero `</script`
/// occurrences in any case today (grep-verified when it was vendored), but this guards future
/// re-vendors that might introduce one.
fn mermaid_js_source() -> &'static str {
    static ESCAPED: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    ESCAPED.get_or_init(|| escape_script_close(MERMAID_JS_RAW))
}

/// Inline mermaid bundle + init/run script — only called when the document actually has a
/// `language-mermaid` fenced block (see call site in [`render_document`]).
///
/// `mermaid_theme` mirrors mermaid's built-in `dark`/`default` palettes onto the host
/// `Theme.is_light`. `reload_all_webviews` (`main.rs`) regenerates the whole document from
/// scratch on `theme.changed`, so a fresh `is_light` gets baked in on every theme flip — no
/// separate runtime re-theme path is needed.
///
/// `mermaid.run` is called with `querySelector: 'code.language-mermaid'` — the DOM already has
/// every code block at this point (this script tag is emitted after `#tasty-md-body` in document
/// order, so the HTML parser has already built those elements by the time it executes this
/// `<script>`). `suppressErrors: true` plus a `.catch()` guard against a broken diagram killing
/// page script execution: this exact vendored build's `run()` already renders every matched
/// diagram independently inside an internal loop and only aggregates+rethrows *after* the loop
/// completes (verified by reading the bundled source) — so a bad diagram never blocks the others
/// and is simply left as its original unrendered code text; `suppressErrors` just stops that
/// trailing rethrow from rejecting the returned promise, and `.catch()` is defense-in-depth for
/// any other failure path (e.g. `mermaid.initialize` itself).
fn mermaid_script(is_light: bool) -> String {
    let mermaid_theme = if is_light { "default" } else { "dark" };
    format!(
        r#"<script>{js}</script><script>(function(){{try{{mermaid.initialize({{startOnLoad:false,theme:'{mermaid_theme}'}});mermaid.run({{querySelector:'code.language-mermaid',suppressErrors:true}}).catch(function(e){{console.error('mermaid render failed',e);}});}}catch(e){{console.error('mermaid init failed',e);}}}})();</script>"#,
        js = mermaid_js_source(),
        mermaid_theme = mermaid_theme,
    )
}

// ── syntax highlighting (highlight.js) ──────────────────────────────────────────

/// Vendored `highlight.js` "common" bundle (see `assets/NOTICE.md` for version/license/source/
/// language coverage). Fetched once at packaging time — never over the network at runtime,
/// matching Tasty's offline-first principle.
const HIGHLIGHT_JS_RAW: &str = include_str!("../assets/highlight.min.js");

/// [`HIGHLIGHT_JS_RAW`] with [`escape_script_close`] applied (memoized — ~125KB, cheap, but no
/// reason to re-scan on every render).
fn highlight_js_source() -> &'static str {
    static ESCAPED: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    ESCAPED.get_or_init(|| escape_script_close(HIGHLIGHT_JS_RAW))
}

/// Inline the highlight.js bundle + a run-once init script — only called when the document
/// actually has a fenced code block (see call site in [`render_document`]).
///
/// Unlike [`mermaid_script`], this never bakes a light/dark choice into the emitted `<script>`:
/// highlight.js only tokenizes code into `<span class="hljs-*">` elements, it never picks colors
/// itself. Coloring those classes is `hljs_css`'s job (in `theme_css`, which — like every other
/// rule in that stylesheet — is derived straight from `Theme` and regenerated on every
/// `theme.changed` re-render), so this function has nothing theme-dependent left to do.
///
/// For every `<code class="language-<lang>">` under a `<pre>` (the exact shape
/// [`rewrite_code_block_event`]/[`sanitize_fence_lang`] produce): re-extract `<lang>` from the
/// class (independently of pulldown-cmark's own already-sanitized value — this only trusts the
/// same `[A-Za-z0-9_+-]` shape, nothing more), skip it silently via `hljs.getLanguage` if
/// highlight.js doesn't recognize that identifier (covers both genuinely unsupported languages
/// and non-code fences like `language-mermaid`), otherwise call `hljs.highlightElement`.
/// `highlightElement` reads the node's plain-text content and rewrites its `innerHTML` with its
/// own HTML-escaped token spans — it never reinterprets existing markup, so a code block
/// containing the literal text `<script>` (already HTML-escaped by `sanitize_html` before this
/// script ever runs) stays inert text after highlighting too. Each block's own try/catch means an
/// unexpected highlight.js failure on one block leaves that block as plain unhighlighted text
/// without aborting the rest of the document (mirrors `mermaid_script`'s `suppressErrors`/`.catch`
/// per-diagram isolation, just no async promise chain here since `highlightElement` is
/// synchronous).
fn highlight_script() -> String {
    format!(
        r#"<script>{js}</script><script>(function(){{try{{document.querySelectorAll('pre code[class*="language-"]').forEach(function(el){{try{{var m=/language-([A-Za-z0-9_+-]+)/.exec(el.className);if(!m)return;if(!hljs.getLanguage(m[1]))return;hljs.highlightElement(el);}}catch(e){{console.error('highlight.js block failed',e);}}}});}}catch(e){{console.error('highlight.js init failed',e);}}}})();</script>"#,
        js = highlight_js_source(),
    )
}

// ── copy-to-clipboard button ────────────────────────────────────────────────

/// Inline a copy-to-clipboard button attachment script — only called when the document actually
/// has at least one `<pre><code>` block (see call site in [`render_document`]).
///
/// Selector is scoped to `#tasty-md-body pre > code`, **not** every `<pre>` in the document — the
/// load-error detail box (`.tasty-state-detail`, also a bare `<pre>`, see [`render_document`]'s
/// `load_error` branch) lives outside `#tasty-md-body` and never gets a button.
///
/// Unconditionally skips `code.language-mermaid`, regardless of execution order relative to
/// [`mermaid_script`]: `mermaid.run()` is asynchronous (returns a promise it never awaits — see
/// that function's doc comment), so even placing this script strictly after `mermaid_script` in
/// document order gives no guarantee the diagram DOM-replacement has already happened by the time
/// this one runs. The class check is the only race-proof way to never attach a button to a
/// soon-to-be-replaced mermaid block; script ordering can't substitute for it.
///
/// Reads `code.textContent` at click time (not `innerHTML`) — this is why ordering relative to
/// [`highlight_script`] doesn't matter either: `highlightElement` only wraps existing characters
/// in `<span class="hljs-*">`, it never changes them. Live-verified against the actual vendored
/// bundle in the real WebKitGTK engine (details: `docs/plugins/markdown/screens/markdown.md`), so
/// `textContent` returns the exact original code whether or not highlighting has already run on
/// that block.
///
/// `navigator.clipboard.writeText` falls back to the legacy `document.execCommand('copy')` path
/// (an offscreen, unfocused-looking `<textarea>`) when the modern API is unavailable or its
/// promise rejects. Live-verified on Linux/WebKitGTK — the primary `writeText` path succeeded
/// outright there, so the fallback wasn't exercised on this backend (details:
/// `docs/plugins/markdown/screens/markdown.md`); the same code path is expected to hold on
/// WKWebView/WebView2 since both implement the standard async Clipboard API, but that's read-only
/// verification (no macOS/Windows execution environment here) — noted, not claimed as tested.
fn copy_button_script(tr: &Translator) -> String {
    let json_or_empty = |s: &str| serde_json::to_string(s).unwrap_or_else(|_| "\"\"".to_string());
    format!(
        r#"<script>(function(){{
var COPY={copy},COPIED={copied},FAILED={failed};
function fallbackCopy(text){{
try{{
var ta=document.createElement('textarea');
ta.value=text;
ta.setAttribute('readonly','');
ta.style.position='fixed';
ta.style.top='-1000px';
ta.style.opacity='0';
document.body.appendChild(ta);
ta.select();
ta.setSelectionRange(0,text.length);
var ok=document.execCommand('copy');
document.body.removeChild(ta);
return ok;
}}catch(e){{return false;}}
}}
function feedback(btn,ok){{
if(btn._tastyResetTimer)clearTimeout(btn._tastyResetTimer);
btn.textContent=ok?COPIED:FAILED;
btn.setAttribute('data-state',ok?'copied':'failed');
btn._tastyResetTimer=setTimeout(function(){{
btn.textContent=COPY;
btn.removeAttribute('data-state');
}},1500);
}}
try{{
document.querySelectorAll('#tasty-md-body pre > code').forEach(function(code){{
try{{
if(code.classList.contains('language-mermaid'))return;
var pre=code.parentElement;
if(!pre||pre.querySelector('.tasty-copy-btn'))return;
var btn=document.createElement('button');
btn.type='button';
btn.className='tasty-copy-btn';
btn.setAttribute('tabindex','0');
btn.setAttribute('aria-label',COPY);
btn.textContent=COPY;
btn.addEventListener('click',function(){{
var text=code.textContent;
if(navigator.clipboard&&navigator.clipboard.writeText){{
navigator.clipboard.writeText(text).then(function(){{feedback(btn,true);}},function(){{feedback(btn,fallbackCopy(text));}});
}}else{{
feedback(btn,fallbackCopy(text));
}}
}});
pre.appendChild(btn);
}}catch(e){{console.error('copy button attach failed',e);}}
}});
}}catch(e){{console.error('copy button init failed',e);}}
}})();</script>"#,
        copy = json_or_empty(tr.t("markdown.copy.label")),
        copied = json_or_empty(tr.t("markdown.copy.copied")),
        failed = json_or_empty(tr.t("markdown.copy.failed")),
    )
}

/// Inline an image load-failure watcher — only called when the document actually has at least
/// one `<img>` (see call site in [`render_document`]). `sanitize_html` strips every inline event
/// handler (`onerror=` included, its core defense line), so this attaches `error` listeners
/// programmatically instead — the same trust-script pattern [`copy_button_script`] uses to
/// post-process sanitized DOM.
///
/// Two failure paths, both required: a listener catches images that fail *after* this script
/// runs, but by the time a script this far down the document executes, some images may have
/// already finished failing — `img.complete && img.naturalWidth===0` catches those retroactively
/// (a loaded-with-zero-pixels image is exactly what a failed load looks like; a genuinely empty
/// `src` also reads as `complete` with no dimensions, so this also naturally covers `![alt]()`).
/// A `data-tasty-img-failed` guard on the element makes the replacement idempotent in case both
/// paths fire for the same image (the listener can still be queued when the retroactive check
/// already ran).
///
/// Reads `img.getAttribute('src')` (the literal markdown-authored value), not the `img.src`
/// property (which `<base href>`, see [`file_dir_uri`], has already normalized into an absolute
/// `file://` URI) — this is what keeps the placeholder's path human-readable.
///
/// A remote image blocked by the host's remote-content policy fails to load for a real reason
/// (the request never resolves successfully) — the browser's `error` event fires for it exactly
/// as it would for any other failed fetch, so it correctly lands on the placeholder rather than
/// silently passing through as a false positive.
///
/// Live-verified in the real WebKitGTK engine against a document with one real image and one
/// genuinely missing path loaded over a real `file://` base (details:
/// `docs/plugins/markdown/screens/markdown.md`): the real image stayed an `<img>`, the missing
/// one was replaced with `.tasty-img-error` carrying the original alt as `aria-label` and the
/// original relative path as its text — and re-running this exact script a second time against
/// the same DOM left `placeholderCount` at 1, confirming the per-element guards hold.
fn image_error_script(tr: &Translator) -> String {
    let json_or_empty = |s: &str| serde_json::to_string(s).unwrap_or_else(|_| "\"\"".to_string());
    format!(
        r#"<script>(function(){{
var LABEL={label};
function replaceWithPlaceholder(img){{
if(img.dataset.tastyImgFailed)return;
img.dataset.tastyImgFailed='1';
var src=img.getAttribute('src')||'';
var alt=img.getAttribute('alt')||'';
var ph=document.createElement('span');
ph.className='tasty-img-error';
ph.setAttribute('role','img');
ph.setAttribute('aria-label',alt||LABEL);
var icon=document.createElement('span');
icon.className='tasty-img-error-icon';
ph.appendChild(icon);
var label=document.createElement('span');
label.className='tasty-img-error-label';
label.textContent=LABEL;
ph.appendChild(label);
if(src){{
var path=document.createElement('span');
path.className='tasty-img-error-path';
path.textContent=src;
ph.appendChild(path);
}}
if(img.parentNode)img.parentNode.replaceChild(ph,img);
}}
try{{
document.querySelectorAll('#tasty-md-body img').forEach(function(img){{
try{{
if(img.dataset.tastyImgChecked)return;
img.dataset.tastyImgChecked='1';
img.addEventListener('error',function(){{replaceWithPlaceholder(img);}});
if(img.complete&&img.naturalWidth===0){{replaceWithPlaceholder(img);}}
}}catch(e){{console.error('image error-check attach failed',e);}}
}});
}}catch(e){{console.error('image error-check init failed',e);}}
}})();</script>"#,
        label = json_or_empty(tr.t("markdown.image.failed")),
    )
}

// ── math (KaTeX) ─────────────────────────────────────────────────────────────

/// Vendored `katex.min.js` bundle (see `assets/NOTICE.md` for version/license/source). Fetched
/// once at packaging time — never over the network at runtime.
const KATEX_JS_RAW: &str = include_str!("../assets/katex.min.js");

/// Vendored `katex.min.css` — still has its original `@font-face { src: url(fonts/...) }`
/// relative paths at this point; [`katex_css_with_embedded_fonts`] rewrites those before use.
const KATEX_CSS_RAW: &str = include_str!("../assets/katex.min.css");

/// Every font KaTeX 0.18.4 ships (`woff2` only — see `assets/NOTICE.md`), paired with the exact
/// basename [`KATEX_CSS_RAW`]'s `@font-face` rules reference so [`katex_css_with_embedded_fonts`]
/// can find-and-replace each one directly by string match (no regex dependency needed — the set
/// of basenames is fixed and known at compile time).
const KATEX_FONTS: &[(&str, &[u8])] = &[
    (
        "KaTeX_AMS-Regular",
        include_bytes!("../assets/fonts/KaTeX_AMS-Regular.woff2"),
    ),
    (
        "KaTeX_Caligraphic-Bold",
        include_bytes!("../assets/fonts/KaTeX_Caligraphic-Bold.woff2"),
    ),
    (
        "KaTeX_Caligraphic-Regular",
        include_bytes!("../assets/fonts/KaTeX_Caligraphic-Regular.woff2"),
    ),
    (
        "KaTeX_Fraktur-Bold",
        include_bytes!("../assets/fonts/KaTeX_Fraktur-Bold.woff2"),
    ),
    (
        "KaTeX_Fraktur-Regular",
        include_bytes!("../assets/fonts/KaTeX_Fraktur-Regular.woff2"),
    ),
    (
        "KaTeX_Main-Bold",
        include_bytes!("../assets/fonts/KaTeX_Main-Bold.woff2"),
    ),
    (
        "KaTeX_Main-BoldItalic",
        include_bytes!("../assets/fonts/KaTeX_Main-BoldItalic.woff2"),
    ),
    (
        "KaTeX_Main-Italic",
        include_bytes!("../assets/fonts/KaTeX_Main-Italic.woff2"),
    ),
    (
        "KaTeX_Main-Regular",
        include_bytes!("../assets/fonts/KaTeX_Main-Regular.woff2"),
    ),
    (
        "KaTeX_Math-BoldItalic",
        include_bytes!("../assets/fonts/KaTeX_Math-BoldItalic.woff2"),
    ),
    (
        "KaTeX_Math-Italic",
        include_bytes!("../assets/fonts/KaTeX_Math-Italic.woff2"),
    ),
    (
        "KaTeX_SansSerif-Bold",
        include_bytes!("../assets/fonts/KaTeX_SansSerif-Bold.woff2"),
    ),
    (
        "KaTeX_SansSerif-Italic",
        include_bytes!("../assets/fonts/KaTeX_SansSerif-Italic.woff2"),
    ),
    (
        "KaTeX_SansSerif-Regular",
        include_bytes!("../assets/fonts/KaTeX_SansSerif-Regular.woff2"),
    ),
    (
        "KaTeX_Script-Regular",
        include_bytes!("../assets/fonts/KaTeX_Script-Regular.woff2"),
    ),
    (
        "KaTeX_Size1-Regular",
        include_bytes!("../assets/fonts/KaTeX_Size1-Regular.woff2"),
    ),
    (
        "KaTeX_Size2-Regular",
        include_bytes!("../assets/fonts/KaTeX_Size2-Regular.woff2"),
    ),
    (
        "KaTeX_Size3-Regular",
        include_bytes!("../assets/fonts/KaTeX_Size3-Regular.woff2"),
    ),
    (
        "KaTeX_Size4-Regular",
        include_bytes!("../assets/fonts/KaTeX_Size4-Regular.woff2"),
    ),
    (
        "KaTeX_Typewriter-Regular",
        include_bytes!("../assets/fonts/KaTeX_Typewriter-Regular.woff2"),
    ),
];

/// [`KATEX_JS_RAW`] with [`escape_script_close`] applied (memoized, same convention as
/// [`mermaid_js_source`]/[`highlight_js_source`] — the bundle is inlined verbatim inside an HTML
/// `<script>` element).
fn katex_js_source() -> &'static str {
    static ESCAPED: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    ESCAPED.get_or_init(|| escape_script_close(KATEX_JS_RAW))
}

/// [`KATEX_CSS_RAW`] with every `@font-face`'s `src:` list collapsed from three formats
/// (`woff2`/`woff`/`truetype`, each a relative `fonts/<name>.<ext>` URL) down to a single
/// `data:font/woff2;base64,<...>` entry built from the matching [`KATEX_FONTS`] bytes. Memoized —
/// base64-encoding ~254KB of font data is wasted work to repeat on every render.
///
/// Data URIs, not relative `file://` paths: see `assets/NOTICE.md`'s "Font offline delivery"
/// note — there is no on-disk plugin-assets directory a relative URL inside the rendered document
/// could resolve against at runtime (everything is `include_str!`/`include_bytes!`-baked into the
/// binary), and the document's one `<base href>` already belongs to the user's own markdown file
/// directory.
fn katex_css_with_embedded_fonts() -> &'static str {
    static EMBEDDED: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    EMBEDDED.get_or_init(|| {
        use base64::Engine;
        let mut css = KATEX_CSS_RAW.to_string();
        for (name, bytes) in KATEX_FONTS {
            let original = format!(
                r#"url(fonts/{name}.woff2) format("woff2"),url(fonts/{name}.woff) format("woff"),url(fonts/{name}.ttf) format("truetype")"#
            );
            let b64 = base64::engine::general_purpose::STANDARD.encode(bytes);
            let replacement = format!(r#"url(data:font/woff2;base64,{b64}) format("woff2")"#);
            css = css.replace(&original, &replacement);
        }
        css
    })
}

/// Inline the KaTeX bundle + init script — only called when the document actually has at least
/// one `math`/`math-display` span (see call site in [`render_document`]). Scoped to
/// `#tasty-md-body .math-inline, #tasty-md-body .math-display` — the same shape
/// `pulldown-cmark`'s `Options::ENABLE_MATH` HTML writer emits by default
/// (`<span class="math math-inline">`/`<span class="math math-display">`, LaTeX source
/// HTML-escaped as the span's text) — no custom AST event rewrite was needed to produce this
/// shape (see `sanitize_html`'s `span`+`class` whitelist comment for why that shape is allowed
/// through sanitization as-is).
///
/// Reads `el.textContent` (not `innerHTML`) to recover the literal LaTeX source — the DOM
/// decodes the HTML entities `escape_html` encoded back into the original characters, so this
/// gets exactly what the author typed between `$...$`/`$$...$$`.
///
/// `throwOnError: false` + `trust: false` are set explicitly (not left as defaults) per this
/// task's security requirement: `trust: false` blocks LaTeX commands (`\includegraphics`,
/// `\href`, `\url`, ...) that could otherwise smuggle arbitrary URLs/markup through a macro,
/// independent of `sanitize_html`'s own allowlist. `throwOnError: false` makes KaTeX render a
/// parse failure as a visible (originally `errorColor`-tinted) rendering of the original TeX
/// source instead of throwing — combined with the outer per-element `try/catch` (defense-in-depth
/// against a non-`ParseError` exception), a broken formula never crashes the page or blanks the
/// element. Live-verified in the real WebKitGTK engine (details: `docs/plugins/markdown/screens/
/// markdown.md`): a genuinely broken formula (`\frac{1}` — missing its second argument) produced
/// a `.katex-error` element whose text is exactly the original TeX source, while two valid
/// formulas alongside it rendered as real KaTeX MathML output (`.katex`), and the display-mode
/// one was wrapped in KaTeX's own `.katex-display`.
///
/// `color: currentColor` is KaTeX's own default (verified directly in the vendored
/// `katex.min.css` — zero hardcoded colors) — math already inherits `body`'s `color:var(--md-fg)`
/// with no extra theme wiring needed, and follows dark/light exactly like every other themed
/// element (`theme_css`'s whole `<style>` block is regenerated on every theme change/reload,
/// same as everything else in this document).
///
/// `data-tasty-math-rendered` per-span guard mirrors [`copy_button_script`]/
/// [`image_error_script`]'s idempotency convention — defense-in-depth in case this script somehow
/// runs twice against the same DOM (structurally it shouldn't: `reload_webview` always replaces
/// the whole document, never patches it in place). Without the guard, a second `katex.render`
/// call would try to re-parse the *already-rendered* KaTeX DOM's `textContent` as TeX instead of
/// the original source.
fn katex_script() -> String {
    format!(
        r#"<style>{css}</style><script>{js}</script><script>(function(){{
try{{
document.querySelectorAll('#tasty-md-body .math-inline, #tasty-md-body .math-display').forEach(function(el){{
try{{
if(el.dataset.tastyMathRendered)return;
el.dataset.tastyMathRendered='1';
var tex=el.textContent;
var display=el.classList.contains('math-display');
katex.render(tex,el,{{throwOnError:false,trust:false,displayMode:display}});
}}catch(e){{console.error('katex render failed',e);}}
}});
}}catch(e){{console.error('katex init failed',e);}}
}})();</script>"#,
        css = katex_css_with_embedded_fonts(),
        js = katex_js_source(),
    )
}

// ── escaping helpers ──────────────────────────────────────────────────────────

/// Escape text for placement in an HTML text node.
fn html_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            _ => out.push(c),
        }
    }
    out
}

/// Escape text for placement inside a double-quoted HTML attribute value.
fn attr_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(c),
        }
    }
    out
}

/// Percent-encode every byte outside `A-Za-z0-9-_.~` — used for the nav-fragment payload
/// (embedded in an `href` attribute, so this alone also makes HTML-attribute-escaping moot:
/// no `&`/`"`/`<` survive encoding).
fn percent_encode_fragment(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char);
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// Percent-encode a `file://` path's characters (keeps `/` unescaped, unlike
/// [`percent_encode_fragment`] — this builds a path, not an opaque fragment payload).
fn percent_encode_path(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' | b'/' => {
                out.push(b as char);
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// Decode `%XX` percent-escapes (inverse of [`percent_encode_fragment`] / JS `encodeURIComponent`).
/// Malformed sequences pass through literally rather than erroring — this decodes a same-process
/// value we generated ourselves moments earlier, so a defensive fallback is enough.
fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && let Ok(byte) = u8::from_str_radix(&s[i + 1..i + 3], 16)
        {
            out.push(byte);
            i += 3;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base() -> PathBuf {
        if cfg!(windows) {
            PathBuf::from(r"C:\docs\md")
        } else {
            PathBuf::from("/docs/md")
        }
    }

    fn file_str(c: Option<LinkClick>) -> String {
        match c {
            Some(LinkClick::File(p)) => p.to_string_lossy().replace('\\', "/"),
            other => panic!("expected File, got {other:?}"),
        }
    }

    #[test]
    fn relative_resolves_against_base_dir() {
        let b = base();
        let got = file_str(classify_link("docs/index.md", Some(&b)));
        let want = base()
            .join("docs/index.md")
            .to_string_lossy()
            .replace('\\', "/");
        assert_eq!(got, want);
    }

    #[test]
    fn parent_relative_is_normalized() {
        let b = base();
        let got = file_str(classify_link("../sibling.md", Some(&b)));
        let want = if cfg!(windows) {
            "C:/docs/sibling.md"
        } else {
            "/docs/sibling.md"
        };
        assert_eq!(got, want);
    }

    #[test]
    fn absolute_dest_passes_through() {
        let b = base();
        let abs = if cfg!(windows) {
            r"C:\other\readme.md"
        } else {
            "/other/readme.md"
        };
        let got = file_str(classify_link(abs, Some(&b)));
        let want = if cfg!(windows) {
            "C:/other/readme.md"
        } else {
            "/other/readme.md"
        };
        assert_eq!(got, want);
    }

    #[test]
    fn external_schemes_are_external() {
        let b = base();
        for url in [
            "http://example.com",
            "https://example.com/page",
            "mailto:foo@bar.com",
            "data:text/plain;base64,AAAA",
        ] {
            assert_eq!(
                classify_link(url, Some(&b)),
                Some(LinkClick::External(url.to_string())),
                "{url} should be External",
            );
        }
    }

    #[test]
    fn javascript_scheme_is_ignored_not_treated_as_file() {
        let b = base();
        assert_eq!(classify_link("javascript:alert(1)", Some(&b)), None);
        assert_eq!(
            classify_link("JavaScript:alert(document.cookie)", Some(&b)),
            None
        );
    }

    #[test]
    fn anchor_is_ignored() {
        assert_eq!(classify_link("#section", base().as_path().into()), None);
    }

    #[test]
    fn relative_without_base_dir_is_unresolvable() {
        assert_eq!(classify_link("docs/index.md", None), None);
    }

    #[test]
    fn empty_dest_is_ignored() {
        assert_eq!(classify_link("", Some(&base())), None);
    }

    #[test]
    fn rewrite_link_dest_leaves_anchors_untouched() {
        assert_eq!(rewrite_link_dest("#heading"), "#heading");
        assert_eq!(rewrite_link_dest(""), "");
    }

    #[test]
    fn rewrite_link_dest_percent_encodes_into_nav_fragment() {
        let got = rewrite_link_dest("../sibling.md");
        assert_eq!(got, "#tasty-nav:link:..%2Fsibling.md");
    }

    #[test]
    fn parse_nav_fragment_roundtrips_link_and_addr() {
        let link_url = format!(
            "about:blank#{}",
            rewrite_link_dest("../sibling.md").trim_start_matches('#')
        );
        assert_eq!(
            parse_nav_fragment(&link_url),
            Some(NavIntent::Link("../sibling.md".to_string()))
        );

        let addr_url = "file:///docs/md/#tasty-nav:addr:%2Fhome%2Fu%2Fnotes.md";
        assert_eq!(
            parse_nav_fragment(addr_url),
            Some(NavIntent::Addr("/home/u/notes.md".to_string()))
        );
    }

    #[test]
    fn parse_nav_fragment_none_for_unrelated_url() {
        assert_eq!(parse_nav_fragment("about:blank"), None);
        assert_eq!(parse_nav_fragment("https://example.com/#section"), None);
    }

    #[test]
    fn file_dir_uri_adds_trailing_slash_and_scheme() {
        let dir = if cfg!(windows) {
            PathBuf::from(r"C:\docs\md")
        } else {
            PathBuf::from("/docs/md")
        };
        let got = file_dir_uri(&dir);
        assert!(got.starts_with("file:///"));
        assert!(got.ends_with('/'));
    }

    #[test]
    fn sanitize_html_strips_script_tags() {
        let out = sanitize_html("<p>hi</p><script>alert(1)</script>");
        assert!(!out.contains("script"));
        assert!(out.contains("hi"));
    }

    #[test]
    fn sanitize_html_strips_event_handler_attributes() {
        let out = sanitize_html(r#"<img src="x.png" onerror="alert(1)">"#);
        // ★ 먼저 img 가 **살아남았다**는 것을 못박는다. 이 줄이 없으면 아래 부정 둘은
        // `out` 이 빈 문자열일 때도 통과하고, 그 초록의 뜻은 "핸들러를 벗겼다" 가 아니라
        // "아무것도 안 남았다" 다. 빈 출력은 가상이 아니다 — 태그 허용목록에서 `img` 가
        // 빠지면 ammonia 가 요소를 통째로 지우고, 그때도 이 시험은 계속 초록이다.
        assert!(out.contains("x.png"));
        assert!(!out.contains("onerror"));
        assert!(!out.contains("alert"));
    }

    #[test]
    fn sanitize_html_strips_javascript_scheme_href() {
        let out = sanitize_html(r#"<a href="javascript:alert(1)">click</a>"#);
        // 위와 같은 이유의 양성 짝. 다만 이 줄이 증명하는 것은 **출력이 비지 않았다**
        // 까지다 — ammonia 는 `<a>` 를 지워도 안쪽 텍스트를 남기므로, "앵커가 살아남았다"
        // 는 뜻이 아니다. 그 이상을 주장하면 그게 새 거짓이다.
        assert!(out.contains("click"));
        assert!(!out.contains("javascript:"));
    }

    #[test]
    fn sanitize_html_keeps_tables_checkboxes_and_code() {
        let source = "| a | b |\n|---|---|\n| 1 | 2 |\n\n- [x] done\n- [ ] todo\n\n`inline` and:\n\n```\nfenced\n```\n";
        let out = sanitize_html(&unsafe_content_html(source, &Translator::default()));
        assert!(out.contains("<table"));
        assert!(out.contains("checkbox"));
        assert!(out.contains("<code"));
        assert!(out.contains("fenced"));
    }

    #[test]
    fn fenced_code_block_language_class_survives_sanitize() {
        let source = "```rust\nfn main() {}\n```\n";
        let out = sanitize_html(&unsafe_content_html(source, &Translator::default()));
        assert!(
            out.contains(r#"class="language-rust""#),
            "expected language class to survive sanitize, got: {out}"
        );
    }

    #[test]
    fn fenced_code_block_language_is_normalized_before_sanitize() {
        // `"` 이후 aren't valid identifier chars; pulldown-cmark itself HTML-escapes the info
        // string too (attribute breakout was never possible), but the point here is that the
        // *value* collapses to a plain identifier — the shape a future mermaid consumer relies on.
        assert_eq!(
            sanitize_fence_lang(r#"rust"><script>alert(1)</script>"#),
            "rustscriptalert1script"
        );
        assert_eq!(sanitize_fence_lang(""), "");
        assert_eq!(
            sanitize_fence_lang("mermaid extra-ignored-token"),
            "mermaid"
        );
    }

    // ── image captions (solo-image paragraphs → <figure>/<figcaption>) ─────────

    #[test]
    fn figurize_promotes_solo_image_with_alt_to_figure_caption() {
        let source = "![A caption](img.png)\n";
        let out = sanitize_html(&unsafe_content_html(source, &Translator::default()));
        assert!(out.contains("<figure>"), "got: {out}");
        assert!(out.contains("</figure>"), "got: {out}");
        assert!(
            out.contains("<figcaption>A caption</figcaption>"),
            "got: {out}"
        );
        assert!(out.contains(r#"src="img.png""#), "got: {out}");
        assert!(out.contains(r#"alt="A caption""#), "got: {out}");
        // No leftover empty <p></p> wrapper around the promoted image.
        assert!(!out.contains("<p>"), "got: {out}");
    }

    #[test]
    fn figurize_leaves_alt_less_image_unpromoted() {
        // alt-less image — nothing to caption, must render exactly as before this feature.
        let source = "![](img.png)\n";
        let out = sanitize_html(&unsafe_content_html(source, &Translator::default()));
        assert!(!out.contains("<figure>"), "got: {out}");
        assert!(!out.contains("<figcaption>"), "got: {out}");
        assert!(out.contains("<p>"), "got: {out}");
        assert!(out.contains(r#"src="img.png""#), "got: {out}");
    }

    #[test]
    fn figurize_does_not_promote_image_mixed_with_text() {
        let source = "before ![alt](img.png) after\n";
        let out = sanitize_html(&unsafe_content_html(source, &Translator::default()));
        assert!(!out.contains("<figure>"), "got: {out}");
        assert!(!out.contains("<figcaption>"), "got: {out}");
        assert!(out.contains("before"), "got: {out}");
        assert!(out.contains("after"), "got: {out}");
        assert!(out.contains(r#"alt="alt""#), "got: {out}");
    }

    #[test]
    fn figurize_does_not_promote_link_wrapped_image() {
        // `[![alt](img.png)](url)` — the image is alone, but wrapped in a link; conservative
        // policy declines promotion here too (see figurize_paragraph_buffer doc).
        let source = "[![alt](img.png)](https://example.com)\n";
        let out = sanitize_html(&unsafe_content_html(source, &Translator::default()));
        assert!(!out.contains("<figure>"), "got: {out}");
        assert!(!out.contains("<figcaption>"), "got: {out}");
        assert!(out.contains("<a "), "got: {out}");
        assert!(out.contains(r#"alt="alt""#), "got: {out}");
    }

    #[test]
    fn figurize_does_not_promote_paragraph_with_two_images() {
        let source = "![a](1.png)![b](2.png)\n";
        let out = sanitize_html(&unsafe_content_html(source, &Translator::default()));
        assert!(!out.contains("<figure>"), "got: {out}");
        assert!(!out.contains("<figcaption>"), "got: {out}");
        assert!(out.contains(r#"src="1.png""#), "got: {out}");
        assert!(out.contains(r#"src="2.png""#), "got: {out}");
    }

    #[test]
    fn figurize_caption_strips_markup_from_alt_with_emphasis() {
        let source = "![**bold** caption](img.png)\n";
        let out = sanitize_html(&unsafe_content_html(source, &Translator::default()));
        assert!(
            out.contains("<figcaption>bold caption</figcaption>"),
            "got: {out}"
        );
        assert!(!out.contains("<strong>"), "got: {out}");
    }

    #[test]
    fn figurize_caption_escapes_special_characters_in_alt() {
        // Spaces around `<`/`>` keep pulldown-cmark from mis-parsing them as inline HTML tags —
        // they stay literal `Event::Text`, matching how a reader would actually type "1 < 2".
        let source = "![1 < 2 & 3 > 1](img.png)\n";
        let out = sanitize_html(&unsafe_content_html(source, &Translator::default()));
        assert!(
            out.contains("<figcaption>1 &lt; 2 &amp; 3 &gt; 1</figcaption>"),
            "got: {out}"
        );
    }

    #[test]
    fn footnote_markup_survives_sanitize() {
        let source = "See[^1].\n\n[^1]: A note.\n";
        let out = sanitize_html(&unsafe_content_html(source, &Translator::default()));
        assert!(out.contains("footnote-reference"), "got: {out}");
        assert!(out.contains("footnote-definition"), "got: {out}");
        assert!(out.contains("A note."), "got: {out}");
    }

    fn real_translator() -> Translator {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("en.toml"), include_str!("../lang/en.toml")).unwrap();
        Translator::load(dir.path(), "en")
    }

    #[test]
    fn footnote_reference_and_backlink_are_wired_correctly() {
        let source = "See[^1].\n\n[^1]: A note.\n";
        let out = unsafe_content_html(source, &Translator::default());
        assert!(
            out.contains(r#"id="fnref-1""#),
            "reference should get id=fnref-1, got: {out}"
        );
        assert!(
            out.contains(r##"href="#fndef-1""##),
            "reference should link to the definition, got: {out}"
        );
        assert!(
            out.contains(r#"id="fndef-1""#),
            "definition should get id=fndef-1, got: {out}"
        );
        assert!(
            out.contains(r##"href="#fnref-1""##),
            "definition should carry exactly one backlink to the reference, got: {out}"
        );
        assert_eq!(
            out.matches(r##"href="#fnref-1""##).count(),
            1,
            "a single reference should produce exactly one backlink, got: {out}"
        );
    }

    #[test]
    fn footnote_multiple_references_get_distinct_backlinks() {
        // Same footnote referenced twice — the definition must carry two backlinks, each
        // targeting its own reference occurrence, with no cross-contamination (e.g. both
        // pointing at the same id, or the second reference silently reusing the first's id).
        let source = "First[^a] and second[^a].\n\n[^a]: Shared note.\n";
        let out = unsafe_content_html(source, &Translator::default());

        assert!(out.contains(r#"id="fnref-a""#), "got: {out}");
        assert!(out.contains(r#"id="fnref-a-2""#), "got: {out}");
        assert_ne!(
            out.matches(r#"id="fnref-a""#).count() + out.matches(r#"id="fnref-a-2""#).count(),
            0
        );

        // Both references must point at the same definition.
        assert_eq!(
            out.matches(r##"href="#fndef-a""##).count(),
            2,
            "both references should link to the shared definition, got: {out}"
        );

        // The definition must carry exactly one backlink per reference occurrence, each
        // pointing at its own distinct id.
        assert_eq!(out.matches(r##"href="#fnref-a""##).count(), 1, "got: {out}");
        assert_eq!(
            out.matches(r##"href="#fnref-a-2""##).count(),
            1,
            "got: {out}"
        );
    }

    #[test]
    fn footnote_name_with_special_chars_gets_safe_id() {
        // Footnote names can contain whitespace/unicode (not valid verbatim in an HTML id) —
        // percent-encoding (reusing `percent_encode_fragment`, the same helper nav-fragments and
        // baked icon data URIs already use) keeps it a single-token, ASCII, collision-safe id.
        let source = "See[^노트 1].\n\n[^노트 1]: Definition.\n";
        let out = unsafe_content_html(source, &Translator::default());

        // No raw space or raw unicode byte sequence should leak into an id/href attribute value.
        assert!(
            !out.contains(r#"id="fnref-노트 1""#),
            "raw unicode/space must not appear verbatim in an id, got: {out}"
        );
        // The percent-encoded form must be internally consistent: whatever id the reference
        // carries is exactly what the definition's href (and vice versa) target.
        let safe = percent_encode_fragment("노트 1");
        assert!(
            out.contains(&format!(r#"id="fnref-{safe}""#)),
            "got: {out} (expected safe id fnref-{safe})"
        );
        assert!(
            out.contains(&format!(r##"href="#fndef-{safe}""##)),
            "got: {out}"
        );
        assert!(out.contains(&format!(r#"id="fndef-{safe}""#)), "got: {out}");
        assert!(
            out.contains(&format!(r##"href="#fnref-{safe}""##)),
            "got: {out}"
        );
    }

    #[test]
    fn undefined_footnote_reference_does_not_panic() {
        // No matching `[^missing]: ...` definition anywhere — pulldown-cmark's parser itself
        // never emits a `FootnoteReference` event for this (confirmed by dumping the raw event
        // stream: it falls back to plain `[`/`^missing`/`]` text), so rewrite_footnote_event
        // never even sees it. This just proves the whole pipeline degrades gracefully rather
        // than panicking or losing content.
        let source = "This has an[^missing] undefined reference.\n";
        let out = unsafe_content_html(source, &Translator::default());
        assert!(!out.contains("footnote-reference"), "got: {out}");
        assert!(out.contains("^missing"), "got: {out}");
    }

    #[test]
    fn footnote_aria_labels_present_in_rendered_html() {
        let tr = real_translator();
        let source = "See[^1].\n\n[^1]: A note.\n";
        let out = unsafe_content_html(source, &tr);
        assert!(
            out.contains(r#"aria-label="Jump to footnote 1""#),
            "got: {out}"
        );
        assert!(
            out.contains(r#"aria-label="Back to footnote 1""#),
            "got: {out}"
        );

        // Survives sanitize_html's allowlist (`a` -> aria-label).
        let sanitized = sanitize_html(&out);
        assert!(
            sanitized.contains(r#"aria-label="Jump to footnote 1""#),
            "got: {sanitized}"
        );
        assert!(
            sanitized.contains(r#"aria-label="Back to footnote 1""#),
            "got: {sanitized}"
        );
    }

    #[test]
    fn footnote_aria_label_distinguishes_multiple_backlinks() {
        let tr = real_translator();
        let source = "First[^a] and second[^a].\n\n[^a]: Shared note.\n";
        let out = unsafe_content_html(source, &tr);
        assert!(
            out.contains(r#"aria-label="Back to footnote 1, reference 1""#),
            "got: {out}"
        );
        assert!(
            out.contains(r#"aria-label="Back to footnote 1, reference 2""#),
            "got: {out}"
        );
    }

    #[test]
    fn document_without_footnotes_renders_unaffected() {
        let source = "# Title\n\nJust a normal paragraph, nothing special.\n";
        let out = unsafe_content_html(source, &Translator::default());
        assert!(!out.contains("footnote"), "got: {out}");
        assert!(!out.contains("fnref-"), "got: {out}");
        assert!(!out.contains("fndef-"), "got: {out}");
        assert!(out.contains("Just a normal paragraph"), "got: {out}");
    }

    #[test]
    fn unsafe_content_html_rewrites_internal_link_hrefs() {
        let out = unsafe_content_html("[go](./other.md)", &Translator::default());
        assert!(out.contains("#tasty-nav:link:"));
        assert!(!out.contains(r#"href="./other.md""#));
    }

    #[test]
    fn unsafe_content_html_leaves_anchor_links_untouched() {
        let out = unsafe_content_html("[jump](#section)", &Translator::default());
        assert!(out.contains(r##"href="#section""##));
    }

    #[test]
    fn wikilink_to_existing_file_becomes_plain_nav_link() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("Other.md"), "content").unwrap();
        let out = sanitize_html(&unsafe_content_html_in_dir(
            "See [[Other]] for details.",
            &Translator::default(),
            Some(dir.path()),
        ));
        assert!(out.contains("#tasty-nav:link:"), "got: {out}");
        assert!(out.contains(">Other</a>"), "got: {out}");
        assert!(
            !out.contains("tasty-wikilink-missing"),
            "existing target must not get the missing marker, got: {out}"
        );
    }

    #[test]
    fn wikilink_to_missing_file_gets_missing_marker() {
        let dir = tempfile::tempdir().unwrap();
        let out = sanitize_html(&unsafe_content_html_in_dir(
            "See [[Nonexistent]] for details.",
            &Translator::default(),
            Some(dir.path()),
        ));
        assert!(
            out.contains("tasty-wikilink-missing"),
            "missing target should get the visual marker, got: {out}"
        );
        // Still a real link (destination present, not stripped) — clicking falls through to
        // the existing nonexistent-file handling in main.rs::dispatch_file_link.
        assert!(out.contains("#tasty-nav:link:"), "got: {out}");
        assert!(out.contains(">Nonexistent</a>"), "got: {out}");
    }

    #[test]
    fn wikilink_with_no_base_dir_is_treated_as_missing() {
        // Same "unresolvable" treatment as classify_link gives an ordinary relative destination
        // with no base directory — still a link, just visually marked.
        let out = sanitize_html(&unsafe_content_html_in_dir(
            "See [[Other]] for details.",
            &Translator::default(),
            None,
        ));
        assert!(out.contains("tasty-wikilink-missing"), "got: {out}");
    }

    #[test]
    fn wikilink_custom_display_text() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("Other.md"), "content").unwrap();
        let out = unsafe_content_html_in_dir(
            "See [[Other|a different page]] for details.",
            &Translator::default(),
            Some(dir.path()),
        );
        assert!(out.contains(">a different page</a>"), "got: {out}");
        assert!(!out.contains(">Other</a>"), "got: {out}");
    }

    #[test]
    fn wikilink_inside_code_block_is_not_linked() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("Other.md"), "content").unwrap();
        let out = unsafe_content_html_in_dir(
            "```\n[[Other]]\n```\n",
            &Translator::default(),
            Some(dir.path()),
        );
        assert!(!out.contains("<a "), "got: {out}");
        assert!(out.contains("[[Other]]"), "got: {out}");
    }

    #[test]
    fn wikilink_with_path_traversal_is_left_as_literal_text() {
        let dir = tempfile::tempdir().unwrap();
        for body in ["[[../secret]]", "[[sub/dir]]", r"[[sub\dir]]"] {
            let out = unsafe_content_html_in_dir(body, &Translator::default(), Some(dir.path()));
            assert!(
                !out.contains("<a "),
                "{body} must not become a link, got: {out}"
            );
            assert!(
                out.contains(body),
                "{body} should pass through unchanged, got: {out}"
            );
        }
    }

    #[test]
    fn wikilink_does_not_collide_with_footnote_reference() {
        // Empirical confirmation (not just the structural `Event::FootnoteReference` argument in
        // resolve_wikilinks's doc comment): both syntaxes in the same document, each must resolve
        // to its own kind of markup with no cross-contamination.
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("Other.md"), "content").unwrap();
        let source = "See[^1] and [[Other]].\n\n[^1]: A note.\n";
        let out = unsafe_content_html_in_dir(source, &Translator::default(), Some(dir.path()));
        assert!(out.contains("footnote-reference"), "got: {out}");
        assert!(out.contains(r#"id="fnref-1""#), "got: {out}");
        assert!(out.contains(">Other</a>"), "got: {out}");
        assert!(
            !out.contains("[[Other]]"),
            "wikilink should have been rewritten, got: {out}"
        );
        assert!(
            !out.contains("[^1]"),
            "footnote ref should have been rewritten, got: {out}"
        );
    }

    #[test]
    fn bare_url_becomes_nav_fragment_link() {
        let out = sanitize_html(&unsafe_content_html(
            "Visit https://example.com today.",
            &Translator::default(),
        ));
        assert!(out.contains("#tasty-nav:link:"), "got: {out}");
        assert!(!out.contains(r#"href="https://example.com""#), "got: {out}");
        assert!(out.contains(">https://example.com</a>"), "got: {out}");
    }

    #[test]
    fn bare_url_inside_inline_code_is_not_linked() {
        let out = unsafe_content_html("`https://example.com` is code.", &Translator::default());
        assert!(
            out.contains("<code>https://example.com</code>"),
            "got: {out}"
        );
        assert!(!out.contains("<a "), "got: {out}");
    }

    #[test]
    fn bare_url_inside_code_block_is_not_linked() {
        let out = unsafe_content_html("```\nhttps://example.com\n```\n", &Translator::default());
        assert!(out.contains("<pre><code>https://example.com"), "got: {out}");
        assert!(!out.contains("<a "), "got: {out}");
    }

    #[test]
    fn already_explicit_link_is_not_double_linked() {
        let out = unsafe_content_html(
            "[https://example.com](https://example.com)",
            &Translator::default(),
        );
        assert_eq!(out.matches("<a ").count(), 1, "got: {out}");
        assert!(out.contains("#tasty-nav:link:"), "got: {out}");
    }

    #[test]
    fn bare_url_trailing_sentence_period_stays_outside_link() {
        let out = unsafe_content_html("Visit https://example.com. Thanks.", &Translator::default());
        assert!(out.contains(">https://example.com</a>."), "got: {out}");
    }

    #[test]
    fn bare_url_with_balanced_parens_keeps_trailing_paren() {
        let out = unsafe_content_html(
            "See https://en.wikipedia.org/wiki/Rust_(programming_language) here.",
            &Translator::default(),
        );
        assert!(
            out.contains(">https://en.wikipedia.org/wiki/Rust_(programming_language)</a>"),
            "got: {out}"
        );
    }

    #[test]
    fn bare_url_wrapped_in_sentence_parens_excludes_outer_paren() {
        let out = unsafe_content_html("See (https://example.com) here.", &Translator::default());
        assert!(out.contains(">https://example.com</a>)"), "got: {out}");
    }

    #[test]
    fn yaml_frontmatter_at_document_start_is_hidden() {
        let out = unsafe_content_html("---\nkey: value\n---\n\n# Body\n", &Translator::default());
        assert!(!out.contains("<hr"));
        assert!(!out.contains("key: value"));
        assert!(!out.contains("<h2"));
        // 이제 heading 은 자동 슬러그 `id` 를 받는다 — "heading ids + TOC" 절.
        assert!(out.contains(r#"<h1 id="body">Body</h1>"#), "got: {out}");
    }

    #[test]
    fn toml_frontmatter_at_document_start_is_hidden() {
        let out = unsafe_content_html(
            "+++\nkey = \"value\"\n+++\n\n# Body\n",
            &Translator::default(),
        );
        assert!(!out.contains("<hr"));
        assert!(!out.contains("key = "));
        assert!(out.contains(r#"<h1 id="body">Body</h1>"#), "got: {out}");
    }

    #[test]
    fn thematic_break_mid_document_still_renders_as_hr() {
        let out = unsafe_content_html("# Title\n\n---\n\nMore text\n", &Translator::default());
        assert!(out.contains("<hr"));
        assert!(out.contains(r#"<h1 id="title">Title</h1>"#), "got: {out}");
        assert!(out.contains("More text"));
    }

    #[test]
    fn smart_punctuation_converts_quotes_dashes_and_ellipsis() {
        let out = unsafe_content_html("\"hello\" and 'test'\n", &Translator::default());
        assert!(out.contains('\u{201c}'));
        assert!(out.contains('\u{201d}'));
        assert!(out.contains('\u{2018}'));
        assert!(out.contains('\u{2019}'));

        let out = unsafe_content_html("a -- b and a --- b\n", &Translator::default());
        assert!(out.contains('\u{2013}'));
        assert!(out.contains('\u{2014}'));

        let out = unsafe_content_html("wait...\n", &Translator::default());
        assert!(out.contains('\u{2026}'));
    }

    #[test]
    fn smart_punctuation_does_not_convert_inside_code() {
        let out = unsafe_content_html("`--` stays literal\n", &Translator::default());
        assert!(out.contains("<code>--</code>"));

        let out = unsafe_content_html("```\n--\n```\n", &Translator::default());
        assert!(out.contains("--\n"));
        assert!(!out.contains('\u{2013}'));
        assert!(!out.contains('\u{2014}'));
    }

    #[test]
    fn escaped_punctuation_stays_literal() {
        let out = unsafe_content_html("\\\"hello\\\"\n", &Translator::default());
        assert!(out.contains("\"hello\""));
        assert!(!out.contains('\u{201c}'));

        let out = unsafe_content_html("a \\-\\- b\n", &Translator::default());
        assert!(out.contains("a -- b"));
        assert!(!out.contains('\u{2013}'));
    }

    #[test]
    fn percent_encode_and_decode_roundtrip() {
        let raw = "a b/c#d?e&f=g";
        let enc = percent_encode_fragment(raw);
        assert_eq!(percent_decode(&enc), raw);
    }

    #[test]
    fn render_document_embeds_theme_css_and_addr_bar() {
        let theme = Theme::with_colors_and_zoom(tasty_themes::mocha_fallback_colors(), false, 1.0);
        let tr = Translator::default();
        let recent = vec!["/a/one.md".to_string(), "/b/two.md".to_string()];
        let html = render_document(DocumentInput {
            theme: &theme,
            tr: &tr,
            file_path: "/a/current.md",
            source: "# Hello\n\nSome *text*.",
            load_error: None,
            base_dir: Some(Path::new("/a")),
            recent: &recent,
        });
        assert!(html.contains("<style>"));
        assert!(html.contains("tasty-addr-bar"));
        assert!(html.contains("tasty-addr-input"));
        assert!(html.contains("/a/one.md"));
        assert!(html.contains("<base href="));
        assert!(html.contains("Hello"));
        assert!(!html.contains("<script>alert"));
    }

    #[test]
    fn nav_script_keys_scroll_restore_by_file_path_and_json_escapes_it() {
        let script = nav_script("/a/weird \"path\".md");
        assert!(script.contains("sessionStorage"));
        assert!(script.contains("tasty-md-scroll:"));
        assert!(script.contains("scrollTo"));
        // file_path is embedded via serde_json::to_string, so an embedded quote must come
        // through backslash-escaped (valid JS string literal) rather than breaking out of it.
        assert!(script.contains(r#"weird \"path\".md"#));
        assert!(!script.contains(r#"'weird "path".md'"#));
    }

    #[test]
    fn render_document_shows_error_state() {
        let theme = Theme::with_colors_and_zoom(tasty_themes::mocha_fallback_colors(), false, 1.0);
        let tr = Translator::default();
        let html = render_document(DocumentInput {
            theme: &theme,
            tr: &tr,
            file_path: "/a/missing.md",
            source: "",
            load_error: Some("No such file"),
            base_dir: None,
            recent: &[],
        });
        assert!(html.contains("No such file"));
    }

    #[test]
    fn render_document_inlines_mermaid_only_when_block_present() {
        let theme = Theme::with_colors_and_zoom(tasty_themes::mocha_fallback_colors(), false, 1.0);
        let tr = Translator::default();
        let with_mermaid = render_document(DocumentInput {
            theme: &theme,
            tr: &tr,
            file_path: "/a/diagram.md",
            source: "```mermaid\ngraph TD; A-->B;\n```\n",
            load_error: None,
            base_dir: None,
            recent: &[],
        });
        assert!(with_mermaid.contains(r#"class="language-mermaid""#));
        assert!(with_mermaid.contains("mermaid.initialize"));
        assert!(with_mermaid.contains("mermaid.run"));
        assert!(with_mermaid.contains("querySelector:'code.language-mermaid'"));
        assert!(with_mermaid.contains("suppressErrors:true"));

        let without_mermaid = render_document(DocumentInput {
            theme: &theme,
            tr: &tr,
            file_path: "/a/plain.md",
            source: "```rust\nfn main() {}\n```\n",
            load_error: None,
            base_dir: None,
            recent: &[],
        });
        assert!(without_mermaid.contains(r#"class="language-rust""#));
        assert!(!without_mermaid.contains("mermaid.initialize"));
        assert!(!without_mermaid.contains("mermaid.run"));
    }

    #[test]
    fn mermaid_script_picks_theme_from_is_light() {
        assert!(mermaid_script(true).contains("theme:'default'"));
        assert!(mermaid_script(false).contains("theme:'dark'"));
    }

    #[test]
    fn mermaid_script_js_source_has_no_premature_script_close() {
        // The vendored bundle is inlined verbatim inside an HTML <script> element — any literal
        // `</script` in it, in any case (the browser's raw-text-element end tag scan is
        // case-insensitive), would truncate the tag early and break the page.
        // `mermaid_js_source` neutralizes that; this locks the invariant in regardless of what a
        // future re-vendor introduces. The vendored file itself has zero occurrences in any
        // case, so this only proves the current bundle is clean — see
        // `escape_script_close_neutralizes_mixed_case_occurrences` for the actual logic test.
        let js = mermaid_js_source();
        // 번들이 비어 있으면 아래 부정은 무조건 통과한다 — 그 갈래를 먼저 닫는다.
        assert!(js.contains("mermaid"));
        assert!(!js.to_ascii_lowercase().contains("</script"));
    }

    #[test]
    fn escape_script_close_neutralizes_mixed_case_occurrences() {
        let input = "a</script>b</SCRIPT>c</Script>d</ScRiPt>e";
        let escaped = escape_script_close(input);
        // No case variant of `</script` survives.
        assert!(!escaped.to_ascii_lowercase().contains("</script"));
        // Original case of the tag name is preserved, only `</` becomes `<\/`.
        assert_eq!(escaped, r#"a<\/script>b<\/SCRIPT>c<\/Script>d<\/ScRiPt>e"#);
    }

    #[test]
    fn escape_script_close_leaves_unrelated_text_untouched() {
        assert_eq!(escape_script_close(""), "");
        assert_eq!(
            escape_script_close("no closing tags here"),
            "no closing tags here"
        );
        assert_eq!(escape_script_close("</scrip"), "</scrip"); // too short to match
        // Non-ASCII text around/inside the match must survive intact (UTF-8 char-boundary safety).
        assert_eq!(
            escape_script_close("한글</script>한글"),
            "한글<\\/script>한글"
        );
    }

    #[test]
    fn mermaid_script_wraps_run_in_try_catch_with_console_error_fallback() {
        let script = mermaid_script(false);
        assert!(script.contains("try{"));
        assert!(script.contains("catch(e){console.error"));
        assert!(script.contains(".catch(function(e){console.error"));
    }

    // ── syntax highlighting (highlight.js) ──────────────────────────────────

    #[test]
    fn render_document_inlines_highlight_js_only_when_code_block_present() {
        let theme = Theme::with_colors_and_zoom(tasty_themes::mocha_fallback_colors(), false, 1.0);
        let tr = Translator::default();
        let with_code = render_document(DocumentInput {
            theme: &theme,
            tr: &tr,
            file_path: "/a/snippet.md",
            source: "```rust\nfn main() {}\n```\n",
            load_error: None,
            base_dir: None,
            recent: &[],
        });
        assert!(with_code.contains(r#"class="language-rust""#));
        assert!(with_code.contains("hljs.getLanguage"));
        assert!(with_code.contains("hljs.highlightElement"));

        let without_code = render_document(DocumentInput {
            theme: &theme,
            tr: &tr,
            file_path: "/a/plain.md",
            source: "# Just prose\n\nNo fenced blocks here.",
            load_error: None,
            base_dir: None,
            recent: &[],
        });
        assert!(!without_code.contains("hljs.getLanguage"));
        assert!(!without_code.contains("hljs.highlightElement"));
    }

    #[test]
    fn highlight_js_source_has_no_premature_script_close() {
        // Same invariant as `mermaid_script_js_source_has_no_premature_script_close` — the
        // bundle is inlined verbatim inside an HTML <script> element.
        let js = highlight_js_source();
        assert!(!js.to_ascii_lowercase().contains("</script"));
    }

    #[test]
    fn highlight_script_wraps_each_block_in_try_catch_with_console_error_fallback() {
        let script = highlight_script();
        assert!(script.contains("try{"));
        assert!(script.contains("catch(e){console.error('highlight.js block failed'"));
        assert!(script.contains("catch(e){console.error('highlight.js init failed'"));
    }

    #[test]
    fn highlight_script_skips_unsupported_languages_without_erroring() {
        // `sanitize_fence_lang` lets any `[A-Za-z0-9_+-]` token through as a class (including
        // languages this vendored bundle doesn't ship, e.g. "brainfuck" isn't in the "common"
        // bundle, and "mermaid" is a diagram block, not code). The init script must check
        // `hljs.getLanguage(...)` before calling `highlightElement` so those fall back to plain,
        // unhighlighted text instead of throwing (`hljs.highlight` throws synchronously on an
        // unknown language — verified against the vendored bundle directly).
        let script = highlight_script();
        assert!(script.contains("if(!hljs.getLanguage(m[1]))return;"));
    }

    #[test]
    fn highlight_js_recognizes_every_minimum_required_language() {
        // Locks in the language-coverage claim in `assets/NOTICE.md`: every language this task
        // requires (rust/js/ts/python/json/toml/bash/yaml/markdown) is registered in the
        // vendored bundle. The bundle registers each language by stripping the `grmr_` prefix
        // off its internal grammar-function key (verified by reading the bundle's own
        // registration loop — `Ke.registerLanguage(n,Pe[e])` where `n` derives from `grmr_<id>`),
        // so `grmr_<id>:` is what `hljs.getLanguage(id)` actually resolves against — this is a
        // more precise signal than grepping for the bare language name, which also appears in
        // unrelated places (comments, the human-readable `name:"Rust"` field, etc).
        //
        // `toml` isn't checked directly: highlight.js registers it only as an *alias* of the
        // `ini` grammar (`grmr_ini`, `aliases:["toml"]`) — no `grmr_toml` key exists — so it's
        // asserted separately below via the alias list instead of this loop's naming convention.
        let js = highlight_js_source();
        for lang in [
            "rust",
            "javascript",
            "typescript",
            "python",
            "json",
            "bash",
            "yaml",
            "markdown",
        ] {
            assert!(
                js.contains(&format!("grmr_{lang}:")),
                "expected the vendored bundle to register grammar grmr_{lang}"
            );
        }
        assert!(
            js.contains("grmr_ini:") && js.contains(r#"aliases:["toml"]"#),
            "expected toml to be covered as an alias of the ini grammar"
        );
    }

    #[test]
    fn code_block_with_literal_script_tag_stays_escaped_text_after_highlight_insertion() {
        // XSS regression: a code block containing the literal text `<script>alert(1)</script>`
        // must render as HTML-escaped text — sanitize_html already guarantees this independently
        // of syntax highlighting (pulldown-cmark HTML-escapes code block content, and
        // highlight.js's own `highlightElement` reads `textContent`/rewrites `innerHTML` with its
        // own escaped spans, never reinterpreting existing markup) — this locks in that adding
        // the (conditional) highlight.js script tag doesn't change that.
        let theme = Theme::with_colors_and_zoom(tasty_themes::mocha_fallback_colors(), false, 1.0);
        let tr = Translator::default();
        let html = render_document(DocumentInput {
            theme: &theme,
            tr: &tr,
            file_path: "/a/xss.md",
            source: "```rust\n<script>alert(1)</script>\n```\n",
            load_error: None,
            base_dir: None,
            recent: &[],
        });
        assert!(html.contains("&lt;script&gt;alert(1)&lt;/script&gt;"));
        // The only `<script` occurrences in the whole document must be tasty's own trusted
        // script tags (nav_script/highlight_script) — never a literal, still-live `<script>`
        // reconstructed from the user's code block content.
        assert!(!html.contains("<script>alert(1)</script>"));
    }

    #[test]
    fn hljs_css_derives_colors_from_theme_not_a_hardcoded_palette() {
        let base_colors = tasty_themes::mocha_fallback_colors();
        let mut alt_colors = base_colors.clone();
        // Only need one hue field to actually differ to prove `hljs_css` re-derives from
        // `Theme` on every call rather than baking in a fixed palette (e.g. a vendored
        // highlight.js theme like github.css would render identically regardless of this).
        alt_colors.mauve =
            tasty_type_appearance::color::HexColor::from_hex("#ff00ff").expect("valid hex literal");
        let base = Theme::with_colors_and_zoom(base_colors, false, 1.0);
        let alt = Theme::with_colors_and_zoom(alt_colors, false, 1.0);
        let base_css = hljs_css(&base);
        let alt_css = hljs_css(&alt);
        assert!(base_css.contains(".hljs-keyword"));
        assert_ne!(
            base_css, alt_css,
            "hljs token colors should follow the active theme, not a fixed palette"
        );
        assert!(alt_css.contains("#ff00ff"));
    }

    // ── copy button ──────────────────────────────────────────────────────────

    #[test]
    fn render_document_inlines_copy_button_only_when_code_block_present() {
        let theme = Theme::with_colors_and_zoom(tasty_themes::mocha_fallback_colors(), false, 1.0);
        let tr = Translator::default();
        let with_code = render_document(DocumentInput {
            theme: &theme,
            tr: &tr,
            file_path: "/a/snippet.md",
            source: "```rust\nfn main() {}\n```\n",
            load_error: None,
            base_dir: None,
            recent: &[],
        });
        // `.tasty-copy-btn` itself is always in the document (it's a CSS rule in `theme_css`,
        // emitted unconditionally like every other selector) — the actual conditional signal is
        // whether the attach *script* (and its selector) got inlined at all.
        assert!(with_code.contains("#tasty-md-body pre > code"));

        let without_code = render_document(DocumentInput {
            theme: &theme,
            tr: &tr,
            file_path: "/a/plain.md",
            source: "# Just prose\n\nNo fenced blocks here.",
            load_error: None,
            base_dir: None,
            recent: &[],
        });
        assert!(!without_code.contains("#tasty-md-body pre > code"));
    }

    #[test]
    fn render_document_inlines_copy_button_for_unlabeled_code_block() {
        // No language token → pulldown-cmark emits `<pre><code>` with no `class` at all. The
        // gate condition (`<pre><code` substring, not `class="language-`) must still catch this
        // — `highlight`'s gate deliberately doesn't, but the copy button applies regardless of
        // whether the block has a recognized (or any) language.
        let theme = Theme::with_colors_and_zoom(tasty_themes::mocha_fallback_colors(), false, 1.0);
        let tr = Translator::default();
        let html = render_document(DocumentInput {
            theme: &theme,
            tr: &tr,
            file_path: "/a/unlabeled.md",
            source: "```\nplain text, no language\n```\n",
            load_error: None,
            base_dir: None,
            recent: &[],
        });
        assert!(html.contains("<pre><code>"));
        assert!(html.contains("#tasty-md-body pre > code"));
    }

    #[test]
    fn render_document_error_state_has_no_copy_button() {
        // `.tasty-state-detail` is a bare `<pre>` with no `<code>` child, and it lives outside
        // `#tasty-md-body` — the copy-button gate (`<pre><code` substring on `body_html`) must
        // not fire for it.
        let theme = Theme::with_colors_and_zoom(tasty_themes::mocha_fallback_colors(), false, 1.0);
        let tr = Translator::default();
        let html = render_document(DocumentInput {
            theme: &theme,
            tr: &tr,
            file_path: "/a/broken.md",
            source: "",
            load_error: Some("No such file"),
            base_dir: None,
            recent: &[],
        });
        assert!(html.contains("tasty-state-detail"));
        assert!(!html.contains("#tasty-md-body pre > code"));
    }

    #[test]
    fn copy_button_script_is_scoped_to_body_code_blocks_only() {
        let script = copy_button_script(&Translator::default());
        assert!(script.contains("#tasty-md-body pre > code"));
    }

    #[test]
    fn copy_button_script_skips_mermaid_blocks_regardless_of_script_order() {
        // mermaid.run() is async and unawaited (see `mermaid_script`'s doc comment) — script
        // *ordering* can't guarantee the diagram DOM-replacement already happened, so the skip
        // must be an explicit class check inside the attach loop, not implicit via placement.
        let script = copy_button_script(&Translator::default());
        assert!(script.contains("code.classList.contains('language-mermaid')"));
    }

    #[test]
    fn copy_button_script_reads_text_content_not_inner_html() {
        let script = copy_button_script(&Translator::default());
        assert!(script.contains("code.textContent"));
        assert!(!script.contains("code.innerHTML"));
    }

    #[test]
    fn copy_button_script_dedups_per_pre_against_repeat_attachment() {
        // Defense-in-depth against duplicate listeners: even though `reload_webview` always
        // replaces the whole document (so listeners can't literally accumulate across reloads —
        // see `render_document`'s module docs), an existing `.tasty-copy-btn` inside the same
        // `<pre>` still short-circuits the attach loop for that block.
        let script = copy_button_script(&Translator::default());
        assert!(script.contains("pre.querySelector('.tasty-copy-btn')"));
    }

    #[test]
    fn copy_button_script_has_clipboard_api_with_exec_command_fallback() {
        let script = copy_button_script(&Translator::default());
        assert!(script.contains("navigator.clipboard"));
        assert!(script.contains("navigator.clipboard.writeText"));
        assert!(script.contains("document.execCommand('copy')"));
    }

    #[test]
    fn copy_button_script_is_keyboard_focusable_with_aria_label() {
        let script = copy_button_script(&Translator::default());
        assert!(script.contains("tabindex"));
        assert!(script.contains("aria-label"));
    }

    #[test]
    fn copy_button_script_wraps_attachment_in_try_catch_with_console_error_fallback() {
        let script = copy_button_script(&Translator::default());
        assert!(script.contains("try{"));
        assert!(script.contains("catch(e){console.error"));
    }

    #[test]
    fn copy_button_css_uses_theme_tokens_not_hardcoded_colors() {
        let base_colors = tasty_themes::mocha_fallback_colors();
        let mut alt_colors = base_colors.clone();
        alt_colors.green =
            tasty_type_appearance::color::HexColor::from_hex("#00ff00").expect("valid hex literal");
        let base = Theme::with_colors_and_zoom(base_colors, false, 1.0);
        let alt = Theme::with_colors_and_zoom(alt_colors, false, 1.0);
        let base_css = theme_css(&base);
        let alt_css = theme_css(&alt);
        assert!(base_css.contains(".tasty-copy-btn"));
        assert!(base_css.contains(r#".tasty-copy-btn[data-state="copied"]"#));
        assert_ne!(
            base_css, alt_css,
            "copy button color should follow accent_success, not a fixed palette"
        );
    }

    // ── image load failure ───────────────────────────────────────────────────

    #[test]
    fn render_document_inlines_image_error_script_only_when_image_present() {
        let theme = Theme::with_colors_and_zoom(tasty_themes::mocha_fallback_colors(), false, 1.0);
        let tr = Translator::default();
        let with_image = render_document(DocumentInput {
            theme: &theme,
            tr: &tr,
            file_path: "/a/pic.md",
            source: "![alt text](missing.png)\n",
            load_error: None,
            base_dir: None,
            recent: &[],
        });
        assert!(with_image.contains("<img"));
        assert!(with_image.contains("#tasty-md-body img"));

        let without_image = render_document(DocumentInput {
            theme: &theme,
            tr: &tr,
            file_path: "/a/plain.md",
            source: "# Just prose\n\nNo images here.",
            load_error: None,
            base_dir: None,
            recent: &[],
        });
        assert!(!without_image.contains("#tasty-md-body img"));
    }

    #[test]
    fn render_document_error_state_has_no_image_error_script() {
        // Error/empty states never render an `<img>` at all, so the gate (`<img` substring on
        // `body_html`) must not fire for them.
        let theme = Theme::with_colors_and_zoom(tasty_themes::mocha_fallback_colors(), false, 1.0);
        let tr = Translator::default();
        let html = render_document(DocumentInput {
            theme: &theme,
            tr: &tr,
            file_path: "/a/broken.md",
            source: "",
            load_error: Some("No such file"),
            base_dir: None,
            recent: &[],
        });
        assert!(!html.contains("#tasty-md-body img"));
    }

    #[test]
    fn image_error_script_is_scoped_to_body_images_only() {
        let script = image_error_script(&Translator::default());
        assert!(script.contains("#tasty-md-body img"));
    }

    #[test]
    fn image_error_script_checks_both_future_and_already_failed_images() {
        // Listener attachment alone misses images whose `error` event already fired before this
        // script ran (it executes near the end of the document) — `complete && naturalWidth===0`
        // catches those retroactively.
        let script = image_error_script(&Translator::default());
        assert!(script.contains("addEventListener('error'"));
        assert!(script.contains("img.complete&&img.naturalWidth===0"));
    }

    #[test]
    fn image_error_script_reads_src_attribute_not_property() {
        // `img.src` (property) is already normalized to an absolute `file://` URI by `<base
        // href>` — `getAttribute('src')` preserves the original markdown-authored path, which is
        // what the placeholder should show.
        let script = image_error_script(&Translator::default());
        assert!(script.contains("img.getAttribute('src')"));
        assert!(!script.contains("img.src"));
    }

    #[test]
    fn image_error_script_preserves_alt_as_aria_label() {
        let script = image_error_script(&Translator::default());
        assert!(script.contains("img.getAttribute('alt')"));
        assert!(script.contains("role','img'"));
        assert!(script.contains("aria-label"));
    }

    #[test]
    fn image_error_script_guards_against_duplicate_replacement() {
        let script = image_error_script(&Translator::default());
        assert!(script.contains("img.dataset.tastyImgChecked"));
        assert!(script.contains("img.dataset.tastyImgFailed"));
    }

    #[test]
    fn image_error_script_wraps_attachment_in_try_catch_with_console_error_fallback() {
        let script = image_error_script(&Translator::default());
        assert!(script.contains("try{"));
        assert!(script.contains("catch(e){console.error"));
    }

    #[test]
    fn image_error_css_uses_theme_tokens_not_hardcoded_colors() {
        let base_colors = tasty_themes::mocha_fallback_colors();
        let mut alt_colors = base_colors.clone();
        alt_colors.red =
            tasty_type_appearance::color::HexColor::from_hex("#00ff00").expect("valid hex literal");
        let base = Theme::with_colors_and_zoom(base_colors, false, 1.0);
        let alt = Theme::with_colors_and_zoom(alt_colors, false, 1.0);
        let base_css = theme_css(&base);
        let alt_css = theme_css(&alt);
        assert!(base_css.contains(".tasty-img-error"));
        assert!(base_css.contains(".tasty-img-error-icon"));
        assert_ne!(
            base_css, alt_css,
            "image error placeholder color should follow accent_danger, not a fixed palette"
        );
    }

    // ── math (KaTeX) ─────────────────────────────────────────────────────────

    #[test]
    fn render_document_inlines_katex_only_when_math_present() {
        let theme = Theme::with_colors_and_zoom(tasty_themes::mocha_fallback_colors(), false, 1.0);
        let tr = Translator::default();
        let with_math = render_document(DocumentInput {
            theme: &theme,
            tr: &tr,
            file_path: "/a/math.md",
            source: "Einstein: $E=mc^2$\n\n$$\\sum_{i=1}^n i$$\n",
            load_error: None,
            base_dir: None,
            recent: &[],
        });
        assert!(with_math.contains(r#"class="math math-inline""#));
        assert!(with_math.contains(r#"class="math math-display""#));
        assert!(with_math.contains("katex.render"));

        let without_math = render_document(DocumentInput {
            theme: &theme,
            tr: &tr,
            file_path: "/a/plain.md",
            source: "# Just prose\n\nNo math here, and no literal dollar signs either.",
            load_error: None,
            base_dir: None,
            recent: &[],
        });
        assert!(!without_math.contains("katex.render"));
    }

    #[test]
    fn render_document_preserves_original_latex_source_in_math_span() {
        // The HTML writer HTML-escapes the LaTeX source into the span's text content — this
        // locks in that pulldown-cmark's own default `ENABLE_MATH` output survives
        // `sanitize_html` unmangled (no custom event rewrite needed for this shape).
        let theme = Theme::with_colors_and_zoom(tasty_themes::mocha_fallback_colors(), false, 1.0);
        let tr = Translator::default();
        let html = render_document(DocumentInput {
            theme: &theme,
            tr: &tr,
            file_path: "/a/math.md",
            source: "$a < b$\n",
            load_error: None,
            base_dir: None,
            recent: &[],
        });
        assert!(html.contains("a &lt; b"), "got: {html}");
    }

    #[test]
    fn render_document_error_state_has_no_katex_script() {
        let theme = Theme::with_colors_and_zoom(tasty_themes::mocha_fallback_colors(), false, 1.0);
        let tr = Translator::default();
        let html = render_document(DocumentInput {
            theme: &theme,
            tr: &tr,
            file_path: "/a/broken.md",
            source: "",
            load_error: Some("No such file"),
            base_dir: None,
            recent: &[],
        });
        assert!(!html.contains("katex.render"));
    }

    #[test]
    fn katex_script_is_scoped_to_body_math_spans_only() {
        let script = katex_script();
        assert!(script.contains("#tasty-md-body .math-inline, #tasty-md-body .math-display"));
    }

    #[test]
    fn katex_script_sets_required_security_options_explicitly() {
        // Task requirement: throwOnError/trust must be explicitly false, not left as defaults.
        let script = katex_script();
        assert!(script.contains("throwOnError:false"));
        assert!(script.contains("trust:false"));
    }

    #[test]
    fn katex_script_reads_text_content_not_inner_html() {
        let script = katex_script();
        assert!(script.contains("el.textContent"));
        assert!(!script.contains("el.innerHTML"));
    }

    #[test]
    fn katex_script_distinguishes_inline_from_display_mode() {
        let script = katex_script();
        assert!(script.contains("el.classList.contains('math-display')"));
        assert!(script.contains("displayMode:display"));
    }

    #[test]
    fn katex_script_guards_against_duplicate_rendering() {
        let script = katex_script();
        assert!(script.contains("el.dataset.tastyMathRendered"));
    }

    #[test]
    fn katex_script_wraps_render_in_try_catch_with_console_error_fallback() {
        let script = katex_script();
        assert!(script.contains("try{"));
        assert!(script.contains("catch(e){console.error"));
    }

    #[test]
    fn katex_js_source_has_no_premature_script_close() {
        let js = katex_js_source();
        assert!(!js.to_ascii_lowercase().contains("</script"));
    }

    #[test]
    fn katex_css_embeds_every_vendored_font_as_a_data_uri_with_no_leftover_relative_urls() {
        let css = katex_css_with_embedded_fonts();
        assert!(!css.contains("url(fonts/"), "leftover relative font url");
        assert_eq!(
            css.matches("data:font/woff2;base64,").count(),
            KATEX_FONTS.len()
        );
    }

    #[test]
    fn katex_css_sets_no_hardcoded_color_so_math_inherits_body_text_color() {
        // Locks in the "no extra theme wiring needed" claim in `katex_script`'s doc comment —
        // if a future re-vendor ever introduces a hardcoded `.katex{color:...}` rule, this fails
        // loudly instead of silently breaking dark/light theme following.
        let css = KATEX_CSS_RAW;
        assert!(
            !css.contains(".katex{color:") && !css.contains(".katex {color:"),
            "katex.min.css should rely on inherited `currentColor`, not set its own color"
        );
    }

    // ── callouts (GFM alerts + Obsidian extensions) ─────────────────────────

    #[test]
    fn gfm_alert_tags_render_their_respective_classes() {
        for (tag, class) in [
            ("NOTE", "markdown-alert-note"),
            ("TIP", "markdown-alert-tip"),
            ("IMPORTANT", "markdown-alert-important"),
            ("WARNING", "markdown-alert-warning"),
            ("CAUTION", "markdown-alert-caution"),
        ] {
            let source = format!("> [!{tag}]\n> body text\n");
            let out = unsafe_content_html(&source, &Translator::default());
            assert!(
                out.contains(&format!(r#"class="{class}""#)),
                "[!{tag}] should render class={class}, got: {out}"
            );
        }
    }

    #[test]
    fn gfm_alert_tag_is_case_insensitive() {
        for tag in ["[!Note]", "[!NOTE]", "[!note]", "[!nOtE]"] {
            let source = format!("> {tag}\n> body\n");
            let out = unsafe_content_html(&source, &Translator::default());
            assert!(
                out.contains(r#"class="markdown-alert-note""#),
                "{tag} should be recognized case-insensitively, got: {out}"
            );
        }
    }

    #[test]
    fn gfm_tag_with_trailing_text_becomes_an_obsidian_custom_title_callout() {
        // `scan_blockquote_tag` only recognizes the tag as a genuine GFM AST event when the rest
        // of that line is blank (pulldown-cmark `scanners.rs`) — trailing text on the same line
        // makes the parser fall back to a plain blockquote whose literal first line is that text,
        // tag brackets included. Before Obsidian-style callouts existed, that was the end of the
        // story (see git history for the old version of this test). Now `rewrite_callout_buffer`
        // parses that literal first line itself and recognizes it as Obsidian's own documented
        // "custom title, no fold marker" shape (`[!type] Title`) — this is an intentional
        // behavior change from Obsidian support, not a regression: the trailing text is real
        // Obsidian syntax, not malformed GFM syntax.
        let out = unsafe_content_html(
            "> [!NOTE] with trailing text\n> more\n",
            &Translator::default(),
        );
        assert!(out.contains(r#"class="markdown-alert-note""#), "got: {out}");
        assert!(
            out.contains(r#"data-label="with trailing text""#),
            "trailing text after the tag should become the custom title, got: {out}"
        );
        assert!(
            !out.contains("[!NOTE] with trailing text"),
            "the tag line itself must not leak into the body, got: {out}"
        );
    }

    #[test]
    fn plain_blockquote_without_alert_tag_is_unaffected() {
        // Regression: an ordinary `>` quote must render exactly as before — no class attribute
        // at all (pulldown-cmark's `Tag::BlockQuote(None)` emits an empty class_str) and no
        // `data-label`.
        let out = unsafe_content_html("> just a quote\n", &Translator::default());
        assert!(out.contains("<blockquote>"), "got: {out}");
        assert!(!out.contains("class="), "got: {out}");
        assert!(!out.contains("data-label"), "got: {out}");
    }

    #[test]
    fn raw_html_blockquote_spoofing_an_alert_class_gets_no_data_label() {
        // Adversarial regression for the vulnerability Gate4 flagged: a document author writes
        // a *raw HTML* blockquote (CommonMark HTML block type 6 — `blockquote` is one of the
        // block-level tags eligible for verbatim passthrough) carrying one of the 5 literal
        // alert classes directly, with no `[!NOTE]`-style tag anywhere. pulldown-cmark passes
        // this through byte-for-byte as `Event::Html`, entirely bypassing `Tag::BlockQuote` —
        // it must never be mistaken for a genuine alert and must never receive a `data-label`
        // (a forged label matching real translated alert text is what makes the spoof
        // convincing; without it there's no fake header text, just an inert class string).
        let source = "Intro\n\n\
<blockquote class=\"markdown-alert-note\">Spoofed trustworthy-looking note</blockquote>\n\n\
Outro\n";
        let out = unsafe_content_html(source, &Translator::default());
        assert!(
            !out.contains("data-label"),
            "raw HTML blockquote must never receive a data-label, got: {out}"
        );
        // The raw HTML itself still passes through verbatim (that's pulldown-cmark's own raw
        // HTML block behavior, unrelated to this fix) — confirms this is testing the real
        // spoofing vector and not a source the parser silently dropped.
        assert!(
            out.contains(r#"<blockquote class="markdown-alert-note">Spoofed trustworthy-looking note</blockquote>"#),
            "expected the raw HTML to survive unmodified pre-sanitize, got: {out}"
        );

        // A genuine alert elsewhere in the *same* document must still be labeled correctly —
        // proves this isn't a blanket "never label anything" regression, just a refusal to
        // label anything that didn't come from a real `Tag::BlockQuote(Some(kind))` event.
        let mixed = format!("{source}\n> [!NOTE]\n> a real one\n");
        let mixed_out = unsafe_content_html(&mixed, &Translator::default());
        assert_eq!(
            mixed_out.matches("data-label").count(),
            1,
            "exactly the genuine alert should get a data-label, got: {mixed_out}"
        );
    }

    #[test]
    fn gfm_alert_data_label_uses_translator_and_survives_sanitize() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("en.toml"),
            "[markdown.alert]\nnote = \"Custom Note Label\"\n",
        )
        .unwrap();
        let tr = Translator::load(dir.path(), "en");

        let html = unsafe_content_html("> [!NOTE]\n> body\n", &tr);
        assert!(
            html.contains(r#"data-label="Custom Note Label""#),
            "got: {html}"
        );

        // Must also survive `sanitize_html`'s allowlist (blockquote now allows data-label).
        let sanitized = sanitize_html(&html);
        assert!(
            sanitized.contains(r#"data-label="Custom Note Label""#),
            "got: {sanitized}"
        );
        assert!(sanitized.contains(r#"class="markdown-alert-note""#));
    }

    #[test]
    fn obsidian_extended_types_render_their_classes() {
        for (tag, class) in [
            ("info", "markdown-alert-info"),
            ("abstract", "markdown-alert-abstract"),
            ("todo", "markdown-alert-todo"),
            ("success", "markdown-alert-success"),
            ("question", "markdown-alert-question"),
            ("failure", "markdown-alert-failure"),
            ("danger", "markdown-alert-danger"),
            ("bug", "markdown-alert-bug"),
            ("example", "markdown-alert-example"),
            ("quote", "markdown-alert-quote"),
        ] {
            // Bare (no fold marker, no title) — pulldown-cmark's own GFM scanner never
            // recognizes these 10 (not one of its 5 fixed names), so this exercises the
            // plain-blockquote/first-line-parsing half of `rewrite_callout_buffer`.
            let source = format!("> [!{tag}]\n> body text\n");
            let out = unsafe_content_html(&source, &Translator::default());
            assert!(
                out.contains(&format!(r#"class="{class}""#)),
                "[!{tag}] should render class={class}, got: {out}"
            );
        }
    }

    #[test]
    fn obsidian_documented_aliases_resolve_to_their_canonical_kind() {
        for (alias, class) in [
            ("summary", "markdown-alert-abstract"),
            ("tldr", "markdown-alert-abstract"),
            ("hint", "markdown-alert-tip"),
            ("check", "markdown-alert-success"),
            ("done", "markdown-alert-success"),
            ("help", "markdown-alert-question"),
            ("faq", "markdown-alert-question"),
            ("fail", "markdown-alert-failure"),
            ("missing", "markdown-alert-failure"),
            ("error", "markdown-alert-danger"),
            ("cite", "markdown-alert-quote"),
        ] {
            let source = format!("> [!{alias}]\n> body\n");
            let out = unsafe_content_html(&source, &Translator::default());
            assert!(
                out.contains(&format!(r#"class="{class}""#)),
                "[!{alias}] should alias to class={class}, got: {out}"
            );
        }
    }

    #[test]
    fn obsidian_aliases_never_shadow_a_gfm_kind_of_the_same_name() {
        // Obsidian's own docs list `important`/`caution`/`attention` as aliases of
        // `tip`/`warning`/`warning` respectively — but those 3 keywords are already distinct,
        // pre-existing GFM kinds in this codebase ("GFM 5종 기존 유지" is a hard requirement), so
        // they must keep resolving to their own GFM entry, not get redefined into an alias.
        let important = unsafe_content_html("> [!important]\n> body\n", &Translator::default());
        assert!(
            important.contains(r#"class="markdown-alert-important""#),
            "got: {important}"
        );
        let caution = unsafe_content_html("> [!caution]\n> body\n", &Translator::default());
        assert!(
            caution.contains(r#"class="markdown-alert-caution""#),
            "got: {caution}"
        );
        // `attention` has no GFM entry of its own and isn't in `CALLOUT_ALIASES` either (kept
        // out deliberately, see `CALLOUT_KINDS` doc) — an unrecognized type must leave the
        // blockquote alone, literal tag text and all.
        let attention = unsafe_content_html("> [!attention]\n> body\n", &Translator::default());
        assert!(!attention.contains("markdown-alert-"), "got: {attention}");
        assert!(attention.contains("[!attention]"), "got: {attention}");
    }

    #[test]
    fn unrecognized_bracket_tag_is_left_as_a_plain_blockquote() {
        let out = unsafe_content_html("> [!not-a-real-type]\n> body\n", &Translator::default());
        assert!(!out.contains("markdown-alert-"), "got: {out}");
        assert!(!out.contains("<details"), "got: {out}");
        assert!(out.contains("[!not-a-real-type]"), "got: {out}");
    }

    #[test]
    fn fold_marker_plus_renders_initially_open_details() {
        let out = unsafe_content_html("> [!note]+\n> body\n", &Translator::default());
        assert!(
            out.contains("<details class=\"markdown-alert-note\" open>"),
            "got: {out}"
        );
        assert!(out.contains("<summary>"), "got: {out}");
        // No custom title given — falls back to the default label key (`Translator::default()`
        // has no loaded strings, so `t()`'s "key itself" miss-fallback is what surfaces here —
        // `gfm_alert_data_label_uses_translator_and_survives_sanitize` below covers the real,
        // translated label text via a loaded `Translator`).
        assert!(out.contains(">markdown.alert.note</summary>"), "got: {out}");
        assert!(out.contains("body"), "got: {out}");
    }

    #[test]
    fn fold_marker_minus_renders_initially_closed_details() {
        let out = unsafe_content_html("> [!warning]-\n> body\n", &Translator::default());
        assert!(
            out.contains("<details class=\"markdown-alert-warning\">"),
            "collapsed details must omit `open`, got: {out}"
        );
        assert!(!out.contains("markdown-alert-warning\" open"), "got: {out}");
    }

    #[test]
    fn no_fold_marker_never_uses_details_even_with_a_custom_title() {
        // Scope: "마커 없음 = 접기 UI 자체 없음" — fold and title are independent (matches
        // Obsidian's own docs), so a title with no `+`/`-` must still render as a plain
        // (non-foldable) blockquote, not `<details>`.
        let out = unsafe_content_html("> [!tip] Custom Title\n> body\n", &Translator::default());
        assert!(!out.contains("<details"), "got: {out}");
        assert!(
            out.contains(r#"class="markdown-alert-tip" data-label="Custom Title""#),
            "got: {out}"
        );
    }

    #[test]
    fn fold_marker_with_custom_title() {
        let out = unsafe_content_html(
            "> [!danger]+ Look out below\n> body\n",
            &Translator::default(),
        );
        assert!(
            out.contains("<details class=\"markdown-alert-danger\" open>"),
            "got: {out}"
        );
        assert!(out.contains(">Look out below</summary>"), "got: {out}");
    }

    #[test]
    fn nested_callouts_do_not_crash_and_keep_all_text() {
        // Scope: nested callouts don't need perfect styling, only crash/data-loss safety.
        let source = "> [!note] Outer\n\
> outer body\n\
> > [!warning]- Inner\n\
> > inner body\n";
        let out = unsafe_content_html(source, &Translator::default());
        assert!(out.contains("markdown-alert-note"), "got: {out}");
        assert!(out.contains("markdown-alert-warning"), "got: {out}");
        assert!(out.contains("outer body"), "got: {out}");
        assert!(out.contains("inner body"), "got: {out}");
        // Must also survive sanitize + a full render_document() pass without panicking.
        let _ = sanitize_html(&out);
    }

    #[test]
    fn nested_plain_blockquote_inside_a_callout_is_unaffected() {
        let source = "> [!note]\n> intro\n> > just a nested quote\n";
        let out = unsafe_content_html(source, &Translator::default());
        assert!(out.contains("markdown-alert-note"), "got: {out}");
        assert!(out.contains("just a nested quote"), "got: {out}");
    }

    #[test]
    fn alert_css_produces_distinct_rules_per_kind() {
        let theme = Theme::with_colors_and_zoom(tasty_themes::mocha_fallback_colors(), false, 1.0);
        let css = alert_css(&theme);
        let mut blocks = Vec::new();
        for kind in CALLOUT_KINDS {
            let needle = format!(".{}{{", kind.class);
            assert!(
                css.contains(&needle),
                "missing rule block for {}, got: {css}",
                kind.class
            );
            // Icon/color rule covers both the `<blockquote>` shape (`::before`) and the
            // `<details>`/`<summary>` foldable shape (`>summary::before`) in one selector.
            assert!(
                css.contains(&format!(
                    ".{cls}::before,.{cls}>summary::before{{color:",
                    cls = kind.class
                )),
                "missing ::before color rule for {}, got: {css}",
                kind.class
            );
            assert!(
                css.contains("background-image:url(\"data:image/svg+xml,"),
                "missing baked icon data URI, got: {css}"
            );
            blocks.push(needle);
        }
        // Every kind (GFM 5 + Obsidian extensions) gets its own distinct selector (no accidental
        // collision/reuse) — icon/color duplication across kinds is expected and fine (module
        // doc), only the selector itself must stay unique per class.
        let unique: std::collections::HashSet<_> = blocks.iter().collect();
        assert_eq!(unique.len(), CALLOUT_KINDS.len(), "got: {css}");
    }

    #[test]
    fn alert_icon_data_uri_bakes_requested_color() {
        let uri = alert_icon_data_uri(tasty_icons::ALERT_TRIANGLE.body, false, "#ff0000");
        assert!(uri.starts_with("data:image/svg+xml,"));
        // percent-encoded `stroke="#ff0000"` — `#` and `"` are both escaped by
        // percent_encode_fragment, so check the decoded round-trip instead of raw substrings.
        let decoded = percent_decode(&uri["data:image/svg+xml,".len()..]);
        assert!(decoded.contains("stroke=\"#ff0000\""), "got: {decoded}");
    }

    // ── heading ids + TOC ────────────────────────────────────────────────────

    #[test]
    fn headings_get_unique_ids_across_all_levels() {
        let source = "# One\n\n## Two\n\n### Three\n\n#### Four\n\n##### Five\n\n###### Six\n";
        let out = unsafe_content_html(source, &Translator::default());
        for (tag, text) in [
            ("h1", "One"),
            ("h2", "Two"),
            ("h3", "Three"),
            ("h4", "Four"),
            ("h5", "Five"),
            ("h6", "Six"),
        ] {
            let expected = format!(r#"<{tag} id="{}">{text}</{tag}>"#, text.to_lowercase());
            assert!(out.contains(&expected), "expected {expected:?}, got: {out}");
        }
    }

    #[test]
    fn duplicate_heading_text_gets_deduped_ids() {
        let source = "# Foo\n\n## Foo\n\n### Foo\n";
        let out = unsafe_content_html(source, &Translator::default());
        assert!(out.contains(r#"<h1 id="foo">Foo</h1>"#), "got: {out}");
        assert!(out.contains(r#"<h2 id="foo-1">Foo</h2>"#), "got: {out}");
        assert!(out.contains(r#"<h3 id="foo-2">Foo</h3>"#), "got: {out}");
    }

    #[test]
    fn heading_slug_strips_markup_and_uses_plain_text_only() {
        let source = "# Hello `code` and [a link](x.md) and **bold**\n";
        let out = unsafe_content_html(source, &Translator::default());
        assert!(
            out.contains(r#"id="hello-code-and-a-link-and-bold""#),
            "got: {out}"
        );
    }

    #[test]
    fn non_ascii_heading_gets_a_reasonable_slug() {
        let source = "# 한글 제목입니다\n";
        let out = unsafe_content_html(source, &Translator::default());
        assert!(
            out.contains(r#"<h1 id="한글-제목입니다">한글 제목입니다</h1>"#),
            "got: {out}"
        );
    }

    #[test]
    fn all_punctuation_heading_falls_back_to_default_slug() {
        let source = "# !!! ??? ...\n";
        let out = unsafe_content_html(source, &Translator::default());
        assert!(out.contains(r#"id="heading""#), "got: {out}");
    }

    #[test]
    fn heading_id_survives_sanitize() {
        let html = unsafe_content_html("# Title\n", &Translator::default());
        let sanitized = sanitize_html(&html);
        assert!(sanitized.contains(r#"id="title""#), "got: {sanitized}");
    }

    #[test]
    fn render_document_omits_toc_when_no_headings() {
        let theme = Theme::with_colors_and_zoom(tasty_themes::mocha_fallback_colors(), false, 1.0);
        let tr = Translator::default();
        let html = render_document(DocumentInput {
            theme: &theme,
            tr: &tr,
            file_path: "/a/plain.md",
            source: "Just a paragraph, no headings.\n",
            load_error: None,
            base_dir: None,
            recent: &[],
        });
        // `#tasty-toc{...}` still appears in the static `<style>` block regardless of
        // headings (theme_css isn't conditional) — assert on the actual `<nav>` element, not
        // the bare "tasty-toc" substring, or this would spuriously pass/fail on CSS presence.
        assert!(!html.contains(r#"<nav id="tasty-toc""#), "got: {html}");
    }

    #[test]
    fn render_document_includes_toc_with_matching_anchors_when_headings_present() {
        let theme = Theme::with_colors_and_zoom(tasty_themes::mocha_fallback_colors(), false, 1.0);
        let tr = Translator::default();
        let html = render_document(DocumentInput {
            theme: &theme,
            tr: &tr,
            file_path: "/a/doc.md",
            source: "# Intro\n\nSome text.\n\n## Details\n\nMore text.\n",
            load_error: None,
            base_dir: None,
            recent: &[],
        });
        assert!(html.contains(r#"id="tasty-toc""#), "got: {html}");
        assert!(html.contains(r##"href="#intro""##), "got: {html}");
        assert!(html.contains(r##"href="#details""##), "got: {html}");
        assert!(html.contains(r#"<h1 id="intro">Intro</h1>"#), "got: {html}");
        assert!(
            html.contains(r#"<h2 id="details">Details</h2>"#),
            "got: {html}"
        );
        // toggle button + collapsed-state hook present (collapsibility, task requirement).
        assert!(html.contains(r#"id="tasty-toc-toggle""#), "got: {html}");
        assert!(html.contains("tasty-toc-collapsed"), "got: {html}");
    }

    #[test]
    fn toc_is_placed_between_addr_bar_and_body() {
        let theme = Theme::with_colors_and_zoom(tasty_themes::mocha_fallback_colors(), false, 1.0);
        let tr = Translator::default();
        let html = render_document(DocumentInput {
            theme: &theme,
            tr: &tr,
            file_path: "/a/doc.md",
            source: "# Intro\n\nSome text.\n",
            load_error: None,
            base_dir: None,
            recent: &[],
        });
        // Match the actual elements, not the bare id substrings — those also appear earlier,
        // in the static `<style>` block's `#tasty-addr-bar{...}`/`#tasty-md-body{...}`/
        // `#tasty-toc{...}` selectors, which would corrupt the ordering check.
        let addr_idx = html
            .find(r#"<div id="tasty-addr-bar""#)
            .expect("addr bar present");
        let toc_idx = html.find(r#"<nav id="tasty-toc""#).expect("toc present");
        let body_idx = html
            .find(r#"<div id="tasty-md-body""#)
            .expect("body present");
        assert!(
            addr_idx < toc_idx && toc_idx < body_idx,
            "expected addr bar < toc < body, got: {html}"
        );
    }

    // ── find-in-page (find bar + TreeWalker highlight script) ──────────────────

    #[test]
    fn render_document_always_embeds_find_bar_hidden_by_default() {
        let theme = Theme::with_colors_and_zoom(tasty_themes::mocha_fallback_colors(), false, 1.0);
        let tr = Translator::default();
        let html = render_document(DocumentInput {
            theme: &theme,
            tr: &tr,
            file_path: "/a/doc.md",
            source: "Just a plain paragraph, no headings.",
            load_error: None,
            base_dir: None,
            recent: &[],
        });
        assert!(html.contains(r#"id="tasty-find-bar""#), "got: {html}");
        // hidden by default — the bar only appears on Ctrl+F, never on load.
        assert!(
            html.contains(r#"<div id="tasty-find-bar" role="search" hidden>"#),
            "got: {html}"
        );
        assert!(html.contains(r#"id="tasty-find-input""#), "got: {html}");
        assert!(html.contains(r#"id="tasty-find-count""#), "got: {html}");
        assert!(html.contains(r#"id="tasty-find-prev""#), "got: {html}");
        assert!(html.contains(r#"id="tasty-find-next""#), "got: {html}");
        assert!(html.contains(r#"id="tasty-find-close""#), "got: {html}");
    }

    #[test]
    fn find_bar_html_uses_translator_for_placeholder_and_tooltips() {
        let tr = Translator::default();
        let html = find_bar_html(&tr);
        assert!(
            html.contains(tr.t("markdown.find.placeholder")),
            "got: {html}"
        );
        assert!(
            html.contains(tr.t("markdown.find.prev_tooltip")),
            "got: {html}"
        );
        assert!(
            html.contains(tr.t("markdown.find.next_tooltip")),
            "got: {html}"
        );
        assert!(
            html.contains(tr.t("markdown.find.close_tooltip")),
            "got: {html}"
        );
    }

    #[test]
    fn find_script_excludes_code_blocks_from_the_tree_walker_scan() {
        let script = find_in_page_script(&Translator::default());
        assert!(
            script.contains("tagName==='PRE'||el.tagName==='CODE'"),
            "got: {script}"
        );
    }

    #[test]
    fn find_script_restores_dom_before_every_search_not_only_on_close() {
        let script = find_in_page_script(&Translator::default());
        // clearHighlights() unwraps every <mark> back to plain text + normalize()s the parent —
        // called at the top of runSearch() (every keystroke) as well as from closeBar().
        assert!(
            script.contains("function clearHighlights()"),
            "got: {script}"
        );
        assert!(script.contains(".normalize()"), "got: {script}");
        assert!(
            script.contains("function runSearch(){\nclearHighlights();"),
            "got: {script}"
        );
        assert!(script.contains("function closeBar(){"), "got: {script}");
    }

    #[test]
    fn find_script_skips_search_while_ime_composing() {
        let script = find_in_page_script(&Translator::default());
        assert!(script.contains("compositionstart"), "got: {script}");
        assert!(script.contains("compositionend"), "got: {script}");
        assert!(script.contains("if(composing)return;"), "got: {script}");
    }

    #[test]
    fn find_script_handles_escape_enter_and_shift_enter_on_the_input() {
        let script = find_in_page_script(&Translator::default());
        assert!(script.contains("e.key==='Escape'"), "got: {script}");
        assert!(script.contains("e.key==='Enter'"), "got: {script}");
        assert!(
            script.contains("if(e.shiftKey)prev();else next();"),
            "got: {script}"
        );
    }

    #[test]
    fn find_script_debounces_input_before_searching() {
        let script = find_in_page_script(&Translator::default());
        assert!(
            script.contains("setTimeout(runSearch,150)"),
            "got: {script}"
        );
    }

    #[test]
    fn find_script_opens_on_ctrl_or_meta_f_and_prevents_default() {
        let script = find_in_page_script(&Translator::default());
        assert!(
            script.contains("(e.ctrlKey||e.metaKey)&&!e.altKey&&(e.key==='f'||e.key==='F')"),
            "got: {script}"
        );
        assert!(
            script.contains("e.preventDefault();\nopenBar();"),
            "got: {script}"
        );
    }

    #[test]
    fn find_script_escapes_query_as_a_regex_literal() {
        // Query is only ever used as a case-insensitive literal substring match — RegExp is
        // reused as the multi-match engine, not exposed as a user-facing regex feature.
        let script = find_in_page_script(&Translator::default());
        assert!(script.contains("function escapeRegExp(s)"), "got: {script}");
        assert!(
            script.contains("new RegExp(escapeRegExp(query),'gi')"),
            "got: {script}"
        );
    }

    #[test]
    fn theme_css_derives_find_highlight_colors_from_theme_not_hardcoded() {
        let theme = Theme::with_colors_and_zoom(tasty_themes::mocha_fallback_colors(), false, 1.0);
        let css = theme_css(&theme);
        assert!(css.contains("mark.tasty-find-hit{"), "got: {css}");
        assert!(
            css.contains(&format!(
                "background:{};",
                theme
                    .accent_warning()
                    .with_alpha(FIND_HIT_BG_ALPHA)
                    .to_hex()
            )),
            "got: {css}"
        );
        assert!(
            css.contains(&format!("background:{};", theme.accent_primary().to_hex())),
            "got: {css}"
        );
    }
}
