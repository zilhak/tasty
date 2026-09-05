//! `notifications` specimen — 알림 패널 popup (Overlays).
//!
//! 본체 `src/adapters/ui/notification.rs::draw_notification_content_inner` 의
//! 구조 전사. `popup/defs.rs` 의 `notifications` 정의는 350×400 이고, **전체화면
//! 무대를 선언한 유일한 popup** 이라 타이틀바에 fullscreen 버튼이 X 왼쪽에 붙는다.
//!
//! 세로 구성:
//! 1. **헤더 행** — 좌측 `"{n} unread"`(caption, `text_muted`), 우측 정렬
//!    `Mark all read` 버튼(sm).
//! 2. **separator**.
//! 3. **목록** — 본체와 같이 `ScrollArea`(auto_shrink 없음)에 담아 패널 밖으로
//!    흘러나가지 않는다. 항목 없으면 중앙에 muted `No notifications`.
//!    항목 하나는 `Frame`(unread 면 `accent-primary` 저알파 배경, read 면 투명)
//!    + `spacing_xs` inner margin + `corner_radius` 이고 그 안이
//!      `[* ] 제목 … 경과시간` / `본문` / `워크스페이스명 + Jump` 3줄이다.
//!      항목 사이 간격은 `STRUCT_GAP_2`.
//!
//! **토큰 이관 2건** (값 보존):
//! - 본체 unread 배경은 primitive `theme().blue.with_alpha(20)` 직접 접근이다 →
//!   specimen 은 semantic `accent_primary()`(= 같은 blue) 로 읽는다.
//! - 본체 `ui.small_button` → 공용 `Button`(`ControlSize::Sm`).

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
            // 본체는 `ui.horizontal` 이지만 그 안의 right_to_left 가 남은 세로를
            // 전부 차지해(centered) 행이 패널 높이만큼 부푼다. 행 높이를 묶는다.
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
            // 본체와 같이 목록만 스크롤 영역에 담는다 — 패널 밖으로 흘러나가지 않는다.
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
