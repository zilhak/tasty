//! `tasty settings` subcommand 정의 — 원격 전송 저장 정책(07) get/set.
//!
//! general settings 전역 조작 CLI 는 이 서브커맨드가 처음이다(07 신설). 현재는
//! `remote-transfer` 저장 폴더 + 용량 상한만 노출한다. focus 독립(전역 설정,
//! 대상 ID 불요) — 인스턴스 IPC 포트 하나로 처리한다.

use clap::Subcommand;

#[derive(Subcommand)]
pub enum SettingsCommands {
    /// 원격 전송(bulk 파일 채널) 수신측 저장 폴더 + 용량 상한 조회.
    GetRemoteTransfer,
    /// 원격 전송 저장 폴더/용량 상한 설정. 하나 이상 지정해야 한다.
    SetRemoteTransfer {
        /// 저장 폴더 경로(빈 문자열 = 기본 `~/.tasty/transfers/`).
        #[arg(long)]
        dir: Option<String>,
        /// 폴더 최대 용량(MiB).
        #[arg(long)]
        max_mb: Option<u64>,
    },
}
