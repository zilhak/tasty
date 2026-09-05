//! 창 소유 자원의 `list` 가 단일 engine 만 보지 않고 모든 main + parked engine 을
//! 순회해 결과를 합치도록 호스트 레벨에서 special-case 처리한다.
//! CLAUDE.md "list 명령은 전체 워크스페이스를 순회" 원칙.
//!
//! **여기 없는 list 는 포커스된 창의 것만 답한다** — 에러 없이. 실측(2026-09-05, 창 둘):
//! 창1 에서 만든 headless pty 가 창2 포커스에서 `pty.list` 에 안 나오는데
//! `pty.read {id}` 는 그 pty 를 읽었다. **조작할 수 있는데 볼 수 없는** 상태이고,
//! 답이 틀렸다는 신호가 없다. 창 소유 자원의 list 를 새로 만들면 여기에 등록한다.
//!
//! 합산이 옳으려면 **id 가 engine 을 건너 유일해야 한다**(`IdGenerator`) — 안 그러면
//! 합친 목록에 같은 id 가 둘 들어가 호출자가 어느 쪽도 지목할 수 없다.

use serde_json::json;

use crate::app::App;
use crate::ipc as host_ipc;
use crate::ipc::handler::{output, pane, pty, surface, workspace};
use crate::ipc::protocol::JsonRpcResponse;

impl App {
    /// list 류 메서드면 모든 engine 결과를 합쳐 반환. 그 외는 None 반환해
    /// caller 가 일반 routing 계속.
    pub(crate) fn dispatch_list_global(
        &mut self,
        request: &host_ipc::protocol::JsonRpcRequest,
    ) -> Option<JsonRpcResponse> {
        let id = request.id.clone().unwrap_or(serde_json::Value::Null);
        match request.method.as_str() {
            "workspace.list" => Some(self.collect_list(id, |_c, s, e, id| {
                workspace::handle_workspace_list(s, e, id)
            })),
            "surface.list" => {
                Some(self.collect_list(id, |_c, s, e, id| surface::handle_surface_list(s, e, id)))
            }
            "pane.list" => {
                Some(self.collect_list(id, |_c, s, e, id| pane::handle_pane_list(s, e, id)))
            }
            // 아래 둘은 결과가 맨 배열이 아니라 **이름 붙은 배열**이라 합산 함수가
            // 필드를 알아야 한다. 막힌 것은 함수의 거처가 아니라 결과 모양이었다.
            // 아래 둘은 결과가 맨 배열이 아니라 **이름 붙은 배열**이라 합산 함수가
            // 필드를 알아야 한다. 막힌 것은 함수의 거처가 아니라 결과 모양이었다.
            "pty.list" => {
                Some(self.collect_field(id, "ptys", |_c, _s, e, id| pty::handle_list(e, id)))
            }
            "output.observe_list" => Some(self.collect_field(id, "observers", |c, s, e, id| {
                output::handle_observe_list(c, s, e, id)
            })),
            _ => None,
        }
    }

    /// 결과가 **맨 배열**인 list 를 합친다.
    fn collect_list<F>(&mut self, id: serde_json::Value, f: F) -> JsonRpcResponse
    where
        F: FnMut(
            &crate::core::Core,
            &mut crate::state::AppState,
            &mut crate::core::CoreState,
            serde_json::Value,
        ) -> JsonRpcResponse,
    {
        let merged = self.merge(&id, f, None);
        JsonRpcResponse::success(id, json!(merged))
    }

    /// 결과가 `{ "<field>": [...] }` 인 list 를 합쳐 같은 모양으로 되돌린다.
    fn collect_field<F>(&mut self, id: serde_json::Value, field: &str, f: F) -> JsonRpcResponse
    where
        F: FnMut(
            &crate::core::Core,
            &mut crate::state::AppState,
            &mut crate::core::CoreState,
            serde_json::Value,
        ) -> JsonRpcResponse,
    {
        let merged = self.merge(&id, f, Some(field));
        JsonRpcResponse::success(id, json!({ field: merged }))
    }

    /// 모든 main + parked engine 을 돌며 배열을 잇는다. `field` 가 있으면 결과 객체의
    /// 그 필드에서, 없으면 결과 자체에서 배열을 꺼낸다.
    fn merge<F>(
        &mut self,
        id: &serde_json::Value,
        mut f: F,
        field: Option<&str>,
    ) -> Vec<serde_json::Value>
    where
        F: FnMut(
            &crate::core::Core,
            &mut crate::state::AppState,
            &mut crate::core::CoreState,
            serde_json::Value,
        ) -> JsonRpcResponse,
    {
        let take = |resp: JsonRpcResponse, out: &mut Vec<serde_json::Value>| {
            let result = resp.result;
            let arr = match field {
                Some(k) => result.as_ref().and_then(|v| v.get(k)),
                None => result.as_ref(),
            }
            .and_then(|v| v.as_array());
            if let Some(arr) = arr {
                out.extend(arr.iter().cloned());
            }
        };
        // `pty.list` 는 목록을 만들기 전에 idle/종료분을 걷어내므로 engine 이 `&mut` 다.
        // 그래서 필드를 쪼개 빌린다 — `&mut self.view` 와 `&self.core` 가 겹치지 않는다.
        let Self {
            view,
            parked_states,
            core,
            ..
        } = self;
        let mut combined: Vec<serde_json::Value> = Vec::new();
        for w in view.views.values_mut() {
            if let Some(m) = w.as_main_mut() {
                take(
                    f(core, &mut m.state, &mut m.core_state, id.clone()),
                    &mut combined,
                );
            }
        }
        for (s, e) in parked_states.iter_mut() {
            take(f(core, s, e, id.clone()), &mut combined);
        }
        combined
    }
}
