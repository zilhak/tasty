//! 호스트 코드에서 사용할 tasty-plugin-protocol의 공개 타입을 다시 내보낸다.

#![allow(unused_imports)]

pub use tasty_plugin_protocol::protocol::{
    AuthAck, AuthAckEnvelope, AuthMessage, BannerCloseReason, BannerClosedParams, BannerOpenParams,
    BannerOpenResult, BannerSetContextParams, CommandInvokeParams, IpcCallResult, PluginEvent,
    PluginRequest, PluginResponse, PopupCloseReason, PopupClosedParams, PopupOpenParams,
    PopupOpenResult, SurfaceResult, WebviewNavigationAttemptParams,
};
pub use tasty_plugin_protocol::protocol::{
    METHOD_BANNER_CLOSED, METHOD_BANNER_OPEN, METHOD_BANNER_SET_CONTEXT, METHOD_COMMAND_INVOKE,
    METHOD_HOST_HELLO, METHOD_IPC_RESULT, METHOD_PING, METHOD_POPUP_CLOSED, METHOD_POPUP_OPEN,
    METHOD_POPUP_SET_CONTEXT, METHOD_SHUTDOWN, METHOD_SURFACE_CREATE, METHOD_SURFACE_DESTROY,
    METHOD_SURFACE_RESTORE, METHOD_SURFACE_SET_CONTEXT, METHOD_SURFACE_SNAPSHOT,
    METHOD_WEBVIEW_NAVIGATION_ATTEMPT,
};
