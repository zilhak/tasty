//! 등록 후 내용이 바뀐 Lua 스크립트의 실행 확인 popup 콘텐츠.
//! 본체 popup 과 갤러리 예제가 이 함수를 함께 부른다. 문자열은 호출자가 주입하고,
//! Escape 처리와 popup 높이 측정은 호출자가 맡는다.

use tasty_type_appearance::theme::Theme;

use crate::ControlSize;
use crate::button::{Button, ButtonVariant};
use crate::chip::{TagVariant, tag};

/// 확인 콘텐츠의 입력값. 본체는 번역 문자열을, 갤러리는 영어 문자열을 넣는다.
pub struct ScriptConfirmView<'a> {
    pub title: &'a str,
    /// 변경된 스크립트 표시 이름. 한 줄로 말줄임한다.
    pub name: &'a str,
    pub changed_tag: &'a str,
    pub body: &'a str,
    pub run: &'a str,
    pub cancel: &'a str,
}

/// 이번 프레임에 눌린 버튼.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ScriptConfirmOutput {
    pub run: bool,
    pub cancel: bool,
}

/// 제목 → 이름 → changed 태그 → 안내문 문단 → 우측 정렬 버튼 행을 위에서 아래로 그린다.
pub fn script_confirm(
    ui: &mut egui::Ui,
    theme: &Theme,
    view: &ScriptConfirmView<'_>,
) -> ScriptConfirmOutput {
    let mut out = ScriptConfirmOutput::default();

    ui.spacing_mut().item_spacing.y = theme.spacing_sm.value();

    ui.label(
        egui::RichText::new(view.title)
            .size(theme.font_size_body.value())
            .strong()
            .color(theme.text_primary().to_egui()),
    );

    ui.add(
        egui::Label::new(
            egui::RichText::new(view.name)
                .size(theme.font_size_caption.value())
                .family(egui::FontFamily::Monospace)
                .color(theme.text_muted().to_egui()),
        )
        .truncate(),
    );

    // 태그는 단독 줄이고 안내문은 그 아래 전폭 문단이다. 안내문은 주어진 폭 안에서 줄바꿈한다.
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = theme.spacing_xs.value();
        tag(ui, theme, view.changed_tag, TagVariant::Warning, false);
        let size = theme.font_size_caption.value();
        let mut job = egui::text::LayoutJob::default();
        job.wrap.max_width = ui.available_width();
        job.append(
            view.body,
            0.0,
            egui::TextFormat {
                font_id: egui::FontId::proportional(size),
                color: theme.text_secondary().to_egui(),
                line_height: Some(size * theme.line_height_ui),
                ..Default::default()
            },
        );
        ui.add(egui::Label::new(job).wrap());
    });

    // 본문 → 버튼은 행 간격(space-sm)에 space-xs 를 더해 space-md 다.
    ui.add_space(theme.spacing_xs.value());

    ui.horizontal(|ui| {
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            out.run = Button::new(view.run)
                .variant(ButtonVariant::Primary)
                .size(ControlSize::Sm)
                .show(ui, theme)
                .clicked();
            out.cancel = Button::new(view.cancel)
                .variant(ButtonVariant::Ghost)
                .size(ControlSize::Sm)
                .show(ui, theme)
                .clicked();
        });
    });

    out
}
