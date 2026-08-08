//! Lua 스크립트 TOFU 변경 확인 팝업 (`script_changed_confirm`) — ADR-0031.
//!
//! 단축키 발화 시 등록 해시(03)와 현재 파일 해시가 다르면 게이트가 실행을 보류하고
//! 이 팝업을 띄운다. [실행] 확정 시에만 `App::dispatch_pending_script_confirm` 이 해시를
//! 갱신·영속하고 워커에서 실행한다. 구조는 `size_confirm.rs` 와 동일하게 순수 view +
//! 본체 wrapper 로 분리한다.

use crate::adapters::ui::popup::PopupAction;
use crate::i18n::t;
use crate::state::AppState;
use crate::theme;
use tasty_type_appearance::theme::Theme;
use tasty_ui_widgets::{Button, ButtonVariant, TagVariant, tag};

/// Pure view 의 입력. AppState/CoreState 를 알지 못한다.
pub struct ScriptConfirmProps<'a> {
    pub theme: &'a Theme,
    /// 변경된 스크립트 표시 이름.
    pub name: &'a str,
}

/// view 가 보고하는 사용자 의도.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScriptConfirmAction {
    None,
    /// Escape — 취소로 간주(실행 안 함).
    Close,
    /// Cancel 버튼 — 실행 안 함.
    Cancel,
    /// Run 버튼 — 변경본을 실행하고 해시를 갱신.
    Run,
}

/// 순수 view. AppState/CoreState 접근 금지.
pub fn draw_script_confirm_view(
    ui: &mut egui::Ui,
    props: &ScriptConfirmProps<'_>,
) -> ScriptConfirmAction {
    let th = props.theme;

    if ui.ctx().input(|i| i.key_pressed(egui::Key::Escape)) {
        return ScriptConfirmAction::Close;
    }

    let mut action = ScriptConfirmAction::None;

    ui.spacing_mut().item_spacing.y = th.spacing_sm.value();

    // 제목.
    ui.label(
        egui::RichText::new(t("script.confirm.title"))
            .size(th.font_size_body.value())
            .strong()
            .color(th.text_primary().to_egui()),
    );

    // 스크립트 이름 (mono, muted).
    ui.add(
        egui::Label::new(
            egui::RichText::new(props.name)
                .size(th.font_size_caption.value())
                .family(egui::FontFamily::Monospace)
                .color(th.text_muted().to_egui()),
        )
        .truncate(),
    );

    // 경고 태그 + 안내문.
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = th.spacing_sm.value();
        tag(
            ui,
            th,
            &t("script.confirm.changed_tag"),
            TagVariant::Warning,
            false,
        );
        ui.label(
            egui::RichText::new(t("script.confirm.body"))
                .size(th.font_size_caption.value())
                .color(th.text_secondary().to_egui()),
        );
    });

    ui.add_space(th.spacing_xs.value());

    // 푸터: Cancel(ghost) / Run(primary), 우측 정렬.
    ui.horizontal(|ui| {
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if Button::new(&t("script.confirm.run"))
                .variant(ButtonVariant::Primary)
                .show(ui, th)
                .clicked()
            {
                action = ScriptConfirmAction::Run;
            }
            if Button::new(&t("button.cancel"))
                .variant(ButtonVariant::Ghost)
                .show(ui, th)
                .clicked()
            {
                action = ScriptConfirmAction::Cancel;
            }
        });
    });

    action
}

/// PopupDef::on_close 진입점 — X 버튼(draw_fn 을 우회하는 닫힘 경로)에서만
/// 실질적으로 정리할 게 있다. Cancel/Close(Escape) 는 draw_fn 이 이미 `None` 으로
/// 비워서 여기선 no-op. Run 은 `result = Some(true)` 를 남긴 채 닫히는데, 다음
/// 프레임의 `App::dispatch_pending_script_confirm` 이 그 값을 읽고 해시 갱신·실행을
/// 하므로 **여기서 지우면 안 된다** — `result.is_none()` 일 때만(=아직 아무 결정도
/// 없이 강제로 닫힌 경우) 정리한다.
pub fn on_close_script_confirm_popup(
    _ctx: &egui::Context,
    state: &mut AppState,
    _engine: &mut crate::core::CoreState,
) {
    if let Some(pending) = state.dialogs.pending_script_confirm.as_ref()
        && pending.result.is_none()
    {
        state.dialogs.pending_script_confirm = None;
    }
}

/// PopupDef::draw_fn entry.
pub fn draw_script_confirm_popup(
    ui: &mut egui::Ui,
    state: &mut AppState,
    _engine: &mut crate::core::CoreState,
) -> PopupAction {
    let Some(pending) = state.dialogs.pending_script_confirm.as_ref() else {
        return PopupAction::Close;
    };
    let name = pending.name.clone();

    let action = {
        let theme_guard = theme::theme();
        let props = ScriptConfirmProps {
            theme: &theme_guard,
            name: &name,
        };
        draw_script_confirm_view(ui, &props)
    };

    match action {
        ScriptConfirmAction::None => PopupAction::None,
        ScriptConfirmAction::Close | ScriptConfirmAction::Cancel => {
            // 취소 — 보류 폐기, 실행 안 함.
            state.dialogs.pending_script_confirm = None;
            PopupAction::Close
        }
        ScriptConfirmAction::Run => {
            // 결정 기록 — frame begin 의 `dispatch_pending_script_confirm` 이 갱신·실행.
            if let Some(p) = state.dialogs.pending_script_confirm.as_mut() {
                p.result = Some(true);
            }
            PopupAction::Close
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run_view_once(events: Vec<egui::Event>) -> ScriptConfirmAction {
        let ctx = egui::Context::default();
        let theme = tasty_themes::mocha_fallback();
        let input = egui::RawInput {
            events,
            ..Default::default()
        };
        let mut captured = ScriptConfirmAction::None;
        let _full_output = ctx.run(input, |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                let props = ScriptConfirmProps {
                    theme: &theme,
                    name: "deploy.lua",
                };
                captured = draw_script_confirm_view(ui, &props);
            });
        });
        captured
    }

    #[test]
    fn escape_returns_close() {
        let action = run_view_once(vec![egui::Event::Key {
            key: egui::Key::Escape,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        }]);
        assert_eq!(action, ScriptConfirmAction::Close);
    }

    #[test]
    fn no_input_returns_none() {
        assert_eq!(run_view_once(Vec::new()), ScriptConfirmAction::None);
    }
}
