//! Listening ports — 디자인(4) Overlays `ports` Spec.
//!
//! 660×520 모달. 헤더(port icon + title + count Tag + filter + columns + refresh +
//! close) · Show-all 체크행 · 즐겨찾기 섹션(bounded, system-wide LISTEN/NONE) ·
//! 최소폭 컬럼 Table(가로 스크롤, Workspace 숨김 케이스, leading fav 컬럼) ·
//! footer(count + Copy address + Close). 색·치수는 Theme 토큰, Table 은 공용 위젯.

use tasty_type_appearance::theme::Theme;
use tasty_type_geometry::length::LogicalPx;
use tasty_ui_widgets::{
    Button, ButtonVariant, IconButton, IconButtonVariant, StatusKind, Table, TableAlign,
    TableColumn, TableColumnWidth, TagVariant, status_dot, tag,
};

use crate::catalog::icons;
use crate::catalog::spec::{self, StageVariant, TokenChip};
use crate::catalog::widgets::dialog as kit;

const WIDTH: LogicalPx = LogicalPx(660.0);
/// 즐겨찾기 별 컬럼 폭 — 본체 `port_scanner.rs` 의 `FAV_COL_WIDTH` 미러.
const FAV_COL_WIDTH: LogicalPx = LogicalPx(28.0);

struct PortRow {
    port: &'static str,
    proto: &'static str,
    addr: &'static str,
    proc: &'static str,
    pid: &'static str,
    ws: &'static str,
    state: &'static str,
    selected: bool,
    favorited: bool,
}

/// 즐겨찾기 섹션 1행 mock — 매칭(LISTEN/other) 또는 NONE.
struct FavoriteRow {
    key: &'static str,
    detail: &'static str,
    /// `None` → NONE 배지(idle). `Some(true)` → running(pulse). `Some(false)` → waiting.
    listening: Option<bool>,
}

const FAVORITE_ROWS: &[FavoriteRow] = &[
    FavoriteRow {
        key: "127.0.0.1:3000",
        detail: "node · 48213 · Project A",
        listening: Some(true),
    },
    FavoriteRow {
        key: "0.0.0.0:9443",
        detail: "not running",
        listening: None,
    },
];

const ROWS: &[PortRow] = &[
    PortRow {
        port: "3000",
        proto: "tcp",
        addr: "127.0.0.1",
        proc: "node",
        pid: "48213",
        ws: "Project A",
        state: "LISTEN",
        selected: false,
        favorited: true,
    },
    PortRow {
        port: "5173",
        proto: "tcp",
        addr: "127.0.0.1",
        proc: "vite",
        pid: "48990",
        ws: "Project A",
        state: "LISTEN",
        selected: false,
        favorited: false,
    },
    PortRow {
        port: "8080",
        proto: "tcp",
        addr: "0.0.0.0",
        proc: "tasty-agent",
        pid: "50321",
        ws: "Project B",
        state: "LISTEN",
        selected: true,
        favorited: false,
    },
    PortRow {
        port: "8443",
        proto: "tcp6",
        addr: "::",
        proc: "tasty-agent",
        pid: "50321",
        ws: "Project B",
        state: "LISTEN",
        selected: false,
        favorited: false,
    },
    PortRow {
        port: "9229",
        proto: "tcp",
        addr: "127.0.0.1",
        proc: "node",
        pid: "48213",
        ws: "Project A",
        state: "CLOSE_WAIT",
        selected: false,
        favorited: false,
    },
];

pub fn draw(ui: &mut egui::Ui, theme: &Theme) {
    spec::stage(ui, theme, StageVariant::Wrap, |ui| {
        kit::frame_card(ui, theme, WIDTH, kit::panel_fill(theme), |ui| {
            // 헤더 (padding 10x14).
            kit::region_sym(ui, theme.spacing_md, theme.spacing_sm, |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = theme.spacing_sm.value();
                    kit::icon(
                        ui,
                        icons::PORT,
                        theme.icon_glyph_size_md,
                        theme.text_secondary().to_egui(),
                    );
                    kit::title(ui, theme, "Listening ports");
                    tag(ui, theme, "5 listening", TagVariant::Accent, false);
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        IconButton::new().variant(IconButtonVariant::Ghost).show(
                            ui,
                            theme,
                            &|ui, rect, c| icons::CLOSE.image(rect.height(), c).paint_at(ui, rect),
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
            });
            kit::hsep(ui, theme);

            // Show-all 체크행 (padding 8x14).
            kit::region_sym(ui, theme.spacing_md, theme.spacing_sm, |ui| {
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
                    kit::body(ui, theme, "Show all (system-wide)");
                    // 우측 정렬 상태 필터 버튼(적용 변형 — accent 채움).
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        funnel_button(ui, theme, "State · 1/3", true);
                    });
                });
            });

            // 즐겨찾기 섹션 (design FavoritesSection) — 캡션(22px: "Favorites · N" +
            // 우측 "system-wide") + bounded 리스트(최대 112px, 행 22px). bg-sidebar
            // 배경 + 하단 separator. 별 컬럼 폭은 메인 테이블과 정렬(FAV_COL_WIDTH).
            // 혼합 상태(매칭 1 + NONE 1) — 빈 상태는 아래 별도 stage 에서 시연한다.
            draw_favorites_section(ui, theme, FAVORITE_ROWS);

            // Table — 컬럼별 최소폭 + 가로 스크롤. 최소폭 합(708)이 660 프레임을 넘어
            // 본문이 좌우 스크롤된다(말줄임 대신). Workspace 컬럼은 chooser 로 숨긴
            // 상태(컬럼 표시/숨김 시각 케이스). leading fav 컬럼(28px, 헤더 라벨 없음)
            // 은 chooser 대상이 아니라 나머지 7컬럼과 별개로 항상 표시.
            kit::region_sym(ui, theme.spacing_sm, LogicalPx(0.0), |ui| {
                let cols = vec![
                    col(
                        "",
                        TableColumnWidth::Exact(FAV_COL_WIDTH.value()),
                        TableAlign::Left,
                    ),
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
            kit::region_sym(ui, theme.spacing_md, theme.spacing_sm, |ui| {
                ui.horizontal(|ui| {
                    kit::caption(ui, theme, "5 of 5 ports", false);
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        Button::new("Close")
                            .variant(ButtonVariant::Secondary)
                            .show(ui, theme);
                        Button::new("Copy address")
                            .variant(ButtonVariant::Ghost)
                            .show(ui, theme);
                    });
                });
            });
        });
    });

    // 상태 필터 — 닫힘 버튼 + 열린 드롭다운(체크박스 목록 + 일괄 조작). 본체 신규 UI 라
    // gallery-first 로 닫힘/적용/열림 3상태를 노출한다(적용 변형은 위 모달 show-all 행).
    spec::stage(ui, theme, StageVariant::Wrap, |ui| {
        // 닫힘(미적용) 버튼 — surface-raised + border.
        funnel_button(ui, theme, "State", false);
        // 열린 드롭다운 카드(min-width 216).
        kit::frame_card(ui, theme, LogicalPx(216.0), kit::panel_fill(theme), |ui| {
            kit::region_sym(ui, theme.spacing_sm, theme.spacing_sm, |ui| {
                kit::caption(ui, theme, "Filter by state", true);
                ui.add_space(theme.spacing_xs.value());
                check_row(ui, theme, "LISTEN", true);
                check_row(ui, theme, "ESTABLISHED", false);
                check_row(ui, theme, "CLOSE_WAIT", false);
                kit::hsep(ui, theme);
                ui.horizontal(|ui| {
                    Button::new("Select all")
                        .variant(ButtonVariant::Ghost)
                        .show(ui, theme);
                    Button::new("Deselect all")
                        .variant(ButtonVariant::Ghost)
                        .show(ui, theme);
                });
                ui.horizontal(|ui| {
                    Button::new("Reset (LISTEN only)")
                        .variant(ButtonVariant::Ghost)
                        .show(ui, theme);
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        Button::new("Apply")
                            .variant(ButtonVariant::Primary)
                            .show(ui, theme);
                    });
                });
            });
        });
    });

    // 즐겨찾기 섹션 — 빈 상태(0개). design §6.4 확정: 0개여도 캡션은 유지하고
    // 흐린 별(37%) + 안내 문구 1행을 보여준다(Explorer 사이드바 즐겨찾기와 동일
    // 관례). 위 모달의 "즐겨찾기 1개 이상"(혼합 상태) 시연과 별개로 gallery-first
    // 정책에 따라 노출한다.
    spec::stage(ui, theme, StageVariant::Wrap, |ui| {
        kit::frame_card(ui, theme, WIDTH, kit::panel_fill(theme), |ui| {
            draw_favorites_section(ui, theme, &[]);
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
            ("fav column", "leading 28px, no header label, always shown"),
            (
                "favorites",
                "bounded caption(22) + list(max 112) · system-wide",
            ),
            (
                "favorites empty",
                "caption stays · faded star(37%) + hint row",
            ),
            ("columns", "chooser hides cols (Workspace hidden here)"),
            (
                "state filter",
                "funnel button · dropdown · default LISTEN-only",
            ),
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
            TokenChip::new(
                "accent-warning",
                "star on (favorited)",
                theme.accent_warning().to_egui(),
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
         labels visible while scrolling. The state filter (funnel button, filter \
         row) defaults to LISTEN-only; its dropdown is a shown set — checked \
         states are shown, Reset restores LISTEN-only, Apply commits the draft. \
         The favorites section (between the filter row and the table) always \
         shows pinned ports regardless of the table's scope/search/state \
         filter — its LISTEN/NONE judgment is system-wide. Its list is bounded \
         to 112px (5 rows) before scrolling; a leading 28px star column (no \
         header label, not hideable) toggles favorites in both the section and \
         the main table.",
    );
}

/// 상태 필터 funnel 버튼 mock — 본체 `state_filter_button` 전사. applied 면 accent
/// 채움 + on-accent, 아니면 surface-raised + border.
fn funnel_button(ui: &mut egui::Ui, theme: &Theme, label: &str, applied: bool) {
    let text_col = if applied {
        theme.text_on_accent().to_egui()
    } else {
        theme.text_primary().to_egui()
    };
    let fill = if applied {
        theme.accent_primary().to_egui()
    } else {
        theme.surface_raised().to_egui()
    };
    let stroke = if applied {
        egui::Stroke::NONE
    } else {
        egui::Stroke::new(theme.border_width.value(), theme.border_default().to_egui())
    };
    ui.add(
        egui::Button::image_and_text(
            icons::FUNNEL.image(theme.icon_glyph_size_sm.value(), text_col),
            egui::RichText::new(label)
                .color(text_col)
                .size(theme.font_size_body.value()),
        )
        .fill(fill)
        .stroke(stroke),
    );
}

/// 드롭다운 체크박스 행 mock — checked 면 accent 채움 + check, 아니면 빈 보더 박스.
fn check_row(ui: &mut egui::Ui, theme: &Theme, label: &str, checked: bool) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = theme.spacing_sm.value();
        let s = theme.icon_glyph_size_md.value();
        let (r, _) = ui.allocate_exact_size(egui::vec2(s, s), egui::Sense::hover());
        if checked {
            ui.painter().rect_filled(
                r,
                theme.corner_radius_sm.value(),
                theme.accent_primary().to_egui(),
            );
            icons::CHECK
                .image(s, theme.text_on_accent().to_egui())
                .paint_at(ui, r);
        } else {
            ui.painter().rect_stroke(
                r,
                theme.corner_radius_sm.value(),
                egui::Stroke::new(theme.border_width.value(), theme.border_default().to_egui()),
                egui::StrokeKind::Inside,
            );
        }
        kit::body(ui, theme, label);
    });
}

fn col(title: &str, width: TableColumnWidth, align: TableAlign) -> TableColumn<'_, ()> {
    TableColumn {
        title,
        width,
        align,
        sort_id: None,
    }
}

/// `FavoritesSection` mock — 캡션(22px: "Favorites"(+개수, 0개면 생략) + 우측
/// "system-wide") + bounded 리스트(최대 112px, 행 22px) 또는 빈 상태(흐린 별 37% +
/// 안내 1행). `favorites` 가 비면 빈 상태를 그린다(design §6.4). 본체
/// `port_scanner.rs::draw_favorites_section` 전사.
fn draw_favorites_section(ui: &mut egui::Ui, theme: &Theme, favorites: &[FavoriteRow]) {
    let fav_row_h = theme.item_height_tree.value();
    let fav_ir = egui::Frame::NONE
        .fill(theme.bg_sidebar().to_egui())
        .show(ui, |ui| {
            kit::region_sym(ui, theme.spacing_md, LogicalPx(0.0), |ui| {
                ui.allocate_ui_with_layout(
                    egui::vec2(ui.available_width(), fav_row_h),
                    egui::Layout::left_to_right(egui::Align::Center),
                    |ui| {
                        let heading = if favorites.is_empty() {
                            "Favorites".to_string()
                        } else {
                            format!("Favorites · {}", favorites.len())
                        };
                        kit::caption(ui, theme, &heading, false);
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            kit::caption(ui, theme, "system-wide", false);
                        });
                    },
                );

                if favorites.is_empty() {
                    // 빈 상태 — Explorer 사이드바 즐겨찾기와 동일 관례(흐린 별 + 안내 1행).
                    ui.allocate_ui_with_layout(
                        egui::vec2(ui.available_width(), fav_row_h),
                        egui::Layout::left_to_right(egui::Align::Center),
                        |ui| {
                            ui.spacing_mut().item_spacing.x = theme.spacing_xs.value();
                            let sz = theme.icon_glyph_size_sm.value();
                            let (r, _) =
                                ui.allocate_exact_size(egui::vec2(sz, sz), egui::Sense::hover());
                            icons::STAR
                                .image(sz, theme.text_muted().to_egui().gamma_multiply(0.37))
                                .paint_at(ui, r);
                            ui.label(
                                egui::RichText::new(
                                    "No favorites yet — click a star in the list below to pin a port.",
                                )
                                .italics()
                                .size(theme.font_size_caption.value())
                                .color(theme.text_muted().to_egui()),
                            );
                        },
                    );
                } else {
                    for fav in favorites {
                        ui.allocate_ui_with_layout(
                            egui::vec2(ui.available_width(), fav_row_h),
                            egui::Layout::left_to_right(egui::Align::Center),
                            |ui| {
                                ui.allocate_ui_with_layout(
                                    egui::vec2(FAV_COL_WIDTH.value(), fav_row_h),
                                    egui::Layout::left_to_right(egui::Align::Center),
                                    |ui| star(ui, theme, true),
                                );
                                ui.label(
                                    egui::RichText::new(fav.key)
                                        .monospace()
                                        .size(theme.font_size_caption.value())
                                        .color(theme.text_primary().to_egui()),
                                );
                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        match fav.listening {
                                            Some(true) => status_dot(
                                                ui,
                                                theme,
                                                StatusKind::Running,
                                                "LISTEN",
                                                true,
                                                false,
                                            ),
                                            Some(false) => status_dot(
                                                ui,
                                                theme,
                                                StatusKind::Waiting,
                                                "CLOSE_WAIT",
                                                false,
                                                false,
                                            ),
                                            None => status_dot(
                                                ui,
                                                theme,
                                                StatusKind::Idle,
                                                "NONE",
                                                false,
                                                false,
                                            ),
                                        };
                                        kit::caption(ui, theme, fav.detail, false);
                                    },
                                );
                            },
                        );
                    }
                }
            });
        });
    ui.painter().hline(
        fav_ir.response.rect.x_range(),
        fav_ir.response.rect.bottom(),
        egui::Stroke::new(theme.border_width.value(), theme.separator.to_egui()),
    );
}

/// `PortStar` mock — 22×22, on(채운 STAR_FILL + accent-warning) / off(outline STAR
/// + text-muted). 본체 `port_scanner.rs::draw_port_star` 전사.
fn star(ui: &mut egui::Ui, theme: &Theme, on: bool) {
    let side = theme.item_height_tree.value();
    let (rect, _) = ui.allocate_exact_size(egui::vec2(side, side), egui::Sense::hover());
    let glyph = theme.icon_glyph_size_sm.value();
    let icon_rect = egui::Rect::from_center_size(rect.center(), egui::vec2(glyph, glyph));
    if on {
        icons::STAR_FILL
            .image(glyph, theme.accent_warning().to_egui())
            .paint_at(ui, icon_rect);
    } else {
        icons::STAR
            .image(glyph, theme.text_muted().to_egui())
            .paint_at(ui, icon_rect);
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
    // 컬럼: fav / Port / Proto / Address / Process / State / copy (Workspace 는
    // chooser 로 숨김). c==0 은 leading fav 컬럼(28px, 나머지는 기존대로 1 씩 밀림).
    let _ = row.ws; // Workspace 는 chooser 로 숨겨 렌더 안 함 — 필드 미사용(값 drop, Result 아님).
    match c {
        0 => star(ui, theme, row.favorited),
        1 => mono(ui, row.port, theme.text_primary().to_egui()),
        2 => mono(ui, row.proto, theme.text_muted().to_egui()),
        3 => mono(ui, row.addr, theme.text_secondary().to_egui()),
        4 => {
            // Process name + pid Tag (design: <span>{proc}<Tag>{pid}</Tag></span>).
            mono(ui, row.proc, theme.text_secondary().to_egui());
            tag(ui, theme, row.pid, TagVariant::Default, false);
        }
        5 => {
            let v = if row.state == "LISTEN" {
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
                theme.icon_glyph_size_sm,
                theme.text_muted().to_egui(),
            );
        }
    }
}
