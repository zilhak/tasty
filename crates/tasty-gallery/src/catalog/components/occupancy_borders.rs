//! `surfaces` specimen — Occupancy & completion borders (ADR-0040, ADR-0062).
//!
//! surface 테두리 = **하나의 시각 채널**. 세 상태가 색으로만 구분된다:
//! - **occupied · soft**: green 1px(`accent-occupied-soft`). 주체(원격 사용자/AI 에이전트)
//!   가 점유하나 write 제한 없음(협조 신호). force-detach 없음.
//! - **occupied · hard**: peach 1px(`accent-occupied-hard`) + readonly(mirror-observe) +
//!   우상단 force-detach. 기존 remote-attach 테두리 흡수.
//! - **completed**: blue 2px(`accent-primary`). `AttentionStore` 의 `AttentionKind::Completion`
//!   레코드가 소스 — 포커스 시 clear. 이 문서 시점 kind 는 `Completion` 1종뿐이라 색은
//!   하나지만, `NeedsInput` kind 추가 시(승인 대기 등) 이 자리에 색이 하나 더 늘어난다
//!   (우선순위·자리는 이 specimen 구조를 그대로 확장 — cluster 하나 추가).
//!
//! 우선순위(소스 규칙, 토큰 아님): 점유 > 완료 — 점유 중 surface 는 완료 테두리 억제.
//! kind 가 늘어도 이 우선순위 축(점유 vs attention)은 별개로 유지된다 — attention 내부
//! kind 우선순위(예: NeedsInput > Completion)는 그 아래 층위의 문제.
//! 본체 렌더는 `egui_panels.rs::draw_occupied_overlays`(soft/hard) +
//! `divider.rs::draw_surface_highlights_view`(completed). 시각 동기화는 수동.

use tasty_type_appearance::theme::Theme;

use crate::catalog::icons;
use crate::catalog::spec::{self, StageVariant, TokenChip};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Soft,
    Hard,
    Done,
}

/// 디자인 occPane — 테두리(tier 색·굵기) + 헤더(label+sub, hard 는 force-detach) + 본문.
fn occ_pane(ui: &mut egui::Ui, theme: &Theme, kind: Kind) {
    let term = theme.surface("terminal");
    let w = theme.field_width_lg.value(); // 200
    let h = theme.spacing_xl.value() * 6.0; // 144
    let (rect, _) = ui.allocate_exact_size(egui::vec2(w, h), egui::Sense::hover());

    // tier 별 테두리 색·굵기 (전부 Theme 토큰).
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
    };
    let border_color = egui::Color32::from(border_color);

    let p = ui.painter_at(rect);
    // 본문 배경 = focused_bg(#000), 테두리 = tier 색.
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

    // 헤더: label(tier 색) + sub(muted). bottom separator + panel bg.
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
    // sub 라벨(mono label 폭 근사 뒤).
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

    // 본문: 프롬프트 두 줄.
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

pub fn draw(ui: &mut egui::Ui, theme: &Theme) {
    spec::stage(ui, theme, StageVariant::Wrap, |ui| {
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

    spec::meta(
        ui,
        theme,
        &[
            ("channel", "surface border — color-only"),
            ("soft", "green 1px — held, writable"),
            ("hard", "peach 1px — readonly + force-detach"),
            ("completed", "blue 2px — clears on focus"),
            (
                "completed kind",
                "AttentionKind::Completion (1 of N — NeedsInput next)",
            ),
            ("priority", "occupancy > completion (source rule)"),
        ],
        &[
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
        ],
    );

    spec::note(
        ui,
        theme,
        "세 상태가 하나의 테두리 채널을 색으로만 나눈다 — 얇은 엣지에서도 서로 \
         혼동되지 않아야 한다. blue(완료)는 green(soft)·peach(hard) 사이에서 가장 \
         구분성이 높다(sky 는 1px 에서 green 과 너무 가까워 배제). 점유 중이면 완료 \
         테두리를 억제해 점유색만 남긴다 — 탭 제목(yellow)·워크스페이스 배지는 불변. \
         completed 클러스터는 `AttentionStore` 의 `AttentionKind::Completion` 레코드를 \
         그린다 — kind 가 하나뿐인 지금은 이 자리도 하나지만, kind 가 늘면(NeedsInput 등) \
         같은 자리에 색이 하나 더 늘어나는 형태로 이 specimen 이 확장된다(cut 금지).",
    );
}
