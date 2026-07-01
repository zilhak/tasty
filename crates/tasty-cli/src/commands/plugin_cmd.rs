//! `tasty plugin ...` + extension + tool subcommand 정의.

use clap::Subcommand;

use super::passkey::PasskeyCommands;
use super::remote_profile::RemoteProfileCommands;

#[derive(Subcommand)]
pub enum PluginCommands {
    /// List installed plugins (id, version, enabled, running).
    List,
    /// Show full manifest, permissions, commands, and runtime state for a plugin.
    Show {
        /// Plugin id.
        id: String,
    },
    /// Install a plugin from a directory containing tasty-plugin.toml.
    Install {
        /// Path to plugin directory (must contain tasty-plugin.toml).
        path: String,
    },
    /// Remove an installed plugin by id.
    Remove {
        /// Plugin id (e.g. com.example.explorer).
        id: String,
    },
    /// Re-sync all built-in plugins from the bundle. By default only upgrades
    /// plugins whose bundled manifest version is greater than the installed one.
    /// Use --force to overwrite same-or-older versions (recovery scenarios).
    UpgradeBuiltins {
        /// Overwrite even when installed version >= bundle version.
        #[arg(long)]
        force: bool,
        /// Restore specific builtins from `removed_builtins` so they become
        /// eligible for auto-install in the same call. Repeat for multiple ids.
        #[arg(long = "restore-removed", value_name = "ID")]
        restore_removed: Vec<String>,
        /// Restore ALL builtins from `removed_builtins`. Overrides
        /// `--restore-removed` when both are given.
        #[arg(long = "restore-removed-all")]
        restore_all: bool,
        /// Restart running builtin processes whose binary is replaced — graceful
        /// swap. Also unblocks Windows sharing violation on in-place overwrite.
        /// Default off (conservative — briefly closes affected plugin surfaces).
        #[arg(long = "restart-running")]
        restart_running: bool,
    },
    /// Enable a disabled plugin and start it.
    Enable { id: String },
    /// Disable a plugin (graceful shutdown if running).
    Disable { id: String },
    /// Print the contents of a plugin's log file.
    Logs {
        /// Plugin id.
        id: String,
        /// Tail and follow new output (Ctrl-C to stop).
        #[arg(long)]
        follow: bool,
    },
    /// Diagnose a plugin's manifest — list contributed detectors / handlers and
    /// flag rule kinds the current host does not understand.
    Doctor {
        /// Plugin id (e.g. com.example.foo).
        id: String,
    },
    /// Show a plugin's manifest permissions and currently granted set.
    Permissions {
        /// Plugin id.
        id: String,
    },
    /// Grant a permission to a plugin (must be declared in its manifest).
    Grant {
        /// Plugin id.
        id: String,
        /// Permission token (e.g. fs.read, surface.write).
        permission: String,
    },
    /// Revoke a previously-granted permission from a plugin.
    Revoke {
        /// Plugin id.
        id: String,
        /// Permission token.
        permission: String,
    },
    /// Grant a temporary permission to an active agent session.
    /// Permission lives until TTL expiry or explicit revoke; base permissions
    /// (issued at session.issue) are unaffected.
    GrantAgentPermission {
        /// Agent id (e.g. `claude:child-1`).
        #[arg(long)]
        agent: String,
        /// Permission token (e.g. fs.write, surface.write).
        #[arg(long)]
        permission: String,
        /// Time-to-live in seconds. Omit for indefinite (until revoke).
        #[arg(long)]
        ttl: Option<u64>,
    },
    /// Revoke a previously-granted temporary permission from an agent session.
    /// Does not affect base permissions assigned at issue time.
    RevokeAgentPermission {
        /// Agent id.
        #[arg(long)]
        agent: String,
        /// Permission token.
        #[arg(long)]
        permission: String,
    },
    /// List base + temporary permissions for active agent sessions.
    ListAgentPermissions {
        /// Filter to a specific agent. Omit to list all active sessions.
        #[arg(long)]
        agent: Option<String>,
    },
    /// Manually publish a capability elevation approval. Useful for operators
    /// pre-granting a permission before the agent's first call. The popup is
    /// the same one shown by automatic elevation on permission_denied.
    RequestPermission {
        /// Agent id to grant on approval.
        #[arg(long)]
        agent: String,
        /// Permission token (e.g. fs.write).
        #[arg(long)]
        permission: String,
        /// Reason shown to the user in the popup body.
        #[arg(long)]
        reason: Option<String>,
    },
    /// Inspect plugin extensions (extends-blocks).
    Extension {
        #[command(subcommand)]
        command: ExtensionCommands,
    },
    /// Query the IPC audit log. Returns filtered records with allow/deny decisions.
    AuditQuery {
        /// Filter by caller kind: local | internal | plugin | agent.
        #[arg(long)]
        caller_kind: Option<String>,
        /// Filter by caller id (plugin id or agent id).
        #[arg(long)]
        caller_id: Option<String>,
        /// Filter to methods starting with this prefix (e.g. `surface.`).
        #[arg(long)]
        method_prefix: Option<String>,
        /// Filter by decision: allow | deny.
        #[arg(long)]
        decision: Option<String>,
        /// Lower bound on timestamp (unix ms, inclusive).
        #[arg(long)]
        since_ms: Option<u64>,
        /// Upper bound on timestamp (unix ms, exclusive).
        #[arg(long)]
        until_ms: Option<u64>,
        /// Cap on number of records returned.
        #[arg(long)]
        limit: Option<u64>,
    },
    /// Aggregate audit records into totals + top callers/methods.
    AuditSummary {
        /// Filter by caller kind: local | internal | plugin | agent.
        #[arg(long)]
        caller_kind: Option<String>,
        /// Filter by caller id.
        #[arg(long)]
        caller_id: Option<String>,
        /// Filter to methods starting with this prefix.
        #[arg(long)]
        method_prefix: Option<String>,
        /// Filter by decision: allow | deny.
        #[arg(long)]
        decision: Option<String>,
        /// Lower bound on timestamp (unix ms, inclusive).
        #[arg(long)]
        since_ms: Option<u64>,
        /// Upper bound on timestamp (unix ms, exclusive).
        #[arg(long)]
        until_ms: Option<u64>,
        /// Top-N cap for by_caller / by_method lists. Default: 10.
        #[arg(long)]
        top_n: Option<u64>,
    },
    /// Delete audit records older than `before_ms`, or all if omitted.
    AuditClear {
        /// Delete records with ts < before_ms. Omit to clear everything.
        #[arg(long)]
        before_ms: Option<u64>,
    },
    /// Tail audit records as they arrive. Polls every `interval_ms` ms until
    /// Ctrl-C. Filters are the same as `audit-query`.
    AuditFollow {
        /// Filter by caller kind: local | internal | plugin | agent.
        #[arg(long)]
        caller_kind: Option<String>,
        /// Filter by caller id.
        #[arg(long)]
        caller_id: Option<String>,
        /// Filter to methods starting with this prefix.
        #[arg(long)]
        method_prefix: Option<String>,
        /// Filter by decision: allow | deny.
        #[arg(long)]
        decision: Option<String>,
        /// Per-poll cap on record batch size.
        #[arg(long, default_value_t = 100)]
        batch: u64,
        /// Polling interval in milliseconds.
        #[arg(long, default_value_t = 500)]
        interval_ms: u64,
    },
}

#[derive(Subcommand)]
pub enum ExtensionCommands {
    /// List all extensions and their current state.
    List,
}

#[derive(Subcommand)]
// reason: clap 파싱 1회용 enum — Box 화는 derive(Subcommand) 와 충돌하고 런타임 이득 없음.
#[allow(clippy::large_enum_variant)]
pub enum ToolCommands {
    /// 저장된 ssh 프로필로 대화형 ssh 접속을 띄운다(identity/port 자동 주입). 로컬 (no IPC).
    Ssh {
        /// 접속할 ssh 프로필 name.
        profile: String,
        /// 원격에서 1회 실행할 명령(비우면 대화형 셸). 예: `tasty tool ssh gb10 --command hostname`.
        #[arg(long = "command", num_args = 1.., allow_hyphen_values = true)]
        command: Vec<String>,
    },
    /// 원격 접속 프로필 통합 CRUD (ssh + tasty-attach kind). 로컬 파일 (no IPC).
    RemoteProfile {
        #[command(subcommand)]
        command: RemoteProfileCommands,
    },
    /// tasty-attach 프로필로 원격 surface/workspace 에 attach 한다. `--list` = 목록만. 로컬.
    Attach {
        /// attach 대상 tasty-attach 프로필 name(생략+`--list` 면 목록만).
        name: Option<String>,
        /// attach 할 원격 surface id.
        surface: Option<u32>,
        /// attach 할 원격 workspace id(surface 와 배타).
        #[arg(long)]
        workspace: Option<u32>,
        /// attach 직후 1회 비대화형 입력(escape 디코딩).
        #[arg(long)]
        send: Option<String>,
        /// workspace 모드에서 --send 를 보낼 대상 surface id.
        #[arg(long)]
        send_to: Option<u32>,
        /// mirror-dump 수집 시간(ms). 생략 시 기본.
        #[arg(long)]
        dump_after: Option<u64>,
        /// raw stdin↔stdout 브리지(surface 전용).
        #[arg(long)]
        raw: bool,
        /// 연결 끊김 시 자동 재연결 비활성.
        #[arg(long)]
        no_reconnect: bool,
        /// tasty-attach kind 프로필 목록만 출력하고 종료(attach 안 함).
        #[arg(long)]
        list: bool,
    },
    /// Passkeys (`~/.tasty/passkeys.toml`) — 프로필이 이름으로 참조하는 자격증명 저장소.
    /// list/show 는 값을 노출하지 않는다. 로컬 파일 (no IPC).
    Passkey {
        #[command(subcommand)]
        command: PasskeyCommands,
    },
}
