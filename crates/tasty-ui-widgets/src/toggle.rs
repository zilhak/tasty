//! bool 상태를 바꾸는 체크박스와 스위치. 변경하면 response.changed로 알린다.
//! 상태는 즉시 표시하며 키보드 포커스 테두리는 구현하지 않는다.

use tasty_type_appearance::theme::Theme;

/// 체크마크 선 굵기. 창 버튼용 icon_stroke_width와 역할이 달라 별도로 둔다.
const CHECK_STROKE: f32 = 2.0;

/// 체크박스·간격·라벨의 전체 폭. 메뉴 등에서 필요한 폭을 먼저 계산할 때 사용한다.
pub fn checkbox_width(ui: &egui::Ui, theme: &Theme, label: &str) -> f32 {
    let galley = ui.fonts(|f| {
        f.layout_no_wrap(
            label.to_owned(),
            egui::FontId::proportional(theme.font_size_body.value()),
            egui::Color32::PLACEHOLDER,
        )
    });
    theme.checkbox_size().value() + theme.spacing_sm.value() + galley.rect.width()
}

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

    // 상자가 차지한 폭을 제외한 공간에 맞춰 라벨을 줄인다.
    let label_max = (ui.available_width() - box_sz - gap).max(0.0);
    let mut job = egui::text::LayoutJob::simple_singleline(
        label.to_owned(),
        egui::FontId::proportional(body),
        egui::Color32::PLACEHOLDER,
    );
    job.wrap = egui::text::TextWrapping::truncate_at_width(label_max);
    let galley = ui.fonts(|f| f.layout_job(job));
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
        // 체크마크 — box 중앙 아이콘 가족 xs(12) 영역에 꺾은선 2 segment.
        let glyph = theme.icon_glyph_size_xs.value();
        let o = box_rect.center() - egui::vec2(glyph, glyph) * 0.5;
        let p = |fx: f32, fy: f32| o + egui::vec2(glyph * fx, glyph * fy);
        let stroke = egui::Stroke::new(CHECK_STROKE, dim(theme.checkbox_check_fg().to_egui()));
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
    // 완전히 둥근 트랙이 되도록 높이의 절반을 반경으로 사용한다.
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
