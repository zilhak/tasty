//! 제거 확인. 본문 끝이나 popup 이 아니라 액션 바의 내용을 그 자리에서 바꾼다.

use std::sync::Arc;

use tasty_type_appearance::theme::Theme;

use super::plugin_detail_bar_frame;
use crate::button::{Button, ButtonVariant};
use crate::control::ControlSize;
use crate::plugin_add::PLUGIN_ADD_INSET;

/// 확인 바 문구. 본체는 번역 문자열을, 갤러리는 영어 문자열을 넣는다.
pub struct PluginUninstallConfirmView<'a> {
    /// `Uninstall {name}?` 처럼 이름을 넣은 질문.
    pub title: &'a str,
    /// built-in 여부에 따른 한 줄 안내.
    pub note: &'a str,
    pub cancel: &'a str,
    pub uninstall: &'a str,
    /// 확인이 막 열린 프레임이면 true. Cancel 에 키보드 포커스를 준다.
    pub focus_cancel: bool,
}

/// 확인 바에서 눌린 버튼.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PluginUninstallConfirmClicks {
    pub cancel: bool,
    pub uninstall: bool,
}

/// alertTriangle(accent-attention) · [질문(body · text-primary) / 안내(caption · text-muted)] ·
/// Cancel(ghost) · Uninstall(danger). 글 열은 버튼을 뺀 폭에서 줄바꿈한다. `focus_cancel` 이면
/// Cancel 에 포커스를 준다. Esc 로 닫는 일은 확인 상태를 가진 호출자가 맡는다.
pub fn plugin_uninstall_confirm_bar(
    ui: &mut egui::Ui,
    theme: &Theme,
    view: &PluginUninstallConfirmView<'_>,
) -> PluginUninstallConfirmClicks {
    let mut clicks = PluginUninstallConfirmClicks::default();
    let text = TextColumn::layout(ui, theme, view, ui.available_width());
    plugin_detail_bar_frame(ui, theme, text.row_h(theme), |ui| {
        let icon = theme.icon_glyph_size_md.value();
        let (rect, _) = ui.allocate_exact_size(egui::vec2(icon, icon), egui::Sense::hover());
        tasty_icons::ALERT_TRIANGLE
            .image(icon, theme.accent_attention().to_egui())
            .paint_at(ui, rect);
        let (rect, _) =
            ui.allocate_exact_size(egui::vec2(text.width, text.height), egui::Sense::hover());
        let mut y = rect.min.y;
        for galley in [&text.title, &text.note] {
            let h = galley.size().y;
            ui.painter().galley(
                egui::pos2(rect.min.x, y),
                galley.clone(),
                egui::Color32::PLACEHOLDER,
            );
            y += h;
        }
        (clicks.cancel, clicks.uninstall) = buttons(ui, theme, view, view.focus_cancel);
    });
    clicks
}

/// 확인 바 전체 높이. 글 열이 컨트롤 높이보다 크면 그만큼 커진다. `width` 는 바가 차지할 폭이다.
pub fn plugin_uninstall_confirm_bar_height(
    ui: &mut egui::Ui,
    theme: &Theme,
    view: &PluginUninstallConfirmView<'_>,
    width: f32,
) -> f32 {
    TextColumn::layout(ui, theme, view, width).row_h(theme) + 2.0 * theme.spacing_md.value()
}

/// 질문과 안내를 글 열 폭에 맞춰 줄바꿈한 결과.
struct TextColumn {
    title: Arc<egui::Galley>,
    note: Arc<egui::Galley>,
    width: f32,
    height: f32,
}

impl TextColumn {
    /// 바 폭 `width` 에서 여백·아이콘·버튼·간격을 뺀 폭으로 두 줄을 배치한다.
    fn layout(
        ui: &mut egui::Ui,
        theme: &Theme,
        view: &PluginUninstallConfirmView<'_>,
        width: f32,
    ) -> Self {
        let gap = theme.spacing_sm.value();
        let fixed = 2.0 * PLUGIN_ADD_INSET.value()
            + theme.icon_glyph_size_md.value()
            + buttons_width(ui, theme, view)
            + 3.0 * gap;
        let text_w = (width - fixed).max(0.0);
        let ctx = ui.ctx().clone();
        let wrap = |text: &str, size: f32, color: egui::Color32| {
            let mut job = egui::text::LayoutJob::simple(
                text.to_owned(),
                egui::FontId::proportional(size),
                color,
                text_w,
            );
            job.wrap.break_anywhere = false;
            ctx.fonts(|f| f.layout_job(job))
        };
        let title = wrap(
            view.title,
            theme.font_size_body.value(),
            theme.text_primary().to_egui(),
        );
        let note = wrap(
            view.note,
            theme.font_size_caption.value(),
            theme.text_muted().to_egui(),
        );
        let height = title.size().y + note.size().y;
        Self {
            title,
            note,
            width: text_w,
            height,
        }
    }

    fn row_h(&self, theme: &Theme) -> f32 {
        self.height.max(ControlSize::Md.height(theme))
    }
}

/// Cancel, Uninstall 순서로 만든다. 눌린 여부를 돌려준다. 폭을 재는 보이지 않는 pass 는
/// `focus` 를 false 로 불러 그 임시 위젯에 포커스를 주지 않는다.
fn buttons(
    ui: &mut egui::Ui,
    theme: &Theme,
    view: &PluginUninstallConfirmView<'_>,
    focus: bool,
) -> (bool, bool) {
    let cancel = Button::new(view.cancel)
        .variant(ButtonVariant::Ghost)
        .show(ui, theme);
    if focus {
        cancel.request_focus();
    }
    let cancel = cancel.clicked();
    let uninstall = Button::new(view.uninstall)
        .variant(ButtonVariant::Danger)
        .show(ui, theme)
        .clicked();
    (cancel, uninstall)
}

/// 두 버튼과 그 사이 간격의 폭. 보이지 않는 sizing pass 로 잰다.
fn buttons_width(ui: &mut egui::Ui, theme: &Theme, view: &PluginUninstallConfirmView<'_>) -> f32 {
    let mut probe = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(f32::INFINITY, f32::INFINITY),
            ))
            .layout(egui::Layout::left_to_right(egui::Align::Center))
            .sizing_pass()
            .invisible(),
    );
    probe.spacing_mut().item_spacing.x = theme.spacing_sm.value();
    buttons(&mut probe, theme, view, false);
    probe.min_rect().width()
}
