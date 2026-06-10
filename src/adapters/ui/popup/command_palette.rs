//! Command palette popup — VS Code 스타일 명령 검색기.
//!
//! 사용자가 입력한 쿼리에 대해 `command_palette::search`로 후보를 매칭하고, 위/아래로
//! 선택하고 Enter로 실행한다. 실행 시 `state.command_palette.pending_run`에 action_id를
//! 적재하고 popup을 닫는다. 실제 dispatch는 `MainView`가 다음 프레임 시작에 수행한다.
//!
//! Tier 3 분리: AppState/CoreState 비의존인 [`draw_command_palette_view`] +
//! [`CommandPaletteProps`] + [`CommandPaletteAction`] 과, 큐/state mutation 을
//! 담당하는 wrapper [`draw_command_palette_popup`] 로 나누어 gallery 에서 단독
//! 시각 검증 가능.

use crate::adapters::ui::popup::PopupAction;
use crate::i18n::t;
use crate::state::AppState;
use crate::state::command_palette::{self, PaletteCommand};
use crate::theme;
use crate::theme::Theme;

pub const COMMAND_PALETTE_POPUP_ID: &str = "command_palette";

/// View 입력 — 한 명령 행의 시각/의미 데이터.
#[derive(Debug, Clone)]
pub struct CommandItemView {
    /// 사용자에게 보이는 라벨 (i18n 해결 + 끝 `:` 제거).
    pub label: String,
    /// 우측에 표시할 단축키 텍스트. None 이면 표시하지 않는다.
    pub shortcut: Option<String>,
}

/// View 입력 — palette 한 화면 분의 모든 데이터. AppState/CoreState 비의존.
///
/// `query_buffer` 는 `&mut String` 으로 외부 상태를 그대로 빌려 받는다 — gallery
/// 에서는 로컬 `String` 의 `&mut` 를 주면 된다.
pub struct CommandPaletteProps<'a> {
    /// TextEdit 의 placeholder.
    pub placeholder: String,
    /// 매칭 결과 0 건일 때 표시할 메시지.
    pub no_results_text: String,
    /// 필터된 명령 목록. 순서는 wrapper 의 score 정렬 결과.
    pub items: Vec<CommandItemView>,
    /// 현재 선택 인덱스 — items 가 비어 있으면 무시.
    pub selected_index: usize,
    /// 쿼리 입력 버퍼. View 가 `TextEdit` 으로 직접 mutate.
    pub query_buffer: &'a mut String,
}

/// View 의 출력 — 사용자 의도. wrapper 가 state mutation 으로 변환.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommandPaletteAction {
    None,
    /// 사용자가 한 항목을 Enter 또는 마우스 클릭으로 실행. items 의 인덱스.
    Execute {
        index: usize,
    },
    /// TextEdit 의 내용이 변경됨. wrapper 는 selected_index 를 0 으로 리셋.
    QueryChanged,
    /// 키보드 ↑/↓ 또는 마우스 hover 로 선택 인덱스 변경. items 범위 내에서만 발생.
    SelectionChanged(usize),
    /// Escape 키.
    Close,
}

/// Pure 시각 view. AppState/CoreState 비의존.
///
/// 키 입력 (Escape / ↑ / ↓ / Enter) 는 view 가 직접 처리해 의도를 action 으로 변환.
/// TextEdit 의 change 도 동일하게 `QueryChanged` 로 일원화한다.
pub fn draw_command_palette_view(
    ui: &mut egui::Ui,
    theme: &Theme,
    props: &mut CommandPaletteProps<'_>,
) -> CommandPaletteAction {
    if ui.ctx().input(|i| i.key_pressed(egui::Key::Escape)) {
        return CommandPaletteAction::Close;
    }

    let (up, down, enter) = ui.ctx().input(|i| {
        (
            i.key_pressed(egui::Key::ArrowUp),
            i.key_pressed(egui::Key::ArrowDown),
            i.key_pressed(egui::Key::Enter),
        )
    });

    let mut action = CommandPaletteAction::None;

    if !props.items.is_empty() {
        if down {
            let next = (props.selected_index + 1).min(props.items.len() - 1);
            if next != props.selected_index {
                action = CommandPaletteAction::SelectionChanged(next);
            }
        }
        if up {
            let next = props.selected_index.saturating_sub(1);
            if next != props.selected_index {
                action = CommandPaletteAction::SelectionChanged(next);
            }
        }
    }

    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = 4.0;

        let resp = ui.add(
            egui::TextEdit::singleline(props.query_buffer)
                .hint_text(tasty_egui_theme::hint_text(
                    theme,
                    props.placeholder.clone(),
                ))
                .desired_width(ui.available_width() - 8.0)
                .font(egui::TextStyle::Body),
        );
        if !resp.has_focus() {
            resp.request_focus();
        }
        if resp.changed() {
            action = CommandPaletteAction::QueryChanged;
        }

        ui.separator();

        if props.items.is_empty() {
            ui.label(
                egui::RichText::new(&props.no_results_text)
                    .color(theme.subtext0.to_egui())
                    .italics(),
            );
        } else {
            let row_height = 24.0;
            let selected_idx = props.selected_index;
            egui::ScrollArea::vertical()
                .max_height(280.0)
                .show(ui, |ui| {
                    for (i, item) in props.items.iter().enumerate() {
                        let (rect, resp) = ui.allocate_exact_size(
                            egui::vec2(ui.available_width(), row_height),
                            egui::Sense::click(),
                        );
                        let is_selected = i == selected_idx;
                        if is_selected {
                            ui.painter().rect_filled(
                                rect,
                                4.0,
                                theme.active_overlay.to_egui_premultiplied(),
                            );
                        } else if resp.hovered() {
                            ui.painter().rect_filled(
                                rect,
                                4.0,
                                theme.hover_overlay.to_egui_premultiplied(),
                            );
                        }
                        let color: egui::Color32 = if is_selected || resp.hovered() {
                            theme.text.into()
                        } else {
                            theme.subtext0.into()
                        };
                        ui.painter().text(
                            egui::pos2(rect.min.x + 8.0, rect.center().y),
                            egui::Align2::LEFT_CENTER,
                            &item.label,
                            egui::FontId::proportional(theme.font_size_body.value()),
                            color,
                        );

                        if let Some(shortcut) = &item.shortcut {
                            ui.painter().text(
                                egui::pos2(rect.max.x - 8.0, rect.center().y),
                                egui::Align2::RIGHT_CENTER,
                                shortcut,
                                egui::FontId::proportional(theme.font_size_body.value() - 1.0),
                                theme.subtext0.into(),
                            );
                        }

                        if resp.clicked() {
                            action = CommandPaletteAction::Execute { index: i };
                        }
                    }
                });
        }
    });

    if enter && !props.items.is_empty() {
        action = CommandPaletteAction::Execute {
            index: props.selected_index,
        };
    }

    action
}

/// 매칭 결과를 `(items, static_ids)` 쌍으로 변환.
///
/// `static_ids[i]` 는 `items[i]` 의 원 `PaletteCommand.id` (`&'static str`).
/// wrapper 가 view 의 `Execute { index }` 를 받아 `pending_run` 에 static id 를
/// 저장하기 위해 같은 순서가 보존된 별도 vec 이 필요하다.
///
/// 별도 함수로 분리해 view 와 무관하게 단위 테스트 가능.
fn items_from_state(
    commands: &[PaletteCommand],
    labels: &[String],
    query: &str,
    bindings_for: impl Fn(&str) -> Option<String>,
) -> (Vec<CommandItemView>, Vec<&'static str>) {
    let matches = command_palette::search(query, commands, labels);
    let mut items = Vec::with_capacity(matches.len());
    let mut ids = Vec::with_capacity(matches.len());
    for (_score, cmd) in matches {
        let raw_label = commands
            .iter()
            .position(|c| c.id == cmd.id)
            .and_then(|i| labels.get(i))
            .map(|s| s.trim_end_matches(':').to_string())
            .unwrap_or_default();
        items.push(CommandItemView {
            label: raw_label,
            shortcut: bindings_for(cmd.id),
        });
        ids.push(cmd.id);
    }
    (items, ids)
}

/// PopupDef.draw_fn — `state.command_palette` 와 `engine.settings` 를 어댑팅하고
/// view 를 호출한다. props 추출 → view 호출 → action 처리.
pub fn draw_command_palette_popup(
    ui: &mut egui::Ui,
    state: &mut AppState,
    engine: &mut crate::core::CoreState,
) -> PopupAction {
    let commands = command_palette::all_commands();
    let labels: Vec<String> = commands.iter().map(label_for).collect();
    let (items, static_ids) =
        items_from_state(&commands, &labels, &state.command_palette.query, |id| {
            engine
                .settings
                .keybindings
                .get_bindings(id)?
                .first()
                .cloned()
        });

    // Clamp selection within result range — view 가 받는 selected_index 가 항상
    // 유효 범위 안에 있도록 보장.
    if items.is_empty() {
        state.command_palette.selected = 0;
    } else if state.command_palette.selected >= items.len() {
        state.command_palette.selected = items.len() - 1;
    }

    let mut props = CommandPaletteProps {
        placeholder: t("command_palette.placeholder").to_string(),
        no_results_text: t("command_palette.no_results").to_string(),
        items,
        selected_index: state.command_palette.selected,
        query_buffer: &mut state.command_palette.query,
    };

    let view_action = draw_command_palette_view(ui, &theme::theme(), &mut props);

    match view_action {
        CommandPaletteAction::None => PopupAction::None,
        CommandPaletteAction::Close => {
            state.command_palette.reset();
            PopupAction::Close
        }
        CommandPaletteAction::QueryChanged => {
            state.command_palette.selected = 0;
            PopupAction::None
        }
        CommandPaletteAction::SelectionChanged(idx) => {
            state.command_palette.selected = idx;
            PopupAction::None
        }
        CommandPaletteAction::Execute { index } => {
            if let Some(action_id) = static_ids.get(index).copied() {
                state.command_palette.pending_run = Some(action_id);
            }
            state.command_palette.reset();
            PopupAction::Close
        }
    }
}

/// label_key를 통해 i18n 라벨을 얻되, 끝의 `:`는 떼어낸다 (Settings UI 라벨 재활용).
fn label_for(cmd: &PaletteCommand) -> String {
    let raw = t(cmd.label_key);
    raw.trim_end_matches(':').to_string()
}

#[cfg(test)]
mod props_tests {
    use super::*;

    #[test]
    fn items_from_state_empty_query_returns_all_in_order() {
        let cmds = vec![
            PaletteCommand {
                id: "a",
                label_key: "k.a",
            },
            PaletteCommand {
                id: "b",
                label_key: "k.b",
            },
        ];
        let labels = vec!["Alpha".to_string(), "Beta".to_string()];
        let (items, ids) = items_from_state(&cmds, &labels, "", |id| match id {
            "a" => Some("Ctrl+A".to_string()),
            _ => None,
        });
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].label, "Alpha");
        assert_eq!(items[0].shortcut.as_deref(), Some("Ctrl+A"));
        assert_eq!(items[1].label, "Beta");
        assert!(items[1].shortcut.is_none());
        assert_eq!(ids, vec!["a", "b"]);
    }

    #[test]
    fn items_from_state_filters_by_query() {
        let cmds = vec![
            PaletteCommand {
                id: "new_workspace",
                label_key: "k.new",
            },
            PaletteCommand {
                id: "close_tab",
                label_key: "k.close",
            },
        ];
        let labels = vec!["New workspace".to_string(), "Close tab".to_string()];
        let (items, ids) = items_from_state(&cmds, &labels, "close", |_| None);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].label, "Close tab");
        assert_eq!(ids, vec!["close_tab"]);
    }

    #[test]
    fn items_from_state_strips_trailing_colon_from_labels() {
        let cmds = vec![PaletteCommand {
            id: "a",
            label_key: "k.a",
        }];
        let labels = vec!["Settings: New window:".to_string()];
        let (items, _ids) = items_from_state(&cmds, &labels, "", |_| None);
        assert_eq!(items[0].label, "Settings: New window");
    }
}

#[cfg(test)]
mod view_tests {
    use super::*;
    use tasty_themes::mocha_fallback;

    fn make_items(count: usize) -> Vec<CommandItemView> {
        (0..count)
            .map(|i| CommandItemView {
                label: format!("Item {i}"),
                shortcut: None,
            })
            .collect()
    }

    /// 1 frame egui Context 안에서 view 함수를 호출하고 결과 action 을 받는다.
    fn run_view(
        items: Vec<CommandItemView>,
        selected_index: usize,
        pressed_key: Option<egui::Key>,
    ) -> CommandPaletteAction {
        let ctx = egui::Context::default();
        let mut action = CommandPaletteAction::None;
        let mut buf = String::new();
        let theme = mocha_fallback();

        let mut raw_input = egui::RawInput::default();
        if let Some(key) = pressed_key {
            raw_input.events.push(egui::Event::Key {
                key,
                physical_key: Some(key),
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            });
        }

        let mut items_opt = Some(items);
        let _full = ctx.run(raw_input, |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                let mut props = CommandPaletteProps {
                    placeholder: "Search…".to_string(),
                    no_results_text: "No matches".to_string(),
                    items: items_opt.take().unwrap_or_default(),
                    selected_index,
                    query_buffer: &mut buf,
                };
                action = draw_command_palette_view(ui, &theme, &mut props);
            });
        });
        action
    }

    #[test]
    fn escape_key_returns_close() {
        let action = run_view(make_items(3), 0, Some(egui::Key::Escape));
        assert_eq!(action, CommandPaletteAction::Close);
    }

    #[test]
    fn enter_key_executes_selected_item() {
        let action = run_view(make_items(3), 1, Some(egui::Key::Enter));
        assert_eq!(action, CommandPaletteAction::Execute { index: 1 });
    }

    #[test]
    fn enter_with_empty_items_does_not_execute() {
        let action = run_view(vec![], 0, Some(egui::Key::Enter));
        assert_eq!(action, CommandPaletteAction::None);
    }

    #[test]
    fn arrow_down_moves_selection() {
        let action = run_view(make_items(3), 0, Some(egui::Key::ArrowDown));
        assert_eq!(action, CommandPaletteAction::SelectionChanged(1));
    }

    #[test]
    fn arrow_up_at_top_does_not_change_selection() {
        let action = run_view(make_items(3), 0, Some(egui::Key::ArrowUp));
        assert_eq!(action, CommandPaletteAction::None);
    }

    #[test]
    fn arrow_down_at_bottom_is_clamped() {
        let action = run_view(make_items(3), 2, Some(egui::Key::ArrowDown));
        // 이미 마지막 인덱스이므로 변화 없음.
        assert_eq!(action, CommandPaletteAction::None);
    }

    #[test]
    fn no_input_returns_none() {
        let action = run_view(make_items(3), 0, None);
        assert_eq!(action, CommandPaletteAction::None);
    }
}
