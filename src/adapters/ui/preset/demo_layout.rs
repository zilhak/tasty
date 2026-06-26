//! Preset 데모 레이아웃 미리보기 위젯 (본체, read-only — TODO 07 Phase 1).
//!
//! 저장된 `Preset*` 트리를 받아 **구조만** 축소 렌더한다 — pane split(상위) /
//! tab strip / surface split(하위) / surface leaf(kind 표시명)을 서로 다른 시각
//! weight 로 그려 계층이 라벨 없이 읽히게 한다. 라이브 surface 렌더(터미널 GPU
//! 패스 / WebView)는 **재사용하지 않는다** — 전용 placeholder 위젯이다.
//!
//! 갤러리 specimen `crates/tasty-gallery/src/catalog/components/preset_editor.rs`
//! 의 시각 구조를 본체 데이터·아이콘으로 1:1 대응시킨다. 차이점:
//!  - 입력이 정적 샘플이 아니라 실제 `WorkspacePreset`/`TabPreset`/`PanePreset`.
//!  - leaf 라벨을 주입된 resolver(런타임 kind→표시명)로 해석.
//!  - mini-tab 이 **live** — 클릭 시 미리보기의 active 탭만 바꾼다(저장본 불변).
//!
//! 3종 구조 레벨의 시각 weight (디자인 changelog 2026-06-25):
//!  - Pane split (상위) → 테두리 카드 + **5px bg-app gap** (무거운 divider).
//!  - Surface split (하위) → **1px border-default hairline** (가벼운 divider).
//!  - Surface leaf → kind 아이콘(accent) + 표시명(가운데, mono). 내용 렌더 안 함.
//!  - Mini tab strip → 20px, bg-sidebar. 활성 = bg-panel + 2px accent 하단 bar + kind 아이콘.

use tasty_presets::{
    PanePreset, PresetPane, PresetPaneNode, PresetSplitDirection, PresetSurfaceLayout, PresetTab,
    TabPreset, WorkspacePreset,
};
use tasty_type_appearance::theme::Theme;

use crate::adapters::ui::icons::{self, Icon};
use crate::i18n::t;

// 디자인 고정 px (Theme 에 대응 토큰 없는 preview 전용 치수 — specimen 과 동일).
/// 상위(pane) divider = bordered 카드 사이 bg-app 공백.
const PANE_GAP: f32 = 5.0;
/// mini tab strip height.
const STRIP_H: f32 = 20.0;
/// 활성 탭 본문 padding.
const BODY_PAD: f32 = 3.0;
/// surface leaf 아이콘↔라벨 gap.
const LEAF_GAP: f32 = 6.0;
/// mini tab 좌우 padding.
const TAB_PAD_X: f32 = 9.0;
/// mini tab 아이콘↔라벨 gap.
const TAB_GAP: f32 = 5.0;

// ── kind 시각 매핑 (아이콘 + accent) ────────────────────────────────────
//
// 표시명(label)은 registry/i18n 으로 해석하지만, *아이콘과 accent 색*은 본질적으로
// 시각 매핑이라 kind 문자열로 직접 결정한다(`tab_bar::kind_icon` 과 동일 idiom).
// 미지정 kind 는 중립(FILE + text-secondary)으로 떨어진다 — plugin/remote kind 안전.

fn kind_icon(kind: &str) -> Icon {
    match kind {
        "markdown" => icons::MD,
        "explorer" => icons::FOLDER,
        "image" => icons::IMAGE,
        "terminal" | "attached" => icons::TERM,
        _ => icons::FILE,
    }
}

fn kind_accent(theme: &Theme, kind: &str) -> egui::Color32 {
    match kind {
        "terminal" | "attached" => theme.accent_success().to_egui(),
        "markdown" => theme.accent_primary().to_egui(),
        "image" => theme.accent_info().to_egui(),
        "explorer" => theme.accent_agent().to_egui(),
        // 미지정 kind: accent 없이 중립(라벨과 같은 secondary).
        _ => theme.text_secondary().to_egui(),
    }
}

/// 레지스트리 없는 컨텍스트(현재 `PresetView` 윈도우는 `CoreState` 미접근)용
/// fallback kind→표시명 해석기.
///
/// `surface.kind.<kind>` i18n 키를 시도하고(= registry `display_name_i18n_key`
/// 규약과 동일 키. builtin/plugin 모두 이 네임스페이스를 쓴다), 미번역이면 kind
/// 첫 글자를 대문자로(`convert.rs::resolve_label` 의 capitalize fallback 패턴).
///
/// TODO 08(화면 통합)에서 `PresetView` 에 registry 가 주입되면, registry
/// `kinds_snapshot()`/`get()` 기반 resolver 로 교체할 자리.
pub fn fallback_kind_label(kind: &str) -> String {
    let key = format!("surface.kind.{kind}");
    let tr = t(&key);
    if tr != key.as_str() {
        return tr.to_string();
    }
    let mut c = kind.chars();
    match c.next() {
        None => String::new(),
        Some(f) => f.to_ascii_uppercase().to_string() + c.as_str(),
    }
}

// ── 정규화된 preview 모델 ───────────────────────────────────────────────
//
// 3종 preset(`Workspace`/`Tab`/`Pane`)을 공통 preview 모델로 정규화한다 — 위젯은
// 단일 타입만 받아 분기를 최소화한다(Codex 크로스체크 제안). 라벨은 build 시
// resolver 로 미리 해석해 둔다(렌더러는 registry/i18n 비의존).

/// surface leaf — kind 식별자 + 미리 해석된 표시명.
#[derive(Clone, Debug, PartialEq)]
struct Leaf {
    kind: String,
    label: String,
}

/// 하위 레이아웃(탭 안의 surface split).
#[derive(Clone, Debug, PartialEq)]
enum SurfNode {
    Leaf(Leaf),
    Split {
        row: bool,
        ratio: f32,
        first: Box<SurfNode>,
        second: Box<SurfNode>,
    },
}

impl SurfNode {
    /// 탭 대표 kind = 첫 leaf (디자인 `activeKind` — mini-tab 아이콘 구동).
    fn rep_kind(&self) -> &str {
        let mut n = self;
        loop {
            match n {
                SurfNode::Leaf(l) => return &l.kind,
                SurfNode::Split { first, .. } => n = first,
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
struct PreviewTab {
    name: String,
    layout: SurfNode,
}

#[derive(Clone, Debug, PartialEq)]
struct PreviewPane {
    /// 안정 식별자(build 시 부여) — 탭 클릭 상호작용 id + active override 키.
    id: usize,
    tabs: Vec<PreviewTab>,
    active: usize,
}

/// 상위 레이아웃(pane split).
#[derive(Clone, Debug, PartialEq)]
enum PaneNode {
    Leaf(PreviewPane),
    Split {
        row: bool,
        ratio: f32,
        first: Box<PaneNode>,
        second: Box<PaneNode>,
    },
}

/// scope variant — Workspace/Pane 은 pane 트리, Tab 은 단일 surface-split 프레임.
#[derive(Clone, Debug, PartialEq)]
enum Root {
    Panes(PaneNode),
    /// Tab scope: strip 없이 단일 탭 본문처럼 프레임.
    TabFrame(SurfNode),
}

/// 정규화된 preview 트리. 라이브 상호작용(active 탭)은 트리 안에 보관된다 —
/// 호출자가 프레임 간 인스턴스를 유지(`Clone`)하면 클릭 전환이 지속된다.
#[derive(Clone, Debug, PartialEq)]
pub struct DemoLayout {
    root: Root,
}

/// build 동안 pane id 를 0..N 으로 부여하는 카운터.
struct IdGen(usize);
impl IdGen {
    fn next(&mut self) -> usize {
        let id = self.0;
        self.0 += 1;
        id
    }
}

fn norm_surf(node: &PresetSurfaceLayout, resolve: &dyn Fn(&str) -> String) -> SurfNode {
    match node {
        PresetSurfaceLayout::Leaf { surface } => SurfNode::Leaf(Leaf {
            kind: surface.kind.clone(),
            label: resolve(&surface.kind),
        }),
        PresetSurfaceLayout::Split {
            direction,
            ratio,
            first,
            second,
        } => SurfNode::Split {
            row: is_row(*direction),
            ratio: *ratio,
            first: Box::new(norm_surf(first, resolve)),
            second: Box::new(norm_surf(second, resolve)),
        },
    }
}

fn norm_tab(tab: &PresetTab, resolve: &dyn Fn(&str) -> String) -> PreviewTab {
    let layout = norm_surf(&tab.layout, resolve);
    // explicit_name 우선, 없으면 대표 surface 의 표시명(디자인의 자동 탭 이름 규칙).
    let name = tab
        .explicit_name
        .clone()
        .unwrap_or_else(|| resolve(layout.rep_kind()));
    PreviewTab { name, layout }
}

fn norm_pane(pane: &PresetPane, resolve: &dyn Fn(&str) -> String, ids: &mut IdGen) -> PreviewPane {
    let tabs: Vec<PreviewTab> = pane.tabs.iter().map(|t| norm_tab(t, resolve)).collect();
    let active = pane.active_tab.min(tabs.len().saturating_sub(1));
    PreviewPane {
        id: ids.next(),
        tabs,
        active,
    }
}

fn norm_pane_node(
    node: &PresetPaneNode,
    resolve: &dyn Fn(&str) -> String,
    ids: &mut IdGen,
) -> PaneNode {
    match node {
        PresetPaneNode::Leaf { pane } => PaneNode::Leaf(norm_pane(pane, resolve, ids)),
        PresetPaneNode::Split {
            direction,
            ratio,
            first,
            second,
        } => PaneNode::Split {
            row: is_row(*direction),
            ratio: *ratio,
            first: Box::new(norm_pane_node(first, resolve, ids)),
            second: Box::new(norm_pane_node(second, resolve, ids)),
        },
    }
}

/// 라이브 모델 의미(`tasty-type-geometry::SplitDirection`)와 동일하게:
/// `Vertical` = 폭 분할(좌우, row), `Horizontal` = 높이 분할(상하, column).
/// capture/apply 와 일치시켜 미리보기가 실제 적용 결과와 같은 방향으로 읽히게 한다.
fn is_row(d: PresetSplitDirection) -> bool {
    matches!(d, PresetSplitDirection::Vertical)
}

impl DemoLayout {
    pub fn from_workspace(p: &WorkspacePreset, resolve: impl Fn(&str) -> String) -> Self {
        let mut ids = IdGen(0);
        Self {
            root: Root::Panes(norm_pane_node(&p.layout, &resolve, &mut ids)),
        }
    }

    pub fn from_tab(p: &TabPreset, resolve: impl Fn(&str) -> String) -> Self {
        Self {
            root: Root::TabFrame(norm_surf(&p.tab.layout, &resolve)),
        }
    }

    pub fn from_pane(p: &PanePreset, resolve: impl Fn(&str) -> String) -> Self {
        let mut ids = IdGen(0);
        Self {
            root: Root::Panes(PaneNode::Leaf(norm_pane(&p.pane, &resolve, &mut ids))),
        }
    }

    /// `rect` 안에 미리보기를 그리고 탭 클릭 상호작용을 처리한다.
    /// 탭 클릭으로 active 가 바뀌면 `true` 를 반환한다(호출자 repaint 신호).
    pub fn show(&mut self, ui: &mut egui::Ui, theme: &Theme, rect: egui::Rect) -> bool {
        let mut clicked: Option<(usize, usize)> = None;
        match &self.root {
            Root::Panes(node) => draw_pane_tree(ui, theme, rect, node, &mut clicked),
            Root::TabFrame(node) => draw_tab_frame(ui, theme, rect, node),
        }
        match clicked {
            Some((pane_id, idx)) => self.set_active(pane_id, idx),
            None => false,
        }
    }

    /// pane_id 의 active 탭을 idx 로 바꾼다. 실제로 변하면 true.
    fn set_active(&mut self, pane_id: usize, idx: usize) -> bool {
        fn walk(node: &mut PaneNode, pane_id: usize, idx: usize) -> bool {
            match node {
                PaneNode::Leaf(pane) => {
                    if pane.id == pane_id && idx < pane.tabs.len() && pane.active != idx {
                        pane.active = idx;
                        return true;
                    }
                    false
                }
                PaneNode::Split { first, second, .. } => {
                    walk(first, pane_id, idx) || walk(second, pane_id, idx)
                }
            }
        }
        match &mut self.root {
            Root::Panes(node) => walk(node, pane_id, idx),
            Root::TabFrame(_) => false,
        }
    }
}

// ── rect 분할 헬퍼 (specimen 과 동일) ───────────────────────────────────

/// `rect` 를 비율로 나눈다. divider 만큼을 가운데 띠로 빼고 first/second 분배.
/// 반환 = (first, divider, second).
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

// ── 재귀 렌더 ───────────────────────────────────────────────────────────

/// 하위 레이아웃(surface split). Leaf = kind 박스, Split = 1px hairline.
fn draw_surf(ui: &mut egui::Ui, theme: &Theme, rect: egui::Rect, node: &SurfNode) {
    match node {
        SurfNode::Leaf(l) => draw_surface_box(ui, theme, rect, l),
        SurfNode::Split {
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
fn draw_surface_box(ui: &mut egui::Ui, theme: &Theme, rect: egui::Rect, leaf: &Leaf) {
    let p = ui.painter_at(rect);
    p.rect_filled(rect, 0.0, theme.bg_app().to_egui());

    let icon = theme.icon_glyph_size_md.value();
    let label_h = theme.font_size_caption.value();
    let total = icon + LEAF_GAP + label_h;
    let icon_cy = rect.center().y - total * 0.5 + icon * 0.5;
    paint_icon(
        ui,
        kind_icon(&leaf.kind),
        egui::pos2(rect.center().x, icon_cy),
        icon,
        kind_accent(theme, &leaf.kind),
    );
    // painter_at 가 rect 로 clip 하므로 좁은 leaf 에서도 라벨이 넘치지 않는다.
    ui.painter_at(rect).text(
        egui::pos2(
            rect.center().x,
            icon_cy + icon * 0.5 + LEAF_GAP + label_h * 0.5,
        ),
        egui::Align2::CENTER_CENTER,
        &leaf.label,
        egui::FontId::monospace(label_h),
        theme.text_secondary().to_egui(),
    );
}

/// 상위 레이아웃(pane split). Leaf = pane 카드, Split = 5px bg-app gap.
fn draw_pane_tree(
    ui: &mut egui::Ui,
    theme: &Theme,
    rect: egui::Rect,
    node: &PaneNode,
    clicked: &mut Option<(usize, usize)>,
) {
    match node {
        PaneNode::Leaf(pane) => draw_pane_card(ui, theme, rect, pane, clicked),
        PaneNode::Split {
            row,
            ratio,
            first,
            second,
        } => {
            // divider(PANE_GAP)는 칠하지 않는다 — bg-app 공백이 무거운 상위 divider.
            let (r1, _gap, r2) = split_rects(rect, *row, *ratio, PANE_GAP);
            draw_pane_tree(ui, theme, r1, first, clicked);
            draw_pane_tree(ui, theme, r2, second, clicked);
        }
    }
}

/// pane 카드 = 테두리 카드 + mini tab strip(클릭 가능) + 활성 탭의 surface 레이아웃.
fn draw_pane_card(
    ui: &mut egui::Ui,
    theme: &Theme,
    rect: egui::Rect,
    pane: &PreviewPane,
    clicked: &mut Option<(usize, usize)>,
) {
    let radius = theme.corner_radius.value();
    let bw = theme.border_width.value();
    let sep = theme.separator.to_egui();
    let p = ui.painter_at(rect);
    p.rect_filled(rect, radius, theme.bg_app().to_egui());

    // mini tab strip 배경.
    let strip = egui::Rect::from_min_size(rect.min, egui::vec2(rect.width(), STRIP_H));
    p.rect_filled(strip, 0.0, theme.bg_sidebar().to_egui());

    let tab_font = egui::FontId::proportional(theme.font_size_caption.value());
    let icon_sz = theme.icon_glyph_size_sm.value();
    let mut x = strip.min.x;
    for (i, t) in pane.tabs.iter().enumerate() {
        let on = i == pane.active;
        let lw = text_width(ui, &t.name, tab_font.clone());
        let tw = TAB_PAD_X + icon_sz + TAB_GAP + lw + TAB_PAD_X;
        let tab_rect =
            egui::Rect::from_min_size(egui::pos2(x, strip.min.y), egui::vec2(tw, STRIP_H));

        // 클릭 상호작용 — active 가 아닌 탭만 pointer + 클릭.
        let resp = ui.interact(
            tab_rect,
            ui.id().with(("preset_demo_tab", pane.id, i)),
            egui::Sense::click(),
        );
        if !on && resp.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        if resp.clicked() {
            *clicked = Some((pane.id, i));
        }

        let rep = t.layout.rep_kind();
        let p = ui.painter_at(strip);
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
            // 탭 사이 separator(borderRight).
            p.vline(x, strip.y_range(), egui::Stroke::new(bw, sep));
        }
        let icon_c = egui::pos2(
            tab_rect.min.x + TAB_PAD_X + icon_sz * 0.5,
            tab_rect.center().y,
        );
        let icon_color = if on {
            kind_accent(theme, rep)
        } else {
            theme.text_muted().to_egui()
        };
        paint_icon(ui, kind_icon(rep), icon_c, icon_sz, icon_color);
        ui.painter_at(strip).text(
            egui::pos2(
                tab_rect.min.x + TAB_PAD_X + icon_sz + TAB_GAP,
                tab_rect.center().y,
            ),
            egui::Align2::LEFT_CENTER,
            &t.name,
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
    if let Some(t) = pane.tabs.get(pane.active).or_else(|| pane.tabs.first()) {
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

/// Tab scope — strip 없이 단일 탭 본문처럼 프레임(테두리 + radius + padding 3).
fn draw_tab_frame(ui: &mut egui::Ui, theme: &Theme, rect: egui::Rect, node: &SurfNode) {
    let radius = theme.corner_radius.value();
    let bw = theme.border_width.value();
    let p = ui.painter_at(rect);
    p.rect_filled(rect, radius, theme.bg_app().to_egui());
    draw_surf(ui, theme, rect.shrink(BODY_PAD), node);
    ui.painter_at(rect).rect_stroke(
        rect,
        radius,
        egui::Stroke::new(bw, theme.border_default().to_egui()),
        egui::StrokeKind::Inside,
    );
}

fn paint_icon(ui: &mut egui::Ui, icon: Icon, center: egui::Pos2, size: f32, color: egui::Color32) {
    let r = egui::Rect::from_center_size(center, egui::vec2(size, size));
    icon.image(size, color).paint_at(ui, r);
}

fn text_width(ui: &egui::Ui, text: &str, font: egui::FontId) -> f32 {
    ui.fonts(|f| {
        f.layout_no_wrap(text.to_owned(), font, egui::Color32::PLACEHOLDER)
            .size()
            .x
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tasty_presets::{PresetPane, PresetSurface, PresetSurfaceLayout, PresetTab};

    fn surf(kind: &str) -> PresetSurfaceLayout {
        PresetSurfaceLayout::Leaf {
            surface: PresetSurface {
                kind: kind.into(),
                cwd: None,
                startup_command: None,
                params: serde_json::Value::Null,
            },
        }
    }

    fn ssplit(
        d: PresetSplitDirection,
        r: f32,
        a: PresetSurfaceLayout,
        b: PresetSurfaceLayout,
    ) -> PresetSurfaceLayout {
        PresetSurfaceLayout::Split {
            direction: d,
            ratio: r,
            first: Box::new(a),
            second: Box::new(b),
        }
    }

    /// 테스트용 resolver — registry 없이 kind 를 그대로 대문자 라벨로.
    fn up(kind: &str) -> String {
        let mut c = kind.chars();
        match c.next() {
            None => String::new(),
            Some(f) => f.to_ascii_uppercase().to_string() + c.as_str(),
        }
    }

    #[test]
    fn vertical_split_is_row_horizontal_is_column() {
        // 라이브 모델 의미와 일치: Vertical=좌우(row), Horizontal=상하(column).
        assert!(is_row(PresetSplitDirection::Vertical));
        assert!(!is_row(PresetSplitDirection::Horizontal));
    }

    #[test]
    fn normalizes_tab_preset_and_resolves_labels() {
        let p = TabPreset {
            name: "t".into(),
            tab: PresetTab {
                explicit_name: None,
                layout: ssplit(
                    PresetSplitDirection::Vertical,
                    0.5,
                    surf("terminal"),
                    surf("markdown"),
                ),
            },
        };
        let dl = DemoLayout::from_tab(&p, up);
        match &dl.root {
            Root::TabFrame(SurfNode::Split {
                row,
                ratio,
                first,
                second,
            }) => {
                assert!(*row);
                assert_eq!(*ratio, 0.5);
                assert!(matches!(first.as_ref(), SurfNode::Leaf(l) if l.label == "Terminal"));
                assert!(matches!(second.as_ref(), SurfNode::Leaf(l) if l.label == "Markdown"));
            }
            _ => panic!("expected TabFrame split"),
        }
    }

    #[test]
    fn tab_name_falls_back_to_rep_kind_label() {
        // explicit_name 없으면 대표(첫) leaf 의 표시명을 탭 이름으로.
        let pane = PresetPane {
            tabs: vec![PresetTab {
                explicit_name: None,
                layout: ssplit(
                    PresetSplitDirection::Horizontal,
                    0.5,
                    surf("markdown"),
                    surf("terminal"),
                ),
            }],
            active_tab: 0,
        };
        let p = PanePreset {
            name: "p".into(),
            pane,
        };
        let dl = DemoLayout::from_pane(&p, up);
        match &dl.root {
            Root::Panes(PaneNode::Leaf(pp)) => {
                assert_eq!(pp.tabs[0].name, "Markdown");
            }
            _ => panic!("expected single pane"),
        }
    }

    #[test]
    fn active_tab_is_clamped_to_range() {
        let pane = PresetPane {
            tabs: vec![
                PresetTab {
                    explicit_name: Some("a".into()),
                    layout: surf("terminal"),
                },
                PresetTab {
                    explicit_name: Some("b".into()),
                    layout: surf("terminal"),
                },
            ],
            active_tab: 9, // 범위 밖
        };
        let p = PanePreset {
            name: "p".into(),
            pane,
        };
        let dl = DemoLayout::from_pane(&p, up);
        match &dl.root {
            Root::Panes(PaneNode::Leaf(pp)) => assert_eq!(pp.active, 1),
            _ => panic!(),
        }
    }

    #[test]
    fn set_active_switches_only_on_real_change() {
        let pane = PresetPane {
            tabs: vec![
                PresetTab {
                    explicit_name: Some("a".into()),
                    layout: surf("terminal"),
                },
                PresetTab {
                    explicit_name: Some("b".into()),
                    layout: surf("markdown"),
                },
            ],
            active_tab: 0,
        };
        let p = PanePreset {
            name: "p".into(),
            pane,
        };
        let mut dl = DemoLayout::from_pane(&p, up);
        // pane id 0 (첫 pane). 0→1 변경 = true, 다시 1→1 = false.
        assert!(dl.set_active(0, 1));
        assert!(!dl.set_active(0, 1));
        // 존재하지 않는 pane id → false.
        assert!(!dl.set_active(99, 0));
        match &dl.root {
            Root::Panes(PaneNode::Leaf(pp)) => assert_eq!(pp.active, 1),
            _ => panic!(),
        }
    }

    #[test]
    fn panes_get_unique_ids() {
        let p = WorkspacePreset {
            name: "w".into(),
            subtitle: String::new(),
            description: String::new(),
            layout: PresetPaneNode::Split {
                direction: PresetSplitDirection::Vertical,
                ratio: 0.5,
                first: Box::new(PresetPaneNode::Leaf {
                    pane: PresetPane {
                        tabs: vec![PresetTab {
                            explicit_name: Some("a".into()),
                            layout: surf("terminal"),
                        }],
                        active_tab: 0,
                    },
                }),
                second: Box::new(PresetPaneNode::Leaf {
                    pane: PresetPane {
                        tabs: vec![PresetTab {
                            explicit_name: Some("b".into()),
                            layout: surf("markdown"),
                        }],
                        active_tab: 0,
                    },
                }),
            },
        };
        let dl = DemoLayout::from_workspace(&p, up);
        let mut ids = Vec::new();
        fn collect(node: &PaneNode, ids: &mut Vec<usize>) {
            match node {
                PaneNode::Leaf(p) => ids.push(p.id),
                PaneNode::Split { first, second, .. } => {
                    collect(first, ids);
                    collect(second, ids);
                }
            }
        }
        if let Root::Panes(node) = &dl.root {
            collect(node, &mut ids);
        }
        assert_eq!(ids, vec![0, 1]);
    }
}
