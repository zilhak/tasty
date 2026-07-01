//! SDK for writing external Tasty plugins.
//!
//! 작성자는 [`Plugin`] trait를 구현하고 [`run`]을 호출하면 된다. SDK가
//! 호스트와의 핸드셰이크/메시지 루프/JSON 직렬화를 처리한다.

pub mod bus;
pub mod connection;
/// egui-mesh plugin SDK 헬퍼 (A1-S4). `egui-mesh` feature 를 켰을 때만 컴파일된다 —
/// 기본 빌드는 egui 의존 없이 유지(lib.rs 불변식).
#[cfg(feature = "egui-mesh")]
pub mod egui_surface;
pub mod env;
pub mod error;
pub mod handle_channel;
pub mod host;
pub mod i18n;
pub mod plugin;
pub mod runtime;
pub mod shared_buffer;
pub mod ui;

pub use bus::BusHandle;
#[cfg(feature = "egui-mesh")]
pub use egui_surface::{EguiMeshBanner, EguiMeshPopup, EguiMeshSurface};
pub use env::PluginEnv;
pub use error::{PluginError, Result};
#[allow(deprecated)]
pub use host::HostCallError;
pub use host::HostHandle;
pub use i18n::Translator;
pub use plugin::{
    BannerClosedCtx, BannerOpenCtx, BannerSetContextCtx, CommandInvokeCtx, EventDispatchCtx,
    ExtensionHookCtx, ExtensionHookOutcome, IpcMethodCtx, IpcMethodError, Plugin, PopupClosedCtx,
    PopupEventCtx, PopupEventResult, PopupOpenCtx, PopupOpenResult, PopupSetContextCtx,
    SurfaceCreateCtx, SurfaceEventCtx, SurfaceRestoreCtx, SurfaceResult, SurfaceSetContextCtx,
    SurfaceSnapshotCtx,
};
pub use runtime::run;
pub use shared_buffer::SharedBuffer;
pub use tasty_plugin_protocol::ui_tree::{
    BadgeTone, ButtonStyle, LabelStyle, SelectionMode, SplitDir, TagTone, TreeNode, UiEvent, UiNode,
};
pub use tasty_plugin_protocol::{
    EventEnvelope, EventMeta, EventOrigin, EventScope, ExtensionHookKind, ExtensionHookMode,
    ExtensionHookPhase, LifecycleReason, PixelRect, PluginEvent, SharedBufferId,
};
