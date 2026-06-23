//! 토글 primitive — `Checkbox` / `Switch` (디자인 `components/forms/*`).
//!
//! 상호작용 컨트롤: `&mut bool` 을 받아 클릭 시 토글하고 `response.changed()` 로
//! 알린다. disabled 는 opacity 0.5. checked 외형은 즉시(기능) — Motion 계약.
//! egui 한계: focus-visible outline 은 키보드 포커스가 드물어 생략(장식).

use tasty_type_appearance::theme::Theme;

const BOX: f32 = 16.0;
const CHECK_GLYPH: f32 = 12.0;
// Switch track 28×16 (token-policy: on-grid; 이전 32×18 은 off-grid 18 포함).
const SWITCH_W: f32 = 28.0;
const SWITCH_H: f32 = 16.0;
const SWITCH_THUMB: f32 = 12.0;
const SWITCH_INSET: f32 = 2.0;

/// Checkbox — 16px 박스 + 라벨. 클릭 시 토글.
pub fn checkbox(
    ui: &mut egui::Ui,
    theme: &Theme,
    checked: &mut bool,
    label: &str,
    enabled: bool,
) -> egui::Response {
    let gap = theme.spacing_sm.value();
    let body = theme.font_size_body.value();
    let radius = theme.corner_radius_sm.value();
    let bw = theme.border_width.value();

    let galley = ui.painter().layout_no_wrap(
        label.to_owned(),
        egui::FontId::proportional(body),
        egui::Color32::PLACEHOLDER,
    );
    let h = BOX.max(galley.rect.height());
    let w = BOX + gap + galley.rect.width();
    let sense = if enabled {
        egui::Sense::click()
    } else {
        egui::Sense::hover()
    };
    let (rect, mut resp) = ui.allocate_exact_size(egui::vec2(w, h), sense);
    if resp.clicked() {
        *checked = !*checked;
        resp.mark_changed();
    }

    let dim = |c: egui::Color32| {
        if enabled {
            c
        } else {
            c.gamma_multiply(theme.opacity_disabled())
        }
    };
    let box_rect = egui::Rect::from_min_size(
        egui::pos2(rect.left(), rect.center().y - BOX * 0.5),
        egui::vec2(BOX, BOX),
    );
    let (fill, border) = if *checked {
        (theme.accent_primary().to_egui(), theme.accent_primary().to_egui())
    } else {
        (theme.surface_raised().to_egui(), theme.border_strong().to_egui())
    };
    ui.painter().rect(
        box_rect,
        radius,
        dim(fill),
        egui::Stroke::new(bw, dim(border)),
        egui::StrokeKind::Inside,
    );
    if *checked {
        // 체크마크 — box 중앙 12px 영역에 꺾은선 2 segment.
        let o = box_rect.center() - egui::vec2(CHECK_GLYPH, CHECK_GLYPH) * 0.5;
        let p = |fx: f32, fy: f32| o + egui::vec2(CHECK_GLYPH * fx, CHECK_GLYPH * fy);
        let stroke = egui::Stroke::new(2.0, dim(theme.text_on_accent().to_egui()));
        ui.painter().line_segment([p(0.22, 0.55), p(0.42, 0.74)], stroke);
        ui.painter().line_segment([p(0.42, 0.74), p(0.80, 0.30)], stroke);
    }
    let label_pos = egui::pos2(
        rect.left() + BOX + gap,
        rect.center().y - galley.rect.height() * 0.5,
    );
    ui.painter()
        .galley(label_pos, galley, dim(theme.text_primary().to_egui()));
    resp
}

/// Switch — 28×16 토글 트랙 + 라벨(옵션). 클릭 시 토글.
pub fn switch(
    ui: &mut egui::Ui,
    theme: &Theme,
    checked: &mut bool,
    label: Option<&str>,
    enabled: bool,
) -> egui::Response {
    let gap = theme.spacing_sm.value();
    let body = theme.font_size_body.value();
    let bw = theme.border_width.value();

    let galley = label.map(|l| {
        ui.painter().layout_no_wrap(
            l.to_owned(),
            egui::FontId::proportional(body),
            egui::Color32::PLACEHOLDER,
        )
    });
    let label_w = galley.as_ref().map(|g| gap + g.rect.width()).unwrap_or(0.0);
    let h = SWITCH_H.max(galley.as_ref().map(|g| g.rect.height()).unwrap_or(0.0));
    let sense = if enabled {
        egui::Sense::click()
    } else {
        egui::Sense::hover()
    };
    let (rect, mut resp) = ui.allocate_exact_size(egui::vec2(SWITCH_W + label_w, h), sense);
    if resp.clicked() {
        *checked = !*checked;
        resp.mark_changed();
    }

    let dim = |c: egui::Color32| {
        if enabled {
            c
        } else {
            c.gamma_multiply(theme.opacity_disabled())
        }
    };
    let track = egui::Rect::from_min_size(
        egui::pos2(rect.left(), rect.center().y - SWITCH_H * 0.5),
        egui::vec2(SWITCH_W, SWITCH_H),
    );
    let (track_fill, track_border) = if *checked {
        (theme.accent_primary().to_egui(), theme.accent_primary().to_egui())
    } else {
        (theme.surface_active().to_egui(), theme.border_default().to_egui())
    };
    ui.painter().rect(
        track,
        SWITCH_H * 0.5,
        dim(track_fill),
        egui::Stroke::new(bw, dim(track_border)),
        egui::StrokeKind::Inside,
    );
    let thumb_x = if *checked {
        track.right() - SWITCH_INSET - SWITCH_THUMB * 0.5
    } else {
        track.left() + SWITCH_INSET + SWITCH_THUMB * 0.5
    };
    let thumb_color = if *checked {
        theme.text_on_accent().to_egui()
    } else {
        theme.subtext0.to_egui()
    };
    ui.painter()
        .circle_filled(egui::pos2(thumb_x, track.center().y), SWITCH_THUMB * 0.5, dim(thumb_color));

    if let Some(g) = galley {
        let pos = egui::pos2(
            track.right() + gap,
            rect.center().y - g.rect.height() * 0.5,
        );
        ui.painter().galley(pos, g, dim(theme.text_primary().to_egui()));
    }
    resp
}
