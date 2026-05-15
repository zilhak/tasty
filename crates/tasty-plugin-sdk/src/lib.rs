//! SDK for writing external Tasty plugins.
//!
//! 작성자는 [`Plugin`] trait를 구현하고 [`run`]을 호출하면 된다. SDK가
//! 호스트와의 핸드셰이크/메시지 루프/JSON 직렬화를 처리한다.

pub mod bus;
pub mod connection;
pub mod env;
pub mod error;
pub mod handle_channel;
pub mod host;
pub mod plugin;
pub mod runtime;
pub mod shared_buffer;
pub mod ui;

pub use bus::BusHandle;
pub use error::{PluginError, Result};
pub use host::HostHandle;
pub use shared_buffer::SharedBuffer;
#[allow(deprecated)]
pub use host::HostCallError;
pub use plugin::{
    CommandInvokeCtx, IpcMethodCtx, IpcMethodError, Plugin, SurfaceCloseReason, SurfaceCreateCtx,
    SurfaceEventCtx, SurfaceLifecycleCtx, SurfaceLifecycleEvent, SurfaceRestoreCtx, SurfaceResult,
    SurfaceSnapshotCtx,
};
pub use runtime::run;

// 자주 쓰이는 wire 타입을 SDK에서 직접 노출 — plugin 작성자가 별도로
// `tasty-plugin-protocol`을 의존하지 않아도 되도록.
pub use tasty_plugin_protocol::ui_tree::{
    ButtonStyle, LabelStyle, SelectionMode, SplitDir, TreeNode, UiEvent, UiNode,
};
pub use tasty_plugin_protocol::{
    EventEnvelope, EventMeta, EventOrigin, EventScope, LifecycleReason, PluginEvent, Rect,
    SharedBufferId,
};
