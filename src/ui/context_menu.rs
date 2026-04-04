use crate::state::AppState;
use crate::theme;

/// A single context menu row: full-width hover highlight, text left-aligned.
fn context_menu_item(ui: &mut egui::Ui, th: &theme::Theme, label: &str) -> bool {
    let desired = egui::vec2(ui.available_width(), th.item_height_interactive);
    let (rect, response) = ui.allocate_exact_size(desired, egui::Sense::click());

    // Hover highlight — full row
    if response.hovered() {
        ui.painter().rect_filled(rect, th.corner_radius, th.hover_overlay);
    }

    // Text
    let text_pos = egui::pos2(
        rect.min.x + th.spacing_sm,
        rect.center().y - th.font_size_body / 2.0,
    );
    ui.painter().text(
        text_pos,
        egui::Align2::LEFT_TOP,
        label,
        egui::FontId::proportional(th.font_size_body),
        th.text,
    );

    response.clicked()
}

/// Render the pane right-click context menu.
pub fn draw_pane_context_menu(
    ctx: &egui::Context,
    state: &mut AppState,
    _scale_factor: f32,
) {
    let th = theme::theme();
    let menu = match &state.pane_context_menu {
        Some(m) => m.clone(),
        None => return,
    };

    let mut close_menu = false;
    let mut open_markdown_dialog = false;
    let mut open_explorer = false;

    let area_response = egui::Area::new(egui::Id::new("pane_context_menu"))
        .fixed_pos(egui::pos2(menu.x, menu.y))
        .order(egui::Order::Foreground)
        .show(ctx, |ui| {
            egui::Frame::new()
                .fill(th.surface0)
                .stroke(egui::Stroke::new(1.0, th.surface1))
                .corner_radius(th.corner_radius)
                .inner_margin(egui::Margin::same(th.spacing_xs as i8))
                .show(ui, |ui| {
                    ui.set_min_width(180.0);
                    ui.spacing_mut().item_spacing.y = 0.0;

                    if context_menu_item(ui, th, "Open Markdown...") {
                        open_markdown_dialog = true;
                        close_menu = true;
                    }
                    if context_menu_item(ui, th, "Open Explorer") {
                        open_explorer = true;
                        close_menu = true;
                    }

                    // Separator
                    ui.add_space(th.spacing_xs);
                    let rect = ui.available_rect_before_wrap();
                    let sep_rect = egui::Rect::from_min_size(
                        rect.min,
                        egui::vec2(rect.width(), th.border_width),
                    );
                    ui.painter().rect_filled(sep_rect, 0.0, th.separator);
                    ui.advance_cursor_after_rect(sep_rect);
                    ui.add_space(th.spacing_xs);

                    if context_menu_item(ui, th, "Cancel") {
                        close_menu = true;
                    }
                });
        });

    // Arming logic: the menu becomes "armed" once all mouse buttons are released
    // after opening. This prevents the opening right-click release from closing it.
    let any_button_down = ctx.input(|i| i.pointer.any_down());
    if !menu.armed {
        if !any_button_down {
            // All buttons released — arm the menu for future click detection
            if let Some(m) = &mut state.pane_context_menu {
                m.armed = true;
            }
        }
        // Not armed yet — don't close
    } else {
        // Armed: close if clicked outside the menu area
        if area_response.response.clicked_elsewhere()
            && !open_markdown_dialog
            && !open_explorer
        {
            close_menu = true;
        }
    }

    if open_markdown_dialog {
        state.markdown_path_dialog = Some((menu.pane_id, String::new()));
        state.pane_context_menu = None;
    } else if open_explorer {
        // Open explorer at home directory as default
        let home = directories::BaseDirs::new()
            .map(|d| d.home_dir().to_string_lossy().to_string())
            .unwrap_or_else(|| ".".to_string());
        // Focus the target pane
        state.active_workspace_mut().focused_pane = menu.pane_id;
        let _ = state.add_explorer_tab(home);
        state.pane_context_menu = None;
    } else if close_menu {
        state.pane_context_menu = None;
    }
}
