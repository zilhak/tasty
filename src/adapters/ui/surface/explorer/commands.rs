//! 툴바의 명령 묶음. 경로 필드와 보기 전환 사이에 둔다.
//! 원격 explorer 는 create 묶음을 숨기고, 쓸 수 없는 폴더에서는 비활성으로 둔다.

use std::path::Path;

use tasty_type_appearance::theme::Theme;
use tasty_ui_widgets::{
    ExplorerCommand, ExplorerCommandClick, ExplorerCommandLabels, ExplorerCommandsView,
    ExplorerToggle, explorer_commands, explorer_commands_compact, explorer_commands_width,
};

use super::ExplorerAction;
use super::view::ExplorerView;
use crate::i18n::t;

fn labels() -> ExplorerCommandLabels<'static> {
    ExplorerCommandLabels {
        new_folder: t("explorer.command.new_folder"),
        new_file: t("explorer.command.new_file"),
        more: t("explorer.command.more"),
        cannot_write: t("explorer.command.cannot_write"),
    }
}

/// view 묶음. 목록 순서대로 그린다.
fn toggles(view: &ExplorerView) -> [ExplorerToggle<'static>; 1] {
    [ExplorerToggle {
        command: ExplorerCommand::Find,
        icon: crate::adapters::ui::icons::SEARCH,
        label: t("explorer.command.find"),
        active: view.find.is_some(),
    }]
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
    let toggles = toggles(view);
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
    let toggles = toggles(view);
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
