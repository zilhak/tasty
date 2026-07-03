//! Custom Markdown renderer (CommonMark + GFM subset) drawn directly with egui, run
//! **inside the plugin process** and composited by the host over the egui-mesh channel
//! (ADR-0028 / B1). Ported from the host's former `MarkdownView` renderer so the design's
//! six-level prose heading hierarchy and `line_height_prose` body leading come straight
//! from the host `Theme` tokens (delivered each frame via `set_context`).
//!
//! Differences from the host renderer (B1 scope, see plugin docs):
//! - **fonts**: the body family is egui's `Proportional` (CJK fallback installed on the
//!   plugin's `Context`); the host's per-surface custom markdown font family is deferred.
//! - **inline images**: rendered as their alt/destination text — bitmap texture loading is
//!   deferred to a follow-up (ADR scope 1 = pure widgets first).

use std::path::{Path, PathBuf};

use egui::text::LayoutJob;
use egui::{Align, Color32, FontFamily, FontId, Sense, Stroke, TextFormat};
use pulldown_cmark::{Alignment, Event, HeadingLevel, Options, Parser, Tag, TagEnd};
use tasty_type_appearance::theme::Theme;

/// Precomputed visual parameters resolved once from the active `Theme` + body font size.
/// Cloned (cheaply) to vary the text color inside blockquotes without re-reading the theme.
#[derive(Clone)]
pub struct MdStyle {
    family: FontFamily,
    body: f32,
    /// prose-h1 heading size (cap-exempt content level).
    font_size_prose_h1: f32,
    /// prose-h2 heading size (= the 14px font cap; shared by h2/h3).
    font_size_prose_h2: f32,
    line_height_prose: f32,
    /// Current body text color for the active block context (muted inside blockquotes).
    text: Color32,
    primary: Color32,
    muted: Color32,
    link: Color32,
    code_bg: Color32,
    code_fg: Color32,
    surface_raised: Color32,
    separator: Color32,
    blockquote_bar: Color32,
    /// markdown 인라인 표 (grid + zebra) 토큰 — 모두 `theme.md_table_*()` 접근자에서 옴.
    table_border: Color32,
    table_header_bg: Color32,
    table_header_fg: Color32,
    table_row_bg: Color32,
    table_row_bg_zebra: Color32,
    table_cell_fg: Color32,
    table_pad_x: f32,
    table_pad_y: f32,
    corner_radius: f32,
    border_width: f32,
    space_xs: f32,
    space_sm: f32,
    space_md: f32,
    space_lg: f32,
    /// The markdown file's directory, used to resolve relative image/link paths.
    base_dir: Option<PathBuf>,
    /// egui temp-memory slot a clicked link writes its [`LinkClick`] into, read back after
    /// `render`. Per-surface (derived from the surface id) so concurrent panels never collide.
    link_slot: egui::Id,
}

impl MdStyle {
    pub fn new(
        theme: &Theme,
        body_px: f32,
        base_dir: Option<PathBuf>,
        link_slot: egui::Id,
    ) -> Self {
        Self {
            // B1: 커스텀 markdown 폰트-패밀리 parity 는 후속. CJK fallback 은 plugin 이
            // Context 폰트에 설치하므로 Proportional 로도 한글/일문이 tofu 되지 않는다.
            family: FontFamily::Proportional,
            body: body_px.max(1.0),
            font_size_prose_h1: theme.font_size_prose_h1.value(),
            font_size_prose_h2: theme.font_size_prose_h2.value(),
            line_height_prose: theme.line_height_prose,
            text: theme.text_secondary().to_egui(),
            primary: theme.text_primary().to_egui(),
            muted: theme.text_muted().to_egui(),
            link: theme.accent_primary().to_egui(),
            code_bg: theme.surface_raised().to_egui(),
            code_fg: theme.text_primary().to_egui(),
            surface_raised: theme.surface_raised().to_egui(),
            separator: theme.separator.to_egui(),
            blockquote_bar: theme.border_strong().to_egui(),
            table_border: theme.md_table_border().to_egui(),
            table_header_bg: theme.md_table_header_bg().to_egui(),
            table_header_fg: theme.md_table_header_fg().to_egui(),
            table_row_bg: theme.md_table_row_bg().to_egui(),
            table_row_bg_zebra: theme.md_table_row_bg_zebra().to_egui(),
            table_cell_fg: theme.md_table_cell_fg().to_egui(),
            table_pad_x: theme.md_table_cell_padding_x().value(),
            table_pad_y: theme.md_table_cell_padding_y().value(),
            corner_radius: theme.corner_radius.value(),
            border_width: theme.border_width.value(),
            space_xs: theme.spacing_xs.value(),
            space_sm: theme.spacing_sm.value(),
            space_md: theme.spacing_md.value(),
            space_lg: theme.spacing_lg.value(),
            base_dir,
            link_slot,
        }
    }
}

/// Outcome of clicking a markdown link, raised out of the pure render module so the plugin
/// shell performs the side effect (host `file_handler.dispatch` for files / OS open for URLs).
///
/// - `File`: a filesystem path already made absolute against the md dir's `base_dir`.
/// - `External`: a URL/scheme handed to the OS (`http(s)`, `mailto:`, `data:`, …).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LinkClick {
    File(PathBuf),
    External(String),
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

/// Classify and resolve a markdown link destination relative to the md file's directory.
fn classify_link(dest: &str, base_dir: Option<&Path>) -> Option<LinkClick> {
    let dest = dest.trim();
    if dest.is_empty() || dest.starts_with('#') {
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

/// Per-level heading appearance, derived from `MD_H` in the design gallery.
struct HeadingStyle {
    size: f32,
    color: Color32,
    upper: bool,
    tracking: f32,
    top: f32,
    bottom: f32,
}

/// Inline formatting flags carried down the emphasis/strong/strike/code nesting.
#[derive(Clone, Copy, Default)]
struct Fmt {
    italics: bool,
    strong: bool,
    strike: bool,
    code: bool,
}

/// One inline fragment of a paragraph / heading / list item / table cell.
enum Inline {
    Text(String, Fmt),
    Link {
        runs: Vec<(String, Fmt)>,
        dest: String,
    },
    Image {
        dest: String,
        alt: String,
    },
    SoftBreak,
    HardBreak,
}

/// Configuration for laying out a sequence of inline fragments as one wrapped block.
struct InlineCfg {
    size: f32,
    color: Color32,
    line_height: f32,
    /// Heading context: inline `**bold**` must not re-tint (the heading color is fixed).
    heading: bool,
    upper: bool,
    tracking: f32,
}

/// Parse `source` and paint it into `ui` using `style` (which carries the base dir for
/// relative image resolution).
pub fn render(ui: &mut egui::Ui, style: &MdStyle, source: &str) {
    let mut opts = Options::empty();
    opts.insert(Options::ENABLE_TABLES);
    opts.insert(Options::ENABLE_TASKLISTS);
    opts.insert(Options::ENABLE_STRIKETHROUGH);
    let mut it = Parser::new_ext(source, opts).peekable();

    ui.spacing_mut().item_spacing.y = 0.0;
    let mut first = true;
    while let Some(ev) = it.next() {
        match ev {
            Event::Start(tag) => {
                block(ui, style, &mut it, tag, first);
                first = false;
            }
            Event::Rule => {
                horizontal_rule(ui, style);
                first = false;
            }
            // Top-level stray inline / html / breaks — ignore (well-formed docs wrap them).
            _ => {}
        }
    }
}

/// Render one block whose `Start(tag)` was just consumed; consumes its matching `End`.
fn block<'a, I>(
    ui: &mut egui::Ui,
    style: &MdStyle,
    it: &mut std::iter::Peekable<I>,
    tag: Tag<'a>,
    first: bool,
) where
    I: Iterator<Item = Event<'a>>,
{
    match tag {
        Tag::Paragraph => {
            if !first {
                ui.add_space(style.space_sm);
            }
            let runs = collect_inline(it);
            paragraph(ui, style, &runs);
        }
        Tag::Heading { level, .. } => {
            let hs = heading_style(style, level);
            ui.add_space(if first { 0.0 } else { hs.top });
            let runs = collect_inline(it);
            let cfg = InlineCfg {
                size: hs.size,
                color: hs.color,
                line_height: 1.3,
                heading: true,
                upper: hs.upper,
                tracking: hs.tracking,
            };
            render_inlines(ui, style, &runs, &cfg);
            ui.add_space(hs.bottom);
        }
        Tag::List(start) => {
            if !first {
                ui.add_space(style.space_xs);
            }
            list(ui, style, it, start, 0);
        }
        Tag::BlockQuote(_) => {
            if !first {
                ui.add_space(style.space_sm);
            }
            block_quote(ui, style, it);
        }
        Tag::CodeBlock(_) => {
            if !first {
                ui.add_space(style.space_sm);
            }
            let text = collect_code(it);
            code_block(ui, style, &text);
        }
        Tag::Table(aligns) => {
            if !first {
                ui.add_space(style.space_sm);
            }
            table(ui, style, it, &aligns);
        }
        // HTML blocks / footnote defs / metadata — skip their content to the matching End.
        _ => skip_to_end(it),
    }
}

// ── inline collection ───────────────────────────────────────────────────────

/// Read inline events until (and consuming) the `End` that closes the current container
/// (paragraph / heading / table cell). Nested emphasis is flattened into the run list.
fn collect_inline<'a, I>(it: &mut std::iter::Peekable<I>) -> Vec<Inline>
where
    I: Iterator<Item = Event<'a>>,
{
    let mut out = Vec::new();
    read_inline(it, &mut out, Fmt::default());
    out
}

/// Read a run of inline events while the next event is inline; stops *without consuming*
/// a block `Start` or the container `End`. Used for tight list items (no paragraph wrap).
fn collect_inline_run<'a, I>(it: &mut std::iter::Peekable<I>) -> Vec<Inline>
where
    I: Iterator<Item = Event<'a>>,
{
    let mut out = Vec::new();
    while let Some(ev) = it.peek() {
        if !is_inline_start(ev) {
            break;
        }
        let ev = it.next().expect("peeked");
        consume_inline_event(it, &mut out, Fmt::default(), ev);
    }
    out
}

fn is_inline_start(ev: &Event<'_>) -> bool {
    matches!(
        ev,
        Event::Text(_)
            | Event::Code(_)
            | Event::SoftBreak
            | Event::HardBreak
            | Event::Start(
                Tag::Emphasis
                    | Tag::Strong
                    | Tag::Strikethrough
                    | Tag::Link { .. }
                    | Tag::Image { .. }
            )
    )
}

/// Recursively read inline events into `out`, returning on the first unmatched `End`
/// (which it consumes) — that `End` closes the container this call was entered for.
fn read_inline<'a, I>(it: &mut std::iter::Peekable<I>, out: &mut Vec<Inline>, fmt: Fmt)
where
    I: Iterator<Item = Event<'a>>,
{
    while let Some(ev) = it.next() {
        if matches!(ev, Event::End(_)) {
            return;
        }
        consume_inline_event(it, out, fmt, ev);
    }
}

fn consume_inline_event<'a, I>(
    it: &mut std::iter::Peekable<I>,
    out: &mut Vec<Inline>,
    fmt: Fmt,
    ev: Event<'a>,
) where
    I: Iterator<Item = Event<'a>>,
{
    match ev {
        Event::Text(t) => out.push(Inline::Text(t.into_string(), fmt)),
        Event::Code(t) => out.push(Inline::Text(t.into_string(), Fmt { code: true, ..fmt })),
        Event::SoftBreak => out.push(Inline::SoftBreak),
        Event::HardBreak => out.push(Inline::HardBreak),
        Event::Start(Tag::Emphasis) => read_inline(
            it,
            out,
            Fmt {
                italics: true,
                ..fmt
            },
        ),
        Event::Start(Tag::Strong) => read_inline(
            it,
            out,
            Fmt {
                strong: true,
                ..fmt
            },
        ),
        Event::Start(Tag::Strikethrough) => read_inline(
            it,
            out,
            Fmt {
                strike: true,
                ..fmt
            },
        ),
        Event::Start(Tag::Link { dest_url, .. }) => {
            let mut inner = Vec::new();
            read_inline(it, &mut inner, fmt);
            out.push(Inline::Link {
                runs: inner_runs(inner),
                dest: dest_url.into_string(),
            });
        }
        Event::Start(Tag::Image { dest_url, .. }) => {
            let mut inner = Vec::new();
            read_inline(it, &mut inner, fmt);
            out.push(Inline::Image {
                dest: dest_url.into_string(),
                alt: inner_text(&inner),
            });
        }
        // Other inline-ish events (inline html, math, footnote refs) are dropped.
        _ => {}
    }
}

fn inner_runs(inner: Vec<Inline>) -> Vec<(String, Fmt)> {
    inner
        .into_iter()
        .filter_map(|i| match i {
            Inline::Text(t, f) => Some((t, f)),
            Inline::SoftBreak => Some((" ".to_string(), Fmt::default())),
            _ => None,
        })
        .collect()
}

fn inner_text(inner: &[Inline]) -> String {
    inner
        .iter()
        .map(|i| match i {
            Inline::Text(t, _) => t.as_str(),
            Inline::SoftBreak => " ",
            _ => "",
        })
        .collect()
}

// ── inline layout ───────────────────────────────────────────────────────────

fn paragraph(ui: &mut egui::Ui, style: &MdStyle, runs: &[Inline]) {
    let cfg = InlineCfg {
        size: style.body,
        color: style.text,
        line_height: style.line_height_prose,
        heading: false,
        upper: false,
        tracking: 0.0,
    };
    render_inlines(ui, style, runs, &cfg);
}

/// Lay out `runs` as one wrapped block. Contiguous text is batched into a single
/// `LayoutJob` (so internal wrapping honours `cfg.line_height`); links and images
/// interrupt the batch and render as their own widgets.
fn render_inlines(ui: &mut egui::Ui, style: &MdStyle, runs: &[Inline], cfg: &InlineCfg) {
    let leading = (cfg.size * (cfg.line_height - 1.0)).max(0.0);
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = egui::vec2(0.0, leading);
        let mut job = LayoutJob::default();
        for inl in runs {
            match inl {
                Inline::Text(t, fmt) => append_section(&mut job, style, cfg, t, *fmt),
                Inline::SoftBreak => append_section(&mut job, style, cfg, " ", Fmt::default()),
                Inline::HardBreak => {
                    flush_job(ui, &mut job);
                    force_wrap(ui);
                }
                Inline::Link { runs, dest } => {
                    flush_job(ui, &mut job);
                    link_widget(ui, style, cfg, runs, dest);
                }
                Inline::Image { dest, alt } => {
                    flush_job(ui, &mut job);
                    image_widget(ui, style, dest, alt);
                }
            }
        }
        flush_job(ui, &mut job);
    });
}

fn flush_job(ui: &mut egui::Ui, job: &mut LayoutJob) {
    if !job.is_empty() {
        ui.label(std::mem::take(job));
    }
}

/// Consume the remainder of the current wrapped row so the next widget starts a new line.
fn force_wrap(ui: &mut egui::Ui) {
    let w = ui.available_width().max(0.0);
    ui.allocate_exact_size(egui::vec2(w, 0.0), Sense::hover());
}

fn append_section(job: &mut LayoutJob, style: &MdStyle, cfg: &InlineCfg, text: &str, fmt: Fmt) {
    let owned;
    let text = if cfg.upper {
        owned = text.to_uppercase();
        owned.as_str()
    } else {
        text
    };
    let mut tf = TextFormat {
        font_id: FontId::new(cfg.size, style.family.clone()),
        color: cfg.color,
        line_height: Some(cfg.size * cfg.line_height),
        extra_letter_spacing: cfg.tracking,
        valign: Align::Center,
        ..Default::default()
    };
    if fmt.italics {
        tf.italics = true;
    }
    if fmt.strike {
        tf.strikethrough = Stroke::new(1.0, cfg.color);
    }
    // Inline bold has no synthetic weight in egui — promote to text-primary as a cue.
    if fmt.strong && !cfg.heading {
        tf.color = style.primary;
    }
    if fmt.code {
        tf.font_id = FontId::new(cfg.size, FontFamily::Monospace);
        tf.color = style.code_fg;
        tf.background = style.code_bg;
    }
    job.append(text, 0.0, tf);
}

fn link_widget(
    ui: &mut egui::Ui,
    style: &MdStyle,
    cfg: &InlineCfg,
    runs: &[(String, Fmt)],
    dest: &str,
) {
    let mut job = LayoutJob::default();
    for (t, fmt) in runs {
        let mut tf = TextFormat {
            font_id: FontId::new(cfg.size, style.family.clone()),
            color: style.link,
            line_height: Some(cfg.size * cfg.line_height),
            underline: Stroke::new(1.0, style.link),
            valign: Align::Center,
            ..Default::default()
        };
        if fmt.italics {
            tf.italics = true;
        }
        job.append(t, 0.0, tf);
    }
    let resp = ui
        .add(egui::Label::new(job).sense(Sense::click()))
        .on_hover_text(dest)
        .on_hover_cursor(egui::CursorIcon::PointingHand);
    if resp.clicked()
        && let Some(click) = classify_link(dest, style.base_dir.as_deref())
    {
        // Defer the side effect: stash the resolved click in egui temp memory; the plugin
        // shell reads it after render and dispatches (host file open / OS url). Last click
        // in a frame wins (single slot).
        let slot = style.link_slot;
        ui.ctx().data_mut(|d| d.insert_temp(slot, click));
    }
}

/// Inline image — B1 defers bitmap texture loading; render the alt/destination text so the
/// reference is visible (never silently vanishes). Full inline-image support is a follow-up.
fn image_widget(ui: &mut egui::Ui, style: &MdStyle, dest: &str, alt: &str) {
    let label = if alt.is_empty() { dest } else { alt };
    ui.label(
        egui::RichText::new(label)
            .italics()
            .size(style.body)
            .color(style.muted),
    );
}

// ── headings ────────────────────────────────────────────────────────────────

/// Map a heading level to its design appearance (see `MD_H` in the gallery).
fn heading_style(style: &MdStyle, level: HeadingLevel) -> HeadingStyle {
    // prose-h1 (20) is the only cap-exempt content level; the rest stay ≤ prose-h2 (14, the font cap).
    let prose_h1 = style.font_size_prose_h1;
    let max = style.font_size_prose_h2;
    let body = style.body;
    match level {
        HeadingLevel::H1 => HeadingStyle {
            size: prose_h1,
            color: style.primary,
            upper: false,
            tracking: 0.0,
            top: 0.0,
            bottom: style.space_sm,
        },
        HeadingLevel::H2 => HeadingStyle {
            size: max,
            color: style.primary,
            upper: false,
            tracking: 0.0,
            top: style.space_lg,
            bottom: style.space_xs,
        },
        HeadingLevel::H3 => HeadingStyle {
            size: max,
            color: style.primary,
            upper: false,
            tracking: 0.0,
            top: style.space_md,
            bottom: style.space_xs,
        },
        HeadingLevel::H4 => HeadingStyle {
            size: body,
            color: style.text,
            upper: false,
            tracking: 0.0,
            top: style.space_md,
            bottom: style.space_xs,
        },
        HeadingLevel::H5 => HeadingStyle {
            size: body,
            color: style.muted,
            upper: false,
            tracking: 0.0,
            top: style.space_sm,
            bottom: style.space_xs,
        },
        HeadingLevel::H6 => HeadingStyle {
            size: body,
            color: style.muted,
            upper: true,
            tracking: 0.6,
            top: style.space_sm,
            bottom: style.space_xs,
        },
    }
}

// ── lists ─────────────────────────────────────────────────────────────────--

fn list<'a, I>(
    ui: &mut egui::Ui,
    style: &MdStyle,
    it: &mut std::iter::Peekable<I>,
    start: Option<u64>,
    depth: usize,
) where
    I: Iterator<Item = Event<'a>>,
{
    let mut number = start;
    loop {
        match it.next() {
            Some(Event::Start(Tag::Item)) => {
                list_item(ui, style, it, number, depth);
                number = number.map(|n| n + 1);
            }
            Some(Event::End(TagEnd::List(_))) | None => break,
            _ => {}
        }
    }
}

fn list_item<'a, I>(
    ui: &mut egui::Ui,
    style: &MdStyle,
    it: &mut std::iter::Peekable<I>,
    number: Option<u64>,
    depth: usize,
) where
    I: Iterator<Item = Event<'a>>,
{
    // One indent step; deeper levels nest visually via the parent item's content ui.
    let indent = style.space_lg;
    // Optional task-list marker comes first inside the item.
    let mut task: Option<bool> = None;
    if let Some(Event::TaskListMarker(done)) = it.peek() {
        task = Some(*done);
        it.next();
    }

    ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        ui.add_space(indent);
        marker(ui, style, number, task, depth);
        ui.add_space(style.space_xs);
        ui.vertical(|ui| {
            item_content(ui, style, it, depth);
        });
    });
}

/// Render an item's content: a leading inline run (tight items aren't paragraph-wrapped),
/// any paragraph blocks, and nested lists — until the `End(Item)`.
fn item_content<'a, I>(
    ui: &mut egui::Ui,
    style: &MdStyle,
    it: &mut std::iter::Peekable<I>,
    depth: usize,
) where
    I: Iterator<Item = Event<'a>>,
{
    let mut first = true;
    loop {
        // Tight inline content (no Paragraph wrapper).
        if it.peek().map(is_inline_start).unwrap_or(false) {
            let runs = collect_inline_run(it);
            if !first {
                ui.add_space(style.space_xs);
            }
            paragraph(ui, style, &runs);
            first = false;
            continue;
        }
        match it.next() {
            Some(Event::Start(Tag::Paragraph)) => {
                if !first {
                    ui.add_space(style.space_xs);
                }
                let runs = collect_inline(it);
                paragraph(ui, style, &runs);
                first = false;
            }
            Some(Event::Start(Tag::List(start))) => {
                ui.add_space(style.space_xs);
                list(ui, style, it, start, depth + 1);
                first = false;
            }
            Some(Event::Start(Tag::CodeBlock(_))) => {
                ui.add_space(style.space_xs);
                let text = collect_code(it);
                code_block(ui, style, &text);
                first = false;
            }
            Some(Event::Start(other)) => {
                // Nested block we don't special-case inside items — render generically.
                block(ui, style, it, other, first);
                first = false;
            }
            Some(Event::End(TagEnd::Item)) | None => break,
            _ => {}
        }
    }
}

fn marker(
    ui: &mut egui::Ui,
    style: &MdStyle,
    number: Option<u64>,
    task: Option<bool>,
    depth: usize,
) {
    if let Some(done) = task {
        checkbox(ui, style, done);
        return;
    }
    let (text, font) = match number {
        Some(n) => (
            format!("{n}."),
            FontId::new(style.body, FontFamily::Monospace),
        ),
        None => (
            bullet(depth).to_string(),
            FontId::new(style.body, style.family.clone()),
        ),
    };
    // Measure the marker so multi-digit ordinals ("10.") don't collide with the content.
    let galley = ui.fonts(|f| f.layout_no_wrap(text.clone(), font.clone(), style.muted));
    let w = galley.size().x.max(style.space_lg);
    let row_h = style.body * style.line_height_prose;
    let (rect, _) = ui.allocate_exact_size(egui::vec2(w, row_h), Sense::hover());
    ui.painter().text(
        egui::pos2(
            rect.left(),
            rect.top() + style.body * (style.line_height_prose - 1.0) * 0.5,
        ),
        egui::Align2::LEFT_TOP,
        text,
        font,
        style.muted,
    );
}

fn bullet(depth: usize) -> &'static str {
    match depth {
        0 => "•",
        1 => "◦",
        _ => "▪",
    }
}

fn checkbox(ui: &mut egui::Ui, style: &MdStyle, done: bool) {
    let s = style.body;
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(style.space_lg, s * style.line_height_prose),
        Sense::hover(),
    );
    let box_rect = egui::Rect::from_center_size(
        egui::pos2(rect.left() + s * 0.5, rect.center().y),
        egui::vec2(s, s) * 0.85,
    );
    let p = ui.painter();
    if done {
        p.rect_filled(box_rect, style.border_width, style.link);
        // simple check glyph
        let c = box_rect.center();
        let h = box_rect.height();
        let pts = vec![
            box_rect.left_top() + egui::vec2(h * 0.22, h * 0.52),
            c + egui::vec2(-h * 0.08, h * 0.22),
            box_rect.right_top() + egui::vec2(-h * 0.18, h * 0.28),
        ];
        p.add(egui::Shape::line(pts, Stroke::new(1.5, style.code_fg)));
    } else {
        p.rect_stroke(
            box_rect,
            style.border_width,
            Stroke::new(style.border_width, style.blockquote_bar),
            egui::StrokeKind::Inside,
        );
    }
}

// ── blockquote ──────────────────────────────────────────────────────────────

fn block_quote<'a, I>(ui: &mut egui::Ui, style: &MdStyle, it: &mut std::iter::Peekable<I>)
where
    I: Iterator<Item = Event<'a>>,
{
    let bar_w = 2.0;
    let gap = style.space_md;
    let top = ui.cursor().min.y;
    let left = ui.min_rect().left();
    let avail = ui.available_width();
    let content_x = left + bar_w + gap;
    let content_w = (avail - bar_w - gap).max(1.0);

    // Quoted text reads one tone down (muted).
    let mut qstyle = style.clone();
    qstyle.text = style.muted;

    let child_rect = egui::Rect::from_min_size(
        egui::pos2(content_x, top),
        egui::vec2(content_w, f32::INFINITY),
    );
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(child_rect)
            .layout(egui::Layout::top_down(Align::Min)),
    );
    child.spacing_mut().item_spacing.y = 0.0;
    render_blocks_until_end(&mut child, &qstyle, it);
    let bottom = child.min_rect().bottom();

    // Reserve the consumed area in the parent and paint the left bar over its height.
    ui.allocate_rect(
        egui::Rect::from_min_max(egui::pos2(left, top), egui::pos2(left + avail, bottom)),
        Sense::hover(),
    );
    ui.painter().rect_filled(
        egui::Rect::from_min_max(egui::pos2(left, top), egui::pos2(left + bar_w, bottom)),
        0.0,
        style.blockquote_bar,
    );
}

/// Render blocks until the `End` that closes the current container (consumes that `End`).
fn render_blocks_until_end<'a, I>(
    ui: &mut egui::Ui,
    style: &MdStyle,
    it: &mut std::iter::Peekable<I>,
) where
    I: Iterator<Item = Event<'a>>,
{
    let mut first = true;
    loop {
        match it.next() {
            Some(Event::Start(tag)) => {
                block(ui, style, it, tag, first);
                first = false;
            }
            Some(Event::Rule) => {
                horizontal_rule(ui, style);
                first = false;
            }
            Some(Event::End(_)) | None => break,
            _ => {}
        }
    }
}

// ── code block / rule / table ─────────────────────────────────────────────--

fn collect_code<'a, I>(it: &mut std::iter::Peekable<I>) -> String
where
    I: Iterator<Item = Event<'a>>,
{
    let mut text = String::new();
    loop {
        match it.next() {
            Some(Event::Text(t)) => text.push_str(&t),
            Some(Event::End(TagEnd::CodeBlock)) | None => break,
            _ => {}
        }
    }
    while text.ends_with('\n') {
        text.pop();
    }
    text
}

fn code_block(ui: &mut egui::Ui, style: &MdStyle, text: &str) {
    egui::Frame::new()
        .fill(style.surface_raised)
        .corner_radius(style.corner_radius)
        .inner_margin(egui::Margin::same(style.space_sm as i8))
        .show(ui, |ui| {
            ui.set_width(ui.available_width() - 2.0 * style.space_sm);
            ui.label(
                egui::RichText::new(text)
                    .monospace()
                    .size(style.body)
                    .color(style.text),
            );
        });
}

fn horizontal_rule(ui: &mut egui::Ui, style: &MdStyle) {
    ui.add_space(style.space_md);
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), style.border_width),
        Sense::hover(),
    );
    ui.painter().hline(
        rect.x_range(),
        rect.center().y,
        Stroke::new(style.border_width, style.separator),
    );
    ui.add_space(style.space_md);
}

fn table<'a, I>(
    ui: &mut egui::Ui,
    style: &MdStyle,
    it: &mut std::iter::Peekable<I>,
    aligns: &[Alignment],
) where
    I: Iterator<Item = Event<'a>>,
{
    let cols = aligns.len().max(1);
    let mut head: Vec<Vec<Inline>> = Vec::new();
    let mut rows: Vec<Vec<Vec<Inline>>> = Vec::new();
    let mut current: Vec<Vec<Inline>> = Vec::new();
    let mut in_head = false;

    loop {
        match it.next() {
            Some(Event::Start(Tag::TableHead)) => in_head = true,
            Some(Event::Start(Tag::TableRow)) => current = Vec::new(),
            Some(Event::Start(Tag::TableCell)) => current.push(collect_inline(it)),
            Some(Event::End(TagEnd::TableHead)) => {
                head = std::mem::take(&mut current);
                in_head = false;
            }
            Some(Event::End(TagEnd::TableRow)) => {
                if !in_head {
                    rows.push(std::mem::take(&mut current));
                }
            }
            Some(Event::End(TagEnd::Table)) | None => break,
            _ => {}
        }
    }

    let r = style.corner_radius as u8;
    let pad_x = style.table_pad_x;
    let n = rows.len();
    // 셀 좌우 패딩(pad_x)이 세로 격자선 양쪽에 대칭으로 오도록 컬럼 간격을 2*pad_x 로 준다.
    // (외곽↔셀 = inner_margin pad_x, 셀↔세로선 = 컬럼간격의 절반 pad_x 로 모두 pad_x 정렬.)
    let col_gap = pad_x * 2.0;
    let margin = egui::Margin::symmetric(pad_x as i8, style.table_pad_y as i8);

    let out = egui::Frame::new()
        .fill(style.table_row_bg)
        .stroke(Stroke::new(style.border_width, style.table_border))
        .corner_radius(style.corner_radius)
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            // 헤더 밴드 — surface-raised 채움 + text-primary. 본문이 없으면 이 밴드가
            // 표 전체라 4모서리 모두 라운드, 있으면 상단 2모서리만.
            let header_radius = if n == 0 {
                egui::CornerRadius::same(r)
            } else {
                egui::CornerRadius {
                    nw: r,
                    ne: r,
                    sw: 0,
                    se: 0,
                }
            };
            egui::Frame::new()
                .fill(style.table_header_bg)
                .corner_radius(header_radius)
                .inner_margin(margin)
                .show(ui, |ui| {
                    table_row(ui, style, &head, cols, aligns, col_gap, true)
                });
            // 본문 0행이면 헤더 하단이 곧 외곽 — 하단 격자선 생략(§2-4).
            if n > 0 {
                table_divider(ui, style);
            }
            for (i, row) in rows.iter().enumerate() {
                let zebra = i % 2 == 1; // 첫 본문행(base) 이후 짝수행(2·4…)=zebra.
                let is_last = i + 1 == n;
                let mut f = egui::Frame::new().inner_margin(margin);
                if zebra {
                    f = f.fill(style.table_row_bg_zebra);
                    if is_last {
                        // zebra 채움이 있는 마지막 행은 하단 2모서리를 외곽 라운드에 맞춘다.
                        f = f.corner_radius(egui::CornerRadius {
                            nw: 0,
                            ne: 0,
                            sw: r,
                            se: r,
                        });
                    }
                }
                f.show(ui, |ui| {
                    table_row(ui, style, row, cols, aligns, col_gap, false)
                });
                if !is_last {
                    table_divider(ui, style);
                }
            }
        });

    // 세로 컬럼 격자선 — egui `ui.columns()` 는 세로선을 그리지 않으므로 표 전체
    // 높이에 걸쳐 컬럼 경계마다 수동 draw. 마지막 열의 오른쪽 선은 외곽과 겹치므로 생략.
    let content = out.response.rect;
    let inner_w = content.width() - col_gap; // = 좌우 inner_margin(pad_x*2) 제외 폭.
    if cols > 1 && inner_w > col_gap * (cols as f32 - 1.0) {
        let col_w = (inner_w - col_gap * (cols as f32 - 1.0)) / cols as f32;
        let inner_left = content.left() + pad_x;
        let painter = ui.painter();
        for i in 1..cols {
            let x = inner_left + i as f32 * col_w + (2 * i - 1) as f32 * pad_x;
            painter.vline(
                x,
                content.y_range(),
                Stroke::new(style.border_width, style.table_border),
            );
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn table_row(
    ui: &mut egui::Ui,
    style: &MdStyle,
    cells: &[Vec<Inline>],
    cols: usize,
    aligns: &[Alignment],
    col_gap: f32,
    header: bool,
) {
    // 헤더 신호는 weight 가 아니라 색+배경 — 헤더는 text-primary, 본문은 text-secondary.
    let color = if header {
        style.table_header_fg
    } else {
        style.table_cell_fg
    };
    ui.spacing_mut().item_spacing.x = col_gap;
    // 동적폭 방어: 잔여폭이 컬럼 간격 합 미만이면 columns 내부 폭이 음수가 되어 panic.
    // 그 경우 세로 폴백(격자 없이)으로 그린다.
    if ui.available_width() <= col_gap * (cols as f32 - 1.0) + 1.0 {
        ui.vertical(|ui| {
            for cell in cells.iter().take(cols) {
                table_cell(ui, style, Some(cell), color, header, false);
            }
        });
        return;
    }
    ui.columns(cols, |c| {
        for (col, cell_ui) in c.iter_mut().enumerate() {
            let right = matches!(aligns.get(col), Some(Alignment::Right));
            table_cell(cell_ui, style, cells.get(col), color, header, right);
        }
    });
}

/// 한 셀 렌더 — 정렬(좌/우) 반영, 헤더면 bold re-tint 억제.
fn table_cell(
    ui: &mut egui::Ui,
    style: &MdStyle,
    cell: Option<&Vec<Inline>>,
    color: Color32,
    header: bool,
    right: bool,
) {
    let cfg = InlineCfg {
        size: style.body,
        color,
        line_height: 1.4,
        heading: header, // 헤더: bold re-tint 억제, 한 weight 유지
        upper: false,
        tracking: 0.0,
    };
    let lay = if right {
        egui::Layout::right_to_left(Align::Min)
    } else {
        egui::Layout::left_to_right(Align::Min)
    };
    ui.with_layout(lay, |ui| {
        if let Some(cell) = cell {
            render_inlines(ui, style, cell, &cfg);
        }
    });
}

fn table_divider(ui: &mut egui::Ui, style: &MdStyle) {
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), style.border_width),
        Sense::hover(),
    );
    ui.painter().hline(
        rect.x_range(),
        rect.center().y,
        Stroke::new(style.border_width, style.table_border),
    );
}

// ── misc ──────────────────────────────────────────────────────────────────--

fn skip_to_end<'a, I>(it: &mut std::iter::Peekable<I>)
where
    I: Iterator<Item = Event<'a>>,
{
    let mut depth = 1usize;
    while depth > 0 {
        match it.next() {
            Some(Event::Start(_)) => depth += 1,
            Some(Event::End(_)) => depth -= 1,
            None => break,
            _ => {}
        }
    }
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
        // `lexically_normalize` collapses the `..` against the base dir on every platform.
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
}
