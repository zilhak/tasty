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
//!
//! # 상태색은 배경·테두리까지 쓴다
//!
//! 바 하나만 상태색이면 실패 카드가 성공 카드와 같은 무게로 읽힌다. 그래서 배경은
//! 전 티어에서 상태 배경을, 테두리는 (아직 결과가 없거나 포기된 세 상태를 뺀)
//! 상태 accent 를 쓴다. 글자가 통째로 사라지는 block 티어에서는 색이 유일한
//! 채널이라 accent 를 진하게 섞어 채운다.
//!
//! 이름 행 앞의 아이콘은 **상태가 아니라 task 종류**(run/custom/reduce/barrier)를
//! 나른다 — 상태 채널과 겹치지 않는 별개 축이다.

use tasty_design_tokens::generated::component::dag::NODE_DIM_OPACITY;
use tasty_type_appearance::color::HexColor;
use tasty_type_appearance::theme::Theme;

use super::model::{DagNodeData, DagStatus, node_duration};
use super::view::Lod;
use crate::adapters::ui::icons;

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

/// 상태 바 오른쪽 본문 영역의 모서리 곡률 — 오른쪽 두 개만 카드와 같게 깎는다.
fn body_corners(radius: f32) -> egui::CornerRadius {
    let r = radius.round().clamp(0.0, u8::MAX as f32) as u8;
    egui::CornerRadius {
        nw: 0,
        sw: 0,
        ne: r,
        se: r,
    }
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

/// block 티어 채움 농도 — 시안 `color-mix(in srgb, <accent> 55%, surface-raised)`.
/// 색 혼합비는 토큰 생성기가 다루지 않아 대응 토큰이 없다(갤러리 specimen 도 같은
/// 값을 직접 쓴다).
const BLOCK_FILL_MIX: f32 = 0.55;

/// 카드 테두리 색.
///
/// 아직 결과가 없거나(waiting) 포기된(cancelled/skipped) 세 상태만 중립 테두리를
/// 쓰고 나머지는 상태 accent 를 두른다 — 실패 카드가 성공 카드와 같은 무게로
/// 읽히지 않게 하는 장치다.
pub fn status_border(theme: &Theme, status: DagStatus) -> HexColor {
    if matches!(
        status,
        DagStatus::Waiting | DagStatus::Cancelled | DagStatus::Skipped
    ) {
        theme.dag_node_border()
    } else {
        status_colors(theme, status).0
    }
}

/// task 종류 4 종의 선두 아이콘. 종류는 캔버스에서 이 아이콘으로만 구분된다.
pub fn kind_icon(command_kind: &str) -> icons::Icon {
    match command_kind {
        "custom" => icons::PLUG,
        "reduce" => icons::LAYERS,
        "wait_barrier" => icons::LOCK,
        _ => icons::TERMINAL,
    }
}

/// 카드 한 장을 그린다. `rect` 는 이미 화면 좌표로 변환된 박스, `zoom` 은 폰트
/// 크기를 맞추는 데만 쓴다.
///
/// `ui` 는 종류 아이콘(SVG 이미지)을 텍스처로 올리는 데만 필요하다 — 나머지 도형과
/// 글자는 전부 `painter` 로 그린다.
pub fn paint_node(
    ui: &egui::Ui,
    painter: &egui::Painter,
    theme: &Theme,
    rect: egui::Rect,
    node: &DagNodeData,
    vis: &NodeVisual,
) {
    let (accent, status_bg, label_fg) = status_colors(theme, node.status);
    let dim = vis.dimmed;
    let (lod, zoom, now_ms) = (vis.lod, vis.zoom, vis.now_ms);
    let radius = (theme.dag_node_radius().value() * zoom).round();
    let stroke_w = theme.border_width.value();

    // 테두리 — 사이클 노드는 경고색으로 승격한다(배너와 같은 색 어휘).
    let border = if vis.in_cycle {
        theme.dag_cycle_border()
    } else if lod == Lod::Block {
        accent
    } else {
        status_border(theme, node.status)
    };

    if lod == Lod::Block {
        // 이름도 글리프도 사라지는 티어라 **색이 상태의 유일한 채널**이다. 옅은
        // 상태 배경으로는 구분이 안 되므로 accent 를 진하게 섞어 채운다.
        let fill = theme
            .surface_raised()
            .to_egui()
            .lerp_to_gamma(accent.to_egui(), BLOCK_FILL_MIX);
        painter.rect_filled(
            rect,
            radius,
            if dim {
                fill.gamma_multiply(NODE_DIM_OPACITY)
            } else {
                fill
            },
        );
        painter.rect_stroke(
            rect,
            radius,
            egui::Stroke::new(stroke_w, tone(border, dim)),
            egui::StrokeKind::Inside,
        );
        return;
    }

    // 바탕은 전 티어에서 상태 배경이다. hover 는 그 위에 겹치는 wash 라 상태색을
    // 지우지 않는다.
    painter.rect_filled(rect, radius, tone(status_bg, dim));
    painter.rect_stroke(
        rect,
        radius,
        egui::Stroke::new(stroke_w, tone(border, dim)),
        egui::StrokeKind::Inside,
    );

    // 좌측 상태 바 (채널 1). 카드 좌변에 붙고 radius 만큼만 둥글다.
    let bar_w = (theme.dag_node_bar_width().value() * zoom).max(1.0);
    painter.rect_filled(
        egui::Rect::from_min_max(rect.min, egui::pos2(rect.min.x + bar_w, rect.max.y)),
        radius,
        tone(accent, dim),
    );

    // hover wash 는 바를 덮지 않는다 — 상태 바는 hover 중에도 원색이어야 한다.
    let body = egui::Rect::from_min_max(egui::pos2(rect.min.x + bar_w, rect.min.y), rect.max);
    if vis.hovered {
        // 오른쪽 두 모서리만 카드와 같은 곡률로 깎는다. 사각으로 칠하면 wash 가
        // 카드의 둥근 모서리 **밖으로** 삐져나와 hover 중에만 카드가 각져 보인다.
        // 왼쪽은 바가 덮고 있어 곡률이 필요 없다.
        // overlay 계열 토큰은 알파가 이미 곱해진 색이라 premultiplied 로 읽는다.
        painter.rect_filled(
            body,
            body_corners(radius),
            theme.dag_node_hover_bg().to_egui_premultiplied(),
        );
    }

    let pad_x = theme.dag_node_padding_x().value() * zoom;
    let pad_y = theme.dag_node_padding_y().value() * zoom;
    let inner = egui::Rect::from_min_max(
        egui::pos2(body.min.x + pad_x, body.min.y + pad_y),
        egui::pos2(body.max.x - pad_x, body.max.y - pad_y),
    );
    if inner.width() <= 0.0 || inner.height() <= 0.0 {
        return;
    }

    let name_font = egui::FontId::proportional(theme.dag_node_name_font_size().value() * zoom);
    let meta_font = egui::FontId::monospace(theme.dag_node_meta_font_size().value() * zoom);
    let gap = theme.dag_node_gap().value() * zoom;
    let row_gap = theme.dag_node_row_gap().value() * zoom;
    let icon_side = theme.icon_glyph_size_sm.value() * zoom;
    let name_h = painter
        .ctx()
        .fonts(|f| f.row_height(&name_font))
        .max(icon_side);
    let meta_h = painter.ctx().fonts(|f| f.row_height(&meta_font));

    // compact 티어는 이름 행만 남는다 — 좌하단 LOD 칩이 "상태는 줌인" 이라고 알린다.
    let total = if lod == Lod::Full {
        name_h + row_gap + meta_h
    } else {
        name_h
    };
    let top = inner.center().y - total / 2.0;

    // 이름 행 — 종류 아이콘 + 이름.
    kind_icon(node.command_kind)
        .image(icon_side, tone(theme.dag_node_meta_fg(), dim))
        .paint_at(
            ui,
            egui::Rect::from_min_size(
                egui::pos2(inner.min.x, top + (name_h - icon_side) / 2.0),
                egui::vec2(icon_side, icon_side),
            ),
        );
    let name_x = inner.min.x + icon_side + gap;
    painter.text(
        egui::pos2(name_x, top + name_h / 2.0),
        egui::Align2::LEFT_CENTER,
        ellipsize(painter, &node.name, &name_font, inner.max.x - name_x),
        name_font,
        tone(theme.dag_node_fg(), dim),
    );

    if lod != Lod::Full {
        return;
    }

    // 메타 행: 글리프(채널 2) + 상태 라벨(채널 3) + duration.
    let meta_y = top + name_h + row_gap + meta_h / 2.0;
    let glyph = node.status.glyph();
    let gw = text_width(painter, glyph, &meta_font) + gap;
    painter.text(
        egui::pos2(inner.min.x, meta_y),
        egui::Align2::LEFT_CENTER,
        glyph,
        meta_font.clone(),
        tone(label_fg, dim),
    );

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
                egui::Align2::RIGHT_CENTER,
                d,
                meta_font.clone(),
                tone(theme.dag_node_meta_fg(), dim),
            );
            right -= dw + gap;
        }
    }
    let label_w = (right - inner.min.x - gw).max(0.0);
    if label_w > 0.0 {
        painter.text(
            egui::pos2(inner.min.x + gw, meta_y),
            egui::Align2::LEFT_CENTER,
            ellipsize(painter, node.status.label(), &meta_font, label_w),
            meta_font,
            tone(label_fg, dim),
        );
    }
}

/// 선택 링. 카드 **바깥**에 그려 카드 내용 폭을 잡아먹지 않는다.
///
/// 시안 `outline 2px` + `outlineOffset 1px` — 링과 카드 변 사이를 1px 띄운다. 붙여
/// 그리면 상태 테두리와 링이 한 겹으로 뭉쳐 선택 여부가 읽히지 않는다.
pub fn paint_selection_ring(painter: &egui::Painter, theme: &Theme, rect: egui::Rect, zoom: f32) {
    let w = (theme.dag_node_selected_ring_width().value() * zoom).max(1.0);
    let offset = theme.border_width.value() * zoom;
    painter.rect_stroke(
        rect.expand(offset + w / 2.0),
        (theme.dag_node_radius().value() * zoom + offset + w).round(),
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
