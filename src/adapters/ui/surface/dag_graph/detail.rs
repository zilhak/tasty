//! 선택 노드의 상세 내용. 위치와 구분선 방향은 호출부에서 정한다.

use tasty_type_appearance::theme::Theme;
use tasty_ui_widgets::{
    Button, ButtonVariant, ControlSize, IconButton, TagVariant, margin_all, tag, vspace,
};

use super::model::{
    DagGraphData, DagNodeData, DagStatus, format_clock, format_duration_ms, kind_label,
    node_duration,
};
use super::node::{node_colors, status_colors};
use crate::adapters::ui::icons;
use crate::i18n::{t, t_fmt, t_fmt2};

/// 상세 패널에서 나온 사용자 조작.
pub enum DetailAction {
    /// 의존성 행을 눌러 그 노드로 선택을 옮긴다.
    Select(String),
    /// 닫기 — 선택을 풀어 패널 자체를 접는다.
    Close,
    /// 입력을 기다리는 agent 세션의 surface 를 사용자 앞으로 가져온다. 사용자 조작 전용이다.
    OpenSession(u32),
}

/// 상세 패널의 배치 위치.
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

    // 배경은 채우되 위치에 따른 구분선은 호출부에서 그린다.
    let dock = ui.available_rect_before_wrap();
    ui.painter()
        .rect_filled(dock, 0.0, theme.dag_detail_bg().to_egui());

    egui::Frame::NONE
        .inner_margin(margin_all(theme.dag_detail_padding()))
        .show(ui, |ui| {
            ui.set_min_width(dock.width() - theme.dag_detail_padding().value() * 2.0);
            ui.spacing_mut().item_spacing =
                egui::vec2(theme.spacing_xs.value(), theme.spacing_xs.value());
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .drag_to_scroll(false)
                .show(ui, |ui| {
                    if header(ui, theme, node) {
                        action = Some(DetailAction::Close);
                    }
                    vspace(ui, theme.spacing_sm);
                    if let Some(surface) = awaiting_notice(ui, theme, node, now_ms) {
                        action = Some(DetailAction::OpenSession(surface));
                    }
                    unknown_reason(ui, theme, node);

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
                        false,
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
                            true,
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
                            true,
                        );
                    }
                });
        });

    action
}

/// 이름·상태·종류·ID와 닫기 버튼.
fn header(ui: &mut egui::Ui, theme: &Theme, node: &DagNodeData) -> bool {
    let (bar, _, label_fg) = node_colors(theme, node);
    let mut close = false;
    ui.horizontal_top(|ui| {
        let btn = theme.dag_chrome_height().value();
        ui.allocate_ui_with_layout(
            egui::vec2(
                (ui.available_width() - btn - theme.spacing_sm.value()).max(0.0),
                btn,
            ),
            egui::Layout::top_down(egui::Align::Min),
            |ui| {
                ui.label(
                    egui::RichText::new(&node.name)
                        .size(theme.font_size_body.value())
                        .color(theme.text_primary().to_egui()),
                );
            },
        );
        close = IconButton::new()
            .size(ControlSize::Sm)
            .show(ui, theme, &|ui, rect, c| {
                icons::CLOSE.image(rect.height(), c).paint_at(ui, rect)
            })
            .on_hover_text(t("dag.detail.close"))
            .clicked();
    });
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new(node.glyph())
                .monospace()
                .size(theme.font_size_caption.value())
                .color(bar.to_egui()),
        );
        ui.label(
            egui::RichText::new(node.status_label())
                .size(theme.font_size_caption.value())
                .color(label_fg.to_egui()),
        );
        tag(
            ui,
            theme,
            kind_label(node.command_kind),
            TagVariant::Default,
            false,
        );
    });
    // CLI에서 사용할 task ID는 선택·복사할 수 있게 한다.
    ui.add(
        egui::Label::new(
            egui::RichText::new(&node.id)
                .monospace()
                .size(theme.font_size_micro.value())
                .color(theme.text_muted().to_egui()),
        )
        .selectable(true),
    );
    close
}

/// 입력 대기 알림 — 누가 무엇을 기다리는지와 그 세션을 여는 버튼. 눌리면 세션 surface id.
fn awaiting_notice(
    ui: &mut egui::Ui,
    theme: &Theme,
    node: &DagNodeData,
    now_ms: u64,
) -> Option<u32> {
    let (provider, surface, since) = node.awaiting()?;
    let mut open = false;
    egui::Frame::NONE
        .fill(theme.dag_phase_awaiting_bg().to_egui())
        .stroke(egui::Stroke::new(
            theme.border_width.value(),
            theme.dag_phase_awaiting().to_egui(),
        ))
        .corner_radius(theme.corner_radius_sm.value())
        .inner_margin(margin_all(theme.spacing_sm))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = theme.spacing_sm.value();
            ui.add(
                egui::Label::new(
                    egui::RichText::new(t_fmt2(
                        "dag.detail.awaiting_notice",
                        provider,
                        &format_duration_ms(now_ms.saturating_sub(since)),
                    ))
                    .size(theme.font_size_caption.value())
                    .color(theme.text_primary().to_egui()),
                )
                .wrap(),
            );
            open = Button::new(t("dag.detail.open_session"))
                .variant(ButtonVariant::Secondary)
                .size(ControlSize::Sm)
                .show(ui, theme)
                .clicked();
        });
    vspace(ui, theme.spacing_sm);
    open.then_some(surface)
}

/// 알 수 없음의 이유와 그래프를 잇는 방법.
fn unknown_reason(ui: &mut egui::Ui, theme: &Theme, node: &DagNodeData) {
    if node.status != DagStatus::Unknown {
        return;
    }
    let Some(reason) = &node.unknown_reason else {
        return;
    };
    section(ui, theme, t("dag.detail.why_unknown"));
    ui.add(
        egui::Label::new(
            egui::RichText::new(reason)
                .size(theme.font_size_caption.value())
                .color(theme.text_secondary().to_egui()),
        )
        .wrap(),
    );
    ui.add(
        egui::Label::new(
            egui::RichText::new(t("dag.why.retry_or_cancel"))
                .size(theme.font_size_caption.value())
                .color(theme.text_muted().to_egui()),
        )
        .wrap(),
    );
    vspace(ui, theme.spacing_sm);
}

/// 상세 캡션. 번역 문구의 대소문자를 그대로 쓰고 caption 크기로 그린다.
fn section(ui: &mut egui::Ui, theme: &Theme, title: &str) {
    ui.label(
        egui::RichText::new(title)
            .size(theme.font_size_caption.value())
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

/// 높이를 넘으면 스크롤하는 텍스트 블록. copy면 복사 버튼도 표시한다.
fn labeled_block(
    ui: &mut egui::Ui,
    theme: &Theme,
    label: &str,
    body: &str,
    max_h: f32,
    copy: bool,
) {
    if copy {
        ui.horizontal(|ui| {
            section(ui, theme, label);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if IconButton::new()
                    .size(ControlSize::Sm)
                    .show(ui, theme, &|ui, rect, c| {
                        icons::COPY.image(rect.height(), c).paint_at(ui, rect)
                    })
                    .on_hover_text(t("dag.detail.copy"))
                    .clicked()
                {
                    ui.ctx().copy_text(body.to_owned());
                }
            });
        });
    } else {
        section(ui, theme, label);
    }
    egui::Frame::NONE
        .fill(theme.dag_detail_log_bg().to_egui())
        .inner_margin(margin_all(theme.spacing_xs))
        .show(ui, |ui| {
            egui::ScrollArea::vertical()
                .max_height(max_h)
                .auto_shrink([false, true])
                .id_salt(label)
                .drag_to_scroll(false)
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
