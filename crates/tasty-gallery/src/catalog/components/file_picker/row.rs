//! 디자인 `FpRow` 한 행: 바탕(선택 surface-active, hover overlay-hover) → 내용 → 선택 막대 → 행 포커스 링.
use super::{Row, cols, elide, row_height};
use crate::catalog::icons::{self, MockGlyph};
use tasty_type_appearance::color::HexColor;
use tasty_type_appearance::theme::Theme;
use tasty_ui_widgets::checkbox;

pub(super) struct RowState {
    pub(super) multi: bool,
    pub(super) checked: bool,
    pub(super) selected: bool,
    /// 행 포커스 — 디자인 `FpRow focus`.
    pub(super) focus: bool,
    /// 포인터가 올라간 행 — 디자인 `FpRow hover`.
    pub(super) hover: bool,
    pub(super) inert: bool,
    pub(super) file_tooltip: &'static str,
}

/// 디자인 `FpRow` 의 `ink(rest, restHover, sel, selHover)`. inert 행은 늘 `fp-folder-file-fg` 다.
fn row_ink(
    theme: &Theme,
    state: tasty_ui_widgets::RowState,
    inert: bool,
    [rest, rest_hover, sel, sel_hover]: [HexColor; 4],
) -> egui::Color32 {
    use tasty_ui_widgets::RowState as S;
    if inert {
        return theme.fp_folder_file_fg().to_egui();
    }
    match state {
        S::SelectedHover | S::SelectedPress => sel_hover,
        S::Selected => sel,
        S::Hover | S::Press => rest_hover,
        S::Rest | S::DropTarget | S::Disabled => rest,
    }
    .to_egui()
}

/// 디자인 `FpRow`: 바탕(선택 surface-active, hover overlay-hover) → 내용 → 선택 막대 → 행 포커스 링 순서로 그린다.
pub(super) fn row(ui: &mut egui::Ui, theme: &Theme, r: &Row, state: RowState) {
    let RowState {
        multi,
        checked,
        selected,
        focus,
        hover,
        inert,
        file_tooltip,
    } = state;
    let w = ui.available_width();
    let h = row_height(theme);
    let (rect, response) = ui.allocate_exact_size(egui::vec2(w, h.value()), egui::Sense::hover());
    if inert {
        response.on_hover_text(file_tooltip);
    }
    let st = tasty_ui_widgets::RowState::of(true, selected, hover && !inert, false);
    st.paint_fill(ui.painter(), rect, 0.0, theme);
    let c = cols(rect, theme, multi);
    let glyph_size = theme.icon_glyph_size_md.value();
    if let Some(cx) = c.checkbox_x {
        let mut chk = checked;
        let cb_rect = egui::Rect::from_min_size(
            egui::pos2(cx.value(), rect.center().y - glyph_size * 0.5),
            egui::vec2(glyph_size, glyph_size),
        );
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(cb_rect));
        checkbox(
            &mut child,
            theme,
            &mut chk,
            "",
            true,
            tasty_ui_widgets::Ground::Panel,
        );
    }
    let (icon_glyph, icon_color): (MockGlyph, egui::Color32) = if r.folder {
        (
            icons::FOLDER,
            row_ink(
                theme,
                st,
                inert,
                [
                    theme.accent_primary_raised(),
                    theme.accent_primary(),
                    theme.accent_primary_raised(),
                    theme.accent_primary_on_active(),
                ],
            ),
        )
    } else {
        (
            icons::FILE,
            row_ink(
                theme,
                st,
                inert,
                [
                    theme.text_muted(),
                    theme.fp_row_glyph_fg_hover(),
                    theme.text_muted(),
                    theme.text_on_active(),
                ],
            ),
        )
    };
    let ir = egui::Rect::from_min_size(
        egui::pos2(c.icon_x.value(), rect.center().y - glyph_size * 0.5),
        egui::vec2(glyph_size, glyph_size),
    );
    icon_glyph.image(glyph_size, icon_color).paint_at(ui, ir);
    let name_color = row_ink(
        theme,
        st,
        inert,
        [
            theme.text_secondary(),
            theme.text_secondary_raised(),
            theme.text_on_active(),
            theme.text_on_active_hover(),
        ],
    );
    let name_font = egui::FontId::proportional(theme.font_size_body.value());
    let name_text = elide(ui, r.name, name_font.clone(), c.name_right - c.name_left);
    ui.painter().text(
        egui::pos2(c.name_left.value(), rect.center().y),
        egui::Align2::LEFT_CENTER,
        name_text,
        name_font,
        name_color,
    );
    let mono_caption = egui::FontId::monospace(theme.font_size_caption.value());
    let meta = row_ink(
        theme,
        st,
        inert,
        [
            theme.text_muted(),
            theme.fp_row_meta_fg_hover(),
            theme.text_on_active(),
            theme.text_on_active_hover(),
        ],
    );
    ui.painter().text(
        egui::pos2(c.size_right.value(), rect.center().y),
        egui::Align2::RIGHT_CENTER,
        r.size,
        mono_caption.clone(),
        meta,
    );
    ui.painter().text(
        egui::pos2(c.mod_right.value(), rect.center().y),
        egui::Align2::RIGHT_CENTER,
        r.modified,
        mono_caption,
        meta,
    );
    if selected {
        let bar = egui::Rect::from_min_size(
            rect.min,
            egui::vec2(theme.selection_edge_width.value(), rect.height()),
        );
        ui.painter()
            .rect_filled(bar, 0.0, theme.accent_primary_on_active().to_egui());
    }
    if focus && !inert {
        let ring = match (selected, hover) {
            (true, true) => theme.fp_row_focus_ring_selected_hover(),
            (true, false) => theme.fp_row_focus_ring_selected(),
            (false, _) => theme.fp_row_focus_ring(),
        };
        ui.painter().rect_stroke(
            rect,
            theme.corner_radius_sm.value(),
            egui::Stroke::new(theme.fp_row_focus_ring_width().value(), ring.to_egui()),
            egui::StrokeKind::Inside,
        );
    }
}
