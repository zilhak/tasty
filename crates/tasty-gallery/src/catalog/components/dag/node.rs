//! 노드 카드 — 디자인 `DagNode` 의 구조 전사 + `node` 섹션 3 개 Spec.
//!
//! 카드 한 장은 flex row 두 칸이다: 좌측 3px 상태 바 + 본문 컬럼.
//! 본문 컬럼은 이름 행(종류 글리프 + 이름)과 메타 행(상태 글리프 + 철자 라벨 +
//! duration)을 `row gap 4`, `padding 4/8` 로 세로 중앙 정렬한다. LOD 는 **박스**
//! 가 아니라 **내용물**만 바꾼다.

use tasty_design_tokens::generated::component::dag::NODE_DIM_OPACITY;
use tasty_design_tokens::generated::semantic::ICON_SIZE_SM;
use tasty_type_appearance::color::HexColor;
use tasty_type_appearance::theme::Theme;

use super::{Kind, Node, STATUS_ORDER, Status};
use crate::catalog::spec::{self, StageVariant, TokenChip};

/// 줌 배율이 정하는 카드 내용 티어 (`lodOf`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lod {
    /// ≥ 0.7 — 이름 + 상태 라벨 + duration.
    Full,
    /// 0.4 – 0.7 — 종류 글리프 + 이름.
    Compact,
    /// < 0.4 — 상태색 블록만.
    Block,
}

impl Lod {
    pub fn of(zoom: f32) -> Self {
        if zoom >= 0.7 {
            Lod::Full
        } else if zoom >= 0.4 {
            Lod::Compact
        } else {
            Lod::Block
        }
    }
}

/// 카드 한 장의 그리기 상태.
#[derive(Debug, Clone, Copy)]
pub struct NodeVis {
    pub lod: Lod,
    /// 캔버스 배율. 박스와 폰트·패딩이 함께 스케일된다(CSS `transform: scale`).
    pub zoom: f32,
    pub selected: bool,
    pub hovered: bool,
    pub dimmed: bool,
}

impl Default for NodeVis {
    fn default() -> Self {
        Self {
            lod: Lod::Full,
            zoom: 1.0,
            selected: false,
            hovered: false,
            dimmed: false,
        }
    }
}

fn tone(color: HexColor, dimmed: bool) -> egui::Color32 {
    let c = color.to_egui();
    if dimmed {
        c.gamma_multiply(NODE_DIM_OPACITY)
    } else {
        c
    }
}

/// 카드 한 장. `rect` 는 이미 화면 좌표로 변환된 박스다.
pub fn paint_card(ui: &mut egui::Ui, theme: &Theme, rect: egui::Rect, node: &Node, vis: NodeVis) {
    let (zoom, dim) = (vis.zoom, vis.dimmed);
    let radius = (theme.dag_node_radius().value() * zoom).round();
    let bw = theme.border_width.value();
    let accent = node.status.accent(theme);

    if vis.lod == Lod::Block {
        // block 티어는 텍스트가 없으니 상태색을 카드 전체로 채운다:
        // `color-mix(in srgb, <accent> 55%, var(--tasty-surface-raised))`.
        let fill = theme
            .surface_raised()
            .to_egui()
            .lerp_to_gamma(accent.to_egui(), 0.55);
        ui.painter().rect_filled(rect, radius, fill);
        ui.painter().rect_stroke(
            rect,
            radius,
            egui::Stroke::new(bw, tone(accent, dim)),
            egui::StrokeKind::Inside,
        );
        if vis.selected {
            paint_selection_ring(ui, theme, rect, zoom);
        }
        return;
    }

    ui.painter()
        .rect_filled(rect, radius, tone(node.status.bg(theme), dim));
    ui.painter().rect_stroke(
        rect,
        radius,
        egui::Stroke::new(bw, tone(node.status.border(theme), dim)),
        egui::StrokeKind::Inside,
    );

    // 좌측 3px 상태 바 — 위치 기반 세 번째 채널.
    let bar_w = (theme.dag_node_bar_width().value() * zoom).max(1.0);
    let bar = egui::Rect::from_min_max(rect.min, egui::pos2(rect.min.x + bar_w, rect.max.y));
    ui.painter().rect_filled(
        bar,
        (theme.corner_radius_sm.value() * zoom).round(),
        tone(accent, dim),
    );

    // 본문 컬럼 — 바 오른쪽, padding 4/8.
    let pad_x = theme.dag_node_padding_x().value() * zoom;
    let pad_y = theme.dag_node_padding_y().value() * zoom;
    let body = egui::Rect::from_min_max(
        egui::pos2(rect.min.x + bar_w, rect.min.y),
        egui::pos2(rect.max.x, rect.max.y),
    );
    if vis.hovered {
        ui.painter()
            .rect_filled(body, 0.0, tone(theme.dag_node_hover_bg(), dim));
    }
    let inner = egui::Rect::from_min_max(
        egui::pos2(body.min.x + pad_x, body.min.y + pad_y),
        egui::pos2(body.max.x - pad_x, body.max.y - pad_y),
    );
    if inner.width() <= 0.0 || inner.height() <= 0.0 {
        return;
    }

    let name_font = egui::FontId::proportional(theme.dag_node_name_font_size().value() * zoom);
    let meta_font = egui::FontId::monospace(theme.dag_node_meta_font_size().value() * zoom);
    let row_gap = theme.dag_node_row_gap().value() * zoom;
    let gap = theme.dag_node_gap().value() * zoom;
    let icon_side = ICON_SIZE_SM.value() * zoom;
    let name_h = row_height(ui, &name_font).max(icon_side);
    let meta_h = row_height(ui, &meta_font);

    let total = if vis.lod == Lod::Full {
        name_h + row_gap + meta_h
    } else {
        name_h
    };
    let top = inner.center().y - total / 2.0;

    // 이름 행 — 종류 글리프 + 이름.
    let icon_rect = egui::Rect::from_min_size(
        egui::pos2(inner.min.x, top + (name_h - icon_side) / 2.0),
        egui::vec2(icon_side, icon_side),
    );
    node.kind
        .icon()
        .image(icon_side, tone(theme.dag_node_meta_fg(), dim))
        .paint_at(ui, icon_rect);
    let name_x = inner.min.x + icon_side + gap;
    ui.painter().text(
        egui::pos2(name_x, top + name_h / 2.0),
        egui::Align2::LEFT_CENTER,
        ellipsize(ui, &node.name, &name_font, inner.max.x - name_x),
        name_font,
        tone(theme.dag_node_fg(), dim),
    );

    if vis.lod != Lod::Full {
        if vis.selected {
            paint_selection_ring(ui, theme, rect, zoom);
        }
        return;
    }

    // 메타 행 — 상태 글리프 + 대문자 라벨 + duration(우측 정렬).
    let meta_y = top + name_h + row_gap + meta_h / 2.0;
    let label_fg = node.status.label_fg(theme);
    let glyph = node.status.glyph();
    let gw = text_width(ui, glyph, &meta_font) + gap;
    ui.painter().text(
        egui::pos2(inner.min.x, meta_y),
        egui::Align2::LEFT_CENTER,
        glyph,
        meta_font.clone(),
        tone(label_fg, dim),
    );

    // duration 이 먼저 자리를 잡고 상태 라벨이 먼저 줄어든다 — 라벨은 바·글리프와
    // 중복되는 채널이지만 duration 은 다른 어디에도 없는 정보다.
    let mut right = inner.max.x;
    if let Some(d) = &node.dur {
        let dw = text_width(ui, d, &meta_font);
        if dw <= inner.width() - gw {
            ui.painter().text(
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
        ui.painter().text(
            egui::pos2(inner.min.x + gw, meta_y),
            egui::Align2::LEFT_CENTER,
            ellipsize(ui, &node.status.label().to_uppercase(), &meta_font, label_w),
            meta_font,
            tone(label_fg, dim),
        );
    }

    if vis.selected {
        paint_selection_ring(ui, theme, rect, zoom);
    }
}

/// 선택 링 — `outline 2px` + `outlineOffset 1px`, 카드 **바깥**에 그린다.
pub fn paint_selection_ring(ui: &egui::Ui, theme: &Theme, rect: egui::Rect, zoom: f32) {
    let w = (theme.dag_node_selected_ring_width().value() * zoom).max(1.0);
    let offset = theme.border_width.value() * zoom;
    ui.painter().rect_stroke(
        rect.expand(offset + w / 2.0),
        (theme.dag_node_radius().value() * zoom + offset + w).round(),
        egui::Stroke::new(w, theme.dag_node_selected_ring().to_egui()),
        egui::StrokeKind::Middle,
    );
}

pub(super) fn row_height(ui: &egui::Ui, font: &egui::FontId) -> f32 {
    ui.fonts(|f| f.row_height(font))
}

pub(super) fn text_width(ui: &egui::Ui, text: &str, font: &egui::FontId) -> f32 {
    ui.fonts(|f| {
        f.layout_no_wrap(text.to_owned(), font.clone(), egui::Color32::WHITE)
            .size()
            .x
    })
}

/// 폭에 맞춰 말줄임. char 경계를 지킨다 — 바이트로 자르면 한글 이름에서 패닉한다.
pub(super) fn ellipsize(ui: &egui::Ui, text: &str, font: &egui::FontId, max_w: f32) -> String {
    if max_w <= 0.0 {
        return String::new();
    }
    if text_width(ui, text, font) <= max_w {
        return text.to_owned();
    }
    let chars: Vec<char> = text.chars().collect();
    let mut keep = 0usize;
    for n in 1..=chars.len() {
        let cand: String = chars[..n].iter().collect::<String>() + "…";
        if text_width(ui, &cand, font) > max_w {
            break;
        }
        keep = n;
    }
    if keep == 0 {
        return "…".to_owned();
    }
    chars[..keep].iter().collect::<String>() + "…"
}

/// 디자인 `NodeBox` — 카드 168×48 + 하단 mono 10 캡션.
///
/// 중첩 `ui.vertical` 이 아니라 **한 번의 `allocate_exact_size`** 로 자리를 잡는다.
/// egui 의 `horizontal_wrapped` 는 위젯 하나의 크기를 미리 알아야 줄바꿈을
/// 판단하는데, 중첩 Ui 는 "남은 폭 전부" 를 요구해 줄이 절대 넘어가지 않는다.
fn node_box(ui: &mut egui::Ui, theme: &Theme, node: &Node, vis: NodeVis, caption: &str) {
    let w = theme.dag_node_width().value();
    let h = theme.dag_node_height().value();
    let gap = theme.spacing_sm.value();
    let cap_font = egui::FontId::monospace(theme.font_size_micro.value());
    let cap_h = row_height(ui, &cap_font);
    // 선택 링은 카드 바깥에 그려지므로 그만큼 자리를 미리 비워 둔다.
    let pad = if vis.selected {
        theme.dag_node_selected_ring_width().value() + theme.border_width.value()
    } else {
        0.0
    };
    let (outer, _) = ui.allocate_exact_size(
        egui::vec2(w + pad * 2.0, h + pad * 2.0 + gap + cap_h),
        egui::Sense::hover(),
    );
    let rect = egui::Rect::from_min_size(outer.min + egui::vec2(pad, pad), egui::vec2(w, h));
    paint_card(ui, theme, rect, node, vis);
    ui.painter().text(
        egui::pos2(outer.min.x, rect.max.y + pad + gap),
        egui::Align2::LEFT_TOP,
        caption,
        cap_font,
        theme.text_muted().to_egui(),
    );
}

fn sample(status: Status, name: &str, dur: Option<&str>) -> Node {
    let mut n = super::Node {
        id: "spec".into(),
        name: name.into(),
        kind: Kind::Run,
        status,
        dur: dur.map(str::to_owned),
        started: Some("10:31:29".into()),
        exit: None,
        cmd: "cargo build".into(),
        err: None,
        deps: Vec::new(),
    };
    n.kind = Kind::Run;
    n
}

/// `node` 섹션 Spec 1 — 8 상태 전부.
pub fn draw_states(ui: &mut egui::Ui, theme: &Theme) {
    spec::stage(ui, theme, StageVariant::Wrap, |ui| {
        for s in STATUS_ORDER {
            let dur = if matches!(s, Status::Waiting | Status::Ready) {
                None
            } else {
                Some("12s")
            };
            let node = sample(s, &format!("task:{}", s.key()), dur);
            node_box(
                ui,
                theme,
                &node,
                NodeVis {
                    dimmed: s.is_dim(),
                    ..NodeVis::default()
                },
                s.key(),
            );
        }
    });
    spec::meta(
        ui,
        theme,
        &[
            ("card", "168 × 48 · radius 4"),
            ("padding", "4 / 8 · row gap 4"),
            ("status bar", "3px, full height"),
            ("name", "13 medium, 1 line, ellipsis"),
            ("meta row", "10 caps · duration mono, right"),
            ("dimmed", "skipped + cancelled at 0.4"),
        ],
        &STATUS_ORDER
            .iter()
            .map(|s| {
                TokenChip::new(
                    match s {
                        Status::Waiting => "--tasty-dag-status-waiting",
                        Status::Ready => "--tasty-dag-status-ready",
                        Status::Running => "--tasty-dag-status-running",
                        Status::Succeeded => "--tasty-dag-status-succeeded",
                        Status::Failed => "--tasty-dag-status-failed",
                        Status::Cancelled => "--tasty-dag-status-cancelled",
                        Status::Skipped => "--tasty-dag-status-skipped",
                        Status::Unknown => "--tasty-dag-status-unknown",
                    },
                    s.key(),
                    s.accent(theme).to_egui(),
                )
            })
            .collect::<Vec<_>>(),
    );
    spec::note(
        ui,
        theme,
        "Colour vs. text: the state colour is the recede channel — bar, border, glyph. \
         The spelled label reads from its own dag-status-<state>-label role: waiting takes \
         text-muted, and the two dimmed states take text-secondary, which still measures \
         4.74:1 after the 0.75 card dim.",
    );
    spec::note(
        ui,
        theme,
        "Truncation rule (i18n): the task name is a single line with ellipsis at the card edge — \
         never wraps, never shrinks the font. On the meta row the status label yields first: it \
         ellipses before the duration is dropped, so a long state string can't push the timing out.",
    );
}

/// `node` 섹션 Spec 2 — task 종류 4 종.
pub fn draw_kinds(ui: &mut egui::Ui, theme: &Theme) {
    spec::stage(ui, theme, StageVariant::Wrap, |ui| {
        for k in [Kind::Run, Kind::Custom, Kind::Reduce, Kind::WaitBarrier] {
            let mut node = sample(
                Status::Succeeded,
                &format!("{}:step", k.label()),
                Some("4s"),
            );
            node.kind = k;
            node_box(ui, theme, &node, NodeVis::default(), k.key());
        }
    });
    spec::meta(
        ui,
        theme,
        &[
            ("glyph", "14px, muted, leading"),
            ("run", "terminal"),
            ("custom", "plug"),
            ("reduce", "layers"),
            ("wait_barrier", "lock"),
        ],
        &[TokenChip::new(
            "--tasty-dag-node-meta-fg",
            "kind glyph",
            theme.dag_node_meta_fg().to_egui(),
        )],
    );
    spec::note(
        ui,
        theme,
        "No new glyphs requested — terminal, plug, layers and lock already carry these four kinds. \
         A dedicated barrier glyph would read better than lock if one is ever drawn.",
    );
}

/// `node` 섹션 Spec 3 — LOD 3 티어 · 선택 · 오버플로.
pub fn draw_lod(ui: &mut egui::Ui, theme: &Theme) {
    spec::stage(ui, theme, StageVariant::Wrap, |ui| {
        for (lod, caption) in [
            (Lod::Full, "full ≥ 70%"),
            (Lod::Compact, "compact ≥ 40%"),
            (Lod::Block, "block < 40%"),
        ] {
            let node = sample(Status::Running, "build:linux-x86_64", Some("12s"));
            node_box(
                ui,
                theme,
                &node,
                NodeVis {
                    lod,
                    ..NodeVis::default()
                },
                caption,
            );
        }
        let long = sample(
            Status::Failed,
            "build:linux-x86_64-with-a-very-long-target-triple",
            Some("12s"),
        );
        node_box(ui, theme, &long, NodeVis::default(), "overflow → ellipsis");
        let sel = sample(Status::Succeeded, "build:linux-x86_64", Some("12s"));
        node_box(
            ui,
            theme,
            &sel,
            NodeVis {
                selected: true,
                ..NodeVis::default()
            },
            "selected (2px ring)",
        );
        let hot = sample(Status::Running, "build:linux-x86_64", Some("12s"));
        node_box(
            ui,
            theme,
            &hot,
            NodeVis {
                hovered: true,
                ..NodeVis::default()
            },
            "hover wash",
        );
    });
    spec::meta(
        ui,
        theme,
        &[
            ("full", "zoom ≥ 0.7"),
            ("compact", "0.4 – 0.7"),
            ("block", "< 0.4"),
            ("selection", "2px accent outline, 1px offset"),
            ("hover", "derived overlay-hover on the body"),
        ],
        &[
            TokenChip::new(
                "--tasty-dag-node-selected-ring",
                "selection",
                theme.dag_node_selected_ring().to_egui(),
            ),
            TokenChip::new(
                "--tasty-dag-node-hover-bg",
                "hover wash",
                theme.dag_node_hover_bg().to_egui(),
            ),
            TokenChip::new(
                "--tasty-dag-node-dim-opacity",
                "skipped path",
                theme.dag_status_skipped().to_egui(),
            ),
        ],
    );
    super::canvas::dense_stage(ui, theme);
    spec::note(
        ui,
        theme,
        "55 nodes, auto-fitted on open — the canvas lands in the compact or block tier and says so \
         in the bottom-left hint chip, so a user who sees colour blocks knows why and knows the way back.",
    );
}
