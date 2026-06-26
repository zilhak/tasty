//! `preset-editor` specimen — 프리셋 데모 레이아웃 **미리보기(read-only)**.
//! 디자인 `(3) gallery/preset_editor.jsx` 의 `SurfaceView` / `Pane` / `PaneTree` /
//! `SurfaceBox` 표시 부분을 구조까지 1:1 전사한다.
//!
//! 이번(TODO 07) 범위는 read-only 미리보기뿐 — `LeafEditor` 의 인라인 편집·핸들과
//! `PresetWindow` 의 목록/툴바 통합은 08/09 후속이라 만들지 않는다. 갤러리 specimen 은
//! 정적(Theme-only, binary 미의존)이라 mini-tab 클릭 전환은 본체에서만 동작한다 —
//! 여기서는 각 pane 의 **활성 탭**만 그린다.
//!
//! 3종 구조 레벨을 서로 다른 시각 weight 로 구분(디자인 changelog):
//!  - Pane split (상위 레이아웃) → 테두리 카드 + **5px bg-app gap** (무거운 divider).
//!  - Surface split (하위 레이아웃) → **1px border-default hairline** (가벼운 divider).
//!  - Surface leaf → kind 아이콘 + 표시명(가운데, mono). 내용은 렌더 안 함(구조만).
//!  - Mini tab strip → 20px, bg-sidebar. 활성 = bg-panel + 2px accent 하단 bar + kind 아이콘.

use tasty_type_appearance::theme::Theme;

use crate::catalog::icons::{self, MockGlyph};
use crate::catalog::spec::{self, StageVariant, TokenChip};

// 디자인 고정 px (Theme 에 대응 토큰 없는 preview 전용 치수 — jsx inline style 전사).
/// `PaneTree` 의 `gap:5` — bordered pane 카드 사이의 bg-app 공백 = 상위(pane) divider.
const PANE_GAP: f32 = 5.0;
/// mini tab strip `height:20`.
const STRIP_H: f32 = 20.0;
/// `Pane` 의 활성 탭 본문 `padding:3`.
const BODY_PAD: f32 = 3.0;
/// `SurfaceBox` 의 아이콘↔라벨 `gap:6`.
const LEAF_GAP: f32 = 6.0;
/// mini tab `padding:0 9px`.
const TAB_PAD_X: f32 = 9.0;
/// mini tab 아이콘↔라벨 `gap:5`.
const TAB_GAP: f32 = 5.0;
/// 편집 상태 `MiniHandle` 한 변 크기.
const E_HANDLE_SZ: f32 = 18.0;
/// 편집 핸들 사이 gap.
const E_HANDLE_GAP: f32 = 2.0;
/// 편집 핸들 클러스터 모서리 inset.
const E_HANDLE_INSET: f32 = 4.0;
/// inline leaf form 좌우 padding.
const E_FORM_PAD: f32 = 6.0;
/// inline leaf form 필드 세로 gap.
const E_FORM_GAP: f32 = 4.0;
/// inline leaf form 필드 입력 박스 높이.
const E_FIELD_H: f32 = 20.0;
/// inline leaf form 라벨 높이.
const E_LABEL_H: f32 = 12.0;

// ── 정적 preview 모델 (디자인 build* 트리 전사) ──────────────────────

#[derive(Clone, Copy)]
enum Kind {
    Terminal,
    Markdown,
    Editor,
    Log,
}

impl Kind {
    fn icon(self) -> MockGlyph {
        match self {
            Kind::Terminal => icons::TERMINAL,
            Kind::Markdown => icons::MARKDOWN,
            Kind::Editor => icons::EDIT,
            Kind::Log => icons::LOG,
        }
    }
    fn label(self) -> &'static str {
        match self {
            Kind::Terminal => "Terminal",
            Kind::Markdown => "Markdown",
            Kind::Editor => "Editor",
            Kind::Log => "Log",
        }
    }
    /// 디자인 KINDS accent 매핑 (terminal→success / markdown→primary / editor→agent / log→warning).
    fn accent(self, theme: &Theme) -> egui::Color32 {
        match self {
            Kind::Terminal => theme.accent_success().to_egui(),
            Kind::Markdown => theme.accent_primary().to_egui(),
            Kind::Editor => theme.accent_agent().to_egui(),
            Kind::Log => theme.accent_warning().to_egui(),
        }
    }
}

/// 하위 레이아웃(surface split) 트리.
enum Surf {
    Leaf(Kind),
    Split {
        row: bool,
        ratio: f32,
        first: Box<Surf>,
        second: Box<Surf>,
    },
}

struct DemoTab {
    name: &'static str,
    layout: Surf,
}

/// 상위 레이아웃(pane split) 트리.
enum Pane {
    Leaf {
        tabs: Vec<DemoTab>,
        active: usize,
    },
    Split {
        row: bool,
        ratio: f32,
        first: Box<Pane>,
        second: Box<Pane>,
    },
}

/// scope variant — workspace/pane 은 pane 트리, tab 은 단일 surface-split 트리(프레임).
enum Scope {
    PaneTree(Pane),
    TabFrame(Surf),
}

fn leaf(k: Kind) -> Surf {
    Surf::Leaf(k)
}
fn ssplit(row: bool, ratio: f32, a: Surf, b: Surf) -> Surf {
    Surf::Split {
        row,
        ratio,
        first: Box::new(a),
        second: Box::new(b),
    }
}
fn tab(name: &'static str, layout: Surf) -> DemoTab {
    DemoTab { name, layout }
}
fn pleaf(tabs: Vec<DemoTab>, active: usize) -> Pane {
    Pane::Leaf { tabs, active }
}
fn psplit(row: bool, ratio: f32, a: Pane, b: Pane) -> Pane {
    Pane::Split {
        row,
        ratio,
        first: Box::new(a),
        second: Box::new(b),
    }
}

// 디자인 buildWorkspace / buildTab / buildPane 전사.
fn build_workspace() -> Scope {
    Scope::PaneTree(psplit(
        true,
        0.6,
        pleaf(
            vec![
                tab(
                    "edit",
                    ssplit(false, 0.64, leaf(Kind::Editor), leaf(Kind::Terminal)),
                ),
                tab("agent", leaf(Kind::Editor)),
            ],
            0,
        ),
        pleaf(
            vec![
                tab("preview", leaf(Kind::Markdown)),
                tab(
                    "logs",
                    ssplit(true, 0.5, leaf(Kind::Log), leaf(Kind::Terminal)),
                ),
            ],
            0,
        ),
    ))
}
fn build_tab() -> Scope {
    Scope::TabFrame(ssplit(
        true,
        0.5,
        leaf(Kind::Editor),
        ssplit(false, 0.5, leaf(Kind::Terminal), leaf(Kind::Log)),
    ))
}
fn build_pane() -> Scope {
    Scope::PaneTree(pleaf(
        vec![
            tab("server", leaf(Kind::Terminal)),
            tab(
                "dev",
                ssplit(false, 0.5, leaf(Kind::Terminal), leaf(Kind::Log)),
            ),
            tab("notes", leaf(Kind::Markdown)),
        ],
        0,
    ))
}

// ── rect 분할 헬퍼 ──────────────────────────────────────────────────

/// `rect` 를 split 비율로 나눈다. `divider` 만큼을 가운데 띠로 빼고 first/second 에 분배.
/// 반환 = (first, divider_rect, second). surface 는 divider 를 1px hairline 으로 칠하고,
/// pane 은 divider 를 칠하지 않아 bg-app 공백(=상위 divider)으로 남긴다.
fn split_rects(
    rect: egui::Rect,
    row: bool,
    ratio: f32,
    divider: f32,
) -> (egui::Rect, egui::Rect, egui::Rect) {
    if row {
        let avail = (rect.width() - divider).max(0.0);
        let fw = avail * ratio;
        let first = egui::Rect::from_min_size(rect.min, egui::vec2(fw, rect.height()));
        let mid = egui::Rect::from_min_size(
            egui::pos2(rect.min.x + fw, rect.min.y),
            egui::vec2(divider, rect.height()),
        );
        let second = egui::Rect::from_min_size(
            egui::pos2(rect.min.x + fw + divider, rect.min.y),
            egui::vec2(avail - fw, rect.height()),
        );
        (first, mid, second)
    } else {
        let avail = (rect.height() - divider).max(0.0);
        let fh = avail * ratio;
        let first = egui::Rect::from_min_size(rect.min, egui::vec2(rect.width(), fh));
        let mid = egui::Rect::from_min_size(
            egui::pos2(rect.min.x, rect.min.y + fh),
            egui::vec2(rect.width(), divider),
        );
        let second = egui::Rect::from_min_size(
            egui::pos2(rect.min.x, rect.min.y + fh + divider),
            egui::vec2(rect.width(), avail - fh),
        );
        (first, mid, second)
    }
}

// ── 재귀 렌더 ───────────────────────────────────────────────────────

/// 하위 레이아웃(surface split). Leaf = kind 박스, Split = 1px hairline 으로 분할.
fn draw_surf(ui: &mut egui::Ui, theme: &Theme, rect: egui::Rect, node: &Surf) {
    match node {
        Surf::Leaf(k) => draw_surface_box(ui, theme, rect, *k),
        Surf::Split {
            row,
            ratio,
            first,
            second,
        } => {
            let (r1, line, r2) = split_rects(rect, *row, *ratio, theme.border_width.value());
            draw_surf(ui, theme, r1, first);
            ui.painter_at(rect)
                .rect_filled(line, 0.0, theme.border_default().to_egui());
            draw_surf(ui, theme, r2, second);
        }
    }
}

/// surface leaf — bg-app fill, 가운데 kind 아이콘(accent) + 표시명(mono, secondary).
fn draw_surface_box(ui: &mut egui::Ui, theme: &Theme, rect: egui::Rect, kind: Kind) {
    let p = ui.painter_at(rect);
    p.rect_filled(rect, 0.0, theme.bg_app().to_egui());

    let icon = theme.icon_glyph_size_md.value();
    let label_h = theme.font_size_caption.value();
    let total = icon + LEAF_GAP + label_h;
    let icon_cy = rect.center().y - total * 0.5 + icon * 0.5;
    paint_glyph(
        ui,
        kind.icon(),
        egui::pos2(rect.center().x, icon_cy),
        icon,
        kind.accent(theme),
    );
    ui.painter_at(rect).text(
        egui::pos2(
            rect.center().x,
            icon_cy + icon * 0.5 + LEAF_GAP + label_h * 0.5,
        ),
        egui::Align2::CENTER_CENTER,
        kind.label(),
        egui::FontId::monospace(label_h),
        theme.text_secondary().to_egui(),
    );
}

/// 상위 레이아웃(pane split). Leaf = pane 카드, Split = 5px bg-app gap 으로 분할.
fn draw_pane_tree(ui: &mut egui::Ui, theme: &Theme, rect: egui::Rect, node: &Pane) {
    match node {
        Pane::Leaf { tabs, active } => draw_pane_card(ui, theme, rect, tabs, *active),
        Pane::Split {
            row,
            ratio,
            first,
            second,
        } => {
            // divider(=PANE_GAP) 는 칠하지 않는다 — preview body 의 bg-app 이 그대로 비쳐
            // 무거운 상위 divider 가 된다 (surface hairline 보다 의도적으로 두껍게).
            let (r1, _gap, r2) = split_rects(rect, *row, *ratio, PANE_GAP);
            draw_pane_tree(ui, theme, r1, first);
            draw_pane_tree(ui, theme, r2, second);
        }
    }
}

/// pane 카드 = 테두리 카드 + mini tab strip + 활성 탭의 surface 레이아웃.
fn draw_pane_card(
    ui: &mut egui::Ui,
    theme: &Theme,
    rect: egui::Rect,
    tabs: &[DemoTab],
    active: usize,
) {
    let radius = theme.corner_radius.value();
    let bw = theme.border_width.value();
    let sep = theme.separator.to_egui();
    let p = ui.painter_at(rect);
    p.rect_filled(rect, radius, theme.bg_app().to_egui());

    // mini tab strip.
    let strip = egui::Rect::from_min_size(rect.min, egui::vec2(rect.width(), STRIP_H));
    p.rect_filled(strip, 0.0, theme.bg_sidebar().to_egui());

    let tab_font = egui::FontId::proportional(theme.font_size_caption.value());
    let icon_sz = theme.icon_glyph_size_sm.value();
    let mut x = strip.min.x;
    for (i, t) in tabs.iter().enumerate() {
        let on = i == active;
        let lw = text_width(ui, t.name, tab_font.clone());
        let tw = TAB_PAD_X + icon_sz + TAB_GAP + lw + TAB_PAD_X;
        let tab_rect =
            egui::Rect::from_min_size(egui::pos2(x, strip.min.y), egui::vec2(tw, STRIP_H));
        if on {
            p.rect_filled(tab_rect, 0.0, theme.bg_panel().to_egui());
            // 2px accent 하단 bar.
            let bar = egui::Rect::from_min_size(
                egui::pos2(
                    tab_rect.min.x,
                    tab_rect.max.y - theme.tab_indicator_width.value(),
                ),
                egui::vec2(tw, theme.tab_indicator_width.value()),
            );
            p.rect_filled(bar, 0.0, theme.accent_primary().to_egui());
        }
        if i > 0 {
            // 탭 사이 separator (borderRight).
            p.vline(x, strip.y_range(), egui::Stroke::new(bw, sep));
        }
        let icon_c = egui::pos2(
            tab_rect.min.x + TAB_PAD_X + icon_sz * 0.5,
            tab_rect.center().y,
        );
        let icon_color = if on {
            tab_kind(t).accent(theme)
        } else {
            theme.text_muted().to_egui()
        };
        paint_glyph(ui, tab_kind(t).icon(), icon_c, icon_sz, icon_color);
        ui.painter_at(strip).text(
            egui::pos2(
                tab_rect.min.x + TAB_PAD_X + icon_sz + TAB_GAP,
                tab_rect.center().y,
            ),
            egui::Align2::LEFT_CENTER,
            t.name,
            tab_font.clone(),
            if on {
                theme.text_primary().to_egui()
            } else {
                theme.text_muted().to_egui()
            },
        );
        x += tw;
    }
    // strip border-bottom.
    ui.painter_at(rect)
        .hline(strip.x_range(), strip.max.y, egui::Stroke::new(bw, sep));

    // 활성 탭 본문 — padding 3, bg-app.
    let body = egui::Rect::from_min_max(egui::pos2(rect.min.x, strip.max.y), rect.max);
    let inner = body.shrink(BODY_PAD);
    let active_tab = tabs.get(active).or_else(|| tabs.first());
    if let Some(t) = active_tab {
        draw_surf(ui, theme, inner, &t.layout);
    }

    // 카드 외곽 border.
    ui.painter_at(rect).rect_stroke(
        rect,
        radius,
        egui::Stroke::new(bw, theme.border_default().to_egui()),
        egui::StrokeKind::Inside,
    );
}

/// 탭 대표 kind = 첫 leaf (디자인 `activeKind`, mini-tab 아이콘 구동).
fn tab_kind(t: &DemoTab) -> Kind {
    let mut n = &t.layout;
    loop {
        match n {
            Surf::Leaf(k) => return *k,
            Surf::Split { first, .. } => n = first,
        }
    }
}

// ── 편집 상태 렌더 (디자인 `SurfaceBox` edit + `MiniHandle` + `LeafEditor`) ──
//
// 정적 specimen 이라 인터랙션은 없다 — "편집 모드의 시각"만 보인다. 모든 surface 가
// 1px separator outline 을 달고, `selected`(방문 순서 index) surface 는 2px accent
// inset outline + 우상단 handle cluster(split-right/split-down/remove) + 중앙 라벨
// 대신 inline leaf form(kind/cwd/startup) 을 보인다. startup 은 terminal 한정.

/// 편집 트리 순회 상태 — leaf 방문 순서 index 로 선택 leaf 를 지정한다.
struct EditWalk {
    next: usize,
    selected: usize,
}

fn draw_surf_edit(
    ui: &mut egui::Ui,
    theme: &Theme,
    rect: egui::Rect,
    node: &Surf,
    w: &mut EditWalk,
) {
    match node {
        Surf::Leaf(k) => {
            let idx = w.next;
            w.next += 1;
            draw_surface_box_edit(ui, theme, rect, *k, idx == w.selected);
        }
        Surf::Split {
            row,
            ratio,
            first,
            second,
        } => {
            let (r1, line, r2) = split_rects(rect, *row, *ratio, theme.border_width.value());
            draw_surf_edit(ui, theme, r1, first, w);
            ui.painter_at(rect)
                .rect_filled(line, 0.0, theme.border_default().to_egui());
            draw_surf_edit(ui, theme, r2, second, w);
        }
    }
}

/// 편집 상태 surface — 비선택: 중앙 라벨 + 1px separator outline. 선택: inline form +
/// 2px accent outline + handle cluster.
fn draw_surface_box_edit(
    ui: &mut egui::Ui,
    theme: &Theme,
    rect: egui::Rect,
    kind: Kind,
    selected: bool,
) {
    ui.painter_at(rect)
        .rect_filled(rect, 0.0, theme.bg_app().to_egui());

    if selected {
        draw_leaf_form_mock(ui, theme, rect, kind);
    } else {
        let icon = theme.icon_glyph_size_md.value();
        let label_h = theme.font_size_caption.value();
        let total = icon + LEAF_GAP + label_h;
        let icon_cy = rect.center().y - total * 0.5 + icon * 0.5;
        paint_glyph(
            ui,
            kind.icon(),
            egui::pos2(rect.center().x, icon_cy),
            icon,
            kind.accent(theme),
        );
        ui.painter_at(rect).text(
            egui::pos2(
                rect.center().x,
                icon_cy + icon * 0.5 + LEAF_GAP + label_h * 0.5,
            ),
            egui::Align2::CENTER_CENTER,
            kind.label(),
            egui::FontId::monospace(label_h),
            theme.text_secondary().to_egui(),
        );
    }

    // outline: 선택 = 2px accent, 비선택 = 1px separator (편집 가능 영역 표시).
    if selected {
        let bw = theme.tab_indicator_width.value();
        ui.painter_at(rect).rect_stroke(
            rect.shrink(bw * 0.5),
            0.0,
            egui::Stroke::new(bw, theme.accent_primary().to_egui()),
            egui::StrokeKind::Inside,
        );
        draw_handle_cluster_mock(ui, theme, rect);
    } else {
        let bw = theme.border_width.value();
        ui.painter_at(rect).rect_stroke(
            rect,
            0.0,
            egui::Stroke::new(bw, theme.separator.to_egui()),
            egui::StrokeKind::Inside,
        );
    }
}

/// 우상단 handle cluster mock — split-right · split-down · remove(danger).
fn draw_handle_cluster_mock(ui: &mut egui::Ui, theme: &Theme, rect: egui::Rect) {
    let step = E_HANDLE_SZ + E_HANDLE_GAP;
    let x0 = rect.max.x - E_HANDLE_INSET - E_HANDLE_SZ * 3.0 - E_HANDLE_GAP * 2.0;
    let y = rect.min.y + E_HANDLE_INSET;
    let cell = |i: f32| {
        egui::Rect::from_min_size(
            egui::pos2(x0 + step * i, y),
            egui::vec2(E_HANDLE_SZ, E_HANDLE_SZ),
        )
    };
    mini_handle_mock(ui, theme, cell(0.0), icons::SPLIT, false);
    mini_handle_mock(ui, theme, cell(1.0), icons::SPLIT_DOWN, false);
    mini_handle_mock(ui, theme, cell(2.0), icons::TRASH, true);
}

fn mini_handle_mock(
    ui: &mut egui::Ui,
    theme: &Theme,
    rect: egui::Rect,
    glyph: MockGlyph,
    danger: bool,
) {
    let radius = theme.corner_radius_sm.value();
    ui.painter_at(rect).rect(
        rect,
        radius,
        theme.surface_raised().to_egui(),
        egui::Stroke::new(theme.border_width.value(), theme.border_strong().to_egui()),
        egui::StrokeKind::Inside,
    );
    let color = if danger {
        theme.accent_danger().to_egui()
    } else {
        theme.text_secondary().to_egui()
    };
    paint_glyph(ui, glyph, rect.center(), E_HANDLE_SZ * 0.62, color);
}

/// inline leaf form mock — kind / cwd / startup(terminal 한정) 필드.
fn draw_leaf_form_mock(ui: &mut egui::Ui, theme: &Theme, rect: egui::Rect, kind: Kind) {
    let inner_w = (rect.width() - E_FORM_PAD * 2.0).max(0.0);
    let mut y = rect.min.y + E_HANDLE_INSET * 2.0 + E_HANDLE_SZ;
    let x = rect.center().x - inner_w * 0.5;
    let terminal = matches!(kind, Kind::Terminal);
    let fields: &[(&str, &str)] = if terminal {
        &[
            ("KIND", "Terminal"),
            ("CWD", "~/tasty"),
            ("STARTUP COMMAND", "cargo build"),
        ]
    } else {
        &[("KIND", "Markdown"), ("CWD", "~/tasty")]
    };
    for (label, value) in fields {
        if y + E_LABEL_H + E_FIELD_H > rect.max.y - E_FORM_PAD {
            break;
        }
        ui.painter_at(rect).text(
            egui::pos2(x, y),
            egui::Align2::LEFT_TOP,
            label,
            egui::FontId::monospace(theme.font_size_micro.value()),
            theme.text_muted().to_egui(),
        );
        y += E_LABEL_H;
        let fr = egui::Rect::from_min_size(egui::pos2(x, y), egui::vec2(inner_w, E_FIELD_H));
        ui.painter_at(rect).rect(
            fr,
            theme.corner_radius.value(),
            theme.surface_raised().to_egui(),
            egui::Stroke::new(theme.border_width.value(), theme.border_default().to_egui()),
            egui::StrokeKind::Inside,
        );
        ui.painter_at(fr).text(
            egui::pos2(fr.min.x + E_FORM_PAD, fr.center().y),
            egui::Align2::LEFT_CENTER,
            value,
            egui::FontId::monospace(theme.font_size_caption.value()),
            theme.text_primary().to_egui(),
        );
        y += E_FIELD_H + E_FORM_GAP;
    }
}

/// 편집 상태 Tab scope mock 프레임(strip 없음 + selected surface).
fn draw_scope_body_edit(
    ui: &mut egui::Ui,
    theme: &Theme,
    rect: egui::Rect,
    surf: &Surf,
    selected: usize,
) {
    let radius = theme.corner_radius.value();
    let bw = theme.border_width.value();
    let p = ui.painter_at(rect);
    p.rect_filled(rect, radius, theme.bg_app().to_egui());
    let mut w = EditWalk { next: 0, selected };
    draw_surf_edit(ui, theme, rect.shrink(BODY_PAD), surf, &mut w);
    ui.painter_at(rect).rect_stroke(
        rect,
        radius,
        egui::Stroke::new(bw, theme.border_default().to_egui()),
        egui::StrokeKind::Inside,
    );
}

fn draw_scope_body(ui: &mut egui::Ui, theme: &Theme, rect: egui::Rect, scope: &Scope) {
    match scope {
        Scope::PaneTree(p) => draw_pane_tree(ui, theme, rect, p),
        Scope::TabFrame(s) => {
            // 단일 탭 본문처럼 프레임(테두리 + radius + padding 3), strip 없음.
            let radius = theme.corner_radius.value();
            let bw = theme.border_width.value();
            let p = ui.painter_at(rect);
            p.rect_filled(rect, radius, theme.bg_app().to_egui());
            draw_surf(ui, theme, rect.shrink(BODY_PAD), s);
            ui.painter_at(rect).rect_stroke(
                rect,
                radius,
                egui::Stroke::new(bw, theme.border_default().to_egui()),
                egui::StrokeKind::Inside,
            );
        }
    }
}

/// 라벨 붙은 scope 데모 한 칸 — 제목/부제 + 미리보기 캔버스(LivePreview outer 전사).
fn scope_demo(
    ui: &mut egui::Ui,
    theme: &Theme,
    label: &str,
    sub: &str,
    scope: &Scope,
    w: f32,
    h: f32,
) {
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = theme.spacing_sm.value();
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = theme.spacing_sm.value();
            ui.label(
                egui::RichText::new(label)
                    .size(theme.font_size_body.value())
                    .strong()
                    .color(theme.text_primary().to_egui()),
            );
            ui.label(
                egui::RichText::new(sub)
                    .monospace()
                    .size(theme.font_size_micro.value())
                    .color(theme.text_muted().to_egui()),
            );
        });
        // LivePreview outer: padding 10, bg-app, radius, 1px border-default.
        let (canvas, _) = ui.allocate_exact_size(egui::vec2(w, h), egui::Sense::hover());
        let radius = theme.corner_radius.value();
        let bw = theme.border_width.value();
        let p = ui.painter_at(canvas);
        p.rect_filled(canvas, radius, theme.bg_app().to_egui());
        p.rect_stroke(
            canvas,
            radius,
            egui::Stroke::new(bw, theme.border_default().to_egui()),
            egui::StrokeKind::Inside,
        );
        draw_scope_body(ui, theme, canvas.shrink(theme.spacing_sm.value()), scope);
    });
}

/// 라벨 붙은 **편집 상태** scope 데모 한 칸 — selected surface 기준.
fn scope_demo_edit(
    ui: &mut egui::Ui,
    theme: &Theme,
    label: &str,
    sub: &str,
    surf: &Surf,
    selected: usize,
    w: f32,
    h: f32,
) {
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = theme.spacing_sm.value();
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = theme.spacing_sm.value();
            ui.label(
                egui::RichText::new(label)
                    .size(theme.font_size_body.value())
                    .strong()
                    .color(theme.text_primary().to_egui()),
            );
            ui.label(
                egui::RichText::new(sub)
                    .monospace()
                    .size(theme.font_size_micro.value())
                    .color(theme.text_muted().to_egui()),
            );
        });
        let (canvas, _) = ui.allocate_exact_size(egui::vec2(w, h), egui::Sense::hover());
        let radius = theme.corner_radius.value();
        let bw = theme.border_width.value();
        let p = ui.painter_at(canvas);
        p.rect_filled(canvas, radius, theme.bg_app().to_egui());
        p.rect_stroke(
            canvas,
            radius,
            egui::Stroke::new(bw, theme.border_default().to_egui()),
            egui::StrokeKind::Inside,
        );
        draw_scope_body_edit(
            ui,
            theme,
            canvas.shrink(theme.spacing_sm.value()),
            surf,
            selected,
        );
    });
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

fn text_width(ui: &egui::Ui, text: &str, font: egui::FontId) -> f32 {
    ui.fonts(|f| {
        f.layout_no_wrap(text.to_owned(), font, egui::Color32::PLACEHOLDER)
            .size()
            .x
    })
}

pub fn draw(ui: &mut egui::Ui, theme: &Theme) {
    let workspace = build_workspace();
    let tab_scope = build_tab();
    let pane_scope = build_pane();

    spec::stage(ui, theme, StageVariant::Wrap, |ui| {
        scope_demo(
            ui,
            theme,
            "Workspace",
            "pane split + tabs + surface split",
            &workspace,
            320.0,
            220.0,
        );
        scope_demo(
            ui,
            theme,
            "Tab",
            "surface split tree only",
            &tab_scope,
            210.0,
            220.0,
        );
        scope_demo(
            ui,
            theme,
            "Pane",
            "tab strip + active tab",
            &pane_scope,
            210.0,
            220.0,
        );
    });

    // 편집 상태(Phase 2 / TODO 09): selected surface 2px accent outline + handle
    // cluster + inline leaf form. 선택 = Terminal leaf(startup 필드 노출).
    let edit_tab = build_tab();
    let edit_surf = match &edit_tab {
        Scope::TabFrame(s) => s,
        Scope::PaneTree(_) => unreachable!("build_tab is a TabFrame"),
    };
    spec::stage(ui, theme, StageVariant::Wrap, |ui| {
        scope_demo_edit(
            ui,
            theme,
            "Edit mode",
            "selected surface + handles + form",
            edit_surf,
            1,
            300.0,
            240.0,
        );
    });

    spec::meta(
        ui,
        theme,
        &[
            ("pane split", "bordered cards · 5px app-bg gap"),
            ("tab strip", "20px mini row · 2px accent bar"),
            ("surface split", "1px hairline (lower layout)"),
            ("leaf", "kind icon + label, centered (mono)"),
            ("interactive", "mini tabs switch live (in app)"),
            ("edit: selected", "2px accent outline + handle cluster"),
            ("edit: handles", "split-right · split-down · remove"),
            ("edit: form", "kind / cwd / startup (terminal only)"),
        ],
        &[
            TokenChip::new("bg-app", "leaf fill / pane gap", theme.bg_app().to_egui()),
            TokenChip::new(
                "border-default",
                "pane card / surface hairline",
                theme.border_default().to_egui(),
            ),
            TokenChip::new(
                "accent-primary",
                "active mini tab bar",
                theme.accent_primary().to_egui(),
            ),
            TokenChip::new(
                "accent-success",
                "kind dot (terminal)",
                theme.accent_success().to_egui(),
            ),
        ],
    );

    spec::note(
        ui,
        theme,
        "Two split levels read by weight — a heavy bg-app gap + border for pane (upper) splits, a \
         1px hairline for surface (lower) splits. A leaf shows only its kind (Terminal / Markdown / \
         Editor / Log), never its contents. The Edit-mode stage shows the WYSIWYG state: every \
         surface gets a faint 1px separator outline, the selected surface gets a 2px accent inset \
         outline + corner handle cluster (split-right=row / split-down=col / remove=danger), and \
         its center label is replaced by the inline leaf form (kind / cwd / startup — startup only \
         when kind=terminal). Mini-tab click-to-switch and live editing are wired in the host \
         component (TODO 08/09).",
    );

    spec::dont(
        ui,
        theme,
        "Don't render surface contents. A leaf shows only its kind — the preview is about \
         structure, not data.",
    );
}
