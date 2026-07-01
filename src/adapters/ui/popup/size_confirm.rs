//! 대용량 markdown 열기 확인 팝업 (`markdown_size_confirm`).
//!
//! 1MB 초과 markdown 을 열기 전에 게이트(`file::dispatch::execute_handler_action`)가
//! 띄운다. [열기] 확정 시에만 실제 오픈이 재개된다(`Core::apply_pending_md_open`).
//!
//! 구조는 `file_open.rs` 와 동일하게 순수 view + 본체 wrapper 로 분리한다:
//! - `MdSizeConfirmProps` / `MdSizeConfirmAction` / `draw_md_size_confirm_view`: 순수 view.
//! - `draw_md_size_confirm_popup`: `state.dialogs.pending_md_open` 에서 props 추출,
//!   view 호출, 결정을 `pending_md_open.result` 로 기록/폐기.
//!
//! 디자인: `.claude-workspace/todo-conductor/md-01-size-confirm-gate.md` "디자인 수령 반영"
//! (gallery `plugins.jsx` `MdLargeFilePopup`) — 360px 셸, 경고 태그 + 안내문, Cancel/Open.

use crate::adapters::ui::popup::PopupAction;
use crate::i18n::t;
use crate::state::AppState;
use crate::theme;
use tasty_type_appearance::theme::Theme;
use tasty_ui_widgets::{Button, ButtonVariant, TagVariant, tag};

/// Pure view 의 입력. AppState/CoreState 를 알지 못한다.
pub struct MdSizeConfirmProps<'a> {
    pub theme: &'a Theme,
    /// 열려는 파일 경로(표시용, 축약 가능).
    pub path: &'a str,
    /// 크기 칩 라벨 (예: "3.2 MB").
    pub size_label: &'a str,
}

/// view 가 보고하는 사용자 의도.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MdSizeConfirmAction {
    None,
    /// Escape — 취소로 간주(오픈 안 함).
    Close,
    /// Cancel 버튼 — 오픈 안 함.
    Cancel,
    /// Open 버튼 — 대용량이어도 열기.
    Open,
}

/// 순수 view. AppState/CoreState 접근 금지.
pub fn draw_md_size_confirm_view(
    ui: &mut egui::Ui,
    props: &MdSizeConfirmProps<'_>,
) -> MdSizeConfirmAction {
    let th = props.theme;

    if ui.ctx().input(|i| i.key_pressed(egui::Key::Escape)) {
        return MdSizeConfirmAction::Close;
    }

    let mut action = MdSizeConfirmAction::None;

    ui.spacing_mut().item_spacing.y = th.spacing_sm.value();

    // 제목.
    ui.label(
        egui::RichText::new(t("markdown.large_file.title"))
            .size(th.font_size_body.value())
            .strong()
            .color(th.text_primary().to_egui()),
    );

    // 파일 경로 (mono, muted, 축약).
    ui.add(
        egui::Label::new(
            egui::RichText::new(ellipsize_path(props.path))
                .size(th.font_size_caption.value())
                .family(egui::FontFamily::Monospace)
                .color(th.text_muted().to_egui()),
        )
        .truncate(),
    );

    // 경고 태그 + 안내문.
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = th.spacing_sm.value();
        tag(ui, th, props.size_label, TagVariant::Warning, false);
        ui.label(
            egui::RichText::new(t("markdown.large_file.body"))
                .size(th.font_size_caption.value())
                .color(th.text_secondary().to_egui()),
        );
    });

    ui.add_space(th.spacing_xs.value());

    // 푸터: Cancel(ghost) / Open(primary), 우측 정렬.
    ui.horizontal(|ui| {
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if Button::new(&t("button.open"))
                .variant(ButtonVariant::Primary)
                .show(ui, th)
                .clicked()
            {
                action = MdSizeConfirmAction::Open;
            }
            if Button::new(&t("button.cancel"))
                .variant(ButtonVariant::Ghost)
                .show(ui, th)
                .clicked()
            {
                action = MdSizeConfirmAction::Cancel;
            }
        });
    });

    action
}

/// PopupDef::draw_fn entry.
pub fn draw_md_size_confirm_popup(
    ui: &mut egui::Ui,
    state: &mut AppState,
    _engine: &mut crate::core::CoreState,
) -> PopupAction {
    let Some(pending) = state.dialogs.pending_md_open.as_ref() else {
        // 보류 상태가 없으면 표시할 게 없다 — 닫는다.
        return PopupAction::Close;
    };
    let path = pending.path.clone();
    let size_label = format_size(pending.size);

    let action = {
        let theme_guard = theme::theme();
        let props = MdSizeConfirmProps {
            theme: &theme_guard,
            path: &path,
            size_label: &size_label,
        };
        draw_md_size_confirm_view(ui, &props)
    };

    match action {
        MdSizeConfirmAction::None => PopupAction::None,
        MdSizeConfirmAction::Close | MdSizeConfirmAction::Cancel => {
            // 취소 — 보류 오픈 폐기, 아무것도 안 열림.
            state.dialogs.pending_md_open = None;
            PopupAction::Close
        }
        MdSizeConfirmAction::Open => {
            // 결정 기록 — frame begin 의 `dispatch_pending_md_open` 이 오픈 재개.
            if let Some(p) = state.dialogs.pending_md_open.as_mut() {
                p.result = Some(true);
            }
            PopupAction::Close
        }
    }
}

/// bytes → "3.2 MB" 형태. 10MB 이상은 소수점 없이.
fn format_size(bytes: u64) -> String {
    let mb = bytes as f64 / (1024.0 * 1024.0);
    if mb >= 10.0 {
        format!("{mb:.0} MB")
    } else {
        format!("{mb:.1} MB")
    }
}

/// 긴 경로를 마지막 2 세그먼트로 축약(`.../parent/name`). view 의 truncate 와 병행.
fn ellipsize_path(path: &str) -> String {
    if path.len() <= 48 {
        return path.to_string();
    }
    let sep = if path.contains('\\') { '\\' } else { '/' };
    let parts: Vec<&str> = path.split(sep).filter(|s| !s.is_empty()).collect();
    if parts.len() <= 2 {
        return path.to_string();
    }
    format!(
        "...{sep}{}",
        parts[parts.len() - 2..].join(&sep.to_string())
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run_view_once(events: Vec<egui::Event>) -> MdSizeConfirmAction {
        let ctx = egui::Context::default();
        let theme = tasty_themes::mocha_fallback();
        let input = egui::RawInput {
            events,
            ..Default::default()
        };
        let mut captured = MdSizeConfirmAction::None;
        let _full_output = ctx.run(input, |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                let props = MdSizeConfirmProps {
                    theme: &theme,
                    path: "/docs/big.md",
                    size_label: "3.2 MB",
                };
                captured = draw_md_size_confirm_view(ui, &props);
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
        assert_eq!(action, MdSizeConfirmAction::Close);
    }

    #[test]
    fn no_input_returns_none() {
        assert_eq!(run_view_once(Vec::new()), MdSizeConfirmAction::None);
    }

    #[test]
    fn format_size_examples() {
        assert_eq!(format_size(2 * 1024 * 1024 + 200 * 1024), "2.2 MB");
        assert_eq!(format_size(12 * 1024 * 1024), "12 MB");
    }

    #[test]
    fn ellipsize_keeps_short_paths() {
        assert_eq!(ellipsize_path("/a/b.md"), "/a/b.md");
    }
}
