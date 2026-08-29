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

pub mod browse;
pub mod create;
