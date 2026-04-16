use egui::emath::GuiRounding as _;

use crate::model::Rect;
use crate::state::AppState;
use crate::theme;

/// Render egui-based panels (Markdown, Explorer, Html, Empty).
/// Terminal panels are rendered by the wgpu shader pipeline; these are rendered by egui.
/// Supports both standalone non-terminal tabs and non-terminal leaves within SurfaceGroups.
pub fn draw_egui_panels(
    ctx: &egui::Context,
    state: &mut AppState,
    pane_rects: &[(u32, Rect)],
    scale_factor: f32,
) {
    let th = theme::theme();

    struct EguiPanelInfo {
        pane_id: u32,
        /// If Some, this is a specific surface within a SurfaceGroup.
        /// If None, this is the entire tab's standalone surface.
        surface_id: Option<u32>,
        logical_x: f32,
        logical_y: f32,
        logical_w: f32,
        logical_h: f32,
    }

    // First pass: gather info about egui-rendered panels (read-only).
    let mut infos = Vec::new();
    {
        let ws = state.active_workspace();
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
                    surface_id: None,
                    logical_x: (pane_rect.x / scale_factor).round_ui(),
                    logical_y: ((pane_rect.y + tab_bar_h) / scale_factor).round_ui(),
                    logical_w: (pane_rect.width / scale_factor).round_ui(),
                    logical_h: ((pane_rect.height - tab_bar_h).max(1.0) / scale_factor).round_ui(),
                });
                continue;
            }

            // Case 2: SurfaceGroup — collect non-terminal leaf regions.
            if let Some(group) = surface.as_surface_group() {
                let content_rect = Rect {
                    x: pane_rect.x,
                    y: pane_rect.y + tab_bar_h,
                    width: pane_rect.width,
                    height: (pane_rect.height - tab_bar_h).max(1.0),
                };
                for (sid, rect) in group.layout().egui_regions(content_rect) {
                    infos.push(EguiPanelInfo {
                        pane_id,
                        surface_id: Some(sid),
                        logical_x: (rect.x / scale_factor).round_ui(),
                        logical_y: (rect.y / scale_factor).round_ui(),
                        logical_w: (rect.width / scale_factor).round_ui(),
                        logical_h: (rect.height / scale_factor).round_ui(),
                    });
                }
            }
        }
    }

    // Second pass: render each egui panel.
    let mut pending_explorer_action: Option<(u32, crate::explorer_ui::ExplorerAction)> = None;
    let mut pending_empty_convert: Option<u32> = None;

    for info in &infos {
        // Unique ID suffix for egui Areas (avoids collisions when multiple panels exist).
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
                continue;
            }
        } else {
            tab.surface_mut()
        };

        if let Some(md_panel) = surface.as_markdown_mut() {
            let scroll_line = 24.0;
            let scroll_page = info.logical_h * 0.8;
            let key_scroll_y = ctx.input(|i| {
                let mut dy = 0.0;
                if i.key_pressed(egui::Key::ArrowUp) { dy += scroll_line; }
                if i.key_pressed(egui::Key::ArrowDown) { dy -= scroll_line; }
                if i.key_pressed(egui::Key::PageUp) { dy += scroll_page; }
                if i.key_pressed(egui::Key::PageDown) { dy -= scroll_page; }
                dy
            });

            egui::Area::new(egui::Id::new(format!("md_panel_{}", id_suffix)))
                .fixed_pos(egui::pos2(info.logical_x, info.logical_y))
                .order(egui::Order::Background)
                .show(ctx, |ui| {
                    ui.set_min_size(egui::vec2(info.logical_w, info.logical_h));
                    ui.set_max_size(egui::vec2(info.logical_w, info.logical_h));
                    egui::Frame::new()
                        .fill(th.crust)
                        .inner_margin(egui::Margin::same(8))
                        .show(ui, |ui| {
                            if key_scroll_y != 0.0 {
                                ui.scroll_with_delta(egui::vec2(0.0, key_scroll_y));
                            }
                            egui::ScrollArea::vertical()
                                .id_salt(format!("md_scroll_{}", id_suffix))
                                .show(ui, |ui| {
                                    let content = md_panel.content.clone();
                                    crate::markdown_ui::render_markdown(ui, &content);
                                });
                        });
                });
        } else if let Some(exp_panel) = surface.as_explorer_mut() {
            egui::Area::new(egui::Id::new(format!("explorer_{}", id_suffix)))
                .fixed_pos(egui::pos2(info.logical_x, info.logical_y))
                .order(egui::Order::Background)
                .show(ctx, |ui| {
                    ui.set_min_size(egui::vec2(info.logical_w, info.logical_h));
                    ui.set_max_size(egui::vec2(info.logical_w, info.logical_h));
                    egui::Frame::new()
                        .fill(th.crust)
                        .inner_margin(egui::Margin::same(4))
                        .show(ui, |ui| {
                            if let Some(act) = crate::explorer_ui::draw_explorer(ui, exp_panel) {
                                pending_explorer_action = Some((info.pane_id, act));
                            }
                        });
                });
        } else if let Some(html_panel) = surface.as_html() {
            let url = html_panel.url.clone();
            egui::Area::new(egui::Id::new(format!("html_panel_{}", id_suffix)))
                .fixed_pos(egui::pos2(info.logical_x, info.logical_y))
                .order(egui::Order::Background)
                .show(ctx, |ui| {
                    ui.set_min_size(egui::vec2(info.logical_w, info.logical_h));
                    ui.set_max_size(egui::vec2(info.logical_w, info.logical_h));
                    egui::Frame::new()
                        .fill(th.crust)
                        .show(ui, |ui| {
                            ui.centered_and_justified(|ui| {
                                ui.label(
                                    egui::RichText::new(&url)
                                        .color(th.overlay0)
                                        .size(th.font_size_body),
                                );
                            });
                        });
                });
        } else if let Some(empty) = surface.as_empty_surface() {
            let sid = empty.id;
            egui::Area::new(egui::Id::new(format!("empty_panel_{}", id_suffix)))
                .fixed_pos(egui::pos2(info.logical_x, info.logical_y))
                .order(egui::Order::Background)
                .show(ctx, |ui| {
                    ui.set_min_size(egui::vec2(info.logical_w, info.logical_h));
                    ui.set_max_size(egui::vec2(info.logical_w, info.logical_h));
                    egui::Frame::new()
                        .fill(th.crust)
                        .show(ui, |ui| {
                            ui.centered_and_justified(|ui| {
                                let btn = ui.button(
                                    egui::RichText::new(crate::i18n::t("convert_popup.title"))
                                        .size(th.font_size_body)
                                        .color(th.text),
                                );
                                if btn.clicked() {
                                    pending_empty_convert = Some(sid);
                                }
                            });
                        });
                });
        }
    }

    // Process deferred empty surface convert
    if let Some(sid) = pending_empty_convert {
        state.dialogs.convert_popup = Some(sid);
        state.dialogs.convert_popup_selected = None;
        state.popups.open_with_scope("convert_surface", crate::ui::popup::PopupScope::Surface(sid));
    }

    // Process deferred explorer actions (requires state mutation outside the render loop)
    if let Some((pane_id, action)) = pending_explorer_action {
        state.active_workspace_mut().focused_pane = pane_id;
        match action {
            crate::explorer_ui::ExplorerAction::OpenMarkdownTab(path) => {
                let _ = state.add_markdown_tab(path);
            }
            crate::explorer_ui::ExplorerAction::OpenHtmlTab(path) => {
                let url = format!("file://{}", path);
                let _ = state.add_html_tab(url);
            }
        }
    }
}
