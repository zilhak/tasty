//! `Combobox` primitive specimen — 디자인 combobox / autocomplete 카드.
//!
//! 편집형 입력 + 최근 항목 드롭다운(브라우저 주소창형). 트리거=Input, 컨테이너=menu
//! container + shadow lift, 후보 행=MenuItem 언어 + middle-ellipsis. 상태를 육안
//! 검증하기 위해 open/empty 는 정적 인라인 드롭다운으로, 실제 합성·키내비는 하단
//! "interactive" 라이브 인스턴스로 노출한다.

use std::cell::RefCell;

use tasty_type_appearance::theme::Theme;
use tasty_ui_widgets::{Combobox, Input, combobox_dropdown};

use super::glyph;
use crate::catalog::spec::{StageVariant, TokenChip, cluster, meta, stage};

/// 최근 파일 데모(최신순). 3번째 항목은 폭을 넘겨 middle-ellipsis 를 보여준다.
const RECENT: &[&str] = &[
    "/home/user/notes/readme.md",
    "/home/user/projects/tasty/CLAUDE.md",
    "/var/log/system/deeply/nested/path/that/overflows/access.log",
    "~/Documents/todo.md",
];

struct ComboState {
    live_buf: String,
    live_active: Option<usize>,
    closed_buf: String,
    open_buf: String,
    empty_buf: String,
    prop_buf: String,
}

thread_local! {
    static STATE: RefCell<Option<ComboState>> = const { RefCell::new(None) };
}

pub fn draw(ui: &mut egui::Ui, theme: &Theme) {
    // 경로 필드는 넓게 — measure-sm(300) 로 긴 경로의 middle-ellipsis 를 드러낸다.
    let field_w = theme.measure_sm.value();
    let file_icon = |ui: &mut egui::Ui, rect: egui::Rect, c: egui::Color32| {
        glyph::FILE.image(rect.height(), c).paint_at(ui, rect);
    };

    STATE.with(|s| {
        let mut slot = s.borrow_mut();
        let st = slot.get_or_insert_with(|| ComboState {
            live_buf: "/home/user/notes/readme.md".to_string(),
            live_active: None,
            closed_buf: "/home/user/projects/tasty/CLAUDE.md".to_string(),
            open_buf: "/home/user/notes/".to_string(),
            empty_buf: "no-match".to_string(),
            prop_buf: "Choose a recent entry".to_string(),
        });

        stage(ui, theme, StageVariant::Column, |ui| {
            // A · CLOSED (idle) — 트리거만(mono 경로 + leading 파일 아이콘).
            cluster(ui, theme, "closed (idle) · mono path", |ui| {
                Input::new()
                    .mono(true)
                    .width(field_w)
                    .icon(&file_icon)
                    .show(ui, theme, &mut st.closed_buf);
            });

            // B · OPEN (editing) — 트리거 + 정적 드롭다운(row 0 keyboard-active).
            cluster(
                ui,
                theme,
                "open · history — row 0 keyboard-active, hover the rest",
                |ui| {
                    trigger_and_panel(ui, theme, field_w, &mut st.open_buf, &file_icon, RECENT);
                },
            );

            // C · OPEN — empty(후보 0개 → "No recent files" 단일 muted 행).
            cluster(ui, theme, "open · empty", |ui| {
                trigger_and_panel(ui, theme, field_w, &mut st.empty_buf, &file_icon, &[]);
            });

            // 라이브 — 클릭해 포커스하면 실제 트리거+floating 드롭다운(focus ring·키내비).
            cluster(ui, theme, "interactive — click to focus", |ui| {
                Combobox::new("gallery-combobox")
                    .mono(true)
                    .placeholder("Path…")
                    .empty_label("No recent files")
                    .width(field_w)
                    .icon(&file_icon)
                    .row_icon(&file_icon)
                    .show(ui, theme, &mut st.live_buf, RECENT, &mut st.live_active);
            });

            // proportional body(13) 변형 — 일반 combobox(경로 아닌 용례).
            cluster(ui, theme, "proportional body variant", |ui| {
                ui.vertical(|ui| {
                    ui.set_width(field_w);
                    ui.spacing_mut().item_spacing.y = 0.0;
                    Input::new()
                        .width(field_w)
                        .placeholder("Choose…")
                        .show(ui, theme, &mut st.prop_buf);
                    ui.add_space(theme.spacing_xs.value());
                    let opts = ["Recent · main.rs", "Recent · lib.rs", "Recent · Cargo.toml"];
                    combobox_dropdown(ui, theme, &opts, "No recent files", false, None, Some(1));
                });
            });
        });
    });

    meta(
        ui,
        theme,
        &[
            ("trigger", "Input — 28 control-height, mono caption"),
            ("dropdown", "menu container + shadow-popover lift"),
            ("row", "28 control-height · middle-ellipsis path"),
            ("keys", "↑/↓ active · Enter pick/submit · Esc cancel"),
        ],
        &[
            TokenChip::new(
                "surface-active",
                "keyboard-active row",
                egui::Color32::from(theme.surface_active()),
            ),
            TokenChip::new(
                "overlay-hover",
                "pointer hover row",
                egui::Color32::from(theme.overlay_hover()),
            ),
            TokenChip::new(
                "surface-raised",
                "dropdown fill",
                egui::Color32::from(theme.surface_raised()),
            ),
            TokenChip::new(
                "text-secondary",
                "idle path",
                egui::Color32::from(theme.text_secondary()),
            ),
        ],
    );
}

/// 트리거(Input) + 바로 아래 정적 인라인 드롭다운을 field 폭으로 세로 적층한다.
/// `entries` 가 비면 empty 행. 정적 데모라 row 0 을 keyboard-active 로 고정한다.
fn trigger_and_panel(
    ui: &mut egui::Ui,
    theme: &Theme,
    field_w: f32,
    buf: &mut String,
    file_icon: &dyn Fn(&mut egui::Ui, egui::Rect, egui::Color32),
    entries: &[&str],
) {
    ui.vertical(|ui| {
        ui.set_width(field_w);
        ui.spacing_mut().item_spacing.y = 0.0;
        Input::new()
            .mono(true)
            .width(field_w)
            .icon(file_icon)
            .show(ui, theme, buf);
        ui.add_space(theme.spacing_xs.value());
        let active = (!entries.is_empty()).then_some(0);
        combobox_dropdown(
            ui,
            theme,
            entries,
            "No recent files",
            true,
            Some(file_icon),
            active,
        );
    });
}
