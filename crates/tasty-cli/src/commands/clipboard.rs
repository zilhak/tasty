//! `tasty clipboard` subcommand 정의.

use clap::Subcommand;

#[derive(Subcommand)]
pub enum ClipboardCommands {
    /// 로컬 클립보드에 텍스트를 쓴다.
    SetText {
        /// 클립보드에 쓸 텍스트.
        text: String,
    },
}
