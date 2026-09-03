//! `tasty tool remote-profile ...` — 원격 접속 프로필 통합 CRUD 의 **clap 선언**.
//!
//! 실행은 [`crate::local::remote_profile`] 이 한다(로컬 파일, IPC 미경유).
//!
//! 2-레이어 모델(ADR-0032): **ssh** = 순수 연결 정보(host/user/port/identity/options/
//! shell), **tasty-attach** = attach 스펙(ssh_ref 참조 또는 인라인 연결 + remote_tasty/
//! port_mode/port_file). attach 동작 자체는 `tasty tool attach` 에서 tasty-attach
//! 프로필을 소비한다.

use clap::Subcommand;

#[derive(Subcommand)]
pub enum RemoteProfileCommands {
    /// List saved profiles (ssh and tasty-attach).
    List {
        #[arg(long)]
        json: bool,
        /// Filter by kind: ssh | tasty-attach.
        #[arg(long)]
        kind: Option<String>,
    },
    /// Show the details of one profile.
    Show {
        #[arg(long)]
        name: String,
        #[arg(long)]
        json: bool,
    },
    /// Add an ssh connection profile (connection details only, no attach spec).
    AddSsh {
        /// Unique profile identifier.
        #[arg(long)]
        name: String,
        /// ssh destination: host | user@host | ssh config alias.
        #[arg(long)]
        host: String,
        /// ssh user (when host has no user@ prefix).
        #[arg(long)]
        user: Option<String>,
        /// ssh port (default: from ssh config, else 22).
        #[arg(long)]
        port: Option<u16>,
        /// Identity file path (-i). Stored separately as the path-kind passkey
        /// `<name>-key`.
        #[arg(long)]
        identity: Option<String>,
        /// Extra ssh -o option (repeatable), e.g. --option ServerAliveInterval=30.
        #[arg(long = "option")]
        options: Vec<String>,
        /// Remote shell: powershell | cmd | bash | zsh | auto (default).
        #[arg(long, default_value = "auto")]
        shell: String,
        /// Optional label shown in the UI.
        #[arg(long)]
        label: Option<String>,
    },
    /// Add a tasty-attach profile. The connection is either a reference
    /// (`--ssh-ref <name>`) or inline fields (host, ...).
    AddAttach {
        /// Unique profile identifier.
        #[arg(long)]
        name: String,
        /// Name of the ssh profile to reference (followed live). When set, the
        /// inline connection fields are ignored.
        #[arg(long = "ssh-ref")]
        ssh_ref: Option<String>,
        /// Inline connection: ssh destination (host | user@host | alias). Used
        /// when `--ssh-ref` is absent.
        #[arg(long)]
        host: Option<String>,
        /// Inline connection: ssh user.
        #[arg(long)]
        user: Option<String>,
        /// Inline connection: ssh port.
        #[arg(long)]
        port: Option<u16>,
        /// Inline connection: identity file path (-i). Stored separately as a
        /// path-kind passkey.
        #[arg(long)]
        identity: Option<String>,
        /// Inline connection: extra ssh -o option (repeatable).
        #[arg(long = "option")]
        options: Vec<String>,
        /// Path to the tasty binary on the remote host (for port discovery).
        /// Defaults to "tasty".
        #[arg(long, default_value = "tasty")]
        remote_tasty: String,
        /// Remote port discovery mode: auto (default) | subcommand | file-unix |
        /// file-windows.
        #[arg(long, default_value = "auto")]
        port_mode: String,
        /// Explicit path of the remote port file (non-standard location). Takes
        /// precedence over the conventional path when set.
        #[arg(long)]
        port_file: Option<String>,
        /// Optional label shown in the UI.
        #[arg(long)]
        label: Option<String>,
    },
    /// Update fields of an existing profile (only the given fields are
    /// overwritten). The kind is kept.
    Edit {
        #[arg(long)]
        name: String,
        #[arg(long)]
        host: Option<String>,
        #[arg(long)]
        user: Option<String>,
        #[arg(long)]
        port: Option<u16>,
        #[arg(long)]
        identity: Option<String>,
        #[arg(long = "option")]
        options: Vec<String>,
        /// tasty-attach: ssh profile to reference instead (by name).
        #[arg(long = "ssh-ref")]
        ssh_ref: Option<String>,
        /// tasty-attach: path to the tasty binary on the remote host.
        #[arg(long)]
        remote_tasty: Option<String>,
        /// tasty-attach: remote port discovery mode.
        #[arg(long)]
        port_mode: Option<String>,
        /// tasty-attach: remote port file path.
        #[arg(long)]
        port_file: Option<String>,
        /// ssh: remote shell (powershell | cmd | bash | zsh | auto).
        #[arg(long)]
        shell: Option<String>,
        #[arg(long)]
        label: Option<String>,
    },
    /// Remove a profile (referenced passkeys are kept, since they may be shared).
    Remove {
        #[arg(long)]
        name: String,
    },
    /// Re-detect a profile (ssh: probe the remote shell / tasty-attach: verify
    /// the remote port). Connects over SSH.
    Detect {
        #[arg(long)]
        name: String,
    },
    /// List Host aliases from the local ssh config (`~/.ssh/config` plus
    /// Include files). Does not connect.
    ListLocal {
        #[arg(long)]
        json: bool,
    },
    /// Import a local ssh config alias as an ssh profile (only the alias is
    /// stored; ssh resolves the actual values).
    Import {
        /// ssh config alias to import (the ALIAS column of `list-local`).
        #[arg(long)]
        from: String,
        /// Unique identifier of the new profile.
        #[arg(long)]
        name: String,
        /// Optional label shown in the UI.
        #[arg(long)]
        label: Option<String>,
    },
}
