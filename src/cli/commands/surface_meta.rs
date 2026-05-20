//! `tasty surface-meta` subcommand 정의.

use clap::Subcommand;

#[derive(Subcommand)]
pub enum SurfaceMetaCommands {
    /// Set a metadata key-value pair on a surface
    Set {
        /// Key name
        #[arg(long)]
        key: String,
        /// Value
        #[arg(long)]
        value: String,
        /// Surface ID (default: focused)
        #[arg(long)]
        surface: Option<u32>,
    },
    /// Get a metadata value by key
    Get {
        /// Key name
        #[arg(long)]
        key: String,
        /// Surface ID (default: focused)
        #[arg(long)]
        surface: Option<u32>,
    },
    /// Remove a metadata key
    Unset {
        /// Key name
        #[arg(long)]
        key: String,
        /// Surface ID (default: focused)
        #[arg(long)]
        surface: Option<u32>,
    },
    /// List all metadata for a surface
    List {
        /// Surface ID (default: focused)
        #[arg(long)]
        surface: Option<u32>,
    },
}

// ── Shared argument introspection ──
