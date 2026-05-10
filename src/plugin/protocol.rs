//! Plugin ↔ Host wire types는 `tasty-plugin-protocol` 크레이트로 이동했다.
//! 호스트 코드 호환을 위해 thin re-export만 남긴다.
//!
//! 이 모듈은 protocol API surface를 한 곳에서 노출하는 역할이므로, 호스트 본문이
//! 일부 항목을 직접 참조하지 않더라도 의도적으로 그대로 둔다.

#![allow(unused_imports)]

pub use tasty_plugin_protocol::protocol::{
    AuthMessage, CommandInvokeParams, IpcCallResult, PluginEvent, PluginRequest, PluginResponse,
    SurfaceEventParams, SurfaceResult,
};
pub use tasty_plugin_protocol::protocol::{
    METHOD_COMMAND_INVOKE, METHOD_HOST_HELLO, METHOD_IPC_RESULT, METHOD_PING, METHOD_SHUTDOWN,
    METHOD_SURFACE_CREATE, METHOD_SURFACE_DESTROY, METHOD_SURFACE_EVENT, METHOD_SURFACE_RESTORE,
    METHOD_SURFACE_SNAPSHOT,
};
