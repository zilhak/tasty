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
    /// The listener takes 127.0.0.1 only unless `tasty webhook allow-external on`
    /// was set before this start; then it takes every IPv4 interface and the
    /// printed `127.0.0.1` URL is just the convenient way to curl it, while
    /// whoever can reach the port reaches this webhook. There is no signature
    /// verification of any kind: `--auth-token` is one fixed shared secret,
    /// and with no `--auth-*` the sequence fires for whoever asks. `--persistent` writes that token in plain text into
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
    /// Show the running listener's port, or save the port for the next start.
    ///
    /// With no argument, asks the running Tasty which address the listener
    /// actually opened and how that port was chosen (`argument`, `config` or
    /// `probe`), plus the port saved for the next start. With a port, saves it
    /// to the data folder's webhooks.toml; with `--unset`, removes the saved
    /// port. If Tasty is not running, the file is edited directly and the output
    /// says so. Either change applies from the next start. A saved port is
    /// explicit: if it cannot be opened, Tasty does not start. Without a saved
    /// port and without `--webhook-port`, Tasty tries 28429 and the next 63
    /// ports and takes the first free one.
    Port {
        /// Port to save (1-65535).
        #[arg(value_parser = clap::value_parser!(u16).range(1..))]
        port: Option<u16>,
        /// Remove the saved port so the next start picks a free one.
        #[arg(long, conflicts_with = "port")]
        unset: bool,
    },
    /// Show or change whether other computers can call webhooks.
    ///
    /// Off by default: the listener takes 127.0.0.1 only, so only programs on
    /// this computer reach it. `on` makes the listener take every IPv4
    /// interface (0.0.0.0) from the next start; then whoever can reach the
    /// port reaches the registered webhooks. With no argument, shows the saved
    /// value and the address the running listener actually opened.
    AllowExternal {
        /// `on` or `off`.
        #[arg(value_parser = ["on", "off"])]
        value: Option<String>,
    },
    /// Same as `tasty webhook port`. Kept for scripts written before it.
    Config {
        /// Save the listener port (1-65535). Applies from the next start.
        #[arg(long)]
        port: Option<u16>,
    },
}
