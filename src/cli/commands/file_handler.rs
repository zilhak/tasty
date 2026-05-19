//! `tasty file-handler` / `tasty script` subcommand 정의.

use clap::Subcommand;

#[derive(Subcommand)]
pub enum FileHandlerCommands {
    /// Reload `~/.tasty/file-handlers.toml`. host/plugin 항목은 영향 없음.
    Reload,
}

#[derive(Subcommand)]
pub enum ScriptCommands {
    /// Reload `~/.tasty/init.lua`. 기존 hook 등록은 모두 제거되고 새 init.lua 의
    /// 등록만 살아남는다.
    Reload,
}


