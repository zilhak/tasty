//! `AutoComplete` primitive specimen — 디자인 `forms/AutoComplete` 카드.
//!
//! 자유입력 트리거 + 후보 드롭다운(typeahead). 트리거=Input, 컨테이너=menu container +
//! shadow lift, 후보 행=MenuItem 언어 + middle-ellipsis + 매치 highlight. 디자인 gallery
//! Spec 의 상태 매트릭스(idle / open / filtered+highlight / overflow→scroll / empty /
//! hover·keyboard-active / 두 아이콘 컨텍스트)를 정적 인라인 드롭다운으로 전사하고, 실제
//! 합성·필터·키내비·스크롤은 하단 "interactive" 라이브 인스턴스로 노출한다.

use std::cell::RefCell;

use tasty_type_appearance::theme::Theme;
use tasty_ui_widgets::{AutoComplete, Input, MatchMode, autocomplete_dropdown};

use super::glyph;
use crate::catalog::spec::{StageVariant, TokenChip, cluster, meta, stage};

/// 최근 디렉토리 후보(explorer 컨텍스트). 뒤쪽 항목은 폭을 넘겨 middle-ellipsis 를 보여준다.
const AC_DIRS: &[&str] = &[
    "~/Downloads",
    "~/work/tasty",
    "~/work/tasty-ui/src",
    "~/work/tasty/crates/tasty-ui-widgets",
    "~/.config/tasty",
];

/// 최근 파일 후보(markdown 컨텍스트).
const AC_FILES: &[&str] = &[
    "~/work/tasty/README.md",
    "~/work/tasty/docs/architecture.md",
    "~/work/tasty/docs/design/systems/theme.md",
    "~/work/tasty/CHANGELOG.md",
];

/// "tasty" substring 필터 결과(AC_DIRS 중 매치) — filtered+highlight 상태 전사용.
const AC_TASTY: &[&str] = &[
    "~/work/tasty",
    "~/work/tasty-ui/src",
    "~/work/tasty/crates/tasty-ui-widgets",
];

/// overflow → 내부 스크롤 데모용 긴 목록(maxDropdownHeight 초과).
const AC_MANY: &[&str] = &[
    "~/work/tasty",
    "~/work/tasty-ui/src",
    "~/work/tasty/crates/tasty-core",
    "~/work/tasty/crates/tasty-ui-widgets",
    "~/work/tasty/crates/tasty-gallery",
    "~/work/tasty/docs/adr",
    "~/work/tasty/docs/design/policies",
    "~/.config/tasty",
    "~/.local/share/tasty",
    "~/Downloads/exports",
];

struct AcState {
    idle_buf: String,
    live_dirs_buf: String,
    live_dirs_active: Option<usize>,
    live_files_buf: String,
    live_files_active: Option<usize>,
}

thread_local! {
    static STATE: RefCell<Option<AcState>> = const { RefCell::new(None) };
}

pub fn draw(ui: &mut egui::Ui, theme: &Theme) {
    // 경로 필드는 넓게 — measure-sm(300) 로 긴 경로의 middle-ellipsis 를 드러낸다.
    let field_w = theme.measure_sm.value();
    let max_h_default = theme.autocomplete_max_height().value();
    let folder_icon = |ui: &mut egui::Ui, rect: egui::Rect, c: egui::Color32| {
        glyph::FOLDER_OPEN
            .image(rect.height(), c)
            .paint_at(ui, rect);
    };
    let file_icon = |ui: &mut egui::Ui, rect: egui::Rect, c: egui::Color32| {
        glyph::FILE.image(rect.height(), c).paint_at(ui, rect);
    };

    STATE.with(|s| {
        let mut slot = s.borrow_mut();
        let st = slot.get_or_insert_with(|| AcState {
            idle_buf: "~/work/tasty".to_string(),
            live_dirs_buf: "~/work/tasty".to_string(),
            live_dirs_active: None,
            live_files_buf: "~/work/tasty/README.md".to_string(),
            live_files_active: None,
        });

        stage(ui, theme, StageVariant::Column, |ui| {
            // A · IDLE — 닫힌 트리거만(mono 경로 + leading folderOpen 아이콘).
            cluster(ui, theme, "idle — closed trigger", |ui| {
                Input::new()
                    .mono(true)
                    .width(field_w)
                    .icon(&folder_icon)
                    .show(ui, theme, &mut st.idle_buf);
            });

            // B · OPEN — 전체 후보(row 0 keyboard-active, 나머지는 hover).
            cluster(
                ui,
                theme,
                "open — full candidate list · row 0 keyboard-active",
                |ui| {
                    pinned(
                        ui,
                        theme,
                        field_w,
                        "ac_open",
                        "",
                        &folder_icon,
                        AC_DIRS,
                        Some(0),
                        "",
                        true,
                        max_h_default,
                        "No matching path",
                    );
                },
            );

            // C · FILTERED + HIGHLIGHT — "tasty" 매치 구간 accent 강조.
            cluster(
                ui,
                theme,
                "typing “tasty” → filtered + highlight",
                |ui| {
                    pinned(
                        ui,
                        theme,
                        field_w,
                        "ac_filtered",
                        "tasty",
                        &folder_icon,
                        AC_TASTY,
                        Some(0),
                        "tasty",
                        true,
                        max_h_default,
                        "No matching path",
                    );
                },
            );

            // D · OVERFLOW → 내부 스크롤(maxDropdownHeight 132 초과).
            cluster(ui, theme, "overflow → internal scroll (max 132)", |ui| {
                pinned(
                    ui,
                    theme,
                    field_w,
                    "ac_scroll",
                    "",
                    &folder_icon,
                    AC_MANY,
                    Some(0),
                    "",
                    true,
                    132.0,
                    "No matching path",
                );
            });

            // E · EMPTY / no match — 단일 muted 행.
            cluster(ui, theme, "empty / no match", |ui| {
                pinned(
                    ui,
                    theme,
                    field_w,
                    "ac_empty",
                    "zzzz",
                    &folder_icon,
                    &[],
                    None,
                    "zzzz",
                    true,
                    max_h_default,
                    "No matching path",
                );
            });

            // F · HOVER vs KEYBOARD-ACTIVE — row 1 keyboard-active(진함), 나머지 hover(약함).
            cluster(
                ui,
                theme,
                "keyboard-active (row 1, surface-active) — hover others (overlay-hover)",
                |ui| {
                    pinned(
                        ui,
                        theme,
                        field_w,
                        "ac_states",
                        "",
                        &folder_icon,
                        AC_DIRS,
                        Some(1),
                        "",
                        true,
                        max_h_default,
                        "No matching path",
                    );
                },
            );

            // G · 두 아이콘 컨텍스트 — 실제 합성(라이브: 포커스→필터→키내비→스크롤).
            cluster(
                ui,
                theme,
                "explorer context — folderOpen · recent directories (interactive)",
                |ui| {
                    AutoComplete::new("ac_live_dirs")
                        .mono(true)
                        .match_mode(MatchMode::Substring)
                        .placeholder("Go to directory…")
                        .empty_label("No matching path")
                        .width(field_w)
                        .icon(&folder_icon)
                        .row_icon(&folder_icon)
                        .show(
                            ui,
                            theme,
                            &mut st.live_dirs_buf,
                            AC_DIRS,
                            &mut st.live_dirs_active,
                        );
                },
            );
            cluster(
                ui,
                theme,
                "markdown context — file · recent files (interactive)",
                |ui| {
                    AutoComplete::new("ac_live_files")
                        .mono(true)
                        .match_mode(MatchMode::Substring)
                        .placeholder("Go to file…")
                        .empty_label("No matching path")
                        .width(field_w)
                        .icon(&file_icon)
                        .row_icon(&file_icon)
                        .show(
                            ui,
                            theme,
                            &mut st.live_files_buf,
                            AC_FILES,
                            &mut st.live_files_active,
                        );
                },
            );
        });
    });

    meta(
        ui,
        theme,
        &[
            (
                "trigger",
                "Input — leading icon · mono · focus ring on open",
            ),
            ("dropdown", "menu container + shadow-popover lift"),
            ("row", "28 control-height · middle-ellipsis path"),
            ("filter", "substring (default) · prefix · none"),
            ("overflow", "scroll past max-height (220) · shrink-to-fit"),
            ("keys", "↑/↓ active · Enter pick/submit · Esc cancel"),
        ],
        &[
            TokenChip::new(
                "accent-primary",
                "match highlight",
                egui::Color32::from(theme.accent_primary()),
            ),
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
        ],
    );
}

/// 트리거(Input) + 바로 아래 정적 인라인 드롭다운을 field 폭으로 세로 적층한다(상태 pin).
/// `entries` 는 이미 필터된 가시 목록, `query` 로 highlight 를 그린다. 비면 empty 행.
#[allow(clippy::too_many_arguments)]
fn pinned(
    ui: &mut egui::Ui,
    theme: &Theme,
    field_w: f32,
    id_salt: &str,
    buf: &str,
    icon: &dyn Fn(&mut egui::Ui, egui::Rect, egui::Color32),
    entries: &[&str],
    active: Option<usize>,
    query: &str,
    highlight: bool,
    max_height: f32,
    empty_label: &str,
) {
    ui.vertical(|ui| {
        ui.set_width(field_w);
        ui.spacing_mut().item_spacing.y = 0.0;
        let mut buf = buf.to_string();
        Input::new()
            .mono(true)
            .width(field_w)
            .icon(icon)
            .show(ui, theme, &mut buf);
        ui.add_space(theme.spacing_xs.value());
        autocomplete_dropdown(
            ui,
            theme,
            id_salt,
            entries,
            empty_label,
            true,
            Some(icon),
            active,
            query,
            highlight,
            max_height,
        );
    });
}
