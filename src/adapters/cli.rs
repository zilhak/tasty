//! CLI — tasty-cli crate 의 thin re-export.
//!
//! 본 바이너리 boot 경로 `boot.rs` / `main.rs` 가 사용하던 `crate::cli::*` 진입점
//! 들은 모두 tasty_cli 가 owning. 본 모듈은 호환 alias 만 제공한다.
#![allow(unused_imports)]

pub use tasty_cli::*;
