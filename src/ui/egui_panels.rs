use egui::emath::GuiRounding as _;
use winit::keyboard::{Key, NamedKey};

use crate::model::Rect;
use crate::state::{AppState, PendingKeyEvent};
use crate::theme;

struct EguiPanelInfo {
    pane_id: u32,
    /// If Some, this is a specific surface within a split tab.
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
/// Supports both standalone non-terminal tabs and non-terminal leaves within split tabs.
pub fn draw_egui_panels(
    ctx: &egui::Context,
    state: &mut AppState,
    pane_rects: &[(u32, Rect)],
    scale_factor: f32,
    canvas_cache: &crate::gpu::canvas_texture::CanvasTextureCache,
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

            // Collect non-GPU-rendered surfaces from this tab.
            let focused_surface_in_tab = tab.focused_surface;
            let content_rect = Rect {
                x: pane_rect.x,
                y: pane_rect.y + tab_bar_h,
                width: pane_rect.width,
                height: (pane_rect.height - tab_bar_h)
                    .max(crate::model::length::PhysicalPx(1.0)),
            };
            // egui로 그려지는 surface = terminal 외 모든 종류.
            for r in tab
                .layout()
                .surface_regions(content_rect)
                .into_iter()
                .filter(|r| r.surface.kind() != "terminal")
            {
                infos.push(EguiPanelInfo {
                    pane_id,
                    surface_id: Some(r.id),
                    logical_x: (r.rect.x.value() / scale_factor).round_ui(),
                    logical_y: (r.rect.y.value() / scale_factor).round_ui(),
                    logical_w: (r.rect.width.value() / scale_factor).round_ui(),
                    logical_h: (r.rect.height.value() / scale_factor).round_ui(),
                    is_keyboard_target: pane_id == focused_pane_id
                        && r.id == focused_surface_in_tab,
                });
            }
        }
    }

    // Drain pending keyboard events for non-terminal surfaces.
    // Only the panel that is_keyboard_target will use these.
    let surface_keys: Vec<PendingKeyEvent> = state.pending_surface_keys.drain(..).collect();

    // Second pass: render each egui panel.
    let mut pending_empty_action: Option<crate::empty_ui::EmptyAction> = None;
    let mut pending_diff_action: Option<(u32, crate::diff_ui::DiffAction)> = None;

    let markdown_colors = state.engine.settings.appearance.markdown_colors.clone();
    let markdown_font = state.engine.settings.appearance.effective_markdown_font();

    // Temporarily extract view stores so we can hold a `&mut View` from
    // the store at the same time as `&mut Panel` from `state.engine.workspaces`.
    // (Same pattern used below for clipboard_history.)
    let mut markdown_views = std::mem::take(&mut state.markdown_views);
    let mut image_views = std::mem::take(&mut state.image_views);

    for info in &infos {
        let id_suffix = info
            .surface_id
            .map_or(format!("pane_{}", info.pane_id), |sid| {
                format!("surface_{}", sid)
            });

        let ws = state.active_workspace_mut();
        let pane = match ws.pane_layout_mut().find_pane_mut(info.pane_id) {
            Some(p) => p,
            None => continue,
        };
        let tab = match pane.active_tab_mut() {
            Some(t) => t,
            None => continue,
        };

        // Get the surface to render: either a leaf within a split tab, or the tab's surface.
        let surface: &mut dyn crate::model::Surface = if let Some(sid) = info.surface_id {
            match tab.layout_mut().find_leaf_mut(sid) {
                Some(leaf) => leaf.as_mut(),
                None => continue,
            }
        } else {
            tab.surface_mut()
        };

        if let Some(md_panel) = surface
            .as_any_mut()
            .downcast_mut::<crate::model::MarkdownPanel>()
        {
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
            let md_bg = if info.is_keyboard_target {
                markdown_colors.focused_bg.to_egui()
            } else {
                markdown_colors.unfocused_bg.to_egui()
            };
            let view = markdown_views.get_or_init(md_panel);
            draw_panel_frame(ctx, &format!("md_panel_{}", id_suffix), info, 8, Some(md_bg), |ui| {
                crate::markdown_ui::draw_markdown(
                    ui,
                    view,
                    key_scroll_y,
                    &id_suffix,
                    &markdown_font,
                );
            });
        } else if let Some(html_panel) = surface
            .as_any()
            .downcast_ref::<crate::model::HtmlPanel>()
        {
            draw_panel_frame(ctx, &format!("html_panel_{}", id_suffix), info, 0, None, |ui| {
                crate::html_ui::draw_html(ui, html_panel);
            });
        } else if let Some(empty) = surface
            .as_any()
            .downcast_ref::<crate::model::EmptySurface>()
        {
            draw_panel_frame_no_margin(ctx, &format!("empty_panel_{}", id_suffix), info, |ui| {
                if let Some(act) = crate::empty_ui::draw_empty(ui, empty) {
                    pending_empty_action = Some(act);
                }
            });
        } else if let Some(image_panel) = surface
            .as_any_mut()
            .downcast_mut::<crate::model::ImagePanel>()
        {
            let view = image_views.get_or_init(image_panel);
            draw_panel_frame(ctx, &format!("image_panel_{}", id_suffix), info, 4, None, |ui| {
                crate::image_ui::draw_image(ui, image_panel, view);
            });
        } else if let Some(diff_panel) = surface
            .as_any()
            .downcast_ref::<crate::model::DiffPanel>()
        {
            draw_panel_frame(ctx, &format!("diff_panel_{}", id_suffix), info, 4, None, |ui| {
                if let Some(act) = crate::diff_ui::draw_diff(ui, diff_panel) {
                    pending_diff_action = Some((diff_panel.id, act));
                }
            });
        } else if let Some(remote) = surface
            .as_any()
            .downcast_ref::<crate::plugin::remote_surface::RemoteSurface>()
        {
            draw_panel_frame(ctx, &format!("remote_panel_{}", id_suffix), info, 4, None, |ui| {
                crate::plugin::ui_tree_render::render_remote_surface(ui, remote, canvas_cache);
            });
        }
    }

    // Restore extracted view stores before any further `state` access below.
    state.markdown_views = markdown_views;
    state.image_views = image_views;

    // Apply deferred empty surface action (must happen after render loop due to state mutation).
    if let Some(crate::empty_ui::EmptyAction::OpenConvertPopup(sid)) = pending_empty_action {
        state.dialogs.convert_popup = Some(sid);
        state.dialogs.convert_popup_selected = None;
        state.dispatch_intent(
            crate::intent::Intent::OpenPopup {
                id: "convert_surface",
                mode: crate::intent::OpenPopupMode::WithScope(
                    crate::ui::popup::PopupScope::Surface(sid),
                ),
            }
            .from_user_menu("empty_surface_convert"),
        );
    }

    // Apply deferred diff surface action. Apply 는 명령을 시스템 클립보드에 복사하고
    // surface 를 닫는다 — 사용자가 활성 터미널에 붙여넣어 실행하도록 안내. 자동
    // spawn 은 사용자 동선을 가로채므로 의도적으로 피한다. Reject 는 surface 만 닫는다.
    if let Some((sid, act)) = pending_diff_action {
        match act {
            crate::diff_ui::DiffAction::Apply(cmd) => {
                if let Err(e) = arboard::Clipboard::new().and_then(|mut cb| cb.set_text(cmd.clone())) {
                    tracing::warn!("diff apply clipboard copy failed: {e}");
                }
                state.toasts.push_info(
                    crate::i18n::t("diff.toast.apply_copied").to_string(),
                    crate::ui::ToastScope::Window,
                );
                state.close_surface_by_id_no_snapshot(sid); // intent-exempt: diff cleanup, undo 의미 없음
            }
            crate::diff_ui::DiffAction::Reject => {
                state.close_surface_by_id_no_snapshot(sid); // intent-exempt: diff cleanup, undo 의미 없음
            }
        }
    }
}

/// 공통 egui Area + Frame 껍데기. `margin`만큼 내부 여백을 준다.
/// `bg_color`가 Some이면 해당 색상을, None이면 th.crust를 배경으로 사용한다.
/// body의 반환값을 그대로 전달한다 (None을 리턴하는 기존 호출처는 ()).
fn draw_panel_frame<R, F>(
    ctx: &egui::Context,
    id: &str,
    info: &EguiPanelInfo,
    margin: i8,
    bg_color: Option<egui::Color32>,
    body: F,
) -> R
where
    F: FnOnce(&mut egui::Ui) -> R,
    R: Default,
{
    let th = theme::theme();
    let bg = bg_color.unwrap_or(th.crust.into());
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
            clip_ui.painter().rect_filled(panel_rect, 0.0, bg);
            egui::Frame::new()
                .fill(bg)
                .inner_margin(egui::Margin::same(margin))
                .show(&mut clip_ui, |ui| {
                    out = body(ui);
                });
        });
    out
}

/// 여백 없이 Area만 거는 변형. Empty surface처럼 배경을 직접 칠하는 경우에 사용.
fn draw_panel_frame_no_margin<F>(ctx: &egui::Context, id: &str, info: &EguiPanelInfo, body: F)
where
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
