//! DAG 식별자·workspace·표시 방향을 보관하는 surface. 레이아웃 캐시·줌·선택은 호스트 뷰가 맡는다.
//! snapshot 변환도 호스트가 처리한다. 그래프가 달라질 수 있어 줌·팬·선택은 복원하지 않고 auto-fit한다.

use std::path::PathBuf;

use super::SurfaceId;
use super::surface_trait::Surface;

/// 레이어가 뻗어나가는 방향. 캔버스 크롬의 방향 토글이 바꾼다.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum DagDirection {
    /// 레이어는 왼쪽에서 오른쪽으로, 같은 레이어의 노드는 세로로 배치한다. 기본 방향이다.
    #[default]
    LeftRight,
    /// 레이어가 위 → 아래. 형제는 가로로 늘어선다.
    TopDown,
}

impl DagDirection {
    /// snapshot 직렬화용 안정 식별자.
    pub fn as_str(self) -> &'static str {
        match self {
            DagDirection::LeftRight => "lr",
            DagDirection::TopDown => "td",
        }
    }

    /// 식별자 → 방향. 알 수 없으면 기본값(`LeftRight`).
    // 알 수 없는 값도 기본값을 반환하므로 실패 가능한 FromStr 대신 별도 API를 쓴다.
    #[allow(clippy::should_implement_trait)]
    pub fn from_str(s: &str) -> Self {
        match s {
            "td" => DagDirection::TopDown,
            _ => DagDirection::LeftRight,
        }
    }

    /// 토글 결과.
    pub fn toggled(self) -> Self {
        match self {
            DagDirection::LeftRight => DagDirection::TopDown,
            DagDirection::TopDown => DagDirection::LeftRight,
        }
    }
}

/// Task DAG 관찰 surface.
pub struct DagGraphSurface {
    pub id: SurfaceId,
    /// 관찰 대상 DAG 의 식별자(`agent.dag_list` 의 `DagSummary::id` — `d:<키>` 또는
    /// `c:<root task id>`).
    ///
    /// `None` 이면 host view 가 workspace 안에서 자동 선택한다(진행 중인 DAG 우선,
    /// 없으면 가장 최근 갱신). 사용자가 헤더 드롭다운으로 고르면 `Some` 이 되어
    /// 그때부터 고정된다 — 폴링이 대상을 바꿔치기하지 않는다.
    pub dag_id: Option<String>,
    /// 조회할 workspace. None이면 현재 활성 workspace가 아니라 이 surface의 소속 workspace다.
    pub workspace_id: Option<u32>,
    /// 레이어 진행 방향. 토글 결과가 레이아웃 영속에 실린다.
    pub direction: DagDirection,
}

impl DagGraphSurface {
    pub fn new(id: SurfaceId) -> Self {
        Self {
            id,
            dag_id: None,
            workspace_id: None,
            direction: DagDirection::default(),
        }
    }

    /// 관찰 대상을 지정한 생성자 (IPC/CLI params · snapshot 복원 공용).
    pub fn with_target(
        id: SurfaceId,
        dag_id: Option<String>,
        workspace_id: Option<u32>,
        direction: DagDirection,
    ) -> Self {
        Self {
            id,
            dag_id,
            workspace_id,
            direction,
        }
    }
}

impl Surface for DagGraphSurface {
    crate::impl_surface_any!();

    fn kind(&self) -> &'static str {
        "dag_graph"
    }
    fn type_name(&self) -> &'static str {
        "DAG"
    }
    fn surface_id(&self) -> Option<SurfaceId> {
        Some(self.id)
    }

    fn display_name(&self) -> String {
        // 사용자 DAG 키는 제목으로 쓰고 자동 생성 ID는 기본 표시명으로 대신한다.
        match self.dag_id.as_deref().and_then(|id| id.strip_prefix("d:")) {
            Some(key) if !key.is_empty() => key.to_string(),
            _ => self.type_name().to_string(),
        }
    }

    fn source_cwd(&self) -> Option<PathBuf> {
        // 파일이나 디렉터리에 연결된 surface가 아니므로 상속할 cwd가 없다.
        None
    }

    fn to_tree_json(&self) -> serde_json::Value {
        let mut obj = serde_json::json!({
            "kind": self.kind(),
            "type": self.type_name(),
            "id": self.id,
            "direction": self.direction.as_str(),
        });
        if let Some(dag) = &self.dag_id {
            obj["dag_id"] = serde_json::json!(dag);
        }
        if let Some(ws) = self.workspace_id {
            obj["workspace_id"] = serde_json::json!(ws);
        }
        obj
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn direction_round_trips_through_str() {
        for d in [DagDirection::LeftRight, DagDirection::TopDown] {
            assert_eq!(DagDirection::from_str(d.as_str()), d);
        }
        assert_eq!(DagDirection::from_str("diagonal"), DagDirection::LeftRight);
    }

    #[test]
    fn direction_toggle_is_an_involution() {
        for d in [DagDirection::LeftRight, DagDirection::TopDown] {
            assert_eq!(d.toggled().toggled(), d);
        }
    }

    #[test]
    fn display_name_uses_explicit_dag_key_only() {
        let mut s = DagGraphSurface::new(7);
        assert_eq!(s.display_name(), "DAG");
        s.dag_id = Some("d:build-and-deploy".to_string());
        assert_eq!(s.display_name(), "build-and-deploy");
        s.dag_id = Some("c:t-1700000000-000001".to_string());
        assert_eq!(s.display_name(), "DAG");
    }

    #[test]
    fn source_cwd_is_none() {
        assert_eq!(DagGraphSurface::new(1).source_cwd(), None);
    }
}
