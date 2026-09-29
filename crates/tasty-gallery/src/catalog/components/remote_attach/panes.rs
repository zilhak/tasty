//! Profile and workspace pane assembly, including connection states.

use super::new_row::new_ws_row;
use super::rows::{empty_line, profile_row, ws_row};
use super::{CAPS_HEADER_H, LEFT_W, NewRow, PROFILES, RaState, WORKSPACES, Ws};
use crate::catalog::icons;
use tasty_type_appearance::theme::Theme;
use tasty_ui_widgets::CenterState;

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
        RaState::Initial => {
            CenterState::empty(icons::REMOTE, "Select an attach profile")
                .sub_line(Some(
                    "Pick a profile on the left to connect and list the remote instance's workspaces.",
                ))
                .show_in(ui, theme, rect);
        }
        RaState::Connecting => {
            CenterState::loading("Connecting…")
                .sub_line(Some(
                    "Establishing the SSH tunnel to gb10 and listing workspaces. If the host does not \
                     respond, the lookup stops on its own after 20s.",
                ))
                .show_in(ui, theme, rect);
        }
        // 본체와 같은 원격 인스턴스 미실행 오류 예제. 내부 stderr·포트 경로는 표시하지 않는다.
        RaState::Error => {
            CenterState::error("Can't connect")
                .sub_line(Some(
                    "No tasty instance appears to be running on the remote host.",
                ))
                .action("Retry", Some(icons::REFRESH))
                .show_in(ui, theme, rect);
        }
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
