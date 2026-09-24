//! 점유·응답 대기·완료에 따른 테두리 예제.
//! 우선순위는 NeedsInput > 점유 > Completion이다. 응답 대기는 점유 중에도 보여야 한다.
//! soft 점유는 쓰기를 허용하며 hard 점유는 읽기 전용과 강제 연결 해제를 표시한다.
//! Completion과 NeedsInput은 포커스를 받으면 지워진다. 본체와의 시각적 일치는 직접 확인한다.

use tasty_type_appearance::theme::Theme;
use tasty_type_geometry::length::LogicalPx;
use tasty_ui_widgets::{Button, ButtonVariant};

use crate::catalog::icons;
use crate::catalog::spec::{self, StageVariant, TokenChip};
use crate::catalog::widgets::dialog as kit;

/// 강제 끊기 확인 다이얼로그 폭 (destructive confirm 공통 380px).
const CONFIRM_WIDTH: LogicalPx = LogicalPx(380.0);

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Soft,
    Hard,
    Done,
    NeedsInput,
}

/// 디자인 occPane — 테두리(tier 색·굵기) + 헤더(label+sub, hard 는 force-detach) + 본문.
fn occ_pane(ui: &mut egui::Ui, theme: &Theme, kind: Kind) {
    let term = theme.surface("terminal");
    let w = theme.field_width_lg.value(); // 200
    let h = theme.spacing_xl.value() * 6.0; // 144
    let (rect, _) = ui.allocate_exact_size(egui::vec2(w, h), egui::Sense::hover());

    let (border_color, border_w, label, sub, readonly) = match kind {
        Kind::Soft => (
            theme.accent_occupied_soft(),
            theme.border_width.value(),
            "occupied · soft",
            "agent holds — writable",
            false,
        ),
        Kind::Hard => (
            theme.accent_occupied_hard(),
            theme.border_width.value(),
            "occupied · hard",
            "readonly · mirror-observe",
            true,
        ),
        Kind::Done => (
            theme.accent_primary(),
            theme.focus_ring_width.value(),
            "completed",
            "clears on focus",
            false,
        ),
        Kind::NeedsInput => (
            theme.accent_warning(),
            theme.focus_ring_width.value(),
            "needs-input",
            "clears on focus",
            false,
        ),
    };
    let border_color = egui::Color32::from(border_color);

    let p = ui.painter_at(rect);
    p.rect_filled(
        rect,
        theme.corner_radius.value(),
        egui::Color32::from(term.focused_bg),
    );
    p.rect_stroke(
        rect,
        theme.corner_radius.value(),
        egui::Stroke::new(border_w, border_color),
        egui::StrokeKind::Inside,
    );

    let pad = theme.spacing_sm.value();
    let header_h = theme.status_dot_size.value() + pad * 2.0;
    let header_rect =
        egui::Rect::from_min_size(rect.min, egui::vec2(rect.width(), header_h)).shrink(border_w);
    p.rect_filled(header_rect, 0.0, egui::Color32::from(theme.bg_panel()));
    p.hline(
        header_rect.x_range(),
        header_rect.max.y,
        egui::Stroke::new(
            theme.border_width.value(),
            egui::Color32::from(theme.border_default()),
        ),
    );
    let label_pos = egui::pos2(header_rect.min.x + pad, header_rect.center().y);
    p.text(
        label_pos,
        egui::Align2::LEFT_CENTER,
        label,
        egui::FontId::monospace(theme.font_size_micro.value()),
        border_color,
    );
    let label_w = label.len() as f32 * theme.font_size_micro.value() * 0.6;
    p.text(
        egui::pos2(
            label_pos.x + label_w + theme.spacing_xs.value(),
            label_pos.y,
        ),
        egui::Align2::LEFT_CENTER,
        format!("· {sub}"),
        egui::FontId::proportional(theme.font_size_micro.value()),
        egui::Color32::from(theme.text_muted()),
    );

    let body_y = header_rect.max.y + theme.spacing_md.value();
    let mono = egui::FontId::monospace(theme.font_size_term_sm.value());
    p.text(
        egui::pos2(rect.min.x + pad, body_y),
        egui::Align2::LEFT_TOP,
        "~/tasty main",
        mono.clone(),
        egui::Color32::from(theme.accent_success()),
    );
    let line2 = match kind {
        Kind::Soft => "❯ running tests…",
        Kind::Hard => "❯ mirror (readonly)",
        Kind::Done => "❯ build passed ✓",
        Kind::NeedsInput => "❯ proceed? (y/n)",
    };
    let fg = if readonly {
        egui::Color32::from(term.unfocused_fg)
    } else {
        egui::Color32::from(term.focused_fg)
    };
    p.text(
        egui::pos2(
            rect.min.x + pad,
            body_y + theme.font_size_term_sm.value() + theme.spacing_xs.value(),
        ),
        egui::Align2::LEFT_TOP,
        line2,
        mono,
        fg,
    );

    // hard 한정: 우상단 force-detach 버튼(× 아이콘, peach).
    if readonly {
        let sz = theme.icon_glyph_size_sm.value();
        let btn_rect = egui::Rect::from_min_size(
            egui::pos2(
                header_rect.max.x - sz - pad,
                header_rect.center().y - sz * 0.5,
            ),
            egui::vec2(sz, sz),
        );
        icons::CLOSE.image(sz, border_color).paint_at(ui, btn_rect);
    }
}

/// 점유자의 숫자 client ID 대신 워크스페이스 이름과 연결 해제 결과를 안내한다.
fn force_detach_confirm(ui: &mut egui::Ui, theme: &Theme) {
    kit::frame_card(ui, theme, CONFIRM_WIDTH, kit::panel_fill(theme), |ui| {
        kit::region_sym(ui, theme.spacing_md, theme.spacing_md, |ui| {
            ui.spacing_mut().item_spacing.y = theme.spacing_sm.value();
            ui.horizontal(|ui| {
                kit::icon(
                    ui,
                    icons::CLOSE,
                    theme.icon_glyph_size_md,
                    theme.accent_danger().to_egui(),
                );
                kit::title(ui, theme, "Force detach this workspace?");
            });
            kit::body(
                ui,
                theme,
                "Services is attached by a client. Detaching ends that session and clears \
                 its mirror. The workspace becomes editable and closable again.",
            );
            ui.horizontal(|ui| {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    Button::new("Force detach")
                        .variant(ButtonVariant::Danger)
                        .show(ui, theme);
                    Button::new("Cancel")
                        .variant(ButtonVariant::Ghost)
                        .show(ui, theme);
                });
            });
        });
    });
}

pub fn draw(ui: &mut egui::Ui, theme: &Theme) {
    spec::stage(ui, theme, StageVariant::Wrap, |ui| {
        spec::cluster(ui, theme, "needs-input · yellow 2px", |ui| {
            occ_pane(ui, theme, Kind::NeedsInput)
        });
        spec::cluster(ui, theme, "soft · green 1px", |ui| {
            occ_pane(ui, theme, Kind::Soft)
        });
        spec::cluster(ui, theme, "hard · peach 1px", |ui| {
            occ_pane(ui, theme, Kind::Hard)
        });
        spec::cluster(ui, theme, "completed · blue 2px", |ui| {
            occ_pane(ui, theme, Kind::Done)
        });
    });

    spec::stage(ui, theme, StageVariant::Wrap, |ui| {
        spec::cluster(ui, theme, "force-detach confirm · 380px", |ui| {
            force_detach_confirm(ui, theme)
        });
    });

    spec::meta(
        ui,
        theme,
        &[
            ("channel", "surface border — color-only"),
            ("needs-input", "yellow 2px — clears on focus, wins ties"),
            ("soft", "green 1px — held, writable"),
            ("hard", "peach 1px — readonly + force-detach"),
            (
                "force-detach confirm",
                "380px destructive — sidebar entry, no holder identity",
            ),
            ("completed", "blue 2px — clears on focus"),
            (
                "kind source",
                "AttentionStore::AttentionKind (Completion/NeedsInput)",
            ),
            (
                "priority",
                "needs-input > occupancy > completion (source rule)",
            ),
        ],
        &[
            TokenChip::new(
                "accent-warning",
                "needs-input edge (→ yellow)",
                theme.accent_warning().into(),
            ),
            TokenChip::new(
                "accent-occupied-soft",
                "soft edge (→ green)",
                theme.accent_occupied_soft().into(),
            ),
            TokenChip::new(
                "accent-occupied-hard",
                "hard edge (→ peach)",
                theme.accent_occupied_hard().into(),
            ),
            TokenChip::new(
                "accent-primary",
                "completion edge (→ blue)",
                theme.accent_primary().into(),
            ),
            TokenChip::new(
                "accent-danger",
                "force-detach confirm",
                theme.accent_danger().into(),
            ),
        ],
    );

    spec::note(
        ui,
        theme,
        "테두리는 응답 대기, 점유, 완료 순서로 표시한다. 점유 중에는 완료 테두리를 숨기지만 응답 대기는 계속 알린다. 탭 제목과 워크스페이스 배지도 응답 대기를 완료보다 우선한다. 강제 연결 해제는 서피스의 × 또는 사이드바 우클릭 메뉴에서 시작하며, 확인창은 워크스페이스 이름과 연결 해제 결과를 안내한다.",
    );
}
