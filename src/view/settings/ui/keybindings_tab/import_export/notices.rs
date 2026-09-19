//! 안내 — 버린 plugin 정보 줄 · 알림 블록(파싱 실패 · 내보내기 실패 · 번들 경고).
//!
//! 경계: 표·카드가 아닌 알림 자리의 그리기다.
//!
//! 알림 블록은 **레시피 하나**다 — 톤 · 글리프 · 제목(+개수) · 본문 · 액션 행. 실패와
//! 경고는 톤만 다르다(디자인 `IeBlockG`). 블록마다 따로 지으면 한쪽 치수만 움직인다.

use tasty_type_appearance::theme::Theme;
use tasty_ui_widgets::{Button, ButtonVariant, ControlSize};

use crate::adapters::ui::icons;
use crate::i18n::{t, t_fmt, t_fmt2};

use super::paint::glyph_at;
use super::{ExportFailReason, ExportFailure, Failure, GROUP_CHEVRON_GAP};

/// 버린 plugin override 안내 — 정보(경고 아님): 잘못된 것도 할 일도 없다.
pub(super) fn dropped_notice(ui: &mut egui::Ui, th: &Theme, text: &str) {
    ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing.x = th.spacing_sm.value();
        glyph_at(
            ui,
            icons::HELP_CIRCLE,
            th.icon_glyph_size_sm,
            th.text_muted().to_egui(),
        );
        ui.label(
            egui::RichText::new(text)
                .size(th.font_size_term_sm.value())
                .color(th.text_muted()),
        );
    });
}

/// 블록의 액션 버튼 하나.
pub(super) struct NoticeAction<'a> {
    pub(super) label: &'a str,
    pub(super) variant: ButtonVariant,
}

/// 알림 블록 — 디자인 `IeBlockG`. 톤 틴트 배경 + 톤 테두리 · 헤더(글리프 · 제목 · 우측 개수) ·
/// 본문 · 액션 행. 눌린 액션의 인덱스를 돌려준다.
// reason: 레시피의 칸(톤 · 글리프 · 제목 · 개수 · 본문 · 액션)이 그대로 인자다 — 묶으면 세 호출부가
// 같은 구조체를 채우는 코드만 늘어난다.
#[allow(clippy::too_many_arguments)]
pub(super) fn notice_block(
    ui: &mut egui::Ui,
    th: &Theme,
    tone: egui::Color32,
    glyph: icons::Icon,
    title: &str,
    count: Option<&str>,
    body: impl FnOnce(&mut egui::Ui),
    actions: &[NoticeAction<'_>],
) -> Option<usize> {
    let mut clicked = None;
    egui::Frame::new()
        .fill(tone.gamma_multiply(th.tint_fill_alpha()))
        .stroke(egui::Stroke::new(
            th.border_width.value(),
            tone.gamma_multiply(th.tint_border_alpha()),
        ))
        .corner_radius(th.corner_radius.value())
        .inner_margin(tasty_ui_widgets::margin_all(th.spacing_md))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = th.spacing_xs.value();
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = th.spacing_sm.value();
                glyph_at(ui, glyph, th.icon_glyph_size_md, tone);
                ui.label(
                    egui::RichText::new(title)
                        .size(th.font_size_body.value())
                        .color(tone),
                );
                if let Some(count) = count {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(
                            egui::RichText::new(count)
                                .monospace()
                                .size(th.font_size_caption.value())
                                .color(tone),
                        );
                    });
                }
            });
            body(ui);
            if !actions.is_empty() {
                tasty_ui_widgets::vspace(ui, th.spacing_xs);
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = th.spacing_sm.value();
                    for (i, action) in actions.iter().enumerate() {
                        if Button::new(action.label)
                            .variant(action.variant)
                            .size(ControlSize::Sm)
                            .show(ui, th)
                            .clicked()
                        {
                            clicked = Some(i);
                        }
                    }
                });
            }
        });
    clicked
}

/// 실패 블록의 본문 한 문단 — 12 text-secondary, max-width measure-md.
fn failure_body(ui: &mut egui::Ui, th: &Theme, text: &str) {
    ui.scope(|ui| {
        ui.set_max_width(th.measure_md.value());
        ui.label(
            egui::RichText::new(text)
                .size(th.font_size_term_sm.value())
                .color(th.text_secondary()),
        );
    });
}

/// 파싱 실패 — 보던 대상에 대한 사실이라 toast/popup 이 아니라 detail 영역 안에 인라인.
/// "다른 파일 고르기" 가 눌리면 true.
///
/// 줄 번호가 없을 때는 줄 구절을 **빼지 않고 바꾼다** — 가운데 문장은 언제나 왜 실패했는지를
/// 말하고, 앞뒤 문장은 두 형태에서 같다.
pub(super) fn parse_failure(ui: &mut egui::Ui, th: &Theme, failure: &Failure) -> bool {
    let body = match failure.line {
        Some(line) => t_fmt2(
            "settings.keybindings.ie_failure_body_line",
            &failure.path,
            &line.to_string(),
        ),
        None => t_fmt("settings.keybindings.ie_failure_body", &failure.path),
    };
    notice_block(
        ui,
        th,
        th.accent_danger().to_egui(),
        icons::ALERT_CIRCLE,
        t("settings.keybindings.ie_failure_title"),
        None,
        |ui| failure_body(ui, th, &body),
        &[NoticeAction {
            label: t("settings.keybindings.ie_choose_another"),
            variant: ButtonVariant::Secondary,
        }],
    )
    .is_some()
}

/// 내보내기 실패 블록에서 눌린 것.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ExportFailureAction {
    Retry,
    ChooseAnother,
}

/// 내보내기 실패 — Export 액션 행 안의 인라인 블록. 재시도가 여기 살기 때문에 toast 가
/// 아니다(toast 는 저절로 사라지고 재시도를 못 싣는다).
pub(super) fn export_failure(
    ui: &mut egui::Ui,
    th: &Theme,
    failure: &ExportFailure,
) -> Option<ExportFailureAction> {
    let reason = match &failure.reason {
        ExportFailReason::ReadOnly => t("settings.keybindings.ie_export_failure_read_only"),
        ExportFailReason::Other(message) => message.as_str(),
    };
    let body = t_fmt2(
        "settings.keybindings.ie_export_failure_body",
        &failure.path.display().to_string(),
        reason,
    );
    notice_block(
        ui,
        th,
        th.accent_danger().to_egui(),
        icons::ALERT_CIRCLE,
        t("settings.keybindings.ie_export_failure_title"),
        None,
        |ui| failure_body(ui, th, &body),
        &[
            NoticeAction {
                label: t("settings.keybindings.ie_export_retry"),
                variant: ButtonVariant::Secondary,
            },
            NoticeAction {
                label: t("settings.keybindings.ie_export_choose_another"),
                variant: ButtonVariant::Ghost,
            },
        ],
    )
    .map(|i| {
        if i == 0 {
            ExportFailureAction::Retry
        } else {
            ExportFailureAction::ChooseAnother
        }
    })
}

/// 번들 경고 — 경고 톤 블록 **하나**에 한 줄씩, 헤더에 개수. 접힌 줄이 있으면
/// "N 개 더 보기" 가 액션 행에 선다. 그것이 눌리면 true.
pub(super) fn bundle_notices(
    ui: &mut egui::Ui,
    th: &Theme,
    lines: &[String],
    expanded: bool,
) -> bool {
    let (shown, hidden) = super::bundle_notices::fold(lines.len(), expanded);
    let count = t_fmt(
        "settings.keybindings.ie_notices_count",
        &lines.len().to_string(),
    );
    let more = t_fmt(
        "settings.keybindings.ie_notices_show_more",
        &hidden.to_string(),
    );
    let actions: &[NoticeAction<'_>] = if hidden > 0 {
        &[NoticeAction {
            label: &more,
            variant: ButtonVariant::Ghost,
        }]
    } else {
        &[]
    };
    notice_block(
        ui,
        th,
        th.accent_warning().to_egui(),
        icons::ALERT_TRIANGLE,
        t("settings.keybindings.ie_notices_title"),
        Some(&count),
        |ui| {
            for line in &lines[..shown] {
                ui.horizontal_top(|ui| {
                    ui.spacing_mut().item_spacing.x = GROUP_CHEVRON_GAP.value();
                    ui.label(
                        egui::RichText::new("·")
                            .size(th.font_size_term_sm.value())
                            .color(th.text_muted()),
                    );
                    ui.label(
                        egui::RichText::new(line)
                            .size(th.font_size_term_sm.value())
                            .color(th.text_secondary()),
                    );
                });
            }
        },
        actions,
    )
    .is_some()
}
