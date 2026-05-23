//! Listening-port viewer popup.
//!
//! Lists TCP ports that the active surface's process tree is listening on.
//! Clicking a port opens `http://<host>:<port>` in the system browser.
//!
//! The scan is driven lazily: on each draw we check the cache; if stale we
//! re-scan the descendants of the active terminal's shell PID. Results are
//! cached in `AppState::port_scan` (5 s TTL).

use std::net::IpAddr;
use std::time::Instant;

use tasty_portscan::ListeningPort;

use crate::i18n::t;
use crate::state::AppState;
use crate::theme;
use crate::ui::popup::PopupAction;

pub const PORT_SCANNER_POPUP_ID: &str = "port_scanner";

pub fn draw_port_scanner_popup(ui: &mut egui::Ui, state: &mut AppState) -> PopupAction {
    if ui.ctx().input(|i| i.key_pressed(egui::Key::Escape)) {
        return PopupAction::Close;
    }

    let th = theme::theme();
    let surface_id = focused_terminal_surface_id(state);

    // Refresh cache lazily.
    refresh_if_stale(state, surface_id);

    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = 4.0;

        // Header
        ui.label(
            egui::RichText::new(t("port_scanner.heading"))
                .color(th.text)
                .size(13.0),
        );
        ui.separator();

        let ports = state
            .port_scan
            .get_any(surface_id)
            .map(|s| s.to_vec())
            .unwrap_or_default();

        if ports.is_empty() {
            ui.label(
                egui::RichText::new(t("port_scanner.no_ports"))
                    .color(th.subtext0)
                    .italics(),
            );
        } else {
            egui::ScrollArea::vertical()
                .max_height(240.0)
                .show(ui, |ui| {
                    for port in &ports {
                        if draw_port_row(ui, port) {
                            open_in_browser(port);
                        }
                    }
                });
        }

        ui.separator();
        ui.horizontal(|ui| {
            if ui.button(t("port_scanner.refresh")).clicked() {
                state.port_scan.forget(surface_id);
                refresh_if_stale(state, surface_id);
            }
            ui.label(
                egui::RichText::new(t("port_scanner.hint"))
                    .color(th.overlay0)
                    .size(11.0),
            );
        });
    });

    PopupAction::None
}

fn draw_port_row(ui: &mut egui::Ui, port: &ListeningPort) -> bool {
    let th = theme::theme();
    let full_width = ui.available_width();
    let (rect, resp) = ui.allocate_exact_size(
        egui::vec2(full_width, 22.0),
        egui::Sense::click().union(egui::Sense::hover()),
    );
    if resp.hovered() {
        ui.painter()
            .rect_filled(rect, 4.0, th.hover_overlay.to_egui_premultiplied());
    }
    let label = format!(
        "{}  ·  {}  ·  PID {}",
        port.port,
        format_addr(port.addr),
        port.pid
    );
    ui.painter().text(
        egui::pos2(rect.min.x + 8.0, rect.center().y),
        egui::Align2::LEFT_CENTER,
        label,
        egui::FontId::proportional(12.0),
        if resp.hovered() {
            th.text.into()
        } else {
            th.subtext0.into()
        },
    );
    resp.clicked()
}

fn format_addr(addr: IpAddr) -> String {
    match addr {
        IpAddr::V4(v4) if v4.is_unspecified() => "0.0.0.0".to_string(),
        IpAddr::V6(v6) if v6.is_unspecified() => "[::]".to_string(),
        IpAddr::V4(v4) => v4.to_string(),
        IpAddr::V6(v6) => format!("[{v6}]"),
    }
}

/// Pick a sensible host for the URL: wildcard binds use `localhost`,
/// everything else uses the exact bound address.
fn open_in_browser(port: &ListeningPort) {
    let host = match port.addr {
        IpAddr::V4(v4) if v4.is_unspecified() => "localhost".to_string(),
        IpAddr::V6(v6) if v6.is_unspecified() => "localhost".to_string(),
        IpAddr::V4(v4) => v4.to_string(),
        IpAddr::V6(v6) => format!("[{v6}]"),
    };
    let url = format!("http://{host}:{}", port.port);
    if let Err(e) = webbrowser::open(&url) {
        tracing::warn!("port_scanner: failed to open {url}: {e}");
    }
}

fn refresh_if_stale(state: &mut AppState, surface_id: u32) {
    let now = Instant::now();
    if !state.port_scan.needs_refresh(surface_id, now) {
        return;
    }
    let shell_pid = match shell_pid_for_surface(state, surface_id) {
        Some(p) => p,
        None => {
            state.port_scan.insert(surface_id, Vec::new(), now);
            return;
        }
    };
    let pids = tasty_portscan::collect_descendant_pids(shell_pid);
    let mut ports = tasty_portscan::scan_for_pids(&pids);
    // Sidebar UX: collapse duplicates that differ only by v4/v6 wildcard.
    ports.dedup_by(|a, b| a.port == b.port && a.pid == b.pid && a.addr == b.addr);
    state.port_scan.insert(surface_id, ports, now);
}

fn shell_pid_for_surface(state: &AppState, surface_id: u32) -> Option<u32> {
    let terminal = engine.find_terminal_by_id(surface_id)?;
    terminal.process_id()
}

fn focused_terminal_surface_id(state: &AppState) -> u32 {
    let ws = state.active_workspace(engine);
    let pane_id = ws.focused_pane;
    ws.pane_layout()
        .find_pane(pane_id)
        .and_then(|pane| pane.tabs.get(pane.active_tab))
        .and_then(|tab| tab.focused_surface_id())
        .unwrap_or(0)
}
