//! `Table` primitive specimen — 디자인(4) `components/data/Table` 카드.
//!
//! 본체·포트 스캐너와 동일한 `tasty_ui_widgets::Table` 위젯을 5행 미니 데모로 호출
//! (demo=main). sticky 헤더(bg-sidebar) · mono 셀 · 행 선택(surface-active) · 내부
//! 스크롤 cap. 하단 `meta` 로 치수/토큰 노출.

use std::cell::RefCell;

use tasty_type_appearance::theme::Theme;
use tasty_ui_widgets::{Table, TableAlign, TableColumn, TableColumnWidth, TableSortDir};

use crate::catalog::spec::{StageVariant, TokenChip, meta, stage};

/// 정렬 가능 컬럼 키 (port 컬럼만 정렬 데모).
#[derive(Clone, Copy, PartialEq, Eq)]
enum SortKey {
    Port,
}

struct Row {
    port: u16,
    proto: &'static str,
    addr: &'static str,
    proc: &'static str,
    state: &'static str,
}

thread_local! {
    static SELECTED: RefCell<usize> = const { RefCell::new(1) };
}

pub fn draw(ui: &mut egui::Ui, theme: &Theme) {
    let rows = [
        Row {
            port: 3000,
            proto: "TCP",
            addr: "127.0.0.1",
            proc: "node",
            state: "LISTEN",
        },
        Row {
            port: 5432,
            proto: "TCP",
            addr: "127.0.0.1",
            proc: "postgres",
            state: "LISTEN",
        },
        Row {
            port: 6379,
            proto: "TCP",
            addr: "127.0.0.1",
            proc: "redis-server",
            state: "LISTEN",
        },
        Row {
            port: 8080,
            proto: "TCP",
            addr: "0.0.0.0",
            proc: "tasty",
            state: "LISTEN",
        },
        Row {
            port: 9229,
            proto: "TCP",
            addr: "127.0.0.1",
            proc: "node",
            state: "CLOSE_WAIT",
        },
    ];

    stage(ui, theme, StageVariant::Tight, |ui| {
        let columns = vec![
            TableColumn {
                title: "port",
                width: TableColumnWidth::Initial {
                    initial: 84.0,
                    at_least: 60.0,
                },
                align: TableAlign::Left,
                sort_id: Some(SortKey::Port),
            },
            TableColumn {
                title: "proto",
                width: TableColumnWidth::Initial {
                    initial: 76.0,
                    at_least: 60.0,
                },
                align: TableAlign::Left,
                sort_id: None,
            },
            TableColumn {
                title: "address",
                width: TableColumnWidth::Remainder {
                    at_least: 100.0,
                    clip: false,
                },
                align: TableAlign::Left,
                sort_id: None,
            },
            TableColumn {
                title: "process",
                width: TableColumnWidth::Remainder {
                    at_least: 100.0,
                    clip: false,
                },
                align: TableAlign::Left,
                sort_id: None,
            },
            TableColumn {
                title: "state",
                width: TableColumnWidth::Initial {
                    initial: 140.0,
                    at_least: 100.0,
                },
                align: TableAlign::Left,
                sort_id: None,
            },
        ];

        SELECTED.with(|s| {
            let mut sel = s.borrow_mut();
            let selected = *sel;
            let out = Table::new(columns)
                .active_sort(SortKey::Port, TableSortDir::Asc)
                .header_fill(egui::Color32::from(theme.bg_sidebar()))
                .selectable(true)
                .max_scroll_height(theme.overlay_top_offset.value() * 3.0)
                .id_salt("prim_table_demo")
                .show(
                    ui,
                    theme,
                    &rows,
                    |row: &Row| rows.iter().position(|r| r.port == row.port) == Some(selected),
                    |ui, th, row, col| {
                        let (text, muted) = match col {
                            0 => (row.port.to_string(), false),
                            1 => (row.proto.to_string(), true),
                            2 => (row.addr.to_string(), true),
                            3 => (row.proc.to_string(), false),
                            _ => (row.state.to_string(), true),
                        };
                        let color = if muted {
                            egui::Color32::from(th.text_muted())
                        } else {
                            egui::Color32::from(th.text_primary())
                        };
                        ui.label(
                            egui::RichText::new(text)
                                .size(th.font_size_body.value())
                                .monospace()
                                .color(color),
                        );
                    },
                );
            if let Some(i) = out.clicked_row {
                *sel = i;
            }
        });
    });

    meta(
        ui,
        theme,
        &[
            ("row", "28 · dense 22"),
            ("header", "sticky bg-sidebar"),
            ("cell", "mono"),
            ("selected", "surface-active"),
        ],
        &[
            TokenChip::new(
                "bg-sidebar",
                "header fill",
                egui::Color32::from(theme.bg_sidebar()),
            ),
            TokenChip::new(
                "surface-active",
                "selected row",
                egui::Color32::from(theme.surface_active()),
            ),
            TokenChip::new(
                "overlay-hover",
                "hover row",
                egui::Color32::from(theme.overlay_hover()),
            ),
            TokenChip::new(
                "accent-primary",
                "sort indicator",
                egui::Color32::from(theme.accent_primary()),
            ),
        ],
    );
}
