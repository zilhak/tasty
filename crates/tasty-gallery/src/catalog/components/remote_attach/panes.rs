//! Profile and workspace pane assembly, including connection states.

use super::new_row::new_ws_row;
use super::rows::{empty_line, profile_row, ws_row};
use super::{CAPS_HEADER_H, LEFT_W, NewRow, PROFILES, RaState, WORKSPACES, Ws};
use crate::catalog::icons::{self, MockGlyph};
use crate::catalog::widgets::dialog as kit;
use tasty_type_appearance::theme::Theme;
use tasty_type_geometry::length::LogicalPx;
use tasty_ui_widgets::tokens::{
    CENTER_BLOCK_H_SPECIMEN as EMPTY_BLOCK_H, CENTER_GLYPH_SIZE as EMPTY_GLYPH,
};
use tasty_ui_widgets::{Button, ButtonVariant, Spinner};

pub(super) fn left_pane(ui: &mut egui::Ui, theme: &Theme, rect: egui::Rect, state: RaState) {
    let mut col = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(rect)
            .layout(egui::Layout::top_down(egui::Align::Min)),
    );
    col.spacing_mut().item_spacing = egui::vec2(0.0, 0.0);
    let hdr =
        egui::Rect::from_min_size(rect.min, egui::vec2(LEFT_W.value(), CAPS_HEADER_H.value()));
    let (_, _) = col.allocate_exact_size(
        egui::vec2(LEFT_W.value(), CAPS_HEADER_H.value()),
        egui::Sense::hover(),
    );
    col.painter().text(
        egui::pos2(
            hdr.left() + theme.spacing_md.value(),
            hdr.top() + theme.spacing_md.value(),
        ),
        egui::Align2::LEFT_TOP,
        "ATTACH PROFILES",
        egui::FontId::monospace(theme.font_size_micro.value()),
        theme.text_muted().to_egui(),
    );
    let sel_name = match state {
        RaState::Error => "legacy-attach",
        RaState::Connecting => "gb10",
        RaState::Empty => "media-nas",
        _ => "prod-web",
    };
    for p in PROFILES {
        profile_row(&mut col, theme, p, p.name == sel_name);
    }
}

pub(super) fn right_pane(ui: &mut egui::Ui, theme: &Theme, rect: egui::Rect, state: RaState) {
    match state {
        // 빈 목록은 새 워크스페이스 행을 미리 선택해 생성할 수 있게 한다.
        RaState::Loaded => loaded_pane(
            ui,
            theme,
            rect,
            "prod-web",
            NewRow::Rest,
            WORKSPACES,
            Some("agents-prod"),
        ),
        RaState::Empty => loaded_pane(ui, theme, rect, "media-nas", NewRow::Selected, &[], None),
        RaState::Initial => center_state(
            ui,
            theme,
            rect,
            icons::REMOTE,
            theme.text_placeholder().to_egui(),
            false,
            "Select an attach profile",
            theme.text_muted(),
            "Pick a profile on the left to connect and list the remote instance's workspaces.",
            false,
        ),
        RaState::Connecting => center_state(
            ui,
            theme,
            rect,
            icons::REMOTE,
            theme.text_placeholder().to_egui(),
            true,
            "Connecting…",
            theme.text_secondary(),
            "Establishing the SSH tunnel to gb10 and listing workspaces. If the host does not \
             respond, the lookup stops on its own after 20s.",
            false,
        ),
        // 본체와 같은 원격 인스턴스 미실행 오류 예제. 내부 stderr·포트 경로는 표시하지 않는다.
        RaState::Error => center_state(
            ui,
            theme,
            rect,
            icons::ALERT_TRIANGLE,
            theme.accent_danger().to_egui(),
            false,
            "Can't connect",
            theme.text_primary(),
            "No tasty instance appears to be running on the remote host.",
            true,
        ),
    }
}

#[allow(clippy::too_many_arguments)]
fn center_state(
    ui: &mut egui::Ui,
    theme: &Theme,
    rect: egui::Rect,
    glyph: MockGlyph,
    glyph_color: egui::Color32,
    spinner: bool,
    heading: &str,
    heading_color: tasty_type_appearance::color::HexColor,
    caption: &str,
    retry: bool,
) {
    let mut col = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(rect.shrink2(egui::vec2(
                theme.spacing_lg.value(),
                theme.spacing_xl.value(),
            )))
            .layout(egui::Layout::top_down(egui::Align::Center)),
    );
    col.add_space((rect.height() - EMPTY_BLOCK_H).max(0.0) * 0.5);
    col.spacing_mut().item_spacing.y = theme.spacing_sm.value();
    if spinner {
        Spinner::new().size(EMPTY_GLYPH).show(&mut col, theme);
    } else {
        kit::icon(&mut col, glyph, LogicalPx(EMPTY_GLYPH), glyph_color);
    }
    col.label(
        egui::RichText::new(heading)
            .size(theme.font_size_body.value())
            .strong()
            .color(heading_color.to_egui()),
    );
    col.label(
        egui::RichText::new(caption)
            .size(theme.font_size_caption.value())
            .color(theme.text_muted().to_egui()),
    );
    if retry {
        col.add_space(theme.spacing_xs.value());
        Button::new("Retry")
            .variant(ButtonVariant::Secondary)
            .leading_icon(&|ui, rect, c| icons::REFRESH.image(rect.height(), c).paint_at(ui, rect))
            .show(&mut col, theme);
    }
}

/// 목록이 비어도 헤더와 새 워크스페이스 행을 표시한다.
fn loaded_pane(
    ui: &mut egui::Ui,
    theme: &Theme,
    rect: egui::Rect,
    profile: &str,
    new_row: NewRow,
    ws: &[Ws],
    sel_ws: Option<&str>,
) {
    let mut col = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(rect)
            .layout(egui::Layout::top_down(egui::Align::Min)),
    );
    col.spacing_mut().item_spacing = egui::vec2(0.0, 0.0);
    let (hdr, _) = col.allocate_exact_size(
        egui::vec2(rect.width(), CAPS_HEADER_H.value()),
        egui::Sense::hover(),
    );
    let base_x = hdr.left() + theme.spacing_md.value();
    let y = hdr.top() + theme.spacing_md.value();
    let caps = col.painter().layout_no_wrap(
        "REMOTE WORKSPACES".to_owned(),
        egui::FontId::monospace(theme.font_size_micro.value()),
        theme.text_muted().to_egui(),
    );
    let caps_w = caps.rect.width();
    col.painter()
        .galley(egui::pos2(base_x, y), caps, theme.text_muted().to_egui());
    col.painter().text(
        egui::pos2(base_x + caps_w + theme.spacing_sm.value(), y),
        egui::Align2::LEFT_TOP,
        format!("· {profile}"),
        egui::FontId::proportional(theme.font_size_caption.value()),
        theme.text_muted().to_egui(),
    );
    new_ws_row(&mut col, theme, new_row);
    if ws.is_empty() {
        empty_line(&mut col, theme, profile);
    } else {
        for w in ws {
            ws_row(&mut col, theme, w, sel_ws == Some(w.name));
        }
    }
}
