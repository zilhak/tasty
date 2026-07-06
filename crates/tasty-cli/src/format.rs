use super::{Commands, ListCommands};

pub fn format_output(command: &Commands, result: &serde_json::Value) {
    match command {
        Commands::List { command } => format_list_output(command, result),
        _ => {
            // Pretty print JSON
            println!("{}", serde_json::to_string_pretty(result).unwrap());
        }
    }
}

fn format_list_output(command: &ListCommands, result: &serde_json::Value) {
    match command {
        ListCommands::Tree => format_tree(result),
        ListCommands::Workspaces => format_workspace_list(result),
        ListCommands::Panes => format_pane_list(result),
        ListCommands::Notifications => format_notification_list(result),
        _ => {
            println!("{}", serde_json::to_string_pretty(result).unwrap());
        }
    }
}

#[allow(clippy::cognitive_complexity)] // complexity-exempt: 리팩터 후보 — workspace→pane→tab→surface 4중 nested 트리 렌더. 레벨별 헬퍼 분리 여지 있으나 게이트 도입과 별건
fn format_tree(result: &serde_json::Value) {
    if let Some(workspaces) = result.as_array() {
        for ws in workspaces {
            let ws_id = ws.get("id").and_then(|v| v.as_u64()).unwrap_or(0);
            let name = ws.get("name").and_then(|v| v.as_str()).unwrap_or("?");
            let active = ws.get("active").and_then(|v| v.as_bool()).unwrap_or(false);
            let marker = if active { " *" } else { "" };
            println!("Workspace: {} (id:{}){}", name, ws_id, marker);

            if let Some(panes) = ws.get("panes").and_then(|v| v.as_array()) {
                for pane in panes {
                    let pid = pane.get("id").and_then(|v| v.as_u64()).unwrap_or(0);
                    let focused = pane
                        .get("focused")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false);
                    let pfx = if focused { ">" } else { " " };
                    println!("  {} Pane {} (id:{})", pfx, pid, pid);

                    if let Some(tabs) = pane.get("tabs").and_then(|v| v.as_array()) {
                        for tab in tabs {
                            let tid = tab.get("id").and_then(|v| v.as_u64());
                            let tname = tab.get("name").and_then(|v| v.as_str()).unwrap_or("?");
                            let tactive =
                                tab.get("active").and_then(|v| v.as_bool()).unwrap_or(false);
                            let tpfx = if tactive { "*" } else { " " };

                            // Extract surface info from the tab's surface field
                            let surface = tab.get("surface");
                            let stype =
                                surface.and_then(|s| s.get("type")).and_then(|v| v.as_str());
                            let sid = surface.and_then(|s| s.get("id")).and_then(|v| v.as_u64());
                            let surfaces_arr = surface
                                .and_then(|s| s.get("surfaces"))
                                .and_then(|v| v.as_array());

                            // Split tab with full nested layout → render the
                            // SurfaceGroup split tree under the tab line.
                            if stype == Some("SplitLayout")
                                && let Some(layout) = surface.and_then(|s| s.get("layout"))
                                && !layout.is_null()
                            {
                                let focused = surface
                                    .and_then(|s| s.get("focused_surface"))
                                    .and_then(|v| v.as_u64());
                                match tid {
                                    Some(t) => {
                                        println!("      {} {} (tab:{})", tpfx, tname, t)
                                    }
                                    None => println!("      {} {}", tpfx, tname),
                                }
                                let mut lines = Vec::new();
                                render_layout(layout, "        ", true, focused, &mut lines);
                                for l in lines {
                                    println!("{}", l);
                                }
                                continue;
                            }

                            let mut ids = String::new();
                            if let Some(t) = tid {
                                ids.push_str(&format!("tab:{}", t));
                            }
                            if let Some(s) = sid {
                                if !ids.is_empty() {
                                    ids.push_str(", ");
                                }
                                ids.push_str(&format!("surface:{}", s));
                            } else if let Some(arr) = surfaces_arr {
                                // SplitLayout: list all surface IDs
                                for s in arr {
                                    if let Some(sv) = s.as_u64() {
                                        if !ids.is_empty() {
                                            ids.push_str(", ");
                                        }
                                        ids.push_str(&format!("surface:{}", sv));
                                    }
                                }
                            }
                            if let Some(t) = stype
                                && t != "Terminal"
                            {
                                if !ids.is_empty() {
                                    ids.push_str(", ");
                                }
                                ids.push_str(t);
                            }

                            if ids.is_empty() {
                                println!("      {} {}", tpfx, tname);
                            } else {
                                println!("      {} {} [{}]", tpfx, tname, ids);
                            }
                        }
                    }
                }
            }
        }
    }
}

/// Render a `to_tree_json_full` split tree as an indented ASCII tree.
///
/// `prefix` is the running indent for this node's line; `is_last` controls the
/// branch glyph (`└─` vs `├─`). `focused` marks the focused surface leaf.
///
/// Split node label carries direction · ratio · child positions:
/// `vertical (L|R)` = first child left, second right; `horizontal (T|B)` =
/// first top, second bottom (Vertical splits width, Horizontal splits height).
fn render_layout(
    node: &serde_json::Value,
    prefix: &str,
    is_last: bool,
    focused: Option<u64>,
    out: &mut Vec<String>,
) {
    let branch = if is_last { "└─ " } else { "├─ " };
    let child_prefix = format!("{}{}", prefix, if is_last { "   " } else { "│  " });

    match node.get("type").and_then(|v| v.as_str()) {
        Some("Split") => {
            let dir = node
                .get("direction")
                .and_then(|v| v.as_str())
                .unwrap_or("?");
            let ratio = node.get("ratio").and_then(|v| v.as_f64()).unwrap_or(0.5);
            let pct = (ratio * 100.0).round() as i64;
            let sides = if dir == "vertical" { "L|R" } else { "T|B" };
            out.push(format!(
                "{}{}{} ({}) {}:{}",
                prefix,
                branch,
                dir,
                sides,
                pct,
                100 - pct
            ));
            if let Some(first) = node.get("first") {
                render_layout(first, &child_prefix, false, focused, out);
            }
            if let Some(second) = node.get("second") {
                render_layout(second, &child_prefix, true, focused, out);
            }
        }
        _ => {
            // Leaf surface.
            let kind = node.get("kind").and_then(|v| v.as_str()).unwrap_or("?");
            let id = node.get("id").and_then(|v| v.as_u64());
            let focus_mark = if id.is_some() && id == focused {
                " *focus"
            } else {
                ""
            };
            match id {
                Some(i) => out.push(format!(
                    "{}{}surface:{} ({}){}",
                    prefix, branch, i, kind, focus_mark
                )),
                None => out.push(format!("{}{}({}){}", prefix, branch, kind, focus_mark)),
            }
        }
    }
}

fn format_workspace_list(result: &serde_json::Value) {
    if let Some(workspaces) = result.as_array() {
        for ws in workspaces {
            let name = ws.get("name").and_then(|v| v.as_str()).unwrap_or("?");
            let active = ws.get("active").and_then(|v| v.as_bool()).unwrap_or(false);
            let pane_count = ws.get("pane_count").and_then(|v| v.as_u64()).unwrap_or(0);
            let marker = if active { " *" } else { "" };
            println!("{}{} ({} panes)", name, marker, pane_count);
        }
    }
}

fn format_pane_list(result: &serde_json::Value) {
    if let Some(panes) = result.as_array() {
        for pane in panes {
            let pid = pane.get("id").and_then(|v| v.as_u64()).unwrap_or(0);
            let focused = pane
                .get("focused")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let tab_count = pane.get("tab_count").and_then(|v| v.as_u64()).unwrap_or(0);
            let ws_id = pane
                .get("workspace_id")
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            let ws_name = pane
                .get("workspace_name")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let marker = if focused { " *" } else { "" };
            println!(
                "Pane {}{} ({} tabs) [ws:{} {}]",
                pid, marker, tab_count, ws_id, ws_name
            );
        }
    }
}

fn format_notification_list(result: &serde_json::Value) {
    if let Some(notifs) = result.as_array() {
        if notifs.is_empty() {
            println!("No notifications");
            return;
        }
        for n in notifs {
            let title = n.get("title").and_then(|v| v.as_str()).unwrap_or("");
            let body = n.get("body").and_then(|v| v.as_str()).unwrap_or("");
            let read = n.get("read").and_then(|v| v.as_bool()).unwrap_or(false);
            let marker = if read { " " } else { "*" };
            if body.is_empty() {
                println!("{} {}", marker, title);
            } else {
                println!("{} {}: {}", marker, title, body);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::render_layout;
    use serde_json::json;

    #[test]
    fn render_layout_nested_split_tree() {
        // vertical(L|R) 60:40 → [left leaf 396, right = horizontal(T|B) 50:50 of 417/418]
        let layout = json!({
            "type": "Split",
            "direction": "vertical",
            "ratio": 0.6,
            "first": { "type": "Leaf", "id": 396, "kind": "terminal" },
            "second": {
                "type": "Split",
                "direction": "horizontal",
                "ratio": 0.5,
                "first": { "type": "Leaf", "id": 417, "kind": "terminal" },
                "second": { "type": "Leaf", "id": 418, "kind": "markdown" },
            },
        });
        let mut out = Vec::new();
        render_layout(&layout, "        ", true, Some(417), &mut out);
        let expected = vec![
            "        └─ vertical (L|R) 60:40",
            "           ├─ surface:396 (terminal)",
            "           └─ horizontal (T|B) 50:50",
            "              ├─ surface:417 (terminal) *focus",
            "              └─ surface:418 (markdown)",
        ];
        assert_eq!(out, expected);
    }
}
