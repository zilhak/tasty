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
            sanitize_html(&unsafe_content_html(source, tr)),
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

    format!(
        r#"<!doctype html><html><head><meta charset="utf-8">{base_tag}<style>{css}</style></head><body>{addr_bar}{toc_html}<div id="tasty-md-body">{body_html}</div><script>{script}</script>{highlight}{mermaid}{copy_buttons}</body></html>"#,
        base_tag = base_tag,
        css = theme_css(theme),
        addr_bar = addr_bar_html(tr, file_path, recent),
        toc_html = toc_html,
        body_html = body_html,
        script = nav_script(file_path),
        highlight = highlight,
        mermaid = mermaid,
        copy_buttons = copy_buttons,
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
/// exist for paragraphs it declines to touch. [`autolink_bare_urls`] itself runs *before*
/// [`rewrite_link_event`], and synthesizes its new `Tag::Link` events with a plain raw `dest_url`
/// (e.g. `https://example.com`) — the exact same shape an explicit `[text](url)` link has at this
/// point in the pipeline. This way `rewrite_link_event`, run last over the *whole* (now
/// autolink-expanded) event stream, is the single place that ever produces the `#tasty-nav:`
/// fragment scheme; the autolink pass doesn't need its own copy of that rewrite.
/// `rewrite_code_block_event`/`rewrite_alert_blockquote_event`/`rewrite_footnote_event` run first
/// since none of them touch `Text`/`Link`/`Image` events, so their relative order doesn't matter.
/// [`assign_heading_ids`] runs last over the fully-rewritten stream — none of the other passes
/// touch `Tag::Heading`, so its position doesn't matter either.
fn unsafe_content_html(source: &str, tr: &Translator) -> String {
    let headings = collect_headings(source);
    let footnote_ref_totals = footnote_reference_totals(source);
    let mut footnote_state = FootnoteState::default();
    let events: Vec<Event> = Parser::new_ext(source, parser_options())
        .map(rewrite_code_block_event)
        .map(|event| rewrite_alert_blockquote_event(event, tr))
        .map(|event| rewrite_footnote_event(event, tr, &footnote_ref_totals, &mut footnote_state))
        .collect();
    let events = figurize_solo_image_paragraphs(events);
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
    // plugin-authored raw markup" escape hatch (mirrors `rewrite_alert_blockquote_event`'s
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

// ── GFM alert blockquotes (`> [!NOTE]` etc) ────────────────────────────────────

/// One of the 5 GitHub-style alert blockquote kinds. With [`Options::ENABLE_GFM`] on,
/// pulldown-cmark's parser recognizes `> [!NOTE]`/`[!TIP]`/`[!IMPORTANT]`/`[!WARNING]`/
/// `[!CAUTION]` (first line of the blockquote, case-insensitive, nothing else on that line —
/// `scanners.rs::scan_blockquote_tag`) and exposes it as an AST event,
/// `Event::Start(Tag::BlockQuote(Some(kind)))`. Its own `html.rs` writer would turn that into
/// `<blockquote class="markdown-alert-<kind>">` and nothing else — no icon, no label string
/// (verified by reading `html.rs`'s `Tag::BlockQuote` match arm) — but [`rewrite_alert_blockquote_event`]
/// intercepts the event before that writer ever runs, so the writer's version of this tag never
/// actually gets produced. This table pairs each kind with an i18n label key
/// ([`rewrite_alert_blockquote_event`]) and a [`tasty_icons`] glyph + `Theme` accent color
/// ([`alert_css`]).
struct AlertKind {
    /// The pulldown-cmark AST kind this entry answers to — matched against the real
    /// `Tag::BlockQuote(Some(kind))` event, never against rendered HTML text (see
    /// [`rewrite_alert_blockquote_event`] doc for why that distinction matters).
    kind: BlockQuoteKind,
    /// The literal class this kind renders as (mirrors pulldown-cmark's own `html.rs` naming,
    /// kept identical so existing CSS/snapshots don't need to change).
    class: &'static str,
    /// `Translator` key for the header label — must exist in `lang/{en,ko,ja}.toml`.
    label_key: &'static str,
    /// Inner markup of a [`tasty_icons`] glyph (`Icon::body` — no wrapping `<svg>`, no color
    /// baked in). Fed to [`alert_icon_data_uri`].
    icon_body: &'static str,
    /// The glyph's own `Icon::filled` — `true` colors it via `fill`, `false` via `stroke`
    /// (mirrors how `tasty_icons`' `stroke_icon!`/`fill_icon!` macros built it).
    icon_filled: bool,
    /// This kind's `Theme` accent accessor — no dedicated alert design token exists yet, so
    /// this reuses the closest existing semantic accent (see module-level design note in
    /// [`alert_css`]).
    accent: fn(&Theme) -> tasty_type_appearance::color::HexColor,
}

/// note=info circle/blue, tip=filled star/green, important=bell/mauve, warning=alert
/// triangle/yellow, caution=close-x/red. Adding a 6th kind (if pulldown-cmark ever grows one)
/// means adding one entry here — [`rewrite_alert_blockquote_event`]/[`alert_css`] both just
/// iterate it.
const ALERT_KINDS: &[AlertKind] = &[
    AlertKind {
        kind: BlockQuoteKind::Note,
        class: "markdown-alert-note",
        label_key: "markdown.alert.note",
        icon_body: tasty_icons::ALERT_CIRCLE.body,
        icon_filled: tasty_icons::ALERT_CIRCLE.filled,
        accent: Theme::accent_primary,
    },
    AlertKind {
        kind: BlockQuoteKind::Tip,
        class: "markdown-alert-tip",
        label_key: "markdown.alert.tip",
        icon_body: tasty_icons::STAR_FILL.body,
        icon_filled: tasty_icons::STAR_FILL.filled,
        accent: Theme::accent_success,
    },
    AlertKind {
        kind: BlockQuoteKind::Important,
        class: "markdown-alert-important",
        label_key: "markdown.alert.important",
        icon_body: tasty_icons::BELL.body,
        icon_filled: tasty_icons::BELL.filled,
        accent: Theme::accent_agent,
    },
    AlertKind {
        kind: BlockQuoteKind::Warning,
        class: "markdown-alert-warning",
        label_key: "markdown.alert.warning",
        icon_body: tasty_icons::ALERT_TRIANGLE.body,
        icon_filled: tasty_icons::ALERT_TRIANGLE.filled,
        accent: Theme::accent_warning,
    },
    AlertKind {
        kind: BlockQuoteKind::Caution,
        class: "markdown-alert-caution",
        label_key: "markdown.alert.caution",
        icon_body: tasty_icons::CLOSE.body,
        icon_filled: tasty_icons::CLOSE.filled,
        accent: Theme::accent_danger,
    },
];

/// Rewrites a genuine `Event::Start(Tag::BlockQuote(Some(kind)))` — the AST event
/// pulldown-cmark's *parser* (not its HTML writer) emits only when `scan_blockquote_tag`
/// actually recognized a `[!NOTE]`-style tag in the source — directly into the fully-labeled
/// opening tag, bypassing the library's own class-only writer entirely.
///
/// This intentionally does *not* run as a post-pass over the assembled HTML string (that was
/// the previous, vulnerable design: a plain blockquote — `Tag::BlockQuote(None)` — is left
/// completely alone, but any *other* Markdown construct that can produce literal
/// `<blockquote class="markdown-alert-note">` text in the output — namely a raw HTML block/inline,
/// which pulldown-cmark passes through byte-for-byte as `Event::Html`/`Event::InlineHtml`,
/// entirely independent of `Tag::BlockQuote` — would have produced an indistinguishable string
/// for a naive whole-document string search to match, letting a document author forge a
/// trusted-looking Note/Tip/Warning header around arbitrary content. Matching on the AST event
/// itself instead of the rendered string closes that gap: raw HTML never surfaces as this event
/// no matter what text it contains, so it can never receive a `data-label`.
///
/// CSS can't branch on the UI language ([`theme_css`]'s `content: attr(data-label)` just echoes
/// whatever lands in the DOM), so the localized text has to be resolved here, at document
/// generation time — [`sanitize_html`]'s `blockquote` allowlist must include `data-label` or
/// this is stripped afterward regardless of origin.
fn rewrite_alert_blockquote_event<'a>(event: Event<'a>, tr: &Translator) -> Event<'a> {
    match event {
        Event::Start(Tag::BlockQuote(Some(kind))) => {
            let Some(alert) = ALERT_KINDS.iter().find(|a| a.kind == kind) else {
                // All 5 `BlockQuoteKind` variants are covered above; unreachable in practice,
                // but fall back to the library's own (label-less) class-only rendering rather
                // than panicking if pulldown-cmark ever adds a 6th kind before this table does.
                return Event::Start(Tag::BlockQuote(Some(kind)));
            };
            Event::Html(
                format!(
                    r#"<blockquote class="{}" data-label="{}">"#,
                    alert.class,
                    attr_escape(tr.t(alert.label_key))
                )
                .into(),
            )
        }
        other => other,
    }
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
/// [`rewrite_alert_blockquote_event`] — chosen for the same reason: matching against fully
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
/// `blockquote` (one of [`ALERT_KINDS`]' 5 fixed literal `markdown-alert-<kind>` classes).
/// ammonia does not validate `class` *values* — a raw HTML block/inline in the source (passed
/// through byte-for-byte by pulldown-cmark, independent of `Tag::BlockQuote`) can already carry
/// any of these class strings verbatim, so a document author *can* make an arbitrary blockquote
/// pick up the alert CSS's background/border/icon purely via `class`, same residual risk the
/// `code`/`sup`/`div` allowances already accept — none of these carry executable content, so
/// that's fine here. What the sanitizer's `class` allowlist does *not* by itself make possible
/// is a forged `data-label` matching one of the real translated alert headers: that attribute is
/// only ever set by [`rewrite_alert_blockquote_event`] from a genuine
/// `Tag::BlockQuote(Some(kind))` AST event, never by matching rendered HTML text, so raw-HTML
/// blockquotes always reach this allowlist with `data-label` absent (see that function's doc for
/// why the distinction matters). `data-label`'s value is `attr_escape`d before injection, same
/// escaping every other attribute value in this module gets.
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
    // alert blockquote — 고정 literal class(markdown-alert-<kind>) + rewrite_alert_blockquote_event 가
    // 진짜 AST 이벤트에서만 심는 localized data-label(attr_escape 済み).
    tag_attributes.insert("blockquote", ["class", "data-label"].into_iter().collect());

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
{alert_rules}
{hljs_rules}
hr{{border:none;border-top:var(--md-border-w) solid var(--md-rule);margin:var(--md-space-md) 0;}}
img{{max-width:100%;}}
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
        alert_rules = alert_css(theme),
        hljs_rules = hljs_css(theme),
    )
}

/// Per-[`AlertKind`] CSS: border/background from its accent color, plus the icon half of the
/// shared `::before` header rule declared alongside `blockquote[class^="markdown-alert-"]`
/// above (the label-text half — `content: attr(data-label)` — is already covered there since
/// it doesn't vary per kind). No dedicated "alert" design token exists yet, so background is
/// derived the same way `drop_overlay.rs`/`Theme::preset_split_zone_bg` already do: the same
/// accent color at low alpha, not a separate token.
fn alert_css(theme: &Theme) -> String {
    /// ~12% opacity — same ratio `drop_overlay.rs` uses for `accent_primary().with_alpha(31)`.
    const BG_ALPHA: u8 = 31;
    let mut rules = String::new();
    for kind in ALERT_KINDS {
        let color = (kind.accent)(theme);
        let icon_uri = alert_icon_data_uri(kind.icon_body, kind.icon_filled, &color.to_hex());
        rules.push_str(&format!(
            ".{class}{{border-left-color:{hex};background:{bg};}}.{class}::before{{color:{hex};background-image:url(\"{icon_uri}\");}}\n",
            class = kind.class,
            hex = color.to_hex(),
            bg = color.with_alpha(BG_ALPHA).to_hex(),
        ));
    }
    rules
}

/// Bakes [`AlertKind::icon_body`] into a complete, `color_hex`-colored `<svg>` and encodes it as
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
        deletion_bg = theme.accent_danger().with_alpha(31).to_hex(),
        addition_bg = theme.accent_success().with_alpha(31).to_hex(),
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
        assert!(!out.contains("onerror"));
        assert!(!out.contains("alert"));
    }

    #[test]
    fn sanitize_html_strips_javascript_scheme_href() {
        let out = sanitize_html(r#"<a href="javascript:alert(1)">click</a>"#);
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

    // ── GFM alert blockquotes ────────────────────────────────────────────────

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
    fn gfm_alert_tag_requires_nothing_else_on_its_line() {
        // `scan_blockquote_tag` only recognizes the tag when the rest of that line is blank
        // (pulldown-cmark `scanners.rs`) — trailing text on the same line falls back to a plain
        // blockquote whose literal first line is that text, tag brackets included.
        let out = unsafe_content_html(
            "> [!NOTE] with trailing text\n> more\n",
            &Translator::default(),
        );
        assert!(
            !out.contains("markdown-alert-note"),
            "trailing text on the tag line must suppress alert recognition, got: {out}"
        );
        assert!(out.contains("[!NOTE] with trailing text"), "got: {out}");
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
    fn alert_css_produces_distinct_rules_per_kind() {
        let theme = Theme::with_colors_and_zoom(tasty_themes::mocha_fallback_colors(), false, 1.0);
        let css = alert_css(&theme);
        let mut blocks = Vec::new();
        for kind in ALERT_KINDS {
            let needle = format!(".{}{{", kind.class);
            assert!(
                css.contains(&needle),
                "missing rule block for {}, got: {css}",
                kind.class
            );
            assert!(
                css.contains(&format!(".{}::before{{color:", kind.class)),
                "missing ::before color rule for {}, got: {css}",
                kind.class
            );
            assert!(
                css.contains("background-image:url(\"data:image/svg+xml,"),
                "missing baked icon data URI, got: {css}"
            );
            blocks.push(needle);
        }
        // 5 distinct kinds → 5 distinct selectors (no accidental collision/reuse).
        let unique: std::collections::HashSet<_> = blocks.iter().collect();
        assert_eq!(unique.len(), ALERT_KINDS.len(), "got: {css}");
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
}
