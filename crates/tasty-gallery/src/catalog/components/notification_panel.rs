//! 알림 목록·빈 상태와 전체화면 진입 버튼의 정적 예제.
//! 목록만 스크롤하며 미확인 알림을 배경색과 별표로 구분한다.

use tasty_type_appearance::theme::Theme;
use tasty_type_geometry::length::LogicalPx;
use tasty_ui_widgets::tokens::STRUCT_GAP_2;
use tasty_ui_widgets::{Button, ButtonVariant, ControlSize, vspace};

use crate::catalog::popup_frame::{self, ContentInset, TitleButtons};
use crate::catalog::spec::{self, StageVariant, TokenChip};

/// `popup/defs.rs` 의 `notifications` 기본 크기.
const PANEL_W: LogicalPx = LogicalPx(350.0);
const PANEL_H: LogicalPx = LogicalPx(400.0);
/// 본체 unread 배경 알파 (0..255).
const UNREAD_BG_ALPHA: u8 = 20;

/// 목록 항목 한 건의 표시 데이터 — 본체가 `engine.notifications` 에서 뽑는 튜플과 동형.
struct Entry {
    read: bool,
    title: &'static str,
    body: &'static str,
    time: &'static str,
    workspace: &'static str,
}

const ENTRIES: &[Entry] = &[
    Entry {
        read: false,
        title: "Build finished",
        body: "cargo build --release completed in 3m 12s.",
        time: "12s ago",
        workspace: "main",
    },
    Entry {
        read: false,
        title: "Agent needs input",
        body: "Waiting for a reply in pane 2.",
        time: "4m ago",
        workspace: "review",
    },
    Entry {
        read: true,
        title: "Plugin reloaded",
        body: "",
        time: "2h ago",
        workspace: "Unknown",
    },
];

fn entry_row(ui: &mut egui::Ui, theme: &Theme, e: &Entry) {
    let bg = if e.read {
        egui::Color32::TRANSPARENT
    } else {
        theme.accent_primary().with_alpha(UNREAD_BG_ALPHA).to_egui()
    };
    egui::Frame::new()
        .fill(bg)
        .inner_margin(egui::Margin::same(theme.spacing_xs.value() as i8))
        .corner_radius(theme.corner_radius.value())
        .show(ui, |ui| {
            // right_to_left 배치가 남은 높이를 모두 차지하지 않도록 행 높이를 제한한다.
            let row_h = theme.font_size_body.value() + theme.spacing_xs.value();
            ui.allocate_ui_with_layout(
                egui::vec2(ui.available_width(), row_h),
                egui::Layout::left_to_right(egui::Align::Center),
                |ui| {
                    if !e.read {
                        ui.label(
                            egui::RichText::new("*")
                                .color(theme.accent_primary().to_egui())
                                .strong(),
                        );
                    }
                    ui.label(
                        egui::RichText::new(e.title)
                            .size(theme.font_size_caption.value())
                            .strong()
                            .color(theme.text_primary().to_egui()),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(
                            egui::RichText::new(e.time)
                                .size(theme.font_size_caption.value())
                                .color(theme.text_muted().to_egui()),
                        );
                    });
                },
            );
            if !e.body.is_empty() {
                ui.label(
                    egui::RichText::new(e.body)
                        .size(theme.font_size_caption.value())
                        .color(theme.text_secondary().to_egui()),
                );
            }
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new(e.workspace)
                        .size(theme.font_size_caption.value())
                        .color(theme.accent_primary().to_egui()),
                );
                Button::new("Jump")
                    .variant(ButtonVariant::Ghost)
                    .size(ControlSize::Sm)
                    .show(ui, theme);
            });
        });
    vspace(ui, STRUCT_GAP_2);
}

/// 패널 본문 — `empty` 면 목록 대신 중앙 안내.
fn panel(ui: &mut egui::Ui, theme: &Theme, empty: bool) {
    popup_frame::draw(
        ui,
        theme,
        "Notifications",
        PANEL_W,
        PANEL_H,
        ContentInset::INSET,
        TitleButtons::FULLSCREEN_AND_CLOSE,
        // 본체 SHADOWLESS_POPUPS에 포함된 알림 창은 그림자를 그리지 않는다.
        None,
        |ui| {
            let unread = if empty {
                0
            } else {
                ENTRIES.iter().filter(|e| !e.read).count()
            };
            // 헤더 행: 우측 정렬 버튼이 남은 세로를 전부 먹지 않도록 행 높이를 묶는다.
            ui.allocate_ui_with_layout(
                egui::vec2(ui.available_width(), theme.item_height_interactive.value()),
                egui::Layout::left_to_right(egui::Align::Center),
                |ui| {
                    ui.label(
                        egui::RichText::new(format!("{unread} unread"))
                            .size(theme.font_size_caption.value())
                            .color(theme.text_muted().to_egui()),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        Button::new("Mark all read")
                            .variant(ButtonVariant::Ghost)
                            .size(ControlSize::Sm)
                            .show(ui, theme);
                    });
                },
            );
            ui.separator();
            egui::ScrollArea::vertical()
                .id_salt(if empty { "notif_empty" } else { "notif_list" })
                .auto_shrink([false, false])
                .drag_to_scroll(false)
                .show(ui, |ui| {
                    if empty {
                        ui.centered_and_justified(|ui| {
                            ui.label(
                                egui::RichText::new("No notifications")
                                    .color(theme.text_muted().to_egui()),
                            );
                        });
                        return;
                    }
                    for e in ENTRIES {
                        entry_row(ui, theme, e);
                    }
                });
        },
    );
}

pub fn draw(ui: &mut egui::Ui, theme: &Theme) {
    spec::stage(ui, theme, StageVariant::Wrap, |ui| {
        spec::cluster(ui, theme, "With notifications", |ui| {
            panel(ui, theme, false)
        });
        spec::cluster(ui, theme, "Empty", |ui| panel(ui, theme, true));
    });

    spec::meta(
        ui,
        theme,
        &[
            ("frame", "350×400 · 전체화면 무대 선언 popup (fit + X)"),
            ("header", "\"{n} unread\" caption muted + Mark all read(sm)"),
            ("unread", "accent-primary 저알파 배경 + `*` 마커"),
            (
                "entry",
                "제목 / 본문 / 워크스페이스+Jump 3줄 · gap STRUCT_GAP_2",
            ),
        ],
        &[
            TokenChip::new(
                "accent-primary",
                "unread bg + marker",
                theme.accent_primary().to_egui(),
            ),
            TokenChip::new("text-muted", "time · count", theme.text_muted().to_egui()),
            TokenChip::new(
                "surface-raised",
                "popup frame",
                theme.surface_raised().to_egui(),
            ),
        ],
    );

    spec::note(
        ui,
        theme,
        "최신순으로 쌓인다. Jump 는 해당 워크스페이스로 이동하면서 그 항목을 읽음 처리한다 — \
         알림을 낸 워크스페이스가 이미 닫혔으면 출처 열이 Unknown 으로 남는다. \
         이 패널은 전체화면 무대를 선언한 유일한 popup 이라 타이틀바에 fit 버튼이 하나 더 붙는다.",
    );
}
