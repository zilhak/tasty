//! Plugin 매니페스트 — tasty-plugin-manifest crate 의 thin re-export.
//!
//! 본 모듈은 phase F.B.6-2 이전의 호출처 경로 `crate::plugin::manifest::<Type>`
//! 와의 하위 호환을 위해 유지된다. 새 코드는 가능하면
//! `tasty_plugin_manifest::<Type>` 를 직접 사용.

pub use tasty_plugin_manifest::*;
