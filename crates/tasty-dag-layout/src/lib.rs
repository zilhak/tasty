//! 노드 ID·의존 엣지·LayoutConfig로 DAG의 좌표를 계산한다. UI나 Theme에는 의존하지 않는다.
//! 레이아웃 라이브러리는 engine 모듈에서만 호출하며 UI·IPC와 독립적으로 시험할 수 있다.
//! tasty-agent에 넣으면 헤드리스에도 레이아웃 의존성이 포함되므로 별도 크레이트로 둔다.
//!
//! # 좌표계
//!
//! 출력은 전부 [`LogicalPx`] 이고 원점은 좌상단 `(0, 0)`, y 는 아래로 증가한다
//! (egui 화면 좌표와 같은 방향). [`NodePosition::x`] / [`NodePosition::y`] 는 노드
//! 사각형의 **좌상단 모서리**이고 크기는 [`LayoutConfig::node_size`] 다.
//!
//! # 위치 안정성
//!
//! 같은 입력은 같은 좌표를 만든다. 작업 상태·진행률은 입력에 포함하지 않아
//! 상태 갱신만으로 노드가 움직이지 않는다.

mod engine;
mod routing;
#[cfg(test)]
mod tests;

use tasty_type_geometry::length::LogicalPx;

/// 레이어 진행 방향. 내부 계산 뒤 레이어 축과 형제 축을 화면 x/y에 대응시킨다.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Orientation {
    /// 왼쪽에서 오른쪽으로 진행하고 형제는 세로로 쌓인다. CLI DOT의 rankdir=LR과 같은 기본 방향이다.
    #[default]
    LeftRight,
    /// 레이어가 위 → 아래로 진행하고 형제는 가로로 늘어선다.
    TopDown,
}

/// 호출자가 전달하는 LogicalPx 치수. Theme 의존 없이 사용하며 호출자가 토큰 값으로 덮어쓸 수 있다.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LayoutConfig {
    /// 레이어 진행 방향. 기본 [`Orientation::LeftRight`].
    pub orientation: Orientation,
    /// 노드 카드 크기 `(width, height)` — **화면 기준**이다. 방향을 바꿔도 카드가
    /// 회전하지는 않으므로 orientation 과 무관하게 고정이다.
    /// 디자인 확정: 168 × 48 (`component.dag-node-width/height`).
    pub node_size: (LogicalPx, LogicalPx),
    /// 이웃한 두 레이어 사이의 빈 간격. 디자인 확정: 32 (`component.dag-layer-gap`).
    pub layer_gap: LogicalPx,
    /// 같은 레이어 안 이웃한 두 노드 사이의 빈 간격.
    /// 디자인 확정: 24 (`component.dag-sibling-gap`).
    pub sibling_gap: LogicalPx,
    /// 서로 연결되지 않은 컴포넌트 사이의 간격. 대응하는 디자인 토큰은 없다.
    pub component_gap: LogicalPx,
}

impl Default for LayoutConfig {
    fn default() -> Self {
        Self {
            orientation: Orientation::default(),
            node_size: (LogicalPx(168.0), LogicalPx(48.0)),
            layer_gap: LogicalPx(32.0),
            sibling_gap: LogicalPx(24.0),
            component_gap: LogicalPx(48.0),
        }
    }
}

/// 노드 한 개의 배치 결과.
#[derive(Debug, Clone, PartialEq)]
pub struct NodePosition {
    /// 입력 `node_ids` 의 원소를 그대로 돌려준다.
    pub id: String,
    /// 노드 사각형 좌상단 x.
    pub x: LogicalPx,
    /// 노드 사각형 좌상단 y.
    pub y: LogicalPx,
    /// 0 부터 시작하는 레이어 번호. 방향과 무관하게 "몇 번째 단계인가" 를 뜻한다.
    ///
    /// 조각(약연결 컴포넌트)마다 0 부터 다시 센다 — 모든 조각의 레이어 0 이 같은
    /// 줄에 정렬된다.
    pub layer: u32,
}

/// 엣지의 시작점·꺾임점·끝점을 잇는 폴리라인.
/// 인접 레이어는 두 점, 건너뛴 레이어는 단계마다 꺾임점을 하나씩 추가한다.
/// 양 끝점은 카드의 마주보는 변 위에 있고 꺾임점은 실노드 카드를 피한다.
/// 직교 선분·모서리 반경·화살촉은 렌더러가 이 좌표로 그린다.
#[derive(Debug, Clone, PartialEq)]
pub struct EdgeRoute {
    /// 입력 `node_ids` 인덱스 (출발).
    pub from: usize,
    /// 입력 `node_ids` 인덱스 (도착).
    pub to: usize,
    /// 시작점 → 꺾임점들 → 끝점. 항상 2 개 이상이다.
    pub points: Vec<(LogicalPx, LogicalPx)>,
    /// 레이어 진행 방향을 거스르는지(`layer[to] <= layer[from]`). 렌더러의 역방향 엣지 표시에 쓴다.
    pub back: bool,
}

/// [`layout_dag`] 의 결과.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct GraphLayout {
    /// 입력 `node_ids` 와 **같은 길이·같은 순서**다. 즉 `nodes[i].id == node_ids[i]`
    /// 이고, dummy vertex 같은 내부 산출물은 절대 새어 나오지 않는다.
    pub nodes: Vec<NodePosition>,
    /// 라우팅된 엣지. 입력 순서를 유지하되 self-loop · 범위 밖 인덱스 · 중복은
    /// 빠진다.
    pub edges: Vec<EdgeRoute>,
    /// 노드 사각형과 엣지 폴리라인을 모두 감싸는 경계 상자의 너비.
    pub width: LogicalPx,
    /// 노드 사각형과 엣지 폴리라인을 모두 감싸는 경계 상자의 높이.
    pub height: LogicalPx,
    /// 입력에 자기 참조를 포함한 사이클이 있었는지. 렌더러가 경고 여부를 정할 수 있다.
    pub has_cycle: bool,
}

/// 노드 id 목록 + `(from, to)` 엣지 목록 → 좌표 + 엣지 폴리라인.
///
/// `edges` 의 원소는 `node_ids` 의 **인덱스** 쌍이며 `(의존 대상, 의존하는 쪽)`
/// 방향, 즉 레이어가 증가하는 방향이다.
///
/// 반환은 항상 **단일 레이아웃**이다 — DAG 분해는 호출 측 모델이 이미 끝냈다고
/// 보고 여기서는 "DAG 하나" 를 통째로 배치한다. 그래도 고립 노드처럼 약연결이
/// 끊긴 조각이 섞일 수 있으므로, 조각별로 배치한 뒤 형제 축 방향으로 나란히 붙인다.
///
/// # 방어 동작
///
/// - 노드 0 개 → 빈 [`GraphLayout`].
/// - self-loop(`(i, i)`) · 범위 밖 인덱스 · 중복 엣지 → 그 엣지만 조용히 버린다.
///   (self-loop 는 사이클로는 계산되어 [`GraphLayout::has_cycle`] 에 반영된다.)
/// - 사이클 → 되돌아가는 엣지를 뒤집어 **정상 레이어 배치를 그대로 산출**한다.
///   격자 폴백으로 떨어지지 않는다.
/// - 레이아웃 엔진이 패닉하거나 신뢰할 수 없는 결과를 내면 격자 폴백으로
///   떨어진다(패닉이 호출자 = GUI 프레임으로 새지 않는다).
#[must_use]
pub fn layout_dag(
    node_ids: &[String],
    edges: &[(usize, usize)],
    cfg: &LayoutConfig,
) -> GraphLayout {
    if node_ids.is_empty() {
        return GraphLayout::default();
    }
    engine::run(node_ids, edges, cfg)
}
