//! surface 별 뷰 상태 + 폴링 게이트 + 레이아웃 캐시.
//!
//! `DagGraphSurface`(모델)는 "어떤 DAG 를 어느 방향으로 보는가" 만 들고 있고,
//! 여기 있는 것들은 전부 **휘발성**이다 — 재시작하면 auto-fit 부터 다시 시작한다.
//!
//! # 폴링
//!
//! runner 는 별도 스레드(500ms tick)라 그 상태 변화가 egui 렌더 루프를 깨우지
//! 않는다. 그래서 캔버스는 보이는 동안 스스로 `request_repaint_after` 를 걸고,
//! 그 프레임에서 [`DagGraphViewStore::poll`] 이 memory store 를 다시 읽는다.
//! **보이지 않으면 아무것도 예약하지 않는다** — 안 보이는 탭이 유휴 CPU 를 태우지
//! 않게 하려는 것이다.
//!
//! # 레이아웃 캐시
//!
//! 캐시 키는 **`(노드 id 나열, 엣지 나열, 방향, 치수)`** 다. `TaskState` 는 키에
//! 들어가지 않는다 — 상태만 바뀌었는데 좌표를 다시 계산하면 0.5 초마다 노드가
//! 미세하게 튄다. 노드/엣지가 실제로 추가·삭제될 때만 재배치한다.

use std::collections::HashMap;
use std::hash::{Hash as _, Hasher as _};
use std::time::{Duration, Instant};

use tasty_dag_layout::{GraphLayout, LayoutConfig, Orientation, layout_dag};
use tasty_model::{DagDirection, DagGraphSurface, SurfaceId};
use tasty_type_appearance::theme::Theme;

use super::model::{DagData, DagListEntry, DagStatus, RunnerBadgeData, build_graph};

/// 폴링 주기. runner tick(500ms)과 맞춘다 — 더 자주 읽어봐야 새 값이 없다.
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

/// 그래프 화면이 실제로 필요로 하는 **관찰 대상**.
///
/// surface 는 `DagGraphSurface` 의 필드를, workspace popup 은 자기 dialog 상태를
/// 빌려준다 — 렌더 코드는 자기가 탭 안인지 팝업 안인지 몰라야 두 경로가 같은
/// 그림을 그린다. 대상 workspace 는 여기 없다: 렌더는 이미 읽어 둔
/// [`DagGraphView::data`] 만 그리고, workspace 는 그 데이터를 **채울 때**만
/// 필요하다([`DagGraphView::poll_if_stale`]).
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

/// 캐시된 레이아웃 한 벌.
///
/// `Rc` 로 감싸는 이유는 소유권 한 가지뿐이다 — 렌더는 레이아웃을 읽으면서 동시에
/// `&mut DagGraphView`(줌/팬/선택 갱신)를 잡아야 하는데, 캐시에서 참조를 빌려 오면
/// 그 둘이 겹친다. 500 노드짜리 좌표를 프레임마다 복사하지 않으려면 값 복제가
/// 아니라 참조계수 복제여야 한다.
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
    /// `(dag id, 방향, 뷰포트 버킷)` — auto-fit 이 이미 돈 조합. 폴링으로는 절대
    /// 바뀌지 않는 값들로만 구성해, 데이터 갱신이 프레이밍을 건드리지 못하게 한다.
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

    /// 다음 폴링까지 남은 시간 — 캔버스가 `request_repaint_after` 에 그대로 쓴다.
    pub fn until_next_poll(&self) -> Duration {
        self.last_poll
            .map(|t| POLL_INTERVAL.saturating_sub(t.elapsed()))
            .unwrap_or(Duration::ZERO)
    }

    /// 폴링 게이트를 지나면 memory store 를 다시 읽는다.
    ///
    /// surface 는 [`DagGraphViewStore::poll`] 을 거쳐, popup 은 자기 draw_fn 에서
    /// 직접 부른다(popup 은 `engine` 을 통째로 받으므로 렌더 루프의 재차입 제약이
    /// 없다). 주기·실패 처리를 한곳에 두어 두 경로가 갈라지지 않게 한다.
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
                // 데이터는 마지막으로 성공한 것을 그대로 둔다 — 일시적 실패로
                // 그래프가 사라졌다 나타나면 읽는 사람이 더 혼란스럽다.
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
        // 위에서 반드시 채웠다.
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

    /// 레이아웃 캐시 키. **상태를 넣지 않는다** — 이 파일 상단의 불변식.
    fn shape_key(&self, direction: DagDirection, cfg: &LayoutConfig) -> u64 {
        let mut h = std::collections::hash_map::DefaultHasher::new();
        direction.as_str().hash(&mut h);
        // 치수가 바뀌면(테마 zoom 등) 좌표도 바뀐다.
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

    /// auto-fit 을 이번 조합에서 아직 안 돌렸으면 `true` 를 돌려주고 표시해 둔다.
    ///
    /// 뷰포트는 32px 버킷으로 뭉갠다 — 1px 흔들림마다 다시 fit 하면 리사이즈 중
    /// 화면이 요동친다. 폴링은 키의 어느 성분도 건드리지 않으므로 **데이터 갱신은
    /// 절대 fit 을 발화시키지 않는다**.
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
}

impl DagGraphViewStore {
    pub fn get_or_init(&mut self, surface_id: SurfaceId) -> &mut DagGraphView {
        self.views.entry(surface_id).or_default()
    }

    /// surface 가 닫힐 때 호출 — 뷰 상태를 버린다.
    pub fn drop_view(&mut self, surface_id: SurfaceId) {
        self.views.remove(&surface_id);
    }

    /// 이번 프레임에 보이는 DAG surface 들의 데이터를 필요하면 새로 읽는다.
    ///
    /// 렌더 루프 **진입 전에** 호출한다 — 루프 안에서는 `engine` 이 workspace/pane/
    /// tab 에 배타 차용돼 store 를 읽을 수 없다(explorer 의 outbox 패턴과 같은 제약).
    pub fn poll(&mut self, engine: &crate::core::CoreState, requests: &[DagPollRequest]) {
        for req in requests {
            self.views.entry(req.surface_id).or_default().poll_if_stale(
                engine,
                req.workspace_id,
                req.dag_id.as_deref(),
            );
        }
        // 이번 프레임에 보이지 않은 surface 의 뷰는 남겨 둔다(줌/선택 유지). 정리는
        // surface 종료 시 `drop_view` 가 한다.
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
        // 사이클은 검출만 — 그래프는 그대로 그린다(레이아웃 엔진이 FAS 로 역엣지를
        // 걷어내고 배치한다). 판정 자체는 `group_tasks_into_dags` 가 이미 했으니
        // 여기서는 배너 문구가 필요한 경우에만 한 번 더 돌려 메시지를 얻는다.
        // `UnknownDependency`(그룹 밖 참조)는 사이클이 아니므로 배너를 띄우지 않는다.
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

/// 어떤 DAG 를 그릴지 고른다.
///
/// 명시 지정이 있으면 그것뿐이다 — 없는 id 를 다른 DAG 로 슬쩍 대체하지 않는다
/// (사용자가 고른 대상이 사라졌다는 사실 자체가 화면에 드러나야 한다).
/// 미지정이면 진행 중인 그래프를 먼저, 그다음 가장 최근에 갱신된 것을 고른다.
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
        // 앵커 아래의 그래프 좌표는 줌 전후로 같아야 한다.
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
        // 1px 흔들림은 같은 버킷 — 다시 fit 하지 않는다.
        assert!(!v.take_fit("d:a", DagDirection::LeftRight, vp + egui::vec2(1.0, 0.0)));
        // 방향 전환은 새 프레이밍.
        assert!(v.take_fit("d:a", DagDirection::TopDown, vp));
        // 대상 DAG 전환도.
        assert!(v.take_fit("d:b", DagDirection::TopDown, vp));
        // 큰 리사이즈도.
        assert!(v.take_fit("d:b", DagDirection::TopDown, vp + egui::vec2(200.0, 0.0)));
    }

    #[test]
    fn explicit_target_is_never_silently_substituted() {
        let summaries = vec![
            summary("d:one", "running", 10),
            summary("d:two", "waiting", 20),
        ];
        assert_eq!(pick_target(&summaries, Some("d:two")).unwrap().id, "d:two");
        // 없는 id 는 대체하지 않는다.
        assert!(pick_target(&summaries, Some("d:gone")).is_none());
        // 미지정이면 running 우선.
        assert_eq!(pick_target(&summaries, None).unwrap().id, "d:one");
        // running 이 없으면 최신 갱신.
        let idle = vec![
            summary("d:one", "succeeded", 10),
            summary("d:two", "waiting", 20),
        ];
        assert_eq!(pick_target(&idle, None).unwrap().id, "d:two");
        assert!(pick_target(&[], None).is_none());
    }

    /// **이 기능의 핵심 불변식.** 캐시 키에 `TaskState` 가 들어가면 0.5 초 폴링에서
    /// 상태가 바뀔 때마다 좌표가 다시 계산돼 노드가 미세하게 튄다. 노드/엣지가 그대로면
    /// 어떤 상태 조합이 와도 같은 키여야 한다.
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

        // 같은 노드/엣지, 상태만 8 종을 오간다.
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

        // 반대로 **모양**이 바뀌면 반드시 달라져야 한다 — 노드 추가.
        v.data = Some(graph_data(&[
            ("a", DagStatus::Waiting),
            ("b", DagStatus::Waiting),
            ("c", DagStatus::Waiting),
        ]));
        assert_ne!(v.shape_key(DagDirection::LeftRight, &cfg), waiting);

        // 방향과 치수도 좌표를 바꾸므로 키의 성분이다.
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
