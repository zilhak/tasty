#![forbid(unsafe_code)]

//! Host-only Lua scripting layer for Tasty.
//!
//! 사용자가 `~/.tasty/init.lua` 를 작성해 Tasty 이벤트에 자기 스크립트를 붙일 수
//! 있게 한다. 현재 시점 hook 은 **observe-only** — 반환값으로 Tasty 동작을 바꿀
//! 수 없고 단지 외부 자동화 (로그/알림/CLI 호출) 만 한다.
//!
//! # 신뢰 모델
//!
//! 사용자가 자기 머신에서 자기 권한으로 작성하는 스크립트이므로 plugin escape 같은
//! 위험은 없다. sandbox 의 목적은:
//!
//! - **DoS 보호**: 무한 루프 (instruction cap), 메모리 폭발 (memory cap)
//! - **호스트 무결성**: native crash 유발 가능한 표면 차단 (debug, bytecode loader,
//!   `package.loadlib`)
//!
//! `io` / `os.execute` 는 *제거하지 않는다*. 사용자가 자기 권한으로 임의 명령을
//! 실행할 수 있는 환경에서 굳이 막을 이유가 없고, Lua hook 의 주된 용도가
//! "외부 동작" 이라 차단 시 효용이 크게 떨어진다. 대신 `tasty.run_cli` 등 명시적
//! 호스트 API 를 우선 권장.

mod engine;
mod host_api;
mod sandbox;

pub use engine::{LuaEngine, LuaEngineError};
