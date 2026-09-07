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
    ///
    /// The listener binds every interface, not loopback. The printed URL reads
    /// `127.0.0.1` because that is the convenient way to curl it, but whoever
    /// can reach the port reaches this webhook. There is no signature
    /// verification of any kind: `--auth-token` is one fixed shared secret
    /// compared in constant time, and with no `--auth-*` the sequence fires for
    /// whoever asks. `--persistent` writes that token in plain text into
    /// `~/.tasty/webhooks.toml`.
    ///
    /// The HTTP answer is a fixed acknowledgement chosen before the sequence
    /// runs, so `200` says the call matched and was handed off, never that it
    /// worked. A step that fails neither stops the steps after it nor changes
    /// the answer, which leaves a partly applied sequence as an ordinary
    /// outcome; the log is the only place that failure appears.
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
        /// Persist across restarts (default: temporary, dropped on restart).
        #[arg(long)]
        persistent: bool,
        /// Time limit in seconds; the webhook auto-expires after this many
        /// seconds. Mutually exclusive with --count.
        #[arg(long, value_name = "SECS", conflicts_with = "count")]
        ttl_secs: Option<u64>,
        /// Count limit; the webhook auto-destructs after this many accepted
        /// calls. Mutually exclusive with --ttl-secs.
        ///
        /// Accepted means the request matched the path and one of `--method`
        /// and, where auth is set, carried the right token. A method mismatch
        /// (`405`) and a rejected token (`401`) leave the count where it was, so
        /// a sender who does not know the token cannot spend it. What the count
        /// does not measure is whether the sequence then worked: the answer is
        /// already sent by that point and nothing puts a call back.
        #[arg(long, value_name = "N")]
        count: Option<u64>,
        /// Optional auth: where the token is presented. Unset = no auth.
        #[arg(long = "auth-location", value_name = "LOCATION",
              value_parser = ["query", "bearer", "body", "header"], requires = "auth_token")]
        auth_location: Option<String>,
        /// Token location key: query param / body field path / header name.
        /// Required for query|body|header locations (ignored for bearer).
        #[arg(long = "auth-key", value_name = "KEY")]
        auth_key: Option<String>,
        /// Fixed shared token the sender must present to pass auth.
        #[arg(long = "auth-token", value_name = "TOKEN", requires = "auth_location")]
        auth_token: Option<String>,
    },
    /// List all registered webhooks (URL, methods, handler, steps, lifetime).
    List,
    /// Show a single webhook's details by id (incl. remaining count / expiry).
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
    /// Sweep (bulk-remove) all expired webhooks (time-elapsed / count-exhausted).
    ///
    /// Nothing expires on a timer, so an expired webhook stays registered until
    /// something looks at it. A call to its path removes it right then and
    /// answers `410` that once — every later call gets `404` — a restart drops
    /// the expired persistent ones, and this command clears the whole set at
    /// once without waiting for either.
    Sweep,
    /// Get or set the listener bind port (persisted to ~/.tasty/webhooks.toml).
    ///
    /// With no `--port`, prints the active port and bound status. With `--port`,
    /// persists the new port; the listener rebinds only on the next restart.
    /// The port is config-only — tasty never silently binds a fallback port.
    /// The bind address is not configurable and is not loopback: the listener
    /// takes every interface, so this port is the whole of the exposure.
    Config {
        /// Set the listener port (1-65535). Requires restart to take effect.
        #[arg(long)]
        port: Option<u16>,
    },
}
