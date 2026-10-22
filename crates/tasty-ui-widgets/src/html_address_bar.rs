//! Host-only HTML address band. It emits actions; it never navigates, looks up files,
//! changes a document, or acquires native keyboard focus. Labels are caller supplied.

use crate::{
    ControlSize, IconButton, Input, Spinner, Tooltip, TooltipPlacement, tooltip_hover_delay_elapsed,
};
use tasty_type_appearance::theme::Theme;
use tasty_type_geometry::length::LogicalPx;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HtmlAddressKind {
    Empty,
    Local,
    Web,
    Mirror,
    MirrorPlaceholder,
}

#[derive(Clone, Debug, Default)]
pub struct HtmlAddressState {
    pub buffer: String,
    pub editing: bool,
    pub error: Option<String>,
}

impl HtmlAddressState {
    /// Commit updates do not overwrite a user's draft or a rejected input.
    pub fn synchronize(&mut self, current: &str) {
        if !self.editing && self.error.is_none() {
            self.buffer = current.into();
        }
    }
    pub fn reject(&mut self, message: String) {
        self.error = Some(message);
        self.editing = true;
    }
    pub fn accepted(&mut self) {
        self.error = None;
        self.editing = false;
    }
    pub fn revert(&mut self, current: &str) {
        self.buffer = current.into();
        self.error = None;
        self.editing = false;
    }
}

pub struct HtmlAddressLabels<'a> {
    pub placeholder: &'a str,
    pub back: &'a str,
    pub forward: &'a str,
    pub reload: &'a str,
    pub disconnected: &'a str,
}

pub struct HtmlAddressView<'a> {
    /// Display form of the committed address, or the attempted address while loading/failed.
    /// The host keeps these separate in its navigation model.
    pub address: &'a str,
    /// Latest committed display address; Esc never restores a failed/pending attempt.
    pub committed_address: &'a str,
    pub kind: HtmlAddressKind,
    pub can_back: bool,
    pub can_forward: bool,
    pub loading: bool,
    pub disconnected: bool,
    /// One-frame, user-origin focus request. Never set this for an agent navigation.
    pub focus: bool,
    pub labels: HtmlAddressLabels<'a>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HtmlAddressAction {
    None,
    Back,
    Forward,
    Reload,
    Open(String),
    Revert,
}

pub struct HtmlAddressOutput {
    pub rect: egui::Rect,
    pub field_rect: Option<egui::Rect>,
    pub forward_rect: Option<egui::Rect>,
    pub reload_rect: Option<egui::Rect>,
    pub action: HtmlAddressAction,
    /// Host uses this to keep keyboard input out of the native WebView.
    pub has_keyboard_focus: bool,
}

/// Separator is painted inside the allocated band. An error adds exactly one error row.
pub fn html_address_height(theme: &Theme, kind: HtmlAddressKind, error: bool) -> LogicalPx {
    if kind == HtmlAddressKind::MirrorPlaceholder {
        LogicalPx(0.0)
    } else {
        theme.html_address_bar_height()
            + if error {
                theme.html_address_error_height()
            } else {
                LogicalPx(0.0)
            }
    }
}

/// Shared inset arithmetic; banner heights are measured by their views in size→script order.
pub fn html_address_content_inset(
    theme: &Theme,
    band: LogicalPx,
    banners: &[LogicalPx],
) -> LogicalPx {
    if banners.is_empty() {
        band
    } else {
        banners.iter().fold(band + theme.banner_margin(), |h, b| {
            h + *b + theme.banner_inset_gap()
        })
    }
}

fn nav(
    ui: &mut egui::Ui,
    theme: &Theme,
    icon: tasty_icons::Icon,
    enabled: bool,
    label: &str,
) -> egui::Response {
    let resp = IconButton::new()
        .size(ControlSize::Sm)
        .enabled(enabled)
        .show(ui, theme, &|ui, rect, color| {
            icon.image(rect.height(), color).paint_at(ui, rect);
        });
    if tooltip_hover_delay_elapsed(ui.ctx(), theme, resp.id, resp.hovered()) {
        Tooltip::new(label)
            .placement(TooltipPlacement::Top)
            .id_source(resp.id)
            .show(ui, theme, resp.rect);
    }
    resp
}

fn left_ellipsis(ui: &egui::Ui, theme: &Theme, text: &str, width: LogicalPx) -> String {
    let font = egui::FontId::monospace(theme.input_font_size().value());
    let fits = |s: &str| {
        ui.painter()
            .layout_no_wrap(s.into(), font.clone(), egui::Color32::PLACEHOLDER)
            .rect
            .width()
            <= width.value()
    };
    if fits(text) {
        return text.into();
    }
    let boundaries: Vec<_> = text
        .char_indices()
        .map(|(i, _)| i)
        .chain(std::iter::once(text.len()))
        .collect();
    let mut lo = 0;
    let mut hi = boundaries.len() - 1;
    while lo < hi {
        let mid = (lo + hi) / 2;
        if fits(&format!("…{}", &text[boundaries[mid]..])) {
            hi = mid;
        } else {
            lo = mid + 1;
        }
    }
    format!("…{}", &text[boundaries[lo]..])
}

/// Uses the same Input box as PathField, without its Go/completion composite. Explicit
/// child rectangles keep nav cells fixed even below the minimum supported field width.
pub fn html_address_bar(
    ui: &mut egui::Ui,
    theme: &Theme,
    id: egui::Id,
    width: LogicalPx,
    state: &mut HtmlAddressState,
    view: &HtmlAddressView<'_>,
) -> HtmlAddressOutput {
    let height = html_address_height(theme, view.kind, state.error.is_some());
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(width.value(), height.value()),
        egui::Sense::hover(),
    );
    let mut out = HtmlAddressOutput {
        rect,
        field_rect: None,
        forward_rect: None,
        reload_rect: None,
        action: HtmlAddressAction::None,
        has_keyboard_focus: false,
    };
    if view.kind == HtmlAddressKind::MirrorPlaceholder {
        return out;
    }
    state.synchronize(view.address);
    let mirror = view.kind == HtmlAddressKind::Mirror;
    let forward = !mirror && width >= theme.banner_narrow_below();
    ui.painter()
        .rect_filled(rect, 0.0, theme.html_address_bar_bg().to_egui());
    let row_height = theme.html_address_bar_height();
    let mut x = nav_buttons(ui, theme, id, rect, view, forward, &mut out);
    x += theme.html_address_bar_gap() - theme.html_address_bar_nav_gap();
    let field_width = (LogicalPx(rect.right()) - theme.html_address_bar_padding_x() - x)
        .max(theme.html_address_field_min_width());
    let field_rect = egui::Rect::from_min_size(
        egui::pos2(
            x.value(),
            (LogicalPx(rect.top()) + (row_height - theme.input_height()) / 2.0).value(),
        ),
        egui::vec2(field_width.value(), theme.input_height().value()),
    );
    let mut field = ui.new_child(egui::UiBuilder::new().id_salt(id).max_rect(field_rect));
    field.set_clip_rect(ui.clip_rect().intersect(rect));
    let field_id = id.with("address");
    let had_focus = ui.memory(|m| m.has_focus(field_id));
    let was_focused = ui.data(|d| {
        d.get_temp::<bool>(field_id.with("previous_focus"))
            .unwrap_or(false)
    });
    let escape = ui.input(|i| i.key_pressed(egui::Key::Escape));
    let enter = ui.input(|i| i.key_pressed(egui::Key::Enter));
    let clicked_field = ui.input(|i| i.pointer.primary_pressed())
        && ui.rect_contains_pointer(field_rect.intersect(rect))
        && ui.is_enabled();
    if clicked_field && !mirror {
        state.editing = true;
        ui.memory_mut(|m| m.request_focus(field_id));
    }
    if had_focus && ui.input(|i| i.events.iter().any(|e| matches!(e, egui::Event::Key { key, pressed: true, .. } if !matches!(key, egui::Key::Escape | egui::Key::Enter)))) {
        state.error = None;
    }

    if view.focus && !mirror && ui.is_enabled() {
        state.editing = true;
        ui.memory_mut(|m| m.request_focus(field_id));
    }
    let icon = match view.kind {
        HtmlAddressKind::Local => tasty_icons::FILE,
        HtmlAddressKind::Web => tasty_icons::LINK,
        HtmlAddressKind::Empty => tasty_icons::HTML,
        _ => tasty_icons::REMOTE,
    };
    let paint = |ui: &mut egui::Ui, r: egui::Rect, c| {
        let size = theme.icon_glyph_size_sm;
        icon.image(size.value(), c).paint_at(
            ui,
            egui::Rect::from_center_size(r.center(), egui::vec2(size.value(), size.value())),
        );
    };
    // Keep the real buffer intact. The idle label alone is shortened, so focusing/copying
    // always sees the complete address and agent updates cannot rewrite an active draft.
    let idle = !state.editing && !had_focus && !view.focus;
    let text_width =
        field_width - theme.input_padding_x() * 2.0 - theme.icon_glyph_size_sm - theme.input_gap();
    let mut display = if idle {
        left_ellipsis(ui, theme, &state.buffer, text_width)
    } else {
        state.buffer.clone()
    };
    let response = Input::new()
        .id(field_id)
        .width(field_width.value())
        .mono(!display.is_empty())
        .font_size(theme.input_font_size())
        .icon_size(theme.icon_glyph_size_sm)
        .text_margin(LogicalPx(0.0))
        .placeholder(view.labels.placeholder)
        .icon(&paint)
        .read_only(mirror)
        .invalid(state.error.is_some())
        .focus_look(state.editing)
        .text_color(
            if state.editing {
                theme.input_fg()
            } else {
                theme.text_secondary()
            }
            .to_egui(),
        )
        .show(&mut field, theme, &mut display);
    out.field_rect = Some(field_rect);
    if view.focus && !mirror && ui.is_enabled() {
        select_all(ui, field_id, state.buffer.chars().count());
    }
    if response.changed() && !mirror && !idle {
        state.buffer = display;
        state.error = None;
    }
    let focused = ui.memory(|m| m.has_focus(field_id));
    if !mirror {
        if (had_focus || focused || was_focused) && escape {
            state.revert(view.committed_address);
            response.surrender_focus();
            out.action = HtmlAddressAction::Revert;
        } else if had_focus && enter {
            state.editing = true;
            response.request_focus();
            out.action = HtmlAddressAction::Open(state.buffer.clone());
        } else if focused {
            state.editing = true;
        } else if response.lost_focus() && state.error.is_none() {
            state.revert(view.committed_address);
        }
    }
    out.has_keyboard_focus = ui.memory(|m| m.has_focus(field_id));
    ui.data_mut(|d| d.insert_temp(field_id.with("previous_focus"), out.has_keyboard_focus));
    if let Some(error) = &state.error {
        error_row(ui, theme, rect, error);
    }
    ui.painter().hline(
        rect.x_range(),
        rect.bottom() - theme.border_width.value() / 2.0,
        egui::Stroke::new(
            theme.border_width.value(),
            theme.html_address_bar_border().to_egui_premultiplied(),
        ),
    );
    out
}

/// The error row under the field row: alert glyph and one truncated caption line.
fn error_row(ui: &mut egui::Ui, theme: &Theme, rect: egui::Rect, error: &str) {
    let row_height = theme.html_address_bar_height();
    let top = LogicalPx(rect.top()) + row_height;
    let mut row = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(egui::Rect::from_min_max(
                egui::pos2(
                    (LogicalPx(rect.left()) + theme.html_address_bar_padding_x()).value(),
                    top.value(),
                ),
                egui::pos2(
                    (LogicalPx(rect.right()) - theme.html_address_bar_padding_x()).value(),
                    rect.bottom(),
                ),
            ))
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
    );
    row.set_clip_rect(ui.clip_rect().intersect(rect));
    row.spacing_mut().item_spacing.x = theme.spacing_sm.value();
    row.add(tasty_icons::ALERT_CIRCLE.image(
        theme.icon_glyph_size_sm.value(),
        theme.html_address_error_glyph().to_egui(),
    ));
    row.add(
        egui::Label::new(
            egui::RichText::new(error)
                .size(theme.font_size_caption.value())
                .color(theme.text_primary().to_egui()),
        )
        .truncate(),
    );
}

/// Selects the whole address when the band takes focus from a shortcut.
fn select_all(ui: &egui::Ui, field_id: egui::Id, chars: usize) {
    if let Some(mut edit) = egui::TextEdit::load_state(ui.ctx(), field_id) {
        edit.cursor
            .set_char_range(Some(egui::text::CCursorRange::two(
                egui::text::CCursor::new(0),
                egui::text::CCursor::new(chars),
            )));
        edit.store(ui.ctx(), field_id);
    }
}

/// Back, Forward and Reload cells from the left padding. Returns the x after the last cell
/// and its nav gap. Loading shows a Spinner in the Reload cell instead of the button.
fn nav_buttons(
    ui: &mut egui::Ui,
    theme: &Theme,
    id: egui::Id,
    rect: egui::Rect,
    view: &HtmlAddressView<'_>,
    forward: bool,
    out: &mut HtmlAddressOutput,
) -> LogicalPx {
    let mirror = view.kind == HtmlAddressKind::Mirror;
    let row_height = theme.html_address_bar_height();
    let side = LogicalPx(ControlSize::Sm.height(theme));
    let mut x = LogicalPx(rect.left()) + theme.html_address_bar_padding_x();
    let y = LogicalPx(rect.top()) + (row_height - side) / 2.0;
    let buttons = [
        (
            tasty_icons::CHEVRON_LEFT,
            !mirror,
            view.can_back,
            view.labels.back,
            HtmlAddressAction::Back,
        ),
        (
            tasty_icons::CHEVRON_RIGHT,
            forward,
            view.can_forward,
            view.labels.forward,
            HtmlAddressAction::Forward,
        ),
        (
            tasty_icons::REFRESH,
            true,
            !view.disconnected,
            if view.disconnected {
                view.labels.disconnected
            } else {
                view.labels.reload
            },
            HtmlAddressAction::Reload,
        ),
    ];
    for (icon, visible, enabled, label, action) in buttons {
        if !visible {
            continue;
        }
        let cell = egui::Rect::from_min_size(
            egui::pos2(x.value(), y.value()),
            egui::vec2(side.value(), side.value()),
        );
        let mut child = ui.new_child(egui::UiBuilder::new().id_salt((id, label)).max_rect(cell));
        child.set_clip_rect(ui.clip_rect().intersect(rect));
        if action == HtmlAddressAction::Forward {
            out.forward_rect = Some(cell);
        }
        if action == HtmlAddressAction::Reload {
            out.reload_rect = Some(cell);
        }
        if action == HtmlAddressAction::Reload && view.loading {
            let spin = theme.icon_glyph_size_sm;
            Spinner::new().paint_in(
                &mut child,
                theme,
                egui::Rect::from_center_size(cell.center(), egui::vec2(spin.value(), spin.value())),
            );
        } else if nav(&mut child, theme, icon, enabled, label).clicked() {
            out.action = action;
        }
        x += side + theme.html_address_bar_nav_gap();
    }
    x
}
