//! 실행 중 세부 단계와 노드 호버의 이유 줄 예제.
//! 입력 대기만 상태 모양 전체를 needs-input 색으로 바꾸고, 후처리 두 단계는 실행 중 색에 라벨만 바꾼다.

use tasty_type_appearance::color::HexColor;
use tasty_type_appearance::theme::Theme;

use super::detail::{Dock, draw_docked};
use super::node::{Lod, NodeVis, node_box, sample};
use super::{Graph, Kind, Node, Runner, Skip, Status};
use crate::catalog::spec::{self, StageVariant, TokenChip};

/// 실행 중인 v2 task 의 세부 단계(`DAG_PHASE`). `executing` 은 `None` 으로 둔다.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Phase {
    /// agent 세션이 사람의 입력을 기다린다. `since` 는 기다린 시간이다.
    AwaitingInput { provider: String, since: String },
    /// 후처리 `run` 번째 실행.
    Postprocessing { run: u32 },
    /// 후처리 `run` 번째 실행을 기다린다(재시도).
    RetryWait { run: u32 },
}

const RETRY_OR_CANCEL: &str = "Retry or cancel it to let the graph continue.";
const LOST_REASON: &str = "run result lost: pid 4242 ended after a host restart and its exit status could not be collected";

impl Node {
    fn awaiting(&self) -> Option<(&str, &str)> {
        match (&self.status, &self.phase) {
            (Status::Running, Some(Phase::AwaitingInput { provider, since })) => {
                Some((provider, since))
            }
            _ => None,
        }
    }

    /// 시안 `nodeLook` 의 라벨. 경로가 선택되지 않은 skipped 는 그 사실을 적는다.
    pub fn status_label(&self) -> String {
        match (&self.status, &self.phase, &self.skip) {
            (Status::Running, Some(Phase::AwaitingInput { .. }), _) => "Needs input".into(),
            (Status::Running, Some(Phase::Postprocessing { run }), _) => {
                format!("Postprocess \u{b7} run {run}")
            }
            (Status::Running, Some(Phase::RetryWait { run }), _) => {
                format!("Retry wait \u{b7} run {run}")
            }
            (Status::Skipped, _, Some(Skip::BranchNotSelected)) => "Not selected".into(),
            (status, _, _) => status.label().into(),
        }
    }

    pub fn glyph(&self) -> &'static str {
        if self.awaiting().is_some() {
            "!"
        } else {
            self.status.glyph()
        }
    }

    pub fn accent(&self, theme: &Theme) -> HexColor {
        if self.awaiting().is_some() {
            theme.dag_phase_awaiting()
        } else {
            self.status.accent(theme)
        }
    }

    pub fn bg(&self, theme: &Theme) -> HexColor {
        if self.awaiting().is_some() {
            theme.dag_phase_awaiting_bg()
        } else {
            self.status.bg(theme)
        }
    }

    pub fn label_fg(&self, theme: &Theme) -> HexColor {
        if self.awaiting().is_some() {
            theme.dag_phase_awaiting_label()
        } else {
            self.status.label_fg(theme)
        }
    }

    pub fn border(&self, theme: &Theme) -> HexColor {
        if self.awaiting().is_some() {
            theme.dag_phase_awaiting()
        } else {
            self.status.border(theme)
        }
    }

    /// 시안 `nodeTitle` — `이름 — 라벨` 다음에 건너뜀·알 수 없음·입력 대기의 이유 줄.
    pub fn hover_text(&self) -> String {
        let mut lines = vec![format!("{} \u{2014} {}", self.name, self.status_label())];
        match (&self.status, &self.skip, &self.reason) {
            (Status::Skipped, Some(Skip::BranchNotSelected), _) => {
                lines.push("Why: Not selected by the upstream result".into());
            }
            (Status::Skipped, Some(Skip::UpstreamUnavailable { .. }), _) => {
                lines.push("Why: An upstream task did not succeed".into());
            }
            (Status::Unknown, _, Some(reason)) => {
                lines.push(format!("Why: {reason}"));
                lines.push(RETRY_OR_CANCEL.into());
            }
            _ => {}
        }
        if let Some((provider, since)) = self.awaiting() {
            lines.push(format!(
                "Waiting for a person in the {provider} session \u{b7} {since}"
            ));
        }
        lines.join("\n")
    }

    /// 상세 패널의 입력 대기 알림 문구.
    pub fn awaiting_notice(&self) -> Option<String> {
        let (provider, since) = self.awaiting()?;
        Some(format!(
            "The {provider} session is waiting for a person \u{b7} {since}. \
             The graph continues after you answer it."
        ))
    }
}

/// 상세 패널의 알 수 없음 문구 — 이유와 그래프를 잇는 방법.
pub fn unknown_lines(node: &Node) -> Option<(&str, &'static str)> {
    match (&node.status, &node.reason) {
        (Status::Unknown, Some(reason)) => Some((reason, RETRY_OR_CANCEL)),
        _ => None,
    }
}

fn awaiting_node() -> Node {
    let mut n = sample(Status::Running, "agent:review", Some("2m"));
    n.id = "a".into();
    n.kind = Kind::Agent;
    n.cmd = "agent: claude · review the diff".into();
    n.phase = Some(Phase::AwaitingInput {
        provider: "claude".into(),
        since: "2m".into(),
    });
    n
}

fn unknown_node() -> Node {
    let mut n = sample(Status::Unknown, "deploy:site", None);
    n.id = "u".into();
    n.reason = Some(LOST_REASON.into());
    n
}

fn running(phase: Option<Phase>, name: &str, dur: &str) -> Node {
    let mut n = sample(Status::Running, name, Some(dur));
    n.phase = phase;
    n
}

fn single(node: Node) -> Graph {
    Graph {
        id: node.id.clone(),
        name: node.name.clone(),
        workspace: "tasty".into(),
        updated: "now".into(),
        cycle: None,
        nodes: vec![node],
        runner: Runner {
            running: true,
            crashed: false,
            ready: 0,
            active: 1,
        },
    }
}

/// `phase` 섹션 Spec 1 — 실행 중 세부 단계.
pub fn draw_phases(ui: &mut egui::Ui, theme: &Theme) {
    spec::stage(ui, theme, StageVariant::Wrap, |ui| {
        let plain = running(None, "test:unit", "12s");
        node_box(ui, theme, &plain, NodeVis::default(), "executing (default)");
        let waiting = awaiting_node();
        node_box(ui, theme, &waiting, NodeVis::default(), "awaiting_input");
        let compact = NodeVis {
            lod: Lod::Compact,
            ..NodeVis::default()
        };
        node_box(ui, theme, &waiting, compact, "awaiting_input · compact");
        let post = running(Some(Phase::Postprocessing { run: 1 }), "build:docs", "41s");
        node_box(ui, theme, &post, NodeVis::default(), "postprocessing");
        let retry = running(Some(Phase::RetryWait { run: 2 }), "build:docs", "1m 3s");
        node_box(ui, theme, &retry, NodeVis::default(), "retry_wait");
    });
    spec::meta(
        ui,
        theme,
        &[
            ("executing", "no change — RUNNING"),
            (
                "awaiting_input",
                "! · NEEDS INPUT · yellow · duration = time waiting",
            ),
            ("postprocessing", "◑ · POSTPROCESS · RUN n"),
            ("retry_wait", "◑ · RETRY WAIT · RUN n"),
            ("v1 tasks", "never have a phase → plain running"),
        ],
        &[
            TokenChip::new(
                "dag-phase-awaiting",
                "bar · border",
                theme.dag_phase_awaiting().to_egui(),
            ),
            TokenChip::new(
                "dag-phase-awaiting-bg",
                "card wash",
                theme.dag_phase_awaiting_bg().to_egui(),
            ),
            TokenChip::new(
                "dag-phase-awaiting-label",
                "glyph + label",
                theme.dag_phase_awaiting_label().to_egui(),
            ),
        ],
    );
    spec::note(
        ui,
        theme,
        "Labels (i18n). needs input · 입력 대기 · 入力待ち — postprocess · 후처리 · 後処理 — retry wait · \
         재시도 대기 · 再試行待ち — run {n} · {n}회차 · {n}回目. The meta row is caps for Latin scripts only.",
    );
    spec::note(
        ui,
        theme,
        "Click. Selecting an awaiting node opens the detail panel with a yellow notice and an Open \
         session button (secondary sm) that focuses the agent session surface. It is a user action \
         only; agents never trigger it. The DAG list row adds a mono ! 1 needs input before the \
         rollup chip; the rollup itself stays running.",
    );
}

/// `phase` 섹션 Spec 2 — 호버 이유 줄과 상세 패널의 입력 대기·알 수 없음.
pub fn draw_why(ui: &mut egui::Ui, theme: &Theme) {
    let mut skipped = sample(Status::Skipped, "sign:artifacts", None);
    skipped.skip = Some(Skip::BranchNotSelected);
    let nodes = [unknown_node(), skipped, awaiting_node()];
    spec::stage(ui, theme, StageVariant::Wrap, |ui| {
        let width = theme.dag_detail_width().value();
        for node in &nodes {
            ui.allocate_ui_with_layout(
                egui::vec2(width, ui.available_height()),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                    ui.spacing_mut().item_spacing.y = theme.spacing_sm.value();
                    let caption = match node.status {
                        Status::Running => "awaiting_input",
                        other => other.key(),
                    };
                    node_box(
                        ui,
                        theme,
                        node,
                        NodeVis {
                            dimmed: node.status.is_dim(),
                            ..NodeVis::default()
                        },
                        caption,
                    );
                    // 툴팁 내용을 펼쳐 보인다 — 캡처에서는 호버를 만들 수 없다.
                    egui::Frame::NONE
                        .fill(theme.surface_raised().to_egui())
                        .stroke(egui::Stroke::new(
                            theme.border_width.value(),
                            theme.border_strong().to_egui(),
                        ))
                        .corner_radius(theme.corner_radius_sm.value())
                        .inner_margin(theme.spacing_sm.value())
                        .show(ui, |ui| {
                            ui.set_width(width - theme.spacing_sm.value() * 2.0);
                            ui.add(
                                egui::Label::new(
                                    egui::RichText::new(node.hover_text())
                                        .size(theme.font_size_caption.value())
                                        .color(theme.text_primary().to_egui()),
                                )
                                .wrap(),
                            );
                        });
                },
            );
        }
    });
    let h = theme.dag_popup_height().value();
    spec::stage(ui, theme, StageVariant::Tight, |ui| {
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            for node in [awaiting_node(), unknown_node()] {
                let id = node.id.clone();
                draw_docked(
                    ui,
                    theme,
                    &single(node),
                    &id,
                    Dock::Side,
                    egui::vec2(theme.dag_detail_width().value(), h),
                );
            }
        });
    });
    spec::note(
        ui,
        theme,
        "Labels (i18n). Why: · 이유: · 理由: — Why unknown · 알 수 없는 이유 · 不明の理由 — “Retry or \
         cancel it to let the graph continue.” · “다시 실행하거나 취소해야 그래프가 이어집니다.” · \
         “再実行またはキャンセルするとグラフが続行します。” — “Waiting for a person in the {provider} \
         session” · “{provider} 세션이 사람의 입력을 기다리는 중” · “{provider} セッションが入力を待っています”. \
         The reason text is the host's free text, shown as is.",
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 시안 `nodeTitle` 과 같은 문구.
    #[test]
    fn hover_text_matches_the_design_why_lines() {
        assert_eq!(
            unknown_node().hover_text(),
            format!("deploy:site \u{2014} Unknown\nWhy: {LOST_REASON}\n{RETRY_OR_CANCEL}")
        );
        assert_eq!(
            awaiting_node().hover_text(),
            "agent:review \u{2014} Needs input\nWaiting for a person in the claude session \u{b7} 2m"
        );
        let retry = running(Some(Phase::RetryWait { run: 2 }), "x", "1s");
        assert_eq!(retry.status_label(), "Retry wait \u{b7} run 2");
        assert_eq!(retry.glyph(), Status::Running.glyph());
    }
}
