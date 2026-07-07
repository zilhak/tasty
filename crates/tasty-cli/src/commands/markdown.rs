//! `tasty markdown` subcommand 정의 — markdown surface 관련 조회/조작.
//!
//! 현재는 `recent`(최근 연 markdown 목록 조회)만. navigate(제자리 이동)는 주소창
//! 플러그인 전용 IPC 라 CLI 표면에 두지 않는다.

use clap::Subcommand;

#[derive(Subcommand)]
pub enum MarkdownCommands {
    /// List recently opened markdown files (newest first, up to 10).
    ///
    /// 읽기 전용 조회 — 포커스에 의존하지 않고 전역 최근목록을 반환한다
    /// (불가침 원칙: 에이전트 조회가 사용자 상태를 바꾸지 않는다).
    Recent,
}
