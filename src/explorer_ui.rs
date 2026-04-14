use egui::emath::GuiRounding as _;

use crate::model::{ExplorerPanel, FileNode};
use crate::theme;

/// Draw the explorer panel with a file tree on the left and a file viewer on the right.
pub fn draw_explorer(ui: &mut egui::Ui, panel: &mut ExplorerPanel) -> Option<ExplorerAction> {
    let th = theme::theme();
    let mut explorer_action: Option<ExplorerAction> = None;
    let available_width = ui.available_width();
    let tree_width = (available_width * 0.35).min(250.0).max(150.0).round_ui();

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
                            // Keyboard navigation: Up/Down to move selection, Enter to open/toggle
                            let key_up = ui.input(|i| i.key_pressed(egui::Key::ArrowUp));
                            let key_down = ui.input(|i| i.key_pressed(egui::Key::ArrowDown));
                            let key_enter = ui.input(|i| i.key_pressed(egui::Key::Enter));

                            if (key_up || key_down || key_enter) && action.is_none() {
                                let mut visible = Vec::new();
                                for child in children.iter() {
                                    collect_visible_paths(child, &mut visible);
                                }
                                let current_idx = panel.selected_file.as_ref()
                                    .and_then(|sel| visible.iter().position(|p| p == sel));

                                if key_up || key_down {
                                    let new_idx = match current_idx {
                                        Some(idx) => {
                                            if key_up { idx.saturating_sub(1) }
                                            else { (idx + 1).min(visible.len().saturating_sub(1)) }
                                        }
                                        None => 0,
                                    };
                                    if let Some(path) = visible.get(new_idx) {
                                        action = Some(TreeAction::SelectFile(path.clone()));
                                    }
                                } else if key_enter {
                                    if let Some(sel) = &panel.selected_file {
                                        // Check if it's a directory
                                        let is_dir = visible.contains(sel) && find_node(&panel.root_node, sel)
                                            .is_some_and(|n| n.is_directory);
                                        if is_dir {
                                            action = Some(TreeAction::ToggleDir(sel.clone()));
                                        }
                                    }
                                }
                            }

                            // Apply action
                            if let Some(act) = action {
                                match act {
                                    TreeAction::SelectFile(path) => {
                                        panel.select_file(&path);
                                    }
                                    TreeAction::DoubleClickFile(path) => {
                                        let ext = path.rsplit('.').next().unwrap_or("").to_lowercase();
                                        match ext.as_str() {
                                            "md" | "markdown" => {
                                                explorer_action = Some(ExplorerAction::OpenMarkdownTab(path));
                                            }
                                            "html" | "htm" => {
                                                explorer_action = Some(ExplorerAction::OpenHtmlTab(path));
                                            }
                                            _ => {
                                                panel.select_file(&path);
                                            }
                                        }
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
                                        .color(th.subtext1),
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

    explorer_action
}

enum TreeAction {
    SelectFile(String),
    DoubleClickFile(String),
    ToggleDir(String),
}

/// Action returned from `draw_explorer` for the caller to process.
pub enum ExplorerAction {
    /// Open a markdown file as a new Markdown tab.
    OpenMarkdownTab(String),
    /// Open an HTML file as a new Html tab (file:// URL).
    OpenHtmlTab(String),
}

fn draw_file_node(
    ui: &mut egui::Ui,
    node: &mut FileNode,
    depth: usize,
    selected_path: Option<&str>,
    action: &mut Option<TreeAction>,
) {
    let th = theme::theme();
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
                .color(th.blue)
        } else {
            egui::RichText::new(&text)
        };

        let resp = ui.selectable_label(is_selected, label);
        if resp.double_clicked() && action.is_none() && !node.is_directory {
            *action = Some(TreeAction::DoubleClickFile(node.path.clone()));
        } else if resp.clicked() && action.is_none() {
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

/// Find a node by path in the tree.
fn find_node<'a>(node: &'a FileNode, target: &str) -> Option<&'a FileNode> {
    if node.path == target { return Some(node); }
    if node.is_directory {
        if let Some(ref children) = node.children {
            for child in children {
                if let Some(found) = find_node(child, target) {
                    return Some(found);
                }
            }
        }
    }
    None
}

/// Collect all visible (expanded) node paths in tree order.
fn collect_visible_paths(node: &FileNode, out: &mut Vec<String>) {
    out.push(node.path.clone());
    if node.is_directory && node.is_expanded {
        if let Some(ref children) = node.children {
            for child in children {
                collect_visible_paths(child, out);
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
