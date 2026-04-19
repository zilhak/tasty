use std::collections::HashSet;

use egui::emath::GuiRounding as _;
use winit::keyboard::{Key, NamedKey};

use crate::model::{ExplorerPanel, FileNode};
use crate::state::PendingKeyEvent;
use crate::theme;

/// Check if a specific key was pressed in the pending key events.
fn key_pressed(keys: &[PendingKeyEvent], target: NamedKey) -> bool {
    keys.iter().any(|k| k.key == Key::Named(target))
}

/// Check if a character key was pressed in the pending key events.
fn char_pressed(keys: &[PendingKeyEvent], ch: &str) -> bool {
    keys.iter().any(|k| matches!(&k.key, Key::Character(c) if c.as_str().eq_ignore_ascii_case(ch)))
}

/// Check if any key event has command modifier (Ctrl on Linux/Win, Cmd on macOS).
fn has_command(keys: &[PendingKeyEvent]) -> bool {
    keys.iter().any(|k| {
        #[cfg(target_os = "macos")]
        { k.modifiers.super_key() }
        #[cfg(not(target_os = "macos"))]
        { k.modifiers.control_key() }
    })
}

/// Check if any key event has shift modifier.
fn has_shift(keys: &[PendingKeyEvent]) -> bool {
    keys.iter().any(|k| k.modifiers.shift_key())
}

/// Draw the explorer panel with a file tree on the left and a file viewer on the right.
/// `keys` contains keyboard events routed by the central dispatcher — only present
/// when this explorer is the focused surface.
pub fn draw_explorer(ui: &mut egui::Ui, panel: &mut ExplorerPanel, keys: &[PendingKeyEvent]) -> Option<ExplorerAction> {
    let th = theme::theme();
    let mut explorer_action: Option<ExplorerAction> = None;
    let available_width = ui.available_width();
    let tree_width = if panel.show_preview {
        (available_width * panel.tree_ratio).max(80.0).min(available_width - 80.0).round_ui()
    } else {
        available_width
    };

    // ── Address bar (top, full width) ──
    let address_bar_h = 20.0;
    ui.horizontal(|ui| {
        ui.set_height(address_bar_h);
        let resp = ui.add_sized(
            [ui.available_width(), address_bar_h],
            egui::TextEdit::singleline(&mut panel.address_bar_text)
                .font(egui::FontId::proportional(th.font_size_caption))
                .margin(egui::Margin::symmetric(4, 2)),
        );
        if resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
            let path = panel.address_bar_text.clone();
            panel.navigate_to(path);
        }
    });
    ui.add_space(2.0);

    ui.horizontal_top(|ui| {
        // Left: File tree + Bookmarks
        ui.vertical(|ui| {
            ui.set_width(tree_width);

            // ── Toolbar above file tree ──
            ui.horizontal(|ui| {
                let label = if panel.show_preview {
                    crate::i18n::t("explorer.hide_preview")
                } else {
                    crate::i18n::t("explorer.show_preview")
                };
                if ui.button(
                    egui::RichText::new(label)
                        .size(th.font_size_caption),
                ).clicked() {
                    panel.show_preview = !panel.show_preview;
                }
            });
            ui.separator();

            // 좌측 패널 수직 분할: 파일 트리 75%, 즐겨찾기 25% 고정.
            // 두 섹션 사이에 separator(≈1px) + add_space(2.0) 두 번 = 약 5px의 고정 비용.
            const SECTION_GAP: f32 = 5.0;
            let total_height = ui.available_height();
            let bookmark_height = ((total_height - SECTION_GAP) * 0.25).max(40.0);
            let tree_height = (total_height - bookmark_height - SECTION_GAP).max(40.0);

            // ── File tree (고정 75% 점유) ──
            egui::ScrollArea::vertical()
                .id_salt("explorer_tree")
                .max_height(tree_height)
                .min_scrolled_height(tree_height)
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.set_min_width(ui.available_width());
                    let mut needs_refresh = false;
                    let root = &mut panel.root_node;
                    if root.is_directory {
                        if let Some(ref mut children) = root.children {
                            // Collect visible paths for range selection
                            let mut visible: Vec<String> = Vec::new();
                            for child in children.iter() {
                                collect_visible_paths(child, &mut visible);
                            }

                            // Draw tree nodes — collect action
                            let mut action: Option<TreeAction> = None;
                            for child in children.iter_mut() {
                                draw_file_node(
                                    ui,
                                    child,
                                    0,
                                    &panel.selected_files,
                                    panel.selected_file.as_deref(),
                                    &mut action,
                                );
                            }

                            // Read keyboard from dispatched key queue (NOT egui global input)
                            let cmd = has_command(keys);
                            let shift = has_shift(keys);

                            // Keyboard: file clipboard (Ctrl/Cmd+C/X/V, Ctrl/Cmd+A)
                            if cmd && action.is_none() {
                                let key_c = char_pressed(keys, "c");
                                let key_x = char_pressed(keys, "x");
                                let key_v = char_pressed(keys, "v");
                                let key_a = char_pressed(keys, "a");

                                if key_a {
                                    action = Some(TreeAction::SelectAll);
                                } else if (key_c || key_x) && !panel.selected_files.is_empty() {
                                    let op = if key_x {
                                        crate::file_clipboard::FileClipboardOp::Cut
                                    } else {
                                        crate::file_clipboard::FileClipboardOp::Copy
                                    };
                                    if shift && key_c {
                                        // Shift+Ctrl+C = copy paths as text
                                        let text = panel.selected_files.iter()
                                            .cloned().collect::<Vec<_>>().join("\n");
                                        action = Some(TreeAction::CopyPath(text));
                                    } else {
                                        let paths: Vec<&str> = panel.selected_files.iter()
                                            .map(|s| s.as_str()).collect();
                                        let _ = crate::file_clipboard::set_file_clipboard(&paths, op);
                                    }
                                } else if key_v {
                                    // Determine paste destination
                                    let dest_dir = paste_destination(panel);
                                    if let Ok(Some((sources, op))) = crate::file_clipboard::get_file_clipboard() {
                                        for src in &sources {
                                            let file_name = std::path::Path::new(src)
                                                .file_name()
                                                .map(|n| n.to_string_lossy().to_string())
                                                .unwrap_or_default();
                                            let dest = std::path::Path::new(&dest_dir).join(&file_name);
                                            if op == crate::file_clipboard::FileClipboardOp::Cut {
                                                let _ = std::fs::rename(src, &dest);
                                            } else if std::path::Path::new(src).is_dir() {
                                                let _ = copy_dir_recursive(src, &dest.to_string_lossy());
                                            } else {
                                                let _ = std::fs::copy(src, &dest);
                                            }
                                        }
                                        needs_refresh = true;
                                    }
                                }
                            }

                            // Keyboard navigation (from dispatched key queue)
                            let key_up = key_pressed(keys, NamedKey::ArrowUp);
                            let key_down = key_pressed(keys, NamedKey::ArrowDown);
                            let key_enter = key_pressed(keys, NamedKey::Enter);

                            if (key_up || key_down || key_enter) && action.is_none() {
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
                                        if shift {
                                            action = Some(TreeAction::RangeSelect(path.clone()));
                                        } else {
                                            action = Some(TreeAction::SelectFile(path.clone()));
                                        }
                                    }
                                } else if key_enter {
                                    if let Some(sel) = &panel.selected_file {
                                        let is_dir = find_node(&panel.root_node, sel)
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
                                        panel.select_single(&path);
                                    }
                                    TreeAction::ToggleSelect(path) => {
                                        panel.toggle_select(&path);
                                    }
                                    TreeAction::RangeSelect(path) => {
                                        panel.range_select(&path, &visible);
                                    }
                                    TreeAction::SelectAll => {
                                        panel.select_all(&visible);
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
                                                panel.select_single(&path);
                                            }
                                        }
                                    }
                                    TreeAction::ToggleDir(path) => {
                                        toggle_dir_by_path(&mut panel.root_node, &path);
                                    }
                                    TreeAction::CopyPath(text) => {
                                        ui.ctx().copy_text(text);
                                    }
                                    TreeAction::ContextMenu(path, pos) => {
                                        let is_bookmarked = crate::bookmarks::Bookmarks::load().is_bookmarked(&path);
                                        explorer_action = Some(ExplorerAction::FolderContextMenu { path, is_bookmarked, x: pos.x, y: pos.y });
                                    }
                                }
                            }
                        }
                    }
                    if needs_refresh {
                        crate::model::ExplorerPanel::load_directory(&mut panel.root_node);
                    }
                });

            ui.add_space(2.0);
            ui.separator();

            // ── Bookmarks (고정 25% 점유) ──
            ui.label(
                egui::RichText::new(crate::i18n::t("explorer.bookmarks_heading"))
                    .size(th.font_size_caption)
                    .strong()
                    .color(th.subtext0),
            );
            ui.add_space(2.0);
            egui::ScrollArea::vertical()
                .id_salt("explorer_bookmarks")
                .max_height(bookmark_height)
                .min_scrolled_height(bookmark_height)
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.set_min_width(ui.available_width());
                    let mut nav_path: Option<String> = None;
                    let bookmarks = crate::bookmarks::Bookmarks::load();
                    for bm in &bookmarks.entries {
                        let resp = ui.selectable_label(
                            false,
                            egui::RichText::new(format!("\u{2605} {}", bm.name)).size(th.font_size_caption),
                        );
                        if resp.double_clicked() {
                            nav_path = Some(bm.path.clone());
                        }
                        if resp.secondary_clicked() {
                            if let Some(pos) = ui.ctx().input(|i| i.pointer.latest_pos()) {
                                explorer_action = Some(ExplorerAction::BookmarkContextMenu {
                                    path: bm.path.clone(),
                                    name: bm.name.clone(),
                                    x: pos.x,
                                    y: pos.y,
                                });
                            }
                        }
                    }
                    if bookmarks.entries.is_empty() {
                        ui.label(
                            egui::RichText::new(crate::i18n::t("explorer.bookmarks_empty"))
                                .small()
                                .color(th.overlay0),
                        );
                    }
                    if let Some(path) = nav_path {
                        panel.navigate_to(path);
                    }
                });
        });

        if panel.show_preview {
            // Draggable divider between tree and preview
            let divider_width = 6.0;
            let divider_rect = {
                let cursor = ui.cursor();
                egui::Rect::from_min_size(
                    egui::pos2(cursor.min.x, cursor.min.y),
                    egui::vec2(divider_width, ui.available_height()),
                )
            };
            let divider_id = ui.id().with("explorer_divider");
            let divider_resp = ui.interact(divider_rect, divider_id, egui::Sense::drag());

            // Paint the divider line
            let divider_color = if divider_resp.hovered() || divider_resp.dragged() {
                th.blue
            } else {
                th.surface1
            };
            ui.painter().rect_filled(
                egui::Rect::from_min_size(
                    egui::pos2(divider_rect.center().x - 0.5, divider_rect.min.y),
                    egui::vec2(1.0, divider_rect.height()),
                ),
                0.0,
                divider_color,
            );

            // Change cursor on hover
            if divider_resp.hovered() || divider_resp.dragged() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
            }

            // Handle drag
            if divider_resp.dragged() {
                let delta = divider_resp.drag_delta().x;
                if delta != 0.0 {
                    let new_width = tree_width + delta;
                    panel.tree_ratio = (new_width / available_width).clamp(0.15, 0.85);
                }
            }
            ui.advance_cursor_after_rect(divider_rect);

            // Right: File viewer
            ui.vertical(|ui| {
                ui.set_min_width(ui.available_width());
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
                                ui.style_mut().interaction.selectable_labels = true;
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
                    } else {
                        ui.centered_and_justified(|ui| {
                            ui.label(
                                egui::RichText::new(crate::i18n::t("explorer.unsupported_format"))
                                    .color(th.overlay0),
                            );
                        });
                    }
                } else {
                    ui.centered_and_justified(|ui| {
                        ui.label(
                            egui::RichText::new(crate::i18n::t("explorer.select_file"))
                                .color(egui::Color32::GRAY),
                        );
                    });
                }
            });
        }
    });

    explorer_action
}

enum TreeAction {
    /// Normal click — clear selection, select one.
    SelectFile(String),
    /// Ctrl/Cmd+click — toggle one item.
    ToggleSelect(String),
    /// Shift+click — range select from anchor.
    RangeSelect(String),
    /// Ctrl/Cmd+A — select all visible.
    SelectAll,
    /// Double-click on file — open in dedicated tab.
    DoubleClickFile(String),
    /// Double-click on directory or Enter — expand/collapse.
    ToggleDir(String),
    /// Copy path(s) as text.
    CopyPath(String),
    /// Right-click on a directory — show context menu for bookmark add/remove.
    ContextMenu(String, egui::Pos2),
}

/// Action returned from `draw_explorer` for the caller to process.
pub enum ExplorerAction {
    /// Open a markdown file as a new Markdown tab.
    OpenMarkdownTab(String),
    /// Open an HTML file as a new Html tab (file:// URL).
    OpenHtmlTab(String),
    /// Request native context menu for a folder (path, is_bookmarked, x, y).
    FolderContextMenu { path: String, is_bookmarked: bool, x: f32, y: f32 },
    /// Request native context menu for a bookmark item (path, name, x, y).
    BookmarkContextMenu { path: String, name: String, x: f32, y: f32 },
}

fn draw_file_node(
    ui: &mut egui::Ui,
    node: &mut FileNode,
    depth: usize,
    selected_files: &HashSet<String>,
    focus_path: Option<&str>,
    action: &mut Option<TreeAction>,
) {
    let th = theme::theme();
    let indent = depth as f32 * 16.0;
    let is_in_selection = selected_files.contains(&node.path);
    let is_focus = focus_path == Some(&node.path);

    ui.horizontal(|ui| {
        ui.add_space(indent);

        // Directory arrow icon — clickable separately for toggle
        if node.is_directory {
            let arrow = if node.is_expanded { "\u{25BC}" } else { "\u{25B6}" };
            let arrow_resp = ui.small_button(
                egui::RichText::new(arrow).size(10.0),
            );
            if arrow_resp.clicked() && action.is_none() {
                *action = Some(TreeAction::ToggleDir(node.path.clone()));
            }
        }

        let icon = if node.is_directory {
            "\u{1F4C1}"
        } else {
            let ext = node.name.rsplit('.').next().unwrap_or("");
            match ext {
                "md" | "markdown" => "\u{1F4DD}",
                "rs" => "\u{1F980}",
                "toml" | "json" | "yaml" | "yml" => "\u{2699}",
                _ => "\u{1F4C4}",
            }
        };

        // For non-directory items, add spacing to align with directory items
        if !node.is_directory {
            ui.add_space(4.0);
        }

        let text = format!("{} {}", icon, node.name);
        let label = if is_focus {
            egui::RichText::new(&text).strong().color(th.blue)
        } else if is_in_selection {
            egui::RichText::new(&text).color(th.text)
        } else {
            egui::RichText::new(&text)
        };

        let resp = ui.selectable_label(is_in_selection, label);

        if resp.double_clicked() && action.is_none() {
            if node.is_directory {
                *action = Some(TreeAction::ToggleDir(node.path.clone()));
            } else {
                *action = Some(TreeAction::DoubleClickFile(node.path.clone()));
            }
        } else if resp.secondary_clicked() && action.is_none() && node.is_directory {
            let pos = resp.interact_pointer_pos().unwrap_or_default();
            *action = Some(TreeAction::ContextMenu(node.path.clone(), pos));
        } else if resp.clicked() && action.is_none() {
            let modifiers = ui.input(|i| i.modifiers);
            if modifiers.command {
                *action = Some(TreeAction::ToggleSelect(node.path.clone()));
            } else if modifiers.shift {
                *action = Some(TreeAction::RangeSelect(node.path.clone()));
            } else {
                *action = Some(TreeAction::SelectFile(node.path.clone()));
            }
        }
    });

    // Render children if expanded
    if node.is_directory && node.is_expanded {
        if let Some(ref mut children) = node.children {
            for child in children.iter_mut() {
                draw_file_node(ui, child, depth + 1, selected_files, focus_path, action);
            }
        }
    }
}

/// Determine the paste destination directory.
fn paste_destination(panel: &ExplorerPanel) -> String {
    // If exactly one directory is selected, paste into it
    if panel.selected_files.len() == 1 {
        let path = panel.selected_files.iter().next().unwrap();
        if std::path::Path::new(path).is_dir() {
            return path.clone();
        }
    }
    // If the focused file exists, paste into its parent directory
    if let Some(ref sel) = panel.selected_file {
        if let Some(parent) = std::path::Path::new(sel).parent() {
            return parent.to_string_lossy().to_string();
        }
    }
    // Fallback to root
    panel.root_path.clone()
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

/// Recursively copy a directory.
fn copy_dir_recursive(src: &str, dst: &str) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let src_path = entry.path();
        let dst_path = std::path::Path::new(dst).join(entry.file_name());
        if src_path.is_dir() {
            copy_dir_recursive(&src_path.to_string_lossy(), &dst_path.to_string_lossy())?;
        } else {
            std::fs::copy(&src_path, &dst_path)?;
        }
    }
    Ok(())
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
