#![cfg(feature = "process")]
//! Tasty Clipboard History plugin — 외부 plugin.
//!
//! 호스트가 OS clipboard listener로 채우는 `tool.clipboard.list` IPC를 호출해
//! viewer popup을 그린다. 데이터는 호스트가 계속 소유하고 plugin은 stateless viewer다.
//!
//! 호스트가 publish하는 `shortcut.toggle_clipboard_viewer` 이벤트가 발화되면
//! popup contribute trigger 매처에 의해 자동으로 새 인스턴스가 열린다. 이미
//! 떠 있는 viewer가 있다면 새 인스턴스는 placeholder를 띄워두며, 닫기는
//! outside-click / Escape / 항목 클릭으로 한다.

use std::sync::Mutex;

use serde_json::{Value, json};
use tasty_plugin_sdk::{
    IpcMethodCtx, IpcMethodError, Plugin, PluginEnv, PopupClosedCtx, PopupEventCtx,
    PopupEventResult, PopupOpenCtx, PopupOpenResult, SurfaceCreateCtx, SurfaceEventCtx,
    SurfaceResult, Translator, UiEvent, UiNode, bus::BusHandle, host::HostHandle,
};

const PLUGIN_ID: &str = "com.tasty.clipboard-history";
const PLUGIN_VERSION: &str = "0.1.0";

/// `paste-{index}` — 항목 클릭 → 붙여넣기.
const PASTE_PREFIX: &str = "paste-";
/// `remove-{index}` — 항목 옆 × 버튼 → 단일 제거.
const REMOVE_PREFIX: &str = "remove-";
/// 전체 비우기 버튼 id.
const CLEAR_ID: &str = "clear-all";

struct ClipboardHistoryPlugin {
    /// `on_start`에서 호스트가 건네준 핸들. open_popup / handle_popup_event에서
    /// 호스트 IPC를 호출할 때 사용한다.
    host: Mutex<Option<HostHandle>>,
    /// 현재 떠 있는 viewer instance_id. Some이면 추가 open 요청에 placeholder만 표시.
    current_instance: Mutex<Option<u64>>,
    /// plugin 자체 i18n 카탈로그.
    tr: Translator,
}

impl ClipboardHistoryPlugin {
    fn new(tr: Translator) -> Self {
        Self {
            host: Mutex::new(None),
            current_instance: Mutex::new(None),
            tr,
        }
    }

    fn host_handle(&self) -> Option<HostHandle> {
        self.host.lock().ok().and_then(|g| g.clone())
    }
}

impl Plugin for ClipboardHistoryPlugin {
    fn id(&self) -> &str {
        PLUGIN_ID
    }

    fn version(&self) -> &str {
        PLUGIN_VERSION
    }

    fn on_start(&mut self, host: HostHandle, _bus: BusHandle) {
        if let Ok(mut g) = self.host.lock() {
            *g = Some(host);
        }
    }

    fn create_surface(&mut self, _ctx: SurfaceCreateCtx) -> SurfaceResult {
        SurfaceResult {
            tree: None,
            display_name: None,
        }
    }

    fn handle_event(&mut self, _ctx: SurfaceEventCtx) -> SurfaceResult {
        SurfaceResult {
            tree: None,
            display_name: None,
        }
    }

    fn open_popup(&mut self, ctx: PopupOpenCtx) -> PopupOpenResult {
        let mut guard = match self.current_instance.lock() {
            Ok(g) => g,
            Err(_) => return PopupOpenResult { tree: None },
        };
        if guard.is_some() {
            // 이미 떠 있는 viewer가 있음을 알리는 placeholder. 사용자는 첫
            // viewer를 outside-click 또는 Esc로 닫고 다시 열면 된다.
            let tree = UiNode::Vbox {
                spacing: 8,
                children: vec![UiNode::Label {
                    text: self.tr.t("clipboard_history.popup.already_open").into(),
                    style: Default::default(),
                    color: Some("subtext0".into()),
                }],
            };
            return PopupOpenResult { tree: Some(tree) };
        }
        *guard = Some(ctx.instance_id);
        drop(guard);

        let tree = match self.host_handle() {
            Some(h) => fetch_and_render(&h, &self.tr),
            None => build_loading_tree(&self.tr),
        };
        PopupOpenResult { tree: Some(tree) }
    }

    fn handle_popup_event(&mut self, ctx: PopupEventCtx) -> PopupEventResult {
        let UiEvent::Click { node_id } = &ctx.event else {
            return PopupEventResult {
                tree: None,
                close: false,
            };
        };
        let Some(host) = self.host_handle() else {
            return PopupEventResult {
                tree: None,
                close: false,
            };
        };

        if let Some(idx_str) = node_id.strip_prefix(PASTE_PREFIX)
            && let Ok(idx) = idx_str.parse::<u64>()
        {
            if let Err(e) = host.call("tool.clipboard.paste", json!({ "index": idx })) {
                tracing::warn!("tool.clipboard.paste failed: {e}");
            }
            return PopupEventResult {
                tree: None,
                close: true,
            };
        }

        if let Some(idx_str) = node_id.strip_prefix(REMOVE_PREFIX)
            && let Ok(idx) = idx_str.parse::<u64>()
        {
            if let Err(e) = host.call("tool.clipboard.remove", json!({ "index": idx })) {
                tracing::warn!("tool.clipboard.remove failed: {e}");
            }
            return PopupEventResult {
                tree: Some(fetch_and_render(&host, &self.tr)),
                close: false,
            };
        }

        if node_id == CLEAR_ID {
            if let Err(e) = host.call("tool.clipboard.clear", json!({})) {
                tracing::warn!("tool.clipboard.clear failed: {e}");
            }
            return PopupEventResult {
                tree: Some(fetch_and_render(&host, &self.tr)),
                close: false,
            };
        }

        PopupEventResult {
            tree: None,
            close: false,
        }
    }

    fn on_popup_closed(&mut self, ctx: PopupClosedCtx) {
        if let Ok(mut guard) = self.current_instance.lock()
            && *guard == Some(ctx.instance_id)
        {
            *guard = None;
        }
    }

    fn handle_ipc_method(&mut self, ctx: IpcMethodCtx) -> Result<Value, IpcMethodError> {
        // 자기 namespace 메서드 노출 없음. prefix는 i18n 키 충돌 방지용으로만 점유.
        Err(IpcMethodError::not_found(&ctx.method))
    }
}

fn build_loading_tree(tr: &Translator) -> UiNode {
    UiNode::Vbox {
        spacing: 8,
        children: vec![UiNode::Label {
            text: tr.t("clipboard_history.popup.loading").into(),
            style: Default::default(),
            color: Some("subtext0".into()),
        }],
    }
}

fn fetch_and_render(host: &HostHandle, tr: &Translator) -> UiNode {
    match host.call("tool.clipboard.list", json!({ "limit": 50 })) {
        Ok(v) => render_entries(&v, tr),
        Err(e) => {
            tracing::warn!("tool.clipboard.list failed: {e}");
            UiNode::Vbox {
                spacing: 8,
                children: vec![UiNode::Label {
                    text: tr.t("clipboard_history.popup.load_failed").into(),
                    style: Default::default(),
                    color: Some("red".into()),
                }],
            }
        }
    }
}

fn render_entries(list: &Value, tr: &Translator) -> UiNode {
    let entries = list.get("entries").and_then(|v| v.as_array());
    let Some(entries) = entries else {
        return build_loading_tree(tr);
    };
    if entries.is_empty() {
        return UiNode::Vbox {
            spacing: 8,
            children: vec![UiNode::Label {
                text: tr.t("clipboard_history.popup.empty").into(),
                style: Default::default(),
                color: Some("subtext0".into()),
            }],
        };
    }
    let mut children: Vec<UiNode> = Vec::with_capacity(entries.len() + 2);
    children.push(UiNode::Hbox {
        spacing: 8,
        children: vec![
            UiNode::Label {
                text: tr.t("clipboard_history.popup.title").into(),
                style: Default::default(),
                color: None,
            },
            UiNode::Spacer { size: 0 },
            UiNode::Button {
                id: CLEAR_ID.into(),
                label: tr.t("clipboard_history.popup.clear_all").into(),
                enabled: true,
                style: Default::default(),
                tooltip_i18n_key: None,
            },
        ],
    });
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
        children.push(UiNode::Hbox {
            spacing: 4,
            children: vec![
                UiNode::Button {
                    id: format!("{PASTE_PREFIX}{idx}"),
                    label,
                    enabled: true,
                    style: Default::default(),
                    tooltip_i18n_key: None,
                },
                UiNode::Button {
                    id: format!("{REMOVE_PREFIX}{idx}"),
                    label: "×".into(),
                    enabled: true,
                    style: Default::default(),
                    tooltip_i18n_key: None,
                },
            ],
        });
    }
    UiNode::Vbox {
        spacing: 4,
        children,
    }
}

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();
    let env = PluginEnv::load()?;
    let tr = Translator::from_plugin_env(&env);
    tasty_plugin_sdk::run(ClipboardHistoryPlugin::new(tr))
}
