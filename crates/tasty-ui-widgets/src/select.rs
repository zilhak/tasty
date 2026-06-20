//! `Select` — 드롭다운 (디자인 `components/forms/Select`).
//!
//! 닫힌 트리거(height 28, surface-raised, border-default, 우측 chevron)는 디자인
//! 토큰 그대로. 열린 메뉴는 egui popup 으로 옵션을 나열한다(메뉴 항목 스타일은
//! 근사 — MenuItem 위젯과 통합 여지). `selected` 변경 시 `true` 반환.

use tasty_type_appearance::theme::Theme;

const CHEVRON_PAD: f32 = 28.0; // 우측 chevron 영역(디자인 padding-right 28)

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
    let height = theme.item_height_interactive.value();
    let pad_x = theme.spacing_md.value();
    let radius = theme.corner_radius.value();
    let bw = theme.border_width.value();
    let body = theme.font_size_body.value();

    let sense = if enabled {
        egui::Sense::click()
    } else {
        egui::Sense::hover()
    };
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(width, height), sense);
    let dim = |c: egui::Color32| if enabled { c } else { c.gamma_multiply(0.5) };

    // 트리거 박스.
    let border = if enabled && resp.hovered() {
        theme.border_strong()
    } else {
        theme.border_default()
    };
    ui.painter().rect(
        rect,
        radius,
        dim(theme.surface_raised().to_egui()),
        egui::Stroke::new(bw, dim(border.to_egui())),
        egui::StrokeKind::Inside,
    );
    // 현재 값.
    let label = options.get(*selected).copied().unwrap_or("");
    let galley = ui.painter().layout_no_wrap(
        label.to_owned(),
        egui::FontId::proportional(body),
        egui::Color32::PLACEHOLDER,
    );
    let text_pos = egui::pos2(
        rect.left() + pad_x,
        rect.center().y - galley.rect.height() * 0.5,
    );
    ui.painter()
        .galley(text_pos, galley, dim(theme.text_primary().to_egui()));
    // chevron (▾) — 우측.
    let cx = rect.right() - CHEVRON_PAD * 0.5;
    let cy = rect.center().y;
    let ch = dim(theme.subtext0.to_egui());
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
