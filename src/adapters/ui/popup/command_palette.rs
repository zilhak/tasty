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

use crate::adapters::ui::icons;
use crate::adapters::ui::popup::PopupAction;
use crate::i18n::t;
use crate::state::AppState;
use crate::state::command_palette::{self, PaletteCommand};
use crate::theme;
use crate::theme::Theme;
use tasty_settings::KeybindingSettings;
use tasty_ui_widgets::{margin_all, margin_sym};

pub const COMMAND_PALETTE_POPUP_ID: &str = "command_palette";

/// View 입력 — 한 명령 행의 시각/의미 데이터.
#[derive(Debug, Clone)]
pub struct CommandItemView {
    /// 사용자에게 보이는 라벨 (i18n 해결 + 끝 `:` 제거).
    pub label: String,
    /// 우측에 표시할 단축키 — 키캡 토큰 단위(`["Ctrl","Shift","N"]`). 빈 vec 이면
    /// 표시하지 않는다. `+` 구분자 모호성 회피를 위해 단일 문자열이 아닌 토큰 벡터.
    pub shortcut_keys: Vec<String>,
    /// 행 좌측 leading 아이콘. 6개 디자인 명시 명령은 전용 아이콘, 나머지 동적
    /// 명령은 `COMMAND` fallback 글리프. None 이면 빈 슬롯 (라벨 정렬은 유지).
    pub icon: Option<icons::Icon>,
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
    /// 푸터 힌트 — 네비게이션 동작 라벨 (`↑↓ {navigate}`).
    pub hint_navigate: String,
    /// 푸터 힌트 — 실행 동작 라벨 (`↵ {run}`).
    pub hint_run: String,
    /// 푸터 힌트 — 닫기 동작 라벨 (`esc {close}`).
    pub hint_close: String,
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

    // 디자인(command_palette.jsx:51 `active = n===0 && q!==""`): 쿼리가 비면 어떤
    // 행도 강조하지 않는다. 한 글자라도 입력하면 첫 매칭 행이 강조된다.
    let query_empty = props.query_buffer.is_empty();

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

    // design-parity: 디자인 command_palette.jsx 는 컨테이너 패딩 0 + 구역별 패딩
    // (search 10 / list 6 / footer 8,12). search·list 는 off-grid 라 4px 그리드의
    // 가장 가까운 값으로 snap(10→space-sm(8), 6→space-sm/space-xs(8/4)). content_margin
    // 은 command_palette 한정 0(popup.rs) 이라 full 은 popup 가장자리. 구역 divider 는
    // Frame 실제 좌표에 그린다.
    let full = ui.max_rect();
    let sep = egui::Stroke::new(theme.border_width.value(), theme.border_strong());
    ui.spacing_mut().item_spacing.y = 0.0;

    // ── 검색 구역 (디자인 padding 10 → space-sm(8) snap, Input control-height 28, borderBottom) ──
    let mut query_changed = false;
    let search_ir = egui::Frame::NONE
        .inner_margin(margin_all(theme.spacing_sm))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.set_min_height(28.0); // 디자인 Input control-height
                let icon_size = 16.0;
                let (icon_rect, _) =
                    ui.allocate_exact_size(egui::vec2(icon_size, icon_size), egui::Sense::hover());
                icons::SEARCH
                    .image(icon_size, theme.text_muted().to_egui())
                    .paint_at(ui, icon_rect);
                let resp = ui.add(
                    egui::TextEdit::singleline(props.query_buffer)
                        .hint_text(tasty_egui_theme::hint_text(
                            theme,
                            props.placeholder.clone(),
                        ))
                        .desired_width(ui.available_width())
                        .font(egui::TextStyle::Body),
                );
                if !resp.has_focus() {
                    resp.request_focus();
                }
                if resp.changed() {
                    query_changed = true;
                }
            });
        });
    if query_changed {
        action = CommandPaletteAction::QueryChanged;
    }
    ui.painter()
        .hline(full.x_range(), search_ir.response.rect.bottom(), sep);

    // footer 높이 예약 (디자인 footer ≈ hint row + pad8*2 + border1 = 31).
    let footer_h = theme.font_size_caption.value() + 20.0;
    let footer_top = full.bottom() - footer_h;

    // ── 리스트 구역 (디자인 padding 6 → space-sm/space-xs(8,4) snap, MenuItem height 28) ──
    // 같은 6 이 축마다 다른 값으로 snap: x 는 8(spacing_sm) 로 검색 구역 좌우 inset 과
    // 맞춰 두 구역의 좌측 정렬선을 통일하고, y 는 더 타이트한 4(spacing_xs) 로 둬
    // 스크롤 리스트 높이가 행 수만큼 불필요하게 늘어나지 않게 한다.
    egui::Frame::NONE
        .inner_margin(margin_sym(theme.spacing_sm, theme.spacing_xs))
        .show(ui, |ui| {
            if props.items.is_empty() {
                ui.label(
                    egui::RichText::new(&props.no_results_text)
                        .color(theme.text_muted().to_egui())
                        .italics(),
                );
                return;
            }
            let row_height = 28.0; // 디자인 MenuItem control-height
            let selected_idx = props.selected_index;
            let list_h = (footer_top - ui.cursor().top() - 6.0).max(row_height);
            egui::ScrollArea::vertical()
                .max_height(list_h)
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    for (i, item) in props.items.iter().enumerate() {
                        let (rect, resp) = ui.allocate_exact_size(
                            egui::vec2(ui.available_width(), row_height),
                            egui::Sense::click(),
                        );
                        let is_selected = row_highlighted(query_empty, i, selected_idx);
                        if is_selected {
                            ui.painter().rect_filled(
                                rect,
                                2.0,
                                theme.active_overlay.to_egui_premultiplied(),
                            );
                        } else if resp.hovered() {
                            ui.painter().rect_filled(
                                rect,
                                2.0,
                                theme.hover_overlay.to_egui_premultiplied(),
                            );
                        }
                        let color: egui::Color32 = if is_selected || resp.hovered() {
                            theme.text_primary().into()
                        } else {
                            theme.text_muted().into()
                        };

                        // 디자인 MenuItem: padding 0 12, icon 15, gap 8.
                        let pad_x = 12.0;
                        let icon_size = 15.0;
                        let icon_gap = 8.0;
                        if let Some(icon) = item.icon {
                            let icon_rect = egui::Rect::from_min_size(
                                egui::pos2(rect.min.x + pad_x, rect.center().y - icon_size / 2.0),
                                egui::vec2(icon_size, icon_size),
                            );
                            icon.image(icon_size, color).paint_at(ui, icon_rect);
                        }
                        let label_x = rect.min.x + pad_x + icon_size + icon_gap;
                        ui.painter().text(
                            egui::pos2(label_x, rect.center().y),
                            egui::Align2::LEFT_CENTER,
                            &item.label,
                            egui::FontId::proportional(theme.font_size_body.value()),
                            color,
                        );
                        draw_keycaps(
                            ui,
                            theme,
                            rect.max.x - pad_x,
                            rect.center().y,
                            &item.shortcut_keys,
                        );
                        if resp.clicked() {
                            action = CommandPaletteAction::Execute { index: i };
                        }
                    }
                });
        });

    // ── footer (디자인 padding 8 12, gap 14, mono 10.5, borderTop) — 바닥 고정 ──
    let cur = ui.cursor().top();
    if cur < footer_top {
        ui.add_space(footer_top - cur);
    }
    ui.painter().hline(full.x_range(), footer_top, sep);
    egui::Frame::NONE
        .inner_margin(egui::Margin {
            left: 12,
            right: 12,
            top: 8,
            bottom: 8,
        })
        .show(ui, |ui| {
            let hint_color = theme.text_muted().to_egui();
            // 디자인 footer fontSize 10.5 (토큰 아닌 raw).
            let hint_font = egui::FontId::monospace(10.5);
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 14.0;
                for hint in [
                    format!("↑↓ {}", props.hint_navigate),
                    format!("↵ {}", props.hint_run),
                    format!("esc {}", props.hint_close),
                ] {
                    ui.label(
                        egui::RichText::new(hint)
                            .font(hint_font.clone())
                            .color(hint_color),
                    );
                }
            });
        });

    if enter && !props.items.is_empty() {
        action = CommandPaletteAction::Execute {
            index: props.selected_index,
        };
    }

    action
}

/// 행 강조 여부 — 쿼리가 비어있지 않고(`!query_empty`) 선택 인덱스와 일치할 때만.
/// 디자인 `command_palette.jsx:51 active = n===0 && q!==""` 의 일반화(선택 인덱스 n).
fn row_highlighted(query_empty: bool, row: usize, selected: usize) -> bool {
    !query_empty && row == selected
}

/// Kbd 키캡 그룹을 우측 정렬로 그린다 (디자인 `components/core/Kbd.jsx`).
///
/// 각 키는 개별 키캡 박스(`[Ctrl] [Shift] [N]`), 사이에 muted `+` 구분자. `right_x`
/// 에서 좌측으로 정렬한다. 키캡 스펙: min-width/height 18, h-padding 5, radius-sm,
/// surface-raised 배경, border-strong 1px + 하단 2px(물리 키캡 깊이감), mono caption +
/// text-secondary. `+` 는 text-muted. (디자인 Kbd `border-bottom-width: kbd-shadow-depth`
/// = size-2. egui rect_stroke 는 균일 두께라 하단만 별도 2px 라인으로 근사 — chip.rs kbd 동일.)
/// 키캡 하단 edge 두께 = 디자인 `--tasty-kbd-shadow-depth`(size-2). Theme 에 size-2 토큰이
/// 없어 chip.rs `kbd()` 와 동일하게 고정 2px 로 둔다(디자인 고정 px).
const KEYCAP_BOTTOM_BORDER: f32 = 2.0;

fn draw_keycaps(ui: &egui::Ui, theme: &Theme, right_x: f32, center_y: f32, keys: &[String]) {
    if keys.is_empty() {
        return;
    }
    let cap_h = 18.0;
    let min_w = 18.0;
    let pad_x = 5.0;
    let sep_gap = 4.0;
    let radius = theme.corner_radius_sm.value();
    let key_font = egui::FontId::monospace(theme.font_size_caption.value());
    let key_color = theme.text_secondary().to_egui();
    let sep_color = theme.text_muted().to_egui();

    // 키 galley + 키캡 너비(좁은 키도 min_w 확보).
    let galleys: Vec<_> = keys
        .iter()
        .map(|k| {
            ui.painter()
                .layout_no_wrap(k.clone(), key_font.clone(), key_color)
        })
        .collect();
    let cap_widths: Vec<f32> = galleys
        .iter()
        .map(|g| (g.size().x + pad_x * 2.0).max(min_w))
        .collect();
    let sep_galley = ui
        .painter()
        .layout_no_wrap("+".to_string(), key_font.clone(), sep_color);
    let sep_w = sep_galley.size().x + sep_gap * 2.0;

    // 총 너비 → 좌측 시작점 (우측 정렬).
    let total: f32 = cap_widths.iter().sum::<f32>() + sep_w * keys.len().saturating_sub(1) as f32;
    let mut x = right_x - total;
    let top = center_y - cap_h / 2.0;

    for (idx, galley) in galleys.into_iter().enumerate() {
        if idx > 0 {
            let sep = ui
                .painter()
                .layout_no_wrap("+".to_string(), key_font.clone(), sep_color);
            ui.painter().galley(
                egui::pos2(x + sep_gap, center_y - sep.size().y / 2.0),
                sep,
                sep_color,
            );
            x += sep_w;
        }
        let cap_w = cap_widths[idx];
        let box_rect = egui::Rect::from_min_size(egui::pos2(x, top), egui::vec2(cap_w, cap_h));
        let bw = theme.border_width.value();
        let border = theme.border_strong().to_egui();
        ui.painter()
            .rect_filled(box_rect, radius, theme.surface_raised().to_egui());
        ui.painter().rect_stroke(
            box_rect,
            radius,
            egui::Stroke::new(bw, border),
            egui::StrokeKind::Inside,
        );
        // 하단 2px edge (디자인 Kbd shadow-depth = size-2) — 균일 stroke 위에 덧그린다.
        ui.painter().line_segment(
            [
                egui::pos2(box_rect.left() + radius, box_rect.bottom() - bw),
                egui::pos2(box_rect.right() - radius, box_rect.bottom() - bw),
            ],
            egui::Stroke::new(KEYCAP_BOTTOM_BORDER, border),
        );
        let gx = box_rect.center().x - galley.size().x / 2.0;
        let gy = center_y - galley.size().y / 2.0;
        ui.painter().galley(egui::pos2(gx, gy), galley, key_color);
        x += cap_w;
    }
}

/// 매칭 결과를 `(items, commands)` 쌍으로 변환.
///
/// `commands[i]` 는 `items[i]` 의 원 `PaletteCommand`(clone). wrapper 가 view 의
/// `Execute { index }` 를 받아 `pending_run` 에 저장하기 위해 같은 순서가 보존된
/// 별도 vec 이 필요하다.
///
/// 별도 함수로 분리해 view 와 무관하게 단위 테스트 가능.
fn items_from_state(
    commands: &[PaletteCommand],
    labels: &[String],
    query: &str,
    keys_for: impl Fn(&PaletteCommand) -> Vec<String>,
) -> (Vec<CommandItemView>, Vec<PaletteCommand>) {
    let matches = command_palette::search(query, commands, labels);
    let mut items = Vec::with_capacity(matches.len());
    let mut ids = Vec::with_capacity(matches.len());
    for (_score, cmd) in matches {
        // `cmd` 는 `commands` 슬라이스 원소의 참조이므로 포인터 동일성으로 같은
        // 라벨을 되찾는다 (동적 `PaletteCommand::Plugin` 은 `==` 비교 대신도 되지만
        // 포인터 비교가 더 저렴하고 검색 로직과 완전히 무관하다).
        let raw_label = commands
            .iter()
            .position(|c| std::ptr::eq(c, cmd))
            .and_then(|i| labels.get(i))
            .map(|s| s.trim_end_matches(':').to_string())
            .unwrap_or_default();
        items.push(CommandItemView {
            label: raw_label,
            shortcut_keys: keys_for(cmd),
            icon: icon_for(cmd),
        });
        ids.push(cmd.clone());
    }
    (items, ids)
}

/// PopupDef::on_close 진입점 — 어떤 경로로 닫히든 쿼리·선택 인덱스를 리셋한다.
/// `Close`/`Execute` 경로는 draw_fn 안에서 이미 `reset()`을 부르지만
/// `command_palette::reset()`은 멱등이라 여기서 다시 불러도 안전하다. 이 훅이
/// 실제로 의미를 갖는 건 draw_fn 을 거치지 않는 바깥 클릭/`UiIntent::ClosePopup`
/// 경로 — 지금은 다음 open 시점의 방어적 리셋(`keybinding.rs`/`status_bar.rs`)이
/// 그 틈을 가려주고 있을 뿐이다(그 마스킹 제거는 별도 후속 작업 담당).
pub fn on_close_command_palette_popup(
    _ctx: &egui::Context,
    state: &mut AppState,
    _engine: &mut crate::core::CoreState,
) {
    state.command_palette.reset();
}

/// PopupDef.draw_fn — `state.command_palette` 와 `engine.settings` 를 어댑팅하고
/// view 를 호출한다. props 추출 → view 호출 → action 처리.
pub fn draw_command_palette_popup(
    ui: &mut egui::Ui,
    state: &mut AppState,
    engine: &mut crate::core::CoreState,
) -> PopupAction {
    let commands = command_palette::all_commands(&state.palette_plugin_commands);
    let labels: Vec<String> = commands.iter().map(label_for).collect();
    let (items, matched_commands) =
        items_from_state(&commands, &labels, &state.command_palette.query, |cmd| {
            // Plugin 명령은 shortcut override 해석에 PluginManager 접근이 필요해
            // (팔레트 draw 함수는 `PopupDef` 고정 시그니처상 접근 불가) 키캡을 표시하지
            // 않는다 — 잘못된(override 반영 안 된) 키를 보여주는 것보다 안전.
            let PaletteCommand::Host { id, .. } = cmd else {
                return Vec::new();
            };
            // 첫 바인딩(원문 `alt+n`)을 키캡 토큰(`["Alt","N"]`)으로 변환. `+`키
            // 모호성 회피를 위해 display 문자열 split 대신 format_display_parts 사용.
            engine
                .settings
                .keybindings
                .get_bindings(id)
                .and_then(|b| b.first())
                .map(|s| KeybindingSettings::format_display_parts(s, &engine.settings.general))
                .unwrap_or_default()
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
        hint_navigate: t("command_palette.hint_navigate").to_string(),
        hint_run: t("command_palette.hint_run").to_string(),
        hint_close: t("command_palette.hint_close").to_string(),
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
            if let Some(cmd) = matched_commands.get(index) {
                state.command_palette.pending_run = Some(cmd.clone());
            }
            state.command_palette.reset();
            PopupAction::Close
        }
    }
}

/// leading 아이콘. 디자인(`command_palette.jsx`)이 명시한 6개 호스트 명령은 전용
/// 아이콘, 나머지(동적 호스트 명령 + plugin 명령)는 모두 `COMMAND` fallback 글리프.
fn icon_for(cmd: &PaletteCommand) -> Option<icons::Icon> {
    let PaletteCommand::Host { id, .. } = cmd else {
        return Some(icons::COMMAND);
    };
    Some(match *id {
        "new_workspace" => icons::PLUS,
        "new_tab" => icons::TERM,
        "open_markdown" => icons::MD,
        "toggle_settings" => icons::SETTINGS,
        "split_pane_vertical" => icons::SPLIT,
        _ => icons::COMMAND,
    })
}

/// i18n 라벨을 얻되, 끝의 `:`는 떼어낸다 (Settings UI 라벨 재활용).
///
/// Plugin 명령의 `title_i18n_key`는 그 plugin 자신의 lang 네임스페이스에 등록되어
/// 있다 — 호스트 `t()`는 plugin discovery 시점에 각 plugin의 lang catalog를 같은
/// 전역 resolver에 namespace로 등록해 두므로(`i18n.rs`의 `PluginLangPort::register`,
/// tools_menu의 `label_i18n_key` 해석과 동일 메커니즘) 별도 라우팅 없이 그대로
/// `t()`를 호출하면 된다. 다만 plugin 작성자가 카탈로그에 키를 등록하지 않았을 수
/// 있으므로, `t()`가 키를 그대로 반환하는 경우(미해석) raw 키를 그대로 보여준다
/// (tools_menu의 동일 fallback과 동형).
fn label_for(cmd: &PaletteCommand) -> String {
    let raw = match cmd {
        PaletteCommand::Host { label_key, .. } => t(label_key).to_string(),
        PaletteCommand::Plugin { title_i18n_key, .. } => {
            let translated = t(title_i18n_key);
            if translated == title_i18n_key.as_str() {
                title_i18n_key.clone()
            } else {
                translated.to_string()
            }
        }
    };
    raw.trim_end_matches(':').to_string()
}

#[cfg(test)]
mod props_tests {
    use super::*;

    fn host(id: &'static str, label_key: &'static str) -> PaletteCommand {
        PaletteCommand::Host { id, label_key }
    }

    fn plugin_cmd(plugin_id: &str, command_id: &str) -> PaletteCommand {
        PaletteCommand::Plugin {
            plugin_id: plugin_id.to_string(),
            command_id: command_id.to_string(),
            title_i18n_key: format!("{plugin_id}.{command_id}.title"),
        }
    }

    fn host_id(cmd: &PaletteCommand) -> &str {
        match cmd {
            PaletteCommand::Host { id, .. } => id,
            PaletteCommand::Plugin { .. } => panic!("expected Host"),
        }
    }

    #[test]
    fn items_from_state_empty_query_returns_all_in_order() {
        let cmds = vec![host("a", "k.a"), host("b", "k.b")];
        let labels = vec!["Alpha".to_string(), "Beta".to_string()];
        let (items, ids) = items_from_state(&cmds, &labels, "", |cmd| match host_id(cmd) {
            "a" => vec!["Ctrl".to_string(), "A".to_string()],
            _ => Vec::new(),
        });
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].label, "Alpha");
        assert_eq!(items[0].shortcut_keys, vec!["Ctrl", "A"]);
        assert_eq!(items[1].label, "Beta");
        assert!(items[1].shortcut_keys.is_empty());
        assert_eq!(ids, vec![host("a", "k.a"), host("b", "k.b")]);
    }

    #[test]
    fn items_from_state_filters_by_query() {
        let cmds = vec![host("new_workspace", "k.new"), host("close_tab", "k.close")];
        let labels = vec!["New workspace".to_string(), "Close tab".to_string()];
        let (items, ids) = items_from_state(&cmds, &labels, "close", |_| Vec::new());
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].label, "Close tab");
        assert_eq!(ids, vec![host("close_tab", "k.close")]);
    }

    #[test]
    fn items_from_state_strips_trailing_colon_from_labels() {
        let cmds = vec![host("a", "k.a")];
        let labels = vec!["Settings: New window:".to_string()];
        let (items, _ids) = items_from_state(&cmds, &labels, "", |_| Vec::new());
        assert_eq!(items[0].label, "Settings: New window");
    }

    #[test]
    fn items_from_state_includes_plugin_commands() {
        let cmds = vec![host("a", "k.a"), plugin_cmd("com.example.x", "x.open")];
        let labels = vec!["Alpha".to_string(), "Open X".to_string()];
        let (items, ids) = items_from_state(&cmds, &labels, "open", |_| Vec::new());
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].label, "Open X");
        assert_eq!(ids, vec![plugin_cmd("com.example.x", "x.open")]);
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
                shortcut_keys: Vec::new(),
                icon: None,
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
                    hint_navigate: "navigate".to_string(),
                    hint_run: "run".to_string(),
                    hint_close: "close".to_string(),
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

    #[test]
    fn empty_query_never_highlights() {
        // 빈 쿼리: 선택 인덱스와 일치해도 강조하지 않는다.
        assert!(!row_highlighted(true, 0, 0));
        assert!(!row_highlighted(true, 2, 2));
    }

    #[test]
    fn non_empty_query_highlights_selected_row_only() {
        // 비어있지 않은 쿼리: 선택 인덱스 행만 강조.
        assert!(row_highlighted(false, 0, 0));
        assert!(row_highlighted(false, 3, 3));
        assert!(!row_highlighted(false, 1, 0));
    }
}
