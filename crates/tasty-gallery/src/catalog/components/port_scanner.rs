//! Listening ports — 디자인(4) Overlays `ports` Spec.
//!
//! 660×520 모달. 헤더(port icon + title + count Tag + filter + columns + refresh +
//! close) · Show-all 체크행 · 최소폭 컬럼 Table(가로 스크롤, Workspace 숨김 케이스) ·
//! footer(count + Copy address + Close). 색·치수는 Theme 토큰, Table 은 공용 위젯.

use tasty_type_appearance::theme::Theme;
use tasty_ui_widgets::{
    Button, ButtonVariant, IconButton, IconButtonVariant, Table, TableAlign, TableColumn,
    TableColumnWidth, TagVariant, tag,
};

use crate::catalog::icons;
use crate::catalog::spec::{self, StageVariant, TokenChip};
use crate::catalog::widgets::dialog as kit;

const WIDTH: f32 = 660.0;

struct PortRow {
    port: &'static str,
    proto: &'static str,
    addr: &'static str,
    proc: &'static str,
    ws: &'static str,
    state: &'static str,
    selected: bool,
}

const ROWS: &[PortRow] = &[
    PortRow {
        port: "3000",
        proto: "tcp",
        addr: "127.0.0.1",
        proc: "node",
        ws: "web",
        state: "listening",
        selected: true,
    },
    PortRow {
        port: "5173",
        proto: "tcp",
        addr: "127.0.0.1",
        proc: "vite",
        ws: "web",
        state: "listening",
        selected: false,
    },
    PortRow {
        port: "8080",
        proto: "tcp",
        addr: "0.0.0.0",
        proc: "caddy",
        ws: "infra",
        state: "listening",
        selected: false,
    },
    PortRow {
        port: "5432",
        proto: "tcp",
        addr: "127.0.0.1",
        proc: "postgres",
        ws: "db",
        state: "listening",
        selected: false,
    },
    PortRow {
        port: "6379",
        proto: "tcp",
        addr: "127.0.0.1",
        proc: "redis",
        ws: "db",
        state: "established",
        selected: false,
    },
];

pub fn draw(ui: &mut egui::Ui, theme: &Theme) {
    spec::stage(ui, theme, StageVariant::Wrap, |ui| {
        kit::frame_card(ui, theme, WIDTH, kit::panel_fill(theme), |ui| {
            // 헤더 (padding 10x14).
            kit::region_sym(
                ui,
                theme.spacing_md.value(),
                theme.spacing_sm.value(),
                |ui| {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = theme.spacing_sm.value();
                        kit::icon(
                            ui,
                            icons::PORT,
                            theme.icon_glyph_size_md.value(),
                            theme.text_secondary().to_egui(),
                        );
                        kit::title(ui, theme, "Listening ports");
                        tag(ui, theme, "5", TagVariant::Default, false);
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            IconButton::new().variant(IconButtonVariant::Ghost).show(
                                ui,
                                theme,
                                &|ui, rect, c| {
                                    icons::CLOSE.image(rect.height(), c).paint_at(ui, rect)
                                },
                            );
                            // 컬럼 chooser 트리거(컬럼 표시/숨김). Refresh 옆.
                            IconButton::new().variant(IconButtonVariant::Ghost).show(
                                ui,
                                theme,
                                &|ui, rect, c| {
                                    icons::COLUMNS.image(rect.height(), c).paint_at(ui, rect)
                                },
                            );
                            IconButton::new().variant(IconButtonVariant::Ghost).show(
                                ui,
                                theme,
                                &|ui, rect, c| {
                                    icons::REFRESH.image(rect.height(), c).paint_at(ui, rect)
                                },
                            );
                            kit::field(
                                ui,
                                theme,
                                Some(theme.field_width_md.value()),
                                "Filter…",
                                true,
                                false,
                            );
                        });
                    });
                },
            );
            kit::hsep(ui, theme);

            // Show-all 체크행 (padding 8x14).
            kit::region_sym(
                ui,
                theme.spacing_md.value(),
                theme.spacing_sm.value(),
                |ui| {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = theme.spacing_sm.value();
                        // 체크박스 mock (checked).
                        let s = theme.icon_glyph_size_md.value();
                        let (r, _) = ui.allocate_exact_size(egui::vec2(s, s), egui::Sense::hover());
                        ui.painter().rect_filled(
                            r,
                            theme.corner_radius_sm.value(),
                            theme.accent_primary().to_egui(),
                        );
                        icons::SHIELD_CHECK
                            .image(s, theme.text_on_accent().to_egui())
                            .paint_at(ui, r);
                        kit::body(ui, theme, "Show all interfaces (0.0.0.0)");
                    });
                },
            );

            // Table — 컬럼별 최소폭 + 가로 스크롤. 최소폭 합(708)이 660 프레임을 넘어
            // 본문이 좌우 스크롤된다(말줄임 대신). Workspace 컬럼은 chooser 로 숨긴
            // 상태(컬럼 표시/숨김 시각 케이스).
            kit::region_sym(ui, theme.spacing_sm.value(), 0.0, |ui| {
                let cols = vec![
                    col("Port", TableColumnWidth::Exact(84.0), TableAlign::Right),
                    col("Proto", TableColumnWidth::Exact(76.0), TableAlign::Left),
                    col("Address", TableColumnWidth::Exact(140.0), TableAlign::Left),
                    col("Process", TableColumnWidth::Exact(200.0), TableAlign::Left),
                    col("State", TableColumnWidth::Exact(140.0), TableAlign::Left),
                    col(
                        "",
                        TableColumnWidth::Exact(theme.item_height_interactive.value()),
                        TableAlign::Right,
                    ),
                ];
                Table::new(cols)
                    .id_salt("ports_table")
                    .horizontal_scroll(true)
                    .header_fill(theme.bg_sidebar().to_egui())
                    .header_pad_x(theme.spacing_sm.value())
                    .row_height(theme.item_height_interactive.value())
                    .max_scroll_height(theme.measure_md.value() * 0.7)
                    .selectable(true)
                    .show(ui, theme, ROWS, |r| r.selected, cell);
            });
            kit::hsep(ui, theme);

            // footer (padding 8x14).
            kit::region_sym(
                ui,
                theme.spacing_md.value(),
                theme.spacing_sm.value(),
                |ui| {
                    ui.horizontal(|ui| {
                        kit::caption(ui, theme, "5 ports · 1 selected", false);
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            Button::new("Close")
                                .variant(ButtonVariant::Ghost)
                                .show(ui, theme);
                            Button::new("Copy address")
                                .variant(ButtonVariant::Secondary)
                                .show(ui, theme);
                        });
                    });
                },
            );
        });
    });

    spec::meta(
        ui,
        theme,
        &[
            ("frame", "660×520 · bg-panel"),
            (
                "header",
                "icon · title · count · filter · columns · refresh · close",
            ),
            ("table", "min-width cols · h-scroll · sticky header"),
            ("columns", "chooser hides cols (Workspace hidden here)"),
            ("header bg", "bg-sidebar · mono caption"),
            ("footer", "count · Copy address · Close"),
        ],
        &[
            TokenChip::new("bg-panel", "frame", theme.bg_panel().to_egui()),
            TokenChip::new("bg-sidebar", "header row", theme.bg_sidebar().to_egui()),
            TokenChip::new(
                "surface-active",
                "selected row",
                theme.surface_active().to_egui(),
            ),
            TokenChip::new(
                "accent-success",
                "listening",
                theme.accent_success().to_egui(),
            ),
        ],
    );

    spec::note(
        ui,
        theme,
        "Cells are monospace for scannable columns. Each column has a minimum \
         width; when the visible columns' minimums exceed the frame the table \
         scrolls horizontally (instead of ellipsizing). The columns chooser \
         (header) toggles which columns show — here Workspace is hidden. \
         Selecting a row enables Copy address; the sticky header keeps column \
         labels visible while scrolling.",
    );
}

fn col(title: &str, width: TableColumnWidth, align: TableAlign) -> TableColumn<'_, ()> {
    TableColumn {
        title,
        width,
        align,
        sort_id: None,
    }
}

fn cell(ui: &mut egui::Ui, theme: &Theme, row: &PortRow, c: usize) {
    let mono = |ui: &mut egui::Ui, text: &str, color: egui::Color32| {
        ui.label(
            egui::RichText::new(text)
                .monospace()
                .size(theme.font_size_term_sm.value())
                .color(color),
        );
    };
    // 컬럼: Port / Proto / Address / Process / State / copy (Workspace 는 chooser 로 숨김).
    let _ = row.ws;
    match c {
        0 => mono(ui, row.port, theme.text_primary().to_egui()),
        1 => mono(ui, row.proto, theme.text_muted().to_egui()),
        2 => mono(ui, row.addr, theme.text_secondary().to_egui()),
        3 => mono(ui, row.proc, theme.text_secondary().to_egui()),
        4 => {
            let v = if row.state == "listening" {
                TagVariant::Success
            } else {
                TagVariant::Default
            };
            tag(ui, theme, row.state, v, true);
        }
        _ => {
            kit::icon(
                ui,
                icons::COPY,
                theme.icon_glyph_size_sm.value(),
                theme.text_muted().to_egui(),
            );
        }
    }
}
