//! 공유 훅 핸들러 레지스트리 (webhook/hook 트리거 공유).
//!
//! 파일 핸들러(`src/file/handler/`)를 정본 템플릿으로 미러링한다. MVP 범위는
//! 타입 + 인메모리 레지스트리 + `source` 바인딩 게이트 + IpcSequence 실행 코어다.
//! 3출처 병합·user config 영속화·plugin contribute 는 후속 stage(S1b)에서 정식화한다.
//!
//! 핵심 불변식은 [`types`] 모듈 문서 참조 — 데이터/흐름 분리(`IpcCall.method`
//! 고정) + 셸 웹훅 거부(`ShellCommand` 는 webhook 바인딩 불가).

pub mod registry;
pub mod types;

pub use registry::{HookHandlerRegistry, RegistryError, global};
pub use types::{
    BindingError, HookHandler, HookHandlerAction, HookHandlerId, HookHandlerOwner, HookSource,
    IpcCall, TriggerSource, validate_binding,
};
