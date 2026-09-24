#![forbid(unsafe_code)]

//! CLI·GUI·로컬 IPC가 공유하는 원격 workspace 조회와 생성.
//! tasty-ssh로 연결한 뒤 JSON-RPC를 보내며 로컬 화면 상태는 변경하지 않는다.

//! # 공개 계약
//!
//! 아래 항목은 호스트·CLI가 공유한다. doc(hidden) 항목은 CLI 구현 내부용이다.
//!
//! | 항목 | 소비자 |
//! |------|--------|
//! | [`browse::RemoteWorkspace`] · [`browse::browse`] · [`browse::browse_via_port`] | 본체 · CLI |
//! | [`browse::resolve_connection_spec`] · [`browse::resolve_endpoint`] | 본체 · CLI |
//! | [`create::create_via_port`] ([`create::CreatedRemoteWorkspace`] 는 그 반환형) | 본체 · CLI |
//! | [`browse::probe_method`] | 상황별 오류 안내를 만드는 호스트 호출자 |
//! | [`browse::PROBE_TIMEOUT`] | 소비자가 진행 표시·문구를 같은 값에 맞추도록 노출(`docs/dev-guide/attach-behavior.md#ssh-터널-원격-client-공통`) |

pub mod browse;
pub mod create;
