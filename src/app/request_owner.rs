//! IPC request 에서 대상 리소스 id 를 추출해 owner MainWindow 를 찾는다.
//!
//! CLAUDE.md "포커스 독립" 원칙: 모든 명령은 대상 리소스를 ID 로 직접 지정한다.
//! request.params 의 `surface_id` / `workspace_id` / `pane_id` 가 명시되면 그
//! 리소스를 가진 main 으로 routing — focus 와 무관.

use winit::window::WindowId;

use crate::app::App;

#[derive(Debug, Clone, Copy)]
pub(crate) enum Kind {
    Surface,
    Workspace,
    Pane,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct ResourceId {
    pub kind: Kind,
    pub id: u32,
}

/// params 에서 resource id 추출. surface_id 우선 — 같은 request 에 여러 ID 가
/// 있을 경우 surface 가 가장 구체적인 대상.
pub(crate) fn params_resource_id(params: &serde_json::Value) -> Option<(&str, ResourceId)> {
    for key in ["surface_id", "pane_id", "workspace_id"] {
        if let Some(v) = params.get(key).and_then(|v| v.as_u64()) {
            let kind = match key {
                "surface_id" => Kind::Surface,
                "pane_id" => Kind::Pane,
                "workspace_id" => Kind::Workspace,
                _ => unreachable!(),
            };
            return Some((key, ResourceId { kind, id: v as u32 }));
        }
    }
    None
}

impl App {
    /// request.params 에 resource id 가 있으면 그 리소스를 가진 MainWindow 의 id 반환.
    pub(crate) fn find_request_owner(&self, params: &serde_json::Value) -> Option<WindowId> {
        let (_, rid) = params_resource_id(params)?;
        match rid.kind {
            Kind::Surface => self.find_main_with_surface(rid.id),
            Kind::Workspace => self.find_main_with_workspace(rid.id),
            Kind::Pane => self.find_main_with_pane(rid.id),
        }
    }
}
