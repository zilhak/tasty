//! 프리셋 목록·미리보기·편집 화면.
//! 구조 편집은 자동 저장하며 surface 설정은 확인할 때 저장한다. 설정 화면이 열린 동안은
//! 목록·범위 탭과 구조 단축키를 막아 편집 대상을 바꾸지 않는다.
//! 캐시 생성 후 저장소 레이아웃이 바뀌면 덮어쓰지 않고 저장소 값을 다시 읽어 알린다.
//! 보기 모드는 다음 프레임에 갱신하고 편집 모드는 저장 직전에 충돌을 확인한다.

#[cfg(test)]
mod cache_slot_tests;
mod demo_cache;
pub mod demo_layout;
mod layout_base;
#[cfg(test)]
mod persist_tests;
pub mod surface_settings;
mod toolbar;
#[cfg(test)]
mod view_refresh_tests;

use tasty_presets::{PresetKind, PresetPaneNode, PresetResult, PresetStore, PresetSurfaceLayout};
use tasty_settings::KeybindingSettings;
use tasty_type_appearance::theme::Theme;
use tasty_type_geometry::length::LogicalPx;
use tasty_ui_widgets::tokens::STRUCT_GAP_1;
use tasty_ui_widgets::{ControlSize, IconButton, IconButtonVariant};

use toolbar::{apply_toolbar_actions, draw_toolbar_editing, draw_toolbar_view};

use crate::adapters::ui::icons;
use crate::adapters::ui::input::shortcuts::any_binding_pressed_egui;
use crate::adapters::ui::{ToastKind, ToastManager, ToastScope};
use crate::i18n::{t, t_fmt};

use demo_cache::{
    DemoCache, drew_editing_last, load_demo, preset_key, refresh_view_cache, reload_after_conflict,
    set_drew_editing_last, store_demo,
};
use demo_layout::{DemoLayout, KindCatalog, ShortcutAction, ShowOutcome};
use layout_base::LayoutBase;
use surface_settings::{CfgOutcome, SurfaceCfg, breadcrumb, draw_surface_settings};

/// 설정 단축키에서 surface→tab→pane 순으로 첫 일치 동작을 고른다.
/// double-tap은 parse_binding에서 지원하지 않으므로 편집기에서도 처리하지 않는다.
fn match_preset_shortcut(
    kb: &KeybindingSettings,
    input: &egui::InputState,
) -> Option<ShortcutAction> {
    let pressed = |b: &[String]| any_binding_pressed_egui(b, input);
    if pressed(&kb.split_surface_vertical) {
        Some(ShortcutAction::SplitSurfaceVertical)
    } else if pressed(&kb.split_surface_horizontal) {
        Some(ShortcutAction::SplitSurfaceHorizontal)
    } else if pressed(&kb.close_surface) {
        Some(ShortcutAction::CloseSurface)
    } else if pressed(&kb.new_tab) {
        Some(ShortcutAction::NewTab)
    } else if pressed(&kb.close_active) {
        Some(ShortcutAction::CloseActive)
    } else if pressed(&kb.split_pane_vertical) {
        Some(ShortcutAction::SplitPaneVertical)
    } else if pressed(&kb.split_pane_horizontal) {
        Some(ShortcutAction::SplitPaneHorizontal)
    } else if pressed(&kb.close_pane) {
        Some(ShortcutAction::ClosePane)
    } else {
        None
    }
}

// 디자인 고정 px (Theme 에 대응 토큰 없는 preset-window 셸 전용 치수 — specimen 전사).
/// 좌측 리스트 폭.
const LIST_WIDTH: LogicalPx = LogicalPx(196.0);
/// 우측 detail 툴바 높이. `size-44` 이지만 툴바 높이라는 역할의 토큰이 없다 — 값이 같은
/// `preset_cfg_header_height` 는 설정 화면 헤더의 치수라 읽으면 틀린 결합이 된다.
const TOOLBAR_HEIGHT: LogicalPx = LogicalPx(44.0);
/// 리스트 row 상하 padding.
const ROW_PAD_Y: LogicalPx = LogicalPx(7.0);
/// 리스트 row 좌우 padding (좌측 accent bar 다음 텍스트 들여쓰기).
const ROW_PAD_X: LogicalPx = LogicalPx(9.0);
/// 리스트 내부 좌우 inset (row 가 패널 가장자리에 붙지 않게).
const LIST_INSET: LogicalPx = LogicalPx(6.0);
/// rename 인라인 입력 폭.
pub(super) const RENAME_W: LogicalPx = LogicalPx(150.0);
/// 툴바 separator 높이.
pub(super) const TOOLBAR_SEP_H: LogicalPx = LogicalPx(18.0);

/// rename 인라인 편집 상태 (egui temp memory 에 보관 — 프레임 간 유지).
#[derive(Clone)]
pub(super) struct RenameState {
    kind: PresetKind,
    original: String,
    buffer: String,
    request_focus: bool,
}

/// 편집 모드 툴바의 name/subtitle 인라인 버퍼 (egui temp memory 보관). `key`
/// (`{kind}:{name}`)가 바뀌면 store 값으로 재초기화한다. subtitle 은 Workspace 만
/// 실제 필드를 가지며 Tab/Pane 은 구조 파생이라 편집 불가(버퍼 미사용).
#[derive(Clone, Default)]
pub(super) struct EditMetaState {
    key: String,
    name: String,
    subtitle: String,
}

/// [`persist_layout`] 의 결과.
#[derive(Debug, PartialEq)]
enum Persisted {
    /// 썼다(또는 쓸 것이 없었다). 값은 캐시의 새 기준 판 — 저장 뒤 저장소의 값이다.
    Saved(Option<LayoutBase>),
    /// 캐시가 지어진 뒤 저장소의 레이아웃이 바뀌었다. 아무것도 쓰지 않았다.
    Conflict,
}

/// 캐시를 만들 때의 base와 현재 저장소를 비교한 뒤 저장한다. 다르면 Conflict이며
/// 사라진 프리셋은 되살리지 않는다.
fn persist_layout(
    store: &mut PresetStore,
    kind: PresetKind,
    name: &str,
    layout: &DemoLayout,
    base: &Option<LayoutBase>,
) -> PresetResult<Persisted> {
    let current = LayoutBase::current(store, kind, name);
    if current.is_some() && current != *base {
        return Ok(Persisted::Conflict);
    }
    write_layout(store, kind, name, layout)?;
    Ok(Persisted::Saved(LayoutBase::current(store, kind, name)))
}

/// 메타데이터는 유지하고 레이아웃만 저장한다. 범위가 맞지 않거나 프리셋이 없으면 아무것도 쓰지 않는다.
fn write_layout(
    store: &mut PresetStore,
    kind: PresetKind,
    name: &str,
    layout: &DemoLayout,
) -> PresetResult<()> {
    match kind {
        PresetKind::Workspace => {
            let Some(node) = layout.rebuild_pane_node() else {
                return Ok(());
            };
            let Some(mut p) = store.get_workspace(name).cloned() else {
                return Ok(());
            };
            p.layout = node;
            // intent-exempt: [결과사용] 응답이 필요한 mutate 는 Core method(sync 리턴) — 저장 결과를 호출부가 토스트로 쓴다
            store.save_workspace_overwrite(p)
        }
        PresetKind::Tab => {
            let Some(surf) = layout.rebuild_surface_layout() else {
                return Ok(());
            };
            let Some(mut p) = store.get_tab(name).cloned() else {
                return Ok(());
            };
            p.tab.layout = surf;
            // intent-exempt: [결과사용] 응답이 필요한 mutate 는 Core method(sync 리턴) — 저장 결과를 호출부가 토스트로 쓴다
            store.save_tab_overwrite(p)
        }
        PresetKind::Pane => {
            let Some(pane) = layout.rebuild_single_pane() else {
                return Ok(());
            };
            let Some(mut p) = store.get_pane(name).cloned() else {
                return Ok(());
            };
            p.pane = pane;
            // intent-exempt: [결과사용] 응답이 필요한 mutate 는 Core method(sync 리턴) — 저장 결과를 호출부가 토스트로 쓴다
            store.save_pane_overwrite(p)
        }
    }
}

// 워크스페이스는 저장된 부제가 있으면 사용한다. 나머지는 구조 개수를 단수·복수 문구로 표시한다.

fn count_panes(node: &PresetPaneNode) -> usize {
    match node {
        PresetPaneNode::Leaf { .. } => 1,
        PresetPaneNode::Split { first, second, .. } => count_panes(first) + count_panes(second),
    }
}

fn count_ws_tabs(node: &PresetPaneNode) -> usize {
    match node {
        PresetPaneNode::Leaf { pane } => pane.tabs.len(),
        PresetPaneNode::Split { first, second, .. } => count_ws_tabs(first) + count_ws_tabs(second),
    }
}

fn count_surfaces(layout: &PresetSurfaceLayout) -> usize {
    match layout {
        PresetSurfaceLayout::Leaf { .. } => 1,
        PresetSurfaceLayout::Split { first, second, .. } => {
            count_surfaces(first) + count_surfaces(second)
        }
    }
}

/// 단/복수 i18n 라벨. `n==1` → `one_key`, 그 외 → `many_key`({} 치환).
fn count_label(n: usize, one_key: &str, many_key: &str) -> String {
    if n == 1 {
        t(one_key).to_string()
    } else {
        t_fmt(many_key, &n.to_string())
    }
}

/// 편집 가능한 **실제** subtitle 필드값(Workspace 만 보유). Tab/Pane 은 구조 파생
/// subtitle 이라 편집 불가 → 빈 문자열.
pub(super) fn workspace_subtitle_field(
    store: &PresetStore,
    kind: PresetKind,
    name: &str,
) -> String {
    if kind == PresetKind::Workspace {
        store
            .get_workspace(name)
            .map(|p| p.subtitle.clone())
            .unwrap_or_default()
    } else {
        String::new()
    }
}

fn subtitle(store: &PresetStore, kind: PresetKind, name: &str) -> String {
    match kind {
        PresetKind::Workspace => store
            .get_workspace(name)
            .map(|p| {
                if !p.subtitle.is_empty() {
                    return p.subtitle.clone();
                }
                let panes = count_label(
                    count_panes(&p.layout),
                    "preset.count.pane_one",
                    "preset.count.pane_many",
                );
                let tabs = count_label(
                    count_ws_tabs(&p.layout),
                    "preset.count.tab_one",
                    "preset.count.tab_many",
                );
                format!("{panes} · {tabs}")
            })
            .unwrap_or_default(),
        PresetKind::Tab => store
            .get_tab(name)
            .map(|p| {
                count_label(
                    count_surfaces(&p.tab.layout),
                    "preset.count.surface_one",
                    "preset.count.surface_many",
                )
            })
            .unwrap_or_default(),
        PresetKind::Pane => store
            .get_pane(name)
            .map(|p| {
                count_label(
                    p.pane.tabs.len(),
                    "preset.count.tab_one",
                    "preset.count.tab_many",
                )
            })
            .unwrap_or_default(),
    }
}

// 현재 실행 레이아웃 저장은 컨텍스트 메뉴에서 처리한다. 여기서는 터미널 하나의 최소 프리셋을 만든다.

fn minimal_surface() -> PresetSurfaceLayout {
    use tasty_presets::PresetSurface;
    PresetSurfaceLayout::Leaf {
        surface: PresetSurface {
            id: None,
            kind: "terminal".into(),
            cwd: None,
            startup_command: None,
            params: serde_json::Value::Null,
        },
    }
}

fn minimal_pane() -> tasty_presets::PresetPane {
    use tasty_presets::PresetTab;
    tasty_presets::PresetPane {
        tabs: vec![PresetTab {
            explicit_name: None,
            layout: minimal_surface(),
        }],
        active_tab: 0,
    }
}

/// 최소 preset 을 만들어 저장하고, 부여된 이름을 반환한다. 실패 시 `None`.
fn create_minimal(store: &mut PresetStore, kind: PresetKind) -> Option<String> {
    use tasty_presets::{PanePreset, TabPreset, WorkspacePreset};
    let name = store.unique_name(kind, kind.as_str());
    let result = match kind {
        // intent-exempt: [결과사용] 응답이 필요한 mutate 는 Core method(sync 리턴) — 저장 결과를 호출부가 토스트로 쓴다
        PresetKind::Workspace => store.save_workspace(WorkspacePreset {
            name: name.clone(),
            subtitle: String::new(),
            description: String::new(),
            layout: PresetPaneNode::Leaf {
                pane: minimal_pane(),
            },
        }),
        // intent-exempt: [결과사용] 응답이 필요한 mutate 는 Core method(sync 리턴) — 저장 결과를 호출부가 토스트로 쓴다
        PresetKind::Tab => store.save_tab(TabPreset {
            name: name.clone(),
            tab: tasty_presets::PresetTab {
                explicit_name: None,
                layout: minimal_surface(),
            },
        }),
        // intent-exempt: [결과사용] 응답이 필요한 mutate 는 Core method(sync 리턴) — 저장 결과를 호출부가 토스트로 쓴다
        PresetKind::Pane => store.save_pane(PanePreset {
            name: name.clone(),
            pane: minimal_pane(),
        }),
    };
    match result {
        Ok(()) => Some(name),
        Err(e) => {
            tracing::warn!("create minimal preset failed: {e}");
            None
        }
    }
}

/// 기존 preset 의 복사본을 만들어 저장하고, 새 이름을 반환한다. 실패 시 `None`.
pub(super) fn duplicate_preset(
    store: &mut PresetStore,
    kind: PresetKind,
    name: &str,
) -> Option<String> {
    let new_name = store.unique_name(kind, &format!("{name}-copy"));
    let result = match kind {
        PresetKind::Workspace => match store.get_workspace(name).cloned() {
            Some(mut p) => {
                p.name = new_name.clone();
                // intent-exempt: [결과사용] 응답이 필요한 mutate 는 Core method(sync 리턴) — 저장 결과를 호출부가 토스트로 쓴다
                store.save_workspace(p)
            }
            None => return None,
        },
        PresetKind::Tab => match store.get_tab(name).cloned() {
            Some(mut p) => {
                p.name = new_name.clone();
                // intent-exempt: [결과사용] 응답이 필요한 mutate 는 Core method(sync 리턴) — 저장 결과를 호출부가 토스트로 쓴다
                store.save_tab(p)
            }
            None => return None,
        },
        PresetKind::Pane => match store.get_pane(name).cloned() {
            Some(mut p) => {
                p.name = new_name.clone();
                // intent-exempt: [결과사용] 응답이 필요한 mutate 는 Core method(sync 리턴) — 저장 결과를 호출부가 토스트로 쓴다
                store.save_pane(p)
            }
            None => return None,
        },
    };
    match result {
        Ok(()) => Some(new_name),
        Err(e) => {
            tracing::warn!("duplicate preset failed: {e}");
            None
        }
    }
}

/// 리스트 row 한 줄을 그린다. 선택 시 surface-active 채움 + 2px accent 좌측 bar.
fn draw_list_row(
    ui: &mut egui::Ui,
    theme: &Theme,
    name: &str,
    sub: &str,
    selected: bool,
) -> egui::Response {
    let name_h = theme.font_size_body;
    let sub_h = theme.font_size_caption;
    let row_h = ROW_PAD_Y.scaled(2.0) + name_h + STRUCT_GAP_1 + sub_h;
    let (full, resp) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), row_h.value()),
        egui::Sense::click(),
    );
    let rect = egui::Rect::from_min_max(
        egui::pos2(full.min.x + LIST_INSET.value(), full.min.y),
        egui::pos2(full.max.x - LIST_INSET.value(), full.max.y),
    );
    let radius = theme.corner_radius_sm.value();
    let p = ui.painter_at(full);
    if selected {
        p.rect_filled(rect, radius, theme.surface_active().to_egui());
        let bar = egui::Rect::from_min_size(
            rect.min,
            egui::vec2(theme.tab_indicator_width.value(), rect.height()),
        );
        p.rect_filled(bar, 0.0, theme.accent_primary().to_egui());
    } else if resp.hovered() {
        p.rect_filled(rect, radius, theme.overlay_hover().to_egui_premultiplied());
    }
    let name_color = if selected {
        theme.text_primary().to_egui()
    } else {
        theme.text_secondary().to_egui()
    };
    let text_x = rect.min.x + ROW_PAD_X.value();
    let name_y = rect.min.y + ROW_PAD_Y.value();
    p.text(
        egui::pos2(text_x, name_y),
        egui::Align2::LEFT_TOP,
        name,
        egui::FontId::proportional(name_h.value()),
        name_color,
    );
    p.text(
        egui::pos2(text_x, name_y + (name_h + STRUCT_GAP_1).value()),
        egui::Align2::LEFT_TOP,
        sub,
        egui::FontId::monospace(sub_h.value()),
        theme.text_muted().to_egui(),
    );
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    resp
}

/// 프리셋 미리보기를 캐시해 선택한 탭과 편집 결과를 유지한다. 편집 변경은 자동 저장하고 실패하면 알린다.
#[allow(clippy::too_many_arguments)]
fn draw_preview(
    ui: &mut egui::Ui,
    store: &mut PresetStore,
    theme: &Theme,
    kind: PresetKind,
    name: &str,
    rect: egui::Rect,
    editing: bool,
    selected_node: &mut Option<usize>,
    surface_cfg: &mut Option<SurfaceCfg>,
    toasts: &mut ToastManager,
    catalog: &KindCatalog,
    kb: &KeybindingSettings,
) {
    ui.painter_at(rect)
        .rect_filled(rect, 0.0, theme.bg_app().to_egui());
    let pad = theme.spacing_md.value();
    let canvas = rect.shrink(pad);
    if canvas.width() <= 0.0 || canvas.height() <= 0.0 {
        return;
    }

    let Some(mut cache) = load_demo(ui, store, kind, name, catalog) else {
        return;
    };

    // 보기에서 편집으로 들어가는 첫 프레임은 아직 사용자 변경이 없어 저장소 값을 먼저 읽는다.
    let entering_edit = editing && !drew_editing_last(ui, &cache.key);
    if !editing || entering_edit {
        // 편집 중에는 저장 직전에만 저장소와 비교한다(ADR-0038).
        let refreshed = refresh_view_cache(store, kind, name, catalog, &mut cache);
        if refreshed {
            ui.ctx().request_repaint();
        }
    }
    set_drew_editing_last(ui, &cache.key, editing);

    if editing {
        draw_preview_editing(
            ui,
            store,
            theme,
            kind,
            name,
            canvas,
            &mut cache,
            selected_node,
            surface_cfg,
            toasts,
            catalog,
            kb,
        );
    } else {
        let changed = cache.layout.show(ui, theme, canvas, catalog);
        if changed {
            ui.ctx().request_repaint();
        }
    }
    store_demo(ui, cache);
}

/// 단축키·마우스 편집을 적용하고 변경이 있으면 자동 저장한다.
#[allow(clippy::too_many_arguments)]
fn draw_preview_editing(
    ui: &mut egui::Ui,
    store: &mut PresetStore,
    theme: &Theme,
    kind: PresetKind,
    name: &str,
    canvas: egui::Rect,
    cache: &mut DemoCache,
    selected_node: &mut Option<usize>,
    surface_cfg: &mut Option<SurfaceCfg>,
    toasts: &mut ToastManager,
    catalog: &KindCatalog,
    kb: &KeybindingSettings,
) {
    let layout = &mut cache.layout;
    // 텍스트 입력 중에는 구조 단축키를 막는다. 바인딩 검사는 키를 소비하지 않아 중복 처리될 수 있다.
    let key_outcome = if ui.ctx().wants_keyboard_input() {
        ShowOutcome::None
    } else {
        match ui.input(|i| match_preset_shortcut(kb, i)) {
            Some(action) => layout.apply_shortcut(action, selected_node, catalog),
            None => ShowOutcome::None,
        }
    };
    let draw_outcome = layout.show_edit(ui, theme, canvas, selected_node, catalog);

    if let ShowOutcome::OpenSettings(id) = draw_outcome {
        if let Some(orig) = layout.leaf_draft(id) {
            *surface_cfg = Some(SurfaceCfg::open(preset_key(kind, name), id, orig));
        }
        ui.ctx().request_repaint();
    }

    let mutated =
        matches!(key_outcome, ShowOutcome::Mutated) || matches!(draw_outcome, ShowOutcome::Mutated);
    let repaint = mutated
        || matches!(key_outcome, ShowOutcome::Repaint)
        || matches!(draw_outcome, ShowOutcome::Repaint);
    if repaint {
        ui.ctx().request_repaint();
    }
    if !mutated {
        return;
    }
    match persist_layout(store, kind, name, &cache.layout, &cache.base) {
        Ok(Persisted::Saved(base)) => cache.base = base,
        Ok(Persisted::Conflict) => {
            reload_after_conflict(store, kind, name, catalog, cache, selected_node, toasts);
        }
        Err(e) => {
            tracing::warn!("preset auto-save failed: {e}");
            toasts.push(
                t("preset.toast.save_failed"),
                ToastKind::Error,
                ToastScope::Window,
            );
        }
    }
}

/// surface 설정을 확인하면 레이아웃 사본에 적용하고 저장한다. 성공할 때만 캐시를 바꾸고 닫는다.
/// 저장 실패는 입력을 유지해 알리고, 취소는 사본을 버린다.
/// 저장소 레이아웃이 바뀌었으면 입력을 버리고 저장소를 다시 읽어 알린다.
/// 프리셋·leaf가 사라져도 적용하지 않는다. 충돌이 없으면 선택한 leaf는 유지한다.
#[allow(clippy::too_many_arguments)] // reason: 형제 draw_preview_editing 과 같은 패널 상태 묶음을 그대로 받는다 — 구조체로 묶으면 호출부 한 곳을 위해 빌림 분할만 늘어난다
fn draw_settings_detail(
    ui: &mut egui::Ui,
    store: &mut PresetStore,
    theme: &Theme,
    kind: PresetKind,
    name: &str,
    rect: egui::Rect,
    selected_node: &mut Option<usize>,
    surface_cfg: &mut Option<SurfaceCfg>,
    toasts: &mut ToastManager,
    catalog: &KindCatalog,
) {
    let Some(cfg) = surface_cfg.as_mut() else {
        return;
    };
    let key = preset_key(kind, name);
    let cache = (cfg.preset_key() == key)
        .then(|| load_demo(ui, store, kind, name, catalog))
        .flatten();
    let Some((mut cache, loc)) =
        cache.and_then(|c| c.layout.leaf_location(cfg.leaf_id()).map(|loc| (c, loc)))
    else {
        *surface_cfg = None;
        ui.ctx().request_repaint();
        return;
    };
    let path = breadcrumb(name, &loc);
    let leaf_id = cfg.leaf_id();

    match draw_surface_settings(ui, theme, rect, cfg, catalog, &path) {
        CfgOutcome::None => store_demo(ui, cache),
        CfgOutcome::Cancel => {
            store_demo(ui, cache);
            *selected_node = Some(leaf_id);
            *surface_cfg = None;
            ui.ctx().request_repaint();
        }
        CfgOutcome::Confirm => {
            let mut candidate = cache.layout.clone();
            candidate.apply_leaf_draft(leaf_id, cfg.draft(), catalog);
            match persist_layout(store, kind, name, &candidate, &cache.base) {
                Ok(Persisted::Saved(base)) => {
                    store_demo(
                        ui,
                        DemoCache {
                            key,
                            layout: candidate,
                            base,
                        },
                    );
                    *selected_node = Some(leaf_id);
                    *surface_cfg = None;
                }
                Ok(Persisted::Conflict) => {
                    reload_after_conflict(
                        store,
                        kind,
                        name,
                        catalog,
                        &mut cache,
                        selected_node,
                        toasts,
                    );
                    store_demo(ui, cache);
                    *surface_cfg = None;
                }
                Err(e) => {
                    tracing::warn!("preset surface settings save failed: {e}");
                    toasts.push(
                        t("preset.toast.save_failed"),
                        ToastKind::Error,
                        ToastScope::Window,
                    );
                    store_demo(ui, cache);
                }
            }
            ui.ctx().request_repaint();
        }
    }
}

/// 설정 화면 아래의 목록·탭 입력을 막는다. egui는 나중에 등록한 같은 층의 위젯을 위로 본다.
fn block_input(ui: &mut egui::Ui, rect: egui::Rect, salt: &str) {
    ui.interact(
        rect,
        egui::Id::new(("preset_cfg_lock", salt)),
        egui::Sense::click_and_drag(),
    );
}

/// [`draw_preset_panel`] 본문 2분할([리스트 196px | detail] → 툴바/미리보기)의
/// 사각형들. 좌측 리스트/우측 detail 배경과 구분선도 이 시점에 함께 칠한다.
struct PresetPanelRects {
    list_rect: egui::Rect,
    toolbar_rect: egui::Rect,
    preview_rect: egui::Rect,
}

fn compute_panel_rects(ui: &egui::Ui, theme: &Theme) -> PresetPanelRects {
    let body = ui.available_rect_before_wrap();
    let bw = theme.border_width.value();
    let list_rect =
        egui::Rect::from_min_size(body.min, egui::vec2(LIST_WIDTH.value(), body.height()));
    let detail_rect = egui::Rect::from_min_max(
        egui::pos2(body.min.x + LIST_WIDTH.value(), body.min.y),
        body.max,
    );
    let painter = ui.painter();
    painter.rect_filled(list_rect, 0.0, theme.bg_sidebar().to_egui());
    painter.rect_filled(detail_rect, 0.0, theme.bg_panel().to_egui());
    painter.vline(
        body.min.x + LIST_WIDTH.value(),
        body.y_range(),
        egui::Stroke::new(bw, theme.separator.to_egui()),
    );

    let toolbar_rect = egui::Rect::from_min_size(
        detail_rect.min,
        egui::vec2(detail_rect.width(), TOOLBAR_HEIGHT.value()),
    );
    let preview_rect = egui::Rect::from_min_max(
        egui::pos2(detail_rect.min.x, toolbar_rect.max.y),
        detail_rect.max,
    );
    ui.painter().hline(
        toolbar_rect.x_range(),
        toolbar_rect.max.y,
        egui::Stroke::new(bw, theme.separator.to_egui()),
    );

    PresetPanelRects {
        list_rect,
        toolbar_rect,
        preview_rect,
    }
}

/// 목록 선택과 새 프리셋 버튼을 그려 selected를 갱신한다.
#[allow(clippy::too_many_arguments)]
fn draw_preset_list(
    ui: &mut egui::Ui,
    ctx: &egui::Context,
    theme: &Theme,
    store: &mut PresetStore,
    kind: PresetKind,
    selected: &mut Option<String>,
    resolved: &Option<String>,
    rows: &[(String, String)],
    list_rect: egui::Rect,
    locked: bool,
) {
    let mut new_clicked = false;
    let mut clicked_name: Option<String> = None;

    {
        let mut lui = ui.new_child(egui::UiBuilder::new().max_rect(list_rect));
        lui.set_clip_rect(list_rect);
        if locked {
            lui.set_opacity(theme.preset_cfg_dim_opacity());
        }
        lui.add_space(theme.spacing_sm.value());
        lui.horizontal(|ui| {
            ui.add_space(LIST_INSET.value());
            let count = t_fmt("preset.header.count", &rows.len().to_string());
            ui.label(
                egui::RichText::new(count.to_uppercase())
                    .monospace()
                    .size(theme.font_size_caption.value())
                    .color(theme.text_muted().to_egui()),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.add_space(LIST_INSET.value());
                if IconButton::new()
                    .variant(IconButtonVariant::Ghost)
                    .size(ControlSize::Sm)
                    .show(ui, theme, &|ui, rect, c| {
                        icons::PLUS.image(rect.width(), c).paint_at(ui, rect)
                    })
                    .on_hover_text(t("preset.header.new"))
                    .clicked()
                {
                    new_clicked = true;
                }
            });
        });
        lui.add_space(theme.spacing_xs.value());

        egui::ScrollArea::vertical()
            .auto_shrink([false; 2])
            .drag_to_scroll(false)
            .enable_scrolling(!locked)
            .show(&mut lui, |ui| {
                if rows.is_empty() {
                    ui.add_space(theme.spacing_sm.value());
                    ui.vertical_centered(|ui| {
                        ui.label(
                            egui::RichText::new(t("preset.popup.empty"))
                                .size(theme.font_size_caption.value())
                                .color(theme.text_muted().to_egui()),
                        );
                    });
                    return;
                }
                for (name, sub) in rows {
                    let is_sel = resolved.as_deref() == Some(name.as_str());
                    if draw_list_row(ui, theme, name, sub, is_sel).clicked() {
                        clicked_name = Some(name.clone());
                    }
                }
            });
    }

    if locked {
        // 설정 초안이 사라지지 않도록 이번 프레임의 선택도 무시하고 이후 입력을 막는다.
        block_input(ui, list_rect, "list");
        return;
    }
    if let Some(n) = clicked_name {
        *selected = Some(n);
        ctx.request_repaint();
    }
    if new_clicked && let Some(n) = create_minimal(store, kind) {
        *selected = Some(n);
        ctx.request_repaint();
    }
}

/// [`apply_toolbar_actions`] 에 전달할 이번 프레임 툴바 클릭 결과 묶음.
pub(super) struct PresetToolbarClicks {
    edit_clicked: bool,
    done_clicked: bool,
    rename_clicked: bool,
    duplicate_clicked: bool,
    delete_clicked: bool,
}

/// PresetView 의 본문을 그린다.
#[allow(clippy::too_many_arguments)]
pub fn draw_preset_panel(
    ctx: &egui::Context,
    store: &mut PresetStore,
    active_kind: &mut PresetKind,
    selected_workspace: &mut Option<String>,
    selected_tab: &mut Option<String>,
    selected_pane: &mut Option<String>,
    editing: &mut bool,
    selected_node: &mut Option<usize>,
    surface_cfg: &mut Option<SurfaceCfg>,
    toasts: &mut ToastManager,
    catalog: &KindCatalog,
    kb: &KeybindingSettings,
) {
    let theme = crate::theme::theme();
    if !*editing {
        *surface_cfg = None;
    }
    let locked = surface_cfg.is_some();
    let rename_id = egui::Id::new("preset_rename_state");
    let mut rename: Option<RenameState> = ctx
        .data_mut(|d| d.get_temp::<Option<RenameState>>(rename_id))
        .flatten();
    let edit_meta_id = egui::Id::new("preset_edit_meta");

    egui::CentralPanel::default().show(ctx, |ui| {
        let mut scope = *active_kind;
        let l1 = ui
            .scope(|ui| {
                if locked {
                    ui.set_opacity(theme.preset_cfg_dim_opacity());
                }
                ui.horizontal(|ui| {
                    ui.selectable_value(
                        &mut scope,
                        PresetKind::Workspace,
                        t("preset.tab.workspace"),
                    );
                    ui.selectable_value(&mut scope, PresetKind::Tab, t("preset.tab.tab"));
                    ui.selectable_value(&mut scope, PresetKind::Pane, t("preset.tab.pane"));
                });
            })
            .response
            .rect;
        if locked {
            block_input(ui, l1, "scope");
        } else {
            *active_kind = scope;
        }
        ui.separator();

        let kind = *active_kind;
        let names = store.list(kind);
        let selected: &mut Option<String> = match kind {
            PresetKind::Workspace => selected_workspace,
            PresetKind::Tab => selected_tab,
            PresetKind::Pane => selected_pane,
        };
        let resolved = selected
            .clone()
            .filter(|n| names.contains(n))
            .or_else(|| names.first().cloned());

        let rows: Vec<(String, String)> = names
            .iter()
            .map(|n| (n.clone(), subtitle(store, kind, n)))
            .collect();

        let rects = compute_panel_rects(ui, &theme);

        draw_preset_list(
            ui,
            ctx,
            &theme,
            store,
            kind,
            selected,
            &resolved,
            &rows,
            rects.list_rect,
            locked,
        );

        let current = selected
            .clone()
            .or_else(|| store.list(kind).first().cloned());
        let detail_sub = current
            .as_deref()
            .map(|n| subtitle(store, kind, n))
            .unwrap_or_default();

        let mut clicks = PresetToolbarClicks {
            edit_clicked: false,
            done_clicked: false,
            rename_clicked: false,
            duplicate_clicked: false,
            delete_clicked: false,
        };
        let mut edit_meta: Option<EditMetaState> = None;

        if locked {
            if let Some(n) = current.as_deref() {
                let detail = rects.toolbar_rect.union(rects.preview_rect);
                draw_settings_detail(
                    ui,
                    store,
                    &theme,
                    kind,
                    n,
                    detail,
                    selected_node,
                    surface_cfg,
                    toasts,
                    catalog,
                );
            } else {
                *surface_cfg = None;
            }
            // 설정 화면 뒤의 미리보기와 구조 단축키를 실행하지 않는다.
            return;
        }

        if let Some(name) = current.clone() {
            let toolbar_inner = rects
                .toolbar_rect
                .shrink2(egui::vec2(theme.spacing_md.value(), 0.0));
            let mut tui = ui.new_child(egui::UiBuilder::new().max_rect(toolbar_inner));
            tui.set_clip_rect(rects.toolbar_rect);
            tui.horizontal_centered(|ui| {
                if *editing {
                    let outcome = draw_toolbar_editing(
                        ui,
                        ctx,
                        store,
                        &theme,
                        kind,
                        &name,
                        selected,
                        toasts,
                        edit_meta_id,
                    );
                    edit_meta = outcome.edit_meta;
                    clicks.done_clicked = outcome.done_clicked;
                    return;
                }

                let view_clicks = draw_toolbar_view(
                    ui,
                    &theme,
                    store,
                    kind,
                    &name,
                    &detail_sub,
                    &mut rename,
                    selected,
                );
                clicks.rename_clicked = view_clicks.rename_clicked;
                clicks.duplicate_clicked = view_clicks.duplicate_clicked;
                clicks.delete_clicked = view_clicks.delete_clicked;
                clicks.edit_clicked = view_clicks.edit_clicked;
            });
        }

        if let Some(meta) = edit_meta {
            ctx.data_mut(|d| d.insert_temp(edit_meta_id, meta));
        }

        apply_toolbar_actions(
            ctx,
            store,
            kind,
            &current,
            editing,
            selected_node,
            selected,
            &mut rename,
            clicks,
        );

        let preview_name = selected
            .clone()
            .or_else(|| store.list(kind).first().cloned());
        match preview_name {
            Some(n) => draw_preview(
                ui,
                store,
                &theme,
                kind,
                &n,
                rects.preview_rect,
                *editing,
                selected_node,
                surface_cfg,
                toasts,
                catalog,
                kb,
            ),
            None => {
                ui.painter_at(rects.preview_rect).rect_filled(
                    rects.preview_rect,
                    0.0,
                    theme.bg_app().to_egui(),
                );
                ui.painter_at(rects.preview_rect).text(
                    rects.preview_rect.center(),
                    egui::Align2::CENTER_CENTER,
                    t("preset.popup.empty"),
                    egui::FontId::proportional(theme.font_size_body.value()),
                    theme.text_muted().to_egui(),
                );
            }
        }
    });

    ctx.data_mut(|d| d.insert_temp(rename_id, rename));
}
