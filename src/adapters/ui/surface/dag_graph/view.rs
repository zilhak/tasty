//! surface별 이동·배율·선택과 조회·레이아웃 캐시.
//! 보이는 surface만 다음 조회 시각을 내보내고 호스트가 나머지 타이머를 취소한다.
//! 레이아웃 캐시는 ID·엣지·방향·치수를 비교하며 task 상태만 바뀌면 좌표를 유지한다.

use std::collections::HashMap;
use std::hash::{Hash as _, Hasher as _};
use std::time::{Duration, Instant};

use tasty_dag_layout::{GraphLayout, LayoutConfig, Orientation, layout_dag};
use tasty_model::{DagDirection, DagGraphSurface, SurfaceId};
use tasty_type_appearance::theme::Theme;

use super::model::{DagData, DagListEntry, DagStatus, RunnerBadgeData, build_graph};

/// DAG 데이터 조회 주기.
pub const POLL_INTERVAL: Duration = Duration::from_millis(500);

/// 줌 범위와 단위. 길이가 아니라 배율이라 `LogicalPx` 대상이 아니다.
pub const ZOOM_MIN: f32 = 0.2;
pub const ZOOM_MAX: f32 = 1.5;
pub const ZOOM_STEP: f32 = 0.1;
/// `fit` 의 상한 — 작은 그래프를 확대하지 않는다.
pub const ZOOM_FIT_MAX: f32 = 1.0;

/// 노드 LOD 3단계. 박스 크기는 tier 와 무관하게 고정이고 **내용만** 바뀐다.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lod {
    /// 이름 + 상태 라벨 + duration.
    Full,
    /// 글리프 + 이름.
    Compact,
    /// 상태 채움만.
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

/// surface와 팝업이 공유하는 관찰 대상. 워크스페이스는 데이터 조회 시 별도로 받는다.
pub struct DagTarget<'a> {
    /// 보고 있는 DAG. 헤더 드롭다운 선택이 여기 반영된다.
    pub dag_id: &'a mut Option<String>,
    /// 레이어 진행 방향. 방향 토글이 여기 반영된다.
    pub direction: &'a mut DagDirection,
}

/// 이번 프레임에 어떤 DAG surface 가 무엇을 보고 있는지 — 렌더 루프 진입 **전에**
/// 수집해 폴링에 넘긴다(루프 안에서는 `engine` 을 재차입할 수 없다).
#[derive(Debug, Clone)]
pub struct DagPollRequest {
    pub surface_id: SurfaceId,
    /// 관찰 대상 workspace — 모델이 명시하지 않으면 소속 workspace.
    pub workspace_id: u32,
    pub dag_id: Option<String>,
}

impl DagPollRequest {
    /// 렌더 중인 surface 로부터. `containing_workspace` 는 이 surface 가 놓인
    /// workspace 다(활성 workspace 가 아니라 **소속** — 렌더 대상이 활성 workspace
    /// 뿐이라 값이 같을 뿐이다).
    pub fn from_surface(panel: &DagGraphSurface, containing_workspace: u32) -> Self {
        Self {
            surface_id: panel.id,
            workspace_id: panel.workspace_id.unwrap_or(containing_workspace),
            dag_id: panel.dag_id.clone(),
        }
    }
}

/// 캐시된 좌표를 읽으며 뷰 상태도 바꿀 수 있도록 Rc로 공유한다.
struct CachedLayout {
    key: u64,
    layout: std::rc::Rc<GraphLayout>,
}

/// surface 하나의 휘발성 뷰 상태.
pub struct DagGraphView {
    /// 마지막 폴링 결과. `None` 이면 아직 한 번도 읽지 않았다.
    pub data: Option<DagData>,
    /// 마지막 폴링 시각 — [`POLL_INTERVAL`] 게이트.
    last_poll: Option<Instant>,
    /// 폴링이 실패했을 때의 사유(스토어 에러). 화면 하단에 조용히 표시한다.
    pub error: Option<String>,

    pub zoom: f32,
    /// 그래프 원점의 화면 오프셋(캔버스 좌상단 기준, logical px).
    pub offset: egui::Vec2,
    /// 선택된 task id.
    pub selected: Option<String>,
    /// 자동 맞춤을 적용한 DAG·방향·뷰포트 조합.
    fit_key: Option<String>,
    layout: Option<CachedLayout>,
}

impl Default for DagGraphView {
    fn default() -> Self {
        Self {
            data: None,
            last_poll: None,
            error: None,
            zoom: 1.0,
            offset: egui::Vec2::ZERO,
            selected: None,
            fit_key: None,
            layout: None,
        }
    }
}

impl DagGraphView {
    /// 이 프레임에 새로 읽어야 하는지.
    fn is_stale(&self, now: Instant) -> bool {
        self.last_poll
            .is_none_or(|t| now.duration_since(t) >= POLL_INTERVAL)
    }

    /// 다음 폴링 시각. 아직 한 번도 안 읽었으면 `None`(= 즉시 읽어야 함).
    /// 호스트가 이 값을 `Tick::DagGraph` 데드라인으로 쓴다.
    pub fn next_poll_at(&self) -> Option<Instant> {
        self.last_poll.map(|t| t + POLL_INTERVAL)
    }

    /// surface와 팝업이 공유하는 주기별 데이터 조회.
    pub fn poll_if_stale(
        &mut self,
        engine: &crate::core::CoreState,
        workspace_id: u32,
        dag_id: Option<&str>,
    ) {
        let now = Instant::now();
        if !self.is_stale(now) {
            return;
        }
        self.last_poll = Some(now);
        match fetch(engine, workspace_id, dag_id) {
            Ok(data) => {
                self.error = None;
                self.data = Some(data);
            }
            Err(e) => {
                // 일시적 실패에는 마지막으로 읽은 그래프를 유지한다.
                tracing::warn!(target: "tasty::dag", "dag poll failed: {e}");
                self.error = Some(e);
            }
        }
    }

    /// 보고 있던 DAG 가 바뀌었으니 다음 프레임에 곧바로 다시 읽는다.
    /// (popup 의 목록→디테일 진입처럼 500ms 를 기다릴 이유가 없는 전환용.)
    pub fn invalidate_poll(&mut self) {
        self.last_poll = None;
    }

    /// 현재 데이터에 대한 레이아웃. 그래프 **모양**이 그대로면 캐시를 돌려준다.
    pub fn layout(
        &mut self,
        direction: DagDirection,
        cfg: &LayoutConfig,
    ) -> std::rc::Rc<GraphLayout> {
        let key = self.shape_key(direction, cfg);
        if self.layout.as_ref().is_none_or(|c| c.key != key) {
            let layout = std::rc::Rc::new(self.compute_layout(cfg));
            self.layout = Some(CachedLayout { key, layout });
        }
        std::rc::Rc::clone(
            &self
                .layout
                .as_ref()
                .expect("layout cache filled above")
                .layout,
        )
    }

    fn compute_layout(&self, cfg: &LayoutConfig) -> GraphLayout {
        let Some(graph) = self.data.as_ref().and_then(|d| d.current.as_ref()) else {
            return GraphLayout::default();
        };
        let ids: Vec<String> = graph.nodes.iter().map(|n| n.id.clone()).collect();
        let edges: Vec<(usize, usize)> = graph.edges.iter().map(|e| (e.from, e.to)).collect();
        layout_dag(&ids, &edges, cfg)
    }

    /// 상태를 제외한 레이아웃 캐시 키.
    fn shape_key(&self, direction: DagDirection, cfg: &LayoutConfig) -> u64 {
        let mut h = std::collections::hash_map::DefaultHasher::new();
        direction.as_str().hash(&mut h);
        for px in [
            cfg.node_size.0,
            cfg.node_size.1,
            cfg.layer_gap,
            cfg.sibling_gap,
            cfg.component_gap,
        ] {
            px.value().to_bits().hash(&mut h);
        }
        if let Some(graph) = self.data.as_ref().and_then(|d| d.current.as_ref()) {
            graph.id.hash(&mut h);
            for n in &graph.nodes {
                n.id.hash(&mut h);
            }
            for e in &graph.edges {
                e.from.hash(&mut h);
                e.to.hash(&mut h);
            }
        }
        h.finish()
    }

    /// 같은 DAG·방향·32px 단위 뷰포트에서는 자동 맞춤을 한 번만 수행한다.
    pub fn take_fit(
        &mut self,
        dag_id: &str,
        direction: DagDirection,
        viewport: egui::Vec2,
    ) -> bool {
        let key = format!(
            "{dag_id}:{}:{}:{}",
            direction.as_str(),
            (viewport.x / 32.0).round() as i32,
            (viewport.y / 32.0).round() as i32
        );
        if self.fit_key.as_deref() == Some(key.as_str()) {
            return false;
        }
        self.fit_key = Some(key);
        true
    }

    /// 줌을 한 단계 바꾸되 `anchor`(캔버스 로컬 좌표) 아래의 그래프 점을 고정한다.
    pub fn zoom_by(&mut self, steps: f32, anchor: egui::Vec2) {
        let next = (self.zoom + steps * ZOOM_STEP).clamp(ZOOM_MIN, ZOOM_MAX);
        // 부동소수 누적으로 88.00001% 같은 값이 되지 않게 10% 격자에 맞춘다.
        let next = (next * 10.0).round() / 10.0;
        if (next - self.zoom).abs() < f32::EPSILON {
            return;
        }
        self.offset = anchor - (anchor - self.offset) / self.zoom * next;
        self.zoom = next;
    }

    /// 그래프 전체가 들어오도록 줌/오프셋을 맞춘다.
    pub fn fit(&mut self, graph_size: egui::Vec2, viewport: egui::Vec2, padding: f32) {
        if graph_size.x <= 0.0 || graph_size.y <= 0.0 {
            return;
        }
        let usable = viewport - egui::vec2(padding * 2.0, padding * 2.0);
        let k = (usable.x / graph_size.x)
            .min(usable.y / graph_size.y)
            .clamp(ZOOM_MIN, ZOOM_FIT_MAX);
        self.zoom = k;
        self.offset = egui::vec2(
            ((viewport.x - graph_size.x * k) / 2.0).max(padding),
            ((viewport.y - graph_size.y * k) / 2.0).max(padding),
        );
    }
}

/// surface id 로 keying 하는 뷰 스토어. `AppState` 가 보유한다.
#[derive(Default)]
pub struct DagGraphViewStore {
    views: HashMap<SurfaceId, DagGraphView>,
    /// 직전 [`Self::poll`] 이 받은 = **이번 프레임에 실제로 보인** surface 들.
    /// 폴링 타이머는 이 집합에만 걸린다(보이지 않으면 예약 없음).
    visible: Vec<SurfaceId>,
}

impl DagGraphViewStore {
    pub fn get_or_init(&mut self, surface_id: SurfaceId) -> &mut DagGraphView {
        self.views.entry(surface_id).or_default()
    }

    /// 닫힌 surface의 상태와 조회 예약 대상을 함께 지운다.
    pub fn drop_view(&mut self, surface_id: SurfaceId) {
        self.views.remove(&surface_id);
        self.visible.retain(|s| *s != surface_id);
    }

    /// 보이는 뷰의 다음 조회 시각. 호스트는 여기에 없는 타이머를 취소한다.
    pub fn pending_poll_deadlines(&self, now: Instant) -> Vec<(SurfaceId, Instant)> {
        self.visible
            .iter()
            .filter_map(|sid| {
                let v = self.views.get(sid)?;
                Some((*sid, v.next_poll_at().unwrap_or(now)))
            })
            .collect()
    }

    /// 렌더링의 engine 대여가 시작되기 전에 보이는 DAG 데이터를 읽는다.
    pub fn poll(&mut self, engine: &crate::core::CoreState, requests: &[DagPollRequest]) {
        self.note_visible(requests);
        for req in requests {
            self.views.entry(req.surface_id).or_default().poll_if_stale(
                engine,
                req.workspace_id,
                req.dag_id.as_deref(),
            );
        }
    }

    /// 보이는 대상 목록을 교체한다. 모두 숨겨진 프레임도 빈 requests로 호출해야
    /// 이전 대상의 타이머가 다시 등록되지 않는다.
    fn note_visible(&mut self, requests: &[DagPollRequest]) {
        self.visible.clear();
        self.visible.extend(requests.iter().map(|r| r.surface_id));
    }
}

/// 한 화면 분의 데이터를 memory store 에서 읽어 화면 형태로 만든다.
fn fetch(
    engine: &crate::core::CoreState,
    workspace_id: u32,
    dag_id: Option<&str>,
) -> Result<DagData, String> {
    use tasty_agent::{TaskGraph, group_tasks_into_dags};

    let tasks = crate::core::agent::task::task_list_from_state(engine, workspace_id)
        .map_err(|e| e.to_string())?;
    let summaries = group_tasks_into_dags(&tasks);

    let dags: Vec<DagListEntry> = summaries
        .iter()
        .map(|s| DagListEntry {
            id: s.id.clone(),
            name: s.name.clone(),
            rollup: DagStatus::from_name(s.rollup_state),
            task_count: s.task_count,
        })
        .collect();

    let chosen = pick_target(&summaries, dag_id);
    let target_missing = dag_id.is_some() && chosen.is_none();

    let current = chosen.map(|summary| {
        let subset: Vec<tasty_agent::Task> = tasks
            .iter()
            .filter(|t| summary.task_ids.contains(&t.id))
            .cloned()
            .collect();
        let mut graph = build_graph(summary, &subset);
        // 순환 관계가 있어도 그래프는 그린다. 배너에 표시할 경로만 추가로 찾으며
        // 그룹 밖 의존성 오류는 순환 경고로 처리하지 않는다.
        graph.cycle = if summary.has_cycle {
            match TaskGraph::build(&subset).detect_cycles() {
                Err(tasty_agent::AgentError::DependencyCycle(msg)) => Some(msg),
                _ => None,
            }
        } else {
            None
        };
        graph
    });

    let (running, crashed) = crate::core::agent::task::runner_liveness(engine, workspace_id);
    let runner = current
        .as_ref()
        .map(|g| RunnerBadgeData {
            running,
            crashed,
            ready: g
                .nodes
                .iter()
                .filter(|n| n.status == DagStatus::Ready)
                .count(),
            running_count: g
                .nodes
                .iter()
                .filter(|n| n.status == DagStatus::Running)
                .count(),
        })
        .unwrap_or(RunnerBadgeData {
            running,
            crashed,
            ..RunnerBadgeData::default()
        });

    Ok(DagData {
        workspace_id,
        dags,
        current,
        runner,
        target_missing,
    })
}

/// 지정한 DAG가 없으면 대체하지 않는다. 미지정이면 실행 중인 것, 그다음 최근 갱신한 것을 고른다.
fn pick_target<'a>(
    summaries: &'a [tasty_agent::DagSummary],
    dag_id: Option<&str>,
) -> Option<&'a tasty_agent::DagSummary> {
    if let Some(id) = dag_id {
        return summaries.iter().find(|s| s.id == id);
    }
    summaries
        .iter()
        .filter(|s| s.rollup_state == "running")
        .max_by_key(|s| s.updated_at)
        .or_else(|| summaries.iter().max_by_key(|s| s.updated_at))
}

/// 테마 토큰 → 레이아웃 설정. 이 crate 는 `Theme` 를 모르므로 여기서 주입한다.
pub fn layout_config(theme: &Theme, direction: DagDirection) -> LayoutConfig {
    LayoutConfig {
        orientation: match direction {
            DagDirection::LeftRight => Orientation::LeftRight,
            DagDirection::TopDown => Orientation::TopDown,
        },
        node_size: (theme.dag_node_width(), theme.dag_node_height()),
        layer_gap: theme.dag_layer_gap(),
        sibling_gap: theme.dag_sibling_gap(),
        ..LayoutConfig::default()
    }
}

#[cfg(test)]
mod tests {
    use super::super::model::{DagEdgeData, DagGraphData, DagNodeData, DagRelation};
    use super::*;

    fn req(surface_id: SurfaceId) -> DagPollRequest {
        DagPollRequest {
            surface_id,
            workspace_id: 1,
            dag_id: None,
        }
    }

    #[test]
    fn dropping_a_view_removes_it_from_the_poll_deadlines() {
        let now = Instant::now();
        let mut store = DagGraphViewStore::default();
        store.get_or_init(7);
        store.note_visible(&[req(7)]);
        assert_eq!(store.pending_poll_deadlines(now).len(), 1);

        store.drop_view(7);
        assert!(
            store.pending_poll_deadlines(now).is_empty(),
            "닫힌 뷰가 폴링 예약을 남기면 누수가 된다"
        );
    }

    /// 닫히지 않고 배경으로 간 뷰는 상태를 남기되 조회 예약에서는 제외한다.
    #[test]
    fn a_frame_with_no_visible_dag_view_clears_the_poll_deadlines() {
        let now = Instant::now();
        let mut store = DagGraphViewStore::default();
        store.get_or_init(7);
        store.note_visible(&[req(7)]);
        assert_eq!(store.pending_poll_deadlines(now).len(), 1);

        store.note_visible(&[]);
        assert!(store.pending_poll_deadlines(now).is_empty());
        assert!(
            store.views.contains_key(&7),
            "배경으로 밀렸을 뿐 닫힌 게 아니다 — 뷰 상태는 보존한다"
        );
    }

    #[test]
    fn only_the_views_visible_this_frame_keep_their_deadlines() {
        let now = Instant::now();
        let mut store = DagGraphViewStore::default();
        store.get_or_init(7);
        store.get_or_init(9);
        store.note_visible(&[req(7), req(9)]);
        assert_eq!(store.pending_poll_deadlines(now).len(), 2);

        store.note_visible(&[req(9)]);
        let due = store.pending_poll_deadlines(now);
        assert_eq!(due.len(), 1);
        assert_eq!(due[0].0, 9);
    }

    #[test]
    fn poll_deadline_advances_after_a_read() {
        let now = Instant::now();
        let mut view = DagGraphView::default();
        assert_eq!(view.next_poll_at(), None, "미폴링 = 즉시");
        view.last_poll = Some(now);
        assert_eq!(view.next_poll_at(), Some(now + POLL_INTERVAL));
    }

    #[test]
    fn lod_tiers_follow_zoom_thresholds() {
        assert_eq!(Lod::of(1.0), Lod::Full);
        assert_eq!(Lod::of(0.7), Lod::Full);
        assert_eq!(Lod::of(0.69), Lod::Compact);
        assert_eq!(Lod::of(0.4), Lod::Compact);
        assert_eq!(Lod::of(0.39), Lod::Block);
    }

    #[test]
    fn zoom_snaps_to_ten_percent_steps_and_clamps() {
        let mut v = DagGraphView::default();
        v.zoom_by(-1.0, egui::Vec2::ZERO);
        assert!((v.zoom - 0.9).abs() < 1e-6, "{}", v.zoom);
        for _ in 0..50 {
            v.zoom_by(-1.0, egui::Vec2::ZERO);
        }
        assert!((v.zoom - ZOOM_MIN).abs() < 1e-6, "{}", v.zoom);
        for _ in 0..50 {
            v.zoom_by(1.0, egui::Vec2::ZERO);
        }
        assert!((v.zoom - ZOOM_MAX).abs() < 1e-6, "{}", v.zoom);
    }

    #[test]
    fn zoom_keeps_the_anchor_point_fixed() {
        let mut v = DagGraphView {
            offset: egui::vec2(10.0, 20.0),
            ..DagGraphView::default()
        };
        let anchor = egui::vec2(100.0, 50.0);
        let before = (anchor - v.offset) / v.zoom;
        v.zoom_by(2.0, anchor);
        let after = (anchor - v.offset) / v.zoom;
        assert!((before - after).length() < 1e-3, "{before:?} vs {after:?}");
    }

    #[test]
    fn fit_never_magnifies_beyond_one_hundred_percent() {
        let mut v = DagGraphView::default();
        v.fit(egui::vec2(50.0, 20.0), egui::vec2(800.0, 600.0), 16.0);
        assert!(v.zoom <= ZOOM_FIT_MAX + 1e-6, "{}", v.zoom);
    }

    #[test]
    fn fit_runs_once_per_dag_direction_and_viewport_bucket() {
        let mut v = DagGraphView::default();
        let vp = egui::vec2(800.0, 600.0);
        assert!(v.take_fit("d:a", DagDirection::LeftRight, vp));
        assert!(!v.take_fit("d:a", DagDirection::LeftRight, vp));
        assert!(!v.take_fit("d:a", DagDirection::LeftRight, vp + egui::vec2(1.0, 0.0)));
        assert!(v.take_fit("d:a", DagDirection::TopDown, vp));
        assert!(v.take_fit("d:b", DagDirection::TopDown, vp));
        assert!(v.take_fit("d:b", DagDirection::TopDown, vp + egui::vec2(200.0, 0.0)));
    }

    #[test]
    fn explicit_target_is_never_silently_substituted() {
        let summaries = vec![
            summary("d:one", "running", 10),
            summary("d:two", "waiting", 20),
        ];
        assert_eq!(pick_target(&summaries, Some("d:two")).unwrap().id, "d:two");
        assert!(pick_target(&summaries, Some("d:gone")).is_none());
        assert_eq!(pick_target(&summaries, None).unwrap().id, "d:one");
        let idle = vec![
            summary("d:one", "succeeded", 10),
            summary("d:two", "waiting", 20),
        ];
        assert_eq!(pick_target(&idle, None).unwrap().id, "d:two");
        assert!(pick_target(&[], None).is_none());
    }

    /// 상태만 바뀌면 레이아웃 캐시 키는 유지한다.
    #[test]
    fn shape_key_ignores_task_state() {
        let cfg = LayoutConfig::default();
        let mut v = DagGraphView {
            data: Some(graph_data(&[
                ("a", DagStatus::Waiting),
                ("b", DagStatus::Waiting),
            ])),
            ..DagGraphView::default()
        };
        let waiting = v.shape_key(DagDirection::LeftRight, &cfg);

        for state in [
            DagStatus::Ready,
            DagStatus::Running,
            DagStatus::Succeeded,
            DagStatus::Failed,
            DagStatus::Cancelled,
            DagStatus::Skipped,
            DagStatus::Unknown,
        ] {
            v.data = Some(graph_data(&[("a", state), ("b", DagStatus::Running)]));
            assert_eq!(
                v.shape_key(DagDirection::LeftRight, &cfg),
                waiting,
                "state {state:?} 가 레이아웃 캐시를 무효화했다"
            );
        }

        v.data = Some(graph_data(&[
            ("a", DagStatus::Waiting),
            ("b", DagStatus::Waiting),
            ("c", DagStatus::Waiting),
        ]));
        assert_ne!(v.shape_key(DagDirection::LeftRight, &cfg), waiting);

        v.data = Some(graph_data(&[
            ("a", DagStatus::Waiting),
            ("b", DagStatus::Waiting),
        ]));
        assert_eq!(v.shape_key(DagDirection::LeftRight, &cfg), waiting);
        assert_ne!(v.shape_key(DagDirection::TopDown, &cfg), waiting);
        let wider = LayoutConfig {
            layer_gap: cfg.layer_gap + tasty_type_geometry::length::LogicalPx(8.0),
            ..LayoutConfig::default()
        };
        assert_ne!(v.shape_key(DagDirection::LeftRight, &wider), waiting);
    }

    /// `(id, 상태)` 목록 → 앞 노드에서 뒤 노드로 이어지는 사슬 그래프.
    fn graph_data(nodes: &[(&str, DagStatus)]) -> DagData {
        let graph = DagGraphData {
            id: "d:test".to_string(),
            name: "test".to_string(),
            nodes: nodes
                .iter()
                .map(|(id, status)| DagNodeData {
                    id: (*id).to_string(),
                    name: (*id).to_string(),
                    status: *status,
                    command_kind: "run",
                    on_failure_kind: "abort",
                    command_text: String::new(),
                    started_at: None,
                    finished_at: None,
                    exit_code: None,
                    error_tail: None,
                    output_tail: None,
                    incoming: Vec::new(),
                })
                .collect(),
            edges: (1..nodes.len())
                .map(|i| DagEdgeData {
                    from: i - 1,
                    to: i,
                    relation: DagRelation::DependsOn,
                })
                .collect(),
            cycle: None,
            done: 0,
        };
        DagData {
            workspace_id: 1,
            dags: Vec::new(),
            current: Some(graph),
            runner: RunnerBadgeData::default(),
            target_missing: false,
        }
    }

    fn summary(id: &str, rollup: &'static str, updated_at: u64) -> tasty_agent::DagSummary {
        tasty_agent::DagSummary {
            id: id.to_string(),
            workspace_id: 1,
            name: id.to_string(),
            source: "explicit",
            task_count: 0,
            state_counts: Default::default(),
            rollup_state: rollup,
            created_at: 0,
            updated_at,
            root_task_ids: Vec::new(),
            has_cycle: false,
            task_ids: Vec::new(),
        }
    }
}
