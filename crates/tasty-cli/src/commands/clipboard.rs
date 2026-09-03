//! `tasty clipboard` subcommand 정의.

use clap::Subcommand;

#[derive(Subcommand)]
pub enum ClipboardCommands {
    /// Write text to the local clipboard.
    SetText {
        /// Text to write to the clipboard.
        text: String,
    },
}
