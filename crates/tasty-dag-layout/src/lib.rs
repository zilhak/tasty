//! Task DAG 의 레이어 레이아웃 — "어느 노드를 어디에 놓을지" 만 계산한다.
//!
//! 입력은 노드 id 목록 + 의존 엣지 목록이고 출력은 좌표다. 이 crate 는 아무것도
//! 그리지 않고 `egui` 를 모르며 `Theme` 를 보지 않는다 — 치수는 전부 호출자가
//! [`LayoutConfig`] 로 주입한다.
//!
//! # 왜 별도 crate 인가
//!
//! - 좌표 계산은 시각화 전용 관심사라 "IPC/GUI 와 독립" 을 원칙으로 삼는
//!   `tasty-agent` 에 넣을 수 없다(헤드리스 빌드에 `petgraph` 가 딸려 들어간다).
//! - `rust-sugiyama` / `petgraph` 의존이 이 crate 하나에 갇힌다.
//! - UI 없이 유닛테스트가 되고, DAG surface·popup·갤러리 specimen 이 같은 함수를
//!   공유한다.
//!
//! # 어댑터 경계
//!
//! 외부 레이아웃 라이브러리 호출은 [`engine`] 모듈 **안에서만** 일어난다.
//! 라이브러리를 자체 구현으로 갈아끼워도 공개 API 는 그대로다.
//!
//! # 좌표계
//!
//! 출력은 전부 [`LogicalPx`] 이고 원점은 좌상단 `(0, 0)`, y 는 아래로 증가한다
//! (egui 화면 좌표와 같은 방향). [`NodePosition::x`] / [`NodePosition::y`] 는 노드
//! 사각형의 **좌상단 모서리**이고 크기는 [`LayoutConfig::node_size`] 다.
//!
//! # 위치 안정성 불변식
//!
//! 입력은 **id + 의존 엣지 + config** 뿐이다. task 상태·duration·진행률 같은 값은
//! 절대 인자로 받지 않는다 — 0.5 초 폴링에서 상태가 바뀔 때마다 노드가 움직이면
//! 그래프를 읽을 수 없기 때문이다. 상태 전이는 카드 한 장을 다시 칠할 뿐
//! 레이아웃을 바꾸지 않는다. 같은 `(node_ids, edges, cfg)` 는 언제나 같은 좌표를
//! 낸다(결정적).

mod engine;
mod routing;
#[cfg(test)]
mod tests;

use tasty_type_geometry::length::LogicalPx;

/// 레이어가 뻗어나가는 방향.
///
/// 내부 계산은 화면 축이 아니라 "레이어 축 / 형제 축" 이라는 추상 축으로 하고,
/// 마지막에 화면 좌표로 옮길 때만 두 축을 x/y 중 어디에 붙일지 정한다. 그래서
/// 방향 토글은 알고리즘을 두 벌 만들지 않고 축 교환 한 번으로 끝난다.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Orientation {
    /// 레이어가 왼쪽 → 오른쪽으로 진행하고 형제는 세로로 쌓인다.
    ///
    /// 기본값이다. `agent.task_graph --format dot` 이 `rankdir=LR` 을 내보내 CLI
    /// 출력과 멘탈 모델이 일치하고, 노드 카드가 가로로 긴 형태(168×48)라 LR 이
    /// 화면 폭을 아낀다.
    #[default]
    LeftRight,
    /// 레이어가 위 → 아래로 진행하고 형제는 가로로 늘어선다.
    TopDown,
}

/// 레이아웃 파라미터. 전부 [`LogicalPx`] — raw `f32` 를 노출하지 않는다.
///
/// 이 crate 는 `Theme` 를 의존하지 않는다(레이어 위반 방지). 기본값이 디자인
/// 확정치와 같으므로, 토큰을 가진 호출자는 `component.dag-*` 값을 그대로
/// 덮어쓰면 된다.
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
    /// 서로 이어지지 않은 조각(고립 노드 포함) 사이의 간격.
    ///
    /// 대응하는 디자인 토큰이 없다 — 시안은 연결된 DAG 한 개만 그렸다. 토큰이
    /// 생기면 호출자가 주입하면 된다.
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

/// 엣지 한 개의 라우팅 경로.
///
/// `points` 는 시작점 → (꺾임점들) → 끝점 순서의 폴리라인이다. 인접 레이어를 잇는
/// 엣지는 점 2 개, 레이어를 건너뛰는 엣지는 건너뛴 레이어마다 꺾임점이 하나씩
/// 붙어 3 개 이상이 된다. 시작점/끝점은 노드 사각형의 **변 위**(레이어 축 방향으로
/// 마주보는 변의 중점)라 카드 안쪽으로 파고들지 않는다.
///
/// 직교(orthogonal) 세그먼트와 elbow 반경, 화살촉은 **렌더가** 이 폴리라인을 펴서
/// 만든다. 이 crate 는 꺾임점 좌표까지만 책임진다 — 그 좌표는 레이어 배치 결과를
/// 알아야만 구할 수 있어 렌더 쪽에서 재현할 수 없기 때문이다.
///
/// 꺾임점은 항상 그 레이어의 실노드 카드를 피해 빈 통로에 놓인다.
#[derive(Debug, Clone, PartialEq)]
pub struct EdgeRoute {
    /// 입력 `node_ids` 인덱스 (출발).
    pub from: usize,
    /// 입력 `node_ids` 인덱스 (도착).
    pub to: usize,
    /// 시작점 → 꺾임점들 → 끝점. 항상 2 개 이상이다.
    pub points: Vec<(LogicalPx, LogicalPx)>,
    /// 레이어 진행 방향을 거스르는 엣지인지(`layer[to] <= layer[from]`).
    ///
    /// 사이클이 있는 그래프에서만 `true` 가 된다. 렌더가 되돌아가는 엣지를 다르게
    /// 칠하거나 사이클 배너와 연결하는 데 쓴다.
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
    /// 입력 그래프에 사이클(자기 참조 포함)이 있었는지.
    ///
    /// 사이클이어도 레이아웃은 정상적으로 나온다 — 되돌아가는 엣지를 임시로
    /// 뒤집어 레이어를 매기기 때문이다. 이 플래그는 렌더가 사이클 배너를 띄울지
    /// 판단하는 용도다.
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
