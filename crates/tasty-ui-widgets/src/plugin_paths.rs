//! 설치된 플러그인 상세의 마지막 절 — 설치 경로와 로그 경로, `Open folder`. 본체와 갤러리가 함께 호출한다.
//!
//! `Open folder` 는 경로 줄이 아니라 머리글 줄 오른쪽에 둔다. 긴 경로가 버튼과 폭을 다투면
//! 좁은 창에서 버튼이 잘리기 때문이다. 경로는 아무 문자에서 줄바꿈하고 선택할 수 있어
//! 잘리는 글자가 없으므로 말줄임과 툴팁을 두지 않는다.

use tasty_type_appearance::theme::Theme;

use crate::button::{Button, ButtonVariant};
use crate::control::ControlSize;
use crate::plugin_add::mono_header;

/// 호출부가 번역해 넘기는 문구와 경로.
pub struct PluginInstallPathsView<'a> {
    /// 머리글. 대문자로 그린다.
    pub label: &'a str,
    pub open_folder: &'a str,
    pub install_dir: &'a str,
    /// `Log: <path>` 처럼 접두를 붙인 한 줄.
    pub log_line: &'a str,
}

/// 절을 그리고 `Open folder` 가 눌렸는지 돌려준다.
pub fn plugin_install_paths(
    ui: &mut egui::Ui,
    theme: &Theme,
    view: &PluginInstallPathsView<'_>,
) -> bool {
    let mut open = false;
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = theme.spacing_sm.value();
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = theme.spacing_sm.value();
            mono_header(ui, theme, view.label);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                open = Button::new(view.open_folder)
                    .variant(ButtonVariant::Secondary)
                    .size(ControlSize::Sm)
                    .leading_icon(&|ui, rect, c| {
                        tasty_icons::FOLDER
                            .image(rect.height(), c)
                            .paint_at(ui, rect)
                    })
                    .show(ui, theme)
                    .clicked();
            });
        });
        path_line(ui, theme, view.install_dir);
        path_line(ui, theme, view.log_line);
    });
    open
}

/// mono caption · text-muted 한 줄. 공백이 없는 경로도 열 폭에서 끊기도록 아무 문자에서 줄바꿈한다.
fn path_line(ui: &mut egui::Ui, theme: &Theme, text: &str) {
    let mut job = egui::text::LayoutJob::single_section(
        text.to_owned(),
        egui::TextFormat {
            font_id: egui::FontId::monospace(theme.font_size_caption.value()),
            color: theme.text_muted().to_egui(),
            ..Default::default()
        },
    );
    job.wrap.break_anywhere = true;
    ui.add(egui::Label::new(job).wrap().selectable(true));
}
