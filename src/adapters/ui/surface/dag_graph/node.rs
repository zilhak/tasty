//! 노드 카드 한 장.
//!
//! **박스 크기는 LOD 3 단계 전부에서 동일**하다(`dag-node-width`×`dag-node-height`).
//! 줌아웃에서 카드를 실제로 줄이면 레이아웃 좌표까지 다시 계산해야 하고, 그러면
//! 줌 조작 중에 그래프 모양이 흔들린다. 바뀌는 건 **안에 들어가는 내용**뿐이다.
//!
//! # 상태를 3 중으로 표기한다
//!
//! 색 하나로 상태를 표기하면 색각 이상 사용자에게는 정보가 통째로 사라진다. 그래서
//! 같은 상태를 (1) 좌측 3px 상태 바 색, (2) 모노 글리프, (3) 철자 라벨 세 채널로
//! 동시에 표기한다 — 색을 못 봐도 글리프와 라벨이 남는다.

use tasty_design_tokens::generated::component::dag::NODE_DIM_OPACITY;
use tasty_type_appearance::color::HexColor;
use tasty_type_appearance::theme::Theme;

use super::model::{DagNodeData, DagStatus, node_duration};
use super::view::Lod;

/// 카드 한 장의 그리기 상태.
pub struct NodeVisual {
    /// 이 배율에서 카드 안에 무엇을 넣을지.
    pub lod: Lod,
    /// 화면 배율 — 폰트/패딩만 따라 커진다. 박스 크기는 이것과 무관하게 고정이다.
    pub zoom: f32,
    /// duration 계산 기준 시각(epoch ms).
    pub now_ms: u64,
    pub selected: bool,
    pub hovered: bool,
    /// 실패/취소 상류 때문에 절대 실행되지 않을 경로.
    pub dimmed: bool,
    /// 사이클을 이루는 노드.
    pub in_cycle: bool,
}

/// 상태별 (바 색, 배지 배경, 라벨 색).
pub fn status_colors(theme: &Theme, status: DagStatus) -> (HexColor, HexColor, HexColor) {
    match status {
        DagStatus::Waiting => (
            theme.dag_status_waiting(),
            theme.dag_status_waiting_bg(),
            theme.dag_status_waiting_label(),
        ),
        DagStatus::Ready => (
            theme.dag_status_ready(),
            theme.dag_status_ready_bg(),
            theme.dag_status_ready_label(),
        ),
        DagStatus::Running => (
            theme.dag_status_running(),
            theme.dag_status_running_bg(),
            theme.dag_status_running_label(),
        ),
        DagStatus::Succeeded => (
            theme.dag_status_succeeded(),
            theme.dag_status_succeeded_bg(),
            theme.dag_status_succeeded_label(),
        ),
        DagStatus::Failed => (
            theme.dag_status_failed(),
            theme.dag_status_failed_bg(),
            theme.dag_status_failed_label(),
        ),
        DagStatus::Cancelled => (
            theme.dag_status_cancelled(),
            theme.dag_status_cancelled_bg(),
            theme.dag_status_cancelled_label(),
        ),
        DagStatus::Skipped => (
            theme.dag_status_skipped(),
            theme.dag_status_skipped_bg(),
            theme.dag_status_skipped_label(),
        ),
        DagStatus::Unknown => (
            theme.dag_status_unknown(),
            theme.dag_status_unknown_bg(),
            theme.dag_status_unknown_label(),
        ),
    }
}

/// dim 을 적용해 egui 색으로. `dimmed` 가 아니면 원색 그대로다.
fn tone(color: HexColor, dimmed: bool) -> egui::Color32 {
    let c = color.to_egui();
    if dimmed {
        c.gamma_multiply(NODE_DIM_OPACITY)
    } else {
        c
    }
}

/// 카드 한 장을 그린다. `rect` 는 이미 화면 좌표로 변환된 박스, `zoom` 은 폰트
/// 크기를 맞추는 데만 쓴다.
pub fn paint_node(
    painter: &egui::Painter,
    theme: &Theme,
    rect: egui::Rect,
    node: &DagNodeData,
    vis: &NodeVisual,
) {
    let (bar, badge_bg, label_fg) = status_colors(theme, node.status);
    let dim = vis.dimmed;
    let (lod, zoom, now_ms) = (vis.lod, vis.zoom, vis.now_ms);
    let radius = (theme.dag_node_radius().value() * zoom).round();

    // 바탕.
    let bg = if vis.hovered {
        theme.dag_node_hover_bg()
    } else if lod == Lod::Block {
        // block tier 는 텍스트가 없으니 상태 배경으로 채워 색만으로 읽히게 한다.
        badge_bg
    } else {
        theme.dag_node_bg()
    };
    painter.rect_filled(rect, radius, tone(bg, dim));

    // 테두리 — 사이클 노드는 경고색으로 승격한다(배너와 같은 색 어휘).
    let border = if vis.in_cycle {
        theme.dag_cycle_border()
    } else {
        theme.dag_node_border()
    };
    painter.rect_stroke(
        rect,
        radius,
        egui::Stroke::new(theme.dag_edge_width().value(), tone(border, dim)),
        egui::StrokeKind::Inside,
    );

    // 좌측 상태 바 (채널 1). 카드 좌변에 붙고 radius 만큼만 둥글다.
    let bar_w = (theme.dag_node_bar_width().value() * zoom).max(1.0);
    painter.rect_filled(
        egui::Rect::from_min_max(rect.min, egui::pos2(rect.min.x + bar_w, rect.max.y)),
        radius,
        tone(bar, dim),
    );

    if lod == Lod::Block {
        return;
    }

    let pad_x = theme.dag_node_padding_x().value() * zoom;
    let pad_y = theme.dag_node_padding_y().value() * zoom;
    let inner = egui::Rect::from_min_max(
        egui::pos2(rect.min.x + bar_w + pad_x, rect.min.y + pad_y),
        egui::pos2(rect.max.x - pad_x, rect.max.y - pad_y),
    );
    if inner.width() <= 0.0 || inner.height() <= 0.0 {
        return;
    }

    let name_font = egui::FontId::proportional(theme.dag_node_name_font_size().value() * zoom);
    let meta_font = egui::FontId::monospace(theme.dag_node_meta_font_size().value() * zoom);
    let name_fg = tone(theme.dag_node_fg(), dim);

    if lod == Lod::Compact {
        // 글리프(채널 2) + 이름. 라벨/시간은 이 배율에서 읽히지 않으니 뺀다.
        let glyph = format!("{} ", node.status.glyph());
        let gw = text_width(painter, &glyph, &meta_font);
        painter.text(
            inner.left_center(),
            egui::Align2::LEFT_CENTER,
            glyph,
            meta_font,
            tone(bar, dim),
        );
        let avail = inner.width() - gw;
        painter.text(
            egui::pos2(inner.min.x + gw, inner.center().y),
            egui::Align2::LEFT_CENTER,
            ellipsize(painter, &node.name, &name_font, avail),
            name_font,
            name_fg,
        );
        return;
    }

    // full tier — 이름 행 + 메타 행.
    let row_gap = theme.dag_node_row_gap().value() * zoom;
    let name_h = painter.ctx().fonts(|f| f.row_height(&name_font));
    let meta_h = painter.ctx().fonts(|f| f.row_height(&meta_font));
    let total = name_h + row_gap + meta_h;
    let top = inner.center().y - total / 2.0;

    painter.text(
        egui::pos2(inner.min.x, top),
        egui::Align2::LEFT_TOP,
        ellipsize(painter, &node.name, &name_font, inner.width()),
        name_font,
        name_fg,
    );

    // 메타 행: 글리프 + 상태 라벨(채널 3) + duration.
    let meta_y = top + name_h + row_gap;
    let mut x = inner.min.x;
    let glyph = format!("{} ", node.status.glyph());
    let gw = text_width(painter, &glyph, &meta_font);
    painter.text(
        egui::pos2(x, meta_y),
        egui::Align2::LEFT_TOP,
        glyph,
        meta_font.clone(),
        tone(bar, dim),
    );
    x += gw;

    // duration 은 우측 정렬로 먼저 자리를 잡는다 — 폭이 모자라면 **상태 라벨이 먼저**
    // 줄어든다. 라벨은 글리프·바 색과 중복되는 세 번째 채널이지만 duration 은 다른
    // 어디에도 없는 정보라, 하나를 버려야 하면 중복된 쪽을 버린다.
    let duration = node_duration(node, now_ms);
    let mut right = inner.max.x;
    if let Some(d) = &duration {
        let dw = text_width(painter, d, &meta_font);
        if dw <= inner.width() - gw {
            painter.text(
                egui::pos2(right, meta_y),
                egui::Align2::RIGHT_TOP,
                d,
                meta_font.clone(),
                tone(theme.dag_node_meta_fg(), dim),
            );
            right -= dw + theme.dag_node_gap().value() * zoom;
        }
    }
    let label_w = (right - x).max(0.0);
    if label_w > 0.0 {
        painter.text(
            egui::pos2(x, meta_y),
            egui::Align2::LEFT_TOP,
            ellipsize(painter, node.status.label(), &meta_font, label_w),
            meta_font,
            tone(label_fg, dim),
        );
    }
}

/// 선택 링. 카드 **바깥**에 그려 카드 내용 폭을 잡아먹지 않는다.
pub fn paint_selection_ring(painter: &egui::Painter, theme: &Theme, rect: egui::Rect, zoom: f32) {
    let w = (theme.dag_node_selected_ring_width().value() * zoom).max(1.0);
    painter.rect_stroke(
        rect.expand(w),
        (theme.dag_node_radius().value() * zoom + w).round(),
        egui::Stroke::new(w, theme.dag_node_selected_ring().to_egui()),
        egui::StrokeKind::Middle,
    );
}

fn text_width(painter: &egui::Painter, text: &str, font: &egui::FontId) -> f32 {
    painter.ctx().fonts(|f| {
        f.layout_no_wrap(text.to_string(), font.clone(), egui::Color32::WHITE)
            .size()
            .x
    })
}

/// 폭에 맞게 말줄임. 문자 단위로 자르되 **char 경계**를 지킨다 — 바이트로 자르면
/// 한글/일본어 이름에서 panic 한다.
fn ellipsize(painter: &egui::Painter, text: &str, font: &egui::FontId, max_w: f32) -> String {
    if max_w <= 0.0 {
        return String::new();
    }
    if text_width(painter, text, font) <= max_w {
        return text.to_string();
    }
    let chars: Vec<char> = text.chars().collect();
    let mut lo = 0usize;
    let mut hi = chars.len();
    while lo < hi {
        let mid = lo.midpoint(hi + 1).min(chars.len());
        let cand: String = chars[..mid].iter().collect::<String>() + "…";
        if text_width(painter, &cand, font) <= max_w {
            lo = mid;
        } else {
            hi = mid - 1;
        }
        if mid == lo && lo == hi {
            break;
        }
    }
    if lo == 0 {
        return "…".to_string();
    }
    chars[..lo].iter().collect::<String>() + "…"
}
