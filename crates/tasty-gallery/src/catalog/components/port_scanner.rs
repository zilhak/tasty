//! Port Scanner popup 데모 (Tier 3).
//!
//! 본체 `src/adapters/ui/popup/port_scanner.rs::draw_port_scanner_view` 가
//! 표현하는 시각 상태를 mock props 로 재현. 본체와 *시각 동일* 하지만
//! gallery 가 본체 binary 에 의존할 수 없으므로 view 로직은 로컬 미러
//! (POC 패턴 — `.claude-workspace/conductor/tier-3-props-extraction-pattern.md`).
//!
//! 6 종 시각 케이스:
//! 1. Loading — Spinner + "scanning…" 메시지.
//! 2. Tasty 기본 — 7 컬럼 (Port/Proto/Address/Process/Workspace/Tab/State), Tasty 행 4 건.
//! 3. System (전체 보기) — Tasty 행 + External 행 혼합 (workspace/tab em-dash).
//! 4. Search Zero — query 가 모든 행을 거름 → search_zero 메시지.
//! 5. Tasty Empty — show_all_system=false 인데 Ready rows 0 건 → tasty_empty 메시지.
//! 6. Desc Sort — 동일 행 셋을 SortKey::Port + SortDir::Desc 로 역순 표시.

use egui_extras::{Column, TableBuilder};
use tasty_type_appearance::theme::Theme;

#[derive(Clone, Debug, PartialEq, Eq)]
enum SourceTag {
    Tasty {
        workspace_name: String,
        tab_name: Option<String>,
    },
    External,
}

#[derive(Clone, Debug)]
struct PortRowView {
    port: u16,
    addr_display: String,
    pid: Option<u32>,
    process_name: Option<String>,
    source: SourceTag,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SortKey {
    Port,
    Address,
    Process,
    Workspace,
    Tab,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SortDir {
    Asc,
    Desc,
}

#[derive(Clone, Copy)]
enum PortScannerViewState<'a> {
    Loading,
    Ready { rows: &'a [PortRowView] },
}

struct PortScannerFilter<'a> {
    show_all_system: bool,
    query: &'a str,
    sort_key: SortKey,
    sort_dir: SortDir,
}

struct PortScannerProps<'a> {
    theme: &'a Theme,
    view_state: PortScannerViewState<'a>,
    filter: PortScannerFilter<'a>,
    label_heading: &'a str,
    label_search_placeholder: &'a str,
    label_filter_show_all_system: &'a str,
    label_loading: &'a str,
    label_close: &'a str,
    label_external_dash: &'a str,
    label_no_ports_tasty_empty: &'a str,
    label_no_ports_search_zero: &'a str,
    label_footer_loading: &'a str,
    label_header_tag_scanning: &'a str,
    label_header_tag_count: &'a str,
    label_footer_counter: &'a str,
    label_column_port: &'a str,
    label_column_proto: &'a str,
    label_column_address: &'a str,
    label_column_process: &'a str,
    label_column_workspace: &'a str,
    label_column_tab: &'a str,
    label_column_state: &'a str,
}

/// 본체 view 의 시각 미러 (gallery 측 복제). Action 은 catalog 에서 실행되지 않음.
fn draw_mock_port_scanner_view(ui: &mut egui::Ui, props: &PortScannerProps<'_>) {
    let th = props.theme;
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = 6.0;
        draw_header_row(ui, props);
        draw_filter_row(ui, props);
        ui.separator();
        match &props.view_state {
            PortScannerViewState::Loading => draw_loading_body(ui, props),
            PortScannerViewState::Ready { rows } => {
                if rows.is_empty() {
                    let empty_label = if !props.filter.query.trim().is_empty() {
                        props.label_no_ports_search_zero
                    } else {
                        props.label_no_ports_tasty_empty
                    };
                    ui.vertical_centered(|ui| {
                        ui.add_space(28.0);
                        ui.label(
                            egui::RichText::new(empty_label)
                                .color(egui::Color32::from(th.subtext0))
                                .italics()
                                .size(th.font_size_body.value()),
                        );
                    });
                } else {
                    draw_table(ui, props, rows);
                }
            }
        }
        draw_footer(ui, props);
    });
}

fn draw_header_row(ui: &mut egui::Ui, props: &PortScannerProps<'_>) {
    let th = props.theme;
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new(props.label_heading)
                .color(egui::Color32::from(th.text))
                .size(th.font_size_heading.value())
                .strong(),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            // mock — 갤러리에선 클릭 처리 없이 hover tooltip 만 보임.
            let _btn = ui
                .button(
                    egui::RichText::new("×")
                        .size(16.0)
                        .color(egui::Color32::from(th.text)),
                )
                .on_hover_text(props.label_close);
            let avail = ui.available_width().max(120.0);
            let mut buf = props.filter.query.to_string();
            ui.add_sized(
                egui::vec2(avail, 22.0),
                egui::TextEdit::singleline(&mut buf)
                    .hint_text(props.label_search_placeholder)
                    .desired_width(avail),
            );
        });
    });
}

fn draw_filter_row(ui: &mut egui::Ui, props: &PortScannerProps<'_>) {
    let th = props.theme;
    ui.horizontal(|ui| {
        let mut checked = props.filter.show_all_system;
        ui.checkbox(&mut checked, props.label_filter_show_all_system);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let tag = match &props.view_state {
                PortScannerViewState::Loading => props.label_header_tag_scanning.to_string(),
                PortScannerViewState::Ready { rows } => props
                    .label_header_tag_count
                    .replace("{n}", &rows.len().to_string()),
            };
            if !tag.is_empty() {
                ui.label(
                    egui::RichText::new(tag)
                        .color(egui::Color32::from(th.subtext0))
                        .size(th.font_size_caption.value()),
                );
            }
        });
    });
}

fn draw_loading_body(ui: &mut egui::Ui, props: &PortScannerProps<'_>) {
    let th = props.theme;
    ui.vertical_centered(|ui| {
        ui.add_space(28.0);
        ui.horizontal(|ui| {
            ui.add(
                egui::Spinner::new()
                    .size(16.0)
                    .color(egui::Color32::from(th.subtext0)),
            );
            ui.label(
                egui::RichText::new(props.label_loading)
                    .color(egui::Color32::from(th.subtext0))
                    .size(th.font_size_body.value()),
            );
        });
        ui.add_space(8.0);
    });
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
    };
    if let Some(s) = text {
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(
                egui::RichText::new(s)
                    .color(egui::Color32::from(th.overlay0))
                    .size(th.font_size_caption.value()),
            );
        });
    }
}

fn draw_table(ui: &mut egui::Ui, props: &PortScannerProps<'_>, rows: &[PortRowView]) {
    let th = props.theme;
    let text_h = th.font_size_body.value() + 6.0;

    // Mirror of the wrapper: cap the inner ScrollArea so the table scrolls
    // within its bounded host instead of overflowing it. Reserve the sticky
    // header row, the pinned footer, and the inter-widget gap from the
    // remaining height. (egui_extras' default max_scroll_height is 800px.)
    let header_h = text_h + 4.0;
    let footer_h = th.font_size_caption.value() + 6.0;
    let gap = ui.spacing().item_spacing.y;
    let max_scroll = (ui.available_height() - header_h - footer_h - gap).max(text_h + 8.0);

    TableBuilder::new(ui)
        .striped(false)
        .resizable(false)
        .max_scroll_height(max_scroll)
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
                draw_header_cell(
                    ui,
                    th,
                    props.label_column_port,
                    Some(SortKey::Port),
                    &props.filter,
                );
            });
            header.col(|ui| {
                draw_header_cell(ui, th, props.label_column_proto, None, &props.filter);
            });
            header.col(|ui| {
                draw_header_cell(
                    ui,
                    th,
                    props.label_column_address,
                    Some(SortKey::Address),
                    &props.filter,
                );
            });
            header.col(|ui| {
                draw_header_cell(
                    ui,
                    th,
                    props.label_column_process,
                    Some(SortKey::Process),
                    &props.filter,
                );
            });
            header.col(|ui| {
                draw_header_cell(
                    ui,
                    th,
                    props.label_column_workspace,
                    Some(SortKey::Workspace),
                    &props.filter,
                );
            });
            header.col(|ui| {
                draw_header_cell(
                    ui,
                    th,
                    props.label_column_tab,
                    Some(SortKey::Tab),
                    &props.filter,
                );
            });
            header.col(|ui| {
                draw_header_cell(ui, th, props.label_column_state, None, &props.filter);
            });
        })
        .body(|mut body| {
            for row in rows.iter() {
                body.row(text_h + 8.0, |mut tr| {
                    tr.col(|ui| {
                        ui.label(
                            egui::RichText::new(row.port.to_string())
                                .color(egui::Color32::from(th.text))
                                .size(th.font_size_body.value()),
                        );
                    });
                    tr.col(|ui| {
                        ui.label(
                            egui::RichText::new("TCP")
                                .color(egui::Color32::from(th.subtext0))
                                .size(th.font_size_body.value()),
                        );
                    });
                    tr.col(|ui| {
                        ui.label(
                            egui::RichText::new(&row.addr_display)
                                .color(egui::Color32::from(th.subtext0))
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
}

fn draw_header_cell(
    ui: &mut egui::Ui,
    th: &Theme,
    label: &str,
    this_col: Option<SortKey>,
    filter: &PortScannerFilter<'_>,
) {
    let is_active = this_col.map(|k| k == filter.sort_key).unwrap_or(false);
    let arrow = if is_active {
        match filter.sort_dir {
            SortDir::Asc => " ▲",
            SortDir::Desc => " ▼",
        }
    } else {
        ""
    };
    let text = if arrow.is_empty() {
        label.to_string()
    } else {
        format!("{label}{arrow}")
    };
    let rich = egui::RichText::new(text)
        .color(if is_active {
            egui::Color32::from(th.text)
        } else {
            egui::Color32::from(th.subtext0)
        })
        .size(th.font_size_caption.value())
        .strong();
    ui.label(rich);
}

fn draw_process_cell(ui: &mut egui::Ui, th: &Theme, row: &PortRowView) {
    ui.horizontal(|ui| {
        let name = row.process_name.as_deref().unwrap_or("—");
        ui.label(
            egui::RichText::new(name)
                .color(egui::Color32::from(th.text))
                .size(th.font_size_body.value()),
        );
        if let Some(pid) = row.pid {
            let badge = egui::RichText::new(format!("PID {pid}"))
                .color(egui::Color32::from(th.subtext0))
                .size(th.font_size_caption.value())
                .monospace();
            egui::Frame::default()
                .fill(egui::Color32::from(th.surface1))
                .corner_radius(egui::CornerRadius::same(3))
                .inner_margin(egui::Margin::symmetric(4, 1))
                .show(ui, |ui| {
                    ui.label(badge);
                });
        }
    });
}

fn draw_workspace_cell(ui: &mut egui::Ui, th: &Theme, row: &PortRowView, dash: &str) {
    match &row.source {
        SourceTag::Tasty { workspace_name, .. } => {
            ui.label(
                egui::RichText::new(workspace_name)
                    .color(egui::Color32::from(th.text))
                    .size(th.font_size_body.value()),
            );
        }
        SourceTag::External => {
            ui.colored_label(egui::Color32::from(th.subtext0), dash);
        }
    }
}

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
                    .color(egui::Color32::from(th.subtext0))
                    .size(th.font_size_body.value()),
            );
        }
        None => {
            ui.colored_label(egui::Color32::from(th.subtext0), dash);
        }
    }
}

fn draw_state_cell(ui: &mut egui::Ui, th: &Theme) {
    ui.horizontal(|ui| {
        let (rect, _) = ui.allocate_exact_size(egui::vec2(8.0, 8.0), egui::Sense::hover());
        ui.painter()
            .circle_filled(rect.center(), 4.0, egui::Color32::from(th.green));
        ui.label(
            egui::RichText::new("LISTEN")
                .color(egui::Color32::from(th.subtext0))
                .size(th.font_size_caption.value()),
        );
    });
}

fn tasty_row(
    port: u16,
    addr: &str,
    pid: u32,
    process: &str,
    workspace: &str,
    tab: Option<&str>,
) -> PortRowView {
    PortRowView {
        port,
        addr_display: addr.to_string(),
        pid: Some(pid),
        process_name: Some(process.to_string()),
        source: SourceTag::Tasty {
            workspace_name: workspace.to_string(),
            tab_name: tab.map(str::to_string),
        },
    }
}

fn external_row(port: u16, addr: &str, pid: u32, process: &str) -> PortRowView {
    PortRowView {
        port,
        addr_display: addr.to_string(),
        pid: Some(pid),
        process_name: Some(process.to_string()),
        source: SourceTag::External,
    }
}

fn mock_tasty_rows() -> Vec<PortRowView> {
    vec![
        tasty_row(
            3000,
            "0.0.0.0",
            12345,
            "node",
            "frontend",
            Some("dev-server"),
        ),
        tasty_row(5173, "127.0.0.1", 12346, "vite", "frontend", Some("docs")),
        tasty_row(8080, "[::]", 12347, "cargo", "backend", Some("api")),
        tasty_row(9229, "127.0.0.1", 12348, "node", "backend", None),
    ]
}

fn mock_system_rows() -> Vec<PortRowView> {
    vec![
        tasty_row(
            3000,
            "0.0.0.0",
            12345,
            "node",
            "frontend",
            Some("dev-server"),
        ),
        tasty_row(8080, "[::]", 12347, "cargo", "backend", Some("api")),
        external_row(22, "0.0.0.0", 412, "sshd"),
        external_row(53, "127.0.0.1", 87, "dnsmasq"),
        external_row(631, "[::]", 624, "cupsd"),
        external_row(5432, "127.0.0.1", 1840, "postgres"),
    ]
}

fn props<'a>(
    theme: &'a Theme,
    view_state: PortScannerViewState<'a>,
    query: &'a str,
    show_all_system: bool,
    sort_key: SortKey,
    sort_dir: SortDir,
) -> PortScannerProps<'a> {
    PortScannerProps {
        theme,
        view_state,
        filter: PortScannerFilter {
            show_all_system,
            query,
            sort_key,
            sort_dir,
        },
        label_heading: "Listening ports",
        label_search_placeholder: "Search…",
        label_filter_show_all_system: "전체 보기 (system)",
        label_loading: "Scanning…",
        label_close: "Close",
        label_external_dash: "—",
        label_no_ports_tasty_empty: "이 터미널에서 listening 포트가 없습니다.",
        label_no_ports_search_zero: "검색 결과가 없습니다.",
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

fn case_label(ui: &mut egui::Ui, theme: &Theme, title: &str) {
    ui.label(
        egui::RichText::new(title)
            .strong()
            .color(egui::Color32::from(theme.text)),
    );
    ui.add_space(2.0);
}

fn frame_card(ui: &mut egui::Ui, theme: &Theme, width: f32, body: impl FnOnce(&mut egui::Ui)) {
    egui::Frame::group(ui.style())
        .corner_radius(theme.corner_radius.value())
        .show(ui, |ui| {
            ui.set_min_width(width);
            ui.set_max_width(width);
            body(ui);
        });
    ui.add_space(16.0);
}

pub fn draw(ui: &mut egui::Ui, theme: &Theme) {
    ui.label(
        egui::RichText::new(
            "draw_port_scanner_view — 7컬럼 TableBuilder, 비동기 스캔, 검색·정렬, 시스템 전체 토글",
        )
        .small()
        .color(egui::Color32::from(theme.subtext0)),
    );
    ui.add_space(4.0);
    ui.label(
        egui::RichText::new(
            "Wrapper: src/adapters/ui/popup/port_scanner.rs::draw_port_scanner_popup",
        )
        .small()
        .color(egui::Color32::from(theme.subtext0)),
    );
    ui.add_space(12.0);

    let card_width = 760.0;

    // ── Case 1: Loading.
    case_label(ui, theme, "Case 1 — Loading (Spinner + scanning…)");
    let p = props(
        theme,
        PortScannerViewState::Loading,
        "",
        false,
        SortKey::Port,
        SortDir::Asc,
    );
    frame_card(ui, theme, card_width, |ui| {
        draw_mock_port_scanner_view(ui, &p);
    });

    // ── Case 2: Tasty 기본 7 컬럼.
    case_label(ui, theme, "Case 2 — Tasty 기본 (7컬럼, 4건)");
    let rows = mock_tasty_rows();
    let p = props(
        theme,
        PortScannerViewState::Ready { rows: &rows },
        "",
        false,
        SortKey::Port,
        SortDir::Asc,
    );
    frame_card(ui, theme, card_width, |ui| {
        draw_mock_port_scanner_view(ui, &p);
    });

    // ── Case 3: System (전체 보기) — Tasty + External 혼합.
    case_label(
        ui,
        theme,
        "Case 3 — System 전체 보기 (Tasty + External 혼합, External 은 ws/tab em-dash)",
    );
    let rows = mock_system_rows();
    let p = props(
        theme,
        PortScannerViewState::Ready { rows: &rows },
        "",
        true,
        SortKey::Port,
        SortDir::Asc,
    );
    frame_card(ui, theme, card_width, |ui| {
        draw_mock_port_scanner_view(ui, &p);
    });

    // ── Case 4: Search Zero.
    case_label(ui, theme, "Case 4 — Search Zero (query 가 모든 행을 거름)");
    let empty: Vec<PortRowView> = Vec::new();
    let p = props(
        theme,
        PortScannerViewState::Ready { rows: &empty },
        "nonexistent",
        false,
        SortKey::Port,
        SortDir::Asc,
    );
    frame_card(ui, theme, card_width, |ui| {
        draw_mock_port_scanner_view(ui, &p);
    });

    // ── Case 5: Tasty Empty (Ready rows 0 건, 검색어 없음).
    case_label(
        ui,
        theme,
        "Case 5 — Tasty Empty (show_all_system=false, Ready rows 0건)",
    );
    let empty: Vec<PortRowView> = Vec::new();
    let p = props(
        theme,
        PortScannerViewState::Ready { rows: &empty },
        "",
        false,
        SortKey::Port,
        SortDir::Asc,
    );
    frame_card(ui, theme, card_width, |ui| {
        draw_mock_port_scanner_view(ui, &p);
    });

    // ── Case 6: Desc 정렬 — 동일 rows 를 Port Desc 로 정렬.
    case_label(
        ui,
        theme,
        "Case 6 — Desc 정렬 (SortKey::Port, SortDir::Desc — 헤더에 ▼ 인디케이터)",
    );
    let mut rows = mock_tasty_rows();
    rows.sort_by_key(|r| std::cmp::Reverse(r.port));
    let p = props(
        theme,
        PortScannerViewState::Ready { rows: &rows },
        "",
        false,
        SortKey::Port,
        SortDir::Desc,
    );
    frame_card(ui, theme, card_width, |ui| {
        draw_mock_port_scanner_view(ui, &p);
    });

    ui.label(
        egui::RichText::new(
            "Note: 본체 wrapper 는 백그라운드 thread + mpsc 로 PortScanState 머신을 \
             Idle → Loading → Ready 로 전이시키고, descendant PID + (Tasty / System) 스코프 \
             toggle 로 row set 을 채운다. 갤러리는 상태 머신 없이 mock props 로 시각만 재현.",
        )
        .small()
        .color(egui::Color32::from(theme.subtext0)),
    );
}
