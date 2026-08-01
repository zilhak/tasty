#![forbid(unsafe_code)]

//! Cross-cutting utility helpers for Tasty.
//!
//! **Leaf crate** — 어떤 `tasty-*` crate 도 의존하지 않는다. cross-cutting 인프라
//! (경로 해석, 도메인 식별자 등) 의 단일 출처.
//!
//! 현재 제공:
//! - [`path`] — `tasty_home()` 등 사용자 데이터 디렉토리 경로
//! - [`id`] — Workspace/Pane/Tab/Surface 식별자 alias
//! - [`process`] — 자식 프로세스 spawn 공통 설정 (Windows 콘솔 창 숨김 등)
//! - [`notify`] — child→caller 완료 알림 로그(Monitor tool tail 대상). claude/codex
//!   plugin 이 공유하는 append 경로 규약이라, 개별 plugin 이 아니라 leaf utils 에서
//!   [`path::tasty_home`] 위에 단일 정의한다(writer/reader 경로 일치 보장).
//! - [`shell_family`] — 셸 바이너리 경로에서 bash/zsh/기타 계열 판정(basename 기준).
//!   tasty-settings(rc 주입 여부 결정)와 tasty-terminal(spawn 인자 조정) 양쪽이
//!   공유한다.
//!
//! 도메인별 경로 (themes 디렉토리, memory db 경로, config 파일 위치 등) 는 각
//! 도메인 crate 가 [`path::tasty_home`] 위에 자체 정의한다. utils 는 *공통 기반*
//! 만 노출한다.

pub mod id;
pub mod notify;
pub mod path;
pub mod process;
pub mod shell_family;
