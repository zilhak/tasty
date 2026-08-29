#![forbid(unsafe_code)]

//! 원격 tasty 인스턴스에 대한 **client 측 능력** — 워크스페이스 조회와 생성.
//!
//! 소비자가 셋이다: CLI(`tasty remote workspaces` / `tasty remote new-workspace`) ·
//! 본체 GUI 의 원격 attach 팝업 · 로컬 IPC method(`remote.workspaces` / `remote.attach`
//! 의 생성 옵션). 같은 능력을 CLI 와 IPC 양면으로 노출하는 것이 불가침 원칙 2 라,
//! 코어는 어느 한 소비자의 패키지가 아니라 공유 크레이트에 둔다.
//!
//! 조회([`browse`])와 변경([`create`])을 모듈 경계로 가른다 — browse 라는 이름 아래
//! mutate 를 숨기지 않는다.
//!
//! SSH 위임(`tasty-ssh`)에 얹혀 원격 포트를 얻고, 그 포트로 JSON-RPC(`tasty-ipc`)를
//! 호출한다. 의존은 이 방향으로만 흐른다 — `tasty-ssh` 는 IPC 를 모른다.

//! # 공개 계약
//!
//! 크레이트 밖에서 쓰는 것은 아래가 전부다. `pub` 이 아닌 항목은 내부 구현이고,
//! `#[doc(hidden)]` 이 붙은 항목은 `tasty-cli` 커맨드 구현 전용이라 계약이 아니다.
//!
//! | 항목 | 소비자 |
//! |------|--------|
//! | [`browse::RemoteWorkspace`] · [`browse::browse`] · [`browse::browse_via_port`] | 본체 · CLI |
//! | [`browse::resolve_connection_spec`] · [`browse::resolve_endpoint`] | 본체 · CLI |
//! | [`create::create_via_port`] ([`create::CreatedRemoteWorkspace`] 는 그 반환형) | 본체 · CLI |
//! | [`browse::PROBE_TIMEOUT`] | 소비자가 진행 표시·문구를 같은 값에 맞추도록 노출(`docs/adr/0070-port-discovery-timeout.md`) |

pub mod browse;
pub mod create;
