//! Listening-port viewer popup.
//!
//! Lists TCP ports that the active surface's process tree is listening on.
//! Clicking a row selects it (re-clicking deselects); the footer's
//! "Copy address" button copies the selected row's address to the clipboard.
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
use tasty_type_geometry::length::LogicalPx;

use crate::adapters::ui::icons;
use crate::adapters::ui::popup::PopupAction;
use crate::adapters::ui::popup::port_scanner_favorites::PortFavorites;
use crate::core::CoreState;
use crate::core::state::SurfaceDisplayPath;
use crate::i18n::t;
use crate::state::AppState;
use crate::theme;
use crate::theme::Theme;
use tasty_portscan::PortState;
use tasty_ui_widgets::tokens::{STRUCT_GAP_1, STRUCT_GAP_2, STRUCT_GAP_4, TAG_PILL_CORNER_RADIUS};
use tasty_ui_widgets::{
    Button, ButtonVariant, IconButton, IconButtonVariant, Input, StatusKind, Table, TableAlign,
    TableColumn, TableColumnWidth, TableSortDir, TagVariant, checkbox, hspace, margin_sym,
    status_dot, tag, vspace,
};

/// popup 좌우 안쪽 여백. 디자인 전사값 14 로 4px 그리드 밖이다(가장 가까운
/// `spacing_md`=12 와 2px 차) — 헤더·필터·리스트·푸터가 같은 세로선에 서야 해서
/// 한 값을 공유한다. `egui::Margin` 필드가 `i8` 이라 타입을 맞춰 둔다.
const PANEL_PAD_X: i8 = 14;
/// 푸터 상하 여백. 디자인 전사값 9 로 그리드 밖이다(`spacing_sm`=8 과 1px 차).
const FOOTER_PAD_Y: i8 = 9;

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
    /// TCP connection state, drives the STATE column dot color/pulse + label.
    pub state: PortState,
    /// Whether `(addr_display, port)` is in `engine.port_favorites`. Filled by
    /// the wrapper after the background scan returns (`run_scan` has no
    /// `CoreState` access) — always `false` fresh off the scan thread.
    pub favorited: bool,
}

/// One row of the always-visible favorites section — a pinned `(addr, port)`
/// paired with its current system-wide observation (`None` = not currently
/// listening/connected anywhere on the host, drawn as the `NONE` state).
#[derive(Clone, Debug)]
pub struct FavoriteRowView {
    pub addr_display: String,
    pub port: u16,
    pub matched: Option<FavoriteMatch>,
}

/// The system-wide scan row a favorite currently matches, projected down to
/// what the favorites row needs (full `PortRowView` carries scope/source
/// details the summary row doesn't show).
#[derive(Clone, Debug)]
pub struct FavoriteMatch {
    pub process_name: Option<String>,
    pub pid: Option<u32>,
    /// `Some` only when the matched socket belongs to a Tasty process tree.
    pub workspace_name: Option<String>,
    pub state: PortState,
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

/// One of the seven table columns. Drives column visibility (chooser) and width
/// computation. The order here is the table's left-to-right order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ColumnId {
    Port,
    Proto,
    Address,
    Process,
    Workspace,
    Tab,
    State,
}

impl ColumnId {
    /// Left-to-right table order. Also the bit order of [`ColumnVisibility`].
    pub const ALL: [ColumnId; 7] = [
        ColumnId::Port,
        ColumnId::Proto,
        ColumnId::Address,
        ColumnId::Process,
        ColumnId::Workspace,
        ColumnId::Tab,
        ColumnId::State,
    ];

    /// Bit index in [`ColumnVisibility`].
    fn bit(self) -> u8 {
        match self {
            ColumnId::Port => 0,
            ColumnId::Proto => 1,
            ColumnId::Address => 2,
            ColumnId::Process => 3,
            ColumnId::Workspace => 4,
            ColumnId::Tab => 5,
            ColumnId::State => 6,
        }
    }

    /// Port is the identity / default sort column and is always shown (its
    /// chooser checkbox is locked). This also guarantees ≥1 visible column, so
    /// no extra "can't hide the last column" rule is needed.
    fn mandatory(self) -> bool {
        matches!(self, ColumnId::Port)
    }
}

/// Per-column show/hide state, persisted in [`FilterState`]. Bit `i` set =
/// column `ColumnId::ALL[i]` visible. `Default` shows every column.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ColumnVisibility(u8);

impl Default for ColumnVisibility {
    fn default() -> Self {
        // All seven low bits set → every column visible.
        Self(0b0111_1111)
    }
}

impl ColumnVisibility {
    /// Whether `col` is currently shown. Mandatory columns are always shown.
    pub fn is_visible(self, col: ColumnId) -> bool {
        col.mandatory() || (self.0 & (1 << col.bit())) != 0
    }

    /// Show/hide `col`. Mandatory columns ignore a hide request (stay shown).
    pub fn set(&mut self, col: ColumnId, visible: bool) {
        if col.mandatory() {
            self.0 |= 1 << col.bit();
            return;
        }
        if visible {
            self.0 |= 1 << col.bit();
        } else {
            self.0 &= !(1 << col.bit());
        }
    }
}

/// User-controlled view state, persisted in `egui::Memory` across frames
/// (Id: `"port_scanner.filter"`).
#[derive(Clone, Debug, PartialEq)]
pub struct FilterState {
    pub show_all_system: bool,
    pub query: String,
    pub sort_key: SortKey,
    pub sort_dir: SortDir,
    /// Port number of the currently selected row (design `selectedKey`), or
    /// `None` when nothing is selected. Drives the row highlight + footer
    /// "Copy address" enablement.
    pub selected_port: Option<u16>,
    /// Per-column show/hide state (column chooser). Default shows all columns.
    pub columns: ColumnVisibility,
    /// TCP states to **show** in the table (a shown set, not a hidden set).
    /// Default is `{Listen}` (LISTEN-only) — the backend scans every TCP state,
    /// but the viewer surfaces only listening sockets until the user widens it.
    /// A shown set keeps the default a single entry and stays robust as new
    /// `PortState` variants are added (a hidden set would have to enumerate all
    /// the others).
    pub visible_states: HashSet<PortState>,
}

impl Default for FilterState {
    fn default() -> Self {
        Self {
            show_all_system: false,
            query: String::new(),
            sort_key: SortKey::Port,
            sort_dir: SortDir::Asc,
            selected_port: None,
            columns: ColumnVisibility::default(),
            visible_states: HashSet::from([PortState::Listen]),
        }
    }
}

/// Async view payload the table renders from. `Loading`/`Failed` short-circuit
/// the table; `Ready` carries the (search-filtered) row slice plus two
/// search-independent scope counts so the header tag and footer counter can be
/// drawn per design: footer `{shown} of {total} ports`, header `{listening}
/// listening` (LISTEN-only, since the backend now scans every TCP state).
#[derive(Clone, Copy)]
pub enum PortScannerViewState<'a> {
    Loading,
    Ready {
        rows: &'a [PortRowView],
        /// Count of all ports in the current scope, before the search filter is
        /// applied. Feeds the footer total.
        total: usize,
        /// Count of LISTEN-state ports in the current scope, before the search
        /// filter. Feeds the header tag (`{listening} listening`).
        listening: usize,
    },
    Failed {
        message: &'a str,
    },
}

/// Filter inputs handed to the view (read-only projection of [`FilterState`]).
pub struct PortScannerFilter<'a> {
    pub show_all_system: bool,
    pub query: &'a str,
    pub sort_key: SortKey,
    pub sort_dir: SortDir,
    /// Currently selected row's port (design `selectedKey`). `None` = no
    /// selection.
    pub selected_port: Option<u16>,
    /// Per-column show/hide state (column chooser projection).
    pub columns: ColumnVisibility,
    /// TCP states currently shown (shown set; default `{Listen}`). Drives the
    /// state-filter button's active styling + the dropdown checkbox seeding.
    pub visible_states: &'a HashSet<PortState>,
    /// True when the scope has rows but the state filter (not the search query)
    /// hid them all. Lets the empty body distinguish "no LISTEN ports" from
    /// "no ports at all".
    pub hidden_by_state: bool,
}

/// Pure inputs to `draw_port_scanner_view`. Contains no `AppState` /
/// `CoreState` — every value is read-only. All user-facing strings are
/// pre-resolved by the wrapper so the view is i18n-agnostic.
pub struct PortScannerProps<'a> {
    pub theme: &'a Theme,
    pub view_state: PortScannerViewState<'a>,
    pub filter: PortScannerFilter<'a>,
    /// Pinned favorites, rendered by the always-visible section above the
    /// table regardless of the table's scope/search/state filter. LISTEN/NONE
    /// judgment here is system-wide (see `matched` on each row).
    pub favorites: &'a [FavoriteRowView],
    /// TCP states present in the current scope (before any filter), for the
    /// state-filter dropdown list + select-all / apply intersection. LISTEN
    /// first, then `label()` alphabetical.
    pub present_states: &'a [PortState],
    /// When true, the LISTEN state dot is drawn static (no pulse ring).
    pub reduced_motion: bool,
    pub label_heading: &'a str,
    pub label_search_placeholder: &'a str,
    pub label_filter_show_all_system: &'a str,
    pub label_loading: &'a str,
    pub label_failed: &'a str,
    pub label_close: &'a str,
    pub label_refresh: &'a str,
    pub label_copy_address: &'a str,
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
    /// Column-chooser trigger tooltip (header IconButton).
    pub label_columns_button: &'a str,
    /// Column-chooser popup title.
    pub label_columns_menu_title: &'a str,
    /// State-filter button base label (e.g. `State`).
    pub label_state_filter: &'a str,
    /// State-filter dropdown title (e.g. `Filter by state`).
    pub label_state_filter_title: &'a str,
    /// State-filter dropdown "select all present states" action.
    pub label_state_filter_select_all: &'a str,
    /// State-filter dropdown "deselect all" action.
    pub label_state_filter_deselect_all: &'a str,
    /// State-filter dropdown "reset to LISTEN-only" action.
    pub label_state_filter_reset: &'a str,
    /// State-filter dropdown "apply draft" action.
    pub label_state_filter_apply: &'a str,
    /// Empty-body message when the state filter hid every scope row.
    pub label_no_ports_state_filtered: &'a str,
    /// Favorites section caption when the list is empty (no count suffix).
    pub label_favorites_heading: &'a str,
    /// Favorites section caption format string with `{n}`, e.g. `"Favorites · {n}"`.
    pub label_favorites_count: &'a str,
    /// Favorites section caption's right-aligned "system-wide" hint.
    pub label_favorites_system_wide: &'a str,
    /// Favorites section empty-state hint line.
    pub label_favorites_empty: &'a str,
    /// Favorites row detail text when the target has no system-wide match.
    pub label_favorites_not_running: &'a str,
    /// Status label for a favorite with no system-wide match (`StatusKind::Idle`).
    pub label_state_none: &'a str,
    /// Star tooltip format string with `{key}` (the `addr:port` string), shown
    /// when the row is not yet favorited.
    pub label_favorite_add: &'a str,
    /// Star tooltip format string with `{key}`, shown when the row is already
    /// favorited (click removes it).
    pub label_favorite_remove: &'a str,
}

/// User intent surfaced by the view. The wrapper translates these into
/// state mutation + side effects (browser launch, scan kick-off).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PortScannerAction {
    None,
    Close,
    Refresh,
    /// Row clicked → toggle selection of the given port (design `onRowClick`).
    Select(u16),
    /// Footer "Copy address" clicked → copy this address string to clipboard.
    CopyAddress(String),
    SetShowAllSystem(bool),
    SetQuery(String),
    /// Emitted by header click → toggle sort.
    SetSort(SortKey),
    /// Column chooser toggled a column's visibility.
    SetColumnVisible(ColumnId, bool),
    /// State filter Apply → replace the shown TCP-state set.
    SetVisibleStates(HashSet<PortState>),
    /// Star icon clicked (main table row or favorites section) → toggle
    /// favorite status for this `(addr, port)`. `addr` is the row's
    /// display-form address string (round-trips through `str::parse`).
    ToggleFavorite(String, u16),
}

const FILTER_MEMORY_ID: &str = "port_scanner.filter";
/// egui temp-memory id for the state-filter dropdown's editing buffer
/// (apply-on-confirm draft). Seeded from `visible_states` when the dropdown
/// opens; committed via `SetVisibleStates` on Apply.
const STATE_FILTER_DRAFT_ID: &str = "port_scanner.state_filter_draft";
/// egui popup id for the state-filter dropdown. Used both to open/toggle the
/// `popup_above_or_below_widget` and for the Escape guard's open check.
const STATE_FILTER_POPUP_ID: &str = "port_scanner.state_filter_popup";
/// egui popup id for the column-chooser dropdown (`draw_column_chooser`).
const COLUMN_CHOOSER_POPUP_ID: &str = "port_scanner.columns_chooser";

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
        kick_off_scan(&mut state.port_scan, engine, &ctx, target_show_all_system);
    }

    // 즐겨찾기 판정은 항상 system-wide 라 메인 scope(Tasty/System) 와 무관한 별도 스캔이
    // 필요하다(direction (a) — 즐겨찾기가 있을 때만 병행 실행). 메인 scope 토글에는
    // 반응하지 않고, 즐겨찾기가 비면 재스캔하지 않는다(리소스 절약).
    let has_favorites = !engine.port_favorites.items.is_empty();
    if has_favorites && matches!(state.port_favorites_scan, PortScanState::Idle) {
        kick_off_scan(&mut state.port_favorites_scan, engine, &ctx, true);
    }

    // 상태 필터 + 검색 + 정렬: Ready rows 에 대해서만 적용. 상태 필터를 검색보다 **먼저**
    // 적용해 footer total 을 "상태 통과·검색 전" 기준으로 잡는다. 정렬은 wrapper 에서
    // 적용해 view 가 보이는 순서 그대로 행을 렌더(선택 키는 port 번호라 정렬과 무관).
    let (filtered_rows, state_total): (Vec<PortRowView>, usize) = match &state.port_scan {
        PortScanState::Ready { rows, .. } => {
            // 1) 상태 필터(검색 전) → total 기준.
            let state_rows: Vec<&PortRowView> = rows
                .iter()
                .filter(|r| filter_state.visible_states.contains(&r.state))
                .collect();
            let state_total = state_rows.len();
            // 2) 검색 + 정렬.
            let mut v: Vec<PortRowView> = state_rows
                .into_iter()
                .filter(|r| matches_query(r, &filter_state.query))
                .cloned()
                .collect();
            sort_rows(&mut v, filter_state.sort_key, filter_state.sort_dir);
            // 별 토글 표시 — background scan 은 CoreState 를 몰라 채우지 못한 필드를
            // wrapper 가 여기서 채운다.
            for row in &mut v {
                if let Ok(addr) = row.addr_display.parse::<IpAddr>() {
                    row.favorited = engine.port_favorites.contains(addr, row.port);
                }
            }
            (v, state_total)
        }
        _ => (Vec::new(), 0),
    };

    // 즐겨찾기 섹션 rows — engine.port_favorites 전체(메인 테이블의 scope/검색/상태
    // 필터와 무관) 에 system-wide 스캔 결과를 매칭한다.
    let favorite_system_rows: Option<&[PortRowView]> = match &state.port_favorites_scan {
        PortScanState::Ready { rows, .. } => Some(rows.as_slice()),
        _ => None,
    };
    let favorite_rows = build_favorite_rows(&engine.port_favorites, favorite_system_rows);

    // scope rows 에 존재하는 상태들(필터 전) — 드롭다운 목록 + 모두선택/적용의 교집합 대상.
    let present_states: Vec<PortState> = match &state.port_scan {
        PortScanState::Ready { rows, .. } => present_states(rows),
        _ => Vec::new(),
    };

    // 빈 상태 신호: scope 에 행은 있으나 상태 필터가 전부 걸러 비었고(검색은 빈) → "포트
    // 없음" 이 아니라 "상태 필터로 가려짐" 으로 안내한다.
    let hidden_by_state = match &state.port_scan {
        PortScanState::Ready { rows, .. } => {
            !rows.is_empty() && filtered_rows.is_empty() && filter_state.query.trim().is_empty()
        }
        _ => false,
    };

    let view_state = match &state.port_scan {
        PortScanState::Idle | PortScanState::Loading { .. } => PortScannerViewState::Loading,
        // Scope counts are search-independent: `total` = state-filtered count
        // (state pass, before search — per design), `listening` = scope-wide
        // LISTEN-only count for the header tag (independent of the state filter).
        PortScanState::Ready { rows, .. } => PortScannerViewState::Ready {
            rows: &filtered_rows,
            total: state_total,
            listening: rows.iter().filter(|r| r.state.is_listen()).count(),
        },
        PortScanState::Failed(msg) => PortScannerViewState::Failed { message: msg },
    };

    let props = PortScannerProps {
        theme: &th,
        view_state,
        present_states: &present_states,
        reduced_motion: engine.settings.accessibility.reduced_motion,
        favorites: &favorite_rows,
        filter: PortScannerFilter {
            show_all_system: filter_state.show_all_system,
            query: &filter_state.query,
            sort_key: filter_state.sort_key,
            sort_dir: filter_state.sort_dir,
            selected_port: filter_state.selected_port,
            columns: filter_state.columns,
            visible_states: &filter_state.visible_states,
            hidden_by_state,
        },
        label_heading: t("port_scanner.heading"),
        label_search_placeholder: t("port_scanner.search_placeholder"),
        label_filter_show_all_system: t("port_scanner.filter_show_all_system"),
        label_loading: t("port_scanner.loading"),
        label_failed: t("port_scanner.failed_label"),
        label_close: t("port_scanner.close"),
        label_refresh: t("port_scanner.refresh"),
        label_copy_address: t("port_scanner.copy_address"),
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
        label_columns_button: t("port_scanner.columns_button"),
        label_columns_menu_title: t("port_scanner.columns_menu_title"),
        label_state_filter: t("port_scanner.state_filter"),
        label_state_filter_title: t("port_scanner.state_filter_title"),
        label_state_filter_select_all: t("port_scanner.state_filter_select_all"),
        label_state_filter_deselect_all: t("port_scanner.state_filter_deselect_all"),
        label_state_filter_reset: t("port_scanner.state_filter_reset"),
        label_state_filter_apply: t("port_scanner.state_filter_apply"),
        label_no_ports_state_filtered: t("port_scanner.no_ports_state_filtered"),
        label_favorites_heading: t("port_scanner.favorites_heading"),
        label_favorites_count: t("port_scanner.favorites_count"),
        label_favorites_system_wide: t("port_scanner.favorites_system_wide"),
        label_favorites_empty: t("port_scanner.favorites_empty"),
        label_favorites_not_running: t("port_scanner.favorites_not_running"),
        label_state_none: t("port_scanner.state_none_label"),
        label_favorite_add: t("port_scanner.favorite_add_tooltip"),
        label_favorite_remove: t("port_scanner.favorite_remove_tooltip"),
    };

    let action = draw_port_scanner_view(ui, &props);

    match action {
        PortScannerAction::None => PopupAction::None,
        PortScannerAction::Close => {
            // ⑦ close → Idle reset. 백그라운드 thread 의 rx 가 drop 되어 send 가 실패할 뿐.
            state.port_scan = PortScanState::Idle;
            state.port_favorites_scan = PortScanState::Idle;
            PopupAction::Close
        }
        PortScannerAction::Refresh => {
            // ③ Refresh 클릭: 현재 scope 그대로 재 kick. 즐겨찾기가 있으면 system-wide
            // 판정용 스캔도 함께 재스캔한다.
            kick_off_scan(&mut state.port_scan, engine, &ctx, target_show_all_system);
            if has_favorites {
                kick_off_scan(&mut state.port_favorites_scan, engine, &ctx, true);
            }
            PopupAction::None
        }
        PortScannerAction::Select(port) => {
            // Toggle: re-clicking the selected row clears the selection.
            filter_state.selected_port = if filter_state.selected_port == Some(port) {
                None
            } else {
                Some(port)
            };
            write_filter_state(&ctx, filter_state);
            PopupAction::None
        }
        PortScannerAction::CopyAddress(addr) => {
            // egui 의 platform-output copy 명령으로 OS clipboard 에 복사
            // (`egui_winit` 의 `handle_platform_output` 가 기록).
            ui.ctx().copy_text(addr);
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
        PortScannerAction::SetColumnVisible(col, visible) => {
            // 컬럼 표시/숨김 토글 → 영속. 숨긴 컬럼이 활성 sort key 여도 정렬은 그대로
            // 유지한다(데이터 정렬은 표시와 독립 — 명세 권장). selected_port 도 보존.
            filter_state.columns.set(col, visible);
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
        PortScannerAction::SetVisibleStates(set) => {
            // 상태 필터 Apply → 표시할 상태 집합 교체. 영속(temp memory)되어 재오픈에도
            // 유지(LISTEN-only 기본은 휘발 시 복원).
            filter_state.visible_states = set;
            write_filter_state(&ctx, filter_state);
            PopupAction::None
        }
        PortScannerAction::ToggleFavorite(addr_display, port) => {
            // 별 클릭 → 즉시 토글, 확인 절차 없음. 라벨은 표시용 `addr:port` 로 채운다
            // (친숙한 이름을 입력받는 UI 가 없다 — 시안에 없음).
            if let Ok(addr) = addr_display.parse::<IpAddr>() {
                if engine.port_favorites.contains(addr, port) {
                    engine.port_favorites.remove(addr, port);
                } else {
                    let label = format_host_port(&addr_display, port);
                    engine.port_favorites.add(addr, port, label);
                }
                engine.port_favorites.save();
            }
            PopupAction::None
        }
    }
}

/// Sort rows in place by the given key + direction. `None` values are always
/// placed at the tail, regardless of direction (per design — None ≈ "no info,
/// don't surface").
pub fn sort_rows(rows: &mut [PortRowView], key: SortKey, dir: SortDir) {
    rows.sort_by(|a, b| compare_rows(key, dir, a, b));
}

/// Two-row comparator used by [`sort_rows`]. None-or-empty values for the
/// active key bubble to the tail regardless of `dir`.
pub fn compare_rows(
    key: SortKey,
    dir: SortDir,
    a: &PortRowView,
    b: &PortRowView,
) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    let (a_missing, b_missing) = (value_missing(a, key), value_missing(b, key));
    match (a_missing, b_missing) {
        (true, true) => return Ordering::Equal,
        (true, false) => return Ordering::Greater,
        (false, true) => return Ordering::Less,
        (false, false) => {}
    }
    let ord = match key {
        SortKey::Port => a.port.cmp(&b.port),
        SortKey::Address => a.addr_display.cmp(&b.addr_display),
        SortKey::Process => a
            .process_name
            .as_deref()
            .unwrap_or("")
            .cmp(b.process_name.as_deref().unwrap_or("")),
        SortKey::Workspace => workspace_name(a).cmp(workspace_name(b)),
        SortKey::Tab => tab_name(a).unwrap_or("").cmp(tab_name(b).unwrap_or("")),
    };
    match dir {
        SortDir::Asc => ord,
        SortDir::Desc => ord.reverse(),
    }
}

/// "Missing" for sort purposes: None pid / no process_name / External source
/// (for workspace/tab) / Tasty source with no tab_name (for tab only).
fn value_missing(row: &PortRowView, key: SortKey) -> bool {
    match key {
        SortKey::Port | SortKey::Address => false,
        SortKey::Process => row.process_name.is_none(),
        SortKey::Workspace => matches!(row.source, SourceTag::External),
        SortKey::Tab => tab_name(row).is_none(),
    }
}

fn workspace_name(row: &PortRowView) -> &str {
    match &row.source {
        SourceTag::Tasty { workspace_name, .. } => workspace_name.as_str(),
        SourceTag::External => "",
    }
}

fn tab_name(row: &PortRowView) -> Option<&str> {
    match &row.source {
        SourceTag::Tasty {
            tab_name: Some(t), ..
        } => Some(t.as_str()),
        _ => None,
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

/// TCP states actually present in the scope `rows` (before state/search
/// filtering). Display order: LISTEN first, then `label()` alphabetical.
/// Derived from the full scope so a toggled-off state can still be re-enabled
/// (a state filtered out of the *visible* rows would otherwise vanish from the
/// dropdown). Deduplicated.
pub fn present_states(rows: &[PortRowView]) -> Vec<PortState> {
    let mut seen: Vec<PortState> = Vec::new();
    for r in rows {
        if !seen.contains(&r.state) {
            seen.push(r.state);
        }
    }
    seen.sort_by_key(|s| (!s.is_listen(), s.label()));
    seen
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

/// Move `slot` into `Loading`, spawning a background thread that computes the
/// row set and reports back through an mpsc channel. The thread requests an
/// egui repaint after sending so the main loop wakes up. Shared by the main
/// `state.port_scan` (Tasty/System scope, user-driven) and
/// `state.port_favorites_scan` (always system-wide, favorites-driven) — both
/// are `PortScanState` slots with no other coupling.
pub fn kick_off_scan(
    slot: &mut PortScanState,
    engine: &CoreState,
    ctx: &egui::Context,
    show_all_system: bool,
) {
    let snapshot = build_snapshot(engine, show_all_system);
    let (tx, rx) = mpsc::channel::<Result<Vec<PortRowView>, String>>();
    let scope = scope_from_flag(show_all_system);
    *slot = PortScanState::Loading { rx, scope };
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
                    state: p.state,
                    favorited: false,
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
                    state: p.state,
                    favorited: false,
                }
            })
            .collect();
        Ok(rows)
    }
}

/// Project `favorites` into display rows, matching each `(addr, port)`
/// against `system_rows` (the favorites-scan's `Ready` rows, or `None` while
/// it hasn't completed yet — favorites render with `matched: None` until
/// then, same as a genuine "not observed" result). A favorite with more than
/// one matching socket (e.g. a LISTEN plus an inbound ESTABLISHED sharing the
/// port) prefers the LISTEN row.
fn build_favorite_rows(
    favorites: &PortFavorites,
    system_rows: Option<&[PortRowView]>,
) -> Vec<FavoriteRowView> {
    favorites
        .items
        .iter()
        .map(|fav| {
            let addr_display = format_addr(fav.addr);
            let matched = system_rows.and_then(|rows| {
                rows.iter()
                    .filter(|r| r.addr_display == addr_display && r.port == fav.port)
                    .max_by_key(|r| r.state.is_listen())
            });
            FavoriteRowView {
                addr_display,
                port: fav.port,
                matched: matched.map(|r| FavoriteMatch {
                    process_name: r.process_name.clone(),
                    pid: r.pid,
                    workspace_name: match &r.source {
                        SourceTag::Tasty { workspace_name, .. } => Some(workspace_name.clone()),
                        SourceTag::External => None,
                    },
                    state: r.state,
                }),
            }
        })
        .collect()
}

/// Drain a pending result from the channel for both scan slots (main +
/// favorites). No-op for a slot that isn't `Loading`.
pub fn poll_scan(state: &mut AppState) {
    poll_state(&mut state.port_scan);
    poll_state(&mut state.port_favorites_scan);
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
        // 상태 필터 드롭다운이 열려 있으면 Escape 는 그것만 닫고 popup 은 유지한다
        // (remote_tool `:181-185` 가드 미러). egui popup 위젯이 같은 프레임에 닫히지
        // 않으므로 여기서 명시적으로 닫는다.
        let popup_open = ui
            .ctx()
            .memory(|m| m.is_popup_open(egui::Id::new(STATE_FILTER_POPUP_ID)));
        if popup_open {
            ui.ctx().memory_mut(|m| m.close_popup());
            return PortScannerAction::None;
        }
        return PortScannerAction::Close;
    }

    let mut action = PortScannerAction::None;

    // design-parity: 디자인 port_scanner.jsx 컨테이너 패딩 0 + 구역별 패딩
    // (header 12/14 / filter 8/14 / body 0 / footer 9/14). content_margin 은
    // port_scanner 한정 0(popup.rs). 구역 밀착은 세로 간격만 0, 구역 내부는 복원.
    let full = ui.max_rect();
    let sep = egui::Stroke::new(
        props.theme.border_width.value(),
        props.theme.border_strong(),
    );
    let saved_spacing = ui.spacing().item_spacing;
    ui.spacing_mut().item_spacing.y = 0.0;

    // 헤더 — 디자인 padding 12 14 + borderBottom.
    let h_ir = egui::Frame::NONE
        .inner_margin(egui::Margin {
            left: PANEL_PAD_X,
            right: PANEL_PAD_X,
            top: props.theme.spacing_md.value() as i8,
            bottom: props.theme.spacing_md.value() as i8,
        })
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing = saved_spacing;
            draw_header_row(ui, props)
        });
    if let Some(a) = h_ir.inner {
        action = a;
    }
    ui.painter()
        .hline(full.x_range(), h_ir.response.rect.bottom(), sep);
    // 헤더 전체(전체폭 × 실측 헤더 높이)를 드래그 이동 영역으로 매니저에 보고한다.
    // 좁은 정적 띠(panel_header_drag_strip) 대신 이 rect 가 hit-test 에 우선 사용된다.
    super::report_header_drag_rect(
        ui.ctx(),
        PORT_SCANNER_POPUP_ID,
        egui::Rect::from_x_y_ranges(full.x_range(), full.top()..=h_ir.response.rect.bottom()),
    );

    // 필터 행 — 디자인 padding 8 14 + borderBottom.
    let f_ir = egui::Frame::NONE
        .inner_margin(egui::Margin {
            left: PANEL_PAD_X,
            right: PANEL_PAD_X,
            top: props.theme.spacing_sm.value() as i8,
            bottom: props.theme.spacing_sm.value() as i8,
        })
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing = saved_spacing;
            draw_filter_row(ui, props)
        });
    if let Some(a) = f_ir.inner {
        action = a;
    }
    ui.painter()
        .hline(full.x_range(), f_ir.response.rect.bottom(), sep);

    // 즐겨찾기 섹션 — 필터 행과 테이블 사이, bounded(캡션 22 + 리스트 최대 112)로
    // 삽입한다. footer 가 아래에서 TopBottomPanel::bottom 으로 먼저 하단을 예약하는
    // 것과 대칭으로, 이 구역은 위에서 먼저 자기 높이만큼 소비하고 CentralPanel(테이블)
    // 이 그 사이 남은 높이를 채운다.
    if let Some(a) = draw_favorites_section(ui, props) {
        action = a;
    }

    // footer — 디자인 padding 9 14 + borderTop. TopBottomPanel 로 popup 하단에 고정해
    // 그린다(remote_tool 폼 footer 미러 `:928`). 패널이 하단 공간을 **먼저** 예약하므로
    // CentralPanel 보다 앞서 호출해야 한다. 본문 테이블의 가로 스크롤은 자체 ScrollArea
    // 래퍼에 갇혀 부모 ui 폭을 넓히지 않으므로, 과거의 footer 고정-rect 핵은 불필요하다.
    let mut footer_action: Option<PortScannerAction> = None;
    let footer = egui::TopBottomPanel::bottom("port_scanner.footer")
        .resizable(false)
        .show_separator_line(false)
        .frame(egui::Frame::NONE.inner_margin(egui::Margin {
            left: PANEL_PAD_X,
            right: PANEL_PAD_X,
            top: FOOTER_PAD_Y,
            bottom: FOOTER_PAD_Y,
        }))
        .show_inside(ui, |ui| {
            ui.spacing_mut().item_spacing = saved_spacing;
            draw_footer(ui, props)
        });
    if let Some(a) = footer.inner {
        footer_action = Some(a);
    }
    // footer 위 구분선 — popup 전체폭(full.x_range), 패널 top 좌표(패널 rect 가 아님).
    ui.painter()
        .hline(full.x_range(), footer.response.rect.top(), sep);

    // 본문 (테이블/로딩/빈 상태) — 남은 높이 전체를 채운다. footer 가 TopBottomPanel 로
    // 이미 하단을 예약했으므로 CentralPanel 의 available_height 에는 footer 가 포함되지
    // 않는다. 행이 적으면 테이블이 auto_shrink 로 위로 붙고 빈 공간은 본문 영역 하단
    // (footer 위)에 남는다.
    egui::CentralPanel::default()
        .frame(egui::Frame::NONE)
        .show_inside(ui, |ui| match &props.view_state {
            PortScannerViewState::Loading => draw_loading_body(ui, props),
            PortScannerViewState::Failed { message } => draw_failed_body(ui, props, message),
            PortScannerViewState::Ready { rows, .. } => {
                if let Some(a) = draw_ready_body(ui, props, rows) {
                    action = a;
                }
            }
        });

    // footer 액션을 마지막에 적용해 우선순위를 유지(원 구조와 동일 — 한 프레임에 footer
    // 와 본문 액션이 동시 발생하지 않으므로 실질 충돌은 없음).
    if let Some(a) = footer_action {
        action = a;
    }

    action
}

/// `host:port` string, bracketing bare IPv6 literals (e.g. `[::]:8080`,
/// `127.0.0.1:3000`). Shared by the footer "Copy address" button, the star
/// tooltip's `{key}`, and the favorites section's address cell.
fn format_host_port(addr_display: &str, port: u16) -> String {
    let host = if addr_display.contains(':') {
        format!("[{addr_display}]")
    } else {
        addr_display.to_string()
    };
    format!("{host}:{port}")
}

/// Address string copied by the footer "Copy address" button.
fn row_copy_address(row: &PortRowView) -> String {
    format_host_port(&row.addr_display, row.port)
}

/// footer: 좌측 카운터 + 우측 `Copy address`(선택 없으면 disabled) + `Close`.
fn draw_footer(ui: &mut egui::Ui, props: &PortScannerProps<'_>) -> Option<PortScannerAction> {
    let th = props.theme;
    let mut out: Option<PortScannerAction> = None;
    let counter = match &props.view_state {
        PortScannerViewState::Loading => Some(props.label_footer_loading.to_string()),
        PortScannerViewState::Ready { rows, total, .. } => Some(
            props
                .label_footer_counter
                .replace("{shown}", &rows.len().to_string())
                .replace("{total}", &total.to_string()),
        ),
        PortScannerViewState::Failed { .. } => None,
    };
    // 선택 행이 현재 표시 중일 때만 복사 대상 주소가 존재한다.
    let selected_addr = match &props.view_state {
        PortScannerViewState::Ready { rows, .. } => props
            .filter
            .selected_port
            .and_then(|p| rows.iter().find(|r| r.port == p))
            .map(row_copy_address),
        _ => None,
    };
    // footer 위 구분선은 caller(draw_port_scanner_view)가 패널 top 에 이미 그린다.
    // 디자인 borderTop 은 1개 — 여기서 중복으로 그리지 않는다(이전 중복선 버그 제거).
    ui.horizontal(|ui| {
        if let Some(s) = &counter {
            ui.label(
                egui::RichText::new(s)
                    // divergence: overlay0=disabled-role 이나 값은 placeholder(neutral-600), 코드값 보존
                    .color(th.text_placeholder())
                    .size(th.font_size_caption.value()),
            );
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            // 디자인 footer: Close = secondary, Copy address = ghost(미선택 시 disabled).
            if Button::new(props.label_close)
                .variant(ButtonVariant::Secondary)
                .show(ui, th)
                .clicked()
            {
                out = Some(PortScannerAction::Close);
            }
            if Button::new(props.label_copy_address)
                .variant(ButtonVariant::Ghost)
                .enabled(selected_addr.is_some())
                .show(ui, th)
                .clicked()
                && let Some(addr) = &selected_addr
            {
                out = Some(PortScannerAction::CopyAddress(addr.clone()));
            }
        });
    });
    out
}

/// 헤더 accent Tag pill 텍스트: Loading → `scanning…`, Ready → `{n} listening`
/// (LISTEN-only scope count, search-independent), Failed → 없음.
fn header_tag_text(props: &PortScannerProps<'_>) -> Option<String> {
    match &props.view_state {
        PortScannerViewState::Loading => Some(props.label_header_tag_scanning.to_string()),
        PortScannerViewState::Ready { listening, .. } => Some(
            props
                .label_header_tag_count
                .replace("{n}", &listening.to_string()),
        ),
        PortScannerViewState::Failed { .. } => None,
    }
}

/// accent Tag pill 렌더 (pid 뱃지 패턴 재사용, accent 변형).
fn draw_header_count_tag(ui: &mut egui::Ui, th: &Theme, text: &str) {
    egui::Frame::default()
        .fill(th.accent_primary().into())
        .corner_radius(TAG_PILL_CORNER_RADIUS)
        // structural: 검색줄 control-internal nudge (size-4/size-1), spacing 리듬 아님.
        .inner_margin(margin_sym(STRUCT_GAP_4, STRUCT_GAP_1))
        .show(ui, |ui| {
            ui.label(
                egui::RichText::new(text)
                    .color(th.text_on_accent())
                    .size(th.font_size_caption.value()),
            );
        });
}

/// 1줄 헤더: 좌측 포트 아이콘 + heading + accent count Tag, 우측 search +
/// Refresh + close.
fn draw_header_row(ui: &mut egui::Ui, props: &PortScannerProps<'_>) -> Option<PortScannerAction> {
    let th = props.theme;
    let mut out: Option<PortScannerAction> = None;
    // 헤더 라벨(제목·카운트 태그)을 비선택으로 만들어 press 시 포인터를 가져가지
    // 않게 한다(egui 기본 selectable_labels=true 면 글자 위 드래그가 텍스트 선택으로
    // 가로채짐). 헤더 프레임 서브트리에만 적용 — 본문 라벨 선택성은 불변.
    ui.style_mut().interaction.selectable_labels = false;
    ui.horizontal(|ui| {
        // B1: leading 포트 아이콘.
        ui.add(icons::PORT.image(th.icon_glyph_size_md.value(), th.text_muted().into()));
        ui.label(
            egui::RichText::new(props.label_heading)
                .color(th.text_primary())
                .size(th.font_size_heading.value())
                .strong(),
        );
        // B2: 헤더 안 accent Tag(`{n} listening` / `scanning…`).
        if let Some(tag) = header_tag_text(props) {
            draw_header_count_tag(ui, th, &tag);
        }
        // 우측에 close 버튼 + Refresh + search (디자인 IconButton ghost + Input).
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            // 디자인 close: IconButton ghost(테두리 없음) — 이전 egui 기본 프레임 버그 제거.
            if IconButton::new()
                .variant(IconButtonVariant::Ghost)
                .show(ui, th, &|ui, rect, c| {
                    icons::CLOSE
                        .image(th.icon_glyph_size_md.value(), c)
                        .paint_at(ui, rect)
                })
                .on_hover_text(props.label_close)
                .clicked()
            {
                out = Some(PortScannerAction::Close);
            }
            // B3: 헤더 우측 Refresh 아이콘 버튼 (상시 노출, 현재 scope 재스캔).
            if IconButton::new()
                .variant(IconButtonVariant::Ghost)
                .show(ui, th, &|ui, rect, c| {
                    icons::REFRESH
                        .image(th.icon_glyph_size_md.value(), c)
                        .paint_at(ui, rect)
                })
                .on_hover_text(props.label_refresh)
                .clicked()
            {
                out = Some(PortScannerAction::Refresh);
            }
            // 컬럼 chooser 트리거: Refresh 옆 COLUMNS IconButton. 클릭 시 컬럼 목록
            // 팝업(컬럼별 checkbox)을 토글한다. 토글은 SetColumnVisible 액션으로 emit.
            if let Some(a) = draw_column_chooser(ui, props) {
                out = Some(a);
            }
            // 디자인 search: Input width 200 + leading search 아이콘.
            let mut buf = props.filter.query.to_string();
            let resp = Input::new()
                .placeholder(props.label_search_placeholder)
                .width(200.0)
                .icon(&|ui, rect, c| {
                    icons::SEARCH
                        .image(th.icon_glyph_size_row_action.value(), c)
                        .paint_at(ui, rect)
                })
                .show(ui, th, &mut buf);
            if resp.changed() && buf != props.filter.query {
                out = Some(PortScannerAction::SetQuery(buf));
            }
        });
    });
    out
}

/// 컬럼별 표시 라벨 (props 의 i18n 컬럼 라벨로 투영).
fn column_label<'a>(col: ColumnId, props: &PortScannerProps<'a>) -> &'a str {
    match col {
        ColumnId::Port => props.label_column_port,
        ColumnId::Proto => props.label_column_proto,
        ColumnId::Address => props.label_column_address,
        ColumnId::Process => props.label_column_process,
        ColumnId::Workspace => props.label_column_workspace,
        ColumnId::Tab => props.label_column_tab,
        ColumnId::State => props.label_column_state,
    }
}

/// 헤더 우측 COLUMNS IconButton + 컬럼 표시/숨김 chooser 팝업.
///
/// 팝업 본문은 컬럼별 [`checkbox`] 목록(전체폭). Port 는 식별/기본 정렬 컬럼이라
/// 항상 표시(checkbox disabled). 토글 시 `SetColumnVisible` 액션을 반환한다. 팝업은
/// `CloseOnClickOutside` 라 한 번 열어 여러 컬럼을 연속 토글할 수 있다.
fn draw_column_chooser(
    ui: &mut egui::Ui,
    props: &PortScannerProps<'_>,
) -> Option<PortScannerAction> {
    let th = props.theme;
    let mut out: Option<PortScannerAction> = None;

    let resp = IconButton::new()
        .variant(IconButtonVariant::Ghost)
        .show(ui, th, &|ui, rect, c| {
            icons::COLUMNS
                .image(th.icon_glyph_size_md.value(), c)
                .paint_at(ui, rect)
        })
        .on_hover_text(props.label_columns_button);

    let popup_id = ui.make_persistent_id(COLUMN_CHOOSER_POPUP_ID);
    if resp.clicked() {
        ui.memory_mut(|m| m.toggle_popup(popup_id));
    }

    egui::popup_below_widget(
        ui,
        popup_id,
        &resp,
        egui::PopupCloseBehavior::CloseOnClickOutside,
        |ui| {
            ui.set_min_width(th.port_columns_menu_min_width().value());
            ui.label(
                egui::RichText::new(props.label_columns_menu_title)
                    .color(th.text_muted())
                    .size(th.font_size_caption.value())
                    .strong(),
            );
            ui.add_space(th.spacing_xs.value());
            for col in ColumnId::ALL {
                let mut checked = props.filter.columns.is_visible(col);
                let enabled = !col.mandatory();
                if checkbox(ui, th, &mut checked, column_label(col, props), enabled).changed() {
                    out = Some(PortScannerAction::SetColumnVisible(col, checked));
                }
            }
        },
    );
    // 드롭다운이 popup_rect 밖으로 삐져나가도 그 위 클릭이 outside-click 으로
    // 오판되지 않도록 실측 rect 를 매니저에 보고(닫혀 있으면 None 으로 정리).
    let overlay_rect = ui
        .memory(|m| m.is_popup_open(popup_id))
        .then(|| ui.memory(|m| m.area_rect(popup_id)))
        .flatten();
    super::report_child_overlay_rect(
        ui.ctx(),
        PORT_SCANNER_POPUP_ID,
        COLUMN_CHOOSER_POPUP_ID,
        overlay_rect,
    );

    out
}

fn read_state_draft(ctx: &egui::Context) -> HashSet<PortState> {
    ctx.memory(|m| {
        m.data
            .get_temp::<HashSet<PortState>>(egui::Id::new(STATE_FILTER_DRAFT_ID))
            .unwrap_or_default()
    })
}

fn write_state_draft(ctx: &egui::Context, draft: HashSet<PortState>) {
    ctx.memory_mut(|m| {
        m.data
            .insert_temp(egui::Id::new(STATE_FILTER_DRAFT_ID), draft);
    });
}

/// 상태 필터 버튼(funnel + 라벨). filtered(=일부 상태만 표시) 면 accent 채움,
/// 아니면 surface0 + border. remote_tool `filter_button:578` 전사.
fn state_filter_button(
    ui: &mut egui::Ui,
    th: &Theme,
    label: &str,
    filtered: bool,
) -> egui::Response {
    let text_col: egui::Color32 = if filtered {
        th.text_on_accent().into()
    } else {
        th.text_primary().into()
    };
    let fill: egui::Color32 = if filtered {
        th.accent_primary().into()
    } else {
        th.surface_raised().into()
    };
    let stroke = if filtered {
        egui::Stroke::NONE
    } else {
        egui::Stroke::new(th.border_width.value(), th.border_strong())
    };
    ui.add(
        egui::Button::image_and_text(
            icons::FUNNEL.image(th.icon_glyph_size_sm.value(), text_col),
            egui::RichText::new(label)
                .color(text_col)
                .size(th.font_size_body.value()),
        )
        .fill(fill)
        .stroke(stroke),
    )
}

/// 드롭다운 내부 separator (remote_tool `hsep:317` 전사 — surface1 hline).
fn state_filter_hsep(ui: &mut egui::Ui, th: &Theme) {
    vspace(ui, STRUCT_GAP_2);
    let r = ui.max_rect();
    ui.painter().hline(
        r.x_range(),
        ui.cursor().top(),
        egui::Stroke::new(th.border_width.value(), th.border_strong()),
    );
    vspace(ui, STRUCT_GAP_2);
}

/// 상태 필터 버튼 + 드롭다운(체크박스 목록 + 모두선택/모두해제/초기화/적용).
///
/// Apply-on-confirm: 드롭다운 편집은 egui temp memory draft 에만 쌓이고 **적용** 눌러야
/// `SetVisibleStates` 로 반영된다(remote_tool `draw_protocol_filter:609` 미러). 단
/// 부호가 반대다 — port_scanner 는 **shown 집합**이라 `checked = draft.contains(s)`
/// (remote_tool 은 hidden 집합이라 `!contains`). 초기화도 다르다 — remote_tool 은
/// select-all 이지만 여기는 **LISTEN-only 복원**(`{Listen}`).
fn draw_state_filter(ui: &mut egui::Ui, props: &PortScannerProps<'_>) -> Option<PortScannerAction> {
    let th = props.theme;
    let present = props.present_states;
    let popup_id = egui::Id::new(STATE_FILTER_POPUP_ID);

    // filtered = present 상태 중 일부만 표시 중(전부 표시면 필터 미적용).
    let total = present.len();
    let selected = present
        .iter()
        .filter(|s| props.filter.visible_states.contains(*s))
        .count();
    let filtered = selected < total;
    let label = if filtered {
        format!("{} · {}/{}", props.label_state_filter, selected, total)
    } else {
        props.label_state_filter.to_string()
    };

    let btn = state_filter_button(ui, th, &label, filtered);
    if btn.clicked() {
        // 열릴 때 draft 를 현재 적용 집합(visible_states)으로 시드.
        if !ui.memory(|m| m.is_popup_open(popup_id)) {
            write_state_draft(ui.ctx(), props.filter.visible_states.clone());
        }
        ui.memory_mut(|m| m.toggle_popup(popup_id));
    }

    let mut applied: Option<PortScannerAction> = None;
    egui::popup::popup_above_or_below_widget(
        ui,
        popup_id,
        &btn,
        egui::AboveOrBelow::Below,
        egui::PopupCloseBehavior::CloseOnClickOutside,
        |ui| {
            ui.set_min_width(th.port_state_menu_min_width().value());
            ui.label(
                egui::RichText::new(props.label_state_filter_title)
                    .color(th.text_muted())
                    .size(th.font_size_caption.value())
                    .monospace(),
            );
            ui.add_space(th.spacing_xs.value());

            // draft 를 한 번 읽어 체크박스 렌더 + 토글 적용 후 변경 시 되쓴다.
            let mut draft = read_state_draft(ui.ctx());
            let mut draft_changed = false;
            egui::ScrollArea::vertical()
                .max_height(th.port_state_menu_max_height().value())
                .show(ui, |ui| {
                    for st in present {
                        // shown 집합 → checked = 포함(remote_tool 의 !contains 와 반대).
                        let mut checked = draft.contains(st);
                        if checkbox(ui, th, &mut checked, st.label(), true).changed() {
                            if checked {
                                draft.insert(*st);
                            } else {
                                draft.remove(st);
                            }
                            draft_changed = true;
                        }
                    }
                });
            if draft_changed {
                write_state_draft(ui.ctx(), draft);
            }

            state_filter_hsep(ui, th);
            // 일괄: 모두 선택(present 전체) / 모두 해제(∅).
            ui.horizontal(|ui| {
                if Button::new(props.label_state_filter_select_all)
                    .variant(ButtonVariant::Ghost)
                    .show(ui, th)
                    .clicked()
                {
                    write_state_draft(ui.ctx(), present.iter().copied().collect());
                }
                if Button::new(props.label_state_filter_deselect_all)
                    .variant(ButtonVariant::Ghost)
                    .show(ui, th)
                    .clicked()
                {
                    write_state_draft(ui.ctx(), HashSet::new());
                }
            });
            // 초기화(LISTEN-only 복원) / 적용.
            ui.horizontal(|ui| {
                if Button::new(props.label_state_filter_reset)
                    .variant(ButtonVariant::Ghost)
                    .show(ui, th)
                    .clicked()
                {
                    write_state_draft(ui.ctx(), HashSet::from([PortState::Listen]));
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if Button::new(props.label_state_filter_apply)
                        .variant(ButtonVariant::Primary)
                        .show(ui, th)
                        .clicked()
                    {
                        // Apply = draft ∩ present (사라진 상태가 묻어 들어가지 않게 보정).
                        let set: HashSet<PortState> = read_state_draft(ui.ctx())
                            .into_iter()
                            .filter(|s| present.contains(s))
                            .collect();
                        applied = Some(PortScannerAction::SetVisibleStates(set));
                    }
                });
            });
        },
    );
    // 드롭다운이 popup_rect 밖으로 삐져나가도 그 위 클릭이 outside-click 으로
    // 오판되지 않도록 실측 rect 를 매니저에 보고(닫혀 있으면 None 으로 정리).
    let overlay_rect = ui
        .memory(|m| m.is_popup_open(popup_id))
        .then(|| ui.memory(|m| m.area_rect(popup_id)))
        .flatten();
    super::report_child_overlay_rect(
        ui.ctx(),
        PORT_SCANNER_POPUP_ID,
        STATE_FILTER_POPUP_ID,
        overlay_rect,
    );
    if applied.is_some() {
        ui.memory_mut(|m| m.close_popup());
    }
    applied
}

/// 2줄 헤더(필터 줄): 좌측 "전체 보기" 체크박스 + 우측 상태 필터 버튼.
fn draw_filter_row(ui: &mut egui::Ui, props: &PortScannerProps<'_>) -> Option<PortScannerAction> {
    let mut out: Option<PortScannerAction> = None;
    ui.horizontal(|ui| {
        let mut checked = props.filter.show_all_system;
        if checkbox(
            ui,
            props.theme,
            &mut checked,
            props.label_filter_show_all_system,
            true,
        )
        .changed()
        {
            out = Some(PortScannerAction::SetShowAllSystem(checked));
        }
        // 우측 정렬 상태 필터(remote_tool add-bar `:489-496` 미러).
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if let Some(a) = draw_state_filter(ui, props) {
                out = Some(a);
            }
        });
    });
    out
}

/// 즐겨찾기 별/컬럼 폭(design `--tasty-port-star-col-width`) — 메인 테이블의 leading
/// fav 컬럼과 즐겨찾기 섹션 행의 별 컬럼이 시각적으로 정렬되도록 공용한다. 디자이너
/// 확정값이라 `column_layout` 의 다른 컬럼 최소폭들과 같은 방식으로 리터럴 유지.
const FAV_COL_WIDTH: f32 = 28.0;

/// 로딩 줄 스피너의 한 변. 값은 아이콘 스케일 md(16)와 같지만 아이콘 글리프가 아니라
/// 스피너 지름이라 그 토큰을 쓰지 않고 이름을 따로 둔다.
const LOADING_SPINNER_SIZE: LogicalPx = LogicalPx(16.0);

/// 즐겨찾기 리스트 스크롤 cap(design "5행 × 22px = 110 ≤ 112 cap") — 5행이 꽉 채워도
/// 스크롤 시작 전 여유 2px 를 남겨 스크롤 가능함을 암시한다.
const FAVORITES_LIST_MAX_H: LogicalPx = LogicalPx(112.0);

/// `PortStar` (design `PortStar`) — 22×22(`item_height_tree`) 별 토글. `on` 이면 채운
/// `STAR_FILL` + accent-warning(Explorer 즐겨찾기와 동일 골드), 아니면 outline `STAR`
/// 와 text-muted. hover 배경 = overlay-hover. 클릭 시 즉시 토글(확인 절차 없음) — 호출
/// 측이 `clicked()` 를 보고 액션을 emit 한다.
fn draw_port_star(ui: &mut egui::Ui, th: &Theme, on: bool) -> egui::Response {
    let side = th.item_height_tree.value();
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(side, side), egui::Sense::click());
    if resp.hovered() {
        ui.painter()
            .rect_filled(rect, th.corner_radius_sm.value(), th.overlay_hover());
    }
    let glyph = th.icon_glyph_size_sm.value();
    let icon_rect = egui::Rect::from_center_size(rect.center(), egui::vec2(glyph, glyph));
    if on {
        icons::STAR_FILL
            .image(glyph, th.accent_warning().into())
            .paint_at(ui, icon_rect);
    } else {
        icons::STAR
            .image(glyph, th.text_muted().into())
            .paint_at(ui, icon_rect);
    }
    resp
}

/// `FavoritesSection` (design `FavoritesSection`) — 상단 즐겨찾기 섹션. 항상 노출되는
/// 캡션(22px: "Favorites"(+개수) · 우측 "system-wide") + 빈 상태(37% 투명 별 + 안내
/// 22px) 또는 스크롤 리스트(최대 112px, 행 22px). 배경 bg-sidebar + 하단 separator.
fn draw_favorites_section(
    ui: &mut egui::Ui,
    props: &PortScannerProps<'_>,
) -> Option<PortScannerAction> {
    let th = props.theme;
    let mut out: Option<PortScannerAction> = None;
    let row_h = th.item_height_tree.value();
    let full = ui.max_rect();

    let ir = egui::Frame::NONE
        .fill(th.bg_sidebar().into())
        .inner_margin(egui::Margin {
            left: PANEL_PAD_X,
            right: PANEL_PAD_X,
            top: 0,
            bottom: 0,
        })
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;

            // 캡션 행 — 좌측 "Favorites"(+개수, 0개면 생략) / 우측 "system-wide".
            ui.allocate_ui_with_layout(
                egui::vec2(ui.available_width(), row_h),
                egui::Layout::left_to_right(egui::Align::Center),
                |ui| {
                    let heading = if props.favorites.is_empty() {
                        props.label_favorites_heading.to_string()
                    } else {
                        props
                            .label_favorites_count
                            .replace("{n}", &props.favorites.len().to_string())
                    };
                    ui.label(
                        egui::RichText::new(heading)
                            .color(th.text_muted())
                            .size(th.font_size_caption.value())
                            .strong(),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(
                            egui::RichText::new(props.label_favorites_system_wide)
                                .color(th.text_muted())
                                .size(th.font_size_caption.value()),
                        );
                    });
                },
            );

            if props.favorites.is_empty() {
                // 빈 상태 — Explorer 사이드바 즐겨찾기와 동일 관례(흐린 별 + 안내 1행).
                ui.allocate_ui_with_layout(
                    egui::vec2(ui.available_width(), row_h),
                    egui::Layout::left_to_right(egui::Align::Center),
                    |ui| {
                        ui.spacing_mut().item_spacing.x = th.spacing_xs.value();
                        let sz = th.icon_glyph_size_sm.value();
                        let (r, _) =
                            ui.allocate_exact_size(egui::vec2(sz, sz), egui::Sense::hover());
                        // 즐겨찾기 별 아이콘 톤. 대응 토큰 없음 — 같은 아이콘이 두 곳에서
                        // 서로 다른 값을 쓴다(수렴은 디자인 판단).
                        const FAV_STAR_ICON_OPACITY: f32 = 0.37;
                        icons::STAR
                            .image(
                                sz,
                                th.text_muted()
                                    .to_egui()
                                    .gamma_multiply(FAV_STAR_ICON_OPACITY),
                            )
                            .paint_at(ui, r);
                        ui.label(
                            egui::RichText::new(props.label_favorites_empty)
                                .color(th.text_muted())
                                .italics()
                                .size(th.font_size_caption.value()),
                        );
                    },
                );
            } else {
                egui::ScrollArea::vertical()
                    .id_salt("port_scanner.favorites_scroll")
                    .max_height(FAVORITES_LIST_MAX_H.value())
                    .auto_shrink([false, true])
                    .show(ui, |ui| {
                        for fav in props.favorites {
                            if let Some(a) = draw_favorite_row(ui, props, fav, row_h) {
                                out = Some(a);
                            }
                        }
                    });
            }
        });

    ui.painter().hline(
        full.x_range(),
        ir.response.rect.bottom(),
        egui::Stroke::new(th.border_width.value(), th.separator),
    );
    out
}

/// 즐겨찾기 리스트 1행(요약형) — 별(항상 on, 클릭 시 제거) · `{addr}:{port}`(mono) ·
/// 매칭 있으면 `{process} · {pid}`(+workspace) 없으면 "not running" · 우측 상태 배지
/// (LISTEN → running+pulse, 그 외 매칭 → waiting, 매칭 없음(NONE) → idle+"NONE").
fn draw_favorite_row(
    ui: &mut egui::Ui,
    props: &PortScannerProps<'_>,
    fav: &FavoriteRowView,
    row_h: f32,
) -> Option<PortScannerAction> {
    let th = props.theme;
    let mut out: Option<PortScannerAction> = None;
    let key = format_host_port(&fav.addr_display, fav.port);

    ui.allocate_ui_with_layout(
        egui::vec2(ui.available_width(), row_h),
        egui::Layout::left_to_right(egui::Align::Center),
        |ui| {
            ui.allocate_ui_with_layout(
                egui::vec2(FAV_COL_WIDTH, row_h),
                egui::Layout::left_to_right(egui::Align::Center),
                |ui| {
                    let resp = draw_port_star(ui, th, true)
                        .on_hover_text(props.label_favorite_remove.replace("{key}", &key));
                    if resp.clicked() {
                        out = Some(PortScannerAction::ToggleFavorite(
                            fav.addr_display.clone(),
                            fav.port,
                        ));
                    }
                },
            );
            ui.label(
                egui::RichText::new(&key)
                    .monospace()
                    .color(th.text_primary())
                    .size(th.font_size_caption.value()),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                match &fav.matched {
                    Some(m) if m.state.is_listen() => {
                        status_dot(
                            ui,
                            th,
                            StatusKind::Running,
                            m.state.label(),
                            true,
                            props.reduced_motion,
                        );
                    }
                    Some(m) => {
                        status_dot(
                            ui,
                            th,
                            StatusKind::Waiting,
                            m.state.label(),
                            false,
                            props.reduced_motion,
                        );
                    }
                    None => {
                        status_dot(
                            ui,
                            th,
                            StatusKind::Idle,
                            props.label_state_none,
                            false,
                            props.reduced_motion,
                        );
                    }
                }
                let detail = match &fav.matched {
                    Some(m) => {
                        let proc = m.process_name.as_deref().unwrap_or("—");
                        let mut s = match m.pid {
                            Some(pid) => format!("{proc} · {pid}"),
                            None => proc.to_string(),
                        };
                        if let Some(ws) = &m.workspace_name {
                            s.push_str(" · ");
                            s.push_str(ws);
                        }
                        s
                    }
                    None => props.label_favorites_not_running.to_string(),
                };
                ui.label(
                    egui::RichText::new(detail)
                        .color(th.text_muted())
                        .size(th.font_size_caption.value()),
                );
            });
        },
    );
    out
}

/// 콘텐츠 중앙 horizontal: Spinner + "Collecting…" 텍스트.
fn draw_loading_body(ui: &mut egui::Ui, props: &PortScannerProps<'_>) {
    let th = props.theme;
    ui.vertical_centered(|ui| {
        // 상태 메시지 top offset = space-xl×2(48) 로 통일 — loading/failed/empty 가 각자
        // 32/40/48 로 제각각이던 것을 하나로 맞춰 상태 전환 시 메시지 위치가 튀지
        // 않게 한다(세 상태 모두 같은 위치에 뜨는 게 사용자가 기대하는 동작).
        vspace(ui, th.spacing_xl * 2.0);
        ui.horizontal(|ui| {
            ui.add(
                egui::Spinner::new()
                    .size(LOADING_SPINNER_SIZE.value())
                    .color(th.text_muted()),
            );
            ui.label(
                egui::RichText::new(props.label_loading)
                    .color(th.text_muted())
                    .size(th.font_size_body.value()),
            );
        });
    });
}

/// Failed: 에러 메시지만. Refresh 는 헤더 우측 버튼(상시 노출)에서 처리한다.
fn draw_failed_body(ui: &mut egui::Ui, props: &PortScannerProps<'_>, message: &str) {
    let th = props.theme;
    ui.vertical_centered(|ui| {
        // 상태 메시지 top offset = space-xl×2(48) 로 통일 — loading/failed/empty 가 각자
        // 32/40/48 로 제각각이던 것을 하나로 맞춰 상태 전환 시 메시지 위치가 튀지
        // 않게 한다(세 상태 모두 같은 위치에 뜨는 게 사용자가 기대하는 동작).
        vspace(ui, th.spacing_xl * 2.0);
        ui.label(
            egui::RichText::new(props.label_failed)
                .color(th.accent_danger())
                .size(th.font_size_body.value())
                .strong(),
        );
        ui.label(
            egui::RichText::new(message)
                .color(th.text_muted())
                .size(th.font_size_caption.value()),
        );
    });
}

/// Ready: 빈 결과 분기 4종 OR TableBuilder 7컬럼.
fn draw_ready_body(
    ui: &mut egui::Ui,
    props: &PortScannerProps<'_>,
    rows: &[PortRowView],
) -> Option<PortScannerAction> {
    let th = props.theme;
    if rows.is_empty() {
        // hidden_by_state 는 검색이 빈 상태에서만 wrapper 가 세우므로 search-zero 보다
        // 뒤에서 본다(검색 결과 0 이 우선). scope 에 행은 있으나 상태 필터가 전부 가렸을 때.
        let empty_label = if !props.filter.query.trim().is_empty() {
            props.label_no_ports_search_zero
        } else if props.filter.hidden_by_state {
            props.label_no_ports_state_filtered
        } else if props.filter.show_all_system {
            props.label_no_ports_system_empty
        } else {
            props.label_no_ports_tasty_empty
        };
        ui.vertical_centered(|ui| {
            // space-xl×2(48) 통일 (loading/failed 와 동일 위치).
            vspace(ui, th.spacing_xl * 2.0);
            ui.label(
                egui::RichText::new(empty_label)
                    .color(th.text_muted())
                    .italics()
                    .size(th.font_size_body.value()),
            );
        });
        return None;
    }
    draw_table(ui, props, rows)
}

/// 디자인 Table td/th `padding: 0 12` — 좌측 정렬 셀. 콘텐츠를 좌측 12 들여쓴다.
fn cell_l(ui: &mut egui::Ui, content: impl FnOnce(&mut egui::Ui)) {
    let th = crate::theme::theme();
    hspace(ui, th.spacing_md);
    content(ui);
}

/// 7컬럼 공용 [`Table`] 위젯. 컬럼 폭은 디자이너 확정값.
fn draw_table(
    ui: &mut egui::Ui,
    props: &PortScannerProps<'_>,
    rows: &[PortRowView],
) -> Option<PortScannerAction> {
    let th = props.theme;
    let text_h = th.font_size_body.value() + 6.0;

    // Cap the inner ScrollArea so the table scrolls *within* the bounded popup
    // content rect instead of overflowing (and being clipped) past it. Reserve
    // the sticky header row and the inter-widget gap from the height available
    // in this CentralPanel. The footer is no longer reserved here: the
    // TopBottomPanel split (draw_port_scanner_view) already carves the footer
    // out of the panel, so this CentralPanel's available_height excludes it —
    // reserving footer_h again would double-count it.
    // (egui_extras' default max_scroll_height is 800px, far taller than the
    // 520px popup, so the body never scrolls without this cap.)
    let header_h = text_h + 4.0;
    let gap = ui.spacing().item_spacing.y;
    let max_scroll = (ui.available_height() - header_h - gap).max(text_h + 8.0);

    // 폭 모델 (이번 작업이 뒤집은 지점): 과거엔 고정폭 + flex(remainder) 로 테이블이
    // 항상 popup 안에 fit 되도록 강제했고, 폭이 모자라면 addr/proc 가 말줄임됐다.
    // 이제는 컬럼별 **최소폭**을 주고, 보이는 컬럼 최소폭 합이 본문 가용폭을 넘으면
    // Table 위젯이 본문을 가로 스크롤한다(말줄임 대신). 가로 스크롤은 본문 영역에만
    // 갇혀 footer/header divider 는 popup 폭에 고정 유지된다(아래 회귀 검증 참조).
    //
    // 최소폭: Port 84 / Proto 76 / Address 140(IPv6 fe80::… 고려) / Process 200
    // (asus_framework.exe PID 10316 고려) / Workspace 120 / Tab 80 / State 140.
    // tasty mono(D2Coding)가 디자인 폰트보다 넓은 메트릭 세금을 min 값에 반영.
    // 가용폭이 최소폭 합보다 넓으면 flex 컬럼(Address/Process)에 여유폭을 분배해
    // 빈 공간 없이 채운다. Port 만 우측 정렬, 정렬 가능: Port/Address/Process/
    // Workspace/Tab (Proto/State 는 정적 헤더).
    let visible: Vec<ColumnId> = ColumnId::ALL
        .into_iter()
        .filter(|c| props.filter.columns.is_visible(*c))
        .collect();

    // 본문 가용폭: 세로 스크롤바 폭 + leading fav 컬럼(28px, ColumnId 밖 — chooser 로
    // 숨길 수 없는 상시 컬럼) 만큼 빼서, 세로 스크롤이 생겨도 가짜 가로 스크롤이 뜨지
    // 않게 하고 나머지 7컬럼 폭 계산은 기존 그대로 둔다.
    //
    // 이 폭 예약은 tasty 의 스크롤 어포던스 표준(스크롤바 숨김 + 가장자리 페이드)에 대한
    // **문서화된 예외**다 — 여기서 빼는 폭은 여백이 아니라 Exact 컬럼 폭과 가로 스크롤
    // 발생 여부를 함께 정하는 계산 입력이다. 예외 조건과 근거는
    // `docs/adr/0079-scroll-affordance-standard.md`.
    let scrollbar_reserve = ui.spacing().scroll.bar_width + ui.spacing().scroll.bar_inner_margin;
    let fav_reserve = FAV_COL_WIDTH + ui.spacing().item_spacing.x;
    let available = (ui.available_width() - scrollbar_reserve - fav_reserve).max(0.0);
    let widths = compute_column_widths(&visible, ui.spacing().item_spacing.x, available);

    let mut columns: Vec<TableColumn<SortKey>> = Vec::with_capacity(visible.len() + 1);
    // fav 컬럼 — 헤더 라벨 없음, 정렬 불가, 항상 표시(컬럼 chooser 대상 아님).
    columns.push(TableColumn {
        title: "",
        width: TableColumnWidth::Exact(FAV_COL_WIDTH),
        align: TableAlign::Left,
        sort_id: None,
    });
    columns.extend(visible.iter().zip(&widths).map(|(id, w)| {
        let (_, _, align, sort_id) = column_layout(*id);
        TableColumn {
            title: column_label(*id, props),
            width: TableColumnWidth::Exact(*w),
            align,
            sort_id,
        }
    }));

    let sort_dir = match props.filter.sort_dir {
        SortDir::Asc => TableSortDir::Asc,
        SortDir::Desc => TableSortDir::Desc,
    };
    let selected_port = props.filter.selected_port;

    // 별 클릭은 여기서 직접 캡처해 행 선택과 분리한다: egui_extras 는 셀 콘텐츠와
    // 별개로 행 전체에도 click sense 를 걸어 겹치는 클릭을 판정하므로(선택 가능
    // 테이블의 구조상 특성), Table 의 `clicked_row` 결과보다 이 플래그를 우선한다 —
    // 별을 클릭한 프레임엔 행 선택을 바꾸지 않고 즐겨찾기만 토글한다.
    let mut fav_click: Option<(String, u16)> = None;

    let output = Table::new(columns)
        .id_salt("port_scanner.table")
        .active_sort(props.filter.sort_key, sort_dir)
        .selectable(true)
        .horizontal_scroll(true)
        // 디자인 Table 헤더 th 배경 = bg-sidebar(mantle), sticky header 전체폭. 디자인
        // th padding 0/12 → header_pad_x 12. 헤더/행 높이는 기존 TableBuilder 값 그대로.
        .header_fill(th.bg_sidebar().into())
        .header_pad_x(12.0)
        .header_height(header_h)
        .row_height(text_h + 8.0)
        .max_scroll_height(max_scroll)
        .show(
            ui,
            th,
            rows,
            |row: &PortRowView| selected_port == Some(row.port),
            // 컬럼이 숨겨지면 인덱스가 밀리므로, 보이는 컬럼 인덱스 → ColumnId 로
            // 매핑해 셀을 분기한다(위치 인덱스 하드코딩 금지). index 0 은 fav 컬럼이라
            // `visible` 매핑 전에 먼저 분기하고, 나머지는 1 만큼 당겨 조회한다.
            |ui, th, row, col_index| {
                if col_index == 0 {
                    let key = format_host_port(&row.addr_display, row.port);
                    let tooltip = if row.favorited {
                        props.label_favorite_remove.replace("{key}", &key)
                    } else {
                        props.label_favorite_add.replace("{key}", &key)
                    };
                    let resp = draw_port_star(ui, th, row.favorited).on_hover_text(tooltip);
                    if resp.clicked() {
                        fav_click = Some((row.addr_display.clone(), row.port));
                    }
                    return;
                }
                match visible[col_index - 1] {
                    // Port — 디자인 align right (위젯이 right_to_left 로 감쌈). 셀 padding 12.
                    ColumnId::Port => {
                        hspace(ui, th.spacing_md);
                        ui.label(
                            egui::RichText::new(row.port.to_string())
                                .color(th.text_primary())
                                .size(th.font_size_body.value()),
                        );
                    }
                    ColumnId::Proto => cell_l(ui, |ui| {
                        // Proto derived from the address family: IPv6 displays
                        // (always containing a colon) → `tcp6`, IPv4 → `tcp`.
                        let proto = if row.addr_display.contains(':') {
                            "tcp6"
                        } else {
                            "tcp"
                        };
                        ui.label(
                            egui::RichText::new(proto)
                                .color(th.text_muted())
                                .size(th.font_size_body.value()),
                        );
                    }),
                    ColumnId::Address => cell_l(ui, |ui| {
                        ui.label(
                            egui::RichText::new(&row.addr_display)
                                .color(th.text_muted())
                                .size(th.font_size_body.value())
                                .monospace(),
                        );
                    }),
                    ColumnId::Process => cell_l(ui, |ui| draw_process_cell(ui, th, row)),
                    ColumnId::Workspace => cell_l(ui, |ui| {
                        draw_workspace_cell(ui, th, row, props.label_external_dash)
                    }),
                    ColumnId::Tab => cell_l(ui, |ui| {
                        draw_tab_cell(ui, th, row, props.label_external_dash)
                    }),
                    ColumnId::State => cell_l(ui, |ui| {
                        draw_state_cell(ui, th, props.reduced_motion, row.state)
                    }),
                }
            },
        );

    // 별 클릭이 이 프레임에 있었다면 행 선택보다 우선한다(위 주석 참고).
    if let Some((addr, port)) = fav_click {
        return Some(PortScannerAction::ToggleFavorite(addr, port));
    }
    // B5: 행 클릭 → 선택 토글, 헤더 클릭 → 정렬 토글 (wrapper 에서 처리). 두 영역은
    // 상호 배타라 한 프레임에 동시 발생하지 않는다.
    if let Some(i) = output.clicked_row {
        return Some(PortScannerAction::Select(rows[i].port));
    }
    if let Some(k) = output.clicked_sort {
        return Some(PortScannerAction::SetSort(k));
    }
    None
}

/// 컬럼의 폭 모델 메타: (최소폭, flex 여부, 정렬, 정렬키). flex 컬럼은 가용폭이 남을 때
/// 여유폭을 나눠 받는다(Address/Process). Port 만 우측 정렬.
fn column_layout(col: ColumnId) -> (f32, bool, TableAlign, Option<SortKey>) {
    match col {
        ColumnId::Port => (84.0, false, TableAlign::Right, Some(SortKey::Port)),
        ColumnId::Proto => (76.0, false, TableAlign::Left, None),
        ColumnId::Address => (140.0, true, TableAlign::Left, Some(SortKey::Address)),
        ColumnId::Process => (200.0, true, TableAlign::Left, Some(SortKey::Process)),
        ColumnId::Workspace => (120.0, false, TableAlign::Left, Some(SortKey::Workspace)),
        ColumnId::Tab => (80.0, false, TableAlign::Left, Some(SortKey::Tab)),
        ColumnId::State => (140.0, false, TableAlign::Left, None),
    }
}

/// 보이는 컬럼들의 픽셀 폭을 계산한다.
///
/// 최소폭 합 ≥ 가용폭 → 각 컬럼은 최소폭 그대로(합이 가용폭 초과 → Table 위젯이
/// 가로 스크롤). 최소폭 합 < 가용폭 → 남는 폭(slack)을 flex 컬럼(Address/Process)에
/// 균등 분배해 빈 공간 없이 채운다. flex 컬럼이 하나도 안 보이면 마지막 컬럼이 slack 을
/// 흡수해 테이블이 가용폭을 채운다. (순수 함수 — 단위 테스트로 분기 검증.)
fn compute_column_widths(visible: &[ColumnId], item_spacing_x: f32, available: f32) -> Vec<f32> {
    let mins: Vec<f32> = visible.iter().map(|c| column_layout(*c).0).collect();
    if visible.is_empty() {
        return mins;
    }
    // 컬럼 사이 간격도 가용폭을 잡아먹으므로 콘텐츠 가용폭에서 제외하고 분배한다.
    let gaps = item_spacing_x * (visible.len() - 1) as f32;
    let sum_min: f32 = mins.iter().sum();
    let mut widths = mins;
    let slack = available - gaps - sum_min;
    if slack > 0.0 {
        let flex: Vec<usize> = visible
            .iter()
            .enumerate()
            .filter(|(_, c)| column_layout(**c).1)
            .map(|(i, _)| i)
            .collect();
        if !flex.is_empty() {
            let per = slack / flex.len() as f32;
            for i in flex {
                widths[i] += per;
            }
        } else if let Some(last) = widths.last_mut() {
            *last += slack;
        }
    }
    widths
}

/// Process 셀: process_name + PID 배지.
fn draw_process_cell(ui: &mut egui::Ui, th: &Theme, row: &PortRowView) {
    ui.horizontal(|ui| {
        let name = row.process_name.as_deref().unwrap_or("—");
        ui.label(
            egui::RichText::new(name)
                .color(th.text_primary())
                .size(th.font_size_body.value()),
        );
        if let Some(pid) = row.pid {
            // 디자인 process 셀의 PID 는 Tag(outlined default).
            tag(ui, th, &format!("PID {pid}"), TagVariant::Default, false);
        }
    });
}

/// Workspace 셀: Tasty → workspace_name, External → dash.
fn draw_workspace_cell(ui: &mut egui::Ui, th: &Theme, row: &PortRowView, dash: &str) {
    match &row.source {
        SourceTag::Tasty { workspace_name, .. } => {
            ui.label(
                egui::RichText::new(workspace_name)
                    .color(th.text_primary())
                    .size(th.font_size_body.value()),
            );
        }
        SourceTag::External => {
            ui.colored_label(th.text_muted(), dash);
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
                    .color(th.text_muted())
                    .size(th.font_size_body.value()),
            );
        }
        None => {
            ui.colored_label(th.text_muted(), dash);
        }
    }
}

/// State 셀: 공용 StatusDot 위젯 (디자인 `StatusDot status pulse label`).
///
/// `LISTEN` → running + pulse, 그 외 → waiting(정적). `reduced_motion` 이면
/// pulse 생략. 위젯이 디자인 pulse(scale 0.6→1.8, opacity 0.5→0, 1.6s)와
/// 라벨을 그린다.
fn draw_state_cell(ui: &mut egui::Ui, th: &Theme, reduced_motion: bool, state: PortState) {
    let is_listen = state.is_listen();
    let kind = if is_listen {
        StatusKind::Running
    } else {
        StatusKind::Waiting
    };
    status_dot(ui, th, kind, state.label(), is_listen, reduced_motion);
}

/// Bracketless display form. IPv6 is shown bare (e.g. wildcard `::`) per the
/// design table; brackets are added later in [`row_copy_address`] when building
/// the copyable `host:port` string.
fn format_addr(addr: IpAddr) -> String {
    match addr {
        IpAddr::V4(v4) if v4.is_unspecified() => "0.0.0.0".to_string(),
        IpAddr::V6(v6) if v6.is_unspecified() => "::".to_string(),
        IpAddr::V4(v4) => v4.to_string(),
        IpAddr::V6(v6) => v6.to_string(),
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
            state: PortState::Listen,
            favorited: false,
        }
    }

    /// LISTEN-only shown set referenced by the test props (matches the runtime
    /// default). A static lets `default_props` hand out a `&'a` without owning.
    static TEST_VISIBLE_STATES: std::sync::LazyLock<HashSet<PortState>> =
        std::sync::LazyLock::new(|| HashSet::from([PortState::Listen]));

    fn default_props<'a>(
        theme: &'a Theme,
        view_state: PortScannerViewState<'a>,
        query: &'a str,
        show_all_system: bool,
    ) -> PortScannerProps<'a> {
        PortScannerProps {
            theme,
            view_state,
            present_states: &[],
            reduced_motion: false,
            favorites: &[],
            filter: PortScannerFilter {
                show_all_system,
                query,
                sort_key: SortKey::Port,
                sort_dir: SortDir::Asc,
                selected_port: None,
                columns: ColumnVisibility::default(),
                visible_states: &TEST_VISIBLE_STATES,
                hidden_by_state: false,
            },
            label_heading: "Listening ports",
            label_search_placeholder: "Search…",
            label_filter_show_all_system: "Show all system ports",
            label_loading: "Scanning…",
            label_failed: "Scan failed",
            label_close: "Close",
            label_refresh: "Refresh",
            label_copy_address: "Copy address",
            label_external_dash: "—",
            label_no_ports_tasty_empty: "No Tasty ports.",
            label_no_ports_system_empty: "No system ports.",
            label_no_ports_search_zero: "No matches.",
            label_footer_loading: "Scanning…",
            label_header_tag_scanning: "scanning…",
            label_header_tag_count: "{n} listening",
            label_footer_counter: "{shown} of {total} ports",
            label_column_port: "Port",
            label_column_proto: "Proto",
            label_column_address: "Address",
            label_column_process: "Process",
            label_column_workspace: "Workspace",
            label_column_tab: "Tab",
            label_column_state: "State",
            label_columns_button: "Columns",
            label_columns_menu_title: "Show columns",
            label_state_filter: "State",
            label_state_filter_title: "Filter by state",
            label_state_filter_select_all: "Select all",
            label_state_filter_deselect_all: "Deselect all",
            label_state_filter_reset: "Reset (LISTEN only)",
            label_state_filter_apply: "Apply",
            label_no_ports_state_filtered: "No ports match the state filter.",
            label_favorites_heading: "Favorites",
            label_favorites_count: "Favorites · {n}",
            label_favorites_system_wide: "system-wide",
            label_favorites_empty: "No favorites yet — click a star in the list below to pin a port.",
            label_favorites_not_running: "not running",
            label_state_none: "NONE",
            label_favorite_add: "Add {key} to favorites",
            label_favorite_remove: "Remove {key} from favorites",
        }
    }

    fn run_view(raw: egui::RawInput, rows: &[PortRowView]) -> PortScannerAction {
        let ctx = egui::Context::default();
        let mut out = PortScannerAction::None;
        let theme = test_theme();
        drop(ctx.run(raw, |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                let view_state = PortScannerViewState::Ready {
                    rows,
                    total: rows.len(),
                    listening: rows.iter().filter(|r| r.state.is_listen()).count(),
                };
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
            state: PortState::Listen,
            favorited: false,
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
        let action = run_view_state(
            PortScannerViewState::Ready {
                rows: &rows,
                total: 0,
                listening: 0,
            },
            "",
            false,
        );
        assert_eq!(action, PortScannerAction::None);
    }

    #[test]
    fn empty_ready_renders_system_empty_message() {
        let rows: Vec<PortRowView> = Vec::new();
        let action = run_view_state(
            PortScannerViewState::Ready {
                rows: &rows,
                total: 0,
                listening: 0,
            },
            "",
            true,
        );
        assert_eq!(action, PortScannerAction::None);
    }

    #[test]
    fn empty_ready_with_query_renders_search_zero_message() {
        let rows: Vec<PortRowView> = Vec::new();
        let action = run_view_state(
            PortScannerViewState::Ready {
                rows: &rows,
                total: 0,
                listening: 0,
            },
            "nginx",
            false,
        );
        assert_eq!(action, PortScannerAction::None);
    }

    #[test]
    fn loading_state_renders_without_panic() {
        let action = run_view_state(PortScannerViewState::Loading, "", false);
        assert_eq!(action, PortScannerAction::None);
    }

    fn row_with(port: u16, pid: Option<u32>, process: Option<&str>) -> PortRowView {
        PortRowView {
            port,
            addr_display: "127.0.0.1".into(),
            pid,
            process_name: process.map(str::to_string),
            source: SourceTag::External,
            state: PortState::Listen,
            favorited: false,
        }
    }

    #[test]
    fn sort_toggle_same_column_flips_direction() {
        // Mirrors the wrapper's SetSort logic — same key → flip dir.
        let mut fs = FilterState {
            sort_key: SortKey::Port,
            sort_dir: SortDir::Asc,
            ..FilterState::default()
        };
        // Same column click.
        let key = SortKey::Port;
        if fs.sort_key == key {
            fs.sort_dir = match fs.sort_dir {
                SortDir::Asc => SortDir::Desc,
                SortDir::Desc => SortDir::Asc,
            };
        } else {
            fs.sort_key = key;
            fs.sort_dir = SortDir::Asc;
        }
        assert_eq!(fs.sort_key, SortKey::Port);
        assert_eq!(fs.sort_dir, SortDir::Desc);

        // Different column click.
        let key = SortKey::Address;
        if fs.sort_key == key {
            fs.sort_dir = match fs.sort_dir {
                SortDir::Asc => SortDir::Desc,
                SortDir::Desc => SortDir::Asc,
            };
        } else {
            fs.sort_key = key;
            fs.sort_dir = SortDir::Asc;
        }
        assert_eq!(fs.sort_key, SortKey::Address);
        assert_eq!(fs.sort_dir, SortDir::Asc);
    }

    #[test]
    fn sort_puts_none_last_for_both_directions() {
        // SortKey::Process: row with Some(process) vs row with None.
        let some = row_with(3000, None, Some("nginx"));
        let none = row_with(3000, None, None);
        // Asc: Some before None.
        assert_eq!(
            compare_rows(SortKey::Process, SortDir::Asc, &some, &none),
            std::cmp::Ordering::Less,
        );
        // Desc: still Some before None — None is sticky to the tail.
        assert_eq!(
            compare_rows(SortKey::Process, SortDir::Desc, &some, &none),
            std::cmp::Ordering::Less,
        );

        // Reverse arguments: None first → must report Greater (= goes behind).
        assert_eq!(
            compare_rows(SortKey::Process, SortDir::Asc, &none, &some),
            std::cmp::Ordering::Greater,
        );
        assert_eq!(
            compare_rows(SortKey::Process, SortDir::Desc, &none, &some),
            std::cmp::Ordering::Greater,
        );
    }

    #[test]
    fn sort_port_asc_then_desc() {
        let mut rows = vec![row_with(8080, None, None), row_with(80, None, None)];
        sort_rows(&mut rows, SortKey::Port, SortDir::Asc);
        assert_eq!(rows[0].port, 80);
        assert_eq!(rows[1].port, 8080);
        sort_rows(&mut rows, SortKey::Port, SortDir::Desc);
        assert_eq!(rows[0].port, 8080);
        assert_eq!(rows[1].port, 80);
    }

    #[test]
    fn sort_process_keeps_none_at_tail_with_desc() {
        let mut rows = vec![
            row_with(1, Some(1), None),
            row_with(2, Some(2), Some("alpha")),
            row_with(3, Some(3), Some("zulu")),
        ];
        sort_rows(&mut rows, SortKey::Process, SortDir::Desc);
        // Desc: zulu before alpha; None always last.
        assert_eq!(rows[0].process_name.as_deref(), Some("zulu"));
        assert_eq!(rows[1].process_name.as_deref(), Some("alpha"));
        assert_eq!(rows[2].process_name, None);
    }

    #[test]
    fn sort_workspace_treats_external_as_missing() {
        let tasty = tasty_row(3000, "frontend", Some("tab1"));
        let external = dummy_row(3000);
        // workspace: Tasty has name, External is "missing" → External tail.
        assert_eq!(
            compare_rows(SortKey::Workspace, SortDir::Asc, &tasty, &external),
            std::cmp::Ordering::Less,
        );
        assert_eq!(
            compare_rows(SortKey::Workspace, SortDir::Desc, &tasty, &external),
            std::cmp::Ordering::Less,
        );
    }

    #[test]
    fn failed_state_renders_without_panic() {
        let action = run_view_state(PortScannerViewState::Failed { message: "boom" }, "", false);
        assert_eq!(action, PortScannerAction::None);
    }

    #[test]
    fn copy_address_formats_ipv4_host_port() {
        let mut row = dummy_row(3000);
        row.addr_display = "127.0.0.1".into();
        assert_eq!(row_copy_address(&row), "127.0.0.1:3000");
    }

    #[test]
    fn copy_address_brackets_ipv6_literal() {
        let mut row = dummy_row(8080);
        row.addr_display = "::".into();
        assert_eq!(row_copy_address(&row), "[::]:8080");
    }

    #[test]
    fn header_tag_reflects_listen_count_not_total() {
        let rows = vec![dummy_row(3000)];
        let theme = test_theme();
        // Ready: tag uses the search-independent LISTEN count, not rows.len().
        let ready = default_props(
            &theme,
            PortScannerViewState::Ready {
                rows: &rows,
                total: 5,
                listening: 3,
            },
            "",
            false,
        );
        assert_eq!(header_tag_text(&ready).as_deref(), Some("3 listening"));
        // Loading: scanning placeholder.
        let loading = default_props(&theme, PortScannerViewState::Loading, "", false);
        assert_eq!(header_tag_text(&loading).as_deref(), Some("scanning…"));
        // Failed: no tag.
        let failed = default_props(
            &theme,
            PortScannerViewState::Failed { message: "boom" },
            "",
            false,
        );
        assert_eq!(header_tag_text(&failed), None);
    }

    #[test]
    fn column_visibility_default_shows_all() {
        let vis = ColumnVisibility::default();
        for col in ColumnId::ALL {
            assert!(vis.is_visible(col), "{col:?} should be visible by default");
        }
    }

    #[test]
    fn column_visibility_hide_then_show() {
        let mut vis = ColumnVisibility::default();
        vis.set(ColumnId::Workspace, false);
        assert!(!vis.is_visible(ColumnId::Workspace));
        // Other columns stay visible.
        assert!(vis.is_visible(ColumnId::Tab));
        assert!(vis.is_visible(ColumnId::Port));
        vis.set(ColumnId::Workspace, true);
        assert!(vis.is_visible(ColumnId::Workspace));
    }

    #[test]
    fn column_visibility_port_is_mandatory() {
        let mut vis = ColumnVisibility::default();
        // Hiding the mandatory Port column is a no-op — it stays visible.
        vis.set(ColumnId::Port, false);
        assert!(vis.is_visible(ColumnId::Port));
        // Persistence round-trips through FilterState default too.
        assert!(FilterState::default().columns.is_visible(ColumnId::Port));
    }

    #[test]
    fn compute_widths_overflow_keeps_mins_for_scroll() {
        // All seven columns, but a narrow body → min-width sum exceeds available,
        // so every column stays at its min (the table then scrolls horizontally).
        let visible: Vec<ColumnId> = ColumnId::ALL.to_vec();
        let widths = compute_column_widths(&visible, 0.0, 100.0);
        for (id, w) in visible.iter().zip(&widths) {
            assert_eq!(*w, column_layout(*id).0, "{id:?} should keep its min width");
        }
        let sum: f32 = widths.iter().sum();
        assert!(
            sum > 100.0,
            "min-width sum must overflow the available width"
        );
    }

    #[test]
    fn compute_widths_distributes_slack_to_flex_columns() {
        let visible: Vec<ColumnId> = ColumnId::ALL.to_vec();
        let sum_min: f32 = visible.iter().map(|c| column_layout(*c).0).sum();
        // Generous width → slack distributed to the two flex columns only.
        let available = sum_min + 200.0;
        let widths = compute_column_widths(&visible, 0.0, available);
        for (id, w) in visible.iter().zip(&widths) {
            let (min, flex, ..) = column_layout(*id);
            if flex {
                assert!(*w > min, "flex {id:?} should grow past its min");
            } else {
                assert_eq!(*w, min, "non-flex {id:?} should stay at its min");
            }
        }
        // Address + Process split 200 evenly → +100 each.
        let addr_i = visible
            .iter()
            .position(|c| *c == ColumnId::Address)
            .unwrap();
        assert!((widths[addr_i] - (140.0 + 100.0)).abs() < 0.5);
    }

    #[test]
    fn compute_widths_no_flex_visible_fills_last_column() {
        // Only fixed (non-flex) columns visible: slack goes to the last column so
        // the table still fills the available width (no trailing gap).
        let visible = vec![ColumnId::Port, ColumnId::Proto, ColumnId::State];
        let sum_min: f32 = visible.iter().map(|c| column_layout(*c).0).sum();
        let available = sum_min + 60.0;
        let widths = compute_column_widths(&visible, 0.0, available);
        assert_eq!(widths[0], column_layout(ColumnId::Port).0);
        assert_eq!(widths[1], column_layout(ColumnId::Proto).0);
        assert!((widths[2] - (column_layout(ColumnId::State).0 + 60.0)).abs() < 0.5);
    }

    #[test]
    fn set_column_visible_action_round_trips_filter_state() {
        // Mirrors the wrapper's SetColumnVisible handling.
        let mut fs = FilterState::default();
        fs.columns.set(ColumnId::Tab, false);
        assert!(!fs.columns.is_visible(ColumnId::Tab));
        fs.columns.set(ColumnId::Tab, true);
        assert!(fs.columns.is_visible(ColumnId::Tab));
    }

    #[test]
    fn selection_toggle_clears_on_same_port() {
        // Mirrors the wrapper's Select handling — re-selecting the same port
        // clears it; a different port replaces the selection.
        let mut fs = FilterState::default();
        assert_eq!(fs.selected_port, None);

        let toggle = |fs: &mut FilterState, port: u16| {
            fs.selected_port = if fs.selected_port == Some(port) {
                None
            } else {
                Some(port)
            };
        };

        toggle(&mut fs, 3000);
        assert_eq!(fs.selected_port, Some(3000));
        // Same port again → cleared.
        toggle(&mut fs, 3000);
        assert_eq!(fs.selected_port, None);
        // Different port → selected.
        toggle(&mut fs, 8080);
        assert_eq!(fs.selected_port, Some(8080));
    }

    fn row_with_state(port: u16, state: PortState) -> PortRowView {
        PortRowView {
            port,
            addr_display: "127.0.0.1".into(),
            pid: Some(1),
            process_name: None,
            source: SourceTag::External,
            state,
            favorited: false,
        }
    }

    #[test]
    fn state_filter_default_is_listen_only() {
        let fs = FilterState::default();
        assert_eq!(fs.visible_states, HashSet::from([PortState::Listen]));
    }

    #[test]
    fn state_filter_predicate_passes_only_shown_states() {
        // The wrapper filters rows with `visible_states.contains(&row.state)`.
        let listen = row_with_state(3000, PortState::Listen);
        let established = row_with_state(3001, PortState::Established);

        // Default set: only LISTEN passes.
        let default_set = HashSet::from([PortState::Listen]);
        assert!(default_set.contains(&listen.state));
        assert!(!default_set.contains(&established.state));

        // Widen the set: ESTABLISHED now passes too.
        let widened = HashSet::from([PortState::Listen, PortState::Established]);
        assert!(widened.contains(&listen.state));
        assert!(widened.contains(&established.state));
    }

    #[test]
    fn present_states_listen_first_then_alphabetical_deduped() {
        // Mixed + duplicated states across rows. Expected order: LISTEN first,
        // then the rest by label() alphabetical (CLOSE_WAIT < ESTABLISHED).
        let rows = vec![
            row_with_state(1, PortState::Established),
            row_with_state(2, PortState::CloseWait),
            row_with_state(3, PortState::Listen),
            row_with_state(4, PortState::Established), // dup
            row_with_state(5, PortState::Listen),      // dup
        ];
        let present = present_states(&rows);
        assert_eq!(
            present,
            vec![
                PortState::Listen,
                PortState::CloseWait,
                PortState::Established,
            ],
        );
    }

    #[test]
    fn present_states_empty_for_no_rows() {
        assert!(present_states(&[]).is_empty());
    }

    #[test]
    fn set_visible_states_action_round_trips_filter_state() {
        // Mirrors the wrapper's SetVisibleStates handling.
        let mut fs = FilterState::default();
        assert_eq!(fs.visible_states, HashSet::from([PortState::Listen]));
        let set = HashSet::from([PortState::Listen, PortState::Established]);
        fs.visible_states = set.clone();
        assert_eq!(fs.visible_states, set);
    }

    #[test]
    fn format_host_port_brackets_ipv6_and_leaves_ipv4_bare() {
        assert_eq!(format_host_port("127.0.0.1", 3000), "127.0.0.1:3000");
        assert_eq!(format_host_port("::", 8080), "[::]:8080");
    }

    fn favorite(
        addr: &str,
        port: u16,
    ) -> crate::adapters::ui::popup::port_scanner_favorites::PortFavorite {
        crate::adapters::ui::popup::port_scanner_favorites::PortFavorite {
            label: format!("{addr}:{port}"),
            addr: addr.parse().unwrap(),
            port,
        }
    }

    #[test]
    fn build_favorite_rows_none_when_scan_not_ready() {
        let mut favs = PortFavorites::default();
        favs.items.push(favorite("127.0.0.1", 3000));
        let rows = build_favorite_rows(&favs, None);
        assert_eq!(rows.len(), 1);
        assert!(rows[0].matched.is_none());
    }

    #[test]
    fn build_favorite_rows_matches_by_addr_and_port() {
        // tasty_row() fixes addr_display to "0.0.0.0" — match on that address.
        let mut favs = PortFavorites::default();
        favs.items.push(favorite("0.0.0.0", 3000));
        favs.items.push(favorite("0.0.0.0", 9999)); // no matching scan row
        let system_rows = vec![tasty_row(3000, "frontend", Some("dev"))];
        let rows = build_favorite_rows(&favs, Some(&system_rows));
        assert_eq!(rows.len(), 2);
        let matched = rows[0].matched.as_ref().expect("3000 should match");
        assert_eq!(matched.process_name.as_deref(), Some("node"));
        assert_eq!(matched.workspace_name.as_deref(), Some("frontend"));
        assert!(matched.state.is_listen());
        assert!(rows[1].matched.is_none(), "9999 has no scan row → NONE");
    }

    #[test]
    fn build_favorite_rows_prefers_listen_when_multiple_matches() {
        let mut favs = PortFavorites::default();
        favs.items.push(favorite("127.0.0.1", 3000));
        let system_rows = vec![
            row_with_state(3000, PortState::Established),
            row_with_state(3000, PortState::Listen),
        ];
        let rows = build_favorite_rows(&favs, Some(&system_rows));
        assert!(rows[0].matched.as_ref().unwrap().state.is_listen());
    }

    #[test]
    fn view_renders_with_favorites_without_panic() {
        let theme = test_theme();
        let rows: Vec<PortRowView> = Vec::new();
        let view_state = PortScannerViewState::Ready {
            rows: &rows,
            total: 0,
            listening: 0,
        };
        let mut props = default_props(&theme, view_state, "", false);
        let favorites = vec![
            FavoriteRowView {
                addr_display: "127.0.0.1".into(),
                port: 3000,
                matched: Some(FavoriteMatch {
                    process_name: Some("node".into()),
                    pid: Some(42),
                    workspace_name: None,
                    state: PortState::Listen,
                }),
            },
            FavoriteRowView {
                addr_display: "0.0.0.0".into(),
                port: 9999,
                matched: None,
            },
        ];
        props.favorites = &favorites;
        let ctx = egui::Context::default();
        let mut out = PortScannerAction::None;
        drop(ctx.run(egui::RawInput::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                out = draw_port_scanner_view(ui, &props);
            });
        }));
        assert_eq!(out, PortScannerAction::None);
    }
}
