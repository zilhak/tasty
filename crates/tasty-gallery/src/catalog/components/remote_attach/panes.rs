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
    // caps 헤더 — padding 10/12/4.
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
    // 선택 규칙(디자인 미러): loaded → prod-web, error → legacy-attach, else prod-web.
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
        // loaded / empty 는 같은 경로 — ws 목록의 길이만 다르다. empty 는 새 행을
        // 미리 선택해 두어 pane 이 뜬 순간부터 footer 가 살아 있다.
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
        // 실제 에러 클래스와 동기화(갤러리 완전성 정책) — `PortDiscoveryFailureKind::
        // RemoteInstanceNotRunning` (`crates/tasty-ssh/src/lib.rs`), 문구는
        // `lang/en.toml` `ssh.port_discovery.instance_not_running` 과 동일. 원격
        // stderr/포트 파일 경로 같은 내부 구현은 노출하지 않는다.
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
    // 세로 중앙 정렬 — 위쪽 여백을 대략 반으로.
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

/// caps 헤더 + "+ New workspace" 행 + (ws 목록 | empty 한 줄).
///
/// 렌더 분기가 하나뿐이라 `ws` 가 비어도 이 경로를 그대로 탄다 — 목록이 비는 것은
/// "행이 하나인 목록"이지 다른 화면이 아니다.
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
    // caps 헤더 — "REMOTE WORKSPACES · {profile}". 생성이라는 사실은 행 라벨이 말하므로
    // 그룹을 설명하는 이 문구는 새 행이 생겨도 그대로다.
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
