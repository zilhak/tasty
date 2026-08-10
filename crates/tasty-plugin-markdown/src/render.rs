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

use std::path::{Path, PathBuf};

use pulldown_cmark::{BlockQuoteKind, CodeBlockKind, Event, Options, Parser, Tag};
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

    let body_html = if let Some(err) = load_error {
        format!(
            r#"<div class="tasty-state tasty-state-error"><div class="tasty-state-title">{}</div><pre class="tasty-state-detail">{}</pre></div>"#,
            html_escape(tr.t("markdown.state.failed")),
            html_escape(err)
        )
    } else if source.trim().is_empty() {
        format!(
            r#"<div class="tasty-state">{}</div>"#,
            html_escape(tr.t("markdown.state.empty"))
        )
    } else {
        sanitize_html(&unsafe_content_html(source, tr))
    };

    // 3.5MB 번들이라 mermaid 블록이 실제로 있는 문서에서만 삽입한다 — 대다수 문서는
    // mermaid 를 쓰지 않으므로 매번 inline 하면 순수 낭비다.
    let mermaid = if body_html.contains("language-mermaid") {
        mermaid_script(theme.is_light)
    } else {
        String::new()
    };

    format!(
        r#"<!doctype html><html><head><meta charset="utf-8">{base_tag}<style>{css}</style></head><body>{addr_bar}<div id="tasty-md-body">{body_html}</div><script>{script}</script>{mermaid}</body></html>"#,
        base_tag = base_tag,
        css = theme_css(theme),
        addr_bar = addr_bar_html(tr, file_path, recent),
        body_html = body_html,
        script = nav_script(file_path),
        mermaid = mermaid,
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

// ── HTML generation ───────────────────────────────────────────────────────────

/// Parse `source` and generate (unsanitized) HTML, rewriting every link destination to the
/// internal nav-fragment scheme first (module doc — never a plain `href` to a local/external
/// target). Images are left untouched; the `<base href>` tag resolves relative `src`.
fn unsafe_content_html(source: &str, tr: &Translator) -> String {
    let events = Parser::new_ext(source, parser_options())
        .map(rewrite_link_event)
        .map(rewrite_code_block_event)
        .map(|event| rewrite_alert_blockquote_event(event, tr));
    let mut html = String::new();
    pulldown_cmark::html::push_html(&mut html, events);
    html
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
    ]
    .into_iter()
    .collect();

    let mut tag_attributes: HashMap<&str, HashSet<&str>> = HashMap::new();
    tag_attributes.insert("a", ["href", "title", "id"].into_iter().collect());
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
h1,h2,h3,h4,h5,h6{{color:var(--md-strong);font-weight:600;margin:1em 0 0.5em;}}
h1{{font-size:var(--md-h1);}}h2{{font-size:var(--md-h2);}}h3{{font-size:var(--md-h3);}}
h4{{font-size:var(--md-h4);}}h5{{font-size:var(--md-h5);}}h6{{font-size:var(--md-h6);}}
a{{color:var(--md-link);}}
strong{{color:var(--md-strong);font-weight:600;}}
code{{background:var(--md-code-bg);border-radius:var(--md-radius);padding:0.1em 0.35em;font-family:ui-monospace,SFMono-Regular,Menlo,Consolas,monospace;}}
pre{{background:var(--md-code-bg);border:var(--md-border-w) solid var(--md-code-border);border-radius:var(--md-radius);padding:var(--md-space-sm);overflow:auto;}}
pre code{{background:none;padding:0;}}
table{{border-collapse:collapse;}}
th,td{{border:var(--md-border-w) solid var(--md-border);padding:var(--md-space-xs) var(--md-space-sm);text-align:left;}}
tr:nth-child(even){{background:var(--md-zebra);}}
blockquote{{border-left:calc(var(--md-border-w) * 3) solid var(--md-quote-bar);margin:0.5em 0;padding:0.1em var(--md-space-md);opacity:0.9;}}
blockquote[class^="markdown-alert-"]{{opacity:1;border-radius:var(--md-radius);padding:var(--md-space-sm) var(--md-space-md);}}
blockquote[class^="markdown-alert-"]::before{{content:attr(data-label);display:block;font-weight:600;margin-bottom:var(--md-space-xs);padding-left:22px;background-repeat:no-repeat;background-position:left center;background-size:16px 16px;}}
{alert_rules}
hr{{border:none;border-top:var(--md-border-w) solid var(--md-rule);margin:var(--md-space-md) 0;}}
img{{max-width:100%;}}
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
        alert_rules = alert_css(theme),
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

    #[test]
    fn footnote_markup_survives_sanitize() {
        let source = "See[^1].\n\n[^1]: A note.\n";
        let out = sanitize_html(&unsafe_content_html(source, &Translator::default()));
        assert!(out.contains("footnote-reference"), "got: {out}");
        assert!(out.contains("footnote-definition"), "got: {out}");
        assert!(out.contains("A note."), "got: {out}");
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
    fn yaml_frontmatter_at_document_start_is_hidden() {
        let out = unsafe_content_html("---\nkey: value\n---\n\n# Body\n", &Translator::default());
        assert!(!out.contains("<hr"));
        assert!(!out.contains("key: value"));
        assert!(!out.contains("<h2"));
        assert!(out.contains("<h1>Body</h1>"));
    }

    #[test]
    fn toml_frontmatter_at_document_start_is_hidden() {
        let out = unsafe_content_html(
            "+++\nkey = \"value\"\n+++\n\n# Body\n",
            &Translator::default(),
        );
        assert!(!out.contains("<hr"));
        assert!(!out.contains("key = "));
        assert!(out.contains("<h1>Body</h1>"));
    }

    #[test]
    fn thematic_break_mid_document_still_renders_as_hr() {
        let out = unsafe_content_html("# Title\n\n---\n\nMore text\n", &Translator::default());
        assert!(out.contains("<hr"));
        assert!(out.contains("<h1>Title</h1>"));
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
}
