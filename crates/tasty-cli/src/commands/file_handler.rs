//! `tasty file-handler` subcommand 정의.

use clap::Subcommand;

#[derive(Subcommand)]
pub enum FileHandlerCommands {
    /// Reload `~/.tasty/file-handlers.toml`. host/plugin 항목은 영향 없음.
    Reload,
}
