//! DAG 목록 행 — 디자인 `dagRowItems` 의 구조 전사.
//!
//! `ListCtrl` 행 하나에 얹히는 **trailing 클러스터**가 이 파트의 전부다:
//! 출처 태그(`derived`) · rollup 상태(글리프 + 철자, 8 종 동일 어휘) · mono
//! `done/total`. 진행 막대도 스택바도 없다 — 12 개짜리 그래프에서 막대 한 칸은
//! 8% 라 셋과 넷을 눈으로 못 가르고, 이 화면의 용건은 정확한 수다.

use tasty_icons as icons;
use tasty_type_appearance::theme::Theme;
use tasty_ui_widgets::{ListCtrl, ListCtrlItem, TagVariant, tag};

use super::{Graph, Status};
use crate::catalog::spec::{self, StageVariant, TokenChip};

/// 목록 한 줄이 표현하는 DAG — 그래프 + 집계.
pub struct Entry {
    pub graph: Graph,
    /// 사용자가 `metadata.dag` 로 선언한 그룹이 아니라 의존 연결성에서 도출된 것.
    pub derived: bool,
}

impl Entry {
    /// 그래프 전체를 대표하는 한 상태. 본체 `DagStateCounts::rollup` 과 같은
    /// 우선순위 — 실행 중 > 실패 > 전부 종료 > 준비됨 > 대기.
    pub fn rollup(&self) -> Status {
        let has = |s: Status| self.graph.nodes.iter().any(|n| n.status == s);
        if has(Status::Running) {
            Status::Running
        } else if has(Status::Failed) {
            Status::Failed
        } else if self.done() == self.total() {
            Status::Succeeded
        } else if has(Status::Ready) {
            Status::Ready
        } else {
            Status::Waiting
        }
    }

    /// 더 이상 움직이지 않기로 확정된 task 수 — 성공만이 아니라 실패·취소·건너뜀도
    /// 포함한다. "3/12 남았다" 가 아니라 "9/12 가 끝났다" 를 읽는 숫자다.
    pub fn done(&self) -> usize {
        self.graph
            .nodes
            .iter()
            .filter(|n| {
                matches!(
                    n.status,
                    Status::Succeeded | Status::Failed | Status::Cancelled | Status::Skipped
                )
            })
            .count()
    }

    pub fn total(&self) -> usize {
        self.graph.nodes.len()
    }
}

/// 디자인 `DAG_LIST` — 이 갤러리가 가진 그래프 4 개를 목록 항목으로 접은 것.
pub fn entries() -> Vec<Entry> {
    vec![
        Entry {
            graph: super::build_dag(),
            derived: false,
        },
        Entry {
            graph: super::index_dag(),
            derived: false,
        },
        Entry {
            graph: super::dense_dag(),
            derived: true,
        },
        Entry {
            graph: super::cycle_dag(),
            derived: true,
        },
    ]
}

/// 행 끝 클러스터 — 태그 · 상태 · 카운터.
pub fn trailing(ui: &mut egui::Ui, theme: &Theme, entry: &Entry) {
    // `ListCtrl` 의 trailing 슬롯은 오른쪽에서 왼쪽으로 채워진다 — 시안의
    // 왼→오른 순서(태그 · 상태 · 카운터)를 얻으려면 거꾸로 낸다.
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = theme.dag_row_summary_gap().value();
        ui.label(
            egui::RichText::new(format!("{}/{}", entry.done(), entry.total()))
                .monospace()
                .size(theme.dag_row_count_font_size().value())
                .color(theme.dag_row_count_fg().to_egui()),
        );
        let status = entry.rollup();
        ui.label(
            egui::RichText::new(format!("{} {}", status.glyph(), status.label()))
                .monospace()
                .size(theme.font_size_caption.value())
                // 상태 accent 가 아니라 `-label` role — 캡션 크기에서 4.5:1 을
                // 지키는 쪽은 이 톤이다(노드 카드와 같은 규칙).
                .color(status.label_fg(theme).to_egui()),
        );
        if entry.derived {
            tag(ui, theme, "derived", TagVariant::Default, false);
        }
    });
}

/// 목록 본문 — 주어진 폭 안에 행을 쌓는다. popup specimen 이 그대로 재사용한다.
/// 클릭된 행 인덱스를 돌려준다.
pub fn list(
    ui: &mut egui::Ui,
    theme: &Theme,
    entries: &[Entry],
    width: f32,
    salt: &str,
) -> Option<usize> {
    let metas: Vec<String> = entries
        .iter()
        .map(|e| format!("{} \u{b7} {}", e.graph.workspace, e.graph.updated))
        .collect();
    let icon = |ui: &mut egui::Ui, rect: egui::Rect, color: egui::Color32| {
        icons::GIT_TREE
            .image(rect.height(), color)
            .paint_at(ui, rect);
    };
    type Trailing<'a> = Box<dyn Fn(&mut egui::Ui, &Theme) + 'a>;
    let trailings: Vec<Trailing<'_>> = entries
        .iter()
        .map(|e| -> Trailing<'_> { Box::new(move |ui, theme| trailing(ui, theme, e)) })
        .collect();
    let items: Vec<ListCtrlItem<'_>> = entries
        .iter()
        .zip(&metas)
        .zip(&trailings)
        .map(|((e, meta), tr)| {
            ListCtrlItem::new(&e.graph.name)
                .description(meta)
                .icon(&icon)
                .trailing(&**tr)
        })
        .collect();
    let mut hit = None;
    ui.push_id(salt, |ui| {
        hit = ListCtrl::new()
            .width(width)
            .show(ui, theme, &items, None)
            .clicked;
    });
    hit
}

/// `rows` 섹션 Spec — 목록 행 4 종.
pub fn draw(ui: &mut egui::Ui, theme: &Theme) {
    let entries = entries();
    spec::stage(ui, theme, StageVariant::Column, |ui| {
        let w = ui.available_width();
        list(ui, theme, &entries, w, "dag_rows_spec");
    });
    spec::meta(
        ui,
        theme,
        &[
            ("row height", "36 · listctrl-row-min-height"),
            ("leading", "gitTree icon + name"),
            ("description", "workspace · last update"),
            ("trailing", "origin tag · rollup · done/total"),
            ("counter", "mono, never a progress bar"),
        ],
        &[
            TokenChip::new(
                "--tasty-dag-row-count-fg",
                "done/total",
                theme.dag_row_count_fg().to_egui(),
            ),
            TokenChip::new(
                "--tasty-dag-row-summary-gap",
                "trailing gap",
                theme.text_muted().to_egui(),
            ),
            TokenChip::new("--tasty-tag-bg", "derived tag", theme.tag_bg().to_egui()),
        ],
    );
    spec::note(
        ui,
        theme,
        "The rollup uses the same eight-state vocabulary as the nodes, so a row and the graph it \
         opens never disagree. Precedence is running > failed > all-terminal > ready > waiting — \
         one failure is louder than nine successes.",
    );
}
