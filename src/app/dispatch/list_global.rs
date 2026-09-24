//! 창에 속한 목록을 모든 MainView·parked engine에서 모은다.
//! 새 목록도 이 경로를 사용해야 포커스 창에 따라 결과가 빠지지 않는다.
//! 엔진이 같은 ID 발급기를 사용해 항목 ID가 중복되지 않게 한다.

use serde_json::json;

use crate::app::App;
use crate::ipc as host_ipc;
use crate::ipc::handler::{
    attach, hooks, image, notification, output, pane, pty, surface, workspace, workspace_category,
};
use crate::ipc::protocol::JsonRpcResponse;

impl App {
    pub(crate) fn dispatch_list_global(
        &mut self,
        request: &host_ipc::protocol::JsonRpcRequest,
    ) -> Option<JsonRpcResponse> {
        let id = request.id.clone().unwrap_or(serde_json::Value::Null);
        match request.method.as_str() {
            "notification.list" => {
                // A row outside its engine's newest 50 cannot be in the global top 50.
                let rows = one(self.merge_fields(
                    &id,
                    |_c, _s, e, id| notification::handle_notification_list(e, id),
                    &[],
                ));
                Some(JsonRpcResponse::success(
                    id,
                    json!(notification::latest_notifications(rows)),
                ))
            }
            "workspace.list" => Some(self.collect_list(id, |_c, s, e, id| {
                workspace::handle_workspace_list(s, e, id)
            })),
            "surface.list" => {
                Some(self.collect_list(id, |_c, _s, e, id| surface::handle_surface_list(e, id)))
            }
            "pane.list" => {
                Some(self.collect_list(id, |_c, _s, e, id| pane::handle_pane_list(e, id)))
            }
            "tree" => Some(self.collect_list(id, |_c, s, e, id| {
                JsonRpcResponse::success(id, json!(host_ipc::handler::build_engine_tree(s, e)))
            })),
            "pty.list" => {
                Some(self.collect_field(id, "ptys", |_c, _s, e, id| pty::handle_list(e, id)))
            }
            "output.observe_list" => Some(self.collect_field(id, "observers", |c, _s, e, id| {
                output::handle_observe_list(c, e, id)
            })),
            // image 플러그인이 host.call로 되돌린 요청도 여기서 전체 창의 결과를 모은다.
            "image.list" => {
                Some(self.collect_field(id, "entries", |_c, _s, e, id| image::handle_list(e, id)))
            }
            "workspace_category.list" => Some(self.collect_categories(id)),
            // hook.list의 surface_id는 소유 창 지정이 아닌 필터이므로 각 engine에 그대로 전달한다.
            "hook.list" => {
                let params = request.params.clone();
                Some(self.collect_list(id, move |_c, _s, e, id| {
                    hooks::handle_hook_list(e, id, &params)
                }))
            }
            "global_hook.list" => {
                Some(self.collect_list(id, |_c, _s, e, id| hooks::handle_global_hook_list(e, id)))
            }
            "attach.list" => Some(self.collect_fields(
                id,
                ("attached", "workspaces"),
                |_c, _s, e, id| attach::handle_list(e, id),
            )),
            _ => None,
        }
    }

    /// 각 engine의 예약 normal(id 0)만 한 행으로 합친다. 나머지는 공유 ID로 구별한다.
    fn collect_categories(&mut self, id: serde_json::Value) -> JsonRpcResponse {
        let rows = self.merge_fields(
            &id,
            |_c, _s, e, id| workspace_category::handle_list(e, id),
            &[],
        );
        let rows = one(rows);
        JsonRpcResponse::success(id, json!(fold_normal(rows)))
    }

    /// 배열 응답을 합친다.
    fn collect_list<F>(&mut self, id: serde_json::Value, f: F) -> JsonRpcResponse
    where
        F: FnMut(
            &crate::core::Core,
            &mut crate::state::AppState,
            &mut crate::core::CoreState,
            serde_json::Value,
        ) -> JsonRpcResponse,
    {
        let merged = one(self.merge_fields(&id, f, &[]));
        JsonRpcResponse::success(id, json!(merged))
    }

    /// 객체 안의 같은 이름 배열을 합친다.
    fn collect_field<F>(&mut self, id: serde_json::Value, field: &str, f: F) -> JsonRpcResponse
    where
        F: FnMut(
            &crate::core::Core,
            &mut crate::state::AppState,
            &mut crate::core::CoreState,
            serde_json::Value,
        ) -> JsonRpcResponse,
    {
        let merged = one(self.merge_fields(&id, f, &[field]));
        JsonRpcResponse::success(id, json!({ field: merged }))
    }

    /// 한 engine에서 핸들러를 한 번만 호출해 두 배열을 같은 응답에서 꺼낸다.
    fn collect_fields<F>(
        &mut self,
        id: serde_json::Value,
        fields: (&str, &str),
        f: F,
    ) -> JsonRpcResponse
    where
        F: FnMut(
            &crate::core::Core,
            &mut crate::state::AppState,
            &mut crate::core::CoreState,
            serde_json::Value,
        ) -> JsonRpcResponse,
    {
        let (a, b) = fields;
        let mut merged = self.merge_fields(&id, f, &[a, b]);
        let second = merged.pop().unwrap_or_default();
        let first = merged.pop().unwrap_or_default();
        JsonRpcResponse::success(id, json!({ a: first, b: second }))
    }

    /// engine마다 한 번 호출해 결과를 합친다. fields가 비면 응답 자체가 배열이다.
    /// 필드별로 따로 호출하면 서로 다른 시점의 응답이 섞일 수 있다.
    fn merge_fields<F>(
        &mut self,
        id: &serde_json::Value,
        mut f: F,
        fields: &[&str],
    ) -> Vec<Vec<serde_json::Value>>
    where
        F: FnMut(
            &crate::core::Core,
            &mut crate::state::AppState,
            &mut crate::core::CoreState,
            serde_json::Value,
        ) -> JsonRpcResponse,
    {
        let take = |resp: JsonRpcResponse, out: &mut Vec<Vec<serde_json::Value>>| {
            let result = resp.result;
            for (i, slot) in out.iter_mut().enumerate() {
                let arr = match fields.get(i) {
                    Some(k) => result.as_ref().and_then(|v| v.get(k)),
                    None => result.as_ref(),
                }
                .and_then(|v| v.as_array());
                if let Some(arr) = arr {
                    slot.extend(arr.iter().cloned());
                }
            }
        };
        // pty.list는 종료한 PTY도 정리하므로 engine을 가변으로 빌린다.
        let Self {
            view,
            parked_states,
            core,
            ..
        } = self;
        let mut combined: Vec<Vec<serde_json::Value>> = vec![Vec::new(); fields.len().max(1)];
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

fn one(mut buckets: Vec<Vec<serde_json::Value>>) -> Vec<serde_json::Value> {
    buckets.pop().unwrap_or_default()
}

/// normal의 workspace 수는 합하고 모든 engine에서 접혔을 때만 collapsed를 true로 둔다.
/// 다른 행의 index는 각 engine 안의 위치라 합친 결과에서 중복될 수 있다.
fn fold_normal(rows: Vec<serde_json::Value>) -> Vec<serde_json::Value> {
    let mut count: u64 = 0;
    let mut collapsed = true;
    let mut seen_normal = false;
    let mut rest: Vec<serde_json::Value> = Vec::new();
    let mut name = "normal".to_string();
    for row in rows {
        if row.get("is_normal").and_then(|v| v.as_bool()) != Some(true) {
            rest.push(row);
            continue;
        }
        seen_normal = true;
        count += row
            .get("workspace_count")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);
        collapsed &= row
            .get("collapsed")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        if let Some(n) = row.get("name").and_then(|v| v.as_str()) {
            name = n.to_string();
        }
    }
    if !seen_normal {
        return rest;
    }
    let mut out = Vec::with_capacity(rest.len() + 1);
    out.push(json!({
        "id": 0,
        "name": name,
        "index": 0,
        "collapsed": collapsed,
        "is_normal": true,
        "workspace_count": count,
    }));
    out.extend(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::fold_normal;
    use serde_json::json;

    fn cat(id: u32, is_normal: bool, count: u64, collapsed: bool) -> serde_json::Value {
        json!({
            "id": id,
            "name": if is_normal { "normal" } else { "work" },
            "index": 0,
            "collapsed": collapsed,
            "is_normal": is_normal,
            "workspace_count": count,
        })
    }

    #[test]
    fn normal_folds_into_one_row_carrying_the_summed_count() {
        let rows = vec![
            cat(0, true, 2, false),
            cat(1, false, 1, false),
            cat(0, true, 3, false),
            cat(2, false, 0, false),
        ];
        let out = fold_normal(rows);
        assert_eq!(
            out.len(),
            3,
            "normal 항목이 하나로 합쳐지지 않았다: {out:?}"
        );
        assert_eq!(out[0]["is_normal"], json!(true), "normal 이 맨 앞이 아니다");
        assert_eq!(out[0]["workspace_count"], json!(5));
        assert_eq!(out[0]["index"], json!(0));
        let ids: Vec<u64> = out.iter().map(|r| r["id"].as_u64().expect("id")).collect();
        assert_eq!(ids, vec![0, 1, 2]);
    }

    #[test]
    fn collapsed_is_true_only_when_every_window_has_it_collapsed() {
        assert_eq!(
            fold_normal(vec![cat(0, true, 0, true), cat(0, true, 0, true)])[0]["collapsed"],
            json!(true)
        );
        assert_eq!(
            fold_normal(vec![cat(0, true, 0, true), cat(0, true, 0, false)])[0]["collapsed"],
            json!(false),
            "한 창이라도 펼쳐져 있으면 접혔다고 답하면 안 된다"
        );
    }

    #[test]
    fn no_normal_row_is_invented_when_none_was_listed() {
        assert!(fold_normal(vec![]).is_empty());
        let only_other = fold_normal(vec![cat(1, false, 1, false)]);
        assert_eq!(only_other.len(), 1);
        assert_eq!(only_other[0]["id"], json!(1));
    }
}
