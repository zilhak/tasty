//! Tasty Clipboard Viewer plugin — 현재 시스템 클립보드(최신 하나)의 read-only 뷰어.
//!
//! 호스트 `shortcut.toggle_clipboard_viewer` 이벤트가 발화되면 popup contribute
//! trigger 매처가 새 인스턴스를 연다. 클립보드는 plugin process 내에서 arboard 로
//! **직접** 읽으며 호스트 IPC 를 경유하지 않는다 (ADR-0009 상 first-party 직접 read).
//!
//! UI 는 master-detail: 좌측에 가용 클립보드 타입 목록, 우측에 선택 타입의 상세.
//! 1차는 텍스트 타입만 지원하고, 이미지/헥스/HTML/RTF 등은 `clipboard::ClipboardType`
//! enum arm + reader 추가로 확장한다. read-only 라 쓰기/붙여넣기/제거 액션은 없다.

mod clipboard;
mod view;

use std::sync::Mutex;

use tasty_plugin_sdk::{
    BusHandle, HostHandle, Plugin, PluginEnv, PopupClosedCtx, PopupEventCtx, PopupEventResult,
    PopupOpenCtx, PopupOpenResult, SurfaceCreateCtx, SurfaceEventCtx, SurfaceResult, Translator,
    UiEvent,
};

use crate::clipboard::{ClipboardType, ContentRepr};

const PLUGIN_ID: &str = "com.tasty.clipboard-viewer";
const PLUGIN_VERSION: &str = "0.1.0";

/// open_popup 시점에 읽어둔 클립보드 스냅샷.
struct ViewerState {
    available: Vec<(ClipboardType, ContentRepr)>,
    read_error: Option<String>,
    selected: Option<ClipboardType>,
}

impl ViewerState {
    /// 현재 클립보드를 1회 읽어 스냅샷을 만든다. 첫 가용 타입을 기본 선택.
    fn load() -> Self {
        match clipboard::read_available() {
            Ok(available) => {
                let selected = available.first().map(|(ty, _)| *ty);
                Self {
                    available,
                    read_error: None,
                    selected,
                }
            }
            Err(e) => {
                tracing::warn!("clipboard read failed: {e}");
                Self {
                    available: Vec::new(),
                    read_error: Some(e),
                    selected: None,
                }
            }
        }
    }

    fn build_tree(&self, tr: &Translator) -> tasty_plugin_sdk::UiNode {
        let vm = view::ViewModel {
            available: &self.available,
            read_error: self.read_error.as_deref(),
            selected: self.selected,
        };
        view::main_tree(&vm, tr)
    }
}

struct ClipboardViewerPlugin {
    /// 단일 인스턴스 가드 — 두 번째 open 시 placeholder 만 반환.
    current_instance: Mutex<Option<u64>>,
    /// 활성 인스턴스의 클립보드 스냅샷.
    state: Mutex<Option<ViewerState>>,
    tr: Translator,
}

impl ClipboardViewerPlugin {
    fn new(env: &PluginEnv) -> Self {
        Self {
            current_instance: Mutex::new(None),
            state: Mutex::new(None),
            tr: Translator::from_plugin_env(env),
        }
    }
}

impl Plugin for ClipboardViewerPlugin {
    fn id(&self) -> &str {
        PLUGIN_ID
    }

    fn version(&self) -> &str {
        PLUGIN_VERSION
    }

    fn on_start(&mut self, _host: HostHandle, _bus: BusHandle) {}

    // popup-only plugin 이라 surface 콜백은 빈 결과.
    fn create_surface(&mut self, _ctx: SurfaceCreateCtx) -> SurfaceResult {
        SurfaceResult::default()
    }

    fn handle_event(&mut self, _ctx: SurfaceEventCtx) -> SurfaceResult {
        SurfaceResult::default()
    }

    fn open_popup(&mut self, ctx: PopupOpenCtx) -> PopupOpenResult {
        let mut guard = match self.current_instance.lock() {
            Ok(g) => g,
            Err(_) => return PopupOpenResult { tree: None },
        };
        if guard.is_some() {
            return PopupOpenResult {
                tree: Some(view::already_open_tree(&self.tr)),
            };
        }
        let new_state = ViewerState::load();
        let tree = new_state.build_tree(&self.tr);

        *guard = Some(ctx.instance_id);
        if let Ok(mut s) = self.state.lock() {
            *s = Some(new_state);
        }
        PopupOpenResult { tree: Some(tree) }
    }

    fn handle_popup_event(&mut self, ctx: PopupEventCtx) -> PopupEventResult {
        let UiEvent::Click { node_id } = &ctx.event else {
            return PopupEventResult {
                tree: None,
                close: false,
            };
        };

        // 주(主) 인스턴스가 아닌 인스턴스(중복 placeholder)에서 온 클릭은 무시.
        if self.current_instance.lock().ok().and_then(|g| *g) != Some(ctx.instance_id) {
            return PopupEventResult {
                tree: None,
                close: false,
            };
        }

        let mut state_guard = match self.state.lock() {
            Ok(g) => g,
            Err(_) => {
                return PopupEventResult {
                    tree: None,
                    close: false,
                };
            }
        };
        let Some(state) = state_guard.as_mut() else {
            return PopupEventResult {
                tree: None,
                close: false,
            };
        };

        // `type-{key}` 클릭 → 좌측 선택 변경 → 우측 재렌더.
        let dirty = if let Some(key) = node_id.strip_prefix(view::TYPE_PREFIX) {
            match ClipboardType::from_key(key) {
                Some(ty) if state.selected != Some(ty) => {
                    state.selected = Some(ty);
                    true
                }
                _ => false,
            }
        } else {
            false
        };

        if dirty {
            PopupEventResult {
                tree: Some(state.build_tree(&self.tr)),
                close: false,
            }
        } else {
            PopupEventResult {
                tree: None,
                close: false,
            }
        }
    }

    fn on_popup_closed(&mut self, ctx: PopupClosedCtx) {
        if let Ok(mut g) = self.current_instance.lock()
            && *g == Some(ctx.instance_id)
        {
            *g = None;
            if let Ok(mut s) = self.state.lock() {
                *s = None;
            }
        }
    }
}

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();
    let env = PluginEnv::load()?;
    let plugin = ClipboardViewerPlugin::new(&env);
    tasty_plugin_sdk::run(plugin)
}
