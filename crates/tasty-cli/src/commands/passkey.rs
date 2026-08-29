//! `tasty tool passkey ...` — Passkey(자격증명) CRUD 의 **clap 선언**.
//!
//! 실행은 [`crate::local::passkey`] 가 한다(로컬 파일, IPC 미경유).

use clap::Subcommand;

#[derive(Subcommand)]
pub enum PasskeyCommands {
    /// Passkey 추가/교체. `--path <file>`(파일 참조) 또는 `--inline`(비밀 입력) 중 하나.
    Add {
        /// 고유 식별자(프로필이 참조). 영숫자/-/_ 만 허용.
        #[arg(long)]
        name: String,
        /// path kind — 사용자 소유 기존 키 파일 경로(`-i`). `--inline` 과 배타.
        #[arg(long)]
        path: Option<String>,
        /// inline kind — 비밀을 0600 파일로 materialize. 값은 `--value` 또는 stdin.
        #[arg(long)]
        inline: bool,
        /// inline 값(미지정 시 stdin 에서 읽음). `--inline` 과 함께 쓴다.
        #[arg(long)]
        value: Option<String>,
    },
    /// 저장된 Passkey 목록(name + kind 만 — 값 비노출).
    List {
        #[arg(long)]
        json: bool,
    },
    /// 한 Passkey 의 name + kind 출력(값 비노출).
    Show {
        #[arg(long)]
        name: String,
        #[arg(long)]
        json: bool,
    },
    /// Passkey 제거(inline 이면 관리 파일도 삭제).
    Remove {
        #[arg(long)]
        name: String,
    },
}
