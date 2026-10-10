//! 툴바의 주소창. 공용 PathField 에 최근 폴더를 후보로 주고, 확정한 입력을 `address_action` 에 넘긴다.
//! 원격 확인을 기다리는 동안에는 입력한 경로와 포커스 테두리를 그대로 두고, 기다림이 길어지면 뒤 칸에
//! Spinner 를 보인다. 규칙은 docs/surfaces/explorer/index.md 의 "주소 입력" 절에 있다.

use std::path::Path;
use std::time::Duration;

use tasty_type_appearance::theme::Theme;
use tasty_ui_widgets::{PathField, PathFieldOutcome, PathFieldPending};

use super::super::ExplorerAction;
use super::super::view::ExplorerView;
use crate::adapters::ui::icons;
use crate::i18n::t;

/// 경로를 편집해 Enter·Go로 이동하는 공용 PathField. 최근 디렉터리를 후보로 전달한다.
/// 상태는 surface별로, egui ID는 surface·내부 탭별로 구분한다.
#[allow(clippy::too_many_arguments)]
pub(in super::super) fn address_bar(
    ui: &mut egui::Ui,
    theme: &Theme,
    current: &Path,
    view: &mut ExplorerView,
    id_suffix: &str,
    tab_index: usize,
    recent_dirs: &[String],
    remote: bool,
    action: &mut Option<ExplorerAction>,
) {
    let current_str = current.display().to_string();
    let candidates: Vec<&str> = recent_dirs.iter().map(String::as_str).collect();
    let folder_icon = |ui: &mut egui::Ui, rect: egui::Rect, c: egui::Color32| {
        icons::FOLDER_OPEN
            .image(rect.height(), c)
            .paint_at(ui, rect);
    };
    let go_icon = |ui: &mut egui::Ui, rect: egui::Rect, c: egui::Color32| {
        icons::ARROW_RIGHT
            .image(rect.height(), c)
            .paint_at(ui, rect);
    };
    let delay = Duration::from_secs_f32(theme.explorer_address_pending_delay().to_secs_f32());
    let pending = view.address_waiting().map(|waited| PathFieldPending {
        spinner: spinner_shown(waited, delay)
            .then(|| theme.explorer_address_pending_size().value()),
        tooltip: t("explorer.address.checking"),
    });
    if let Some(waited) = view.address_waiting()
        && !spinner_shown(waited, delay)
    {
        ui.ctx().request_repaint_after(delay - waited);
    }
    let salt = format!("explorer_addr_{id_suffix}_{tab_index}");
    let outcome = PathField::new(&salt)
        .placeholder(t("explorer.address.placeholder"))
        .empty_label(t("explorer.address.empty"))
        .leading_icon(&folder_icon)
        .row_icon(&folder_icon)
        .go_icon(&go_icon)
        .go_tooltip(t("explorer.address.go"))
        .focus(std::mem::take(&mut view.focus_address))
        .pending(pending)
        .show(
            ui,
            theme,
            &mut view.addr_buffer,
            &mut view.addr_editing,
            &mut view.addr_active,
            &candidates,
            &current_str,
        );
    // 기다리는 동안 주소창을 다시 편집하면 그 확인은 버린다. 새로 확정하면 새 확인이 시작된다.
    if view.addr_editing {
        view.cancel_address_probe();
    }
    let input = match outcome {
        PathFieldOutcome::Navigate(input) => Some(input),
        _ => None,
    };
    if action.is_none() {
        *action = view.address_action(ui.ctx(), input.as_deref(), current, remote);
    }
}

/// 기다린 시간이 지연을 넘었으면 Spinner 를 보인다. 빨리 끝나는 확인에서 깜박이지 않게 한다.
fn spinner_shown(waited: Duration, delay: Duration) -> bool {
    waited >= delay
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_spinner_waits_for_the_delay() {
        let delay = Duration::from_millis(200);
        assert!(!spinner_shown(Duration::ZERO, delay));
        assert!(!spinner_shown(Duration::from_millis(199), delay));
        assert!(spinner_shown(Duration::from_millis(200), delay));
    }
}
