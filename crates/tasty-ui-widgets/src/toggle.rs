//! 토글 primitive — `Checkbox` / `Switch` (디자인 `components/forms/*`).
//!
//! 상호작용 컨트롤: `&mut bool` 을 받아 클릭 시 토글하고 `response.changed()` 로
//! 알린다. disabled 는 opacity 0.5. checked 외형은 즉시(기능) — Motion 계약.
//! egui 한계: focus-visible outline 은 키보드 포커스가 드물어 생략(장식).

use tasty_type_appearance::theme::Theme;

/// 체크마크 글리프 영역(box 내부). 대응 checkbox component 토큰 없음 → Rust-only.
const CHECK_GLYPH: f32 = 12.0;

/// Checkbox — 16px 박스 + 라벨. 클릭 시 토글.
pub fn checkbox(
    ui: &mut egui::Ui,
    theme: &Theme,
    checked: &mut bool,
    label: &str,
    enabled: bool,
) -> egui::Response {
    // gap(라벨)·body 는 대응 checkbox component 토큰 없음 → semantic.
    let gap = theme.spacing_sm.value();
    let body = theme.font_size_body.value();
    let radius = theme.checkbox_radius().value();
    let bw = theme.border_width.value();
    let box_sz = theme.checkbox_size().value();

    let galley = ui.painter().layout_no_wrap(
        label.to_owned(),
        egui::FontId::proportional(body),
        egui::Color32::PLACEHOLDER,
    );
    let h = box_sz.max(galley.rect.height());
    let w = box_sz + gap + galley.rect.width();
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
        egui::pos2(rect.left(), rect.center().y - box_sz * 0.5),
        egui::vec2(box_sz, box_sz),
    );
    // checked 는 accent 채움(checkbox-bg-checked)이 fill=border 를 겸한다(별도
    // checkbox-border-checked 토큰 없음). unchecked 는 checkbox-bg/-border.
    let (fill, border) = if *checked {
        (
            theme.checkbox_bg_checked().to_egui(),
            theme.checkbox_bg_checked().to_egui(),
        )
    } else {
        (
            theme.checkbox_bg().to_egui(),
            theme.checkbox_border().to_egui(),
        )
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
        let stroke = egui::Stroke::new(2.0, dim(theme.checkbox_check_fg().to_egui()));
        ui.painter()
            .line_segment([p(0.22, 0.55), p(0.42, 0.74)], stroke);
        ui.painter()
            .line_segment([p(0.42, 0.74), p(0.80, 0.30)], stroke);
    }
    let label_pos = egui::pos2(
        rect.left() + box_sz + gap,
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
    // gap(라벨)·body 는 대응 switch component 토큰 없음 → semantic.
    let gap = theme.spacing_sm.value();
    let body = theme.font_size_body.value();
    let bw = theme.border_width.value();
    let track_w = theme.switch_track_width().value();
    let track_h = theme.switch_track_height().value();
    let thumb_sz = theme.switch_thumb_size().value();
    let thumb_inset = theme.switch_thumb_inset().value();

    let galley = label.map(|l| {
        ui.painter().layout_no_wrap(
            l.to_owned(),
            egui::FontId::proportional(body),
            egui::Color32::PLACEHOLDER,
        )
    });
    let label_w = galley.as_ref().map(|g| gap + g.rect.width()).unwrap_or(0.0);
    let h = track_h.max(galley.as_ref().map(|g| g.rect.height()).unwrap_or(0.0));
    let sense = if enabled {
        egui::Sense::click()
    } else {
        egui::Sense::hover()
    };
    let (rect, mut resp) = ui.allocate_exact_size(egui::vec2(track_w + label_w, h), sense);
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
        egui::pos2(rect.left(), rect.center().y - track_h * 0.5),
        egui::vec2(track_w, track_h),
    );
    // checked on-track 은 switch-track-bg-on 이 fill=border 겸함. unchecked 는
    // switch-track-bg + border-default(switch track-border 토큰 없음 → semantic).
    let (track_fill, track_border) = if *checked {
        (
            theme.switch_track_bg_on().to_egui(),
            theme.switch_track_bg_on().to_egui(),
        )
    } else {
        (
            theme.switch_track_bg().to_egui(),
            theme.border_default().to_egui(),
        )
    };
    // 트랙 radius = pill(height/2 idiom). switch-radius 는 sentinel 9999 라 구현
    // 관습을 유지(값 불일치 → 이식 제외).
    ui.painter().rect(
        track,
        track_h * 0.5,
        dim(track_fill),
        egui::Stroke::new(bw, dim(track_border)),
        egui::StrokeKind::Inside,
    );
    let thumb_x = if *checked {
        track.right() - thumb_inset - thumb_sz * 0.5
    } else {
        track.left() + thumb_inset + thumb_sz * 0.5
    };
    let thumb_color = if *checked {
        theme.switch_thumb_bg_on().to_egui()
    } else {
        theme.switch_thumb_bg().to_egui()
    };
    ui.painter().circle_filled(
        egui::pos2(thumb_x, track.center().y),
        thumb_sz * 0.5,
        dim(thumb_color),
    );

    if let Some(g) = galley {
        let pos = egui::pos2(track.right() + gap, rect.center().y - g.rect.height() * 0.5);
        ui.painter()
            .galley(pos, g, dim(theme.text_primary().to_egui()));
    }
    resp
}
