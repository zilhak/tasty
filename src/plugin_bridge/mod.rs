//! Plugin manager 가 본 바이너리 도메인 (engine / file / shortcuts / model 등)
//! 과 결합한 코드를 모아 두는 bin-side glue.
//!
//! tasty-host-plugin (manager crate) 가 본 바이너리를 역참조할 수 없으므로,
//! 본 모듈이 *protocol port impl* 의 본 바이너리 잔존 지점 역할을 한다.

pub mod manifest_validate;
