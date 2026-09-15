//! Resolve the installation root before changing the child's working directory.
use std::{io, process::Command};

use tasty_plugin_manifest::PluginPackage;

pub(super) fn command(package: &PluginPackage) -> io::Result<Command> {
    // Match the parent-home boundary: lexical absolute paths, without requiring
    // existing files or resolving symlinks. Entry syntax and PATH fallback remain
    // owned by PluginPackage; only its directory gains a stable host-CWD anchor.
    let mut installed = package.clone();
    installed.dir = std::path::absolute(&package.dir)?;
    let mut cmd = Command::new(installed.entry_command_path());
    cmd.current_dir(&installed.dir)
        .env("TASTY_PLUGIN_DIR", &installed.dir);
    Ok(cmd)
}

#[cfg(test)]
mod tests;
