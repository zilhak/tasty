//! DAG 목록 popup — workspace 스코프 + 목록↔단일 DAG drilldown.
//!
//! 탭 하나를 점유하는 [surface](crate::adapters::ui::surface::dag_graph) 와 달리
//! "잠깐 확인하고 닫는" 관측 창이다. 목록에서 DAG 하나를 고르면 **같은 영역**이
//! 그 그래프로 교체되고(`DrillDown`), back bar 로 목록에 돌아온다.
//!
//! # 왜 workspace 스코프인가
//!
//! DAG 는 workspace 단위 자원이다(task 영속 scope 가 workspace). 그 관측 창이
//! workspace 를 따라 붙는 것이 모델과 일치한다 — 다른 workspace 로 넘어가면 이
//! 창은 숨고, 돌아오면 보던 상태 그대로 다시 뜬다. tasty 의 popup 중
//! `PopupScope::Workspace` 를 실제로 쓰는 첫 사례다.
//!
//! # 왜 목록 자체는 전 workspace 인가
//!
//! 창이 workspace 에 붙는 것과 **목록의 범위**는 별개다. `agent.dag_list` 는
//! workspace 를 생략하면 전 workspace 를 훑도록 설계돼 있고(원칙 3 — 포커스
//! 독립성), 사람이 DAG 를 찾을 때도 "어느 워크스페이스에 뒀더라" 가 흔한
//! 질문이다. 그래서 기본은 전 workspace 나열 + 행마다 소속 workspace 표시이고,
//! "이 워크스페이스만" 은 토글로 둔다.
//!
//! # 에이전트 경로가 없는 이유
//!
//! 이 popup 을 여는 IPC 는 release 에 두지 않는다 — popup 강제 open 은 사용자
//! 조작의 재현이라 debug 격리 대상이다(`docs/dev-guide/debug-ipc.md`). 에이전트가
//! 필요한 것은 화면이 아니라 데이터이고, 그 수요는 `agent.dag_list` /
//! `agent.dag_get` 이 이미 충족한다.

use std::time::Instant;

use tasty_icons as icons;
use tasty_model::DagDirection;
use tasty_type_appearance::theme::Theme;
use tasty_ui_widgets::{
    Button, ButtonVariant, ControlSize, DrillDown, DrillDownView, Input, ListCtrl, ListCtrlItem,
    TagVariant, checkbox, hspace, margin_sym, select, tag,
};

use super::PopupAction;
use crate::adapters::ui::surface::dag_graph::{
    DagTarget, draw_dag_graph,
    model::{DagStatus, format_clock},
    node::status_colors,
    view::{DagGraphView, POLL_INTERVAL},
};
use crate::i18n::{t, t_fmt2};
use crate::state::AppState;

pub const DAG_LIST_POPUP_ID: &str = "dag_list";

/// 상태 필터 드롭다운의 위젯 id salt + 자식 오버레이 레지스트리 키. `select` 가
/// `("tasty_select", salt)` 로 egui popup id 를 만들므로 둘을 같은 값에서 유도한다.
const STATUS_SELECT_SALT: &str = "dag_list_status";
const STATUS_SELECT_OVERLAY_KEY: &str = "dag_list_status";

/// 창 크기 — 시안 확정 560 × 460 (`dag-popup-width` / `dag-popup-height`).
///
/// 상수 `default_size` 가 아니라 sizer 인 이유는 host UI zoom 이다. 토큰은 zoom 이
/// 이미 곱해진 값을 돌려주므로 여기서 읽으면 확대 배율에서도 내용이 잘리지 않는다.
pub fn dag_list_sizer(_state: &AppState, _engine: &crate::core::CoreState) -> egui::Vec2 {
    let th = crate::theme::theme();
    egui::vec2(th.dag_popup_width().value(), th.dag_popup_height().value())
}

/// 목록 한 행이 그릴 값 — 폴링 시점의 스냅샷.
///
/// `DagSummary` 를 그대로 들고 있지 않는 이유는 workspace **이름** 때문이다.
/// 요약은 id 만 알고 이름은 `CoreState` 에만 있는데, 그리는 시점에는 이미 engine
/// 을 놓았으므로 폴링에서 함께 접어 둔다.
pub struct DagRow {
    workspace_id: u32,
    workspace_name: String,
    id: String,
    name: String,
    /// `source == "derived"` — 사용자가 묶은 게 아니라 의존 연결성에서 도출된
    /// 그룹. 행 끝에 태그로 표시한다.
    derived: bool,
    rollup: DagStatus,
    done: usize,
    total: usize,
    updated_at: u64,
}

/// 목록 표시 순서 — **가장 최근에 움직인 DAG 가 위**.
///
/// 응답이 오는 순서를 그대로 쓰면 오래된 것이 위로 온다. 목록 응답은 (workspace
/// 순회 순서, DAG id 오름차순)인데 derived id 가 `c:<root_task_id>` 이고 task id 가
/// `t-{now_ms}-{seq}` 라, **id 오름차순 = 생성 시각 오름차순**이다. 방금 만든 DAG 가
/// 스크롤 바닥에 묻히고, workspace 경계가 1차 키라 전역 시간순으로 보이지도 않는다.
/// 덤으로 `'c' < 'd'` 라 derived 가 전부 explicit 앞에 몰린다.
///
/// **응답 순서 자체는 건드리지 않는다.** 그 결정론은 화면이 선택 상태를 id 로 들고
/// 폴링마다 재계산하기 위한 계약이고 CLI/IPC 소비자도 함께 본다. 표시 순서는 화면의
/// 관심사이므로 여기서만 다시 세운다.
///
/// 정렬 키는 `created_at` 이 아니라 `updated_at` 이다 — 소속 task 의 (`finished_at`
/// ∪ `started_at` ∪ `created_at`) 최대값이라 "방금 만든 것" 과 "방금 움직인 것" 을
/// 둘 다 위로 올린다. 목록을 여는 용건이 대개 후자다.
///
/// 동률은 `id` 내림차순, 그래도 같으면 `workspace_id` 오름차순으로 끊는다. explicit
/// id 는 사용자가 정한 키라 workspace 가 다르면 같은 값이 나올 수 있어 `id` 만으로는
/// 전순서가 아니다 — 세 키를 모두 쓰면 상류 순서에 기대지 않고 순서가 확정되고,
/// 아무것도 안 움직이는 동안 폴링이 여러 번 돌아도 행이 자리를 바꾸지 않는다.
fn sort_recent_first(rows: &mut [DagRow]) {
    rows.sort_by(|a, b| {
        b.updated_at
            .cmp(&a.updated_at)
            .then_with(|| b.id.cmp(&a.id))
            .then_with(|| a.workspace_id.cmp(&b.workspace_id))
    });
}

/// popup 의 전 상태. `on_close` 가 통째로 되돌린다(`DialogState` 관례).
#[derive(Default)]
pub struct DagListState {
    view: DrillDownView,
    /// 디테일이 보고 있는 DAG. 완전한 신원은 두 값의 **쌍**이다 — explicit id 는
    /// 사용자가 정한 키라 workspace 마다 같은 값이 나올 수 있다. 그래프 헤더의
    /// DAG 선택기는 같은 workspace 안에서 `dag_id` 만 갈아끼우므로 두 필드를
    /// 따로 둔다.
    open_workspace: Option<u32>,
    open_dag: Option<String>,
    query: String,
    this_workspace_only: bool,
    /// 0 = 전체, 그 외는 `DagStatus::ALL` 인덱스 + 1.
    status_filter: usize,
    direction: DagDirection,
    graph: DagGraphView,
    rows: Vec<DagRow>,
    last_list_poll: Option<Instant>,
}

impl DagListState {
    /// 목록도 그래프와 같은 주기로만 다시 읽는다.
    fn list_is_stale(&self, now: Instant) -> bool {
        self.last_list_poll
            .is_none_or(|t| now.duration_since(t) >= POLL_INTERVAL)
    }

    fn poll_list(&mut self, engine: &crate::core::CoreState) {
        let now = Instant::now();
        if !self.list_is_stale(now) {
            return;
        }
        self.last_list_poll = Some(now);
        match crate::core::agent::task::dag_list_from_state(engine, None) {
            Ok(summaries) => {
                self.rows = summaries
                    .into_iter()
                    .map(|s| {
                        let c = &s.state_counts;
                        DagRow {
                            workspace_name: engine
                                .workspaces
                                .iter()
                                .find(|w| w.id == s.workspace_id)
                                .map(|w| w.name.clone())
                                // 목록을 만드는 사이 workspace 가 사라지는 레이스 —
                                // 이름 대신 id 를 보여준다(행을 감추면 사라진
                                // 이유를 알 수 없다).
                                .unwrap_or_else(|| s.workspace_id.to_string()),
                            workspace_id: s.workspace_id,
                            id: s.id,
                            name: s.name,
                            derived: s.source == "derived",
                            rollup: DagStatus::from_name(s.rollup_state),
                            done: c.succeeded + c.failed + c.cancelled + c.skipped,
                            total: s.task_count,
                            updated_at: s.updated_at,
                        }
                    })
                    .collect();
                sort_recent_first(&mut self.rows);
            }
            Err(e) => {
                // 마지막으로 성공한 목록을 그대로 둔다 — 일시적 실패로 목록이
                // 비었다 돌아오면 읽는 사람이 더 혼란스럽다(그래프 폴링과 같은 계약).
                tracing::warn!(target: "tasty::dag", "dag list poll failed: {e}");
            }
        }
    }

    /// 필터를 통과한 행의 인덱스.
    fn visible(&self, active_workspace_id: Option<u32>) -> Vec<usize> {
        let needle = self.query.trim().to_lowercase();
        let want = self
            .status_filter
            .checked_sub(1)
            .and_then(|i| DagStatus::ALL.get(i).copied());
        self.rows
            .iter()
            .enumerate()
            .filter(|(_, r)| {
                if self.this_workspace_only && active_workspace_id != Some(r.workspace_id) {
                    return false;
                }
                if want.is_some_and(|w| w != r.rollup) {
                    return false;
                }
                needle.is_empty()
                    || r.name.to_lowercase().contains(&needle)
                    || r.workspace_name.to_lowercase().contains(&needle)
            })
            .map(|(i, _)| i)
            .collect()
    }
}

/// popup 본문.
pub fn draw_dag_list_popup(
    ui: &mut egui::Ui,
    state: &mut AppState,
    engine: &mut crate::core::CoreState,
) -> PopupAction {
    let active_workspace_id = engine.workspaces.get(state.active_workspace).map(|w| w.id);
    let dag = &mut state.dialogs.dag_list;

    if ui.ctx().input(|i| i.key_pressed(egui::Key::Escape)) {
        // 디테일에서는 Esc 가 먼저 목록으로 돌아간다 — 한 번에 창까지 닫으면
        // "뒤로" 한 걸음을 통째로 잃는다.
        if dag.view.is_detail() {
            back_to_list(dag);
            return PopupAction::None;
        }
        return PopupAction::Close;
    }

    dag.poll_list(engine);
    if let (Some(ws), Some(id)) = (dag.open_workspace, dag.open_dag.clone()) {
        dag.graph.poll_if_stale(engine, ws, Some(id.as_str()));
    }

    // 러너는 별도 thread 라 그 진행이 egui 를 깨우지 않는다 — 열려 있는 동안만
    // 스스로 다음 폴링 시점에 깨어난다. 스코프 밖 workspace 에서는 draw 자체가
    // 돌지 않으므로 예약도 남지 않는다.
    ui.ctx().request_repaint_after(POLL_INTERVAL);

    let th = crate::theme::theme();
    let theme = &th;
    ui.spacing_mut().item_spacing = egui::vec2(0.0, 0.0);

    let visible = dag.visible(active_workspace_id);
    let total = dag.rows.len();
    let title = dag
        .open_dag
        .as_ref()
        .and_then(|id| dag.rows.iter().find(|r| &r.id == id))
        .map(|r| r.name.clone())
        .unwrap_or_default();

    let mut close = false;
    let view = dag.view;
    // `DrillDown::show` 는 목록/디테일 클로저를 **둘 다** 받고 실제로는 하나만
    // 부른다. 둘 다 상태를 써야 하므로 컴파일 타임에는 겹치는 &mut 두 개가 되고,
    // 런타임에는 절대 겹치지 않는다 — 그 간극을 `RefCell` 로 메운다.
    let cell = std::cell::RefCell::new(dag);
    let out = DrillDown::new("dag_list")
        .view(view)
        .title(&title)
        .back_label(t("dag_list.back"))
        .show(
            ui,
            theme,
            |ui, theme| close |= draw_list(ui, theme, &mut cell.borrow_mut(), &visible, total),
            |ui, theme| draw_detail_graph(ui, theme, &mut cell.borrow_mut()),
            // back bar 우측 actions 슬롯은 비운다 — 러너 배지·줌·방향 토글은
            // 재사용하는 그래프 헤더가 이미 갖고 있어, 여기 또 두면 중복이다.
            None,
        );

    if out.back_clicked {
        back_to_list(cell.into_inner());
    }

    if close {
        PopupAction::Close
    } else {
        PopupAction::None
    }
}

/// 디테일 → 목록. 그래프 쪽 상태도 함께 놓아준다.
fn back_to_list(dag: &mut DagListState) {
    dag.view = DrillDownView::List;
    dag.open_workspace = None;
    dag.open_dag = None;
    dag.graph = DagGraphView::default();
}

/// 목록 뷰 — 검색·필터 줄 · 토글 줄 · 행 목록 · 푸터. 반환값은 "닫기" 눌림.
fn draw_list(
    ui: &mut egui::Ui,
    theme: &Theme,
    dag: &mut DagListState,
    visible: &[usize],
    total: usize,
) -> bool {
    // `separator` 는 **미리 곱해진**(premultiplied) 반투명 색이다 — `to_egui()` 로
    // 읽으면 알파가 한 번 더 곱해져 배경보다 어두운, 사실상 보이지 않는 선이 된다.
    let sep = egui::Stroke::new(
        theme.border_width.value(),
        theme.separator.to_egui_premultiplied(),
    );
    let full = ui.available_rect_before_wrap();
    let x_range = full.x_range();

    // ── 검색 + 상태 필터 ──
    let search_ir = egui::Frame::NONE
        .inner_margin(margin_sym(theme.spacing_md, theme.spacing_sm))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                let filter_w = theme.field_width_md.value();
                let search_w =
                    (ui.available_width() - filter_w - theme.spacing_sm.value()).max(0.0);
                Input::new()
                    .icon(&|ui, rect, color| {
                        icons::SEARCH.image(rect.height(), color).paint_at(ui, rect);
                    })
                    .placeholder(t("dag_list.search_placeholder"))
                    .width(search_w)
                    .show(ui, theme, &mut dag.query);
                hspace(ui, theme.spacing_sm);
                let labels: Vec<&str> = std::iter::once(t("dag_list.status_any"))
                    .chain(DagStatus::ALL.iter().map(|s| s.label()))
                    .collect();
                select(
                    ui,
                    theme,
                    STATUS_SELECT_SALT,
                    &mut dag.status_filter,
                    &labels,
                    filter_w,
                    true,
                );
                // 드롭다운은 egui 자체 오버레이라 popup rect 밖으로 나갈 수 있다 —
                // 그 위의 클릭·호버가 "팝업 바깥" 으로 오판되지 않도록 실측 rect 를
                // 매니저에 보고한다(닫혀 있으면 None 으로 정리).
                let overlay_id = ui.make_persistent_id(("tasty_select", STATUS_SELECT_SALT));
                let overlay_rect = ui
                    .memory(|m| m.is_popup_open(overlay_id))
                    .then(|| ui.memory(|m| m.area_rect(overlay_id)))
                    .flatten();
                super::report_child_overlay_rect(
                    ui.ctx(),
                    DAG_LIST_POPUP_ID,
                    STATUS_SELECT_OVERLAY_KEY,
                    overlay_rect,
                );
            });
        });
    ui.painter()
        .hline(x_range, search_ir.response.rect.bottom(), sep);

    // ── "이 워크스페이스만" 토글 ──
    let toggle_ir = egui::Frame::NONE
        .inner_margin(margin_sym(theme.spacing_md, theme.spacing_xs))
        .show(ui, |ui| {
            checkbox(
                ui,
                theme,
                &mut dag.this_workspace_only,
                t("dag_list.this_workspace_only"),
                true,
            );
        });
    ui.painter()
        .hline(x_range, toggle_ir.response.rect.bottom(), sep);

    // ── 푸터 자리를 먼저 떼어 두고 남는 높이를 목록에 준다 ──
    let footer_h = ControlSize::Md.height(theme) + theme.spacing_sm.value() * 2.0;
    let list_h = (full.bottom() - ui.cursor().top() - footer_h).max(0.0);

    let mut picked = None;
    ui.allocate_ui(egui::vec2(full.width(), list_h), |ui| {
        egui::ScrollArea::vertical()
            .id_salt("dag_list_rows")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                if visible.is_empty() {
                    draw_empty(ui, theme, total);
                    return;
                }
                // `ListCtrlItem` 은 라벨·클로저를 **빌리기만** 한다. 파생 문자열과
                // 행별 trailing 클로저를 먼저 지어 두어야 items 보다 오래 산다.
                let prepared: Vec<(String, &DagRow)> = visible
                    .iter()
                    .map(|&i| {
                        let r = &dag.rows[i];
                        let meta = t_fmt2(
                            "dag_list.row_meta",
                            &r.workspace_name,
                            &format_clock(r.updated_at),
                        );
                        (meta, r)
                    })
                    .collect();
                let icon = |ui: &mut egui::Ui, rect: egui::Rect, color: egui::Color32| {
                    icons::GIT_TREE
                        .image(rect.height(), color)
                        .paint_at(ui, rect);
                };
                type Trailing<'a> = Box<dyn Fn(&mut egui::Ui, &Theme) + 'a>;
                let trailings: Vec<Trailing<'_>> = prepared
                    .iter()
                    .map(|(_, r)| -> Trailing<'_> {
                        Box::new(move |ui, theme| draw_row_trailing(ui, theme, r))
                    })
                    .collect();
                let items: Vec<ListCtrlItem<'_>> = prepared
                    .iter()
                    .zip(&trailings)
                    .map(|((meta, r), trailing)| {
                        ListCtrlItem::new(&r.name)
                            .description(meta)
                            .icon(&icon)
                            .trailing(&**trailing)
                    })
                    .collect();
                if let Some(hit) = ListCtrl::new().show(ui, theme, &items, None).clicked {
                    picked = visible.get(hit).copied();
                }
            });
    });

    if let Some(i) = picked {
        let r = &dag.rows[i];
        dag.open_workspace = Some(r.workspace_id);
        dag.open_dag = Some(r.id.clone());
        // 이전 DAG 의 줌/선택/레이아웃이 새 그래프로 새어 들어가지 않게 통째로
        // 새로 시작한다 — 폴링 시각도 비어 첫 프레임에 곧바로 읽는다.
        dag.graph = DagGraphView::default();
        dag.view = DrillDownView::Detail;
    }

    // ── 푸터 — 보이는 개수 + 닫기 ──
    let mut close = false;
    ui.painter().hline(x_range, ui.cursor().top(), sep);
    ui.allocate_ui(egui::vec2(full.width(), footer_h), |ui| {
        egui::Frame::NONE
            .inner_margin(margin_sym(theme.spacing_md, theme.spacing_sm))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new(t_fmt2(
                            "dag_list.count",
                            &visible.len().to_string(),
                            &total.to_string(),
                        ))
                        .monospace()
                        .size(theme.font_size_caption.value())
                        .color(theme.text_muted().to_egui()),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        close = Button::new(t("dag_list.close"))
                            .variant(ButtonVariant::Secondary)
                            .show(ui, theme)
                            .clicked();
                    });
                });
            });
    });
    close
}

/// 행 끝 클러스터 — 출처 태그 · rollup 상태 · mono `7/12`.
///
/// 진행을 막대가 아니라 **글자**로 둔다: 12 개짜리 그래프에서 막대 한 칸은 8% 라
/// 눈으로 셋과 넷을 구분할 수 없고, 정확한 수가 이 화면의 용건이다.
fn draw_row_trailing(ui: &mut egui::Ui, theme: &Theme, row: &DagRow) {
    // `ListCtrl` 은 trailing 슬롯을 **오른쪽에서 왼쪽으로** 채운다(행 오른쪽 끝에
    // 붙여야 하므로). 그래서 먼저 낸 것이 가장 오른쪽에 놓인다 — 시안의 왼→오른
    // 순서(태그 · 상태 · 카운터)를 얻으려면 여기서는 거꾸로 낸다.
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = theme.dag_row_summary_gap().value();
        ui.label(
            egui::RichText::new(format!("{}/{}", row.done, row.total))
                .monospace()
                .size(theme.dag_row_count_font_size().value())
                .color(theme.dag_row_count_fg().to_egui()),
        );
        // 라벨 색은 상태 색이 아니라 `-label` role 에서 읽는다 — 상태 색을 작은
        // 캡션 라벨에 그대로 쓰면 4.5:1 을 밑돈다(노드 카드와 같은 규칙).
        let (_, _, label_fg) = status_colors(theme, row.rollup);
        ui.label(
            egui::RichText::new(format!("{} {}", row.rollup.glyph(), row.rollup.label()))
                .monospace()
                .size(theme.font_size_caption.value())
                .color(label_fg.to_egui()),
        );
        if row.derived {
            tag(ui, theme, t("dag_list.derived"), TagVariant::Default, false);
        }
    });
}

/// 빈 목록 2 종 — DAG 가 아예 없는 경우와 필터가 다 걸러낸 경우.
///
/// 둘을 나누는 이유는 다음 행동이 다르기 때문이다: 앞은 "DAG 를 만들어야 한다",
/// 뒤는 "필터를 풀어야 한다".
fn draw_empty(ui: &mut egui::Ui, theme: &Theme, total: usize) {
    let (title, hint) = if total == 0 {
        ("dag_list.empty_none", "dag_list.empty_none_hint")
    } else {
        ("dag_list.empty_filtered", "dag_list.empty_filtered_hint")
    };
    ui.vertical_centered(|ui| {
        ui.add_space(ui.available_height() / 3.0);
        ui.label(
            egui::RichText::new(t(title))
                .size(theme.font_size_body.value())
                .color(theme.text_primary().to_egui()),
        );
        ui.add_space(theme.spacing_xs.value());
        ui.label(
            egui::RichText::new(t(hint))
                .size(theme.font_size_caption.value())
                .color(theme.text_muted().to_egui()),
        );
    });
}

/// 디테일 뷰 — surface 와 **같은** 그래프 렌더를 그대로 부른다.
///
/// popup 폭(560)이 상세 도킹 임계값(640) 아래라 노드 상세는 자동으로 하단 시트가
/// 된다 — 시안이 "popup 은 항상 하단 도킹" 으로 확정한 배치와 같은 결과다.
fn draw_detail_graph(ui: &mut egui::Ui, theme: &Theme, dag: &mut DagListState) {
    if dag.open_dag.is_none() {
        // back 직후 한 프레임 — 캔버스 바닥색만 칠하고 넘어간다.
        ui.painter()
            .rect_filled(ui.max_rect(), 0.0, theme.dag_canvas_bg().to_egui());
        return;
    }
    let target = DagTarget {
        dag_id: &mut dag.open_dag,
        direction: &mut dag.direction,
    };
    draw_dag_graph(ui, target, &mut dag.graph);
}

/// 닫힘 정리 — 어떤 경로로 닫히든 다음 open 은 **목록 뷰**에서 시작한다.
pub fn on_close_dag_list_popup(
    _ctx: &egui::Context,
    state: &mut AppState,
    _engine: &mut crate::core::CoreState,
) {
    state.dialogs.dag_list = DagListState::default();
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 정렬이 보는 세 필드만 실제 값으로 채운 행. 나머지는 표시용이라 정렬에
    /// 영향이 없다.
    fn row(id: &str, updated_at: u64, workspace_id: u32) -> DagRow {
        DagRow {
            workspace_id,
            workspace_name: String::new(),
            id: id.to_string(),
            name: String::new(),
            derived: id.starts_with("c:"),
            rollup: DagStatus::Waiting,
            done: 0,
            total: 0,
            updated_at,
        }
    }

    fn ids(rows: &[DagRow]) -> Vec<&str> {
        rows.iter().map(|r| r.id.as_str()).collect()
    }

    #[test]
    fn 최근_갱신이_맨_위로_온다() {
        let mut rows = vec![
            row("c:t-100", 100, 1),
            row("c:t-300", 300, 1),
            row("c:t-200", 200, 1),
        ];
        sort_recent_first(&mut rows);
        assert_eq!(
            rows.iter().map(|r| r.updated_at).collect::<Vec<_>>(),
            [300, 200, 100]
        );
    }

    /// 동률은 `id` 내림차순으로 끊는다 — 같은 ms 에 만들어진 DAG 가 폴링마다
    /// 자리를 바꾸면 클릭하려던 행이 발밑에서 움직인다.
    #[test]
    fn 갱신시각_동률은_id_내림차순으로_끊는다() {
        let mut rows = vec![
            row("c:t-1", 500, 1),
            row("d:beta", 500, 1),
            row("c:t-2", 500, 1),
        ];
        sort_recent_first(&mut rows);
        assert_eq!(ids(&rows), ["d:beta", "c:t-2", "c:t-1"]);
    }

    /// explicit id 는 사용자가 정한 키라 workspace 가 다르면 같은 값이 나올 수
    /// 있다 — 그때도 순서가 확정돼야 한다.
    #[test]
    fn 갱신시각과_id_가_모두_같으면_workspace_로_끊는다() {
        let mut rows = vec![row("d:same", 500, 7), row("d:same", 500, 2)];
        sort_recent_first(&mut rows);
        assert_eq!(
            rows.iter().map(|r| r.workspace_id).collect::<Vec<_>>(),
            [2, 7]
        );
    }

    /// 같은 입력을 두 번 정렬해도 같은 결과 — 아무것도 안 움직이는 동안 폴링이
    /// 여러 번 돌아도 목록이 요동하지 않는다.
    #[test]
    fn 반복_정렬은_같은_결과를_낸다() {
        let build = || {
            vec![
                row("d:alpha", 500, 3),
                row("c:t-9", 500, 1),
                row("c:t-1", 900, 2),
                row("d:zeta", 100, 1),
            ]
        };
        let mut once = build();
        sort_recent_first(&mut once);
        let mut twice = once;
        sort_recent_first(&mut twice);
        let mut from_scratch = build();
        sort_recent_first(&mut from_scratch);
        assert_eq!(ids(&twice), ids(&from_scratch));
    }

    /// 정렬 키가 `created_at` 이 아님을 증명한다 — 가장 먼저 만들어졌지만 방금
    /// 상태가 바뀐 DAG 가 맨 위로 온다. id 가 생성 시각 오름차순이므로 "가장 작은
    /// id" 가 "가장 먼저 만들어진 것" 이다.
    #[test]
    fn 오래_전에_만들어졌어도_방금_움직였으면_맨_위() {
        let mut rows = vec![
            row("c:t-1000000000001", 999, 1), // 가장 오래 전 생성, 방금 갱신
            row("c:t-1000000000002", 200, 1),
            row("c:t-1000000000003", 300, 1), // 가장 최근 생성
        ];
        sort_recent_first(&mut rows);
        assert_eq!(
            ids(&rows),
            [
                "c:t-1000000000001",
                "c:t-1000000000003",
                "c:t-1000000000002"
            ]
        );
    }

    /// `'c' < 'd'` 사전순 편향이 제거됐는지 — 더 최근에 움직인 derived 가 explicit
    /// 보다 위로 온다(그 반대도 성립한다).
    #[test]
    fn derived_와_explicit_이_출처와_무관하게_섞인다() {
        let mut rows = vec![
            row("d:old-explicit", 100, 1),
            row("c:t-new-derived", 900, 1),
            row("d:new-explicit", 800, 1),
            row("c:t-old-derived", 200, 1),
        ];
        sort_recent_first(&mut rows);
        assert_eq!(
            ids(&rows),
            [
                "c:t-new-derived",
                "d:new-explicit",
                "c:t-old-derived",
                "d:old-explicit"
            ]
        );
    }

    /// workspace 경계를 넘어 **전역**으로 정렬된다 — workspace 별로 뭉치면 안 된다.
    #[test]
    fn workspace_경계를_넘어_전역으로_정렬된다() {
        let mut rows = vec![
            row("c:t-a", 100, 1),
            row("c:t-b", 400, 1),
            row("c:t-c", 200, 2),
            row("c:t-d", 300, 2),
        ];
        sort_recent_first(&mut rows);
        assert_eq!(
            rows.iter()
                .map(|r| (r.workspace_id, r.updated_at))
                .collect::<Vec<_>>(),
            [(1, 400), (2, 300), (2, 200), (1, 100)]
        );
    }
}
