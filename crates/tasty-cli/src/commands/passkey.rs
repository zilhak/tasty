//! `tasty tool passkey ...` — Passkey(자격증명) CRUD 의 **clap 선언**.
//!
//! 실행은 [`crate::local::passkey`] 가 한다(로컬 파일, IPC 미경유).

use clap::Subcommand;

#[derive(Subcommand)]
pub enum PasskeyCommands {
    /// Add or replace a passkey. Use either `--path <file>` (reference a file)
    /// or `--inline` (enter a secret).
    Add {
        /// Unique identifier that profiles reference. Only alphanumerics, '-'
        /// and '_' are allowed.
        #[arg(long)]
        name: String,
        /// path kind: path to an existing key file owned by the user (`-i`).
        /// Mutually exclusive with `--inline`.
        #[arg(long)]
        path: Option<String>,
        /// inline kind: materialize the secret as a 0600 file. The value comes
        /// from `--value` or stdin.
        #[arg(long)]
        inline: bool,
        /// Inline value (read from stdin if omitted). Used together with
        /// `--inline`.
        #[arg(long)]
        value: Option<String>,
    },
    /// List saved passkeys (name and kind only; values are never shown).
    List {
        #[arg(long)]
        json: bool,
    },
    /// Show one passkey's name and kind (the value is never shown).
    Show {
        #[arg(long)]
        name: String,
        #[arg(long)]
        json: bool,
    },
    /// Remove a passkey (for the inline kind, the managed file is deleted too).
    Remove {
        #[arg(long)]
        name: String,
    },
}
