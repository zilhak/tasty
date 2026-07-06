//! `sidebar` specimen — Sidebar & rail (research §2.5 Layouts).
//!
//! 좌측 네비게이션. 두 폭:
//! - **Full 212**: 로고+워드마크 헤더 / "Workspaces" railHead / 워크스페이스 행
//!   (dot + name + badge, 활성행 surface-active + 2px inset accent) / footer
//!   (Tools·Plugins·Settings ghost 블록, 상단 border).
//! - **Collapsed rail 52**: 로고 24 + IconButton 28 슬롯들.
//!
//! Theme 토큰만으로 정적 재현 (binary 미의존).

use tasty_type_appearance::theme::Theme;

use crate::catalog::icons::{
    CHEVRON_DOWN, CHEVRON_RIGHT, FOLDER, MockGlyph, PLUG, REMOTE, SETTINGS, TERMINAL,
};
use crate::catalog::spec::{self, StageVariant, TokenChip};

/// 워크스페이스 행 데모 데이터: (name, badge, active, mirror). mirror=원격 워크스페이스
/// 로컬 mirror → 이름 앞 leading `>_→` glyph(디자인 2026-07-02 workspace-mirror-indicator).
type WsRow = (&'static str, Option<&'static str>, bool, bool);
/// 카테고리 섹션 데모 데이터: (label, collapsed, rows).
type CategorySection = (&'static str, bool, &'static [WsRow]);

/// "infra" 는 mirror + notif 배지 공존 데모(채널 분리: glyph / badge 별도 축).
const WORKSPACES: &[WsRow] = &[
    ("main", None, true, false),
    ("infra", Some("2"), false, true),
    ("agent", None, false, false),
];

/// 카테고리 그룹 데모 데이터 (디자인 데모셋: normal / Services / Archived).
/// SERVICES 의 "agent" 는 mirror — full 은 leading glyph, rail 은 아바타 우하단
/// corner chip 으로 표시.
const CATEGORY_SECTIONS: &[CategorySection] = &[
    (
        "WORKSPACES",
        false,
        &[
            ("main", None, true, false),
            ("review", Some("3"), false, false),
        ],
    ),
    ("SERVICES", false, &[("agent", None, false, true)]),
    // 빈 + 접힌 카테고리 — 헤더(chevron ▶)만.
    ("ARCHIVED", true, &[]),
];
/// footer ghost rows.
const FOOTER: &[(MockGlyph, &str)] = &[
    (TERMINAL, "Tools"),
    (PLUG, "Plugins"),
    (SETTINGS, "Settings"),
];
/// collapsed rail slots.
const RAIL_SLOTS: &[MockGlyph] = &[TERMINAL, FOLDER, PLUG, SETTINGS];

fn paint_icon(
    ui: &mut egui::Ui,
    glyph: MockGlyph,
    center: egui::Pos2,
    size: f32,
    color: egui::Color32,
) {
    let r = egui::Rect::from_center_size(center, egui::vec2(size, size));
    glyph.image(size, color).paint_at(ui, r);
}

/// Full 행 — 이름 앞 leading mirror glyph(`>_→`, workspace_mirror_fg). glyph 를
/// `name_x` 중앙에 그리고, 이름이 시작할 x(글리프 폭 + gap 만큼 우측)를 돌려준다.
/// mirror 가 아니면 `name_x` 를 그대로 반환(리플로 없음).
fn mirror_leading_glyph(
    ui: &mut egui::Ui,
    theme: &Theme,
    name_x: f32,
    cy: f32,
    mirror: bool,
) -> f32 {
    if !mirror {
        return name_x;
    }
    let sz = theme.workspace_mirror_icon_size().value();
    paint_icon(
        ui,
        REMOTE,
        egui::pos2(name_x + sz * 0.5, cy),
        sz,
        egui::Color32::from(theme.workspace_mirror_fg()),
    );
    name_x + sz + theme.workspace_mirror_gap().value()
}

/// Collapsed 아바타 우하단 mirror corner chip — bg-sidebar halo(반경 spacing_sm) +
/// 중앙 `>_→` glyph(spacing_sm, workspace_mirror_fg). notif(우상단)·attached(둘레
/// ring)와 채널 분리.
fn mirror_corner_chip(ui: &mut egui::Ui, theme: &Theme, avatar: egui::Rect) {
    let halo_r = theme.spacing_sm.value();
    let glyph = theme.spacing_sm.value();
    let inset = theme.spacing_xs.value();
    let c = egui::pos2(avatar.max.x - inset, avatar.max.y - inset);
    ui.painter()
        .circle_filled(c, halo_r, egui::Color32::from(theme.bg_sidebar()));
    paint_icon(
        ui,
        REMOTE,
        c,
        glyph,
        egui::Color32::from(theme.workspace_mirror_fg()),
    );
}

/// 워크스페이스 행 우측 개수 배지 — 본체 `paint_workspace_count_badge`(sidebar/view.rs)
/// 와 동일한 디자인 Badge variant="primary": accent-primary 채움 pill + count(mono,
/// badge-font-size), text-on-accent. min-width/height=badge-size, padding-x=badge-padding-x,
/// pill(반경=높이/2). 행 우측에 spacing-sm 인셋으로 앵커.
fn paint_ws_count_badge(p: &egui::Painter, theme: &Theme, row: egui::Rect, label: &str) {
    let size = theme.badge_size().value();
    let pad_x = theme.badge_padding_x().value();
    let galley = p.layout_no_wrap(
        label.to_string(),
        egui::FontId::monospace(theme.badge_font_size().value()),
        egui::Color32::from(theme.text_on_accent()),
    );
    let w = (galley.size().x + pad_x * 2.0).max(size);
    let badge_rect = egui::Rect::from_min_size(
        egui::pos2(
            row.max.x - theme.spacing_sm.value() - w,
            row.center().y - size * 0.5,
        ),
        egui::vec2(w, size),
    );
    p.rect_filled(
        badge_rect,
        size / 2.0,
        egui::Color32::from(theme.accent_primary()),
    );
    let gp = egui::pos2(
        badge_rect.center().x - galley.size().x / 2.0,
        badge_rect.center().y - galley.size().y / 2.0,
    );
    p.galley(gp, galley, egui::Color32::from(theme.text_on_accent()));
}

fn full(ui: &mut egui::Ui, theme: &Theme) {
    let w = theme.field_width_lg.value() + theme.spacing_md.value(); // 212
    let h = theme.spacing_xl.value() * 15.0; // 360
    let (rect, _) = ui.allocate_exact_size(egui::vec2(w, h), egui::Sense::hover());
    let p = ui.painter_at(rect);
    p.rect_filled(
        rect,
        theme.corner_radius.value(),
        egui::Color32::from(theme.bg_sidebar()),
    );

    let pad = theme.spacing_md.value(); // 12
    let row_h = theme.item_height_interactive.value(); // 28
    let mut y = rect.min.y + pad;

    // ── header: logo + wordmark ──
    let logo = theme.sidebar_logo_size.value(); // 22
    let logo_c = egui::pos2(rect.min.x + pad + logo * 0.5, y + logo * 0.5);
    paint_icon(
        ui,
        TERMINAL,
        logo_c,
        logo,
        egui::Color32::from(theme.accent_primary()),
    );
    p.text(
        egui::pos2(logo_c.x + logo * 0.5 + theme.spacing_sm.value(), logo_c.y),
        egui::Align2::LEFT_CENTER,
        "Tasty",
        egui::FontId::proportional(theme.sidebar_wordmark_font_size.value()),
        egui::Color32::from(theme.text_primary()),
    );
    y += logo + theme.spacing_xs.value() + theme.spacing_md.value();

    // ── railHead: "WORKSPACES" ──
    p.text(
        egui::pos2(rect.min.x + pad, y),
        egui::Align2::LEFT_TOP,
        "WORKSPACES",
        egui::FontId::proportional(theme.sidebar_section_heading_font_size.value()),
        egui::Color32::from(theme.text_muted()),
    );
    y += theme.spacing_lg.value();

    // ── workspace rows ──
    for (name, badge, active, mirror) in WORKSPACES {
        let row = egui::Rect::from_min_size(
            egui::pos2(rect.min.x + theme.spacing_xs.value(), y),
            egui::vec2(w - theme.spacing_xs.value() * 2.0, row_h),
        );
        if *active {
            p.rect_filled(
                row,
                theme.corner_radius_sm.value(),
                egui::Color32::from(theme.surface_active()),
            );
            // 2px inset accent bar.
            let bar = egui::Rect::from_min_size(
                row.min,
                egui::vec2(theme.focus_ring_width.value(), row.height()),
            );
            p.rect_filled(bar, 0.0, egui::Color32::from(theme.accent_primary()));
        }
        let dot_r = theme.status_dot_size.value() * 0.5;
        let dc = egui::pos2(row.min.x + theme.spacing_md.value() + dot_r, row.center().y);
        // dot 은 실행상태 전용(running=success / idle=muted). mirror 는 별도 축.
        p.circle_filled(
            dc,
            dot_r,
            egui::Color32::from(if *active {
                theme.accent_success()
            } else {
                theme.text_muted()
            }),
        );
        let name_x = dc.x + dot_r + theme.spacing_sm.value();
        // mirror 면 이름 앞 leading glyph 를 그리고 이름 시작 x 를 그만큼 우측으로.
        let text_x = mirror_leading_glyph(ui, theme, name_x, row.center().y, *mirror);
        p.text(
            egui::pos2(text_x, row.center().y),
            egui::Align2::LEFT_CENTER,
            name,
            egui::FontId::proportional(theme.font_size_body.value()),
            egui::Color32::from(if *active {
                theme.text_primary()
            } else {
                theme.text_secondary()
            }),
        );
        if let Some(b) = badge {
            paint_ws_count_badge(&p, theme, row, b);
        }
        y += row_h + theme.spacing_xs.value();
    }

    // ── footer: border-top + ghost rows (bottom-anchored) ──
    let footer_h = row_h * FOOTER.len() as f32 + pad;
    let footer_top = rect.max.y - footer_h;
    p.hline(
        rect.x_range(),
        footer_top,
        egui::Stroke::new(
            theme.border_width.value(),
            egui::Color32::from(theme.border_default()),
        ),
    );
    let mut fy = footer_top + theme.spacing_sm.value();
    for (glyph, label) in FOOTER {
        let cy = fy + row_h * 0.5;
        paint_icon(
            ui,
            *glyph,
            egui::pos2(
                rect.min.x + pad + theme.icon_glyph_size_sm.value() * 0.5,
                cy,
            ),
            theme.icon_glyph_size_sm.value(),
            egui::Color32::from(theme.text_muted()),
        );
        p.text(
            egui::pos2(
                rect.min.x + pad + theme.icon_glyph_size_sm.value() + theme.spacing_sm.value(),
                cy,
            ),
            egui::Align2::LEFT_CENTER,
            label,
            egui::FontId::proportional(theme.sidebar_button_label_font_size.value()),
            egui::Color32::from(theme.text_secondary()),
        );
        fy += row_h;
    }
}

fn rail(ui: &mut egui::Ui, theme: &Theme) {
    // 52 = collapsed slot 32 + lg 16 + xs 4.
    let w = theme.sidebar_collapsed_slot_width.value()
        + theme.spacing_lg.value()
        + theme.spacing_xs.value();
    let h = theme.spacing_xl.value() * 15.0; // 360
    let (rect, _) = ui.allocate_exact_size(egui::vec2(w, h), egui::Sense::hover());
    let p = ui.painter_at(rect);
    p.rect_filled(
        rect,
        theme.corner_radius.value(),
        egui::Color32::from(theme.bg_sidebar()),
    );

    let cx = rect.center().x;
    let mut y = rect.min.y + theme.spacing_md.value();

    // 로고 24.
    let logo = theme.sidebar_logo_collapsed_size.value(); // 24
    paint_icon(
        ui,
        TERMINAL,
        egui::pos2(cx, y + logo * 0.5),
        logo,
        egui::Color32::from(theme.accent_primary()),
    );
    y += logo + theme.spacing_md.value();

    // IconButton 28 슬롯들.
    let slot = theme.item_height_interactive.value(); // 28
    for (i, glyph) in RAIL_SLOTS.iter().enumerate() {
        let area =
            egui::Rect::from_center_size(egui::pos2(cx, y + slot * 0.5), egui::vec2(slot, slot));
        if i == 0 {
            p.rect_filled(
                area,
                theme.corner_radius_sm.value(),
                egui::Color32::from(theme.surface_active()),
            );
        }
        paint_icon(
            ui,
            *glyph,
            area.center(),
            theme.icon_glyph_size_md.value(),
            egui::Color32::from(if i == 0 {
                theme.text_primary()
            } else {
                theme.text_muted()
            }),
        );
        y += slot + theme.spacing_sm.value();
    }
}

/// 워크스페이스 행 1개(그룹 렌더용) — dot + name + optional badge, active 배경/accent bar.
fn paint_ws_row(
    ui: &mut egui::Ui,
    theme: &Theme,
    rect: egui::Rect,
    name: &str,
    badge: Option<&str>,
    active: bool,
    mirror: bool,
) {
    let p = ui.painter_at(rect);
    if active {
        p.rect_filled(
            rect,
            theme.corner_radius_sm.value(),
            egui::Color32::from(theme.surface_active()),
        );
        let bar = egui::Rect::from_min_size(
            rect.min,
            egui::vec2(theme.focus_ring_width.value(), rect.height()),
        );
        p.rect_filled(bar, 0.0, egui::Color32::from(theme.accent_primary()));
    }
    let dot_r = theme.status_dot_size.value() * 0.5;
    let dc = egui::pos2(
        rect.min.x + theme.spacing_md.value() + dot_r,
        rect.center().y,
    );
    p.circle_filled(
        dc,
        dot_r,
        egui::Color32::from(if active {
            theme.accent_success()
        } else {
            theme.text_muted()
        }),
    );
    let name_x = dc.x + dot_r + theme.spacing_sm.value();
    let text_x = mirror_leading_glyph(ui, theme, name_x, rect.center().y, mirror);
    p.text(
        egui::pos2(text_x, rect.center().y),
        egui::Align2::LEFT_CENTER,
        name,
        egui::FontId::proportional(theme.font_size_body.value()),
        egui::Color32::from(if active {
            theme.text_primary()
        } else {
            theme.text_secondary()
        }),
    );
    if let Some(b) = badge {
        paint_ws_count_badge(&p, theme, rect, b);
    }
}

/// 카테고리 그룹(확장) — chevron 헤더 + 소속 행. 접힌/빈 카테고리는 헤더만.
fn full_categories(ui: &mut egui::Ui, theme: &Theme) {
    let w = theme.field_width_lg.value() + theme.spacing_md.value(); // 212
    let h = theme.spacing_xl.value() * 15.0; // 360
    let (rect, _) = ui.allocate_exact_size(egui::vec2(w, h), egui::Sense::hover());
    let p = ui.painter_at(rect);
    p.rect_filled(
        rect,
        theme.corner_radius.value(),
        egui::Color32::from(theme.bg_sidebar()),
    );

    let pad = theme.spacing_md.value(); // 12
    let row_h = theme.item_height_interactive.value(); // 28
    let mut y = rect.min.y + pad;

    for (i, (label, collapsed, rows)) in CATEGORY_SECTIONS.iter().enumerate() {
        // 비-첫 섹션 간격 (본체 그룹 렌더의 섹션 간 add_space 와 동일 토큰).
        if i > 0 {
            y += theme.spacing_sm.value();
        }
        // ── 카테고리 헤더: chevron(▼/▶) + 대문자 캡스 라벨. 상하 spacing_xs
        // 대칭 인셋 (본체 draw_category_header 와 동일). ──
        let chevron = if *collapsed {
            CHEVRON_RIGHT
        } else {
            CHEVRON_DOWN
        };
        y += theme.spacing_xs.value();
        let ch_size = theme.icon_glyph_size_sm.value();
        let ch_c = egui::pos2(
            rect.min.x + theme.spacing_sm.value() + ch_size * 0.5,
            y + ch_size * 0.5,
        );
        paint_icon(
            ui,
            chevron,
            ch_c,
            ch_size,
            egui::Color32::from(theme.text_muted()),
        );
        p.text(
            egui::pos2(ch_c.x + ch_size * 0.5 + theme.spacing_xs.value(), ch_c.y),
            egui::Align2::LEFT_CENTER,
            label,
            egui::FontId::proportional(theme.sidebar_section_heading_font_size.value()),
            egui::Color32::from(theme.text_muted()),
        );
        y += ch_size + theme.spacing_xs.value();

        // ── 행 (접힘/빈 카테고리는 생략). ──
        if !*collapsed && !rows.is_empty() {
            // 헤더 바로 아래 1px rule (본체 목록 블록 상단 보더 — 하단 보더 없음).
            let rule = egui::Rect::from_min_size(
                egui::pos2(rect.min.x, y),
                egui::vec2(w, theme.border_width.value()),
            );
            p.rect_filled(rule, 0.0, theme.separator.to_egui_premultiplied());
            y += theme.border_width.value();
            for (name, badge, active, mirror) in *rows {
                let row = egui::Rect::from_min_size(
                    egui::pos2(rect.min.x + theme.spacing_xs.value(), y),
                    egui::vec2(w - theme.spacing_xs.value() * 2.0, row_h),
                );
                paint_ws_row(ui, theme, row, name, *badge, *active, *mirror);
                y += row_h + theme.spacing_xs.value();
            }
        }
    }
}

/// 카테고리 그룹(축소 레일) — `---` 경계 버튼 + 아바타. 접힌 카테고리는 `---` 만.
fn rail_categories(ui: &mut egui::Ui, theme: &Theme) {
    let w = theme.sidebar_collapsed_slot_width.value()
        + theme.spacing_lg.value()
        + theme.spacing_xs.value();
    let h = theme.spacing_xl.value() * 15.0; // 360
    let (rect, _) = ui.allocate_exact_size(egui::vec2(w, h), egui::Sense::hover());
    let p = ui.painter_at(rect);
    p.rect_filled(
        rect,
        theme.corner_radius.value(),
        egui::Color32::from(theme.bg_sidebar()),
    );

    let cx = rect.center().x;
    let slot = theme.item_height_interactive.value(); // 28
    let mut y = rect.min.y + theme.spacing_md.value();

    for (_label, collapsed, rows) in CATEGORY_SECTIONS {
        // `---` 경계 버튼 — 폭 slot-spacing_sm 의 얇은 선.
        let line_w = theme.sidebar_collapsed_slot_width.value() - theme.spacing_sm.value();
        let line = egui::Rect::from_center_size(
            egui::pos2(cx, y + theme.spacing_lg.value() * 0.5),
            egui::vec2(line_w, theme.border_width.value()),
        );
        p.rect_filled(line, 0.0, egui::Color32::from(theme.border_default()));
        y += theme.spacing_lg.value() + theme.spacing_xs.value();

        // 접힌 카테고리는 아바타 생략(`---` 만).
        if !*collapsed {
            for (name, _badge, active, mirror) in *rows {
                let area = egui::Rect::from_center_size(
                    egui::pos2(cx, y + slot * 0.5),
                    egui::vec2(slot, slot),
                );
                if *active {
                    p.rect_filled(
                        area,
                        theme.corner_radius_sm.value(),
                        egui::Color32::from(theme.surface_active()),
                    );
                }
                let letter = name
                    .chars()
                    .next()
                    .unwrap_or('?')
                    .to_uppercase()
                    .to_string();
                p.text(
                    area.center(),
                    egui::Align2::CENTER_CENTER,
                    letter,
                    egui::FontId::monospace(theme.font_size_body.value()),
                    egui::Color32::from(if *active {
                        theme.accent_primary()
                    } else {
                        theme.text_muted()
                    }),
                );
                // mirror 아바타 → 우하단 sky corner chip.
                if *mirror {
                    mirror_corner_chip(ui, theme, area);
                }
                y += slot + theme.spacing_sm.value();
            }
        }
    }
}

pub fn draw(ui: &mut egui::Ui, theme: &Theme) {
    spec::stage(ui, theme, StageVariant::Wrap, |ui| {
        spec::cluster(ui, theme, "Full · 212", |ui| full(ui, theme));
        spec::cluster(ui, theme, "Collapsed rail · 52", |ui| rail(ui, theme));
        spec::cluster(ui, theme, "Categories · full", |ui| {
            full_categories(ui, theme)
        });
        spec::cluster(ui, theme, "Categories · rail", |ui| {
            rail_categories(ui, theme)
        });
    });

    spec::meta(
        ui,
        theme,
        &[
            ("full width", "212"),
            ("rail width", "52"),
            ("logo", "22 full / 24 rail"),
            ("row", "dot + name + badge"),
            ("active row", "surface-active + 2px inset accent"),
            ("mirror", "leading >_→ glyph / rail corner chip"),
            ("footer", "Tools·Plugins·Settings, border-top"),
        ],
        &[
            TokenChip::new("bg-sidebar", "sidebar fill", theme.bg_sidebar().into()),
            TokenChip::new(
                "surface-active",
                "active row",
                theme.surface_active().into(),
            ),
            TokenChip::new(
                "accent-primary",
                "inset bar + logo",
                theme.accent_primary().into(),
            ),
            TokenChip::new(
                "workspace-mirror-fg",
                "mirror glyph/chip",
                theme.workspace_mirror_fg().into(),
            ),
            TokenChip::new(
                "border-default",
                "footer divider",
                theme.border_default().into(),
            ),
        ],
    );

    spec::note(
        ui,
        theme,
        "Full 은 워크스페이스를 이름·badge 까지 펼치고, 접으면 52px rail 로 줄어 \
         아이콘 슬롯만 남는다. 활성 행은 surface-active + 좌측 2px accent 로 표시. \
         원격 워크스페이스 로컬 mirror 는 status dot(실행상태)과 별개로 — full 은 이름 앞 \
         leading `>_→` glyph, rail 은 아바타 우하단 sky corner chip(workspace-mirror-fg)으로 \
         표시(notif 우상단 / attached 둘레 ring 과 채널 분리). \
         카테고리 토글 on 이면 chevron 헤더로 그룹화(빈·접힌 카테고리는 헤더/`---` 만), \
         레일은 카테고리 경계를 `---` 버튼으로 표시한다.",
    );
}
