//! 펼친 사이드바와 접힌 레일의 정적 예제. 워크스페이스·카테고리·상태 배지를 비교한다.

use tasty_type_appearance::theme::Theme;
use tasty_ui_widgets::{TagVariant, tag};

use crate::catalog::icons::{
    CHEVRON_DOWN, CHEVRON_RIGHT, FOLDER, MockGlyph, PLUG, REMOTE, SETTINGS, TERMINAL,
};
use crate::catalog::spec::{self, StageVariant, TokenChip};

/// (이름, 배지, 활성 여부, 원격 미러 여부). 원격 미러는 이름 아래 별도 줄에 표시한다.
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
/// SERVICES 의 "agent" 는 mirror — full 은 이름 아래 별도 줄의 "REMOTE" pill, rail 은
/// 아바타 우하단 corner chip 으로 표시.
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

/// mirror 행의 pill 줄 높이(추가 행 높이 산정용) — 아이콘/pill 높이 중 큰 값.
fn mirror_pill_line_h(theme: &Theme) -> f32 {
    theme
        .tag_size()
        .value()
        .max(theme.workspace_mirror_icon_size().value())
}

/// 원격 미러는 이름 아래에 REMOTE 배지를 표시한다. 호출자가 추가 행 높이를 확보해야 한다.
fn mirror_pill_line(ui: &mut egui::Ui, theme: &Theme, row: egui::Rect, name_x: f32, mirror: bool) {
    if !mirror {
        return;
    }
    let y = row.bottom() + theme.spacing_xs.value();
    let sz = theme.workspace_mirror_icon_size().value();
    let cy = y + mirror_pill_line_h(theme) * 0.5;
    paint_icon(
        ui,
        REMOTE,
        egui::pos2(name_x + sz * 0.5, cy),
        sz,
        egui::Color32::from(theme.workspace_mirror_fg()),
    );
    let mut tag_ui = ui.new_child(egui::UiBuilder::new().max_rect(egui::Rect::from_min_size(
        egui::pos2(name_x + sz + theme.workspace_mirror_gap().value(), y),
        egui::vec2(ui.available_width(), mirror_pill_line_h(theme)),
    )));
    tag(&mut tag_ui, theme, "REMOTE", TagVariant::Remote, false);
}

/// 접힌 아바타의 원격 표시는 오른쪽 아래에 놓아 알림 점·연결 테두리와 구분한다.
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

/// 오른쪽 끝에서 offset만큼 떨어진 곳에 개수 배지를 그린다.
/// 반환한 폭은 다음 배지를 배치할 때 사용한다.
fn paint_ws_count_badge_at(
    p: &egui::Painter,
    theme: &Theme,
    row: egui::Rect,
    right_offset: f32,
    label: &str,
    fill: egui::Color32,
) -> f32 {
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
            row.max.x - theme.spacing_sm.value() - right_offset - w,
            row.center().y - size * 0.5,
        ),
        egui::vec2(w, size),
    );
    p.rect_filled(badge_rect, size / 2.0, fill);
    let gp = egui::pos2(
        badge_rect.center().x - galley.size().x / 2.0,
        badge_rect.center().y - galley.size().y / 2.0,
    );
    p.galley(gp, galley, egui::Color32::from(theme.text_on_accent()));
    w
}

/// Completion(파랑) 단일 배지 — 기존 데모 행(`WORKSPACES`/`CATEGORY_SECTIONS`)이
/// 쓰는 단순 형태.
fn paint_ws_count_badge(p: &egui::Painter, theme: &Theme, row: egui::Rect, label: &str) {
    paint_ws_count_badge_at(
        p,
        theme,
        row,
        0.0,
        label,
        egui::Color32::from(theme.accent_primary()),
    );
}

/// NeedsInput(좌, 노랑) + Completion(우, 파랑) 배지 쌍 — 디자인 확정: 트레일링
/// 슬롯(우측)은 kind 와 무관하게 유지, 2개면 NeedsInput 이 앞(좌측)·Completion 이
/// 뒤(우측, 기존 자리), 사이 간격 `badge-group-gap`(=`spacing_xs`).
fn paint_ws_badge_pair(
    p: &egui::Painter,
    theme: &Theme,
    row: egui::Rect,
    needs_input_label: &str,
    completion_label: &str,
) {
    let completion_w = paint_ws_count_badge_at(
        p,
        theme,
        row,
        0.0,
        completion_label,
        egui::Color32::from(theme.accent_primary()),
    );
    paint_ws_count_badge_at(
        p,
        theme,
        row,
        completion_w + theme.spacing_xs.value(),
        needs_input_label,
        egui::Color32::from(theme.accent_warning()),
    );
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

    p.text(
        egui::pos2(rect.min.x + pad, y),
        egui::Align2::LEFT_TOP,
        "WORKSPACES",
        egui::FontId::proportional(theme.sidebar_section_heading_font_size.value()),
        egui::Color32::from(theme.text_muted()),
    );
    y += theme.spacing_lg.value();

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
            let bar = egui::Rect::from_min_size(
                row.min,
                egui::vec2(theme.tab_indicator_width.value(), row.height()),
            );
            p.rect_filled(bar, 0.0, egui::Color32::from(theme.accent_primary()));
        }
        let dot_r = theme.status_dot_size.value() * 0.5;
        let dc = egui::pos2(row.min.x + theme.spacing_md.value() + dot_r, row.center().y);
        // 실행 상태 점과 원격 연결 표시는 별개다.
        p.circle_filled(
            dc,
            dot_r,
            egui::Color32::from(if *active {
                theme.accent_success()
            } else {
                theme.status_dot_idle()
            }),
        );
        let name_x = dc.x + dot_r + theme.spacing_sm.value();
        p.text(
            egui::pos2(name_x, row.center().y),
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
        mirror_pill_line(ui, theme, row, name_x, *mirror);
        let extra = if *mirror {
            theme.spacing_xs.value() + mirror_pill_line_h(theme)
        } else {
            0.0
        };
        y += row_h + extra + theme.spacing_xs.value();
    }

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

    let logo = theme.sidebar_logo_collapsed_size.value(); // 24
    paint_icon(
        ui,
        TERMINAL,
        egui::pos2(cx, y + logo * 0.5),
        logo,
        egui::Color32::from(theme.accent_primary()),
    );
    y += logo + theme.spacing_md.value();

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
            // 비활성 레일 아이콘은 본체와 같은 glyph-dim 색을 쓴다.
            egui::Color32::from(if i == 0 {
                theme.text_primary()
            } else {
                theme.glyph_dim()
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
            egui::vec2(theme.tab_indicator_width.value(), rect.height()),
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
            theme.status_dot_idle()
        }),
    );
    let name_x = dc.x + dot_r + theme.spacing_sm.value();
    p.text(
        egui::pos2(name_x, rect.center().y),
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
    mirror_pill_line(ui, theme, rect, name_x, mirror);
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
        if i > 0 {
            y += theme.spacing_md.value();
        }
        let chevron = if *collapsed {
            CHEVRON_RIGHT
        } else {
            CHEVRON_DOWN
        };
        let pad_y = theme.sidebar_category_header_pad_y().value();
        let pad_x = theme.sidebar_category_header_pad_x().value();
        let ch_size = theme.icon_glyph_size_sm.value();
        let header_h = pad_y + ch_size + pad_y;
        let header_rect =
            egui::Rect::from_min_size(egui::pos2(rect.min.x, y), egui::vec2(w, header_h));
        p.rect_filled(
            header_rect,
            0.0,
            theme.sidebar_category_header_bg().to_egui_premultiplied(),
        );
        let border = theme
            .sidebar_category_header_border()
            .to_egui_premultiplied();
        let border_w = theme.border_width.value();
        p.hline(
            header_rect.x_range(),
            header_rect.min.y,
            egui::Stroke::new(border_w, border),
        );
        p.hline(
            header_rect.x_range(),
            header_rect.max.y,
            egui::Stroke::new(border_w, border),
        );
        y += pad_y;
        let fg = egui::Color32::from(theme.sidebar_category_header_fg());
        let ch_c = egui::pos2(rect.min.x + pad_x + ch_size * 0.5, y + ch_size * 0.5);
        paint_icon(ui, chevron, ch_c, ch_size, fg);
        p.text(
            egui::pos2(ch_c.x + ch_size * 0.5 + theme.spacing_xs.value(), ch_c.y),
            egui::Align2::LEFT_CENTER,
            label,
            egui::FontId::proportional(theme.sidebar_section_heading_font_size.value()),
            fg,
        );
        p.text(
            egui::pos2(rect.max.x - pad_x, ch_c.y),
            egui::Align2::RIGHT_CENTER,
            rows.len().to_string(),
            egui::FontId::monospace(theme.sidebar_category_header_count_font_size().value()),
            egui::Color32::from(theme.sidebar_category_header_count_fg()),
        );
        y += ch_size + pad_y;

        // 헤더가 이미 아래쪽 선을 그리므로 구분선을 덧그리지 않는다.
        if !*collapsed && !rows.is_empty() {
            for (name, badge, active, mirror) in *rows {
                let row = egui::Rect::from_min_size(
                    egui::pos2(rect.min.x + theme.spacing_xs.value(), y),
                    egui::vec2(w - theme.spacing_xs.value() * 2.0, row_h),
                );
                paint_ws_row(ui, theme, row, name, *badge, *active, *mirror);
                let extra = if *mirror {
                    theme.spacing_xs.value() + mirror_pill_line_h(theme)
                } else {
                    0.0
                };
                y += row_h + extra + theme.spacing_xs.value();
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
        let line_w = theme.sidebar_collapsed_slot_width.value() - theme.spacing_sm.value();
        let line = egui::Rect::from_center_size(
            egui::pos2(cx, y + theme.spacing_lg.value() * 0.5),
            egui::vec2(line_w, theme.border_width.value()),
        );
        p.rect_filled(line, 0.0, egui::Color32::from(theme.border_default()));
        y += theme.spacing_lg.value() + theme.spacing_xs.value();

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
                if *mirror {
                    mirror_corner_chip(ui, theme, area);
                }
                y += slot + theme.spacing_sm.value();
            }
        }
    }
}

/// Attention kind 데모 — 워크스페이스 행 배지(NeedsInput 단독 / 배지 2종 공존) +
/// collapsed rail dot(kind 우선순위: NeedsInput 노랑 > Completion 파랑 > running 초록).
/// 본체 `sidebar/view.rs::draw_workspace_card`/`draw_collapsed_avatar` 의 kind 분기를
/// theme 토큰만으로 정적 재현.
fn attention_demo(ui: &mut egui::Ui, theme: &Theme) {
    let w = theme.field_width_lg.value() + theme.spacing_md.value(); // 212
    let row_h = theme.item_height_interactive.value();
    let rows = 2.0;
    let h = row_h * rows + theme.spacing_xs.value() * (rows - 1.0) + theme.spacing_md.value();
    let (rect, _) = ui.allocate_exact_size(egui::vec2(w, h), egui::Sense::hover());
    let p = ui.painter_at(rect);
    p.rect_filled(
        rect,
        theme.corner_radius.value(),
        egui::Color32::from(theme.bg_sidebar()),
    );

    let mut y = rect.min.y + theme.spacing_sm.value();
    for (name, needs_input, completion) in [
        ("review-agent", Some("1"), None),
        ("deploy", Some("2"), Some("3")),
    ] {
        let row = egui::Rect::from_min_size(
            egui::pos2(rect.min.x + theme.spacing_xs.value(), y),
            egui::vec2(w - theme.spacing_xs.value() * 2.0, row_h),
        );
        p.text(
            egui::pos2(row.min.x + theme.spacing_md.value(), row.center().y),
            egui::Align2::LEFT_CENTER,
            name,
            egui::FontId::proportional(theme.font_size_body.value()),
            egui::Color32::from(theme.text_secondary()),
        );
        match (needs_input, completion) {
            (Some(ni), Some(c)) => paint_ws_badge_pair(&p, theme, row, ni, c),
            (Some(ni), None) => {
                paint_ws_count_badge_at(
                    &p,
                    theme,
                    row,
                    0.0,
                    ni,
                    egui::Color32::from(theme.accent_warning()),
                );
            }
            (None, Some(c)) => {
                paint_ws_count_badge(&p, theme, row, c);
            }
            (None, None) => {}
        }
        y += row_h + theme.spacing_xs.value();
    }
}

/// Collapsed rail avatar dot — kind 우선순위(needs-input > completion > running)
/// 데모 3종.
fn attention_rail_demo(ui: &mut egui::Ui, theme: &Theme) {
    let slot = theme.sidebar_collapsed_slot_width.value();
    let w = slot * 3.0 + theme.spacing_md.value() * 2.0;
    let h = theme.sidebar_collapsed_workspace_height.value();
    let (rect, _) = ui.allocate_exact_size(egui::vec2(w, h), egui::Sense::hover());
    let p = ui.painter_at(rect);
    p.rect_filled(
        rect,
        theme.corner_radius.value(),
        egui::Color32::from(theme.bg_sidebar()),
    );
    // 접힌 rail 은 24px 크롬 계열 — 점 가족 규칙상 compact 6(본체와 같은 접근자).
    let dot_r = theme.status_dot_size_compact().value() * 0.5;
    let dot_pad = theme.spacing_xs.value();
    for (i, (letter, dot_color)) in [
        ('N', theme.accent_warning()),
        ('C', theme.accent_primary()),
        ('R', theme.accent_success()),
    ]
    .into_iter()
    .enumerate()
    {
        let cx = rect.min.x
            + theme.spacing_md.value()
            + slot * 0.5
            + (slot + theme.spacing_md.value()) * i as f32;
        let avatar =
            egui::Rect::from_center_size(egui::pos2(cx, rect.center().y), egui::vec2(slot, slot));
        p.text(
            avatar.center(),
            egui::Align2::CENTER_CENTER,
            letter.to_string(),
            egui::FontId::monospace(theme.font_size_body.value()),
            egui::Color32::from(theme.text_muted()),
        );
        let dot_center = egui::pos2(
            avatar.max.x - dot_pad - dot_r,
            avatar.min.y + dot_pad + dot_r,
        );
        p.circle_filled(
            dot_center,
            dot_r + 1.5,
            egui::Color32::from(theme.bg_sidebar()),
        );
        p.circle_filled(dot_center, dot_r, egui::Color32::from(dot_color));
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
        spec::cluster(ui, theme, "Attention badges", |ui| {
            attention_demo(ui, theme)
        });
        spec::cluster(ui, theme, "Attention rail dot", |ui| {
            attention_rail_demo(ui, theme)
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
            ("mirror", "REMOTE pill line / rail corner chip"),
            ("footer", "Tools·Plugins·Settings, border-top"),
            (
                "attention badges",
                "NeedsInput(yellow, left) · Completion(blue, right, 기존 자리)",
            ),
            (
                "attention rail dot",
                "needs-input > completion > running (kind 우선순위, dot 1개)",
            ),
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
                "inset bar + logo + completion badge/dot",
                theme.accent_primary().into(),
            ),
            TokenChip::new(
                "accent-warning",
                "needs-input badge/dot",
                theme.accent_warning().into(),
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
        "펼친 사이드바는 이름과 배지를 표시하고 접힌 레일은 아이콘 중심으로 보여준다. 원격 미러는 펼친 행의 REMOTE 배지 또는 아바타 오른쪽 아래 표시로 구분한다. 카테고리에는 접기 버튼이 있으며 접힌 레일에서는 경계선으로 표시한다. 응답 대기 배지는 완료 배지 왼쪽에 놓인다. 레일의 점 하나는 응답 대기, 완료, 실행 순서로 대표 상태를 표시한다.",
    );
}
