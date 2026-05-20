//! 파일 핸들러 시스템.
//!
//! `FileHandlerRegistry` 가 detector → handler 매핑을 관리한다. `file_format` 의
//! evaluator / rule kind 를 모르고, `DetectorId` 만 import 한다.
#![allow(dead_code, unused_imports)]

pub mod config;
pub mod registry;
pub mod types;

pub use config::{HandlerDeclError, UserHandlerActionDecl};
pub use registry::{FileHandlerRegistry, UserHandlerUpsertDecl};
pub use types::{
    is_valid_handler_short_name, FileHandler, HandlerAction, HandlerId, HandlerOwner,
};