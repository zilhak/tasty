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

use std::collections::HashSet;
use std::net::IpAddr;
use std::sync::mpsc;

use crate::adapters::ui::popup::PopupAction;
use crate::core::CoreState;
use crate::core::state::SurfaceDisplayPath;
use crate::i18n::t;
use crate::state::AppState;
use crate::theme;
use crate::theme::Theme;

pub const PORT_SCANNER_POPUP_ID: &str = "port_scanner";

/// Which set of listening ports a scan covers.
/// Tasty: only ports owned by Tasty shell process trees.
/// System: every LISTEN socket on the host, with Tasty rows tagged.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScanScope {
    Tasty,
    System,
}

/// Which workspace/tab a row belongs to. `External` means the listening
/// process is not part of any Tasty shell tree.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SourceTag {
    Tasty {
        workspace_name: String,
        tab_name: Option<String>,
    },
    External,
}

/// One row in the redesigned port table — workspace/tab-named projection of a
/// listening port (Tasty or system-wide).
#[derive(Clone, Debug)]
pub struct PortRowView {
    pub port: u16,
    pub addr_display: String,
    pub pid: Option<u32>,
    pub process_name: Option<String>,
    pub source: SourceTag,
}

/// Async state machine for the port scanner popup. Owned by `AppState.port_scan`.
///
/// Transitions: `Idle` → (kick) → `Loading { rx, scope }` → (poll) → `Ready { rows, scope }`
/// or `Failed(msg)`. Closing the popup resets to `Idle`.
pub enum PortScanState {
    Idle,
    Loading {
        rx: mpsc::Receiver<Result<Vec<PortRowView>, String>>,
        scope: ScanScope,
    },
    Ready {
        rows: Vec<PortRowView>,
        scope: ScanScope,
    },
    Failed(String),
}

/// Send-safe snapshot consumed by the background scan worker. Built on the
/// main thread from the current workspace tree; the worker reads it without
/// touching `CoreState`.
pub struct ScanSnapshot {
    pub surfaces: Vec<(u32, u32, SurfaceDisplayPath)>,
    pub show_all_system: bool,
}

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
    engine: &mut CoreState,
) -> PopupAction {
    let th = theme::theme();
    let ctx = ui.ctx().clone();

    // PR-3: scope toggle UI 는 PR-4 가 추가. 그 전까지는 항상 Tasty 모드 (show_all_system=false).
    let target_show_all_system = false;

    // ④ 매 프레임 poll: Loading → Ready/Failed.
    poll_scan(state);

    // ① 첫 open: Idle → kick_off_scan.
    // ② scope 변경: Ready 상태인데 scope 가 다르면 재 kick_off_scan (PR-4 토글이 입력 줄 자리).
    let need_kick = match &state.port_scan {
        PortScanState::Idle => true,
        PortScanState::Ready { scope, .. } => *scope != scope_from_flag(target_show_all_system),
        _ => false,
    };
    if need_kick {
        kick_off_scan(state, engine, &ctx, target_show_all_system);
    }

    let entries: Vec<PortEntryView> = match &state.port_scan {
        PortScanState::Ready { rows, .. } => rows
            .iter()
            .map(|r| PortEntryView {
                port: r.port,
                addr_display: r.addr_display.clone(),
                pid: r.pid.unwrap_or(0),
            })
            .collect(),
        _ => Vec::new(),
    };

    let heading = t("port_scanner.heading");
    let no_ports_label = t("port_scanner.no_ports");
    let refresh_label = t("port_scanner.refresh");
    let hint_label = t("port_scanner.hint");

    let props = PortScannerProps {
        theme: &th,
        heading,
        no_ports_label,
        refresh_label,
        hint_label,
        entries: &entries,
    };

    let action = draw_port_scanner_view(ui, &props);

    match action {
        PortScannerAction::None => PopupAction::None,
        PortScannerAction::Close => {
            // ⑦ close → reset to Idle. 백그라운드 thread 가 살아 있어도 rx 가 drop 되어
            // 마지막 send 가 실패할 뿐 — 스레드 자체는 자연 종료한다.
            state.port_scan = PortScanState::Idle;
            PopupAction::Close
        }
        PortScannerAction::Refresh => {
            // ③ Refresh 클릭: 현재 scope 그대로 재 kick.
            kick_off_scan(state, engine, &ctx, target_show_all_system);
            PopupAction::None
        }
        PortScannerAction::OpenEntry(i) => {
            if let PortScanState::Ready { rows, .. } = &state.port_scan {
                if let Some(row) = rows.get(i) {
                    open_in_browser(row);
                }
            }
            PopupAction::None
        }
    }
}

fn scope_from_flag(show_all_system: bool) -> ScanScope {
    if show_all_system {
        ScanScope::System
    } else {
        ScanScope::Tasty
    }
}

/// Snapshot every Tasty surface that has a live shell PID, paired with its
/// workspace/tab display path. The background worker can run without any
/// CoreState reference.
fn build_snapshot(engine: &CoreState, show_all_system: bool) -> ScanSnapshot {
    let mut surfaces: Vec<(u32, u32, SurfaceDisplayPath)> = Vec::new();
    for ws in &engine.workspaces {
        for pane_id in ws.pane_layout().all_pane_ids() {
            if let Some(pane) = ws.pane_layout().find_pane(pane_id) {
                for tab in &pane.tabs {
                    for sid in tab.all_surface_ids() {
                        let Some(shell_pid) =
                            engine.find_terminal_by_id(sid).and_then(|t| t.process_id())
                        else {
                            continue;
                        };
                        let Some(path) = engine.surface_display_path(sid) else {
                            continue;
                        };
                        surfaces.push((sid, shell_pid, path));
                    }
                }
            }
        }
    }
    ScanSnapshot {
        surfaces,
        show_all_system,
    }
}

/// Move the state machine into `Loading`, spawning a background thread that
/// computes the row set and reports back through an mpsc channel. The thread
/// requests an egui repaint after sending so the main loop wakes up.
pub fn kick_off_scan(
    state: &mut AppState,
    engine: &CoreState,
    ctx: &egui::Context,
    show_all_system: bool,
) {
    let snapshot = build_snapshot(engine, show_all_system);
    let (tx, rx) = mpsc::channel::<Result<Vec<PortRowView>, String>>();
    let scope = scope_from_flag(show_all_system);
    state.port_scan = PortScanState::Loading { rx, scope };
    let ctx = ctx.clone();
    std::thread::spawn(move || {
        let result = run_scan(snapshot);
        let _ = tx.send(result); // popup 이 먼저 닫혀 rx 가 drop 됐다면 send 실패 — 스레드는 자연 종료.
        ctx.request_repaint();
    });
}

/// Background worker. Builds the descendant PID → display-path map, then
/// resolves listening ports either by Tasty PID set (Tasty mode) or by full
/// system scan (System mode), tagging each row with its source.
fn run_scan(snapshot: ScanSnapshot) -> Result<Vec<PortRowView>, String> {
    let mut pid_to_source: std::collections::HashMap<u32, (String, Option<String>)> =
        std::collections::HashMap::new();
    for (_sid, shell_pid, path) in &snapshot.surfaces {
        let descendants = tasty_portscan::collect_descendant_pids(*shell_pid);
        for pid in descendants {
            pid_to_source
                .entry(pid)
                .or_insert_with(|| (path.workspace_name.clone(), path.tab_name.clone()));
        }
    }

    if snapshot.show_all_system {
        let all = tasty_portscan::scan_all();
        let rows: Vec<PortRowView> = all
            .into_iter()
            .map(|p| {
                let source = p
                    .pid
                    .and_then(|pid| pid_to_source.get(&pid))
                    .map(|(ws, tab)| SourceTag::Tasty {
                        workspace_name: ws.clone(),
                        tab_name: tab.clone(),
                    })
                    .unwrap_or(SourceTag::External);
                PortRowView {
                    port: p.port,
                    addr_display: format_addr(p.addr),
                    pid: p.pid,
                    process_name: p.process_name,
                    source,
                }
            })
            .collect();
        Ok(rows)
    } else {
        let tasty_pids: HashSet<u32> = pid_to_source.keys().copied().collect();
        let ports = tasty_portscan::scan_for_pids(&tasty_pids);
        let rows: Vec<PortRowView> = ports
            .into_iter()
            .map(|p| {
                let source = pid_to_source
                    .get(&p.pid)
                    .map(|(ws, tab)| SourceTag::Tasty {
                        workspace_name: ws.clone(),
                        tab_name: tab.clone(),
                    })
                    .unwrap_or(SourceTag::External);
                PortRowView {
                    port: p.port,
                    addr_display: format_addr(p.addr),
                    pid: Some(p.pid),
                    process_name: p.process_name,
                    source,
                }
            })
            .collect();
        Ok(rows)
    }
}

/// Drain a pending result from the channel. No-op when not in `Loading`.
pub fn poll_scan(state: &mut AppState) {
    poll_state(&mut state.port_scan);
}

/// Pure state-machine step on `PortScanState` — exposed so unit tests can
/// drive the transition without building an `AppState`.
fn poll_state(state: &mut PortScanState) {
    if let PortScanState::Loading { rx, scope } = state {
        match rx.try_recv() {
            Ok(Ok(rows)) => {
                let scope = *scope;
                *state = PortScanState::Ready { rows, scope };
            }
            Ok(Err(e)) => {
                *state = PortScanState::Failed(e);
            }
            Err(mpsc::TryRecvError::Empty) => {}
            Err(mpsc::TryRecvError::Disconnected) => {
                *state = PortScanState::Failed("scan worker disconnected".to_string());
            }
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
/// everything else uses the addr_display string as-is (already v6-bracketed
/// by `format_addr`).
fn open_in_browser(row: &PortRowView) {
    let host = match row.addr_display.as_str() {
        "0.0.0.0" | "[::]" => "localhost".to_string(),
        other => other.to_string(),
    };
    let url = format!("http://{host}:{}", row.port);
    if let Err(e) = webbrowser::open(&url) {
        tracing::warn!("port_scanner: failed to open {url}: {e}");
    }
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

    fn dummy_row(port: u16) -> PortRowView {
        PortRowView {
            port,
            addr_display: "127.0.0.1".into(),
            pid: Some(42),
            process_name: None,
            source: SourceTag::External,
        }
    }

    #[test]
    fn scan_state_transitions_idle_loading_ready() {
        let (tx, rx) = mpsc::channel::<Result<Vec<PortRowView>, String>>();
        let mut state = PortScanState::Loading {
            rx,
            scope: ScanScope::Tasty,
        };

        // No message yet — stays Loading.
        poll_state(&mut state);
        assert!(matches!(state, PortScanState::Loading { .. }));

        // Worker reports success → next poll → Ready.
        tx.send(Ok(vec![dummy_row(3000)])).unwrap();
        poll_state(&mut state);
        match state {
            PortScanState::Ready { rows, scope } => {
                assert_eq!(rows.len(), 1);
                assert_eq!(scope, ScanScope::Tasty);
            }
            other => panic!("expected Ready, got {:?}", std::mem::discriminant(&other)),
        }
    }

    #[test]
    fn scan_state_failed_on_worker_error() {
        let (tx, rx) = mpsc::channel::<Result<Vec<PortRowView>, String>>();
        let mut state = PortScanState::Loading {
            rx,
            scope: ScanScope::System,
        };
        tx.send(Err("boom".to_string())).unwrap();
        poll_state(&mut state);
        match state {
            PortScanState::Failed(msg) => assert_eq!(msg, "boom"),
            _ => panic!("expected Failed"),
        }
    }

    #[test]
    fn scan_state_failed_on_disconnect() {
        let (tx, rx) = mpsc::channel::<Result<Vec<PortRowView>, String>>();
        let mut state = PortScanState::Loading {
            rx,
            scope: ScanScope::Tasty,
        };
        // Worker dies without sending.
        drop(tx);
        poll_state(&mut state);
        assert!(matches!(state, PortScanState::Failed(_)));
    }

    #[test]
    fn scope_change_triggers_reload_decision() {
        // Trigger ②: when state is Ready with a different scope, the popup
        // should choose to kick a new scan. Mirrors `need_kick` logic in
        // `draw_port_scanner_popup`.
        let state = PortScanState::Ready {
            rows: Vec::new(),
            scope: ScanScope::System,
        };
        let target = scope_from_flag(false);
        let need_kick = match &state {
            PortScanState::Idle => true,
            PortScanState::Ready { scope, .. } => *scope != target,
            _ => false,
        };
        assert!(need_kick, "scope change should force reload");
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
