use crate::model::{ExplorerPanel, FileNode};

/// Draw the explorer panel with a file tree on the left and a file viewer on the right.
pub fn draw_explorer(ui: &mut egui::Ui, panel: &mut ExplorerPanel) {
    let available_width = ui.available_width();
    let tree_width = (available_width * 0.35).min(250.0).max(150.0);

    ui.horizontal_top(|ui| {
        // Left: File tree
        ui.vertical(|ui| {
            ui.set_width(tree_width);
            ui.set_min_height(ui.available_height());

            egui::ScrollArea::vertical()
                .id_salt("explorer_tree")
                .show(ui, |ui| {
                    let root = &mut panel.root_node;
                    if root.is_directory {
                        if let Some(ref mut children) = root.children {
                            // We need to collect actions because we can't mutate panel fields
                            // while iterating the tree.
                            let mut action: Option<TreeAction> = None;
                            for child in children.iter_mut() {
                                draw_file_node(
                                    ui,
                                    child,
                                    0,
                                    panel.selected_file.as_deref(),
                                    &mut action,
                                );
                            }
                            // Apply action
                            if let Some(act) = action {
                                match act {
                                    TreeAction::SelectFile(path) => {
                                        panel.select_file(&path);
                                    }
                                    TreeAction::ToggleDir(path) => {
                                        toggle_dir_by_path(&mut panel.root_node, &path);
                                    }
                                }
                            }
                        }
                    }
                });
        });

        ui.separator();

        // Right: File viewer
        ui.vertical(|ui| {
            if let Some(ref path) = panel.selected_file {
                // File path header
                ui.label(
                    egui::RichText::new(path)
                        .small()
                        .color(egui::Color32::GRAY),
                );
                ui.separator();

                if let Some(ref content) = panel.file_content {
                    egui::ScrollArea::vertical()
                        .id_salt("explorer_viewer")
                        .show(ui, |ui| {
                            if panel.is_markdown {
                                crate::markdown_ui::render_markdown(ui, content);
                            } else {
                                // Render as plain text with monospace font
                                ui.label(
                                    egui::RichText::new(content)
                                        .monospace()
                                        .size(12.0)
                                        .color(egui::Color32::from_rgb(200, 200, 210)),
                                );
                            }
                        });
                }
            } else {
                ui.centered_and_justified(|ui| {
                    ui.label(
                        egui::RichText::new("Select a file")
                            .color(egui::Color32::GRAY),
                    );
                });
            }
        });
    });
}

enum TreeAction {
    SelectFile(String),
    ToggleDir(String),
}

fn draw_file_node(
    ui: &mut egui::Ui,
    node: &mut FileNode,
    depth: usize,
    selected_path: Option<&str>,
    action: &mut Option<TreeAction>,
) {
    let indent = depth as f32 * 16.0;
    let is_selected = selected_path == Some(&node.path);

    ui.horizontal(|ui| {
        ui.add_space(indent);

        let icon = if node.is_directory {
            if node.is_expanded {
                "\u{25BC} \u{1F4C1}"
            } else {
                "\u{25B6} \u{1F4C1}"
            }
        } else {
            let ext = node.name.rsplit('.').next().unwrap_or("");
            match ext {
                "md" | "markdown" => "  \u{1F4DD}",
                "rs" => "  \u{1F980}",
                "toml" | "json" | "yaml" | "yml" => "  \u{2699}",
                _ => "  \u{1F4C4}",
            }
        };

        let text = format!("{} {}", icon, node.name);
        let label = if is_selected {
            egui::RichText::new(&text)
                .strong()
                .color(egui::Color32::from_rgb(120, 180, 255))
        } else {
            egui::RichText::new(&text)
        };

        if ui.selectable_label(is_selected, label).clicked() && action.is_none() {
            if node.is_directory {
                *action = Some(TreeAction::ToggleDir(node.path.clone()));
            } else {
                *action = Some(TreeAction::SelectFile(node.path.clone()));
            }
        }
    });

    // Render children if expanded
    if node.is_directory && node.is_expanded {
        if let Some(ref mut children) = node.children {
            for child in children.iter_mut() {
                draw_file_node(ui, child, depth + 1, selected_path, action);
            }
        }
    }
}

/// Toggle a directory node by its path. Recurses through the tree to find it.
fn toggle_dir_by_path(node: &mut FileNode, target_path: &str) {
    if node.path == target_path && node.is_directory {
        node.is_expanded = !node.is_expanded;
        if node.is_expanded && node.children.is_none() {
            ExplorerPanel::load_directory(node);
        }
        return;
    }
    if node.is_directory && node.is_expanded {
        if let Some(ref mut children) = node.children {
            for child in children.iter_mut() {
                toggle_dir_by_path(child, target_path);
            }
        }
    }
}
