//! `tasty webhook` subcommand 정의 — 인바운드 웹훅 등록/조회/해제.
//!
//! 웹훅 lifecycle 은 에이전트 작업이라 CLI/IPC 양면 노출(원칙 2). 대상은 opaque
//! id 로 직접 지정, list 는 전 범위 조회(원칙 3 포커스 독립).

use clap::Subcommand;

#[derive(Subcommand)]
pub enum WebhookCommands {
    /// Register an inbound webhook and print its issued URL.
    ///
    /// Provide exactly one of `--handler <id>` (bind a registered hook handler)
    /// or `--sequence <json>` (define an inline IpcSequence). The external HTTP
    /// payload only fills `${body.x}` / `${header.x}` / `${query.x}` value slots.
    Register {
        /// Allowed HTTP methods (repeatable). Defaults to POST.
        #[arg(long = "method", value_name = "METHOD")]
        methods: Vec<String>,
        /// Registered hook handler id to bind (source must accept webhook).
        #[arg(long, conflicts_with = "sequence")]
        handler: Option<String>,
        /// Inline IpcSequence as a JSON array of {"method","params"} objects.
        #[arg(long)]
        sequence: Option<String>,
    },
    /// List all registered webhooks (URL, methods, handler, steps).
    List,
    /// Show a single webhook's details by id.
    Info {
        /// Webhook opaque id (the `/{id}` path segment).
        #[arg(long)]
        id: String,
    },
    /// Unregister a webhook by id; its path returns 404 afterwards.
    Unregister {
        /// Webhook opaque id.
        #[arg(long)]
        id: String,
    },
}
