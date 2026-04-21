use egui::emath::GuiRounding as _;
use winit::keyboard::{Key, NamedKey};

use crate::model::Rect;
use crate::state::{AppState, PendingKeyEvent};
use crate::theme;

struct EguiPanelInfo {
    pane_id: u32,
    /// If Some, this is a specific surface within a SurfaceGroup.
    /// If None, this is the entire tab's standalone surface.
    surface_id: Option<u32>,
    logical_x: f32,
    logical_y: f32,
    logical_w: f32,
    logical_h: f32,
    /// Whether this panel is the keyboard target (receives pending_surface_keys).
    is_keyboard_target: bool,
}

/// Render egui-based panels (Markdown, Explorer, Html, Empty).
/// Terminal panels are rendered by the wgpu shader pipeline; these are rendered by egui.
/// Supports both standalone non-terminal tabs and non-terminal leaves within SurfaceGroups.
pub fn draw_egui_panels(
    ctx: &egui::Context,
    state: &mut AppState,
    pane_rects: &[(u32, Rect)],
    scale_factor: f32,
) {
    // First pass: gather info about egui-rendered panels (read-only).
    let mut infos = Vec::new();
    {
        let ws = state.active_workspace();
        let focused_pane_id = ws.focused_pane;
        let tab_bar_h = state.tab_bar_height;
        for &(pane_id, pane_rect) in pane_rects {
            let pane = match ws.pane_layout().find_pane(pane_id) {
                Some(p) => p,
                None => continue,
            };
            let tab = match pane.tabs.get(pane.active_tab) {
                Some(t) => t,
                None => continue,
            };
            let surface = tab.surface();

            // Case 1: Entire tab is a non-terminal surface (standalone, not a SurfaceGroup).
            if !surface.has_terminal() && surface.as_surface_group().is_none() {
                infos.push(EguiPanelInfo {
                    pane_id,
                    surface_id: surface.surface_id(),
                    logical_x: (pane_rect.x.value() / scale_factor).round_ui(),
                    logical_y: ((pane_rect.y + tab_bar_h).value() / scale_factor).round_ui(),
                    logical_w: (pane_rect.width.value() / scale_factor).round_ui(),
                    logical_h: (((pane_rect.height - tab_bar_h).max(crate::model::length::PhysicalPx(1.0))).value() / scale_factor).round_ui(),
                    is_keyboard_target: pane_id == focused_pane_id,
                });
                continue;
            }

            // Case 2: SurfaceGroup — collect non-terminal leaf regions.
            if let Some(group) = surface.as_surface_group() {
                let focused_surface_in_group = group.focused_surface;
                let content_rect = Rect {
                    x: pane_rect.x,
                    y: pane_rect.y + tab_bar_h,
                    width: pane_rect.width,
                    height: (pane_rect.height - tab_bar_h).max(crate::model::length::PhysicalPx(1.0)),
                };
                for (sid, rect) in group.layout().egui_regions(content_rect) {
                    infos.push(EguiPanelInfo {
                        pane_id,
                        surface_id: Some(sid),
                        logical_x: (rect.x.value() / scale_factor).round_ui(),
                        logical_y: (rect.y.value() / scale_factor).round_ui(),
                        logical_w: (rect.width.value() / scale_factor).round_ui(),
                        logical_h: (rect.height.value() / scale_factor).round_ui(),
                        is_keyboard_target: pane_id == focused_pane_id
                            && sid == focused_surface_in_group,
                    });
                }
            }
        }
    }

    // Drain pending keyboard events for non-terminal surfaces.
    // Only the panel that is_keyboard_target will use these.
    let surface_keys: Vec<PendingKeyEvent> = state.pending_surface_keys.drain(..).collect();

    // Second pass: render each egui panel.
    let mut pending_explorer_action: Option<(u32, Option<u32>, crate::explorer_ui::ExplorerAction)> = None;
    let mut pending_empty_action: Option<crate::empty_ui::EmptyAction> = None;
    // Clipboard viewer surfaces are rendered after the main loop to sidestep the
    // borrow conflict between surface (via state.workspaces) and state.engine.clipboard_history.
    struct PendingClipboardViewerRender {
        pane_id: u32,
        surface_id: Option<u32>,
        id_suffix: String,
        logical_x: f32,
        logical_y: f32,
        logical_w: f32,
        logical_h: f32,
    }
    let mut pending_clipboard_viewer_renders: Vec<PendingClipboardViewerRender> = Vec::new();

    for info in &infos {
        let id_suffix = info.surface_id.map_or(
            format!("pane_{}", info.pane_id),
            |sid| format!("surface_{}", sid),
        );

        let ws = state.active_workspace_mut();
        let pane = match ws.pane_layout_mut().find_pane_mut(info.pane_id) {
            Some(p) => p,
            None => continue,
        };
        let tab = match pane.active_tab_mut() {
            Some(t) => t,
            None => continue,
        };

        // Get the surface to render: either a leaf within SurfaceGroup, or the tab's surface.
        let surface: &mut dyn crate::model::Surface = if let Some(sid) = info.surface_id {
            if let Some(group) = tab.surface_mut().as_surface_group_mut() {
                match group.layout_mut().find_leaf_mut(sid) {
                    Some(leaf) => leaf.as_mut(),
                    None => continue,
                }
            } else {
                tab.surface_mut()
            }
        } else {
            tab.surface_mut()
        };

        if let Some(md_panel) = surface.as_markdown_mut() {
            let scroll_line = 24.0;
            let scroll_page = info.logical_h * 0.8;
            let key_scroll_y = if info.is_keyboard_target {
                let mut dy = 0.0;
                for k in &surface_keys {
                    match &k.key {
                        Key::Named(NamedKey::ArrowUp) => dy += scroll_line,
                        Key::Named(NamedKey::ArrowDown) => dy -= scroll_line,
                        Key::Named(NamedKey::PageUp) => dy += scroll_page,
                        Key::Named(NamedKey::PageDown) => dy -= scroll_page,
                        _ => {}
                    }
                }
                dy
            } else {
                0.0
            };
            draw_panel_frame(ctx, &format!("md_panel_{}", id_suffix), info, 8, |ui| {
                crate::markdown_ui::draw_markdown(ui, md_panel, key_scroll_y, &id_suffix);
            });
        } else if let Some(exp_panel) = surface.as_explorer_mut() {
            let keys = if info.is_keyboard_target { &surface_keys[..] } else { &[] };
            draw_panel_frame(ctx, &format!("explorer_{}", id_suffix), info, 4, |ui| {
                if let Some(act) = crate::explorer_ui::draw_explorer(ui, exp_panel, keys) {
                    pending_explorer_action = Some((info.pane_id, info.surface_id, act));
                }
            });
        } else if let Some(html_panel) = surface.as_html() {
            draw_panel_frame(ctx, &format!("html_panel_{}", id_suffix), info, 0, |ui| {
                crate::html_ui::draw_html(ui, html_panel);
            });
        } else if let Some(empty) = surface.as_empty_surface() {
            draw_panel_frame_no_margin(ctx, &format!("empty_panel_{}", id_suffix), info, |ui| {
                if let Some(act) = crate::empty_ui::draw_empty(ui, empty) {
                    pending_empty_action = Some(act);
                }
            });
        } else if let Some(image_panel) = surface.as_image_mut() {
            draw_panel_frame(ctx, &format!("image_panel_{}", id_suffix), info, 4, |ui| {
                crate::image_ui::draw_image(ui, image_panel);
            });
        } else if surface.as_clipboard_viewer().is_some() {
            // Defer: we need both engine.clipboard_history and cv.state together,
            // which requires dropping the current surface borrow chain first.
            pending_clipboard_viewer_renders.push(PendingClipboardViewerRender {
                pane_id: info.pane_id,
                surface_id: info.surface_id,
                id_suffix: id_suffix.clone(),
                logical_x: info.logical_x,
                logical_y: info.logical_y,
                logical_w: info.logical_w,
                logical_h: info.logical_h,
            });
        }
    }

    // Render clipboard viewer surfaces now. The main loop is done, so we have
    // exclusive access to state again and can safely borrow engine + surface.
    for pending in &pending_clipboard_viewer_renders {
        let info = EguiPanelInfo {
            pane_id: pending.pane_id,
            surface_id: pending.surface_id,
            logical_x: pending.logical_x,
            logical_y: pending.logical_y,
            logical_w: pending.logical_w,
            logical_h: pending.logical_h,
            is_keyboard_target: false, // egui TextEdit handles focus internally
        };
        // Temporarily take history so it can be borrowed mutably alongside the surface.
        let history_max = state.engine.settings.clipboard.history_max;
        let mut history = std::mem::replace(
            &mut state.engine.clipboard_history,
            crate::clipboard_history::ClipboardHistory::new(1),
        );

        let ws = state.active_workspace_mut();
        let mut paste_index: Option<usize> = None;
        if let Some(pane) = ws.pane_layout_mut().find_pane_mut(pending.pane_id) {
            if let Some(tab) = pane.active_tab_mut() {
                let surface: &mut dyn crate::model::Surface = if let Some(sid) = pending.surface_id {
                    if let Some(group) = tab.surface_mut().as_surface_group_mut() {
                        match group.layout_mut().find_leaf_mut(sid) {
                            Some(leaf) => leaf.as_mut(),
                            None => continue,
                        }
                    } else {
                        tab.surface_mut()
                    }
                } else {
                    tab.surface_mut()
                };
                if let Some(cv) = surface.as_clipboard_viewer_mut() {
                    paste_index = draw_panel_frame(
                        ctx,
                        &format!("clipboard_viewer_{}", pending.id_suffix),
                        &info,
                        4,
                        |ui| {
                            crate::clipboard_viewer_ui::draw_clipboard_viewer_surface(
                                ui,
                                &mut history,
                                &mut cv.state,
                            )
                        },
                    );
                }
            }
        }

        history.set_max(history_max);
        state.engine.clipboard_history = history;
        if let Some(orig) = paste_index {
            crate::clipboard_viewer_ui::paste_from_history(state, orig);
        }
    }

    // Apply deferred empty surface action (must happen after render loop due to state mutation).
    if let Some(crate::empty_ui::EmptyAction::OpenConvertPopup(sid)) = pending_empty_action {
        state.dialogs.convert_popup = Some(sid);
        state.dialogs.convert_popup_selected = None;
        state.popups.open_with_scope("convert_surface", crate::ui::popup::PopupScope::Surface(sid));
    }

    // Process deferred explorer actions (requires state mutation outside the render loop)
    if let Some((pane_id, surface_id, action)) = pending_explorer_action {
        state.active_workspace_mut().focused_pane = pane_id;
        match action {
            crate::explorer_ui::ExplorerAction::OpenMarkdownTab(path) => {
                let _ = state.add_markdown_tab(path);
            }
            crate::explorer_ui::ExplorerAction::OpenHtmlTab(path) => {
                let url = crate::ui::file_open_popup::local_path_to_file_uri(&path);
                let _ = state.add_html_tab(url);
            }
            crate::explorer_ui::ExplorerAction::FolderContextMenu { path, is_bookmarked, x, y } => {
                state.dialogs.pending_native_menu = Some(crate::state::PendingNativeMenu::ExplorerFolder {
                    surface_id: surface_id.unwrap_or(0), path, is_bookmarked, x, y,
                });
            }
            crate::explorer_ui::ExplorerAction::BookmarkContextMenu { path, name, x, y } => {
                state.dialogs.pending_native_menu = Some(crate::state::PendingNativeMenu::BookmarkItem {
                    path, name, x, y,
                });
            }
            crate::explorer_ui::ExplorerAction::CopyFeedback(kind) => {
                let key = match kind {
                    crate::explorer_ui::CopyFeedbackKind::Files => "toast.copied_files",
                    crate::explorer_ui::CopyFeedbackKind::Cut => "toast.cut_files",
                    crate::explorer_ui::CopyFeedbackKind::Path => "toast.copied_path",
                };
                let scope = surface_id
                    .map(crate::ui::ToastScope::Surface)
                    .unwrap_or(crate::ui::ToastScope::Pane(pane_id));
                state.toasts.push_info(crate::i18n::t(key), scope);
            }
        }
    }
}

/// 공통 egui Area + Frame(crust background) 껍데기. `margin`만큼 내부 여백을 준다.
/// body의 반환값을 그대로 전달한다 (None을 리턴하는 기존 호출처는 ()).
fn draw_panel_frame<R, F>(
    ctx: &egui::Context,
    id: &str,
    info: &EguiPanelInfo,
    margin: i8,
    body: F,
) -> R
where
    F: FnOnce(&mut egui::Ui) -> R,
    R: Default,
{
    let th = theme::theme();
    let mut out: R = R::default();
    egui::Area::new(egui::Id::new(id))
        .fixed_pos(egui::pos2(info.logical_x, info.logical_y))
        .order(egui::Order::Background)
        .show(ctx, |ui| {
            ui.set_min_size(egui::vec2(info.logical_w, info.logical_h));
            ui.set_max_size(egui::vec2(info.logical_w, info.logical_h));
            let panel_rect = ui.max_rect();
            let mut clip_ui = ui.new_child(egui::UiBuilder::new().max_rect(panel_rect));
            clip_ui.set_clip_rect(panel_rect);
            clip_ui.painter().rect_filled(panel_rect, 0.0, th.crust);
            egui::Frame::new()
                .fill(th.crust)
                .inner_margin(egui::Margin::same(margin))
                .show(&mut clip_ui, |ui| {
                    out = body(ui);
                });
        });
    out
}

/// 여백 없이 Area만 거는 변형. Empty surface처럼 배경을 직접 칠하는 경우에 사용.
fn draw_panel_frame_no_margin<F>(
    ctx: &egui::Context,
    id: &str,
    info: &EguiPanelInfo,
    body: F,
) where
    F: FnOnce(&mut egui::Ui),
{
    egui::Area::new(egui::Id::new(id))
        .fixed_pos(egui::pos2(info.logical_x, info.logical_y))
        .order(egui::Order::Background)
        .show(ctx, |ui| {
            ui.set_min_size(egui::vec2(info.logical_w, info.logical_h));
            ui.set_max_size(egui::vec2(info.logical_w, info.logical_h));
            let panel_rect = ui.max_rect();
            let mut clip_ui = ui.new_child(egui::UiBuilder::new().max_rect(panel_rect));
            clip_ui.set_clip_rect(panel_rect);
            body(&mut clip_ui);
        });
}
