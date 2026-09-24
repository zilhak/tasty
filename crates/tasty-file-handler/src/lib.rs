//! 파일 핸들러 시스템.
//!
//! `FileHandlerRegistry` 가 detector → handler 매핑을 관리한다. `file_format` 의
//! evaluator / rule kind 를 모르고, `DetectorId` 만 import 한다.

pub mod config;
pub mod recent;
pub mod registry;
pub mod save;
pub mod types;

pub use config::{HandlerDeclError, UserHandlerActionDecl};
pub use registry::{
    FileHandlerRegistry, RejectedUserHandler, UserHandlerRejectReason, UserHandlerUpsertDecl,
};
pub use types::{FileHandler, HandlerAction, HandlerId, HandlerOwner, is_valid_handler_short_name};

/// 호스트 기본 handler TOML. 소비자는 이 값을 공유해 경로별 사본을 만들지 않는다.
pub const HOST_DEFAULTS_TOML: &str = include_str!("defaults/default-file-handlers.toml");
