//! `PresetView` 의 egui UI 그리기 함수 (디자인 2026-06-25 `PresetWindow` 전사).
//!
//! L1 scope 탭(Workspace/Tab/Pane) 아래 **2-depth list→detail** 본문:
//!  - 좌측 리스트(196px, bg-sidebar): 현재 scope 의 저장된 preset. row = name + mono
//!    subtitle. 선택 row = surface-active 채움 + 2px accent 좌측 bar. 헤더 = `N presets`
//!    + New preset(`+`). 빈 scope → `preset.popup.empty`.
//!  - 우측 detail(bg-panel): 44px 툴바(name/subtitle · rename·duplicate·delete · Edit)
//!    위에 선택 preset 의 **데모 레이아웃 미리보기**.
//!
//! Edit 버튼으로 read-only 미리보기(`DemoLayout::show`)와 편집(WYSIWYG) 모드
//! (`DemoLayout::show_edit`)를 토글한다(Edit↔Done). rename·duplicate·delete 는
//! 기존 store API 에 직결돼 동작한다.

use tasty_presets::{PresetKind, PresetPaneNode, PresetResult, PresetStore, PresetSurfaceLayout};
use tasty_settings::KeybindingSettings;
use tasty_type_appearance::theme::Theme;
use tasty_ui_widgets::{Button, ButtonVariant, ControlSize, IconButton, IconButtonVariant};

use crate::adapters::ui::icons;
use crate::adapters::ui::input::shortcuts::any_binding_pressed_egui;
use crate::adapters::ui::{ToastKind, ToastManager, ToastScope};
use crate::i18n::{t, t_fmt};

pub mod demo_layout;

use demo_layout::{DemoLayout, KindCatalog, ShortcutAction, ShowOutcome};

/// 편집 모드 프레임에서 `KeybindingSettings` 바인딩과 이번 프레임 입력을 매칭해
/// 대응하는 [`ShortcutAction`] 을 하나 고른다. 하드코딩 키 문자열 없이 전부
/// 설정 필드로 판정한다(§단축키). 여러 필드가 같은 키를 공유해도 이 순서로 첫
/// 매칭이 이긴다 — surface → tab → pane 순.
///
/// double-tap 바인딩(`shift+shift` 등)은 `parse_binding` 이 거부하므로 여기서도
/// 매칭되지 않는다(편집기 미지원 — docs 명시).
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
const LIST_WIDTH: f32 = 196.0;
/// 우측 detail 툴바 높이.
const TOOLBAR_HEIGHT: f32 = 44.0;
/// 리스트 row 상하 padding.
const ROW_PAD_Y: f32 = 7.0;
/// 리스트 row 좌우 padding (좌측 accent bar 다음 텍스트 들여쓰기).
const ROW_PAD_X: f32 = 9.0;
/// row 안 name↔subtitle 세로 gap.
const ROW_GAP: f32 = 1.0;
/// 리스트 내부 좌우 inset (row 가 패널 가장자리에 붙지 않게).
const LIST_INSET: f32 = 6.0;
/// rename 인라인 입력 폭.
const RENAME_W: f32 = 150.0;
/// 툴바 separator 높이.
const TOOLBAR_SEP_H: f32 = 18.0;

/// rename 인라인 편집 상태 (egui temp memory 에 보관 — 프레임 간 유지).
#[derive(Clone)]
struct RenameState {
    kind: PresetKind,
    original: String,
    buffer: String,
    request_focus: bool,
}

/// 편집 모드 툴바의 name/subtitle 인라인 버퍼 (egui temp memory 보관). `key`
/// (`{kind}:{name}`)가 바뀌면 store 값으로 재초기화한다. subtitle 은 Workspace 만
/// 실제 필드를 가지며 Tab/Pane 은 구조 파생이라 편집 불가(버퍼 미사용).
#[derive(Clone, Default)]
struct EditMetaState {
    key: String,
    name: String,
    subtitle: String,
}

/// 선택된 preset 으로부터 미리보기 위젯을 만든다. `catalog` 는 registry 파생 kind
/// 스냅샷(미주입이면 빈 catalog → 정적 fallback).
fn build_demo(
    store: &PresetStore,
    kind: PresetKind,
    name: &str,
    catalog: &KindCatalog,
) -> Option<DemoLayout> {
    match kind {
        PresetKind::Workspace => store
            .get_workspace(name)
            .map(|p| DemoLayout::from_workspace(p, catalog)),
        PresetKind::Tab => store
            .get_tab(name)
            .map(|p| DemoLayout::from_tab(p, catalog)),
        PresetKind::Pane => store
            .get_pane(name)
            .map(|p| DemoLayout::from_pane(p, catalog)),
    }
}

/// 편집된 `layout` 을 store/disk 에 write-through(auto-save). 메타데이터
/// (name/subtitle/description/explicit_name)는 기존 preset 에서 보존하고 **레이아웃
/// 트리만** 교체한다 — 편집 모드는 구조/leaf 파라미터만 건드리므로. scope 가
/// layout 종류와 안 맞거나 preset 이 사라졌으면 no-op(Ok).
fn persist_layout(
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
            store.save_pane_overwrite(p)
        }
    }
}

// ── subtitle (구조 요약) ─────────────────────────────────────────────────
//
// Workspace 는 저장된 `subtitle` 필드가 있으면 그것을, 없으면 pane/tab 개수를. Tab/Pane
// 은 필드가 없으므로 구조(surface/tab 개수)로 요약. 단/복수는 i18n 키로 분리해 EN 복수
// 문법까지 맞춘다(KO/JA 는 동일 형태).

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
fn workspace_subtitle_field(store: &PresetStore, kind: PresetKind, name: &str) -> String {
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

// ── New preset (최소 preset 생성) ────────────────────────────────────────
//
// PresetView 윈도우는 live layout(CoreState) 에 접근하지 않으므로 "현재 레이아웃
// capture" 는 불가능(그건 컨텍스트 메뉴 "...프리셋으로 저장" 경로가 담당). 여기 `+`
// 는 **terminal surface 1개짜리 최소 preset** 을 만들어 곧장 선택한다. 실제 내용 편집은
// Edit 모드(`DemoLayout::show_edit`)에서 한다.

fn minimal_surface() -> PresetSurfaceLayout {
    use tasty_presets::PresetSurface;
    PresetSurfaceLayout::Leaf {
        surface: PresetSurface {
            // id 는 저장 시 PresetStore 가 정규화로 부여한다(여기선 None).
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
        PresetKind::Workspace => store.save_workspace(WorkspacePreset {
            name: name.clone(),
            subtitle: String::new(),
            description: String::new(),
            layout: PresetPaneNode::Leaf {
                pane: minimal_pane(),
            },
        }),
        PresetKind::Tab => store.save_tab(TabPreset {
            name: name.clone(),
            tab: tasty_presets::PresetTab {
                explicit_name: None,
                layout: minimal_surface(),
            },
        }),
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
fn duplicate_preset(store: &mut PresetStore, kind: PresetKind, name: &str) -> Option<String> {
    let new_name = store.unique_name(kind, &format!("{name}-copy"));
    let result = match kind {
        PresetKind::Workspace => match store.get_workspace(name).cloned() {
            Some(mut p) => {
                p.name = new_name.clone();
                store.save_workspace(p)
            }
            None => return None,
        },
        PresetKind::Tab => match store.get_tab(name).cloned() {
            Some(mut p) => {
                p.name = new_name.clone();
                store.save_tab(p)
            }
            None => return None,
        },
        PresetKind::Pane => match store.get_pane(name).cloned() {
            Some(mut p) => {
                p.name = new_name.clone();
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

// ── 리스트 row ───────────────────────────────────────────────────────────

/// 리스트 row 한 줄을 그린다. 선택 시 surface-active 채움 + 2px accent 좌측 bar.
fn draw_list_row(
    ui: &mut egui::Ui,
    theme: &Theme,
    name: &str,
    sub: &str,
    selected: bool,
) -> egui::Response {
    let name_h = theme.font_size_body.value();
    let sub_h = theme.font_size_caption.value();
    let row_h = ROW_PAD_Y * 2.0 + name_h + ROW_GAP + sub_h;
    let (full, resp) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), row_h),
        egui::Sense::click(),
    );
    let rect = egui::Rect::from_min_max(
        egui::pos2(full.min.x + LIST_INSET, full.min.y),
        egui::pos2(full.max.x - LIST_INSET, full.max.y),
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
    p.text(
        egui::pos2(rect.min.x + ROW_PAD_X, rect.min.y + ROW_PAD_Y),
        egui::Align2::LEFT_TOP,
        name,
        egui::FontId::proportional(name_h),
        name_color,
    );
    // painter_at 가 full 로 clip → 긴 subtitle 도 row 밖으로 넘치지 않는다.
    p.text(
        egui::pos2(
            rect.min.x + ROW_PAD_X,
            rect.min.y + ROW_PAD_Y + name_h + ROW_GAP,
        ),
        egui::Align2::LEFT_TOP,
        sub,
        egui::FontId::monospace(sub_h),
        theme.text_muted().to_egui(),
    );
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    resp
}

// ── 미리보기 ─────────────────────────────────────────────────────────────

/// `rect`(bg-app) 안에 선택 preset 의 데모 레이아웃을 그린다. demo 인스턴스는 egui
/// temp memory 에 (key, layout) 으로 캐시해 탭 클릭 전환·편집 결과가 프레임 간
/// 지속되게 한다. `editing` 이면 WYSIWYG 편집 모드로 그리고, 변경 발생 시 즉시
/// store/disk 에 write-through(auto-save) + 실패 시 toast.
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

    let key = format!("{}:{}", kind.as_str(), name);
    let cache_id = egui::Id::new("preset_demo_layout_cache");
    let cached: Option<(String, DemoLayout)> = ui.data(|d| d.get_temp(cache_id));
    let mut layout = match cached {
        Some((k, dl)) if k == key => dl,
        _ => match build_demo(store, kind, name, catalog) {
            Some(dl) => dl,
            None => return,
        },
    };

    if editing {
        // 표준 단축키 → focus(선택 leaf) 기준 mutation. TextEdit(이름/subtitle/cwd/
        // startup) 포커스 중에는 문자 키가 입력으로 가야 하므로 매칭을 차단한다
        // (any_binding_pressed_egui 는 키를 소비하지 않아 가드가 없으면 이중 처리됨).
        let key_outcome = if ui.ctx().wants_keyboard_input() {
            ShowOutcome::None
        } else {
            match ui.input(|i| match_preset_shortcut(kb, i)) {
                Some(action) => layout.apply_shortcut(action, selected_node, catalog),
                None => ShowOutcome::None,
            }
        };
        let draw_outcome = layout.show_edit(ui, theme, canvas, selected_node, catalog);

        // 단축키·마우스 어느 쪽이든 변형이면 한 번만 write-through(auto-save).
        let mutated = matches!(key_outcome, ShowOutcome::Mutated)
            || matches!(draw_outcome, ShowOutcome::Mutated);
        let repaint = mutated
            || matches!(key_outcome, ShowOutcome::Repaint)
            || matches!(draw_outcome, ShowOutcome::Repaint);
        if repaint {
            ui.ctx().request_repaint();
        }
        if mutated && let Err(e) = persist_layout(store, kind, name, &layout) {
            tracing::warn!("preset auto-save failed: {e}");
            toasts.push(
                t("preset.toast.save_failed"),
                ToastKind::Error,
                ToastScope::Window,
            );
        }
    } else {
        let changed = layout.show(ui, theme, canvas, catalog);
        if changed {
            ui.ctx().request_repaint();
        }
    }
    ui.data_mut(|d| d.insert_temp(cache_id, (key, layout)));
}

// ── 본문 ─────────────────────────────────────────────────────────────────

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
    toasts: &mut ToastManager,
    catalog: &KindCatalog,
    kb: &KeybindingSettings,
) {
    let theme = crate::theme::theme();
    let rename_id = egui::Id::new("preset_rename_state");
    let mut rename: Option<RenameState> = ctx
        .data_mut(|d| d.get_temp::<Option<RenameState>>(rename_id))
        .flatten();
    let edit_meta_id = egui::Id::new("preset_edit_meta");

    egui::CentralPanel::default().show(ctx, |ui| {
        // ── L1 scope 탭 (유지) ──────────────────────────────────────────
        ui.horizontal(|ui| {
            ui.selectable_value(
                active_kind,
                PresetKind::Workspace,
                t("preset.tab.workspace"),
            );
            ui.selectable_value(active_kind, PresetKind::Tab, t("preset.tab.tab"));
            ui.selectable_value(active_kind, PresetKind::Pane, t("preset.tab.pane"));
        });
        ui.separator();

        let kind = *active_kind;
        let names = store.list(kind);
        let selected: &mut Option<String> = match kind {
            PresetKind::Workspace => selected_workspace,
            PresetKind::Tab => selected_tab,
            PresetKind::Pane => selected_pane,
        };
        // 선택 항목이 유효하면 그것을, 아니면 목록 첫 항목을 본다.
        let resolved = selected
            .clone()
            .filter(|n| names.contains(n))
            .or_else(|| names.first().cloned());

        // row (name, subtitle) 를 미리 해석 — store 의 immutable borrow 를 여기서 끝낸다.
        let rows: Vec<(String, String)> = names
            .iter()
            .map(|n| (n.clone(), subtitle(store, kind, n)))
            .collect();

        // ── 본문 2분할: [리스트 196px | detail] ──────────────────────────
        let body = ui.available_rect_before_wrap();
        let bw = theme.border_width.value();
        let list_rect = egui::Rect::from_min_size(body.min, egui::vec2(LIST_WIDTH, body.height()));
        let detail_rect =
            egui::Rect::from_min_max(egui::pos2(body.min.x + LIST_WIDTH, body.min.y), body.max);
        let painter = ui.painter();
        painter.rect_filled(list_rect, 0.0, theme.bg_sidebar().to_egui());
        painter.rect_filled(detail_rect, 0.0, theme.bg_panel().to_egui());
        painter.vline(
            body.min.x + LIST_WIDTH,
            body.y_range(),
            egui::Stroke::new(bw, theme.separator.to_egui()),
        );

        let mut new_clicked = false;
        let mut clicked_name: Option<String> = None;

        // ── 좌측 리스트 ──────────────────────────────────────────────────
        {
            let mut lui = ui.new_child(egui::UiBuilder::new().max_rect(list_rect));
            lui.set_clip_rect(list_rect);
            lui.add_space(theme.spacing_sm.value());
            lui.horizontal(|ui| {
                ui.add_space(LIST_INSET);
                let count = t_fmt("preset.header.count", &rows.len().to_string());
                ui.label(
                    egui::RichText::new(count.to_uppercase())
                        .monospace()
                        .size(theme.font_size_caption.value())
                        .color(theme.text_muted().to_egui()),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.add_space(LIST_INSET);
                    if IconButton::new()
                        .variant(IconButtonVariant::Ghost)
                        .size(ControlSize::Sm)
                        .show(ui, &theme, &|ui, rect, c| {
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
                    for (name, sub) in &rows {
                        let is_sel = resolved.as_deref() == Some(name.as_str());
                        if draw_list_row(ui, &theme, name, sub, is_sel).clicked() {
                            clicked_name = Some(name.clone());
                        }
                    }
                });
        }

        // ── 리스트 인터랙션 적용 ─────────────────────────────────────────
        if let Some(n) = clicked_name {
            *selected = Some(n);
            ctx.request_repaint();
        }
        if new_clicked && let Some(n) = create_minimal(store, kind) {
            *selected = Some(n);
            ctx.request_repaint();
        }

        // detail 에 그릴 현재 preset.
        let current = selected
            .clone()
            .or_else(|| store.list(kind).first().cloned());
        let detail_sub = current
            .as_deref()
            .map(|n| subtitle(store, kind, n))
            .unwrap_or_default();

        // ── 우측 detail: 툴바 + 미리보기 ─────────────────────────────────
        let toolbar_rect = egui::Rect::from_min_size(
            detail_rect.min,
            egui::vec2(detail_rect.width(), TOOLBAR_HEIGHT),
        );
        let preview_rect = egui::Rect::from_min_max(
            egui::pos2(detail_rect.min.x, toolbar_rect.max.y),
            detail_rect.max,
        );
        // 툴바 하단 border.
        ui.painter().hline(
            toolbar_rect.x_range(),
            toolbar_rect.max.y,
            egui::Stroke::new(bw, theme.separator.to_egui()),
        );

        let mut rename_clicked = false;
        let mut duplicate_clicked = false;
        let mut delete_clicked = false;
        let mut edit_clicked = false;
        let mut done_clicked = false;
        // 편집 모드 name/subtitle 인라인 버퍼 — 편집 시에만 로드/저장.
        let mut edit_meta: Option<EditMetaState> = None;

        if let Some(name) = current.clone() {
            let toolbar_inner = toolbar_rect.shrink2(egui::vec2(theme.spacing_md.value(), 0.0));
            let mut tui = ui.new_child(egui::UiBuilder::new().max_rect(toolbar_inner));
            tui.set_clip_rect(toolbar_rect);
            tui.horizontal_centered(|ui| {
                if *editing {
                    // ── 편집 상태 툴바: name/subtitle 인라인 입력 ──────────────
                    let key = format!("{}:{}", kind.as_str(), name);
                    let mut meta = ctx
                        .data_mut(|d| d.get_temp::<EditMetaState>(edit_meta_id))
                        .filter(|m| m.key == key)
                        .unwrap_or_else(|| EditMetaState {
                            key: key.clone(),
                            name: name.clone(),
                            subtitle: workspace_subtitle_field(store, kind, &name),
                        });

                    // name input — lost_focus 시 rename 커밋.
                    let name_resp = ui.add(
                        egui::TextEdit::singleline(&mut meta.name)
                            .desired_width(RENAME_W)
                            .id(egui::Id::new(("preset_edit_name", kind.as_str()))),
                    );
                    if name_resp.lost_focus() {
                        let buf = meta.name.trim().to_string();
                        if buf.is_empty() {
                            meta.name = name.clone(); // 빈 이름 거부 — 되돌림.
                        } else if buf != name {
                            match store.rename(kind, &name, &buf) {
                                Ok(()) => {
                                    *selected = Some(buf.clone());
                                    meta.key = format!("{}:{}", kind.as_str(), buf);
                                    meta.name = buf;
                                }
                                Err(e) => {
                                    tracing::warn!("preset rename failed: {e}");
                                    toasts.push(
                                        t("preset.toast.rename_failed"),
                                        ToastKind::Error,
                                        ToastScope::Window,
                                    );
                                    meta.name = name.clone();
                                }
                            }
                        }
                    }

                    // subtitle input — Workspace 만(실제 필드). changed 시 즉시 저장.
                    if kind == PresetKind::Workspace {
                        let sub_resp = ui.add(
                            egui::TextEdit::singleline(&mut meta.subtitle)
                                .desired_width(RENAME_W)
                                .hint_text(t("preset.edit.subtitle_hint"))
                                .id(egui::Id::new("preset_edit_subtitle")),
                        );
                        if sub_resp.changed()
                            && let Some(mut p) = store.get_workspace(&name).cloned()
                        {
                            p.subtitle = meta.subtitle.clone();
                            if let Err(e) = store.save_workspace_overwrite(p) {
                                tracing::warn!("preset subtitle save failed: {e}");
                                toasts.push(
                                    t("preset.toast.save_failed"),
                                    ToastKind::Error,
                                    ToastScope::Window,
                                );
                            }
                        }
                    }

                    edit_meta = Some(meta);

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        // Done (primary) — 우측 끝.
                        if Button::new(t("preset.toolbar.done"))
                            .variant(ButtonVariant::Primary)
                            .size(ControlSize::Sm)
                            .show(ui, &theme)
                            .clicked()
                        {
                            done_clicked = true;
                        }
                        // "saved automatically" affordance — Save 버튼 없음을 명시.
                        ui.label(
                            egui::RichText::new(t("preset.toolbar.saved"))
                                .size(theme.font_size_caption.value())
                                .color(theme.text_muted().to_egui()),
                        );
                    });
                    return;
                }

                let renaming = rename
                    .as_ref()
                    .is_some_and(|r| r.kind == kind && r.original == name);
                if renaming {
                    let r = rename.as_mut().unwrap();
                    let resp = ui.add(
                        egui::TextEdit::singleline(&mut r.buffer)
                            .desired_width(RENAME_W)
                            .id(egui::Id::new(("preset_rename_input", kind.as_str()))),
                    );
                    if r.request_focus {
                        resp.request_focus();
                        r.request_focus = false;
                    }
                    let esc = ui.input(|i| i.key_pressed(egui::Key::Escape));
                    if esc {
                        rename = None; // 취소
                    } else if resp.lost_focus() {
                        // 커밋: 이름이 바뀌었으면 rename, 아니면 그냥 닫기.
                        let buf = r.buffer.trim().to_string();
                        if !buf.is_empty() && buf != r.original {
                            match store.rename(kind, &r.original, &buf) {
                                Ok(()) => *selected = Some(buf),
                                Err(e) => tracing::warn!("preset rename failed: {e}"),
                            }
                        }
                        rename = None;
                    }
                } else {
                    ui.label(egui::RichText::new(&name).strong());
                    ui.label(
                        egui::RichText::new(&detail_sub)
                            .monospace()
                            .size(theme.font_size_caption.value())
                            .color(theme.text_muted().to_egui()),
                    );
                }

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    // 우측 끝부터: Edit · | · delete · duplicate · rename.
                    if Button::new(t("preset.toolbar.edit"))
                        .variant(ButtonVariant::Secondary)
                        .size(ControlSize::Sm)
                        .leading_icon(&|ui, rect, c| {
                            icons::EDIT.image(rect.width(), c).paint_at(ui, rect)
                        })
                        .show(ui, &theme)
                        .clicked()
                    {
                        edit_clicked = true;
                    }
                    // separator.
                    ui.add_space(theme.spacing_xs.value());
                    let (sep_rect, _) = ui.allocate_exact_size(
                        egui::vec2(bw.max(1.0), TOOLBAR_SEP_H),
                        egui::Sense::hover(),
                    );
                    ui.painter()
                        .rect_filled(sep_rect, 0.0, theme.separator.to_egui());
                    ui.add_space(theme.spacing_xs.value());

                    if IconButton::new()
                        .variant(IconButtonVariant::Ghost)
                        .size(ControlSize::Sm)
                        .show(ui, &theme, &|ui, rect, c| {
                            icons::TRASH.image(rect.width(), c).paint_at(ui, rect)
                        })
                        .on_hover_text(t("preset.toolbar.delete"))
                        .clicked()
                    {
                        delete_clicked = true;
                    }
                    if IconButton::new()
                        .variant(IconButtonVariant::Ghost)
                        .size(ControlSize::Sm)
                        .show(ui, &theme, &|ui, rect, c| {
                            icons::CLIPBOARD.image(rect.width(), c).paint_at(ui, rect)
                        })
                        .on_hover_text(t("preset.toolbar.duplicate"))
                        .clicked()
                    {
                        duplicate_clicked = true;
                    }
                    if IconButton::new()
                        .variant(IconButtonVariant::Ghost)
                        .size(ControlSize::Sm)
                        .show(ui, &theme, &|ui, rect, c| {
                            icons::EDIT.image(rect.width(), c).paint_at(ui, rect)
                        })
                        .on_hover_text(t("preset.toolbar.rename"))
                        .clicked()
                    {
                        rename_clicked = true;
                    }
                });
            });
        }

        // 편집 메타 버퍼를 메모리에 반영.
        if let Some(meta) = edit_meta {
            ctx.data_mut(|d| d.insert_temp(edit_meta_id, meta));
        }

        // Edit↔Done 토글 — 진입/이탈 시 선택 노드 초기화.
        if edit_clicked {
            *editing = true;
            *selected_node = None;
            rename = None;
            ctx.request_repaint();
        }
        if done_clicked {
            *editing = false;
            *selected_node = None;
            ctx.request_repaint();
        }

        // ── 툴바 액션 적용 ───────────────────────────────────────────────
        if let Some(name) = current.clone() {
            if rename_clicked {
                rename = Some(RenameState {
                    kind,
                    original: name.clone(),
                    buffer: name.clone(),
                    request_focus: true,
                });
                ctx.request_repaint();
            }
            if duplicate_clicked && let Some(n) = duplicate_preset(store, kind, &name) {
                *selected = Some(n);
                ctx.request_repaint();
            }
            if delete_clicked {
                match store.delete(kind, &name) {
                    Ok(()) => {
                        *selected = None;
                        rename = None;
                        ctx.request_repaint();
                    }
                    Err(e) => tracing::warn!("preset delete failed: {e}"),
                }
            }
        }

        // ── 미리보기 (최종 선택 기준) ────────────────────────────────────
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
                preview_rect,
                *editing,
                selected_node,
                toasts,
                catalog,
                kb,
            ),
            None => {
                ui.painter_at(preview_rect).rect_filled(
                    preview_rect,
                    0.0,
                    theme.bg_app().to_egui(),
                );
                ui.painter_at(preview_rect).text(
                    preview_rect.center(),
                    egui::Align2::CENTER_CENTER,
                    t("preset.popup.empty"),
                    egui::FontId::proportional(theme.font_size_body.value()),
                    theme.text_muted().to_egui(),
                );
            }
        }
    });

    // rename 상태를 메모리에 반영 (None 으로 덮어쓰면 인라인 편집 종료).
    ctx.data_mut(|d| d.insert_temp(rename_id, rename));
}
