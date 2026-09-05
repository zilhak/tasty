//! `switch-overlay` specimen — Switch-number overlay (디자인 `gallery/overlays.jsx`
//! Switch-number overlay 섹션).
//!
//! modifier(탭=tab_switch_modifier·기본 Ctrl / 워크스페이스=workspace_switch_modifier·
//! 기본 Alt)를 **누르고 있는 동안** 각 항목의 leading indicator(탭 아이콘 / ws status
//! dot / collapsed letter avatar)를 숫자 키캡으로 **제자리 교체**한다. 폭/리플로 변화
//! 없는 16px slot, scrim 없음. 현재 항목은 **accent-filled** 키캡으로 구분.
//!
//! 키캡은 본체 공용 위젯 `tasty_ui_widgets::num_keycap` 을 그대로 호출한다 — specimen
//! 이 자체 키캡을 재구현하지 않고 본체와 **동일 위젯을 공유**한다(gallery-first). 형상은
//! `kbd()` 레시피(surface-raised fill + border-strong + 하단 2px edge + mono micro),
//! active 변종만 accent_primary fill + text_on_accent 숫자. 신규 Theme 필드 없음 —
//! P0 매핑대로 기존 접근자(`docs/design/systems/design-token-mapping.md` switch-overlay).

use tasty_type_appearance::theme::Theme;
use tasty_type_geometry::length::LogicalPx;
use tasty_ui_widgets::num_keycap;

use crate::catalog::icons::{CHEVRON_DOWN, CHEVRON_RIGHT, FILE, MockGlyph, TERMINAL};
use crate::catalog::spec::{self, StageVariant, TokenChip};

// 키캡 slot 의 디자인 고정 px = switch-overlay-size = kbd-size = size-16.
// 본체 `num_keycap` 위젯이 같은 16px 를 할당하므로 slot 폭/오프셋 계산과 정합한다.
const KEYCAP_SIZE: LogicalPx = LogicalPx(16.0);

/// 공용 `num_keycap` 위젯을 16px slot 중앙(`center`)에 배치한다.
/// specimen 은 painter 로 절대 위치에 레이아웃하므로, 위젯을 키캡 rect 크기의 child UI
/// 안에서 호출해 제자리에 그린다(본체와 동일 위젯 공유 — 재구현 금지).
/// `active` = 현재 탭/워크스페이스 → accent_primary fill + text_on_accent 숫자.
fn keycap_at(ui: &mut egui::Ui, theme: &Theme, center: egui::Pos2, digit: &str, active: bool) {
    let rect =
        egui::Rect::from_center_size(center, egui::vec2(KEYCAP_SIZE.value(), KEYCAP_SIZE.value()));
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(rect));
    num_keycap(&mut child, theme, digit, active);
}

fn paint_glyph(
    ui: &mut egui::Ui,
    glyph: MockGlyph,
    center: egui::Pos2,
    size: f32,
    color: egui::Color32,
) {
    let r = egui::Rect::from_center_size(center, egui::vec2(size, size));
    glyph.image(size, color).paint_at(ui, r);
}

// ── Tab switch overlay ──────────────────────────────────────────────

/// (glyph, label, digit, active)
const TABS: &[(MockGlyph, &str, &str, bool)] = &[
    (TERMINAL, "server", "1", false),
    (TERMINAL, "dev", "2", false),
    (TERMINAL, "agent", "3", true),
    (FILE, "README.md", "4", false),
];

fn tab_strip(ui: &mut egui::Ui, theme: &Theme, held: bool) {
    let h = theme.item_height_interactive; // 28
    let pad = theme.spacing_md; // 12
    let gap = theme.spacing_sm; // 8
    let bw = theme.border_width.value();
    let font = egui::FontId::proportional(theme.font_size_body.value());

    // 탭 폭 = pad + 아이콘slot(16) + gap + 라벨폭 + pad (디자인 fit-content).
    let widths: Vec<LogicalPx> = TABS
        .iter()
        .map(|(_, label, _, _)| {
            let lw = LogicalPx(ui.fonts(|f| {
                f.layout_no_wrap(
                    (*label).to_owned(),
                    font.clone(),
                    egui::Color32::PLACEHOLDER,
                )
                .size()
                .x
            }));
            pad + KEYCAP_SIZE + gap + lw + pad
        })
        .collect();
    // `LogicalPx` 에는 `Sum` 이 없다 — 더하기로 접는다(빈 목록은 `Default` = 0).
    let total = widths
        .iter()
        .copied()
        .reduce(|a, b| a + b)
        .unwrap_or_default();

    let (rect, _) =
        ui.allocate_exact_size(egui::vec2(total.value(), h.value()), egui::Sense::hover());
    let p = ui.painter_at(rect);
    p.rect_filled(
        rect,
        theme.corner_radius.value(),
        egui::Color32::from(theme.bg_sidebar()),
    );

    let mut x = LogicalPx(rect.min.x);
    for (i, (glyph, label, digit, active)) in TABS.iter().enumerate() {
        let tw = widths[i];
        let tab = egui::Rect::from_min_size(
            egui::pos2(x.value(), rect.min.y),
            egui::vec2(tw.value(), h.value()),
        );
        if *active {
            p.rect_filled(tab, 0.0, egui::Color32::from(theme.bg_panel()));
            let ind = theme.tab_indicator_width.value();
            let bar = egui::Rect::from_min_size(
                egui::pos2(tab.min.x, tab.max.y - ind),
                egui::vec2(tw.value(), ind),
            );
            p.rect_filled(bar, 0.0, egui::Color32::from(theme.accent_primary()));
        }
        if i > 0 {
            p.vline(
                x.value(),
                rect.y_range(),
                egui::Stroke::new(bw, egui::Color32::from(theme.separator)),
            );
        }
        // leading 16px slot: held → 숫자 키캡, else 표면 아이콘.
        let slot_c = egui::pos2(
            tab.min.x + (pad + KEYCAP_SIZE.scaled(0.5)).value(),
            tab.center().y,
        );
        if held {
            keycap_at(ui, theme, slot_c, digit, *active);
        } else {
            paint_glyph(
                ui,
                *glyph,
                slot_c,
                theme.icon_glyph_size_md.value(),
                egui::Color32::from(if *active {
                    theme.text_primary()
                } else {
                    theme.text_muted()
                }),
            );
        }
        p.text(
            egui::pos2(
                tab.min.x + (pad + KEYCAP_SIZE + gap).value(),
                tab.center().y,
            ),
            egui::Align2::LEFT_CENTER,
            label,
            font.clone(),
            egui::Color32::from(if *active {
                theme.text_primary()
            } else {
                theme.text_muted()
            }),
        );
        x += tw;
    }
    // strip 외곽선 (corner 위 덮어쓰기 방지 위해 콘텐츠 뒤).
    p.rect_stroke(
        rect,
        theme.corner_radius.value(),
        egui::Stroke::new(bw, egui::Color32::from(theme.separator)),
        egui::StrokeKind::Inside,
    );
}

pub fn draw_tab(ui: &mut egui::Ui, theme: &Theme) {
    spec::stage(ui, theme, StageVariant::Column, |ui| {
        spec::cluster(
            ui,
            theme,
            "released — each tab shows its surface icon",
            |ui| tab_strip(ui, theme, false),
        );
        spec::cluster(
            ui,
            theme,
            "Ctrl held — icon slot becomes the number keycap",
            |ui| tab_strip(ui, theme, true),
        );
    });

    spec::meta(
        ui,
        theme,
        &[
            ("widget", "Kbd keycap · 16px"),
            ("content", "digit only · 0 = 10th tab"),
            ("placement", "replaces leading icon, in place"),
            ("range", "1–9 + 0; 11th tab onward: none"),
            ("active tab", "accent-filled keycap"),
            ("scrim", "none"),
            ("appear", "90ms fade · release 0ms"),
        ],
        &[
            TokenChip::new(
                "switch-overlay-bg",
                "keycap fill",
                theme.surface_raised().into(),
            ),
            TokenChip::new(
                "switch-overlay-active-bg",
                "active-tab keycap",
                theme.accent_primary().into(),
            ),
            TokenChip::new(
                "switch-overlay-border",
                "keycap edge",
                theme.border_strong().into(),
            ),
        ],
    );

    spec::note(
        ui,
        theme,
        "숫자 = 누르는 키 그 자체 → 10번째 탭은 \"10\" 이 아니라 0 (Ctrl+0 대응). 11번째 \
         탭부터는 단축키가 없으니 키캡도 없다 — 작동 안 할 숫자는 칠하지 않는다.",
    );
}

// ── Workspace switch overlay ────────────────────────────────────────

/// (digit, name, sub, status, active)
const WS_ROWS: &[(&str, &str, &str, WsStatus, bool)] = &[
    ("1", "tasty-core", "main · 2 tabs", WsStatus::Running, false),
    ("2", "ai-review", "agent", WsStatus::Agent, true),
    ("3", "infra", "idle", WsStatus::Idle, false),
    ("4", "docs-site", "idle", WsStatus::Idle, false),
];

#[derive(Clone, Copy)]
enum WsStatus {
    Running,
    Agent,
    Idle,
}

fn status_color(theme: &Theme, s: WsStatus) -> egui::Color32 {
    egui::Color32::from(match s {
        WsStatus::Running => theme.accent_success(),
        WsStatus::Agent => theme.accent_agent(),
        WsStatus::Idle => theme.text_muted(),
    })
}

fn full_ws(ui: &mut egui::Ui, theme: &Theme, held: bool) {
    let w = theme.field_width_lg - theme.spacing_md * 1.0; // ≈188
    let pad = theme.spacing_sm; // 8
    let gap = theme.spacing_sm; // 8
    let bw = theme.border_width.value();
    let lead = KEYCAP_SIZE; // 16px slot (dot/numcap 공통)
    let text_x_off = pad + lead + gap; // 32 — divider margin-left 와 동일

    let head_h = theme.spacing_lg + theme.spacing_xs; // 10+4 ≈ 헤더 영역
    let name_lh = theme.font_size_body + theme.spacing_xs; // ≈17
    let sub_lh = theme.font_size_caption + theme.spacing_xs * 0.75; // ≈14
    let row_h = pad + name_lh + LogicalPx(1.0) + sub_lh + pad;
    let h = head_h + row_h * WS_ROWS.len() as f32;

    let (rect, _) = ui.allocate_exact_size(egui::vec2(w.value(), h.value()), egui::Sense::hover());
    let p = ui.painter_at(rect);
    p.rect_filled(
        rect,
        theme.corner_radius.value(),
        egui::Color32::from(theme.bg_sidebar()),
    );

    // 헤더 "WORKSPACES".
    p.text(
        egui::pos2(
            rect.min.x + theme.spacing_sm.value(),
            rect.min.y + theme.spacing_sm.value(),
        ),
        egui::Align2::LEFT_TOP,
        "WORKSPACES",
        egui::FontId::proportional(theme.sidebar_section_heading_font_size.value()),
        egui::Color32::from(theme.text_muted()),
    );

    let mut y = LogicalPx(rect.min.y) + head_h;
    for (i, (digit, name, sub, status, active)) in WS_ROWS.iter().enumerate() {
        let row = egui::Rect::from_min_size(
            egui::pos2(rect.min.x, y.value()),
            egui::vec2(w.value(), row_h.value()),
        );
        if *active {
            p.rect_filled(row, 0.0, egui::Color32::from(theme.surface_active()));
            let bar = egui::Rect::from_min_size(
                row.min,
                egui::vec2(theme.tab_indicator_width.value(), row.height()),
            );
            p.rect_filled(bar, 0.0, egui::Color32::from(theme.accent_primary()));
        } else if i > 0 {
            // 행간 divider — 텍스트 시작(32)부터 우측 끝까지.
            p.hline(
                (rect.min.x + text_x_off.value())..=rect.max.x,
                row.min.y,
                egui::Stroke::new(bw, egui::Color32::from(theme.separator)),
            );
        }

        let name_cy = LogicalPx(row.min.y) + pad + name_lh.scaled(0.5);
        let slot_c = egui::pos2(
            row.min.x + (pad + lead.scaled(0.5)).value(),
            name_cy.value(),
        );
        if held {
            keycap_at(ui, theme, slot_c, digit, *active);
        } else {
            // status dot — 16px slot 중앙에 8px dot.
            p.circle_filled(
                slot_c,
                theme.status_dot_size.value() * 0.5,
                status_color(theme, *status),
            );
        }
        p.text(
            egui::pos2(row.min.x + text_x_off.value(), name_cy.value()),
            egui::Align2::LEFT_CENTER,
            name,
            egui::FontId::proportional(theme.font_size_body.value()),
            egui::Color32::from(if *active {
                theme.text_primary()
            } else {
                theme.text_secondary()
            }),
        );
        p.text(
            egui::pos2(
                row.min.x + text_x_off.value(),
                (name_cy + name_lh.scaled(0.5) + LogicalPx(1.0) + sub_lh.scaled(0.5)).value(),
            ),
            egui::Align2::LEFT_CENTER,
            sub,
            egui::FontId::proportional(theme.font_size_caption.value()),
            egui::Color32::from(theme.text_muted()),
        );
        y += row_h;
    }

    p.rect_stroke(
        rect,
        theme.corner_radius.value(),
        egui::Stroke::new(bw, egui::Color32::from(theme.separator)),
        egui::StrokeKind::Inside,
    );
}

/// (digit | None=letter only, letter, status, active)
const RAIL_ITEMS: &[(Option<&str>, &str, WsStatus, bool)] = &[
    (Some("1"), "T", WsStatus::Running, false),
    (Some("2"), "A", WsStatus::Agent, true),
    (Some("3"), "I", WsStatus::Idle, false),
    (None, "D", WsStatus::Idle, false), // 10번째 밖 가정 — letter 유지
];

fn rail_ws(ui: &mut egui::Ui, theme: &Theme) {
    let slot = theme.item_height_interactive.value(); // 28
    let pad = theme.spacing_sm.value(); // 8
    let gap = theme.spacing_xs.value(); // 4
    let bw = theme.border_width.value();
    let w = slot + theme.spacing_lg.value(); // 28+16 ≈ 44

    let h = pad + (slot + gap) * RAIL_ITEMS.len() as f32;
    let (rect, _) = ui.allocate_exact_size(egui::vec2(w, h), egui::Sense::hover());
    let p = ui.painter_at(rect);
    p.rect_filled(
        rect,
        theme.corner_radius.value(),
        egui::Color32::from(theme.bg_sidebar()),
    );

    let cx = rect.center().x;
    let mut y = rect.min.y + pad;
    for (digit, letter, _status, active) in RAIL_ITEMS {
        let area =
            egui::Rect::from_center_size(egui::pos2(cx, y + slot * 0.5), egui::vec2(slot, slot));
        if *active {
            p.rect_filled(
                area,
                theme.corner_radius.value(),
                egui::Color32::from(theme.surface_active()),
            );
            p.rect_stroke(
                area,
                theme.corner_radius.value(),
                egui::Stroke::new(bw, egui::Color32::from(theme.accent_primary())),
                egui::StrokeKind::Inside,
            );
        }
        match digit {
            Some(d) => keycap_at(ui, theme, area.center(), d, *active),
            None => {
                p.text(
                    area.center(),
                    egui::Align2::CENTER_CENTER,
                    letter,
                    egui::FontId::monospace(theme.font_size_body.value()),
                    egui::Color32::from(theme.text_secondary()),
                );
            }
        }
        y += slot + gap;
    }
}

pub fn draw_workspace(ui: &mut egui::Ui, theme: &Theme) {
    spec::stage(ui, theme, StageVariant::Wrap, |ui| {
        spec::cluster(ui, theme, "released — full sidebar", |ui| {
            full_ws(ui, theme, false)
        });
        spec::cluster(
            ui,
            theme,
            "Alt held — status dot becomes the keycap",
            |ui| full_ws(ui, theme, true),
        );
        spec::cluster(
            ui,
            theme,
            "Alt held — collapsed rail · letter becomes the keycap",
            |ui| rail_ws(ui, theme),
        );
    });

    spec::meta(
        ui,
        theme,
        &[
            ("widget", "Kbd keycap · 16px"),
            ("content", "digit only (1–9)"),
            ("full placement", "replaces status dot"),
            ("collapsed", "replaces letter avatar"),
            ("range", "1–9 only; 10th onward: none"),
            ("active ws", "accent-filled keycap"),
            ("scrim", "none"),
        ],
        &[
            TokenChip::new(
                "switch-overlay-bg",
                "keycap fill",
                theme.surface_raised().into(),
            ),
            TokenChip::new(
                "switch-overlay-active-bg",
                "active-ws keycap",
                theme.accent_primary().into(),
            ),
            TokenChip::new(
                "switch-overlay-active-fg",
                "digit on accent",
                theme.text_on_accent().into(),
            ),
        ],
    );

    spec::note(
        ui,
        theme,
        "워크스페이스 단축키는 1–9 뿐(0 없음) → 10번째부터는 키캡 없음. collapsed rail 은 \
         그 항목의 letter avatar 를 그대로 유지한다(맨 아래 D). 바인딩은 단축키와 같은 \
         keybindings 소스에서 읽으므로 modifier 를 재바인딩하면 오버레이도 따라간다.",
    );

    spec::do_(
        ui,
        theme,
        "라벨을 덮지 말고 leading indicator 를 교체한다 — 키캡 자체의 surface-raised \
         fill + border 가 탭/사이드바 배경 위에서 4.5:1 을 확보해 scrim 이 필요 없다.",
    );
}

// ── Category switch overlay (Ctrl+Shift, 기본값) ─────────────────────
//
// 디자인 B/C (`overlays-shared.jsx` CatSwitchSidebarMock / CatSwitchRailMock).
// 카테고리 축은 독립 `category_switch_modifier`(기본 `ctrl+shift`)를 가지며 workspace-switch
// 와 **modifier-exclusive** — 카테고리 조합 홀드 중에는 카테고리 헤더만 키캡을 얻고,
// 워크스페이스 행은 status dot 을 그대로 유지한다.
//
// - Full: 키캡은 카테고리 헤더 행 **우측 정렬**(`[chevron] LABEL … [cap]`). chevron
//   은 접힘/자동확장(D) 을 나타내는 load-bearing 요소라 교체하지 않는다.
// - Rail: 라벨이 없으니 각 경계선(`---`) 슬롯 **중앙**에 키캡을 얹는다(선의 자리가
//   키캡의 자리). 접힌/빈 카테고리도 `---` 를 유지하므로 키캡을 받는다.
// - 번호: reserved normal("Workspaces")=1, 1–9 then 0(10th), 11th+ 키캡 없음.

/// 카테고리 헤더 한 줄(chevron + 라벨 + 우측 키캡). `n=None` → 11번째+ (키캡 없음).
struct CatHead {
    n: Option<&'static str>,
    label: &'static str,
    collapsed: bool,
    active: bool,
}

/// 워크스페이스 행 데모 데이터: (name, status, active).
type WsRow = (&'static str, WsStatus, bool);
/// 카테고리 그룹 데모 데이터: (헤더, 그 카테고리에 속한 행들).
type CatGroup = (CatHead, &'static [WsRow]);

/// 행은 (digit 없음) ws 행이라 status dot 유지.
const CAT_GROUPS: &[CatGroup] = &[
    (
        CatHead {
            n: Some("1"),
            label: "Workspaces",
            collapsed: false,
            active: true,
        },
        &[
            ("tasty-core", WsStatus::Running, true),
            ("scratch", WsStatus::Idle, false),
        ],
    ),
    (
        CatHead {
            n: Some("2"),
            label: "Services",
            collapsed: false,
            active: false,
        },
        &[
            ("api-gateway", WsStatus::Agent, false),
            ("data-pipeline", WsStatus::Running, false),
        ],
    ),
    (
        CatHead {
            n: Some("3"),
            label: "Archived",
            collapsed: true,
            active: false,
        },
        &[],
    ),
];

fn full_cat(ui: &mut egui::Ui, theme: &Theme, held: bool) {
    let w = theme.field_width_lg - theme.spacing_md; // ≈188
    let pad = theme.spacing_sm; // 8 (행 padding)
    let gap = theme.spacing_sm; // 8
    let bw = theme.border_width.value();
    let lead = KEYCAP_SIZE; // 16 status-dot slot
    let text_x_off = pad + lead + gap; // 32 — divider margin-left

    let chev = theme.spacing_md; // 12 chevron slot 폭
    let head_gap = theme.spacing_xs; // 4 chevron↔label
    let head_pad_v = theme.spacing_xs; // 4
    let head_margin_top = theme.spacing_sm; // 8
    let head_line = theme.sidebar_section_heading_font_size; // 10
    let head_h = head_margin_top + head_pad_v * 2.0 + head_line;

    let name_lh = theme.font_size_body + theme.spacing_xs; // ≈17
    let row_h = pad + name_lh + pad;

    // 전체 높이 = Σ(헤더 + 행들) + 아래 패딩.
    let mut total = theme.spacing_sm; // paddingBottom 8
    for (_, rows) in CAT_GROUPS {
        total += head_h + row_h * rows.len() as f32;
    }

    let (rect, _) =
        ui.allocate_exact_size(egui::vec2(w.value(), total.value()), egui::Sense::hover());
    let p = ui.painter_at(rect);
    p.rect_filled(
        rect,
        theme.corner_radius.value(),
        egui::Color32::from(theme.bg_sidebar()),
    );

    let mut y = LogicalPx(rect.min.y);
    for (head, rows) in CAT_GROUPS {
        // ── 카테고리 헤더 ──
        let hrect = egui::Rect::from_min_size(
            egui::pos2(rect.min.x, y.value()),
            egui::vec2(w.value(), head_h.value()),
        );
        let hcy =
            (LogicalPx(hrect.min.y) + head_margin_top + head_pad_v + head_line.scaled(0.5)).value();
        // chevron (load-bearing — 교체 금지). 접힘=우향, 확장=하향.
        let chev_c = egui::pos2(rect.min.x + (pad + chev.scaled(0.5)).value(), hcy);
        let glyph = if head.collapsed {
            CHEVRON_RIGHT
        } else {
            CHEVRON_DOWN
        };
        paint_glyph(
            ui,
            glyph,
            chev_c,
            theme.font_size_body.value(),
            egui::Color32::from(theme.text_muted()),
        );
        // 라벨 (mono uppercase micro).
        p.text(
            egui::pos2(rect.min.x + (pad + chev + head_gap).value(), hcy),
            egui::Align2::LEFT_CENTER,
            head.label.to_uppercase(),
            egui::FontId::monospace(head_line.value()),
            egui::Color32::from(theme.text_muted()),
        );
        // 우측 정렬 키캡 (held + n 있을 때만).
        if held && let Some(d) = head.n {
            let cap_c = egui::pos2(rect.max.x - (pad + KEYCAP_SIZE.scaled(0.5)).value(), hcy);
            keycap_at(ui, theme, cap_c, d, head.active);
        }
        y += head_h;

        // ── 워크스페이스 행 (status dot 유지 — modifier-exclusive) ──
        for (i, (name, status, active)) in rows.iter().enumerate() {
            let row = egui::Rect::from_min_size(
                egui::pos2(rect.min.x, y.value()),
                egui::vec2(w.value(), row_h.value()),
            );
            if *active {
                p.rect_filled(row, 0.0, egui::Color32::from(theme.surface_active()));
                let bar = egui::Rect::from_min_size(
                    row.min,
                    egui::vec2(theme.tab_indicator_width.value(), row.height()),
                );
                p.rect_filled(bar, 0.0, egui::Color32::from(theme.accent_primary()));
            } else if i > 0 {
                p.hline(
                    (rect.min.x + text_x_off.value())..=rect.max.x,
                    row.min.y,
                    egui::Stroke::new(bw, egui::Color32::from(theme.separator)),
                );
            }
            let cy = row.center().y;
            let slot_c = egui::pos2(row.min.x + (pad + lead.scaled(0.5)).value(), cy);
            p.circle_filled(
                slot_c,
                theme.status_dot_size.value() * 0.5,
                status_color(theme, *status),
            );
            p.text(
                egui::pos2(row.min.x + text_x_off.value(), cy),
                egui::Align2::LEFT_CENTER,
                name,
                egui::FontId::proportional(theme.font_size_body.value()),
                egui::Color32::from(if *active {
                    theme.text_primary()
                } else {
                    theme.text_secondary()
                }),
            );
            y += row_h;
        }
    }

    p.rect_stroke(
        rect,
        theme.corner_radius.value(),
        egui::Stroke::new(bw, egui::Color32::from(theme.separator)),
        egui::StrokeKind::Inside,
    );
}

/// 레일 항목: 경계선(`---`)이면 `Boundary(digit, active)`, 아바타면 `Avatar(letter, active)`.
enum RailCat {
    Boundary(&'static str, bool),
    Avatar(&'static str, bool),
}

const RAIL_CAT: &[RailCat] = &[
    RailCat::Boundary("1", true),
    RailCat::Avatar("T", true),
    RailCat::Avatar("S", false),
    RailCat::Boundary("2", false),
    RailCat::Avatar("A", false),
    RailCat::Avatar("D", false),
    RailCat::Boundary("3", false),
];

fn rail_cat(ui: &mut egui::Ui, theme: &Theme, held: bool) {
    let slot = theme.item_height_interactive.value(); // 28 아바타
    let bound_h = theme.spacing_lg.value() + theme.spacing_xs.value() + theme.spacing_xs.value(); // ≈24 경계 슬롯
    let pad = theme.spacing_sm.value(); // 8
    let gap = theme.spacing_xs.value(); // 4
    let bw = theme.border_width.value();
    let w = slot + theme.spacing_lg.value(); // ≈44
    let line_w = theme.spacing_lg.value() + theme.spacing_sm.value(); // 24 `---` 길이

    let mut total = pad * 2.0;
    for item in RAIL_CAT {
        total += match item {
            RailCat::Boundary(..) => bound_h,
            RailCat::Avatar(..) => slot,
        } + gap;
    }
    total -= gap; // 마지막 gap 제거

    let (rect, _) = ui.allocate_exact_size(egui::vec2(w, total), egui::Sense::hover());
    let p = ui.painter_at(rect);
    p.rect_filled(
        rect,
        theme.corner_radius.value(),
        egui::Color32::from(theme.bg_sidebar()),
    );

    let cx = rect.center().x;
    let mut y = rect.min.y + pad;
    for item in RAIL_CAT {
        match item {
            RailCat::Boundary(d, active) => {
                let c = egui::pos2(cx, y + bound_h * 0.5);
                if held {
                    keycap_at(ui, theme, c, d, *active);
                } else {
                    let line = egui::Rect::from_center_size(c, egui::vec2(line_w, bw));
                    p.rect_filled(line, 0.0, egui::Color32::from(theme.separator));
                }
                y += bound_h + gap;
            }
            RailCat::Avatar(letter, active) => {
                let area = egui::Rect::from_center_size(
                    egui::pos2(cx, y + slot * 0.5),
                    egui::vec2(slot, slot),
                );
                if *active {
                    p.rect_filled(
                        area,
                        theme.corner_radius.value(),
                        egui::Color32::from(theme.surface_active()),
                    );
                    p.rect_stroke(
                        area,
                        theme.corner_radius.value(),
                        egui::Stroke::new(bw, egui::Color32::from(theme.accent_primary())),
                        egui::StrokeKind::Inside,
                    );
                }
                p.text(
                    area.center(),
                    egui::Align2::CENTER_CENTER,
                    letter,
                    egui::FontId::monospace(theme.font_size_body.value()),
                    egui::Color32::from(theme.text_secondary()),
                );
                y += slot + gap;
            }
        }
    }
}

pub fn draw_category(ui: &mut egui::Ui, theme: &Theme) {
    spec::stage(ui, theme, StageVariant::Wrap, |ui| {
        spec::cluster(ui, theme, "released — full sidebar", |ui| {
            full_cat(ui, theme, false)
        });
        spec::cluster(
            ui,
            theme,
            "Ctrl+Shift held — keycap right-aligned on each header",
            |ui| full_cat(ui, theme, true),
        );
        spec::cluster(ui, theme, "released — collapsed rail", |ui| {
            rail_cat(ui, theme, false)
        });
        spec::cluster(
            ui,
            theme,
            "Ctrl+Shift held — keycap centered on each --- boundary",
            |ui| rail_cat(ui, theme, true),
        );
    });

    spec::meta(
        ui,
        theme,
        &[
            ("widget", "Kbd keycap · 16px"),
            ("content", "digit · 1–9 then 0 (10th)"),
            ("full placement", "right-aligned on header (keeps chevron)"),
            ("rail placement", "centered on the --- boundary"),
            ("range", "reserved normal = 1; 11th onward: none"),
            (
                "exclusivity",
                "category combo ⇒ headers only; rows keep dots",
            ),
            ("on switch", "auto-expand collapsed + land on last-active"),
        ],
        &[
            TokenChip::new(
                "switch-overlay-bg",
                "keycap fill",
                theme.surface_raised().into(),
            ),
            TokenChip::new(
                "switch-overlay-active-bg",
                "active-category keycap",
                theme.accent_primary().into(),
            ),
            TokenChip::new(
                "switch-overlay-border",
                "keycap edge",
                theme.border_strong().into(),
            ),
        ],
    );

    spec::note(
        ui,
        theme,
        "워크스페이스(`workspace_switch_modifier`)와 카테고리(`category_switch_modifier`, 기본 \
         Ctrl+Shift)는 서로 다른 축의 조합이라 상호 배타 — 두 오버레이는 동시에 그려지지 \
         않는다. 카테고리 헤더엔 교체할 status dot 이 없고 chevron 은 접힘/자동확장 \
         을 나타내는 load-bearing 요소이므로, 키캡은 헤더 우측에 덧붙고 chevron 을 건드리지 \
         않는다. reserved normal(\"Workspaces\")도 전환 대상(1) 이다.",
    );
}
