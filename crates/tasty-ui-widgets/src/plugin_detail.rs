//! Plugins 창 상세의 정체 블록(이름 줄·메타 줄), Command 행, 하단 액션 바.
//! 본체와 갤러리가 같은 함수를 불러 같은 모양을 그린다. Installed 와 Attention 이 정체 블록과
//! 액션 바 틀을 함께 쓰고, 바 안의 내용만 다르다.

mod attention_bar;
mod confirm;
mod identity;

pub use attention_bar::{PluginAttentionBarAction, PluginAttentionBarView, plugin_attention_bar};

pub use confirm::{
    PluginUninstallConfirmClicks, PluginUninstallConfirmView, plugin_uninstall_confirm_bar,
    plugin_uninstall_confirm_bar_height,
};
pub use identity::{PluginIdentityView, plugin_detail_identity};

use tasty_type_appearance::theme::Theme;

use crate::button::{Button, ButtonVariant};
use crate::chip::{TagVariant, kbd, kbd_width, split_keys, tag};
use crate::control::ControlSize;
use crate::plugin_add::PLUGIN_ADD_INSET;
use crate::toggle::switch_with_label_color;

/// 상세 이름 줄. 이름(font-size-max · text-primary · 보통 굵기), 버전 Tag, built-in 이면 기본 Tag 를
/// `spacing_sm` 간격으로 잇는다. agent 색은 agent 플러그인 몫이라 built-in 표시에 쓰지 않는다.
pub fn plugin_detail_name_row(
    ui: &mut egui::Ui,
    theme: &Theme,
    name: &str,
    version: &str,
    builtin_tag: Option<&str>,
) {
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = theme.spacing_sm.value();
        ui.label(
            egui::RichText::new(name)
                .size(theme.font_size_max.value())
                .color(theme.text_primary().to_egui()),
        );
        tag(
            ui,
            theme,
            &format!("v{version}"),
            TagVariant::Default,
            false,
        );
        if let Some(builtin) = builtin_tag {
            tag(ui, theme, builtin, TagVariant::Default, false);
        }
    });
}

/// 상세 설명 문단. body · text-secondary · 줄 높이 `line-height-ui`, 폭은 `measure_lg` 를 넘지 않고
/// 줄바꿈한다. Add 의 매니페스트 카드 설명과 같은 줄 높이다.
pub fn plugin_detail_description(ui: &mut egui::Ui, theme: &Theme, text: &str) {
    let width = theme.measure_lg.value().min(ui.available_width());
    let body = theme.font_size_body.value();
    ui.scope(|ui| {
        ui.set_max_width(width);
        ui.add(
            egui::Label::new(
                egui::RichText::new(text)
                    .size(body)
                    .line_height(Some(body * theme.line_height_ui))
                    .color(theme.text_secondary().to_egui()),
            )
            .wrap(),
        );
    });
}

/// 메타 줄 입력. 빈 문자열은 그 항목을 건너뛴다.
pub struct PluginMetaView<'a> {
    /// 작성자들을 이미 이어 붙인 문자열. 없으면 id 가 맨 앞이다.
    pub authors: &'a str,
    pub id: &'a str,
    /// 매니페스트의 homepage 그대로. 화면에는 `http://`·`https://` 를 뺀 값을 링크로 보인다.
    pub homepage: &'a str,
}

/// 이름 줄 아래 메타 줄 `작성자 · id · homepage`. mono caption · text-muted 를 ` · ` 로 잇고,
/// homepage 는 마지막 항목으로 accent-primary 밑줄 링크다. 링크를 눌렀으면 true.
pub fn plugin_detail_meta(ui: &mut egui::Ui, theme: &Theme, view: &PluginMetaView<'_>) -> bool {
    let font = egui::FontId::monospace(theme.font_size_caption.value());
    let color = theme.text_muted().to_egui();
    let mut clicked = false;
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = theme.spacing_sm.value();
        let mut first = true;
        let mut sep = |ui: &mut egui::Ui| {
            if !first {
                ui.label(egui::RichText::new("·").font(font.clone()).color(color));
            }
            first = false;
        };
        for part in [view.authors, view.id]
            .into_iter()
            .filter(|p| !p.is_empty())
        {
            sep(ui);
            ui.label(egui::RichText::new(part).font(font.clone()).color(color));
        }
        if !view.homepage.is_empty() {
            sep(ui);
            clicked = meta_link(ui, theme, homepage_display(view.homepage)).clicked();
        }
    });
    clicked
}

/// 링크로 보일 homepage. 앞의 `http://`·`https://` 를 뺀다.
pub fn homepage_display(url: &str) -> &str {
    url.strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))
        .unwrap_or(url)
}

/// 메타 줄의 링크. mono caption · accent-primary · 밑줄이며 줄이 넘치면 끝을 말줄임한다.
/// 키보드 포커스면 focus ring 을 두른다.
fn meta_link(ui: &mut egui::Ui, theme: &Theme, text: &str) -> egui::Response {
    let color = theme.accent_primary().to_egui();
    let mut job = egui::text::LayoutJob::default();
    job.append(
        text,
        0.0,
        egui::TextFormat {
            font_id: egui::FontId::monospace(theme.font_size_caption.value()),
            color,
            underline: egui::Stroke::new(theme.border_width.value(), color),
            ..Default::default()
        },
    );
    job.wrap.max_width = ui.available_width();
    job.wrap.max_rows = 1;
    job.wrap.break_anywhere = true;
    let galley = ui.ctx().fonts(|f| f.layout_job(job));
    let (rect, resp) = ui.allocate_exact_size(galley.size(), egui::Sense::click());
    ui.painter().galley(rect.min, galley, color);
    if resp.has_focus() {
        ui.painter().rect_stroke(
            rect,
            theme.corner_radius_sm.value(),
            egui::Stroke::new(theme.focus_ring_width.value(), theme.border_focus()),
            egui::StrokeKind::Outside,
        );
    }
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// 매니페스트 단축키를 키캡 규칙으로 다듬는다. `+` 로 나눠 앞뒤 공백을 빼고, 한 글자 키는 대문자,
/// 나머지는 첫 글자만 대문자로 쓴다(`ctrl + shift + h` → `Ctrl+Shift+H`). `+` 키 자체는 남긴다.
pub fn plugin_keycaps(chord: &str) -> String {
    let keys: Vec<String> = split_keys(chord)
        .into_iter()
        .map(str::trim)
        .filter(|k| !k.is_empty())
        .map(|k| {
            let mut chars = k.chars();
            match chars.next() {
                Some(c) if chars.as_str().is_empty() => c.to_uppercase().collect(),
                Some(c) => c
                    .to_uppercase()
                    .chain(chars.as_str().to_lowercase().chars())
                    .collect(),
                None => String::new(),
            }
        })
        .collect();
    keys.join("+")
}

/// Command 절의 한 행. 왼쪽에 명령 제목(mono term-sm · text-secondary), 오른쪽에 단축키 Kbd 를 두고
/// 행 아래에 구분선을 긋는다. 행 높이는 아래 선을 포함해 `settings_row_min_height` 이고 아래 여백을
/// 더하지 않는다. 단축키는 [`plugin_keycaps`] 규칙으로 다듬어 그린다.
pub fn plugin_command_row(ui: &mut egui::Ui, theme: &Theme, title: &str, keys: Option<&str>) {
    let bw = theme.border_width.value();
    let height = theme.settings_row_min_height().value();
    let width = ui.available_width();
    let row_h = height - bw;
    let gap = theme.spacing_lg.value();
    let keys = keys.map(plugin_keycaps).filter(|k| !k.is_empty());
    let keys = keys.as_deref();
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
    plugin_detail_bar_frame(ui, theme, ControlSize::Md.height(theme), |ui| {
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
    });
    clicks
}

/// 액션 바 틀. 위 구분선 아래 가로 14 · 세로 `spacing_md` 여백 안에 높이 `row_h` 의 가로 줄을 두고
/// `contents` 를 왼쪽부터 `spacing_sm` 간격으로 세로 가운데 맞춰 그린다. Installed·Attention·제거 확인이
/// 같은 틀을 쓰고 내용만 다르다.
pub fn plugin_detail_bar_frame(
    ui: &mut egui::Ui,
    theme: &Theme,
    row_h: f32,
    contents: impl FnOnce(&mut egui::Ui),
) {
    let response = egui::Frame::new()
        .inner_margin(egui::Margin::symmetric(
            PLUGIN_ADD_INSET.value() as i8,
            theme.spacing_md.value() as i8,
        ))
        .show(ui, |ui| {
            ui.allocate_ui_with_layout(
                egui::vec2(ui.available_width(), row_h),
                egui::Layout::left_to_right(egui::Align::Center),
                |ui| {
                    ui.set_min_height(row_h);
                    ui.spacing_mut().item_spacing.x = theme.spacing_sm.value();
                    contents(ui);
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
