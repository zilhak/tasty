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
        let row = WsRowSpec::plain(name, *badge, *active, *mirror);
        let card_h = row_h + ws_row_extra_h(theme, &row);
        let card = egui::Rect::from_min_size(
            egui::pos2(rect.min.x + theme.spacing_xs.value(), y),
            egui::vec2(w - theme.spacing_xs.value() * 2.0, card_h),
        );
        paint_ws_row(ui, theme, card, &row);
        y += card_h + theme.spacing_xs.value();
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

/// 행 배경 상태. hover는 비활성 행 위에 겹치는 overlay다.
#[derive(Clone, Copy, PartialEq)]
enum RowState {
    Active,
    Inactive,
    Hover,
}

/// 워크스페이스 행 1개의 표시 내용. 본체 `draw_workspace_card`의 축을 그대로 옮긴다.
struct WsRowSpec<'a> {
    name: &'a str,
    badge: Option<&'a str>,
    state: RowState,
    busy: bool,
    attached: bool,
    mirror: bool,
    subtitle: Option<&'a str>,
}

impl<'a> WsRowSpec<'a> {
    /// 기존 데모 행. 활성 행만 실행 중으로 표시하고 attached·부제는 없다.
    fn plain(name: &'a str, badge: Option<&'a str>, active: bool, mirror: bool) -> Self {
        Self {
            name,
            badge,
            state: if active {
                RowState::Active
            } else {
                RowState::Inactive
            },
            busy: active,
            attached: false,
            mirror,
            subtitle: None,
        }
    }
}

/// 점 슬롯 중심 x와 본문 x. 슬롯은 attached 여부와 무관하게 모든 행에서 예약해
/// ring 전체(점 8 + 2×(offset + 폭))가 inset·간격 안에 들어가게 한다.
/// 슬롯 폭은 점과 ring을 각각 반올림한 뒤 더한 값이다(0.85 → 15 · 1 → 16 · 1.2 → 18).
fn ws_row_columns(theme: &Theme, card: egui::Rect) -> (f32, f32) {
    let slot_x = card.min.x + theme.workspace_row_padding_x().value();
    let slot = theme.workspace_dot_slot().value();
    (
        slot_x + slot * 0.5,
        slot_x + slot + theme.workspace_dot_gap().value(),
    )
}

/// 부제 한 줄 높이. 본체 부제와 같은 글꼴에 UI 줄간격을 적용한다.
fn subtitle_line_h(theme: &Theme) -> f32 {
    theme.sidebar_button_label_font_size.value() * theme.line_height_ui
}

/// 제목 줄 아래에 붙는 REMOTE 줄·부제 줄의 높이 합.
fn ws_row_extra_h(theme: &Theme, row: &WsRowSpec) -> f32 {
    let mut h = 0.0;
    if row.mirror {
        h += theme.spacing_xs.value() + mirror_pill_line_h(theme);
    }
    if row.subtitle.is_some() {
        h += theme.spacing_xs.value() + subtitle_line_h(theme);
    }
    h
}

/// 워크스페이스 행 1개 — 슬롯 가운데 점(+ attached ring), 이름, 배지, REMOTE 줄, 부제.
/// `card`는 제목 줄과 추가 줄을 모두 포함하며 배경·hover·accent bar가 카드 전체를 덮는다.
fn paint_ws_row(ui: &mut egui::Ui, theme: &Theme, card: egui::Rect, row: &WsRowSpec) {
    let p = ui.painter_at(card);
    match row.state {
        RowState::Active => {
            p.rect_filled(
                card,
                theme.corner_radius_sm.value(),
                egui::Color32::from(theme.surface_active()),
            );
            let bar = egui::Rect::from_min_size(
                card.min,
                egui::vec2(
                    theme.workspace_row_active_bar_width().value(),
                    card.height(),
                ),
            );
            p.rect_filled(bar, 0.0, egui::Color32::from(theme.accent_primary()));
        }
        RowState::Hover => {
            p.rect_filled(
                card,
                theme.corner_radius_sm.value(),
                theme.hover_overlay.to_egui_premultiplied(),
            );
        }
        RowState::Inactive => {}
    }
    let title = egui::Rect::from_min_size(
        card.min,
        egui::vec2(card.width(), theme.item_height_interactive.value()),
    );
    let (dot_x, name_x) = ws_row_columns(theme, card);
    let dc = egui::pos2(dot_x, title.center().y);
    let dot_r = theme.status_dot_size.value() * 0.5;
    p.circle_filled(
        dc,
        dot_r,
        egui::Color32::from(if row.busy {
            theme.accent_success()
        } else {
            theme.status_dot_idle()
        }),
    );
    // 링의 offset은 점의 바깥쪽에서 링 안쪽까지의 거리다(본체와 같은 식).
    if row.attached {
        let ring_w = theme.status_dot_attached_ring_width().value();
        p.circle_stroke(
            dc,
            dot_r + theme.status_dot_attached_ring_offset().value() + ring_w * 0.5,
            egui::Stroke::new(
                ring_w,
                egui::Color32::from(theme.status_dot_attached_ring()),
            ),
        );
    }
    let active = row.state == RowState::Active;
    p.text(
        egui::pos2(name_x, title.center().y),
        egui::Align2::LEFT_CENTER,
        row.name,
        egui::FontId::proportional(theme.font_size_body.value()),
        egui::Color32::from(if active {
            theme.text_primary()
        } else {
            theme.text_secondary()
        }),
    );
    if let Some(b) = row.badge {
        paint_ws_count_badge(&p, theme, title, b);
    }
    mirror_pill_line(ui, theme, title, name_x, row.mirror);
    if let Some(sub) = row.subtitle {
        let pill = if row.mirror {
            theme.spacing_xs.value() + mirror_pill_line_h(theme)
        } else {
            0.0
        };
        let top = title.bottom() + pill + theme.spacing_xs.value();
        p.text(
            egui::pos2(name_x, top + subtitle_line_h(theme) * 0.5),
            egui::Align2::LEFT_CENTER,
            sub,
            egui::FontId::proportional(theme.sidebar_button_label_font_size.value()),
            egui::Color32::from(theme.text_muted()),
        );
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
                let row = WsRowSpec::plain(name, *badge, *active, *mirror);
                let card_h = row_h + ws_row_extra_h(theme, &row);
                let card = egui::Rect::from_min_size(
                    egui::pos2(rect.min.x + theme.spacing_xs.value(), y),
                    egui::vec2(w - theme.spacing_xs.value() * 2.0, card_h),
                );
                paint_ws_row(ui, theme, card, &row);
                y += card_h + theme.spacing_xs.value();
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

/// attached ring 예제 행: 시안 "Attached ring in the workspace row"와 같은 세 행.
/// (이름, attached, REMOTE 줄, 부제).
const ATTACHED_ROWS: &[(&str, bool, bool, Option<&str>)] = &[
    ("second", true, false, None),
    ("api-server", false, false, None),
    ("staging", true, true, Some("ssh deploy@10.0.4.12")),
];

/// 행 여러 개를 사이드바 폭의 카드 열로 그린다.
fn ws_column(ui: &mut egui::Ui, theme: &Theme, rows: &[WsRowSpec]) {
    let w = theme.field_width_lg.value() + theme.spacing_md.value(); // 212
    let row_h = theme.item_height_interactive.value();
    let heights: Vec<f32> = rows
        .iter()
        .map(|r| row_h + ws_row_extra_h(theme, r))
        .collect();
    let h = heights.iter().sum::<f32>() + theme.spacing_xs.value() * (rows.len() as f32 + 1.0);
    let (rect, _) = ui.allocate_exact_size(egui::vec2(w, h), egui::Sense::hover());
    ui.painter_at(rect).rect_filled(
        rect,
        theme.corner_radius.value(),
        egui::Color32::from(theme.bg_sidebar()),
    );
    let mut y = rect.min.y + theme.spacing_xs.value();
    for (row, card_h) in rows.iter().zip(heights) {
        let card = egui::Rect::from_min_size(
            egui::pos2(rect.min.x + theme.spacing_xs.value(), y),
            egui::vec2(w - theme.spacing_xs.value() * 2.0, card_h),
        );
        paint_ws_row(ui, theme, card, row);
        y += card_h + theme.spacing_xs.value();
    }
}

fn attached_rows(state: RowState) -> Vec<WsRowSpec<'static>> {
    ATTACHED_ROWS
        .iter()
        .map(|(name, attached, mirror, subtitle)| WsRowSpec {
            name,
            badge: None,
            state,
            busy: true,
            attached: *attached,
            mirror: *mirror,
            subtitle: *subtitle,
        })
        .collect()
}

/// 같은 팔레트를 다른 UI 배율로 다시 만든다. 토큰은 배율을 타므로 슬롯·inset·간격이 함께 변한다.
fn with_zoom(base: &Theme, zoom: f32) -> Theme {
    Theme::with_colors_and_zoom(base.to_colors(), base.is_light, zoom)
}

/// 워크스페이스 행의 attached ring — 모든 행이 16 슬롯을 예약한다.
pub fn draw_attached_ring(ui: &mut egui::Ui, theme: &Theme) {
    let palettes = [
        ("Mocha", tasty_themes::mocha_fallback()),
        ("Latte", crate::host_shell::latte_theme()),
    ];
    // 팔레트마다 그 팔레트의 무대 배경 위에 세 상태 열을 놓는다.
    for (palette, base) in &palettes {
        let th = with_zoom(base, theme.ui_zoom);
        spec::stage(ui, &th, StageVariant::Wrap, |ui| {
            for (state, label) in [
                (RowState::Active, "active"),
                (RowState::Inactive, "inactive"),
                (RowState::Hover, "hover"),
            ] {
                spec::cluster(ui, &th, &format!("{palette} · {label}"), |ui| {
                    ws_column(ui, &th, &attached_rows(state))
                });
            }
        });
    }
    spec::stage(ui, theme, StageVariant::Wrap, |ui| {
        for key in ["small", "large"] {
            let zoom = tasty_settings::AppearanceSettings::ui_scale_factor_for(key);
            let th = with_zoom(theme, theme.ui_zoom * zoom);
            let mut rows = attached_rows(RowState::Inactive);
            rows[0].state = RowState::Active;
            rows.remove(1);
            let caption = format!(
                "ui_scale {zoom} — slot {} (derived)",
                th.workspace_dot_slot().value()
            );
            spec::cluster(ui, theme, &caption, |ui| ws_column(ui, &th, &rows));
        }
    });

    spec::meta(
        ui,
        theme,
        &[
            ("dot", "8 (status-dot-size)"),
            ("attached bbox", "8 + 2×(2+2) = 16"),
            ("row inset", "8 (workspace-row-padding-x)"),
            (
                "dot slot",
                "16, reserved on every row — derived: round(dot) + 2×(round(ring-width) + round(ring-offset))",
            ),
            (
                "slot height",
                "the title label's own line box — never grows the row",
            ),
            ("slot → body", "4 (workspace-dot-gap)"),
            ("body x", "28 (title · remote pill · subtitle)"),
            ("clearance", "card 8 · accent bar 6 · label 4"),
            (
                "active bar",
                "2, hairline, zoom-exempt (workspace-row-active-bar-width → selection-edge-width)",
            ),
            (
                "ui_scale 0.85",
                "inset 7 · slot 15 (derived) · gap 3 → label x 25 · clear card 7 · bar 5 · label 3",
            ),
            (
                "ui_scale 1.2",
                "inset 10 · slot 18 · gap 5 → label x 33 · row height unchanged",
            ),
        ],
        &[TokenChip::new(
            "status-dot-attached-ring",
            "attached ring",
            theme.status_dot_attached_ring().into(),
        )],
    );

    spec::note(
        ui,
        theme,
        "워크스페이스 행은 일반 점 8을 유지하고 attached ring까지 담는 16 슬롯을 모든 행에 예약한다. ring이 없는 행도 이름·REMOTE 줄·부제가 같은 x에서 시작한다. inset·간격은 토큰마다 반올림하고, 슬롯은 반올림한 점과 ring을 더해 만들어 ring이 항상 슬롯 안에 들어간다. 활성 행의 accent bar는 hairline이라 배율과 무관하게 2다.",
    );
}
