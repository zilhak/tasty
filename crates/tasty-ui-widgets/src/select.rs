//! `Select` — 드롭다운 (디자인 `components/forms/Select`).
//!
//! 닫힌 트리거(height 28, surface-raised, border-default, 우측 chevron)는 디자인
//! 토큰 그대로. 열린 메뉴는 egui popup 으로 옵션을 나열한다(메뉴 항목 스타일은
//! 근사 — MenuItem 위젯과 통합 여지). `selected` 변경 시 `true` 반환.

use tasty_type_appearance::theme::Theme;

/// 드롭다운. `selected` 는 `options` 인덱스. 선택이 바뀌면 `true`.
pub fn select(
    ui: &mut egui::Ui,
    theme: &Theme,
    id_salt: &str,
    selected: &mut usize,
    options: &[&str],
    width: f32,
    enabled: bool,
) -> bool {
    let height = theme.select_height().value();
    let pad_x = theme.select_padding_x().value();
    let radius = theme.select_radius().value();
    let bw = theme.border_width.value();
    let body = theme.select_font_size().value();
    let chevron_room = theme.select_chevron_room().value();

    let sense = if enabled {
        egui::Sense::click()
    } else {
        egui::Sense::hover()
    };
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(width, height), sense);
    let dim = |c: egui::Color32| if enabled { c } else { c.gamma_multiply(0.5) };

    // 트리거 박스. hover border 는 대응 select component 토큰 없어 semantic 유지.
    let border = if enabled && resp.hovered() {
        theme.border_strong()
    } else {
        theme.select_border()
    };
    ui.painter().rect(
        rect,
        radius,
        dim(theme.select_bg().to_egui()),
        egui::Stroke::new(bw, dim(border.to_egui())),
        egui::StrokeKind::Inside,
    );
    // 현재 값 — 가용 폭(좌 padding ~ chevron 앞) 초과 시 말줄임(truncate_at_width)으로
    // border/chevron 침범 방지.
    let label = options.get(*selected).copied().unwrap_or("");
    let text_max_width = (rect.right() - chevron_room - (rect.left() + pad_x)).max(0.0);
    let mut job = egui::text::LayoutJob::simple_singleline(
        label.to_owned(),
        egui::FontId::proportional(body),
        egui::Color32::PLACEHOLDER,
    );
    job.wrap = egui::text::TextWrapping::truncate_at_width(text_max_width);
    let galley = ui.fonts(|f| f.layout_job(job));
    let text_pos = egui::pos2(
        rect.left() + pad_x,
        rect.center().y - galley.rect.height() * 0.5,
    );
    ui.painter()
        .galley(text_pos, galley, dim(theme.select_fg().to_egui()));
    // chevron (▾) — 우측.
    let cx = rect.right() - chevron_room * 0.5;
    let cy = rect.center().y;
    let ch = dim(theme.select_chevron_fg().to_egui());
    ui.painter().add(egui::Shape::line(
        vec![
            egui::pos2(cx - 4.0, cy - 2.0),
            egui::pos2(cx, cy + 2.5),
            egui::pos2(cx + 4.0, cy - 2.0),
        ],
        egui::Stroke::new(1.5, ch),
    ));

    let popup_id = ui.make_persistent_id(("tasty_select", id_salt));
    if enabled && resp.clicked() {
        ui.memory_mut(|m| m.toggle_popup(popup_id));
    }

    let mut changed = false;
    egui::popup_below_widget(
        ui,
        popup_id,
        &resp,
        egui::PopupCloseBehavior::CloseOnClick,
        |ui| {
            ui.set_min_width(width);
            for (i, opt) in options.iter().enumerate() {
                if ui.selectable_label(i == *selected, *opt).clicked() && i != *selected {
                    *selected = i;
                    changed = true;
                }
            }
        },
    );
    changed
}
