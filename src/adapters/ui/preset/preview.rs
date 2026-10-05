//! 프리셋 미리보기와 상세 설정 편집을 그린다.

use super::*;

/// 프리셋 미리보기를 캐시해 선택한 탭과 편집 결과를 유지한다. 편집 변경은 자동 저장하고 실패하면 알린다.
#[allow(clippy::too_many_arguments)]
pub(super) fn draw_preview(
    ui: &mut egui::Ui,
    store: &mut PresetDrafts,
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
pub(super) fn draw_preview_editing(
    ui: &mut egui::Ui,
    store: &mut PresetDrafts,
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
    match queue_layout(store, kind, name, &cache.layout, &cache.base) {
        Ok(QueuedLayout::Queued(base)) => cache.base = base,
        Ok(QueuedLayout::Conflict) => {
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
pub(super) fn draw_settings_detail(
    ui: &mut egui::Ui,
    store: &mut PresetDrafts,
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
            match queue_layout(store, kind, name, &candidate, &cache.base) {
                Ok(QueuedLayout::Queued(base)) => {
                    store_demo(
                        ui,
                        DemoCache {
                            key,
                            layout: candidate,
                            base,
                        },
                    );
                    *selected_node = Some(leaf_id);
                    ui.ctx().data_mut(|data| {
                        data.insert_temp(egui::Id::new("preset.confirmed_surface_cfg"), cfg.clone())
                    });
                    *surface_cfg = None;
                }
                Ok(QueuedLayout::Conflict) => {
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
