//! Markdown rendering via **`egui_commonmark`** (library-driven, ADR-0028 / B1; token
//! exceptions documented in `docs/plugins/markdown/screens/markdown.md`). The former
//! hand-rolled pulldown-cmark → egui layout renderer is retired: the
//! plugin now hands the markdown text to [`egui_commonmark::CommonMarkViewer`], which reads
//! every color from `egui::Visuals`. We inject the host `Theme` semantic tokens into the
//! `Visuals`/text-styles right before `show`, so the design tokens still drive the output.
//!
//! Confirmed design exceptions (token-level, see `tokens/semantic.css:137-138,152`):
//! - **Heading ladder is interpolated** by the library between `Heading` (the `prose-h1`
//!   anchor) and `Body`; per-H2..H6 pixel sizes are not settable (`prose-h2` deprecated).
//! - **Body leading override** is not exposed by the library (`line-height-prose` deprecated).
//!
//! What the library exposes vs. the `md-table` tokens (`tokens/components.css:297-304`): the
//! table is drawn with `Frame::group` + `Grid::striped`, so we can drive the **grid border**
//! (`md-table-border`, opaque so it is finally visible), the **zebra stripe**
//! (`md-table-row-bg-zebra`), and the **cell text** (`md-table-cell-fg`, which equals the body
//! prose tone). A header band / opaque base fill / per-cell 8·4px padding are not reachable
//! through the library's Grid — a library-driven constraint noted alongside the heading one.

use std::path::{Path, PathBuf};

use egui_commonmark::{CommonMarkCache, CommonMarkViewer};
use pulldown_cmark::{Event, Options, Parser, Tag};
use tasty_type_appearance::theme::Theme;

/// Per-surface library cache — persists across frames (installed image loaders flag, link
/// hook table). Kept in the plugin's per-surface state so it is not rebuilt every paint.
pub type MdCache = CommonMarkCache;

/// Outcome of clicking a markdown link, raised so the plugin shell performs the side effect
/// (host `file_handler.dispatch` for files / OS open for URLs).
///
/// - `File`: a filesystem path already made absolute against the md dir's `base_dir`.
/// - `External`: a URL/scheme handed to the OS (`http(s)`, `mailto:`, `data:`, …).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LinkClick {
    File(PathBuf),
    External(String),
}

/// Parser options mirroring `egui_commonmark_backend::pulldown::parser_options` so the link
/// pre-scan classifies the exact same destinations the viewer renders.
fn parser_options() -> Options {
    Options::ENABLE_TABLES
        | Options::ENABLE_TASKLISTS
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_FOOTNOTES
        | Options::ENABLE_DEFINITION_LIST
}

/// Inject the host `Theme` semantic tokens into `ui`'s `Visuals`/text-styles, register the
/// markdown's link destinations as hooks (so every link is routed to the host instead of the
/// OS), and render `source` through `egui_commonmark`. Clicks are read afterwards via
/// [`take_clicked_link`].
pub fn render(ui: &mut egui::Ui, theme: &Theme, body_px: f32, cache: &mut MdCache, source: &str) {
    apply_theme(ui, theme, body_px);
    register_link_hooks(cache, source);
    CommonMarkViewer::new().show(ui, cache, source);
}

/// Map the design tokens onto the `Visuals`/text-style fields the library reads. Every color
/// the viewer paints comes from here — no hardcoded RGB in the render path.
fn apply_theme(ui: &mut egui::Ui, theme: &Theme, body_px: f32) {
    use egui::{FontFamily, FontId, TextStyle};

    let body = body_px.max(1.0);
    let style = ui.style_mut();

    // Body / heading-anchor / mono sizes. `Heading` is the top of the library's interpolated
    // heading ladder (prose-h1 anchor); H2..H6 are interpolated down to `Body`.
    style
        .text_styles
        .insert(TextStyle::Body, FontId::new(body, FontFamily::Proportional));
    style.text_styles.insert(
        TextStyle::Heading,
        FontId::new(theme.font_size_prose_h1.value(), FontFamily::Proportional),
    );
    style.text_styles.insert(
        TextStyle::Monospace,
        FontId::new(body, FontFamily::Monospace),
    );

    let v = &mut style.visuals;
    // Body prose tone (also the md-table cell fg — the two coincide by design).
    v.override_text_color = Some(theme.text_secondary().to_egui());
    // Links.
    v.hyperlink_color = theme.accent_primary().to_egui();
    // Inline `code` background and fenced code-block background.
    v.code_bg_color = theme.surface_raised().to_egui();
    v.extreme_bg_color = theme.surface_raised().to_egui();
    // Table zebra stripe (odd rows in the library's striped Grid).
    v.faint_bg_color = theme.md_table_row_bg_zebra().to_egui();
    // `strong` / heading / bullet color = strong_text_color() = widgets.active fg.
    v.widgets.active.fg_stroke.color = theme.text_primary().to_egui();
    // Shared noninteractive stroke: table grid border + code-block border + horizontal rule.
    // md-table-border is border-strong (opaque) so the grid is finally visible.
    v.widgets.noninteractive.bg_stroke = egui::Stroke::new(
        theme.border_width.value(),
        theme.md_table_border().to_egui(),
    );
    v.widgets.noninteractive.corner_radius =
        egui::CornerRadius::same(theme.corner_radius.value() as u8);
}

/// Register every link destination in `source` as a hook so the viewer renders each link with
/// `ui.link` (no OS open) and flags the hook on click; [`take_clicked_link`] reads them back.
/// Rebuilt each frame (the document can change) — cleared first to drop stale destinations.
fn register_link_hooks(cache: &mut MdCache, source: &str) {
    cache.link_hooks_clear();
    for ev in Parser::new_ext(source, parser_options()) {
        if let Event::Start(Tag::Link { dest_url, .. }) = ev {
            cache.add_link_hook(dest_url.into_string());
        }
    }
}

/// Read back which registered link hook was clicked this frame and classify it against
/// `base_dir`. Returns the first clicked destination (single-dispatch per frame), or `None`.
/// Consumes the click by deactivating the hook so it does not re-fire.
pub fn take_clicked_link(cache: &mut MdCache, base_dir: Option<&Path>) -> Option<LinkClick> {
    let clicked = cache
        .link_hooks()
        .iter()
        .find_map(|(dest, &hit)| if hit { Some(dest.clone()) } else { None })?;
    cache.link_hooks_mut().insert(clicked.clone(), false);
    classify_link(&clicked, base_dir)
}

// ── link classification (unchanged from the former renderer) ─────────────────

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

    #[test]
    fn link_hooks_scan_registers_all_destinations() {
        let mut cache = MdCache::default();
        register_link_hooks(
            &mut cache,
            "[a](./a.md) and [b](https://x.test) and text [c](../c.md)",
        );
        assert_eq!(cache.get_link_hook("./a.md"), Some(false));
        assert_eq!(cache.get_link_hook("https://x.test"), Some(false));
        assert_eq!(cache.get_link_hook("../c.md"), Some(false));
    }

    #[test]
    fn take_clicked_link_classifies_and_consumes() {
        let mut cache = MdCache::default();
        cache.add_link_hook("../sibling.md".to_string());
        cache
            .link_hooks_mut()
            .insert("../sibling.md".to_string(), true);
        let b = base();
        let got = take_clicked_link(&mut cache, Some(&b));
        assert!(matches!(got, Some(LinkClick::File(_))));
        // consumed → hook back to false, no re-fire.
        assert_eq!(cache.get_link_hook("../sibling.md"), Some(false));
        assert_eq!(take_clicked_link(&mut cache, Some(&b)), None);
    }
}
