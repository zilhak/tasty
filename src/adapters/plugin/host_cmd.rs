//! `register_remote_kind`의 create/restore 클로저가 PluginManager에게 보내는 명령.
//!
//! 새 RemoteSurface가 만들어지면 해당 surface의 `Arc<Mutex>` 핸들 묶음을 manager에
//! 전달하여 manager가 plugin과의 메시지 흐름에서 이 surface를 추적할 수 있도록 한다.

use std::sync::{Arc, Mutex};

use serde_json::Value;

use crate::plugin::ui_tree::{UiEvent, UiNode};

/// `RemoteSurface`의 내부 상태에 manager가 외부에서 접근하기 위한 핸들.
#[derive(Clone)]
pub struct SurfaceHandles {
    pub tree: Arc<Mutex<Option<UiNode>>>,
    pub pending_events: Arc<Mutex<Vec<UiEvent>>>,
    pub display_name: Arc<Mutex<String>>,
}

/// registry create/restore closure가 manager에게 보내는 명령.
pub enum HostCmd {
    RemoteSurfaceCreated {
        surface_id: u32,
        plugin_id: String,
        kind: String,
        params: Value,
        handles: SurfaceHandles,
    },
    RemoteSurfaceRestored {
        surface_id: u32,
        plugin_id: String,
        kind: String,
        data: Value,
        handles: SurfaceHandles,
    },
}
