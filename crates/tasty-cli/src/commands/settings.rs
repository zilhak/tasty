//! `tasty settings` subcommand 정의 — 원격 전송 저장 정책(07) get/set.
//!
//! general settings 전역 조작 CLI 는 이 서브커맨드가 처음이다(07 신설). 현재는
//! `remote-transfer` 저장 폴더 + 용량 상한만 노출한다. focus 독립(전역 설정,
//! 대상 ID 불요) — 인스턴스 IPC 포트 하나로 처리한다.

use clap::Subcommand;

#[derive(Subcommand)]
pub enum SettingsCommands {
    /// Show the receive-side storage folder and size cap for remote transfers
    /// (bulk file channel).
    GetRemoteTransfer,
    /// Set the remote-transfer storage folder and/or size cap. At least one
    /// must be given.
    SetRemoteTransfer {
        /// Storage folder path (empty string = default `~/.tasty/transfers/`).
        #[arg(long)]
        dir: Option<String>,
        /// Maximum folder size in MiB.
        #[arg(long)]
        max_mb: Option<u64>,
    },
}
