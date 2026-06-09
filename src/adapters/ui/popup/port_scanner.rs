//! Listening-port viewer popup.
//!
//! Lists TCP ports that the active surface's process tree is listening on.
//! Clicking a port opens `http://<host>:<port>` in the system browser.
//!
//! The scan is driven lazily: on each draw we check the cache; if stale we
//! re-scan the descendants of the active terminal's shell PID. Results are
//! cached in `AppState::port_scan` (5 s TTL).
//!
//! ## Split: wrapper / view / action
//!
//! The pure visual (`draw_port_scanner_view`) takes only `PortScannerProps`
//! (no `AppState` / `CoreState`) and returns `PortScannerAction`. The
//! `draw_port_scanner_popup` wrapper extracts props from runtime state, calls
//! the view, then translates the returned action back into state mutation +
//! `PopupAction`. The gallery (`tasty-gallery`) mirrors the view with mock
//! props to verify visual states without runtime state.

use std::net::IpAddr;
use std::time::Instant;

use tasty_portscan::ListeningPort;

use crate::adapters::ui::popup::PopupAction;
use crate::i18n::t;
use crate::state::AppState;
use crate::theme;
use crate::theme::Theme;

pub const PORT_SCANNER_POPUP_ID: &str = "port_scanner";

/// One entry in the port list — host-side projection of `ListeningPort` with
/// `addr` already formatted for display.
#[derive(Clone, Debug)]
pub struct PortEntryView {
    pub port: u16,
    pub addr_display: String,
    pub pid: u32,
}

/// Pure inputs to `draw_port_scanner_view`. Contains no `AppState` /
/// `CoreState` — every value is read-only.
pub struct PortScannerProps<'a> {
    pub theme: &'a Theme,
    pub heading: &'a str,
    pub no_ports_label: &'a str,
    pub refresh_label: &'a str,
    pub hint_label: &'a str,
    pub entries: &'a [PortEntryView],
}

/// User intent surfaced by the view. The wrapper translates these into
/// state mutation + side effects (browser launch, cache invalidation).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PortScannerAction {
    None,
    Close,
    Refresh,
    OpenEntry(usize),
}

pub fn draw_port_scanner_popup(
    ui: &mut egui::Ui,
    state: &mut AppState,
    engine: &mut crate::core::CoreState,
) -> PopupAction {
    let th = theme::theme();
    let surface_id = focused_terminal_surface_id(state, engine);

    // Refresh cache lazily.
    refresh_if_stale(state, engine, surface_id);

    let ports: Vec<ListeningPort> = state
        .port_scan
        .get_any(surface_id)
        .map(|s| s.to_vec())
        .unwrap_or_default();

    let entries: Vec<PortEntryView> = ports
        .iter()
        .map(|p| PortEntryView {
            port: p.port,
            addr_display: format_addr(p.addr),
            pid: p.pid,
        })
        .collect();

    let heading = t("port_scanner.heading");
    let no_ports_label = t("port_scanner.no_ports");
    let refresh_label = t("port_scanner.refresh");
    let hint_label = t("port_scanner.hint");

    let props = PortScannerProps {
        theme: &th,
        heading: &heading,
        no_ports_label: &no_ports_label,
        refresh_label: &refresh_label,
        hint_label: &hint_label,
        entries: &entries,
    };

    let action = draw_port_scanner_view(ui, &props);

    match action {
        PortScannerAction::None => PopupAction::None,
        PortScannerAction::Close => PopupAction::Close,
        PortScannerAction::Refresh => {
            state.port_scan.forget(surface_id);
            refresh_if_stale(state, engine, surface_id);
            PopupAction::None
        }
        PortScannerAction::OpenEntry(i) => {
            if let Some(port) = ports.get(i) {
                open_in_browser(port);
            }
            PopupAction::None
        }
    }
}

/// Pure view: draws the popup body from `props` and reports intent.
///
/// No `AppState` / `CoreState` / global `theme::theme()` access. Safe to call
/// from a gallery with mock props.
pub fn draw_port_scanner_view(
    ui: &mut egui::Ui,
    props: &PortScannerProps<'_>,
) -> PortScannerAction {
    if ui.ctx().input(|i| i.key_pressed(egui::Key::Escape)) {
        return PortScannerAction::Close;
    }

    let th = props.theme;
    let mut action = PortScannerAction::None;

    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = 4.0;

        // Header
        ui.label(egui::RichText::new(props.heading).color(th.text).size(13.0));
        ui.separator();

        if props.entries.is_empty() {
            ui.label(
                egui::RichText::new(props.no_ports_label)
                    .color(th.subtext0)
                    .italics(),
            );
        } else {
            egui::ScrollArea::vertical()
                .max_height(240.0)
                .show(ui, |ui| {
                    for (i, entry) in props.entries.iter().enumerate() {
                        if draw_port_row(ui, th, entry) {
                            action = PortScannerAction::OpenEntry(i);
                        }
                    }
                });
        }

        ui.separator();
        ui.horizontal(|ui| {
            if ui.button(props.refresh_label).clicked() {
                action = PortScannerAction::Refresh;
            }
            ui.label(
                egui::RichText::new(props.hint_label)
                    .color(th.overlay0)
                    .size(11.0),
            );
        });
    });

    action
}

fn draw_port_row(ui: &mut egui::Ui, th: &Theme, entry: &PortEntryView) -> bool {
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
        entry.port, entry.addr_display, entry.pid
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

fn refresh_if_stale(state: &mut AppState, engine: &mut crate::core::CoreState, surface_id: u32) {
    let now = Instant::now();
    if !state.port_scan.needs_refresh(surface_id, now) {
        return;
    }
    let shell_pid = match shell_pid_for_surface(state, engine, surface_id) {
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

fn shell_pid_for_surface(
    _state: &AppState,
    engine: &crate::core::CoreState,
    surface_id: u32,
) -> Option<u32> {
    let terminal = engine.find_terminal_by_id(surface_id)?;
    terminal.process_id()
}

fn focused_terminal_surface_id(state: &AppState, engine: &crate::core::CoreState) -> u32 {
    let ws = state.active_workspace(engine);
    let pane_id = ws.focused_pane;
    ws.pane_layout()
        .find_pane(pane_id)
        .and_then(|pane| pane.tabs.get(pane.active_tab))
        .and_then(|tab| tab.focused_surface_id())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_theme() -> Theme {
        tasty_themes::mocha_fallback()
    }

    fn run_with_input(raw: egui::RawInput, entries: &[PortEntryView]) -> PortScannerAction {
        let ctx = egui::Context::default();
        let mut out = PortScannerAction::None;
        let theme = test_theme();
        // FullOutput 은 폐기 — 단위 테스트는 view 의 반환 action 만 검증.
        drop(ctx.run(raw, |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                let props = PortScannerProps {
                    theme: &theme,
                    heading: "Listening ports",
                    no_ports_label: "No ports.",
                    refresh_label: "Refresh",
                    hint_label: "Click a row to open in browser.",
                    entries,
                };
                out = draw_port_scanner_view(ui, &props);
            });
        }));
        out
    }

    #[test]
    fn view_returns_none_on_empty_input() {
        let action = run_with_input(egui::RawInput::default(), &[]);
        assert_eq!(action, PortScannerAction::None);
    }

    #[test]
    fn view_returns_close_on_escape() {
        let mut raw = egui::RawInput::default();
        raw.events.push(egui::Event::Key {
            key: egui::Key::Escape,
            physical_key: Some(egui::Key::Escape),
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        });
        let action = run_with_input(raw, &[]);
        assert_eq!(action, PortScannerAction::Close);
    }

    #[test]
    fn view_renders_with_entries_without_panic() {
        let entries = vec![
            PortEntryView {
                port: 3000,
                addr_display: "0.0.0.0".into(),
                pid: 12345,
            },
            PortEntryView {
                port: 8080,
                addr_display: "127.0.0.1".into(),
                pid: 12346,
            },
        ];
        let action = run_with_input(egui::RawInput::default(), &entries);
        assert_eq!(action, PortScannerAction::None);
    }
}
