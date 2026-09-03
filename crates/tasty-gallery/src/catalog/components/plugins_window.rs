//! `plugins-window` specimen — Plugins 관리자 창 (Overlays).
//!
//! 본체 `src/view/plugins/ui.rs::draw_plugins_panel` + `ui/list.rs::draw_list_tab`
//! 의 구조 전사. 그 함수는 이미 props 분리(`PluginsSnapshot` / `PluginsUiState` /
//! `Vec<PluginsAction>`)가 끝나 `AppState`/`CoreState` 를 모르지만, 글로벌
//! `theme::theme()` 를 읽고 `TopBottomPanel`/`SidePanel` 을 `Context` 에 직접
//! 붙이므로 갤러리가 호출할 수는 없다 — 같은 구조를 rect 기준으로 복제한다.
//!
//! 가로 3열, 세로 2단:
//! - **헤더 밴드**(높이 48) — plug 아이콘 + 타이틀 + 1px 세로 구분선 +
//!   세그먼트 탭 3개(`Installed N` / `Attention N` / `Add plugin`), 우측 클러스터는
//!   오른쪽부터 X 닫기 → 필터 입력(Installed 탭에서만).
//! - **좌측 목록**(폭 240) — 행 높이 40 의 2줄 행(이름 13 / 부제 10 muted).
//!   builtin 은 이름 뒤 `•`, 비활성/실행중은 부제에 `·` 로 이어 붙는다.
//!   health error + enabled 인 행만 우측에 danger dot.
//! - **우측 상세** — 이름 + 버전 tag + built-in 배지, id(muted), 설명,
//!   (health error 면) danger 박스, authors/homepage.
//!
//! **토큰 이관** (구조 보존, 값은 가장 가까운 토큰으로): 헤더 48 =
//! `item_height_interactive + spacing_lg + spacing_xs`, 아이콘 17 →
//! `icon_glyph_size_md`, 타이틀 14 → `font_size_max`, 구분선 20 →
//! `spacing_lg + spacing_xs`, 닫기 28 → `item_height_interactive`, 필터 200 →
//! `field_width_lg`, 세그먼트 12.5/9.5/10.5 → `font_size_body`/`font_size_micro`,
//! 행 40 → `item_height_interactive + spacing_md`, 이름 13 → `font_size_body`,
//! 부제 10 → `font_size_micro`.

use tasty_type_appearance::theme::Theme;
use tasty_ui_widgets::tokens::STRUCT_GAP_2;
use tasty_ui_widgets::{TagVariant, tag};

use crate::catalog::icons::{CLOSE, PLUG};
use crate::catalog::spec::{self, StageVariant, TokenChip};

/// 본체 `SidePanel::left("plugins_list").exact_width`.
const LIST_W: f32 = 240.0;

/// 창 데모 무대 크기 — 본체 Plugins 창은 `TopBottomPanel`/`SidePanel` 조합으로 창
/// 전체를 채우므로 전사할 고정 크기가 없다. 그래서 무대는 갤러리가 정하되, 값을
/// 새로 짓지 않고 토큰으로 조립한다.
///
/// - 폭 = `LIST_W`(본체 목록 폭) + `measure_md` — 상세 컬럼을 본문 측정 토큰 하나로
///   잡으면 목록/상세 경계가 무대 폭에 종속되지 않는다.
/// - 높이 = `measure_sm` — 헤더(48) + 40px 행 3개가 잘리지 않는 가장 작은 측정 토큰.
fn stage_size(theme: &Theme) -> egui::Vec2 {
    egui::vec2(LIST_W + theme.measure_md.value(), theme.measure_sm.value())
}

/// 목록 행 하나의 표시 데이터 — 본체 `PluginsSnapshot.plugins` 항목과 동형.
struct Row {
    name: &'static str,
    version: &'static str,
    builtin: bool,
    enabled: bool,
    running: bool,
    health_error: bool,
}

const ROWS: &[Row] = &[
    Row {
        name: "Clipboard viewer",
        version: "0.4.2",
        builtin: true,
        enabled: true,
        running: true,
        health_error: false,
    },
    Row {
        name: "Git viewer",
        version: "0.3.1",
        builtin: true,
        enabled: true,
        running: false,
        health_error: true,
    },
    Row {
        name: "Markdown",
        version: "0.9.0",
        builtin: false,
        enabled: false,
        running: false,
        health_error: false,
    },
];

/// 한 세그먼트 탭의 표시 상태 — 본체 `segment_tab` 의 뒤쪽 인자 4개를 묶은 것.
struct Segment<'a> {
    label: &'a str,
    count: Option<usize>,
    danger: bool,
    selected: bool,
}

/// 본체 `segment_tab` 전사 — 라벨 + (count 또는 danger 배지), selected 면 채운 배경.
fn segment_tab(
    ui: &mut egui::Ui,
    theme: &Theme,
    p: &egui::Painter,
    at: egui::Pos2,
    seg: &Segment<'_>,
) -> f32 {
    let Segment {
        label,
        count,
        danger,
        selected,
    } = *seg;
    let label_color = if selected {
        theme.text_primary().to_egui()
    } else {
        theme.text_muted().to_egui()
    };
    let count_color = if selected {
        theme.text_secondary().to_egui()
    } else {
        theme.text_muted().to_egui()
    };
    let label_font = egui::FontId::proportional(theme.font_size_body.value());
    let label_galley = p.layout_no_wrap(label.to_string(), label_font, label_color);

    let badge = danger && count.is_some_and(|c| c > 0);
    let badge_galley = badge.then(|| {
        p.layout_no_wrap(
            count.unwrap_or(0).to_string(),
            egui::FontId::proportional(theme.font_size_micro.value()),
            theme.text_on_accent().to_egui(),
        )
    });
    let count_galley = (!danger)
        .then(|| {
            count.map(|c| {
                p.layout_no_wrap(
                    c.to_string(),
                    egui::FontId::monospace(theme.font_size_micro.value()),
                    count_color,
                )
            })
        })
        .flatten();

    let pad_x = theme.spacing_md.value();
    let gap = theme.spacing_sm.value();
    let height = theme.item_height_tab.value() + STRUCT_GAP_2.value();
    let badge_h = theme.spacing_lg.value();
    let badge_pad = theme.spacing_xs.value();

    let mut width = label_galley.size().x + pad_x * 2.0;
    if let Some(g) = &count_galley {
        width += gap + g.size().x;
    }
    if let Some(g) = &badge_galley {
        width += gap + (g.size().x + badge_pad * 2.0).max(badge_h);
    }

    let rect = egui::Rect::from_min_size(at, egui::vec2(width, height));
    if selected {
        p.rect(
            rect,
            theme.corner_radius.value(),
            theme.surface_active().to_egui(),
            egui::Stroke::new(theme.border_width.value(), theme.border_default().to_egui()),
            egui::StrokeKind::Inside,
        );
    }

    let mut x = rect.min.x + pad_x;
    p.galley(
        egui::pos2(x, rect.center().y - label_galley.size().y * 0.5),
        label_galley.clone(),
        label_color,
    );
    x += label_galley.size().x;
    if let Some(g) = count_galley {
        x += gap;
        p.galley(
            egui::pos2(x, rect.center().y - g.size().y * 0.5),
            g,
            count_color,
        );
    }
    if let Some(g) = badge_galley {
        x += gap;
        let bw = (g.size().x + badge_pad * 2.0).max(badge_h);
        let brect = egui::Rect::from_min_size(
            egui::pos2(x, rect.center().y - badge_h * 0.5),
            egui::vec2(bw, badge_h),
        );
        p.rect_filled(
            brect,
            theme.corner_radius.value(),
            theme.accent_danger().to_egui(),
        );
        p.galley(
            egui::pos2(
                brect.center().x - g.size().x * 0.5,
                brect.center().y - g.size().y * 0.5,
            ),
            g,
            theme.text_on_accent().to_egui(),
        );
    }
    // 선택되지 않은 탭도 같은 폭을 차지한다 (본체 allocate_exact_size 와 동일).
    let _ = ui;
    width
}

/// 헤더 밴드 (높이 48) — 본체 `TopBottomPanel::top("plugins_header")`.
fn header(ui: &mut egui::Ui, theme: &Theme, rect: egui::Rect) {
    let p = ui.painter_at(rect);
    p.rect_filled(rect, 0.0, theme.bg_sidebar().to_egui());
    p.hline(
        rect.x_range(),
        rect.bottom() - 0.5,
        egui::Stroke::new(theme.border_width.value(), theme.separator.to_egui()),
    );

    let cy = rect.center().y;
    let mut x = rect.min.x + theme.spacing_md.value();

    // plug 아이콘 (헤더 강조 — 본체 divergence 주석대로 accent-attention role).
    let icon = theme.icon_glyph_size_md.value();
    PLUG.image(icon, theme.accent_attention().to_egui())
        .paint_at(
            ui,
            egui::Rect::from_center_size(egui::pos2(x + icon * 0.5, cy), egui::vec2(icon, icon)),
        );
    x += icon + theme.spacing_xs.value();

    // 타이틀.
    let title_font = egui::FontId::proportional(theme.font_size_max.value());
    let title = p.layout_no_wrap(
        "Plugins".to_string(),
        title_font,
        theme.text_primary().to_egui(),
    );
    p.galley(
        egui::pos2(x, cy - title.size().y * 0.5),
        title.clone(),
        theme.text_primary().to_egui(),
    );
    x += title.size().x + theme.spacing_sm.value();

    // 세로 구분선 (1px × 20).
    let div_h = theme.spacing_lg.value() + theme.spacing_xs.value();
    p.rect_filled(
        egui::Rect::from_min_size(
            egui::pos2(x, cy - div_h * 0.5),
            egui::vec2(theme.border_width.value(), div_h),
        ),
        0.0,
        theme.separator.to_egui(),
    );
    x += theme.border_width.value() + theme.spacing_sm.value();

    // 세그먼트 탭 3개.
    let tab_y = cy - (theme.item_height_tab.value() + STRUCT_GAP_2.value()) * 0.5;
    x += segment_tab(
        ui,
        theme,
        &p,
        egui::pos2(x, tab_y),
        &Segment {
            label: "Installed",
            count: Some(ROWS.len()),
            danger: false,
            selected: true,
        },
    ) + STRUCT_GAP_2.value();
    x += segment_tab(
        ui,
        theme,
        &p,
        egui::pos2(x, tab_y),
        &Segment {
            label: "Attention",
            count: Some(1),
            danger: true,
            selected: false,
        },
    ) + STRUCT_GAP_2.value();
    segment_tab(
        ui,
        theme,
        &p,
        egui::pos2(x, tab_y),
        &Segment {
            label: "Add plugin",
            count: None,
            danger: false,
            selected: false,
        },
    );

    // 우측 클러스터 — 오른쪽부터 X → 필터 입력.
    let close = theme.item_height_interactive.value();
    let close_rect = egui::Rect::from_min_size(
        egui::pos2(
            rect.max.x - theme.spacing_sm.value() - close,
            cy - close * 0.5,
        ),
        egui::vec2(close, close),
    );
    CLOSE
        .image(
            theme.icon_glyph_size_md.value(),
            theme.text_secondary().to_egui(),
        )
        .paint_at(
            ui,
            egui::Rect::from_center_size(
                close_rect.center(),
                egui::Vec2::splat(theme.icon_glyph_size_md.value()),
            ),
        );

    let filter_w = theme.field_width_lg.value();
    let filter_h = theme.item_height_interactive.value();
    let filter_rect = egui::Rect::from_min_size(
        egui::pos2(
            close_rect.min.x - theme.spacing_sm.value() - filter_w,
            cy - filter_h * 0.5,
        ),
        egui::vec2(filter_w, filter_h),
    );
    p.rect(
        filter_rect,
        theme.corner_radius.value(),
        theme.surface_raised().to_egui(),
        egui::Stroke::new(theme.border_width.value(), theme.border_default().to_egui()),
        egui::StrokeKind::Inside,
    );
    p.text(
        egui::pos2(
            filter_rect.min.x + theme.spacing_sm.value(),
            filter_rect.center().y,
        ),
        egui::Align2::LEFT_CENTER,
        "Filter plugins…",
        egui::FontId::proportional(theme.font_size_body.value()),
        theme.text_placeholder().to_egui(),
    );
}

/// 좌측 목록 (폭 240) — 40px 2줄 행.
fn list_pane(ui: &mut egui::Ui, theme: &Theme, rect: egui::Rect) {
    let p = ui.painter_at(rect);
    p.rect_filled(rect, 0.0, theme.bg_sidebar().to_egui());

    let row_h = theme.item_height_interactive.value() + theme.spacing_md.value();
    let pad = egui::vec2(theme.spacing_sm.value(), theme.spacing_sm.value() * 0.75);
    let mut y = rect.min.y + theme.spacing_sm.value();

    for (i, row) in ROWS.iter().enumerate() {
        let r =
            egui::Rect::from_min_size(egui::pos2(rect.min.x, y), egui::vec2(rect.width(), row_h));
        let selected = i == 0;
        if selected {
            p.rect(
                r,
                theme.corner_radius.value(),
                theme.surface_active().to_egui(),
                egui::Stroke::new(theme.border_width.value(), theme.border_default().to_egui()),
                egui::StrokeKind::Inside,
            );
        }

        let name = if row.builtin {
            format!("{}  •", row.name)
        } else {
            row.name.to_string()
        };
        let name_pos = r.min + pad;
        p.text(
            name_pos,
            egui::Align2::LEFT_TOP,
            &name,
            egui::FontId::proportional(theme.font_size_body.value()),
            theme.text_primary().to_egui(),
        );

        let mut sub = format!("v{}", row.version);
        if !row.enabled {
            sub.push_str("  ·  Disabled");
        } else if row.running {
            sub.push_str("  ·  Running");
        }
        p.text(
            name_pos + egui::vec2(0.0, theme.spacing_lg.value() + STRUCT_GAP_2.value()),
            egui::Align2::LEFT_TOP,
            &sub,
            egui::FontId::proportional(theme.font_size_micro.value()),
            theme.text_muted().to_egui(),
        );

        if row.health_error && row.enabled {
            p.circle_filled(
                egui::pos2(r.max.x - theme.spacing_md.value(), r.center().y),
                theme.status_dot_size.value() * 0.5,
                theme.accent_danger().to_egui(),
            );
        }
        y += row_h + STRUCT_GAP_2.value();
    }
}

/// 우측 상세 — 선택된 행의 메타.
fn detail_pane(ui: &mut egui::Ui, theme: &Theme, rect: egui::Rect) {
    ui.painter_at(rect)
        .rect_filled(rect, 0.0, theme.bg_panel().to_egui());
    let inner = rect.shrink(theme.spacing_md.value());
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(inner));
    child.spacing_mut().item_spacing.y = theme.spacing_sm.value();
    child.horizontal(|ui| {
        ui.label(
            egui::RichText::new(ROWS[0].name)
                .size(theme.font_size_max.value())
                .strong()
                .color(theme.text_primary().to_egui()),
        );
        tag(ui, theme, "v0.4.2", TagVariant::Default, false);
        ui.label(
            egui::RichText::new("built-in")
                .size(theme.font_size_caption.value())
                .color(theme.accent_agent().to_egui()),
        );
    });
    child.label(
        egui::RichText::new("com.tasty.clipboard-viewer")
            .size(theme.font_size_caption.value())
            .color(theme.text_muted().to_egui()),
    );
    child.label(
        egui::RichText::new("Shows the clipboard history in a popup, grouped by content type.")
            .size(theme.font_size_body.value())
            .color(theme.text_primary().to_egui()),
    );
    child.label(
        egui::RichText::new("Authors: tasty")
            .size(theme.font_size_body.value())
            .color(theme.text_secondary().to_egui()),
    );
}

fn window(ui: &mut egui::Ui, theme: &Theme) {
    let (rect, _) = ui.allocate_exact_size(stage_size(theme), egui::Sense::hover());
    ui.painter().rect_filled(
        rect,
        theme.corner_radius.value(),
        theme.bg_panel().to_egui(),
    );

    let header_h =
        theme.item_height_interactive.value() + theme.spacing_lg.value() + theme.spacing_xs.value();
    let header_rect = egui::Rect::from_min_size(rect.min, egui::vec2(rect.width(), header_h));
    header(ui, theme, header_rect);

    let body_top = header_rect.bottom();
    let list_rect = egui::Rect::from_min_max(
        egui::pos2(rect.min.x, body_top),
        egui::pos2(rect.min.x + LIST_W, rect.max.y),
    );
    list_pane(ui, theme, list_rect);
    detail_pane(
        ui,
        theme,
        egui::Rect::from_min_max(egui::pos2(list_rect.max.x, body_top), rect.max),
    );

    // 목록/상세 경계 1px.
    ui.painter().vline(
        list_rect.max.x,
        egui::Rangef::new(body_top, rect.max.y),
        egui::Stroke::new(theme.border_width.value(), theme.separator.to_egui()),
    );
}

pub fn draw(ui: &mut egui::Ui, theme: &Theme) {
    spec::stage(ui, theme, StageVariant::Solo, |ui| window(ui, theme));

    spec::meta(
        ui,
        theme,
        &[
            (
                "header",
                "높이 48 · plug + 타이틀 + 1px 구분선 + 세그먼트 3",
            ),
            (
                "segments",
                "Installed N(mono count) / Attention N(danger 배지) / Add plugin",
            ),
            ("list", "폭 240 · 행 40 · 이름 13 + 부제 10 muted"),
            ("builtin", "이름 뒤 `•` · 상세는 accent-agent 배지"),
            ("health error", "enabled + error 인 행만 우측 danger dot"),
        ],
        &[
            TokenChip::new(
                "accent-attention",
                "header plug",
                theme.accent_attention().to_egui(),
            ),
            TokenChip::new(
                "accent-danger",
                "attention badge · health dot",
                theme.accent_danger().to_egui(),
            ),
            TokenChip::new(
                "surface-active",
                "selected row · tab",
                theme.surface_active().to_egui(),
            ),
            TokenChip::new("bg-sidebar", "header · list", theme.bg_sidebar().to_egui()),
        ],
    );

    spec::note(
        ui,
        theme,
        "세 탭은 서로 다른 일을 한다 — Installed 는 설치된 plugin 의 상태·권한·명령을 보고, \
         Attention 은 서명·권한 변경으로 등록이 거부됐거나 런타임에서 실패한 plugin 만 모아 \
         이유와 다음 수를 보여주며, Add plugin 은 로컬 폴더 경로로 설치한다. 검색 입력은 \
         목록이 있는 Installed 탭에서만 나타난다. 사용자가 직접 끈 plugin 은 error 가 아니라 \
         정상 종료이므로 danger dot 이 붙지 않는다.",
    );
}
