use crate::i18n::t;
use crate::state::AppState;
use crate::theme;
use crate::ui::popup::{self, PopupAction};
use serde_json::json;

/// Item height in the convert popup menu.
const ITEM_HEIGHT: f32 = 24.0;
/// 빌트인 비표시 kind (변환 메뉴에 등장하면 안 됨).
const HIDDEN_KINDS: &[&str] = &["empty"];
/// 빌트인 우선 표시 순서. 이 목록에 없는 kind는 알파벳순으로 뒤따른다.
const PREFERRED_ORDER: &[&str] = &["terminal", "markdown", "image"];

/// Sizer: 등록된 변환 가능 kind 수에 맞춰 popup 크기를 계산.
/// notification.rs가 프레임마다 호출하므로 plugin이 새 kind를 등록한 직후
/// 자동으로 popup 높이가 맞춰진다.
pub fn convert_popup_sizer(
    state: &AppState,
    engine: &crate::engine_state::CoreState,
) -> egui::Vec2 {
    let count = enumerate_convertible_kinds(state, engine).len();
    convert_popup_size_for(count)
}

/// Default size used when the popup is first registered (registry가 비어 있을 수 있는 시점).
pub fn convert_popup_default_size() -> egui::Vec2 {
    // builtin 4종 + explorer 1 = 5 정도를 가정. sizer가 매 프레임 재계산함.
    convert_popup_size_for(5)
}

fn convert_popup_size_for(count: usize) -> egui::Vec2 {
    let count = count.max(1);
    let item_spacing = 3.0;
    let content_h = count as f32 * ITEM_HEIGHT + (count.saturating_sub(1)) as f32 * item_spacing;
    egui::vec2(
        200.0,
        popup::TITLE_BAR_HEIGHT + popup::CONTENT_MARGIN * 2.0 + content_h,
    )
}

/// PopupDef::draw_fn entry point for the convert surface popup.
pub fn draw_convert_popup(
    ui: &mut egui::Ui,
    state: &mut AppState,
    engine: &mut crate::engine_state::CoreState,
) -> PopupAction {
    match draw_convert_content(ui, state, engine) {
        Some(ConvertResult::Close) => PopupAction::Close,
        Some(ConvertResult::Action(action)) => {
            apply_convert_action(state, engine, action);
            PopupAction::Close
        }
        None => PopupAction::None,
    }
}

/// 변환 가능한 surface kind 한 항목.
struct ConvertItem {
    kind: &'static str,
    label: String,
    shortcut: Option<char>,
}

/// SurfaceKindRegistry로부터 변환 가능한 kind 목록을 생성.
/// - `empty` 같은 시스템 kind는 제외.
/// - 빌트인은 PREFERRED_ORDER, 그 외 plugin kind는 알파벳순.
/// - label: `convert_popup.<kind>`가 번역되어 있으면 그 값, 아니면 registry의
///   `display_name_i18n_key`, 그것도 미번역이면 kind 자체를 대문자로.
/// - shortcut: kind 첫 글자(영문)을 대문자 단축키로. 충돌 시 뒷 항목은 단축키 없음.
fn enumerate_convertible_kinds(
    state: &AppState,
    engine: &crate::engine_state::CoreState,
) -> Vec<ConvertItem> {
    let snapshot = engine.surface_registry.kinds_snapshot();
    let mut kinds: Vec<&'static str> = snapshot
        .iter()
        .map(|(k, _)| *k)
        .filter(|k| !HIDDEN_KINDS.contains(k))
        .collect();
    kinds.sort_by(|a, b| {
        let ia = PREFERRED_ORDER.iter().position(|p| *p == *a);
        let ib = PREFERRED_ORDER.iter().position(|p| *p == *b);
        match (ia, ib) {
            (Some(x), Some(y)) => x.cmp(&y),
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (None, None) => a.cmp(b),
        }
    });

    let mut used_shortcuts: Vec<char> = Vec::new();
    let mut items = Vec::with_capacity(kinds.len());
    for kind in kinds {
        let label = resolve_label(state, engine, kind);
        let shortcut = kind
            .chars()
            .next()
            .filter(|c| c.is_ascii_alphabetic())
            .map(|c| c.to_ascii_uppercase())
            .filter(|c| !used_shortcuts.contains(c));
        if let Some(c) = shortcut {
            used_shortcuts.push(c);
        }
        items.push(ConvertItem {
            kind,
            label,
            shortcut,
        });
    }
    items
}

fn resolve_label(_state: &AppState, engine: &crate::engine_state::CoreState, kind: &str) -> String {
    let popup_key = format!("convert_popup.{kind}");
    let tr = t(&popup_key);
    if tr != popup_key.as_str() {
        return tr.to_string();
    }
    if let Some(def) = engine.surface_registry.get(kind) {
        let key = def.display_name_i18n_key;
        let tr = t(key);
        if tr != key {
            return tr.to_string();
        }
    }
    capitalize_ascii(kind)
}

fn capitalize_ascii(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        None => String::new(),
        Some(f) => f.to_ascii_uppercase().to_string() + c.as_str(),
    }
}

/// Result of drawing the convert popup content.
pub enum ConvertResult {
    /// User selected an action.
    Action(ConvertAction),
    /// User pressed Escape or otherwise wants to close.
    Close,
}

/// Draw the convert surface popup content inside PopupManager.
pub fn draw_convert_content(
    ui: &mut egui::Ui,
    state: &mut AppState,
    engine: &mut crate::engine_state::CoreState,
) -> Option<ConvertResult> {
    let surface_id = state.dialogs.convert_popup?;

    let th = theme::theme();
    let current_kind = current_surface_kind(state, engine, surface_id);
    let popup_w = ui.available_width();

    let items = enumerate_convertible_kinds(state, engine);
    let selectable_indices: Vec<usize> = items
        .iter()
        .enumerate()
        .filter(|(_, it)| Some(it.kind) != current_kind)
        .map(|(i, _)| i)
        .collect();

    let ctx = ui.ctx().clone();

    if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        return Some(ConvertResult::Close);
    }

    let selected = state.dialogs.convert_popup_selected;

    if ctx.input(|i| i.key_pressed(egui::Key::ArrowDown)) && !selectable_indices.is_empty() {
        let new_sel = match selected {
            None => selectable_indices[0],
            Some(cur) => {
                if let Some(pos) = selectable_indices.iter().position(|&i| i == cur) {
                    selectable_indices[(pos + 1) % selectable_indices.len()]
                } else {
                    selectable_indices[0]
                }
            }
        };
        state.dialogs.convert_popup_selected = Some(new_sel);
    }

    if ctx.input(|i| i.key_pressed(egui::Key::ArrowUp)) && !selectable_indices.is_empty() {
        let new_sel = match selected {
            None => *selectable_indices.last().unwrap(),
            Some(cur) => {
                if let Some(pos) = selectable_indices.iter().position(|&i| i == cur) {
                    selectable_indices
                        [(pos + selectable_indices.len() - 1) % selectable_indices.len()]
                } else {
                    *selectable_indices.last().unwrap()
                }
            }
        };
        state.dialogs.convert_popup_selected = Some(new_sel);
    }

    let mut action: Option<ConvertAction> = None;

    if ctx.input(|i| i.key_pressed(egui::Key::Enter))
        && let Some(sel) = state.dialogs.convert_popup_selected
        && selectable_indices.contains(&sel)
    {
        action = Some(action_for_kind(items[sel].kind));
    }

    // 단축키: physical_key 사용 (한글 IME 활성 시에도 영문 매칭 보장).
    // 팝업 open 시 set_ime_allowed(false)로 IME가 비활성화되어 있다.
    ctx.input(|i| {
        for event in &i.events {
            if let egui::Event::Key {
                physical_key,
                pressed: true,
                modifiers,
                ..
            } = event
                && modifiers.is_none()
                && let Some(key) = physical_key
                && let Some(ch) = letter_key_to_char(key)
                && let Some(item) = items
                    .iter()
                    .find(|it| it.shortcut == Some(ch) && Some(it.kind) != current_kind)
            {
                action = Some(action_for_kind(item.kind));
            }
        }
    });

    let selected = state.dialogs.convert_popup_selected;
    for (idx, item) in items.iter().enumerate() {
        let is_current = Some(item.kind) == current_kind;
        let is_selected = selected == Some(idx);

        let shortcut_str: String = item.shortcut.map(|c| c.to_string()).unwrap_or_default();
        let label = if is_current {
            format!("  \u{2713} {}    {}", item.label, shortcut_str)
        } else {
            format!("    {}    {}", item.label, shortcut_str)
        };
        let text_color = if is_current { th.overlay0 } else { th.text };

        let sense = if is_current {
            egui::Sense::hover()
        } else {
            egui::Sense::click()
        };
        let (rect, resp) = ui.allocate_exact_size(egui::vec2(popup_w, ITEM_HEIGHT), sense);

        let highlight = (!is_current && resp.hovered()) || is_selected;
        if highlight {
            ui.painter()
                .rect_filled(rect, 0.0, th.hover_overlay.to_egui_premultiplied());
        }
        if !is_current && resp.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }

        let text_pos = egui::pos2(
            rect.min.x + th.spacing_sm.value(),
            rect.center().y - th.font_size_body.value() / 2.0,
        );
        ui.painter().text(
            text_pos,
            egui::Align2::LEFT_TOP,
            &label,
            egui::FontId::proportional(th.font_size_body.value()),
            text_color.into(),
        );

        if resp.clicked() && !is_current {
            action = Some(action_for_kind(item.kind));
        }
    }

    action.map(ConvertResult::Action)
}

/// Apply the convert action to the state.
pub fn apply_convert_action(
    state: &mut AppState,
    engine: &mut crate::engine_state::CoreState,
    action: ConvertAction,
) {
    let Some(surface_id) = state.dialogs.convert_popup else {
        return;
    };

    match action {
        ConvertAction::Terminal => {
            state.dispatch_intent(
                crate::intent::Intent::ConvertSurface {
                    surface_id,
                    target: crate::intent::ConvertTarget::Terminal,
                }
                .from_user_menu("convert/terminal"),
            );
        }
        ConvertAction::Markdown => {
            let pane_id = state.active_workspace(engine).focused_pane;
            state.dialogs.markdown_convert_surface_id = Some(surface_id);
            state.dialogs.file_open_pane_id = Some(pane_id);
            state.dialogs.markdown_open_buffer.clear();
            state.dispatch_intent(
                crate::intent::Intent::OpenPopup {
                    id: "markdown_open",
                    mode: crate::intent::OpenPopupMode::WithScope(popup::PopupScope::Surface(
                        surface_id,
                    )),
                }
                .from_user_menu("convert/markdown"),
            );
        }
        ConvertAction::Image => {
            state.dispatch_intent(
                crate::intent::Intent::ConvertSurface {
                    surface_id,
                    target: crate::intent::ConvertTarget::Kind {
                        kind: "image".to_string(),
                        params: json!({}),
                    },
                }
                .from_user_menu("convert/image"),
            );
        }
        ConvertAction::Kind(kind) => {
            state.dispatch_intent(
                crate::intent::Intent::ConvertSurface {
                    surface_id,
                    target: crate::intent::ConvertTarget::Kind {
                        kind,
                        params: json!({}),
                    },
                }
                .from_user_menu("convert/kind"),
            );
        }
    }
}

#[derive(Clone)]
pub enum ConvertAction {
    Terminal,
    Markdown,
    Image,
    /// Plugin이 제공하는 kind 또는 별도 인자 없이 생성 가능한 kind.
    Kind(String),
}

fn action_for_kind(kind: &str) -> ConvertAction {
    match kind {
        "terminal" => ConvertAction::Terminal,
        "markdown" => ConvertAction::Markdown,
        "image" => ConvertAction::Image,
        other => ConvertAction::Kind(other.to_string()),
    }
}

fn letter_key_to_char(key: &egui::Key) -> Option<char> {
    use egui::Key;
    Some(match key {
        Key::A => 'A',
        Key::B => 'B',
        Key::C => 'C',
        Key::D => 'D',
        Key::E => 'E',
        Key::F => 'F',
        Key::G => 'G',
        Key::H => 'H',
        Key::I => 'I',
        Key::J => 'J',
        Key::K => 'K',
        Key::L => 'L',
        Key::M => 'M',
        Key::N => 'N',
        Key::O => 'O',
        Key::P => 'P',
        Key::Q => 'Q',
        Key::R => 'R',
        Key::S => 'S',
        Key::T => 'T',
        Key::U => 'U',
        Key::V => 'V',
        Key::W => 'W',
        Key::X => 'X',
        Key::Y => 'Y',
        Key::Z => 'Z',
        _ => return None,
    })
}

/// Get the current surface kind for a specific surface ID.
/// Split tab의 leaf surface도 정확히 식별한다.
fn current_surface_kind(
    _state: &AppState,
    engine: &crate::engine_state::CoreState,
    surface_id: u32,
) -> Option<&'static str> {
    for ws in &engine.workspaces {
        for &pid in &ws.pane_layout().all_pane_ids() {
            if let Some(pane) = ws.pane_layout().find_pane(pid) {
                for tab in &pane.tabs {
                    if !tab.contains_surface(surface_id) {
                        continue;
                    }
                    if let Some(leaf) = tab.layout().find_surface(surface_id) {
                        return Some(leaf.kind());
                    }
                    return Some(tab.surface().kind());
                }
            }
        }
    }
    None
}
