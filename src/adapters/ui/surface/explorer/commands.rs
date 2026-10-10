//! 툴바의 명령 묶음. 경로 필드와 보기 전환 사이에 둔다.
//! 원격 explorer 는 create 묶음을 숨기고, 쓸 수 없는 폴더에서는 비활성으로 둔다.

use std::path::Path;

use tasty_type_appearance::theme::Theme;
use tasty_ui_widgets::{
    ExplorerCommand, ExplorerCommandClick, ExplorerCommandLabels, ExplorerCommandsView,
    ExplorerToggle, explorer_commands, explorer_commands_compact, explorer_commands_width,
};

use super::ExplorerAction;
use super::view::{ExplorerView, hidden};
use crate::i18n::t;

fn labels() -> ExplorerCommandLabels<'static> {
    ExplorerCommandLabels {
        new_folder: t("explorer.command.new_folder"),
        new_file: t("explorer.command.new_file"),
        more: t("explorer.command.more"),
        cannot_write: t("explorer.command.cannot_write"),
    }
}

/// 숨김 파일 토글의 툴팁. More 행과 같은 동작 문구 뒤에 첫 바인딩을 괄호로 붙인다.
fn hidden_tip(view: &ExplorerView) -> String {
    let action = t(hidden::action_key(view.hidden.show));
    match &view.hidden.shortcut {
        Some(keys) => format!("{action} ({keys})"),
        None => action.to_string(),
    }
}

/// view 묶음. 목록 순서대로 그린다. 좁은 칸에서는 create 묶음과 함께 More 로 접힌다.
fn toggles<'a>(view: &ExplorerView, hidden_tip: &'a str) -> [ExplorerToggle<'a>; 3] {
    [
        ExplorerToggle {
            command: ExplorerCommand::Find,
            icon: crate::adapters::ui::icons::SEARCH,
            label: t("explorer.command.find"),
            active: view.find.is_some(),
        },
        ExplorerToggle {
            command: ExplorerCommand::TogglePreview,
            icon: crate::adapters::ui::icons::COLUMNS,
            label: t("explorer.preview.toggle"),
            active: view.preview.open,
        },
        ExplorerToggle {
            command: ExplorerCommand::ToggleHidden,
            icon: crate::adapters::ui::icons::EYE,
            label: hidden_tip,
            active: view.hidden.show,
        },
    ]
}

fn commands_view<'a>(
    theme: &Theme,
    view: &ExplorerView,
    remote: bool,
    cell_w: f32,
    toggles: &'a [ExplorerToggle<'a>],
) -> ExplorerCommandsView<'a> {
    ExplorerCommandsView {
        create: (!remote).then(|| view.can_write_here()),
        toggles,
        compact: explorer_commands_compact(theme, cell_w),
        labels: labels(),
    }
}

/// 주소창 폭을 정하기 전에 뺄 폭. 묶음 뒤 간격까지 포함한다.
pub(super) fn reserve(
    ui: &egui::Ui,
    theme: &Theme,
    view: &ExplorerView,
    remote: bool,
    cell_w: f32,
) -> f32 {
    let tip = hidden_tip(view);
    let toggles = toggles(view, &tip);
    let w = explorer_commands_width(theme, &commands_view(theme, view, remote, cell_w, &toggles));
    if w > 0.0 {
        w + ui.spacing().item_spacing.x + theme.spacing_sm.value()
    } else {
        0.0
    }
}

/// 명령 묶음을 그리고 뒤에 보기 전환과의 간격을 둔다.
pub(super) fn show(
    ui: &mut egui::Ui,
    theme: &Theme,
    view: &mut ExplorerView,
    root: &Path,
    remote: bool,
    cell_w: f32,
    action: &mut Option<ExplorerAction>,
) {
    let tip = hidden_tip(view);
    let toggles = toggles(view, &tip);
    let commands = commands_view(theme, view, remote, cell_w, &toggles);
    if explorer_commands_width(theme, &commands) <= 0.0 {
        return;
    }
    match explorer_commands(ui, theme, &commands) {
        Some(ExplorerCommandClick::Run(ExplorerCommand::NewFolder)) => {
            view.start_create(root.to_path_buf(), true)
        }
        Some(ExplorerCommandClick::Run(ExplorerCommand::NewFile)) => {
            view.start_create(root.to_path_buf(), false)
        }
        Some(ExplorerCommandClick::Run(ExplorerCommand::Find)) => view.toggle_find(),
        Some(ExplorerCommandClick::Run(ExplorerCommand::TogglePreview)) => view.preview.toggle(),
        Some(ExplorerCommandClick::Run(ExplorerCommand::ToggleHidden)) => view.toggle_hidden(),
        Some(ExplorerCommandClick::More(rect)) if action.is_none() => {
            *action = Some(ExplorerAction::MoreMenu {
                x: rect.left(),
                y: rect.bottom(),
            });
        }
        Some(ExplorerCommandClick::More(_)) | None => {}
    }
    ui.add_space(theme.spacing_sm.value());
}

#[cfg(test)]
mod tests {
    use super::*;
    use tasty_ui_widgets::ControlSize;

    #[test]
    fn preview_and_hidden_sit_in_the_view_group_and_fold_into_more_with_it() {
        let theme = crate::theme::theme();
        let mut view = ExplorerView::new();
        view.preview.toggle();
        view.toggle_hidden();
        let toggles = toggles(&view, "");
        let commands: Vec<_> = toggles.iter().map(|t| (t.command, t.active)).collect();
        assert_eq!(
            commands,
            [
                (ExplorerCommand::Find, false),
                (ExplorerCommand::TogglePreview, true),
                (ExplorerCommand::ToggleHidden, true),
            ]
        );
        let narrow = theme.explorer_toolbar_compact_below().value() - 1.0;
        let folded = commands_view(&theme, &view, false, narrow, &toggles);
        assert_eq!(
            explorer_commands_width(&theme, &folded),
            ControlSize::Sm.height(&theme),
            "under the compact width every command, Preview and hidden files included, is one More button"
        );
    }

    #[test]
    fn the_hidden_toggle_names_its_action_and_the_first_binding() {
        crate::i18n::init("en");
        let mut view = ExplorerView::new();
        assert_eq!(hidden_tip(&view), "Show hidden files");
        view.hidden.shortcut = Some("Ctrl+Shift+.".into());
        assert_eq!(hidden_tip(&view), "Show hidden files (Ctrl+Shift+.)");
        view.toggle_hidden();
        assert_eq!(hidden_tip(&view), "Hide hidden files (Ctrl+Shift+.)");
    }
}
