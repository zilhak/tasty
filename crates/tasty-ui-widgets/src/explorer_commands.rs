//! Explorer 툴바의 명령 묶음. 경로 필드와 보기 전환 사이에 create 묶음(New folder · New file)과
//! 1px 구분선, view 묶음(Find · Preview 처럼 켜고 끄는 명령)을 icon-only sm 버튼으로 둔다.
//! 칸이 `explorer-toolbar-compact-below` 보다 좁으면 두 묶음을 More 버튼 하나로 접는다.
//! 본체와 갤러리가 같은 함수를 불러 같은 모양을 그린다.

use tasty_icons::Icon;
use tasty_type_appearance::theme::Theme;

use crate::control::ControlSize;
use crate::icon_button::IconButton;

/// 툴바에서 실행하는 명령.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExplorerCommand {
    NewFolder,
    NewFile,
    Find,
    /// 미리보기 패널 토글.
    TogglePreview,
}

/// view 묶음의 토글 하나. 목록 순서대로 그린다.
pub struct ExplorerToggle<'a> {
    pub command: ExplorerCommand,
    pub icon: Icon,
    pub label: &'a str,
    pub active: bool,
}

/// 버튼 툴팁과 More 버튼 문구.
pub struct ExplorerCommandLabels<'a> {
    pub new_folder: &'a str,
    pub new_file: &'a str,
    pub more: &'a str,
    /// 쓸 수 없는 폴더에서 create 버튼의 툴팁.
    pub cannot_write: &'a str,
}

/// 명령 묶음의 상태.
pub struct ExplorerCommandsView<'a> {
    /// `None` 이면 create 묶음을 숨긴다(원격 explorer). `Some(false)` 면 쓸 수 없는 폴더라 비활성이다.
    pub create: Option<bool>,
    pub toggles: &'a [ExplorerToggle<'a>],
    /// 칸이 좁아 More 하나로 접는다.
    pub compact: bool,
    pub labels: ExplorerCommandLabels<'a>,
}

/// 눌린 것. More 는 메뉴를 띄울 버튼 rect 를 함께 준다.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ExplorerCommandClick {
    Run(ExplorerCommand),
    More(egui::Rect),
}

/// 칸 폭이 이 값보다 좁으면 명령 묶음을 More 로 접는다.
pub fn explorer_commands_compact(theme: &Theme, cell_width: f32) -> bool {
    cell_width < theme.explorer_toolbar_compact_below().value()
}

/// 묶음 안 버튼 사이 간격(디자인 `gap: 1`).
fn button_gap(_theme: &Theme) -> f32 {
    crate::tokens::STRUCT_GAP_1.value()
}

/// 구분선 양옆 여백.
fn separator_margin(theme: &Theme) -> f32 {
    theme.spacing_xs.value()
}

/// 명령 묶음의 폭. 툴바가 경로 필드 폭을 정하기 전에 쓴다.
pub fn explorer_commands_width(theme: &Theme, view: &ExplorerCommandsView<'_>) -> f32 {
    let side = ControlSize::Sm.height(theme);
    if view.compact {
        return side;
    }
    let gap = button_gap(theme);
    let mut buttons = view.toggles.len();
    let mut extra = 0.0;
    if view.create.is_some() {
        buttons += 2;
        if !view.toggles.is_empty() {
            // 구분선은 앞뒤 버튼 간격 사이에 여백과 함께 끼므로 간격 하나가 더 든다.
            extra = theme.border_width.value() + 2.0 * separator_margin(theme) + gap;
        }
    }
    if buttons == 0 {
        return 0.0;
    }
    side * buttons as f32 + gap * (buttons - 1) as f32 + extra
}

/// 명령 묶음을 왼쪽에서 오른쪽으로 그린다. 호출 Ui 의 가로 간격은 바꾸지 않는다.
pub fn explorer_commands(
    ui: &mut egui::Ui,
    theme: &Theme,
    view: &ExplorerCommandsView<'_>,
) -> Option<ExplorerCommandClick> {
    let width = explorer_commands_width(theme, view);
    let height = ControlSize::Sm.height(theme);
    let mut clicked = None;
    ui.allocate_ui_with_layout(
        egui::vec2(width, height),
        egui::Layout::left_to_right(egui::Align::Center),
        |ui| {
            ui.spacing_mut().item_spacing.x = button_gap(theme);
            if view.compact {
                let resp = button(ui, theme, tasty_icons::MORE, false, true, view.labels.more);
                if resp.clicked() {
                    clicked = Some(ExplorerCommandClick::More(resp.rect));
                }
                return;
            }
            if let Some(writable) = view.create {
                let tip = |label| {
                    if writable {
                        label
                    } else {
                        view.labels.cannot_write
                    }
                };
                let items = [
                    (
                        ExplorerCommand::NewFolder,
                        tasty_icons::FOLDER_PLUS,
                        view.labels.new_folder,
                    ),
                    (
                        ExplorerCommand::NewFile,
                        tasty_icons::FILE_PLUS,
                        view.labels.new_file,
                    ),
                ];
                for (command, icon, label) in items {
                    if button(ui, theme, icon, false, writable, tip(label)).clicked() {
                        clicked = Some(ExplorerCommandClick::Run(command));
                    }
                }
                if !view.toggles.is_empty() {
                    separator(ui, theme);
                }
            }
            for toggle in view.toggles {
                if button(ui, theme, toggle.icon, toggle.active, true, toggle.label).clicked() {
                    clicked = Some(ExplorerCommandClick::Run(toggle.command));
                }
            }
        },
    );
    clicked
}

fn button(
    ui: &mut egui::Ui,
    theme: &Theme,
    icon: Icon,
    active: bool,
    enabled: bool,
    tip: &str,
) -> egui::Response {
    IconButton::new()
        .size(ControlSize::Sm)
        .active(active)
        .enabled(enabled)
        .show(ui, theme, &|ui, rect, c| {
            icon.image(rect.height(), c).paint_at(ui, rect)
        })
        .on_hover_text(tip)
}

/// 두 묶음 사이 1px × icon-md 구분선. 양옆에 버튼 간격과 `spacing_xs` 여백을 둔다.
fn separator(ui: &mut egui::Ui, theme: &Theme) {
    let margin = separator_margin(theme);
    let width = theme.border_width.value();
    ui.add_space(margin);
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(width, theme.icon_glyph_size_md.value()),
        egui::Sense::hover(),
    );
    ui.painter()
        .rect_filled(rect, 0.0, theme.separator.to_egui_premultiplied());
    ui.add_space(margin);
}

#[cfg(test)]
mod tests;
