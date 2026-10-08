//! Plugins 창 Installed 상세의 메타 줄, Command 행, 하단 액션 바.
//! 본체와 갤러리가 같은 함수를 불러 같은 모양을 그린다.

use tasty_type_appearance::theme::Theme;

use crate::button::{Button, ButtonVariant};
use crate::chip::{kbd, kbd_width};
use crate::control::ControlSize;
use crate::plugin_add::PLUGIN_ADD_INSET;
use crate::toggle::switch_with_label_color;

/// 이름 줄 아래 메타 줄. 항목을 mono caption · text-muted 로 ` · ` 를 사이에 두고 잇는다.
/// 빈 항목은 건너뛴다.
pub fn plugin_detail_meta(ui: &mut egui::Ui, theme: &Theme, parts: &[&str]) {
    let font = egui::FontId::monospace(theme.font_size_caption.value());
    let color = theme.text_muted().to_egui();
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = theme.spacing_sm.value();
        let mut first = true;
        for part in parts.iter().filter(|p| !p.is_empty()) {
            if !first {
                ui.label(egui::RichText::new("·").font(font.clone()).color(color));
            }
            first = false;
            ui.label(egui::RichText::new(*part).font(font.clone()).color(color));
        }
    });
}

/// Command 절의 한 행. 왼쪽에 명령 제목(mono term-sm · text-secondary), 오른쪽에 단축키 Kbd 를 두고
/// 행 아래에 구분선을 긋는다. 행 높이는 아래 선을 포함해 `settings_row_min_height` 이다.
pub fn plugin_command_row(ui: &mut egui::Ui, theme: &Theme, title: &str, keys: Option<&str>) {
    let bw = theme.border_width.value();
    let height = theme.settings_row_min_height().value();
    let width = ui.available_width();
    let row_h = height - bw;
    let gap = theme.spacing_lg.value();
    let keys = keys.filter(|k| !k.is_empty());
    // 키캡은 왼쪽에서 오른쪽으로 그려야 순서가 맞으므로, 키캡 폭을 먼저 재서 제목 칸 폭을 정한다.
    let kbd_w = keys.map(|k| kbd_width(ui.ctx(), theme, k).value());
    let title_w = (width - kbd_w.map(|w| w + gap).unwrap_or(0.0)).max(0.0);
    let response = ui
        .allocate_ui_with_layout(
            egui::vec2(width, row_h),
            egui::Layout::left_to_right(egui::Align::Center),
            |ui| {
                ui.set_min_size(egui::vec2(width, row_h));
                ui.spacing_mut().item_spacing.x = gap;
                ui.allocate_ui_with_layout(
                    egui::vec2(title_w, row_h),
                    egui::Layout::left_to_right(egui::Align::Center),
                    |ui| {
                        ui.set_min_width(title_w);
                        ui.add(
                            egui::Label::new(
                                egui::RichText::new(title)
                                    .monospace()
                                    .size(theme.font_size_term_sm.value())
                                    .color(theme.text_secondary().to_egui()),
                            )
                            .truncate(),
                        );
                    },
                );
                if let Some(keys) = keys {
                    kbd(ui, theme, keys);
                }
            },
        )
        .response;
    let (line, _) = ui.allocate_exact_size(egui::vec2(width, bw), egui::Sense::hover());
    ui.painter().hline(
        response.rect.x_range(),
        line.center().y,
        egui::Stroke::new(bw, theme.separator.to_egui_premultiplied()),
    );
}

/// 액션 바 문구와 상태.
pub struct PluginDetailBarView<'a> {
    pub enabled: bool,
    /// 스위치 오른쪽 라벨. 켜져 있으면 `enabled_label`, 꺼져 있으면 `disabled_label`.
    pub enabled_label: &'a str,
    pub disabled_label: &'a str,
    pub configure: &'a str,
    pub uninstall: &'a str,
}

/// 액션 바에서 일어난 일.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PluginDetailBarClicks {
    /// 스위치나 라벨을 눌러 활성 상태를 뒤집었다.
    pub toggled: bool,
    pub configure: bool,
    pub uninstall: bool,
}

/// 액션 바 전체 높이. 위 구분선은 바 안쪽 위 경계에 그리므로 따로 더하지 않는다.
pub fn plugin_detail_bar_height(theme: &Theme) -> f32 {
    ControlSize::Md.height(theme) + 2.0 * theme.spacing_md.value()
}

/// 위 구분선 아래에 왼쪽 스위치와 라벨, 오른쪽 Configure(ghost, settings 아이콘)와
/// Uninstall(secondary, accent-danger 글자)을 둔다. 바는 상세 열 폭 전체를 쓰도록 여백 없는
/// rect 에 그린다. 키보드 초점이 화면 순서(스위치 → Configure → Uninstall)를 따르도록
/// 오른쪽 묶음 폭을 먼저 재고 왼쪽에서 오른쪽으로 만든다.
pub fn plugin_detail_bar(
    ui: &mut egui::Ui,
    theme: &Theme,
    view: &PluginDetailBarView<'_>,
) -> PluginDetailBarClicks {
    let mut clicks = PluginDetailBarClicks::default();
    let gap = theme.spacing_sm.value();
    let actions_w = {
        let mut probe = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(ui.available_rect_before_wrap())
                .layout(egui::Layout::left_to_right(egui::Align::Center))
                .sizing_pass()
                .invisible(),
        );
        probe.spacing_mut().item_spacing.x = gap;
        bar_actions(&mut probe, theme, view);
        probe.min_rect().width()
    };
    let response = egui::Frame::new()
        .inner_margin(egui::Margin::symmetric(
            PLUGIN_ADD_INSET.value() as i8,
            theme.spacing_md.value() as i8,
        ))
        .show(ui, |ui| {
            ui.allocate_ui_with_layout(
                egui::vec2(ui.available_width(), ControlSize::Md.height(theme)),
                egui::Layout::left_to_right(egui::Align::Center),
                |ui| {
                    ui.spacing_mut().item_spacing.x = gap;
                    let label = if view.enabled {
                        view.enabled_label
                    } else {
                        view.disabled_label
                    };
                    let mut on = view.enabled;
                    clicks.toggled = switch_with_label_color(
                        ui,
                        theme,
                        &mut on,
                        Some(label),
                        true,
                        theme.text_secondary().to_egui(),
                    )
                    .changed();
                    // 스위치 뒤 간격은 이미 커서에 들어가 있다. 남는 폭만큼 밀어 오른쪽에 붙인다.
                    let spare = ui.available_width() - actions_w;
                    if spare > 0.0 {
                        ui.add_space(spare);
                    }
                    (clicks.configure, clicks.uninstall) = bar_actions(ui, theme, view);
                },
            );
        })
        .response;
    let rect = response.rect;
    ui.painter().hline(
        rect.x_range(),
        rect.top(),
        egui::Stroke::new(
            theme.border_width.value(),
            theme.separator.to_egui_premultiplied(),
        ),
    );
    clicks
}

/// 오른쪽 묶음 — Configure, Uninstall 순서로 만든다. 눌린 여부를 돌려준다.
fn bar_actions(ui: &mut egui::Ui, theme: &Theme, view: &PluginDetailBarView<'_>) -> (bool, bool) {
    let configure = Button::new(view.configure)
        .variant(ButtonVariant::Ghost)
        .leading_icon(&|ui, rect, c| {
            tasty_icons::SETTINGS
                .image(rect.height(), c)
                .paint_at(ui, rect)
        })
        .show(ui, theme)
        .clicked();
    let uninstall = Button::new(view.uninstall)
        .variant(ButtonVariant::Secondary)
        .danger_ink(true)
        .show(ui, theme)
        .clicked();
    (configure, uninstall)
}

#[cfg(test)]
mod tests;
