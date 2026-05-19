//! `tasty output ...` subcommand 정의 — Output + Observe.

use clap::Subcommand;

#[derive(Subcommand)]
pub enum OutputCommands {
    /// Observer management subcommands
    Observe {
        #[command(subcommand)]
        command: OutputObserveCommands,
    },
}

#[derive(Subcommand)]
pub enum OutputObserveCommands {
    /// Register a new observer
    Start {
        /// Surface ID to watch (omit = all surfaces)
        #[arg(long)]
        surface: Option<u32>,
        /// Comma-separated parser ids (default: path,url,prompt_boundary,exit_code)
        #[arg(long, value_delimiter = ',')]
        parsers: Option<Vec<String>>,
        /// Comma-separated kind filter (default: all)
        #[arg(long, value_delimiter = ',')]
        kinds: Option<Vec<String>>,
        /// Sink type: "memory" or "file" (default: memory)
        #[arg(long, default_value = "memory")]
        sink: String,
        /// File path (only when --sink file; omit = ~/.tasty/observers/<id>.jsonl)
        #[arg(long)]
        path: Option<String>,
        /// Memory sink ring-buffer cap (only when --sink memory; 0 = unlimited)
        #[arg(long, default_value_t = 10_000)]
        max_records: usize,
    },
    /// Stop an observer by id
    Stop {
        /// Observer id (from `output observe list`)
        #[arg(long)]
        observer: u64,
    },
    /// List all active observers
    List,
    /// Show stats for a single observer
    Info {
        #[arg(long)]
        observer: u64,
    },
}



