//! `PathField` primitive specimen — 디자인 `plugins.jsx` `PathField`(:59) 카드.
//!
//! 주소창용 편집형 경로 필드(Explorer / Markdown 공용). 구조 = AutoComplete 트리거(Input
//! 언어 + 후보 드롭다운) + 우측 Go IconButton. 디자인 두 아이콘 컨텍스트(explorer folderOpen /
//! markdown file) × idle / editing / editing+list 를 전사한다. idle/editing+list 정적 행은
//! 필드+Go 합성을 육안 고정하고, 실제 편집/포커스링/키내비/이동·원복 결정은 하단 "interactive"
//! 라이브 `PathField` 인스턴스로 노출한다.

use std::cell::RefCell;

use tasty_type_appearance::theme::Theme;
use tasty_ui_widgets::{ControlSize, IconButton, Input, PathField, autocomplete_dropdown};

use super::glyph;
use crate::catalog::spec::{StageVariant, TokenChip, cluster, meta, stage};

/// 최근 디렉토리 후보(explorer 컨텍스트).
const PF_DIRS: &[&str] = &[
    "~/Downloads",
    "~/work/tasty",
    "~/work/tasty-ui/src",
    "~/work/tasty/crates/tasty-ui-widgets",
    "~/.config/tasty",
];

/// 최근 파일 후보(markdown 컨텍스트).
const PF_FILES: &[&str] = &[
    "~/work/tasty/README.md",
    "~/work/tasty/docs/architecture.md",
    "~/work/tasty/docs/design/systems/theme.md",
    "~/work/tasty/CHANGELOG.md",
];

struct PfState {
    live_dirs_buf: String,
    live_dirs_editing: bool,
    live_dirs_active: Option<usize>,
    live_files_buf: String,
    live_files_editing: bool,
    live_files_active: Option<usize>,
}

thread_local! {
    static STATE: RefCell<Option<PfState>> = const { RefCell::new(None) };
}

pub fn draw(ui: &mut egui::Ui, theme: &Theme) {
    // 경로 필드는 넓게 — measure-sm(300) 로 긴 경로의 middle-ellipsis 를 드러낸다.
    let total_w = theme.measure_sm.value();
    let max_h = theme.autocomplete_max_height().value();
    let folder_icon = |ui: &mut egui::Ui, rect: egui::Rect, c: egui::Color32| {
        glyph::FOLDER_OPEN
            .image(rect.height(), c)
            .paint_at(ui, rect);
    };
    let file_icon = |ui: &mut egui::Ui, rect: egui::Rect, c: egui::Color32| {
        glyph::FILE.image(rect.height(), c).paint_at(ui, rect);
    };
    let go_icon = |ui: &mut egui::Ui, rect: egui::Rect, c: egui::Color32| {
        glyph::ARROW_RIGHT
            .image(rect.height(), c)
            .paint_at(ui, rect);
    };

    STATE.with(|s| {
        let mut slot = s.borrow_mut();
        let st = slot.get_or_insert_with(|| PfState {
            live_dirs_buf: "~/Downloads".to_string(),
            live_dirs_editing: false,
            live_dirs_active: None,
            live_files_buf: "~/work/tasty/README.md".to_string(),
            live_files_editing: false,
            live_files_active: None,
        });

        stage(ui, theme, StageVariant::Column, |ui| {
            // ── EXPLORER 컨텍스트 (folderOpen · recent directories) ──
            cluster(
                ui,
                theme,
                "explorer · idle — folderOpen · secondary path",
                |ui| {
                    field_row(
                        ui,
                        theme,
                        total_w,
                        "~/Downloads",
                        &folder_icon,
                        &go_icon,
                        false,
                    );
                },
            );
            cluster(
                ui,
                theme,
                "explorer · editing + list — primary + candidate dropdown",
                |ui| {
                    editing_with_list(
                        ui,
                        theme,
                        total_w,
                        "~/work/tasty",
                        "pf_exp",
                        &folder_icon,
                        &go_icon,
                        PF_DIRS,
                        max_h,
                    );
                },
            );
            cluster(
                ui,
                theme,
                "explorer · interactive — click to edit",
                |ui| {
                    PathField::new("pf_live_dirs")
                        .placeholder("Go to directory…")
                        .empty_label("No matching path")
                        .width(total_w)
                        .leading_icon(&folder_icon)
                        .row_icon(&folder_icon)
                        .go_icon(&go_icon)
                        .show(
                            ui,
                            theme,
                            &mut st.live_dirs_buf,
                            &mut st.live_dirs_editing,
                            &mut st.live_dirs_active,
                            PF_DIRS,
                            "~/Downloads",
                        );
                },
            );

            // ── MARKDOWN 컨텍스트 (file · recent files) ──
            cluster(
                ui,
                theme,
                "markdown · idle — file · secondary path",
                |ui| {
                    field_row(
                        ui,
                        theme,
                        total_w,
                        "~/work/tasty/README.md",
                        &file_icon,
                        &go_icon,
                        false,
                    );
                },
            );
            cluster(
                ui,
                theme,
                "markdown · editing + list — primary + candidate dropdown",
                |ui| {
                    editing_with_list(
                        ui,
                        theme,
                        total_w,
                        "~/work/tasty/README.md",
                        "pf_md",
                        &file_icon,
                        &go_icon,
                        PF_FILES,
                        max_h,
                    );
                },
            );
            cluster(
                ui,
                theme,
                "markdown · interactive — click to edit",
                |ui| {
                    PathField::new("pf_live_files")
                        .placeholder("Go to file…")
                        .empty_label("No matching path")
                        .width(total_w)
                        .leading_icon(&file_icon)
                        .row_icon(&file_icon)
                        .go_icon(&go_icon)
                        .show(
                            ui,
                            theme,
                            &mut st.live_files_buf,
                            &mut st.live_files_editing,
                            &mut st.live_files_active,
                            PF_FILES,
                            "~/work/tasty/README.md",
                        );
                },
            );
        });
    });

    meta(
        ui,
        theme,
        &[
            ("trigger", "AutoComplete (Input + candidate dropdown)"),
            ("trailing", "Go IconButton (sm) — arrow-right"),
            ("idle", "mono path · text-secondary"),
            ("editing", "text-primary + focus ring + caret"),
            ("keys", "Enter/Go navigate · ↑/↓ active · Esc revert"),
            ("emit", "confirmed path string only"),
        ],
        &[
            TokenChip::new(
                "input-bg",
                "field fill",
                egui::Color32::from(theme.surface_raised()),
            ),
            TokenChip::new(
                "text-secondary",
                "idle path",
                egui::Color32::from(theme.text_secondary()),
            ),
            TokenChip::new(
                "text-primary",
                "editing path",
                egui::Color32::from(theme.text_primary()),
            ),
            TokenChip::new(
                "accent-primary",
                "match highlight",
                egui::Color32::from(theme.accent_primary()),
            ),
        ],
    );
}

/// 필드 한 행 — Input(mono, idle=secondary / editing=primary) + Go IconButton. 드롭다운 없음.
/// 디자인 PathField 의 non-AutoComplete 브랜치(정적 표시) 전사. 정적 데모라 버퍼는 로컬.
fn field_row(
    ui: &mut egui::Ui,
    theme: &Theme,
    total_w: f32,
    path: &str,
    leading: &dyn Fn(&mut egui::Ui, egui::Rect, egui::Color32),
    go: &dyn Fn(&mut egui::Ui, egui::Rect, egui::Color32),
    editing: bool,
) {
    let gap = theme.spacing_sm.value();
    let go_side = ControlSize::Sm.height(theme);
    let field_w = (total_w - go_side - gap).max(0.0);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = gap;
        let mut buf = path.to_string();
        let mut field = Input::new().mono(true).width(field_w).icon(leading);
        if !editing {
            field = field.text_color(theme.text_secondary().to_egui());
        }
        field.show(ui, theme, &mut buf);
        IconButton::new().size(ControlSize::Sm).show(ui, theme, go);
    });
}

/// editing + candidates 정적 전사 — 필드 행(primary) + 바로 아래 후보 드롭다운(row 0 active).
#[allow(clippy::too_many_arguments)]
fn editing_with_list(
    ui: &mut egui::Ui,
    theme: &Theme,
    total_w: f32,
    path: &str,
    id_salt: &str,
    leading: &dyn Fn(&mut egui::Ui, egui::Rect, egui::Color32),
    go: &dyn Fn(&mut egui::Ui, egui::Rect, egui::Color32),
    candidates: &[&str],
    max_h: f32,
) {
    let gap = theme.spacing_sm.value();
    let go_side = ControlSize::Sm.height(theme);
    let field_w = (total_w - go_side - gap).max(0.0);
    ui.vertical(|ui| {
        ui.set_width(total_w);
        ui.spacing_mut().item_spacing.y = 0.0;
        field_row(ui, theme, total_w, path, leading, go, true);
        ui.add_space(theme.spacing_xs.value());
        // 드롭다운은 필드 폭(Go 제외)에 정렬 — 트리거 아래 앵커 전사.
        ui.scope(|ui| {
            ui.set_width(field_w);
            autocomplete_dropdown(
                ui,
                theme,
                id_salt,
                candidates,
                "No matching path",
                true,
                Some(leading),
                Some(0),
                "",
                true,
                max_h,
            );
        });
    });
}
