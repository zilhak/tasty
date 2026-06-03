//! Tasty Clipboard History plugin — WASM POC entry (Phase J.C).
//!
//! `wasm` feature 활성 시 wasi-preview2 component 의 `lifecycle` 인터페이스를
//! export 한다. process 빌드와 *동일 동작* 을 보이는 것이 POC 의 검증 기준 — 즉
//! popup tree 모양 + click 응답이 process 버전과 동일해야 한다.
//!
//! ## 아키텍처
//!
//! - host call 4 종 (`tool.clipboard.list/paste/remove/clear`) 은 모두
//!   `bindings::host::host_call(method, params_json)` 로 트램폴린.
//! - i18n 은 `bindings::host::tr(key, locale)` 로 host injection.
//!   `Translator` (lang/<locale>.toml fs read) 는 사용하지 않음.
//! - state 는 thread_local — wasi-preview2 는 single-thread 모델 가정.
//!
//! ## POC 범위 외
//!
//! - SurfaceCreateCtx / SurfaceEventCtx 는 clipboard-history 가 미사용 → no-op.
//! - handle_ipc_method 는 process 버전과 동일하게 "not found" 반환.

#![cfg(feature = "wasm")]

use std::cell::RefCell;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

wit_bindgen::generate!({
    world: "tasty-plugin",
    path: "wit",
    pub_export_macro: true,
});

use exports::tasty::plugin::lifecycle::Guest;
use tasty::plugin::host;

const PASTE_PREFIX: &str = "paste-";
const REMOVE_PREFIX: &str = "remove-";
const CLEAR_ID: &str = "clear-all";

thread_local! {
    static STATE: RefCell<State> = RefCell::new(State::default());
}

#[derive(Default)]
struct State {
    locale: String,
    current_instance: Option<u64>,
}

struct Component;

impl Guest for Component {
    fn init(plugin_id: String, locale: String) {
        STATE.with(|s| {
            let mut g = s.borrow_mut();
            g.locale = locale.clone();
            g.current_instance = None;
        });
        host::log(
            "info",
            &format!("clipboard-history wasm init: id={plugin_id} locale={locale}"),
        );
    }

    fn open_popup(ctx_json: String) -> String {
        let ctx: PopupOpenCtx = match serde_json::from_str(&ctx_json) {
            Ok(c) => c,
            Err(e) => return error_tree(&format!("open_popup decode: {e}")),
        };
        let locale = STATE.with(|s| s.borrow().locale.clone());
        let already_open = STATE.with(|s| {
            let mut g = s.borrow_mut();
            if g.current_instance.is_some() {
                true
            } else {
                g.current_instance = Some(ctx.instance_id);
                false
            }
        });
        if already_open {
            return json!({
                "tree": vbox(8, vec![label_subtext(&tr(&locale, "clipboard_history.popup.already_open"))])
            })
            .to_string();
        }
        let tree = fetch_and_render(&locale);
        json!({ "tree": tree }).to_string()
    }

    fn handle_popup_event(ctx_json: String) -> String {
        let ctx: PopupEventCtx = match serde_json::from_str(&ctx_json) {
            Ok(c) => c,
            Err(e) => return error_tree(&format!("handle_popup_event decode: {e}")),
        };
        let locale = STATE.with(|s| s.borrow().locale.clone());
        let node_id = ctx.event.node_id.unwrap_or_default();

        // host_call 실패 시에도 popup 동작 유지 (process 버전과 동일 의미).
        if let Some(idx_str) = node_id.strip_prefix(PASTE_PREFIX)
            && let Ok(idx) = idx_str.parse::<u64>()
        {
            ignore_err(host_call("tool.clipboard.paste", &json!({ "index": idx })));
            return json!({ "tree": null, "close": true }).to_string();
        }
        if let Some(idx_str) = node_id.strip_prefix(REMOVE_PREFIX)
            && let Ok(idx) = idx_str.parse::<u64>()
        {
            ignore_err(host_call("tool.clipboard.remove", &json!({ "index": idx })));
            let tree = fetch_and_render(&locale);
            return json!({ "tree": tree, "close": false }).to_string();
        }
        if node_id == CLEAR_ID {
            ignore_err(host_call("tool.clipboard.clear", &json!({})));
            let tree = fetch_and_render(&locale);
            return json!({ "tree": tree, "close": false }).to_string();
        }
        json!({ "tree": null, "close": false }).to_string()
    }

    fn on_popup_closed(ctx_json: String) {
        if let Ok(ctx) = serde_json::from_str::<PopupClosedCtx>(&ctx_json) {
            STATE.with(|s| {
                let mut g = s.borrow_mut();
                if g.current_instance == Some(ctx.instance_id) {
                    g.current_instance = None;
                }
            });
        }
    }

    fn create_surface(_ctx_json: String) -> String {
        json!({ "tree": null, "display_name": null }).to_string()
    }

    fn handle_surface_event(_ctx_json: String) -> String {
        json!({ "tree": null, "display_name": null }).to_string()
    }

    fn handle_ipc_method(method: String, _params_json: String) -> String {
        json!({
            "err": {
                "code": "not_found",
                "message": format!("method '{method}' not handled by clipboard-history")
            }
        })
        .to_string()
    }
}

export!(Component);

// ----- helpers -----

fn tr(locale: &str, key: &str) -> String {
    host::tr(key, locale)
}

fn host_call(method: &str, params: &Value) -> Result<Value, String> {
    let res = host::host_call(method, &params.to_string())?;
    serde_json::from_str(&res).map_err(|e| format!("decode {method} response: {e}"))
}

fn ignore_err<T>(res: Result<T, String>) {
    if let Err(e) = res {
        host::log("warn", &format!("host_call err (popup 계속 진행): {e}"));
    }
}

fn fetch_and_render(locale: &str) -> Value {
    match host_call("tool.clipboard.list", &json!({ "limit": 50 })) {
        Ok(v) => render_entries(&v, locale),
        Err(e) => {
            host::log("warn", &format!("tool.clipboard.list failed: {e}"));
            vbox(
                8,
                vec![label_with_color(
                    &tr(locale, "clipboard_history.popup.load_failed"),
                    "red",
                )],
            )
        }
    }
}

fn render_entries(list: &Value, locale: &str) -> Value {
    let Some(entries) = list.get("entries").and_then(|v| v.as_array()) else {
        return vbox(
            8,
            vec![label_subtext(&tr(
                locale,
                "clipboard_history.popup.loading",
            ))],
        );
    };
    if entries.is_empty() {
        return vbox(
            8,
            vec![label_subtext(&tr(locale, "clipboard_history.popup.empty"))],
        );
    }
    let mut children: Vec<Value> = Vec::with_capacity(entries.len() + 2);
    children.push(json!({
        "Hbox": {
            "spacing": 8,
            "children": [
                label_plain(&tr(locale, "clipboard_history.popup.title")),
                { "Spacer": { "size": 0 } },
                button(CLEAR_ID, &tr(locale, "clipboard_history.popup.clear_all")),
            ]
        }
    }));
    for e in entries {
        let idx = e.get("index").and_then(|v| v.as_u64()).unwrap_or(0);
        let text = e
            .get("text")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let label = if text.chars().count() > 80 {
            let truncated: String = text.chars().take(80).collect();
            format!("{truncated}…")
        } else {
            text
        };
        children.push(json!({
            "Hbox": {
                "spacing": 4,
                "children": [
                    button(&format!("{PASTE_PREFIX}{idx}"), &label),
                    button(&format!("{REMOVE_PREFIX}{idx}"), "×"),
                ]
            }
        }));
    }
    json!({
        "Vbox": {
            "spacing": 4,
            "children": children,
        }
    })
}

fn vbox(spacing: u32, children: Vec<Value>) -> Value {
    json!({ "Vbox": { "spacing": spacing, "children": children } })
}

fn label_plain(text: &str) -> Value {
    json!({ "Label": { "text": text, "style": {}, "color": null } })
}

fn label_subtext(text: &str) -> Value {
    json!({ "Label": { "text": text, "style": {}, "color": "subtext0" } })
}

fn label_with_color(text: &str, color: &str) -> Value {
    json!({ "Label": { "text": text, "style": {}, "color": color } })
}

fn button(id: &str, label: &str) -> Value {
    json!({
        "Button": {
            "id": id, "label": label, "enabled": true, "style": {}, "tooltip_i18n_key": null
        }
    })
}

fn error_tree(msg: &str) -> String {
    host::log("error", msg);
    json!({
        "tree": vbox(8, vec![label_with_color(msg, "red")])
    })
    .to_string()
}

// ----- ctx types (host 가 직렬화 → 본 plugin 이 역직렬화) -----

#[derive(Deserialize)]
struct PopupOpenCtx {
    instance_id: u64,
    #[serde(default)]
    #[allow(dead_code)]
    popup_id: String,
}

#[derive(Deserialize)]
struct PopupEventCtx {
    #[allow(dead_code)]
    instance_id: u64,
    event: UiEvent,
}

#[derive(Deserialize)]
struct UiEvent {
    #[allow(dead_code)]
    #[serde(rename = "type")]
    kind: Option<String>,
    node_id: Option<String>,
}

#[derive(Deserialize)]
struct PopupClosedCtx {
    instance_id: u64,
}

// silence unused-imports warnings on serde::Serialize when no fields require it.
#[allow(dead_code)]
#[derive(Serialize)]
struct _PocPlaceholder;
