use egui::emath::GuiRounding as _;

use crate::model::{PanelBehavior, Rect};
use crate::state::AppState;
use crate::theme;

/// Render egui-based panels (Markdown, Explorer, Html, Empty).
/// Terminal panels are rendered by the wgpu shader pipeline; these are rendered by egui.
pub fn draw_egui_panels(
    ctx: &egui::Context,
    state: &mut AppState,
    pane_rects: &[(u32, Rect)],
    scale_factor: f32,
) {
    let th = theme::theme();
    // First pass: gather info about egui-rendered panels (read-only).
    struct EguiPanelInfo {
        pane_id: u32,
        logical_x: f32,
        logical_y: f32,
        logical_w: f32,
        logical_h: f32,
    }

    let mut infos = Vec::new();
    {
        let ws = state.active_workspace();
        for &(pane_id, pane_rect) in pane_rects {
            let pane = match ws.pane_layout().find_pane(pane_id) {
                Some(p) => p,
                None => continue,
            };
            let panel = match pane.active_panel() {
                Some(p) => p,
                None => continue,
            };
            if panel.has_terminal() {
                continue;
            }
            let tab_bar_h = state.tab_bar_height;
            infos.push(EguiPanelInfo {
                pane_id,
                logical_x: (pane_rect.x / scale_factor).round_ui(),
                logical_y: ((pane_rect.y + tab_bar_h) / scale_factor).round_ui(),
                logical_w: (pane_rect.width / scale_factor).round_ui(),
                logical_h: ((pane_rect.height - tab_bar_h).max(1.0) / scale_factor).round_ui(),
            });
        }
    }

    // Second pass: render each egui panel.
    let mut pending_explorer_action: Option<(u32, crate::explorer_ui::ExplorerAction)> = None;
    let mut pending_empty_convert: Option<u32> = None;

    for info in &infos {
        let ws = state.active_workspace_mut();
        let pane = match ws.pane_layout_mut().find_pane_mut(info.pane_id) {
            Some(p) => p,
            None => continue,
        };
        let tab = match pane.active_tab_mut() {
            Some(t) => t,
            None => continue,
        };

        match tab.panel_mut() {
            crate::model::Panel::Markdown(md_panel) => {
                // Keyboard scrolling for Markdown panels
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

                egui::Area::new(egui::Id::new(format!("md_panel_{}", info.pane_id)))
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
                                    .id_salt(format!("md_scroll_{}", info.pane_id))
                                    .show(ui, |ui| {
                                        let content = md_panel.content.clone();
                                        crate::markdown_ui::render_markdown(ui, &content);
                                    });
                            });
                    });
            }
            crate::model::Panel::Explorer(exp_panel) => {
                egui::Area::new(egui::Id::new(format!("explorer_{}", info.pane_id)))
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
            }
            crate::model::Panel::Html(html_panel) => {
                // Draw a placeholder background behind the native WebView.
                // Visible when the WebView is temporarily hidden (e.g. during overlay display).
                egui::Area::new(egui::Id::new(format!("html_panel_{}", info.pane_id)))
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
                                        egui::RichText::new(&html_panel.url)
                                            .color(th.overlay0)
                                            .size(th.font_size_body),
                                    );
                                });
                            });
                    });
            }
            crate::model::Panel::Empty { id: surface_id } => {
                let sid = *surface_id;
                egui::Area::new(egui::Id::new(format!("empty_panel_{}", info.pane_id)))
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
            _ => {}
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
