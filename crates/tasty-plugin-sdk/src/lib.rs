//! SDK for writing external Tasty plugins.
//!
//! 작성자는 [`Plugin`] trait를 구현하고 [`run`]을 호출하면 된다. SDK가
//! 호스트와의 핸드셰이크/메시지 루프/JSON 직렬화를 처리한다.

// 이유: 테스트 본문의 `let _ =` 는 정책이 사유를 요구하지 않는 자리라
// `clippy::let_underscore_must_use` 명부에 섞이면 안 된다 — 그 명부는 프로덕션에서
// 값을 버리는 자리의 목록이고, 테스트가 늘 때마다 숫자만 흔들리면 새 프로덕션
// 자리가 그 안에 묻힌다(docs/dev-guide/error-handling.md). `cfg_attr(test, ..)` 라
// 라이브러리 타깃의 판정은 그대로다 — 프로덕션 자리는 여전히 명부에 오른다.
#![cfg_attr(test, allow(clippy::let_underscore_must_use))]

/// 빌드타임 베이크된 벡터 아이콘을 egui painter 로 그리는 helper. `egui-mesh` feature 를
/// 켰을 때만(= egui 링크 시) 컴파일된다. image / markdown plugin 이 공유한다.
#[cfg(feature = "egui-mesh")]
pub mod baked_icon;
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
    PopupOpenCtx, PopupOpenResult, PopupSetContextCtx, SurfaceCreateCtx, SurfaceRestoreCtx,
    SurfaceResult, SurfaceSetContextCtx, SurfaceSnapshotCtx, WebviewNavigationAttemptCtx,
};
pub use runtime::run;
pub use shared_buffer::SharedBuffer;
pub use tasty_plugin_protocol::{
    EventEnvelope, EventMeta, EventOrigin, EventScope, ExtensionHookKind, ExtensionHookMode,
    ExtensionHookPhase, LifecycleReason, PixelRect, PluginEvent, SharedBufferId,
};
