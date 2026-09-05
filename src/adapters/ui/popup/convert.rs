use crate::adapters::ui::icons;
use crate::adapters::ui::popup::{self, PopupAction};
use crate::i18n::t;
use crate::state::AppState;
use crate::theme;
use crate::theme::Theme;
use egui::emath::GuiRounding as _;
use serde_json::json;
use tasty_type_geometry::length::LogicalPx;

/// Item height in the convert popup menu.
const ITEM_HEIGHT: LogicalPx = LogicalPx(24.0);
/// 빌트인 비표시 kind (변환 메뉴에 등장하면 안 됨).
const HIDDEN_KINDS: &[&str] = &["empty"];
/// 변환 메뉴 상단 우선 표시 순서(bundled UX 정책). 이 목록에 없는 kind 는 알파벳순으로
/// 뒤따른다. 이건 "host 가 kind 의 *동작*을 안다"(=제거 대상 하드코딩)가 아니라 bundled
/// kind 의 *정렬 선호*라는 별도 층위의 host UX 정책이므로 registry 로 데이터화하지 않고
/// 본체에 정책으로 남긴다(generic-kind 마이그레이션 결정). plugin kind 는 여기 없으면
/// 알파벳순으로 자연 편입된다.
const PREFERRED_ORDER: &[&str] = &["terminal", "markdown", "image"];
/// `default_size` 산정 시 가정하는 항목 수. registry 가 비어 있을 수 있는 등록
/// 시점에만 쓰이고, sizer 가 매 프레임 실제 등록된 kind 수로 재계산한다.
/// 현재 빌트인 1 종 (terminal) + plugin 4 종 (markdown, image, explorer, html) = 5.
const DEFAULT_KIND_COUNT: usize = 5;

/// Sizer: 등록된 변환 가능 kind 수에 맞춰 popup 크기를 계산.
/// `popup::frame::draw_popup_layer`가 프레임마다 호출하므로 plugin이 새 kind를
/// 등록한 직후 자동으로 popup 높이가 맞춰진다.
pub fn convert_popup_sizer(state: &AppState, engine: &crate::core::CoreState) -> egui::Vec2 {
    let count = enumerate_convertible_kinds(state, engine).len();
    convert_popup_size_for(count, effective_item_spacing(engine))
}

/// Default size used when the popup is first registered (registry가 비어 있을 수 있는 시점).
pub fn convert_popup_default_size() -> egui::Vec2 {
    // ui_scale 미적용 baseline (medium = 1.0). sizer 가 매 프레임 재계산하므로 실제
    // 렌더링에는 영향 없음 — register 시점 placeholder.
    convert_popup_size_for(DEFAULT_KIND_COUNT, theme::theme().spacing_xs.value())
}

/// `tasty_egui_theme` 가 `style.spacing.item_spacing.y` 로 적용하는 값과 동일하게
/// 계산한다. egui draw 시 `allocate_exact_size` 사이의 vertical gap 이 정확히 이
/// 값이므로 sizer 도 같은 식을 써야 마지막 항목이 잘리지 않는다.
///
/// Theme 토큰 자체가 host UI zoom 곱셈을 이미 반영하므로 (Z-1/Z-2) 여기서
/// 별도 `ui_scale_factor()` 곱셈 없이 그대로 사용한다.
fn effective_item_spacing(_engine: &crate::core::CoreState) -> f32 {
    theme::theme().spacing_xs.value().round_ui()
}

fn convert_popup_size_for(count: usize, item_spacing: f32) -> egui::Vec2 {
    let count = count.max(1);
    let content_h = ITEM_HEIGHT.scaled(count as f32)
        + LogicalPx((count.saturating_sub(1)) as f32 * item_spacing);
    // round_ui 누적 오차 / egui Ui::new 초기 cursor 미세 padding 흡수용 1 px 마진.
    // 마지막 항목 baseline 이 content_rect 경계와 정확히 일치할 때 anti-alias 한 줄이
    // 잘려 보이는 case 예방.
    let safety_margin = 1.0;
    egui::vec2(
        200.0,
        (popup::title_bar_height()
            + popup::content_margin().scaled(2.0)
            + content_h
            + LogicalPx(safety_margin))
        .value(),
    )
}

#[cfg(test)]
mod size_tests {
    use super::*;

    /// 마지막 항목이 잘리지 않으려면 sizer popup_h 가 *실제 필요 height* 이상이어야
    /// 한다. 실제 필요 = title_bar_height() + 2·content_margin() + N·ITEM_HEIGHT
    ///                + (N−1)·actual_spacing.
    fn assert_fits(count: usize, item_spacing: f32) {
        let popup_h = convert_popup_size_for(count, item_spacing).y;
        let needed = popup::title_bar_height()
            + popup::content_margin().scaled(2.0)
            + ITEM_HEIGHT.scaled(count as f32)
            + LogicalPx((count.saturating_sub(1)) as f32 * item_spacing);
        assert!(
            LogicalPx(popup_h) >= needed,
            "popup_h ({popup_h}) < needed ({needed}) for count={count} spacing={item_spacing}"
        );
    }

    #[test]
    fn fits_five_items_medium_scale() {
        // ui_scale=1.0 → spacing_xs(4.0) × 1.0 = 4.0. 옛 하드코딩 3.0 으로는 4 px 부족.
        assert_fits(5, 4.0);
    }

    #[test]
    fn fits_five_items_large_scale() {
        // ui_scale=1.2 → spacing_xs(4.0) × 1.2 = 4.8 → round_ui ≈ 4.78125.
        assert_fits(5, 4.78125);
    }

    #[test]
    fn fits_five_items_small_scale() {
        // ui_scale=0.85 → 3.4 → round_ui ≈ 3.40625.
        assert_fits(5, 3.40625);
    }

    #[test]
    fn fits_single_item() {
        // count=1 이면 gap 0개라 spacing 영향 없음.
        assert_fits(1, 4.0);
    }

    #[test]
    fn fits_many_items_large_scale() {
        // plugin 등록 폭주 가정 (예: 10종). 같은 식이라 안전해야 한다.
        assert_fits(10, 4.78125);
    }
}

/// PopupDef::on_close entry point — 어떤 경로로 닫히든 대상/선택 상태를 비운다.
pub fn on_close_convert_popup(
    _ctx: &egui::Context,
    state: &mut AppState,
    _engine: &mut crate::core::CoreState,
) {
    state.dialogs.convert_popup = None;
    state.dialogs.convert_popup_selected = None;
}

/// PopupDef::draw_fn entry point for the convert surface popup.
pub fn draw_convert_popup(
    ui: &mut egui::Ui,
    state: &mut AppState,
    engine: &mut crate::core::CoreState,
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
    engine: &crate::core::CoreState,
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

fn resolve_label(_state: &AppState, engine: &crate::core::CoreState, kind: &str) -> String {
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

/// Pure visual props for [`draw_convert_view`].
///
/// AppState/CoreState 의존을 *완전히* 제거한 데이터. `String` 으로 owned 화한
/// 것은 gallery mock 에서 임의 mock data 를 만들 수 있게 하기 위함.
#[derive(Debug, Clone)]
pub struct ConvertItemView {
    pub kind: String,
    pub label: String,
    pub shortcut: Option<char>,
    pub is_current: bool,
}

/// Props 일체. 호출처가 AppState/CoreState 에서 추출해서 전달.
#[derive(Debug, Clone, Default)]
pub struct ConvertProps {
    pub items: Vec<ConvertItemView>,
    /// 키보드 선택 위치. None 이면 마우스 호버만 강조.
    pub selected_index: Option<usize>,
}

impl ConvertProps {
    /// `items` 의 인덱스 중 `is_current == false` 인 항목들. selectable 후보.
    pub fn selectable_indices(&self) -> Vec<usize> {
        self.items
            .iter()
            .enumerate()
            .filter(|(_, it)| !it.is_current)
            .map(|(i, _)| i)
            .collect()
    }
}

/// View 의 출력 — 사용자 입력의 의미. wrapper 가 AppState/CoreState 에 반영.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConvertViewAction {
    None,
    /// 사용자가 항목을 클릭. (current 항목은 호출되지 않음.) wrapper 는 `kind`
    /// 를 `action_for_kind` 로 변환해 적용한다.
    Clicked {
        idx: usize,
        kind: String,
    },
}

/// Pure 시각 view. AppState/CoreState 비의존.
///
/// 키보드 처리(Escape/Arrow/Enter/letter shortcut) 는 wrapper 책임 —
/// view 는 마우스 클릭과 시각 강조만 다룬다.
pub fn draw_convert_view(
    ui: &mut egui::Ui,
    theme: &Theme,
    props: &ConvertProps,
) -> ConvertViewAction {
    let popup_w = ui.available_width();
    let mut action = ConvertViewAction::None;

    for (idx, item) in props.items.iter().enumerate() {
        let is_current = item.is_current;
        let is_selected = props.selected_index == Some(idx);

        let shortcut_str: String = item.shortcut.map(|c| c.to_string()).unwrap_or_default();
        // 현재 kind 마커였던 raw `✓`(U+2713)는 UI 폰트에 글리프가 없어 tofu 로
        // 렌더되므로 제거하고, 아래에서 icons::CHECK(SVG)를 좌측 인덴트에 별도로
        // 그린다. 두 분기가 동일한 4-space 인덴트를 써서 라벨 텍스트 x정렬을 맞춘다.
        let label = format!("    {}    {}", item.label, shortcut_str);
        let text_color = if is_current {
            // divergence: overlay0=disabled-role 이나 값은 placeholder(neutral-600), 코드값 보존
            theme.text_placeholder()
        } else {
            theme.text_primary()
        };

        let sense = if is_current {
            egui::Sense::hover()
        } else {
            egui::Sense::click()
        };
        let (rect, resp) = ui.allocate_exact_size(egui::vec2(popup_w, ITEM_HEIGHT.value()), sense);

        if is_selected {
            ui.painter()
                .rect_filled(rect, 0.0, theme.active_overlay.to_egui_premultiplied());
        } else if !is_current && resp.hovered() {
            ui.painter()
                .rect_filled(rect, 0.0, theme.hover_overlay.to_egui_premultiplied());
        }
        if !is_current && resp.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }

        let text_pos = egui::pos2(
            rect.min.x + theme.spacing_sm.value(),
            rect.center().y - theme.font_size_body.value() / 2.0,
        );
        ui.painter().text(
            text_pos,
            egui::Align2::LEFT_TOP,
            &label,
            egui::FontId::proportional(theme.font_size_body.value()),
            text_color.into(),
        );

        // 현재 kind 체크마크 — 라벨 좌측 인덴트(4-space) 자리에 SVG 아이콘으로 배치.
        // 세로는 행 중앙, tint 는 라벨색(text_placeholder)을 그대로 전달해 색을 맞춘다.
        if is_current {
            let icon_sz = theme.font_size_body.value();
            let icon_rect = egui::Rect::from_min_size(
                egui::pos2(text_pos.x, rect.center().y - icon_sz / 2.0),
                egui::vec2(icon_sz, icon_sz),
            );
            icons::CHECK
                .image(icon_sz, text_color.into())
                .paint_at(ui, icon_rect);
        }

        if resp.clicked() && !is_current {
            action = ConvertViewAction::Clicked {
                idx,
                kind: item.kind.clone(),
            };
        }
    }

    action
}

/// 본체 wrapper: AppState/CoreState 로부터 props 추출 + 키보드 처리 + view 호출
/// + action 적용을 담당.
pub fn draw_convert_content(
    ui: &mut egui::Ui,
    state: &mut AppState,
    engine: &mut crate::core::CoreState,
) -> Option<ConvertResult> {
    let surface_id = state.dialogs.convert_popup?;

    let current_kind = current_surface_kind(state, engine, surface_id);
    let internal_items = enumerate_convertible_kinds(state, engine);
    let props = props_from_items(
        &internal_items,
        current_kind,
        state.dialogs.convert_popup_selected,
    );
    let selectable_indices = props.selectable_indices();

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
        action = Some(action_for_kind(engine, internal_items[sel].kind));
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
                && let Some(item) = internal_items
                    .iter()
                    .find(|it| it.shortcut == Some(ch) && Some(it.kind) != current_kind)
            {
                action = Some(action_for_kind(engine, item.kind));
            }
        }
    });

    // 키보드 선택을 view 가 강조할 수 있도록 props 갱신 (Arrow 처리 이후 값).
    let view_props = ConvertProps {
        items: props.items,
        selected_index: state.dialogs.convert_popup_selected,
    };
    let view_action = draw_convert_view(ui, &theme::theme(), &view_props);
    if let ConvertViewAction::Clicked { kind, .. } = view_action {
        action = Some(action_for_kind(engine, &kind));
    }

    action.map(ConvertResult::Action)
}

/// 내부 `ConvertItem` 목록 + 현재 kind 로부터 view 용 props 를 만든다.
/// AppState/CoreState 비의존 — 테스트하기 쉬운 형태.
fn props_from_items(
    items: &[ConvertItem],
    current_kind: Option<&'static str>,
    selected_index: Option<usize>,
) -> ConvertProps {
    let items = items
        .iter()
        .map(|it| ConvertItemView {
            kind: it.kind.to_string(),
            label: it.label.clone(),
            shortcut: it.shortcut,
            is_current: Some(it.kind) == current_kind,
        })
        .collect();
    ConvertProps {
        items,
        selected_index,
    }
}

/// Apply the convert action to the state.
pub fn apply_convert_action(
    state: &mut AppState,
    engine: &mut crate::core::CoreState,
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
        ConvertAction::RequiresInput(kind) => {
            // 이 kind 는 convert 전 파일 입력이 필요하다(capability `convert_requires_input`).
            // host 는 kind 이름을 모르고 registry 데이터로 그 kind plugin 의 file-open
            // 팝업을 연다 — surface_id 를 실어 plugin 이 제자리 변환하게 한다.
            state.enqueue_convert_input_popup(engine, &kind, Some(surface_id));
        }
        ConvertAction::Kind(kind) => {
            state.dispatch_intent(
                crate::intent::Intent::ConvertSurface {
                    surface_id,
                    target: crate::intent::ConvertTarget::Kind {
                        cwd: None,
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
    /// terminal 은 PTY spawn 이라 전용 `ConvertTarget::Terminal` 경로를 탄다(generic
    /// Kind 로 수렴 불가 — host 책임의 PTY 생성).
    Terminal,
    /// `convert_requires_input` capability 를 선언한 kind. 변환 전 그 kind plugin 의
    /// file-open 팝업을 먼저 띄운다(파일 필수) — 빈 params 로 바로 변환하는 generic
    /// Kind 와 동작이 다르다. host 는 kind 이름을 모르고 capability 로만 판정한다.
    RequiresInput(String),
    /// Plugin이 제공하는 kind 또는 별도 인자 없이 생성 가능한 kind. image 를 포함해
    /// 파일 없이 빈 params 로 즉시 변환 가능한 모든 kind 가 이 경로로 수렴한다.
    Kind(String),
}

fn action_for_kind(engine: &crate::core::CoreState, kind: &str) -> ConvertAction {
    // terminal 만 전용 PTY 경로(위 variant 주석). 나머지는 registry capability 로 판정:
    // `convert_requires_input` 이면 파일 입력 팝업 경유, 아니면 빈 params 즉시 변환.
    // kind 이름 하드코딩 없이 데이터로만 라우팅한다.
    if kind == "terminal" {
        return ConvertAction::Terminal;
    }
    if engine
        .surface_registry
        .get(kind)
        .is_some_and(|d| d.convert_requires_input)
    {
        return ConvertAction::RequiresInput(kind.to_string());
    }
    ConvertAction::Kind(kind.to_string())
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
    engine: &crate::core::CoreState,
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

#[cfg(test)]
mod props_tests {
    use super::*;

    fn mk(kind: &'static str, shortcut: Option<char>) -> ConvertItem {
        ConvertItem {
            kind,
            label: kind.to_string(),
            shortcut,
        }
    }

    #[test]
    fn marks_current_kind() {
        let items = vec![mk("terminal", Some('T')), mk("markdown", Some('M'))];
        let props = props_from_items(&items, Some("terminal"), None);
        assert!(props.items[0].is_current);
        assert!(!props.items[1].is_current);
    }

    #[test]
    fn selectable_indices_excludes_current() {
        let items = vec![
            mk("terminal", Some('T')),
            mk("markdown", Some('M')),
            mk("image", Some('I')),
        ];
        let props = props_from_items(&items, Some("markdown"), None);
        assert_eq!(props.selectable_indices(), vec![0, 2]);
    }

    #[test]
    fn no_current_kind_means_all_selectable() {
        let items = vec![mk("terminal", Some('T')), mk("markdown", Some('M'))];
        let props = props_from_items(&items, None, None);
        assert_eq!(props.selectable_indices(), vec![0, 1]);
    }

    #[test]
    fn empty_props_default() {
        let props = ConvertProps::default();
        assert!(props.items.is_empty());
        assert!(props.selectable_indices().is_empty());
        assert_eq!(props.selected_index, None);
    }

    #[test]
    fn preserves_selected_index() {
        let items = vec![mk("terminal", Some('T')), mk("markdown", Some('M'))];
        let props = props_from_items(&items, Some("terminal"), Some(1));
        assert_eq!(props.selected_index, Some(1));
    }
}
