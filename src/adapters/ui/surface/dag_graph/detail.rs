//! 선택 노드 상세 — **콘텐츠만**.
//!
//! 도킹(우측 288px 패널이냐 하단 220px 시트냐)은 이 모듈이 정하지 않는다. 호출자가
//! 자리를 잡아 `ui` 를 넘기고 여기서는 그 안을 채운다. 팝오버로 같은 내용을 띄우는
//! 화면이 생겨도 **콘텐츠 구현은 이 하나뿐**이어야 두 자리의 내용이 갈리지 않는다.

use tasty_type_appearance::theme::Theme;
use tasty_ui_widgets::{margin_all, vspace};

use super::model::{DagGraphData, DagNodeData, format_clock, kind_label, node_duration};
use super::node::status_colors;
use crate::i18n::{t, t_fmt};

/// 상세 패널에서 나온 사용자 조작.
pub enum DetailAction {
    /// 의존성 행을 눌러 그 노드로 선택을 옮긴다.
    Select(String),
}

/// 상세가 어느 자리에 놓였는가. **콘텐츠가 아니라 자리**의 속성이라
/// [`draw_detail`] 이 아니라 자리를 정한 쪽이 들고 있다.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum DetailDock {
    /// 캔버스 오른쪽 고정폭 패널 — 캔버스와의 경계는 **왼쪽 세로선**.
    Side,
    /// 캔버스 아래 시트 — 경계는 **위쪽 가로선**. 팝오버로 띄울 때도 같다.
    Sheet,
}

/// 도킹 자리와 캔버스 사이의 경계선. 도킹을 정한 쪽이 그 자리 rect 로 호출한다.
pub fn dock_divider(painter: &egui::Painter, theme: &Theme, dock: egui::Rect, side: DetailDock) {
    let ends = match side {
        DetailDock::Side => [dock.left_top(), dock.left_bottom()],
        DetailDock::Sheet => [dock.left_top(), dock.right_top()],
    };
    painter.line_segment(
        ends,
        egui::Stroke::new(
            theme.border_width.value(),
            theme.dag_detail_border().to_egui(),
        ),
    );
}

/// 상세 콘텐츠를 `ui` 안에 채운다.
pub fn draw_detail(
    ui: &mut egui::Ui,
    theme: &Theme,
    graph: &DagGraphData,
    node: &DagNodeData,
    now_ms: u64,
) -> Option<DetailAction> {
    let mut action = None;

    // 도킹 자리를 통째로 채운다 — 캔버스와의 경계가 색으로 끊겨야 별개 영역으로 읽힌다.
    // **구분선은 여기서 그리지 않는다**: 어느 변에 그어야 하는지는 이 콘텐츠가 아니라
    // 자리를 정한 쪽만 안다(우측 패널이면 왼쪽 세로선, 하단 시트면 위쪽 가로선).
    // 호출자가 [`dock_divider`] 로 긋는다.
    let dock = ui.available_rect_before_wrap();
    ui.painter()
        .rect_filled(dock, 0.0, theme.dag_detail_bg().to_egui());

    egui::Frame::NONE
        .inner_margin(margin_all(theme.dag_detail_padding()))
        .show(ui, |ui| {
            ui.set_min_width(dock.width() - theme.dag_detail_padding().value() * 2.0);
            // 캔버스는 item_spacing 을 0 으로 눌러 두었다(픽셀 단위 페인팅) — 텍스트
            // 문서인 상세는 그 설정을 물려받으면 라벨과 값이 붙어버린다.
            ui.spacing_mut().item_spacing =
                egui::vec2(theme.spacing_xs.value(), theme.spacing_xs.value());
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    header(ui, theme, node);
                    vspace(ui, theme.spacing_sm);

                    row(
                        ui,
                        theme,
                        t("dag.detail.kind"),
                        kind_label(node.command_kind),
                    );
                    row(
                        ui,
                        theme,
                        t("dag.detail.on_failure"),
                        &on_failure_label(node.on_failure_kind),
                    );
                    if let Some(started) = node.started_at {
                        row(ui, theme, t("dag.detail.started"), &format_clock(started));
                    }
                    if let Some(d) = node_duration(node, now_ms) {
                        row(ui, theme, t("dag.detail.duration"), &d);
                    }
                    if let Some(code) = node.exit_code {
                        row(ui, theme, t("dag.detail.exit_code"), &code.to_string());
                    }

                    vspace(ui, theme.spacing_sm);
                    labeled_block(
                        ui,
                        theme,
                        t("dag.detail.command"),
                        &node.command_text,
                        theme.dag_detail_out_max_height().value(),
                    );

                    if !node.incoming.is_empty() {
                        vspace(ui, theme.spacing_sm);
                        section(ui, theme, t("dag.detail.dependencies"));
                        for (idx, rel) in &node.incoming {
                            let Some(dep) = graph.nodes.get(*idx) else {
                                continue;
                            };
                            if dependency_row(ui, theme, dep, rel.label()) {
                                action = Some(DetailAction::Select(dep.id.clone()));
                            }
                        }
                    }

                    if let Some(err) = &node.error_tail {
                        vspace(ui, theme.spacing_sm);
                        labeled_block(
                            ui,
                            theme,
                            t("dag.detail.error"),
                            err,
                            theme.dag_detail_log_max_height().value(),
                        );
                    }
                    if let Some(out) = &node.output_tail {
                        vspace(ui, theme.spacing_sm);
                        labeled_block(
                            ui,
                            theme,
                            t("dag.detail.output"),
                            out,
                            theme.dag_detail_out_max_height().value(),
                        );
                    }
                });
        });

    action
}

fn header(ui: &mut egui::Ui, theme: &Theme, node: &DagNodeData) {
    let (bar, _, label_fg) = status_colors(theme, node.status);
    ui.label(
        egui::RichText::new(&node.name)
            .size(theme.font_size_body.value())
            .color(theme.text_primary().to_egui()),
    );
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new(node.status.glyph())
                .monospace()
                .size(theme.font_size_caption.value())
                .color(bar.to_egui()),
        );
        ui.label(
            egui::RichText::new(node.status.label())
                .size(theme.font_size_caption.value())
                .color(label_fg.to_egui()),
        );
    });
    // task id 는 CLI(`tasty agent task-get <id>`)로 이어지는 유일한 손잡이라
    // 선택 가능한 텍스트로 둔다.
    ui.add(
        egui::Label::new(
            egui::RichText::new(&node.id)
                .monospace()
                .size(theme.font_size_micro.value())
                .color(theme.text_muted().to_egui()),
        )
        .selectable(true),
    );
}

fn section(ui: &mut egui::Ui, theme: &Theme, title: &str) {
    ui.label(
        egui::RichText::new(title)
            .size(theme.font_size_micro.value())
            .color(theme.text_muted().to_egui()),
    );
}

fn row(ui: &mut egui::Ui, theme: &Theme, label: &str, value: &str) {
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new(label)
                .size(theme.font_size_micro.value())
                .color(theme.text_muted().to_egui()),
        );
        ui.label(
            egui::RichText::new(value)
                .size(theme.font_size_caption.value())
                .color(theme.text_primary().to_egui()),
        );
    });
}

/// 라벨 + 복사 가능한 mono 블록. `max_h` 를 넘으면 안에서 스크롤한다.
fn labeled_block(ui: &mut egui::Ui, theme: &Theme, label: &str, body: &str, max_h: f32) {
    section(ui, theme, label);
    egui::Frame::NONE
        .fill(theme.dag_detail_log_bg().to_egui())
        .inner_margin(margin_all(theme.spacing_xs))
        .show(ui, |ui| {
            egui::ScrollArea::vertical()
                .max_height(max_h)
                .auto_shrink([false, true])
                .id_salt(label)
                .show(ui, |ui| {
                    ui.add(
                        egui::Label::new(
                            egui::RichText::new(body)
                                .monospace()
                                .size(theme.font_size_micro.value())
                                .color(theme.text_primary().to_egui()),
                        )
                        .selectable(true)
                        .wrap(),
                    );
                });
        });
}

/// 의존성 한 줄. 눌리면 `true`.
fn dependency_row(ui: &mut egui::Ui, theme: &Theme, dep: &DagNodeData, rel: &str) -> bool {
    let (bar, _, _) = status_colors(theme, dep.status);
    let resp = ui
        .horizontal(|ui| {
            let label = format!("{}  {}", dep.status.glyph(), dep.name);
            let r = ui.add(
                egui::Label::new(
                    egui::RichText::new(label)
                        .size(theme.font_size_caption.value())
                        .color(bar.to_egui()),
                )
                .sense(egui::Sense::click()),
            );
            ui.label(
                egui::RichText::new(rel)
                    .size(theme.font_size_micro.value())
                    .color(theme.text_muted().to_egui()),
            );
            r
        })
        .inner;
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    resp.on_hover_text(t_fmt("dag.detail.jump_hint", &dep.name))
        .clicked()
}

fn on_failure_label(kind: &str) -> String {
    match kind {
        "continue_downstream" => t("dag.on_failure.continue_downstream").to_string(),
        "fallback" => t("dag.on_failure.fallback").to_string(),
        _ => t("dag.on_failure.abort").to_string(),
    }
}
