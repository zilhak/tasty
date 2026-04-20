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

fn format_tree(result: &serde_json::Value) {
    if let Some(workspaces) = result.as_array() {
        for ws in workspaces {
            let ws_id = ws.get("id").and_then(|v| v.as_u64()).unwrap_or(0);
            let name = ws.get("name").and_then(|v| v.as_str()).unwrap_or("?");
            let active = ws.get("active").and_then(|v| v.as_bool()).unwrap_or(false);
            let marker = if active { " *" } else { "" };
            println!("Workspace: {} (id:{}){}",  name, ws_id, marker);

            if let Some(panes) = ws.get("panes").and_then(|v| v.as_array()) {
                for pane in panes {
                    let pid = pane.get("id").and_then(|v| v.as_u64()).unwrap_or(0);
                    let focused = pane.get("focused").and_then(|v| v.as_bool()).unwrap_or(false);
                    let pfx = if focused { ">" } else { " " };
                    println!("  {} Pane {} (id:{})", pfx, pid, pid);

                    if let Some(tabs) = pane.get("tabs").and_then(|v| v.as_array()) {
                        for tab in tabs {
                            let tid = tab.get("id").and_then(|v| v.as_u64());
                            let tname = tab.get("name").and_then(|v| v.as_str()).unwrap_or("?");
                            let tactive = tab.get("active").and_then(|v| v.as_bool()).unwrap_or(false);
                            let tpfx = if tactive { "*" } else { " " };

                            // Extract surface info from the tab's surface field
                            let surface = tab.get("surface");
                            let stype = surface.and_then(|s| s.get("type")).and_then(|v| v.as_str());
                            let sid = surface.and_then(|s| s.get("id")).and_then(|v| v.as_u64());

                            let mut ids = String::new();
                            if let Some(t) = tid {
                                ids.push_str(&format!("tab:{}", t));
                            }
                            if let Some(s) = sid {
                                if !ids.is_empty() { ids.push_str(", "); }
                                ids.push_str(&format!("surface:{}", s));
                            }
                            if let Some(t) = stype {
                                if t != "Terminal" {
                                    if !ids.is_empty() { ids.push_str(", "); }
                                    ids.push_str(t);
                                }
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
            let focused = pane.get("focused").and_then(|v| v.as_bool()).unwrap_or(false);
            let tab_count = pane.get("tab_count").and_then(|v| v.as_u64()).unwrap_or(0);
            let ws_id = pane.get("workspace_id").and_then(|v| v.as_u64()).unwrap_or(0);
            let ws_name = pane.get("workspace_name").and_then(|v| v.as_str()).unwrap_or("");
            let marker = if focused { " *" } else { "" };
            println!("Pane {}{} ({} tabs) [ws:{} {}]", pid, marker, tab_count, ws_id, ws_name);
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
