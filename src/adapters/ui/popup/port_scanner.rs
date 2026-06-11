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

/// Which column the table is sorted by. `Proto` and `State` are not sortable.
/// PR-5 wires header clicks; PR-4 keeps the field as a stable default.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SortKey {
    Port,
    Address,
    Pid,
    Process,
    Workspace,
    Tab,
}

/// Ascending or descending sort order. Same PR-5 caveat as [`SortKey`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SortDir {
    Asc,
    Desc,
}

/// User-controlled view state, persisted in `egui::Memory` across frames
/// (Id: `"port_scanner.filter"`).
#[derive(Clone, Debug, PartialEq)]
pub struct FilterState {
    pub show_all_system: bool,
    pub query: String,
    pub sort_key: SortKey,
    pub sort_dir: SortDir,
}

impl Default for FilterState {
    fn default() -> Self {
        Self {
            show_all_system: false,
            query: String::new(),
            sort_key: SortKey::Port,
            sort_dir: SortDir::Asc,
        }
    }
}

/// Async view payload the table renders from. `Loading`/`Failed` short-circuit
/// the table; `Ready` carries the row slice + count so the header tag can be
/// drawn consistently.
#[derive(Clone, Copy)]
pub enum PortScannerViewState<'a> {
    Loading,
    Ready { rows: &'a [PortRowView] },
    Failed { message: &'a str },
}

/// Filter inputs handed to the view (read-only projection of [`FilterState`]).
pub struct PortScannerFilter<'a> {
    pub show_all_system: bool,
    pub query: &'a str,
    /// Consumed by PR-5 (sortable header). PR-4 keeps the field for shape.
    #[allow(dead_code)]
    pub sort_key: SortKey,
    /// Consumed by PR-5 (sortable header). PR-4 keeps the field for shape.
    #[allow(dead_code)]
    pub sort_dir: SortDir,
}

/// Pure inputs to `draw_port_scanner_view`. Contains no `AppState` /
/// `CoreState` — every value is read-only. All user-facing strings are
/// pre-resolved by the wrapper so the view is i18n-agnostic.
pub struct PortScannerProps<'a> {
    pub theme: &'a Theme,
    pub view_state: PortScannerViewState<'a>,
    pub filter: PortScannerFilter<'a>,
    pub label_heading: &'a str,
    pub label_search_placeholder: &'a str,
    pub label_filter_show_all_system: &'a str,
    pub label_loading: &'a str,
    pub label_failed: &'a str,
    pub label_close: &'a str,
    pub label_refresh: &'a str,
    pub label_external_dash: &'a str,
    pub label_no_ports_tasty_empty: &'a str,
    pub label_no_ports_system_empty: &'a str,
    pub label_no_ports_search_zero: &'a str,
    pub label_footer_loading: &'a str,
    pub label_header_tag_scanning: &'a str,
    /// Format string with `{n}` placeholder, e.g. `"{n} listening"`.
    pub label_header_tag_count: &'a str,
    /// Format string with `{n}` placeholder, e.g. `"{n} listening"`.
    pub label_footer_counter: &'a str,
    pub label_column_port: &'a str,
    pub label_column_proto: &'a str,
    pub label_column_address: &'a str,
    pub label_column_process: &'a str,
    pub label_column_workspace: &'a str,
    pub label_column_tab: &'a str,
    pub label_column_state: &'a str,
}

/// User intent surfaced by the view. The wrapper translates these into
/// state mutation + side effects (browser launch, scan kick-off).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PortScannerAction {
    None,
    Close,
    Refresh,
    OpenEntry(usize),
    SetShowAllSystem(bool),
    SetQuery(String),
    /// Emitted by PR-5 (header click → toggle sort). PR-4 wires the variant
    /// but never produces it.
    #[allow(dead_code)]
    SetSort(SortKey),
}

const FILTER_MEMORY_ID: &str = "port_scanner.filter";

fn read_filter_state(ctx: &egui::Context) -> FilterState {
    ctx.memory(|mem| {
        mem.data
            .get_temp::<FilterState>(egui::Id::new(FILTER_MEMORY_ID))
            .unwrap_or_default()
    })
}

fn write_filter_state(ctx: &egui::Context, filter: FilterState) {
    ctx.memory_mut(|mem| {
        mem.data
            .insert_temp(egui::Id::new(FILTER_MEMORY_ID), filter);
    });
}

pub fn draw_port_scanner_popup(
    ui: &mut egui::Ui,
    state: &mut AppState,
    engine: &mut CoreState,
) -> PopupAction {
    let th = theme::theme();
    let ctx = ui.ctx().clone();

    let mut filter_state = read_filter_state(&ctx);
    let target_show_all_system = filter_state.show_all_system;

    // ④ 매 프레임 poll: Loading → Ready/Failed.
    poll_scan(state);

    // ① 첫 open (Idle), ② scope 변경 (Ready{scope} ≠ target) → kick_off_scan.
    let need_kick = match &state.port_scan {
        PortScanState::Idle => true,
        PortScanState::Ready { scope, .. } => *scope != scope_from_flag(target_show_all_system),
        _ => false,
    };
    if need_kick {
        kick_off_scan(state, engine, &ctx, target_show_all_system);
    }

    // 검색 필터링: Ready rows 에 대해서만 적용.
    let filtered_rows: Vec<PortRowView> = match &state.port_scan {
        PortScanState::Ready { rows, .. } => rows
            .iter()
            .filter(|r| matches_query(r, &filter_state.query))
            .cloned()
            .collect(),
        _ => Vec::new(),
    };

    let view_state = match &state.port_scan {
        PortScanState::Idle | PortScanState::Loading { .. } => PortScannerViewState::Loading,
        PortScanState::Ready { .. } => PortScannerViewState::Ready {
            rows: &filtered_rows,
        },
        PortScanState::Failed(msg) => PortScannerViewState::Failed { message: msg },
    };

    let props = PortScannerProps {
        theme: &th,
        view_state,
        filter: PortScannerFilter {
            show_all_system: filter_state.show_all_system,
            query: &filter_state.query,
            sort_key: filter_state.sort_key,
            sort_dir: filter_state.sort_dir,
        },
        label_heading: t("port_scanner.heading"),
        label_search_placeholder: t("port_scanner.search_placeholder"),
        label_filter_show_all_system: t("port_scanner.filter_show_all_system"),
        label_loading: t("port_scanner.loading"),
        label_failed: t("port_scanner.failed_label"),
        label_close: t("port_scanner.close"),
        label_refresh: t("port_scanner.refresh"),
        label_external_dash: t("port_scanner.external_dash"),
        label_no_ports_tasty_empty: t("port_scanner.no_ports_tasty_empty"),
        label_no_ports_system_empty: t("port_scanner.no_ports_system_empty"),
        label_no_ports_search_zero: t("port_scanner.no_ports_search_zero"),
        label_footer_loading: t("port_scanner.footer_loading"),
        label_header_tag_scanning: t("port_scanner.header_tag_scanning"),
        label_header_tag_count: t("port_scanner.header_tag_count"),
        label_footer_counter: t("port_scanner.footer_counter"),
        label_column_port: t("port_scanner.column_port"),
        label_column_proto: t("port_scanner.column_proto"),
        label_column_address: t("port_scanner.column_address"),
        label_column_process: t("port_scanner.column_process"),
        label_column_workspace: t("port_scanner.column_workspace"),
        label_column_tab: t("port_scanner.column_tab"),
        label_column_state: t("port_scanner.column_state"),
    };

    let action = draw_port_scanner_view(ui, &props);

    match action {
        PortScannerAction::None => PopupAction::None,
        PortScannerAction::Close => {
            // ⑦ close → Idle reset. 백그라운드 thread 의 rx 가 drop 되어 send 가 실패할 뿐.
            state.port_scan = PortScanState::Idle;
            PopupAction::Close
        }
        PortScannerAction::Refresh => {
            // ③ Refresh 클릭: 현재 scope 그대로 재 kick.
            kick_off_scan(state, engine, &ctx, target_show_all_system);
            PopupAction::None
        }
        PortScannerAction::OpenEntry(i) => {
            if let Some(row) = filtered_rows.get(i) {
                open_in_browser(row);
            }
            PopupAction::None
        }
        PortScannerAction::SetShowAllSystem(v) => {
            filter_state.show_all_system = v;
            write_filter_state(&ctx, filter_state);
            PopupAction::None
        }
        PortScannerAction::SetQuery(q) => {
            filter_state.query = q;
            write_filter_state(&ctx, filter_state);
            PopupAction::None
        }
        PortScannerAction::SetSort(key) => {
            // PR-5 가 emit. PR-4 에서는 wrapper 도 명시적으로 처리하지 않으나
            // pattern exhaustive 를 위해 memory 만 갱신해 둔다.
            if filter_state.sort_key == key {
                filter_state.sort_dir = match filter_state.sort_dir {
                    SortDir::Asc => SortDir::Desc,
                    SortDir::Desc => SortDir::Asc,
                };
            } else {
                filter_state.sort_key = key;
                filter_state.sort_dir = SortDir::Asc;
            }
            write_filter_state(&ctx, filter_state);
            PopupAction::None
        }
    }
}

/// Case-insensitive substring match across every visible column. Empty query
/// matches everything.
pub fn matches_query(row: &PortRowView, query: &str) -> bool {
    let q = query.trim();
    if q.is_empty() {
        return true;
    }
    let q_lower = q.to_lowercase();
    let port_str = row.port.to_string();
    let pid_str = row.pid.map(|p| p.to_string()).unwrap_or_default();
    let process = row.process_name.as_deref().unwrap_or("");
    let (ws, tab) = match &row.source {
        SourceTag::Tasty {
            workspace_name,
            tab_name,
        } => (workspace_name.as_str(), tab_name.as_deref().unwrap_or("")),
        SourceTag::External => ("", ""),
    };
    let haystacks: [&str; 6] = [
        port_str.as_str(),
        row.addr_display.as_str(),
        pid_str.as_str(),
        process,
        ws,
        tab,
    ];
    haystacks
        .iter()
        .any(|h| h.to_lowercase().contains(&q_lower))
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

    let mut action = PortScannerAction::None;

    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = 6.0;
        if let Some(a) = draw_header_row(ui, props) {
            action = a;
        }
        if let Some(a) = draw_filter_row(ui, props) {
            action = a;
        }
        ui.separator();
        match &props.view_state {
            PortScannerViewState::Loading => draw_loading_body(ui, props),
            PortScannerViewState::Failed { message } => {
                if let Some(a) = draw_failed_body(ui, props, message) {
                    action = a;
                }
            }
            PortScannerViewState::Ready { rows } => {
                if let Some(a) = draw_ready_body(ui, props, rows) {
                    action = a;
                }
            }
        }
        draw_footer(ui, props);
    });

    action
}

fn draw_footer(ui: &mut egui::Ui, props: &PortScannerProps<'_>) {
    let th = props.theme;
    let text = match &props.view_state {
        PortScannerViewState::Loading => Some(props.label_footer_loading.to_string()),
        PortScannerViewState::Ready { rows } => Some(
            props
                .label_footer_counter
                .replace("{n}", &rows.len().to_string()),
        ),
        PortScannerViewState::Failed { .. } => None,
    };
    if let Some(s) = text {
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(
                egui::RichText::new(s)
                    .color(th.overlay0)
                    .size(th.font_size_caption.value()),
            );
        });
    }
}

/// 1줄 헤더: 좌측 heading + 우측 search TextEdit + 우측 끝 close 버튼.
fn draw_header_row(ui: &mut egui::Ui, props: &PortScannerProps<'_>) -> Option<PortScannerAction> {
    let th = props.theme;
    let mut out: Option<PortScannerAction> = None;
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new(props.label_heading)
                .color(th.text)
                .size(th.font_size_heading.value())
                .strong(),
        );
        // 우측에 close 버튼 + search.
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui
                .button(egui::RichText::new("×").size(16.0).color(th.text))
                .on_hover_text(props.label_close)
                .clicked()
            {
                out = Some(PortScannerAction::Close);
            }
            let avail = ui.available_width().max(120.0);
            let mut buf = props.filter.query.to_string();
            let resp = ui.add_sized(
                egui::vec2(avail, 22.0),
                egui::TextEdit::singleline(&mut buf)
                    .hint_text(props.label_search_placeholder)
                    .desired_width(avail),
            );
            if resp.changed() && buf != props.filter.query {
                out = Some(PortScannerAction::SetQuery(buf));
            }
        });
    });
    out
}

/// 2줄 헤더: "전체 보기" 체크박스 + 헤더 카운터 Tag.
fn draw_filter_row(ui: &mut egui::Ui, props: &PortScannerProps<'_>) -> Option<PortScannerAction> {
    let th = props.theme;
    let mut out: Option<PortScannerAction> = None;
    ui.horizontal(|ui| {
        let mut checked = props.filter.show_all_system;
        if ui
            .checkbox(&mut checked, props.label_filter_show_all_system)
            .changed()
        {
            out = Some(PortScannerAction::SetShowAllSystem(checked));
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let tag = match &props.view_state {
                PortScannerViewState::Loading => props.label_header_tag_scanning.to_string(),
                PortScannerViewState::Ready { rows } => props
                    .label_header_tag_count
                    .replace("{n}", &rows.len().to_string()),
                PortScannerViewState::Failed { .. } => String::new(),
            };
            if !tag.is_empty() {
                ui.label(
                    egui::RichText::new(tag)
                        .color(th.subtext0)
                        .size(th.font_size_caption.value()),
                );
            }
        });
    });
    out
}

/// 콘텐츠 중앙 horizontal: Spinner + "Collecting…" 텍스트.
fn draw_loading_body(ui: &mut egui::Ui, props: &PortScannerProps<'_>) {
    let th = props.theme;
    ui.vertical_centered(|ui| {
        ui.add_space(48.0);
        ui.horizontal(|ui| {
            ui.add(egui::Spinner::new().size(16.0).color(th.subtext0));
            ui.label(
                egui::RichText::new(props.label_loading)
                    .color(th.subtext0)
                    .size(th.font_size_body.value()),
            );
        });
    });
}

/// Failed: 에러 메시지 + Refresh 버튼.
fn draw_failed_body(
    ui: &mut egui::Ui,
    props: &PortScannerProps<'_>,
    message: &str,
) -> Option<PortScannerAction> {
    let th = props.theme;
    let mut out: Option<PortScannerAction> = None;
    ui.vertical_centered(|ui| {
        ui.add_space(32.0);
        ui.label(
            egui::RichText::new(props.label_failed)
                .color(th.red)
                .size(th.font_size_body.value())
                .strong(),
        );
        ui.label(
            egui::RichText::new(message)
                .color(th.subtext0)
                .size(th.font_size_caption.value()),
        );
        ui.add_space(8.0);
        if ui.button(props.label_refresh).clicked() {
            out = Some(PortScannerAction::Refresh);
        }
    });
    out
}

/// Ready: 빈 결과 분기 3종 OR TableBuilder 7컬럼.
fn draw_ready_body(
    ui: &mut egui::Ui,
    props: &PortScannerProps<'_>,
    rows: &[PortRowView],
) -> Option<PortScannerAction> {
    let th = props.theme;
    if rows.is_empty() {
        let empty_label = if !props.filter.query.trim().is_empty() {
            props.label_no_ports_search_zero
        } else if props.filter.show_all_system {
            props.label_no_ports_system_empty
        } else {
            props.label_no_ports_tasty_empty
        };
        ui.vertical_centered(|ui| {
            ui.add_space(40.0);
            ui.label(
                egui::RichText::new(empty_label)
                    .color(th.subtext0)
                    .italics()
                    .size(th.font_size_body.value()),
            );
        });
        return None;
    }
    draw_table(ui, props, rows)
}

/// 7컬럼 TableBuilder. 컬럼 폭은 디자이너 확정값.
fn draw_table(
    ui: &mut egui::Ui,
    props: &PortScannerProps<'_>,
    rows: &[PortRowView],
) -> Option<PortScannerAction> {
    use egui_extras::{Column, TableBuilder};
    let th = props.theme;
    let mut out: Option<PortScannerAction> = None;
    let text_h = th.font_size_body.value() + 6.0;

    TableBuilder::new(ui)
        .striped(false)
        .resizable(false)
        .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
        .column(Column::initial(84.0).at_least(60.0))
        .column(Column::initial(76.0).at_least(60.0))
        .column(Column::remainder().at_least(80.0))
        .column(Column::remainder().at_least(100.0))
        .column(Column::initial(120.0).at_least(80.0))
        .column(Column::remainder().at_least(80.0))
        .column(Column::initial(140.0).at_least(100.0))
        .header(text_h + 4.0, |mut header| {
            header.col(|ui| {
                draw_header_cell(ui, th, props.label_column_port);
            });
            header.col(|ui| {
                draw_header_cell(ui, th, props.label_column_proto);
            });
            header.col(|ui| {
                draw_header_cell(ui, th, props.label_column_address);
            });
            header.col(|ui| {
                draw_header_cell(ui, th, props.label_column_process);
            });
            header.col(|ui| {
                draw_header_cell(ui, th, props.label_column_workspace);
            });
            header.col(|ui| {
                draw_header_cell(ui, th, props.label_column_tab);
            });
            header.col(|ui| {
                draw_header_cell(ui, th, props.label_column_state);
            });
        })
        .body(|mut body| {
            for (i, row) in rows.iter().enumerate() {
                body.row(text_h + 8.0, |mut tr| {
                    tr.col(|ui| {
                        let resp = ui.add(
                            egui::Label::new(
                                egui::RichText::new(row.port.to_string())
                                    .color(th.text)
                                    .size(th.font_size_body.value()),
                            )
                            .sense(egui::Sense::click()),
                        );
                        if resp.clicked() {
                            out = Some(PortScannerAction::OpenEntry(i));
                        }
                    });
                    tr.col(|ui| {
                        ui.label(
                            egui::RichText::new("TCP")
                                .color(th.subtext0)
                                .size(th.font_size_body.value()),
                        );
                    });
                    tr.col(|ui| {
                        ui.label(
                            egui::RichText::new(&row.addr_display)
                                .color(th.subtext0)
                                .size(th.font_size_body.value())
                                .monospace(),
                        );
                    });
                    tr.col(|ui| {
                        draw_process_cell(ui, th, row);
                    });
                    tr.col(|ui| {
                        draw_workspace_cell(ui, th, row, props.label_external_dash);
                    });
                    tr.col(|ui| {
                        draw_tab_cell(ui, th, row, props.label_external_dash);
                    });
                    tr.col(|ui| {
                        draw_state_cell(ui, th);
                    });
                });
            }
        });
    out
}

fn draw_header_cell(ui: &mut egui::Ui, th: &Theme, label: &str) {
    ui.label(
        egui::RichText::new(label)
            .color(th.subtext0)
            .size(th.font_size_caption.value())
            .strong(),
    );
}

/// Process 셀: process_name + PID 배지.
fn draw_process_cell(ui: &mut egui::Ui, th: &Theme, row: &PortRowView) {
    ui.horizontal(|ui| {
        let name = row.process_name.as_deref().unwrap_or("—");
        ui.label(
            egui::RichText::new(name)
                .color(th.text)
                .size(th.font_size_body.value()),
        );
        if let Some(pid) = row.pid {
            let txt = format!("PID {pid}");
            let badge = egui::RichText::new(txt)
                .color(th.subtext0)
                .size(th.font_size_caption.value())
                .monospace();
            egui::Frame::default()
                .fill(th.surface1.into())
                .corner_radius(egui::CornerRadius::same(3))
                .inner_margin(egui::Margin::symmetric(4, 1))
                .show(ui, |ui| {
                    ui.label(badge);
                });
        }
    });
}

/// Workspace 셀: Tasty → workspace_name, External → dash.
fn draw_workspace_cell(ui: &mut egui::Ui, th: &Theme, row: &PortRowView, dash: &str) {
    match &row.source {
        SourceTag::Tasty { workspace_name, .. } => {
            ui.label(
                egui::RichText::new(workspace_name)
                    .color(th.text)
                    .size(th.font_size_body.value()),
            );
        }
        SourceTag::External => {
            ui.colored_label(th.subtext0, dash);
        }
    }
}

/// Tab 셀: Tasty → tab_name (Some), 없거나 External → dash.
fn draw_tab_cell(ui: &mut egui::Ui, th: &Theme, row: &PortRowView, dash: &str) {
    let tab_name = match &row.source {
        SourceTag::Tasty {
            tab_name: Some(t), ..
        } => Some(t.as_str()),
        _ => None,
    };
    match tab_name {
        Some(name) => {
            ui.label(
                egui::RichText::new(name)
                    .color(th.subtext0)
                    .size(th.font_size_body.value()),
            );
        }
        None => {
            ui.colored_label(th.subtext0, dash);
        }
    }
}

/// State 셀: 초록 dot + "LISTEN".
fn draw_state_cell(ui: &mut egui::Ui, th: &Theme) {
    ui.horizontal(|ui| {
        let (rect, _) = ui.allocate_exact_size(egui::vec2(8.0, 8.0), egui::Sense::hover());
        ui.painter().circle_filled(rect.center(), 4.0, th.green);
        ui.label(
            egui::RichText::new("LISTEN")
                .color(th.subtext0)
                .size(th.font_size_caption.value()),
        );
    });
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

    fn dummy_row(port: u16) -> PortRowView {
        PortRowView {
            port,
            addr_display: "127.0.0.1".into(),
            pid: Some(42),
            process_name: None,
            source: SourceTag::External,
        }
    }

    fn default_props<'a>(
        theme: &'a Theme,
        view_state: PortScannerViewState<'a>,
        query: &'a str,
        show_all_system: bool,
    ) -> PortScannerProps<'a> {
        PortScannerProps {
            theme,
            view_state,
            filter: PortScannerFilter {
                show_all_system,
                query,
                sort_key: SortKey::Port,
                sort_dir: SortDir::Asc,
            },
            label_heading: "Listening ports",
            label_search_placeholder: "Search…",
            label_filter_show_all_system: "Show all system ports",
            label_loading: "Scanning…",
            label_failed: "Scan failed",
            label_close: "Close",
            label_refresh: "Refresh",
            label_external_dash: "—",
            label_no_ports_tasty_empty: "No Tasty ports.",
            label_no_ports_system_empty: "No system ports.",
            label_no_ports_search_zero: "No matches.",
            label_footer_loading: "Scanning…",
            label_header_tag_scanning: "scanning…",
            label_header_tag_count: "{n} listening",
            label_footer_counter: "{n} listening",
            label_column_port: "Port",
            label_column_proto: "Proto",
            label_column_address: "Address",
            label_column_process: "Process",
            label_column_workspace: "Workspace",
            label_column_tab: "Tab",
            label_column_state: "State",
        }
    }

    fn run_view(raw: egui::RawInput, rows: &[PortRowView]) -> PortScannerAction {
        let ctx = egui::Context::default();
        let mut out = PortScannerAction::None;
        let theme = test_theme();
        drop(ctx.run(raw, |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                let view_state = PortScannerViewState::Ready { rows };
                let props = default_props(&theme, view_state, "", false);
                out = draw_port_scanner_view(ui, &props);
            });
        }));
        out
    }

    #[test]
    fn view_returns_none_on_empty_input() {
        let action = run_view(egui::RawInput::default(), &[]);
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
        let action = run_view(raw, &[]);
        assert_eq!(action, PortScannerAction::Close);
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
        let rows = vec![dummy_row(3000), dummy_row(8080)];
        let action = run_view(egui::RawInput::default(), &rows);
        assert_eq!(action, PortScannerAction::None);
    }

    fn tasty_row(port: u16, ws: &str, tab: Option<&str>) -> PortRowView {
        PortRowView {
            port,
            addr_display: "0.0.0.0".into(),
            pid: Some(7),
            process_name: Some("node".into()),
            source: SourceTag::Tasty {
                workspace_name: ws.to_string(),
                tab_name: tab.map(str::to_string),
            },
        }
    }

    #[test]
    fn query_filter_matches_any_column_case_insensitive() {
        let row = tasty_row(3000, "frontend", Some("dev-server"));
        // port number
        assert!(matches_query(&row, "3000"));
        // address (case insensitive)
        assert!(matches_query(&row, "0.0.0.0"));
        // process name (different case)
        assert!(matches_query(&row, "NODE"));
        // workspace
        assert!(matches_query(&row, "frontend"));
        // tab
        assert!(matches_query(&row, "dev-server"));
        // pid
        assert!(matches_query(&row, "7"));
        // no match
        assert!(!matches_query(&row, "xyzzy"));
    }

    #[test]
    fn query_filter_empty_matches_everything() {
        let row = tasty_row(3000, "ws", Some("tab"));
        assert!(matches_query(&row, ""));
        assert!(matches_query(&row, "   "));
    }

    #[test]
    fn query_filter_external_row_skips_workspace_match() {
        let row = dummy_row(80);
        // External rows have no workspace/tab strings to match.
        assert!(!matches_query(&row, "workspace"));
        // But port/addr still match.
        assert!(matches_query(&row, "80"));
        assert!(matches_query(&row, "127.0.0.1"));
    }

    fn run_view_state(
        view_state: PortScannerViewState<'_>,
        query: &str,
        show_all_system: bool,
    ) -> PortScannerAction {
        let ctx = egui::Context::default();
        let mut out = PortScannerAction::None;
        let theme = test_theme();
        drop(ctx.run(egui::RawInput::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                let props = default_props(&theme, view_state, query, show_all_system);
                out = draw_port_scanner_view(ui, &props);
            });
        }));
        out
    }

    #[test]
    fn empty_ready_renders_tasty_empty_message() {
        let rows: Vec<PortRowView> = Vec::new();
        let action = run_view_state(PortScannerViewState::Ready { rows: &rows }, "", false);
        assert_eq!(action, PortScannerAction::None);
    }

    #[test]
    fn empty_ready_renders_system_empty_message() {
        let rows: Vec<PortRowView> = Vec::new();
        let action = run_view_state(PortScannerViewState::Ready { rows: &rows }, "", true);
        assert_eq!(action, PortScannerAction::None);
    }

    #[test]
    fn empty_ready_with_query_renders_search_zero_message() {
        let rows: Vec<PortRowView> = Vec::new();
        let action = run_view_state(PortScannerViewState::Ready { rows: &rows }, "nginx", false);
        assert_eq!(action, PortScannerAction::None);
    }

    #[test]
    fn loading_state_renders_without_panic() {
        let action = run_view_state(PortScannerViewState::Loading, "", false);
        assert_eq!(action, PortScannerAction::None);
    }

    #[test]
    fn failed_state_renders_without_panic() {
        let action = run_view_state(PortScannerViewState::Failed { message: "boom" }, "", false);
        assert_eq!(action, PortScannerAction::None);
    }
}
