//! Plugin 매니페스트 정의 + 파싱 + 검증.
//!
//! `~/.tasty/plugins/<plugin-id>/tasty-plugin.toml` 형식.
//!
//! 일부 필드(authors/homepage/contributes/icon 등)는 deserialize surface로 정의돼
//! 있지만 호스트 본문이 아직 모두 활용하지는 않는다 — 매니페스트 schema를 한 곳에서
//! 정확히 표현하기 위해 의도적으로 남겨둔다.

#![allow(dead_code)]

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use serde::Deserialize;

/// 호스트가 지원하는 plugin protocol 메이저 버전.
/// plugin 매니페스트의 `api_version`과 일치해야 한다.
pub const HOST_API_VERSION: &str = "1";

/// 매니페스트 스키마 버전 (이 파일 형식 자체의 버전).
pub const MANIFEST_VERSION: u32 = 1;

#[derive(Debug, Clone, Deserialize)]
pub struct Manifest {
    pub manifest_version: u32,
    pub id: String,
    pub name: String,
    pub version: String,
    #[serde(default)]
    pub authors: Vec<String>,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub homepage: String,
    pub api_version: String,
    pub entry: Entry,
    #[serde(default)]
    pub surface_kinds: Vec<SurfaceKindDecl>,
    #[serde(default)]
    pub permissions: Vec<String>,
    #[serde(default)]
    pub contributes: Contributes,
    /// plugin이 동봉한 lang 파일 디렉터리 (매니페스트 디렉터리 기준 상대).
    /// 기본 `"lang"`. 호스트는 `<plugin_dir>/<lang_dir>/<locale>.toml`을 i18n
    /// registry에 머지한다.
    #[serde(default = "default_lang_dir")]
    pub lang_dir: String,
}

fn default_lang_dir() -> String {
    "lang".to_string()
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type")]
pub enum Entry {
    #[serde(rename = "process")]
    Process {
        command: String,
        #[serde(default)]
        args: Vec<String>,
    },
    // 향후 옵션:
    // #[serde(rename = "wasm")] Wasm { module: String },
}

/// Plugin이 매니페스트에 선언할 수 있는 권한 카테고리.
///
/// 평면 enum — `fs.write`는 `fs.read`를 자동 포함하지 않는다.
/// 매니페스트에 두 권한이 모두 필요하면 명시적으로 선언해야 한다.
#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash)]
pub enum Permission {
    /// surface/tab/workspace 트리 조회
    SurfaceRead,
    /// surface 생성/닫기/이동
    SurfaceWrite,
    /// 알림 생성/관리
    Notification,
    /// 클립보드 읽기
    ClipboardRead,
    /// 클립보드 쓰기
    ClipboardWrite,
    /// 호스트 노출 fs 읽기
    FsRead,
    /// 파일 쓰기
    FsWrite,
    /// 외부 프로세스 실행
    ProcessSpawn,
    /// 새 터미널 surface 생성
    TerminalSpawn,
    /// 터미널 입력 송신
    TerminalWrite,
    /// 터미널 출력/scrollback 읽기
    TerminalRead,
    /// 호스트를 통한 네트워크 (예약)
    Network,
    /// Claude 세션 메타데이터 조회 (예약)
    ClaudeRead,
    /// Claude API 호출 위임 (예약)
    ClaudeInvoke,
}

impl Permission {
    pub fn from_token(s: &str) -> Option<Self> {
        Some(match s {
            "surface.read" => Self::SurfaceRead,
            "surface.write" => Self::SurfaceWrite,
            "notification" => Self::Notification,
            "clipboard.read" => Self::ClipboardRead,
            "clipboard.write" => Self::ClipboardWrite,
            "fs.read" => Self::FsRead,
            "fs.write" => Self::FsWrite,
            "process.spawn" => Self::ProcessSpawn,
            "terminal.spawn" => Self::TerminalSpawn,
            "terminal.write" => Self::TerminalWrite,
            "terminal.read" => Self::TerminalRead,
            "network" => Self::Network,
            "claude.read" => Self::ClaudeRead,
            "claude.invoke" => Self::ClaudeInvoke,
            _ => return None,
        })
    }

    pub fn as_token(self) -> &'static str {
        match self {
            Self::SurfaceRead => "surface.read",
            Self::SurfaceWrite => "surface.write",
            Self::Notification => "notification",
            Self::ClipboardRead => "clipboard.read",
            Self::ClipboardWrite => "clipboard.write",
            Self::FsRead => "fs.read",
            Self::FsWrite => "fs.write",
            Self::ProcessSpawn => "process.spawn",
            Self::TerminalSpawn => "terminal.spawn",
            Self::TerminalWrite => "terminal.write",
            Self::TerminalRead => "terminal.read",
            Self::Network => "network",
            Self::ClaudeRead => "claude.read",
            Self::ClaudeInvoke => "claude.invoke",
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct SurfaceKindDecl {
    pub kind: String,
    pub display_name_i18n_key: String,
    #[serde(default)]
    pub icon: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct Contributes {
    #[serde(default)]
    pub commands: Vec<CommandDecl>,
    #[serde(default)]
    pub menu_items: Vec<MenuItemDecl>,
    #[serde(default)]
    pub ipc_namespace: Vec<IpcNamespaceDecl>,
    #[serde(default)]
    pub cli: Vec<CliCommandDecl>,
}

/// Plugin이 점유할 IPC 메서드 namespace prefix.
///
/// 호스트는 `<prefix>.*` 패턴의 모든 IPC 메서드를 등록된 plugin에 forward한다.
/// 예: prefix="codex" → "codex.spawn", "codex.wait" 등을 모두 그 plugin이 처리.
#[derive(Debug, Clone, Deserialize)]
pub struct IpcNamespaceDecl {
    pub prefix: String,
    #[serde(default)]
    pub description_i18n_key: Option<String>,
}

/// Plugin이 contributes하는 최상위 CLI 명령. `tasty <name> <sub>` 형태로 노출된다.
#[derive(Debug, Clone, Deserialize)]
pub struct CliCommandDecl {
    pub name: String,
    #[serde(default)]
    pub description_i18n_key: Option<String>,
    #[serde(default)]
    pub subcommands: Vec<CliSubcommandDecl>,
    /// arg group 이름 → 정의. subcommand가 `args = "<key>"`로 참조한다.
    #[serde(default)]
    pub arg_groups: HashMap<String, CliArgGroup>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CliSubcommandDecl {
    pub name: String,
    /// 이 서브커맨드가 호출할 IPC 메서드 (예: "codex.spawn").
    /// plugin 자기 namespace prefix로 시작해야 한다.
    pub ipc_method: String,
    /// `arg_groups`의 키. 비어있는 그룹이라도 명시적으로 가리켜야 한다.
    pub args: String,
    #[serde(default)]
    pub description_i18n_key: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct CliArgGroup {
    #[serde(default)]
    pub positional: Vec<CliArg>,
    #[serde(default)]
    pub flags: Vec<CliArg>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CliArg {
    pub name: String,
    #[serde(rename = "type")]
    pub ty: CliArgType,
    /// `flags`에 들어가는 인자에만 존재. `positional`에서는 None.
    #[serde(default)]
    pub flag: Option<String>,
    #[serde(default)]
    pub required: bool,
    #[serde(default)]
    pub default: Option<toml::Value>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CliArgType {
    U32,
    I64,
    String,
    Bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CommandDecl {
    pub id: String,
    pub title_i18n_key: String,
    #[serde(default)]
    pub default_keybinding: Option<String>,
    /// 단축키를 호스트의 의미론적 액션과 어떻게 묶을지 plugin 작성자가 선언한다.
    /// `"independent"` (기본) 또는 `"inherit:<host_action>"`.
    #[serde(default)]
    pub binding_mode: BindingMode,
}

/// command가 호스트 액션 키와 어떤 관계를 갖는지.
///
/// - `Independent`: plugin 자체 키. 사용자가 설정에서 자유롭게 변경 가능.
/// - `InheritHost(action)`: 호스트의 의미론적 액션(예: `"clipboard.copy"`)
///   키 설정을 그대로 따라감. 사용자가 설정 UI에서 떼어내 독립 키로 만들 수 있다.
///
/// TOML 표기: `binding_mode = "independent"` 또는
/// `binding_mode = "inherit:clipboard.copy"`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BindingMode {
    Independent,
    InheritHost(String),
}

impl Default for BindingMode {
    fn default() -> Self {
        BindingMode::Independent
    }
}

impl<'de> Deserialize<'de> for BindingMode {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        if s == "independent" {
            return Ok(BindingMode::Independent);
        }
        if let Some(action) = s.strip_prefix("inherit:") {
            let action = action.trim();
            if action.is_empty() {
                return Err(serde::de::Error::custom(
                    "binding_mode 'inherit:' must be followed by a host action id",
                ));
            }
            return Ok(BindingMode::InheritHost(action.to_string()));
        }
        Err(serde::de::Error::custom(format!(
            "invalid binding_mode '{}': expected 'independent' or 'inherit:<host_action>'",
            s
        )))
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct MenuItemDecl {
    pub menu: String,
    pub command: String,
    #[serde(default)]
    pub when: Option<String>,
}

impl Manifest {
    pub fn load(dir: &Path) -> anyhow::Result<Self> {
        let path = dir.join("tasty-plugin.toml");
        let s = std::fs::read_to_string(&path)
            .map_err(|e| anyhow::anyhow!("cannot read {}: {}", path.display(), e))?;
        let manifest: Manifest = toml::from_str(&s)
            .map_err(|e| anyhow::anyhow!("invalid manifest at {}: {}", path.display(), e))?;
        manifest.validate()?;
        Ok(manifest)
    }

    pub fn validate(&self) -> anyhow::Result<()> {
        if self.manifest_version != MANIFEST_VERSION {
            anyhow::bail!(
                "unsupported manifest_version: {} (expected {})",
                self.manifest_version,
                MANIFEST_VERSION
            );
        }
        if self.api_version != HOST_API_VERSION {
            anyhow::bail!(
                "plugin api_version '{}' incompatible with host '{}'",
                self.api_version,
                HOST_API_VERSION
            );
        }
        if !is_valid_plugin_id(&self.id) {
            anyhow::bail!(
                "invalid plugin id: '{}' (must be lowercase reverse-domain like com.example.x)",
                self.id
            );
        }
        for kind in &self.surface_kinds {
            if !is_valid_kind(&kind.kind) {
                anyhow::bail!(
                    "invalid surface kind: '{}' (must be lowercase ascii + '_' + digits)",
                    kind.kind
                );
            }
        }
        for raw in &self.permissions {
            if Permission::from_token(raw).is_none() {
                anyhow::bail!(
                    "unknown permission '{}' in manifest (host may be older than plugin)",
                    raw
                );
            }
        }
        self.validate_contributes()?;
        Ok(())
    }

    fn validate_contributes(&self) -> anyhow::Result<()> {
        let mut seen_prefixes = HashSet::new();
        for ns in &self.contributes.ipc_namespace {
            if !is_valid_ipc_prefix(&ns.prefix) {
                anyhow::bail!(
                    "invalid ipc_namespace prefix '{}': must be lowercase ascii + digits + '_', \
                     start with a letter, length ≤ 32, no '.'",
                    ns.prefix
                );
            }
            if is_reserved_ipc_prefix(&ns.prefix) {
                anyhow::bail!(
                    "ipc_namespace prefix '{}' is reserved by the host",
                    ns.prefix
                );
            }
            if !seen_prefixes.insert(ns.prefix.clone()) {
                anyhow::bail!(
                    "ipc_namespace prefix '{}' declared twice in this manifest",
                    ns.prefix
                );
            }
        }

        let mut seen_cli_names = HashSet::new();
        for cli in &self.contributes.cli {
            if !is_valid_cli_name(&cli.name) {
                anyhow::bail!(
                    "invalid cli name '{}': must be lowercase ascii + digits + '-', \
                     start with a letter, length ≤ 32",
                    cli.name
                );
            }
            if is_reserved_cli_name(&cli.name) {
                anyhow::bail!("cli name '{}' is reserved by the host", cli.name);
            }
            if !seen_cli_names.insert(cli.name.clone()) {
                anyhow::bail!(
                    "cli name '{}' declared twice in this manifest",
                    cli.name
                );
            }

            let mut seen_sub_names = HashSet::new();
            for sub in &cli.subcommands {
                if !is_valid_cli_name(&sub.name) {
                    anyhow::bail!(
                        "invalid cli subcommand name '{}' under '{}'",
                        sub.name,
                        cli.name
                    );
                }
                if !seen_sub_names.insert(sub.name.clone()) {
                    anyhow::bail!(
                        "cli subcommand name '{}' declared twice under '{}'",
                        sub.name,
                        cli.name
                    );
                }
                if !cli.arg_groups.contains_key(&sub.args) {
                    anyhow::bail!(
                        "cli subcommand '{} {}' references unknown arg group '{}'",
                        cli.name,
                        sub.name,
                        sub.args
                    );
                }
                // ipc_method는 plugin 자기 namespace로 시작해야 한다.
                let Some(dot) = sub.ipc_method.find('.') else {
                    anyhow::bail!(
                        "cli subcommand '{} {}' ipc_method '{}' has no namespace prefix",
                        cli.name,
                        sub.name,
                        sub.ipc_method
                    );
                };
                let prefix = &sub.ipc_method[..dot];
                if !seen_prefixes.contains(prefix) {
                    anyhow::bail!(
                        "cli subcommand '{} {}' ipc_method '{}' uses prefix '{}' \
                         which is not declared in this plugin's ipc_namespace",
                        cli.name,
                        sub.name,
                        sub.ipc_method,
                        prefix
                    );
                }
            }

            // arg group 내부 정합성: flag는 flags에만, positional은 flag 필드 없음.
            for (group_name, group) in &cli.arg_groups {
                for arg in &group.positional {
                    if arg.flag.is_some() {
                        anyhow::bail!(
                            "arg group '{}.{}' positional arg '{}' must not have a 'flag' field",
                            cli.name,
                            group_name,
                            arg.name
                        );
                    }
                }
                for arg in &group.flags {
                    let Some(flag) = &arg.flag else {
                        anyhow::bail!(
                            "arg group '{}.{}' flag arg '{}' is missing 'flag' field",
                            cli.name,
                            group_name,
                            arg.name
                        );
                    };
                    if !flag.starts_with("--") {
                        anyhow::bail!(
                            "arg group '{}.{}' flag '{}' must start with '--'",
                            cli.name,
                            group_name,
                            flag
                        );
                    }
                }
            }
        }
        Ok(())
    }

    /// 매니페스트에 선언된 권한을 파싱한 set으로 반환.
    /// `validate()`가 통과한 매니페스트에 대해 호출되면 절대 실패하지 않는다.
    pub fn parsed_permissions(&self) -> anyhow::Result<HashSet<Permission>> {
        let mut out = HashSet::with_capacity(self.permissions.len());
        for raw in &self.permissions {
            match Permission::from_token(raw) {
                Some(p) => {
                    out.insert(p);
                }
                None => anyhow::bail!("unknown permission '{}'", raw),
            }
        }
        Ok(out)
    }
}

fn is_valid_plugin_id(s: &str) -> bool {
    !s.is_empty()
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-' || c == '_')
        && s.contains('.')
}

fn is_valid_kind(s: &str) -> bool {
    !s.is_empty()
        && s.chars()
            .all(|c| c.is_ascii_lowercase() || c == '_' || c.is_ascii_digit())
}

/// IPC namespace prefix 형식 검증.
/// 소문자 ascii + 숫자 + `_`. 알파벳으로 시작. 길이 1..=32. `.` 포함 불가.
fn is_valid_ipc_prefix(s: &str) -> bool {
    if s.is_empty() || s.len() > 32 {
        return false;
    }
    let mut chars = s.chars();
    let first = chars.next().unwrap();
    if !first.is_ascii_lowercase() {
        return false;
    }
    s.chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

/// 호스트가 자기 IPC 메서드에 쓰는 prefix들. plugin이 점유하면 호스트 메서드가 가려진다.
fn is_reserved_ipc_prefix(s: &str) -> bool {
    matches!(
        s,
        "claude"
            | "plugin"
            | "system"
            | "surface"
            | "tab"
            | "pane"
            | "workspace"
            | "split"
            | "tree"
            | "hook"
            | "global_hook"
            | "message"
            | "tool"
            | "notification"
            | "window"
            | "debug"
            | "ui"
            | "ime"
            | "ipc"
    )
}

/// CLI 명령 이름 형식 검증.
/// 소문자 ascii + 숫자 + `-`. 알파벳으로 시작. 길이 1..=32.
fn is_valid_cli_name(s: &str) -> bool {
    if s.is_empty() || s.len() > 32 {
        return false;
    }
    let first = s.chars().next().unwrap();
    if !first.is_ascii_lowercase() {
        return false;
    }
    s.chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

/// 호스트가 자기 CLI 서브커맨드로 쓰는 명령들. plugin이 가로채면 호스트가 가려진다.
fn is_reserved_cli_name(s: &str) -> bool {
    matches!(
        s,
        "claude"
            | "plugin"
            | "new"
            | "close"
            | "list"
            | "set"
            | "send"
            | "read"
            | "move"
            | "split"
            | "tree"
            | "debug"
            | "wait"
            | "send-key"
            | "send-combo"
            | "surface-meta"
            | "is-typing"
            | "notify"
            | "unset"
    )
}

/// 매니페스트가 들어 있는 디렉터리 핸들 — 디렉터리 + 파싱된 매니페스트 묶음.
#[derive(Debug, Clone)]
pub struct PluginPackage {
    pub dir: PathBuf,
    pub manifest: Manifest,
}

impl PluginPackage {
    /// 실행할 entry binary의 경로. 매니페스트 디렉터리 기준 상대 경로면
    /// 디렉터리에 합쳐서 반환. 절대 경로 또는 PATH 의존이면 그대로.
    pub fn entry_command_path(&self) -> PathBuf {
        match &self.manifest.entry {
            Entry::Process { command, .. } => {
                let p = Path::new(command);
                if p.is_absolute() {
                    p.to_path_buf()
                } else {
                    let candidate = self.dir.join(command);
                    if candidate.exists() {
                        candidate
                    } else {
                        p.to_path_buf()
                    }
                }
            }
        }
    }

    pub fn entry_args(&self) -> Vec<String> {
        match &self.manifest.entry {
            Entry::Process { args, .. } => args.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(src: &str) -> anyhow::Result<Manifest> {
        let m: Manifest = toml::from_str(src)?;
        m.validate()?;
        Ok(m)
    }

    #[test]
    fn rejects_unsupported_api() {
        let s = r#"
            manifest_version = 1
            id = "com.example.x"
            name = "X"
            version = "0.1"
            api_version = "999"
            [entry]
            type = "process"
            command = "x"
        "#;
        assert!(parse(s).is_err());
    }

    #[test]
    fn rejects_unsupported_manifest_version() {
        let s = r#"
            manifest_version = 99
            id = "com.example.x"
            name = "X"
            version = "0.1"
            api_version = "1"
            [entry]
            type = "process"
            command = "x"
        "#;
        assert!(parse(s).is_err());
    }

    #[test]
    fn rejects_invalid_plugin_id_no_dot() {
        let s = r#"
            manifest_version = 1
            id = "explorer"
            name = "X"
            version = "0.1"
            api_version = "1"
            [entry]
            type = "process"
            command = "x"
        "#;
        assert!(parse(s).is_err());
    }

    #[test]
    fn rejects_invalid_kind_uppercase() {
        let s = r#"
            manifest_version = 1
            id = "com.example.x"
            name = "X"
            version = "0.1"
            api_version = "1"
            [entry]
            type = "process"
            command = "x"
            [[surface_kinds]]
            kind = "Explorer"
            display_name_i18n_key = "k"
        "#;
        assert!(parse(s).is_err());
    }

    #[test]
    fn accepts_minimal_valid() {
        let s = r#"
            manifest_version = 1
            id = "com.example.x"
            name = "X"
            version = "0.1.0"
            api_version = "1"
            [entry]
            type = "process"
            command = "x"
        "#;
        let m = parse(s).expect("should parse");
        assert_eq!(m.id, "com.example.x");
    }

    #[test]
    fn rejects_unknown_permission() {
        let s = r#"
            manifest_version = 1
            id = "com.example.x"
            name = "X"
            version = "0.1"
            api_version = "1"
            permissions = ["fs.read", "made.up.permission"]
            [entry]
            type = "process"
            command = "x"
        "#;
        let err = parse(s).unwrap_err().to_string();
        assert!(err.contains("unknown permission"), "got: {err}");
    }

    #[test]
    fn parsed_permissions_returns_enum_set() {
        let s = r#"
            manifest_version = 1
            id = "com.example.x"
            name = "X"
            version = "0.1"
            api_version = "1"
            permissions = ["fs.read", "surface.write", "notification"]
            [entry]
            type = "process"
            command = "x"
        "#;
        let m = parse(s).expect("should parse");
        let perms = m.parsed_permissions().expect("should resolve");
        assert!(perms.contains(&Permission::FsRead));
        assert!(perms.contains(&Permission::SurfaceWrite));
        assert!(perms.contains(&Permission::Notification));
        assert_eq!(perms.len(), 3);
    }

    #[test]
    fn accepts_full_manifest() {
        // TOML rule: top-level keys must come before any table headers.
        let s = r#"
            manifest_version = 1
            id = "com.example.explorer"
            name = "Explorer"
            version = "1.2.0"
            authors = ["alice@example.com"]
            description = "File explorer"
            homepage = "https://example.com"
            api_version = "1"
            permissions = ["fs.read", "surface.read"]

            [entry]
            type = "process"
            command = "tasty-plugin-explorer"
            args = []

            [[surface_kinds]]
            kind = "explorer"
            display_name_i18n_key = "surface.kind.explorer"
            icon = "📁"

            [[contributes.commands]]
            id = "explorer.refresh"
            title_i18n_key = "explorer.command.refresh"
            default_keybinding = "F5"
        "#;
        let m = parse(s).expect("should parse");
        assert_eq!(m.surface_kinds.len(), 1);
        assert_eq!(m.surface_kinds[0].kind, "explorer");
        assert_eq!(m.permissions.len(), 2);
        assert_eq!(m.contributes.commands.len(), 1);
        // binding_mode 미지정 → Independent 기본값
        assert_eq!(m.contributes.commands[0].binding_mode, BindingMode::Independent);
        // lang_dir 미지정 → "lang" 기본값
        assert_eq!(m.lang_dir, "lang");
    }

    #[test]
    fn binding_mode_independent() {
        let s = r#"
            manifest_version = 1
            id = "com.example.x"
            name = "X"
            version = "0.1"
            api_version = "1"
            [entry]
            type = "process"
            command = "x"
            [[contributes.commands]]
            id = "x.foo"
            title_i18n_key = "x.foo"
            binding_mode = "independent"
        "#;
        let m = parse(s).expect("should parse");
        assert_eq!(m.contributes.commands[0].binding_mode, BindingMode::Independent);
    }

    #[test]
    fn binding_mode_inherit() {
        let s = r#"
            manifest_version = 1
            id = "com.example.x"
            name = "X"
            version = "0.1"
            api_version = "1"
            [entry]
            type = "process"
            command = "x"
            [[contributes.commands]]
            id = "x.copy"
            title_i18n_key = "x.copy"
            binding_mode = "inherit:clipboard.copy"
        "#;
        let m = parse(s).expect("should parse");
        match &m.contributes.commands[0].binding_mode {
            BindingMode::InheritHost(action) => assert_eq!(action, "clipboard.copy"),
            _ => panic!("expected InheritHost"),
        }
    }

    #[test]
    fn binding_mode_inherit_empty_action_rejected() {
        let s = r#"
            manifest_version = 1
            id = "com.example.x"
            name = "X"
            version = "0.1"
            api_version = "1"
            [entry]
            type = "process"
            command = "x"
            [[contributes.commands]]
            id = "x.copy"
            title_i18n_key = "x.copy"
            binding_mode = "inherit:"
        "#;
        assert!(parse(s).is_err());
    }

    #[test]
    fn binding_mode_unknown_value_rejected() {
        let s = r#"
            manifest_version = 1
            id = "com.example.x"
            name = "X"
            version = "0.1"
            api_version = "1"
            [entry]
            type = "process"
            command = "x"
            [[contributes.commands]]
            id = "x.foo"
            title_i18n_key = "x.foo"
            binding_mode = "wat"
        "#;
        assert!(parse(s).is_err());
    }

    #[test]
    fn manifest_with_ipc_namespace_parses() {
        let s = r#"
            manifest_version = 1
            id = "com.example.codex"
            name = "Codex"
            version = "0.1.0"
            api_version = "1"
            [entry]
            type = "process"
            command = "tasty-plugin-codex"
            [[contributes.ipc_namespace]]
            prefix = "codex"
            description_i18n_key = "codex.namespace.desc"
        "#;
        let m = parse(s).expect("should parse");
        assert_eq!(m.contributes.ipc_namespace.len(), 1);
        assert_eq!(m.contributes.ipc_namespace[0].prefix, "codex");
    }

    #[test]
    fn manifest_with_cli_parses() {
        let s = r#"
            manifest_version = 1
            id = "com.example.codex"
            name = "Codex"
            version = "0.1.0"
            api_version = "1"

            [entry]
            type = "process"
            command = "tasty-plugin-codex"

            [[contributes.ipc_namespace]]
            prefix = "codex"

            [[contributes.cli]]
            name = "codex"
            subcommands = [
              { name = "spawn", ipc_method = "codex.spawn", args = "spawn_args" },
              { name = "wait",  ipc_method = "codex.wait",  args = "no_args" },
            ]

            [contributes.cli.arg_groups.spawn_args]
            flags = [
              { name = "surface", type = "u32",    flag = "--surface", required = false },
              { name = "prompt",  type = "string", flag = "--prompt",  required = false },
            ]

            [contributes.cli.arg_groups.no_args]
        "#;
        let m = parse(s).expect("should parse");
        assert_eq!(m.contributes.cli.len(), 1);
        let cli = &m.contributes.cli[0];
        assert_eq!(cli.name, "codex");
        assert_eq!(cli.subcommands.len(), 2);
        assert!(cli.arg_groups.contains_key("spawn_args"));
        assert!(cli.arg_groups.contains_key("no_args"));
        let spawn_args = &cli.arg_groups["spawn_args"];
        assert_eq!(spawn_args.flags.len(), 2);
        assert_eq!(spawn_args.flags[0].ty, CliArgType::U32);
    }

    #[test]
    fn manifest_reserved_ipc_prefix_rejected() {
        let s = r#"
            manifest_version = 1
            id = "com.example.evil"
            name = "Evil"
            version = "0.1.0"
            api_version = "1"
            [entry]
            type = "process"
            command = "x"
            [[contributes.ipc_namespace]]
            prefix = "surface"
        "#;
        let err = parse(s).unwrap_err().to_string();
        assert!(err.contains("reserved"), "got: {err}");
    }

    #[test]
    fn manifest_reserved_cli_name_rejected() {
        let s = r#"
            manifest_version = 1
            id = "com.example.evil"
            name = "Evil"
            version = "0.1.0"
            api_version = "1"
            [entry]
            type = "process"
            command = "x"
            [[contributes.cli]]
            name = "split"
        "#;
        let err = parse(s).unwrap_err().to_string();
        assert!(err.contains("reserved"), "got: {err}");
    }

    #[test]
    fn manifest_cli_args_ref_missing_rejected() {
        let s = r#"
            manifest_version = 1
            id = "com.example.codex"
            name = "Codex"
            version = "0.1.0"
            api_version = "1"

            [entry]
            type = "process"
            command = "x"

            [[contributes.ipc_namespace]]
            prefix = "codex"

            [[contributes.cli]]
            name = "codex"
            subcommands = [
              { name = "spawn", ipc_method = "codex.spawn", args = "missing" },
            ]
        "#;
        let err = parse(s).unwrap_err().to_string();
        assert!(err.contains("unknown arg group"), "got: {err}");
    }

    #[test]
    fn manifest_ipc_method_outside_namespace_rejected() {
        let s = r#"
            manifest_version = 1
            id = "com.example.codex"
            name = "Codex"
            version = "0.1.0"
            api_version = "1"

            [entry]
            type = "process"
            command = "x"

            [[contributes.ipc_namespace]]
            prefix = "codex"

            [[contributes.cli]]
            name = "codex"
            subcommands = [
              { name = "evil", ipc_method = "claude.spawn", args = "no_args" },
            ]

            [contributes.cli.arg_groups.no_args]
        "#;
        let err = parse(s).unwrap_err().to_string();
        assert!(err.contains("not declared"), "got: {err}");
    }

    #[test]
    fn manifest_cli_ipc_method_no_prefix_rejected() {
        let s = r#"
            manifest_version = 1
            id = "com.example.codex"
            name = "Codex"
            version = "0.1.0"
            api_version = "1"

            [entry]
            type = "process"
            command = "x"

            [[contributes.ipc_namespace]]
            prefix = "codex"

            [[contributes.cli]]
            name = "codex"
            subcommands = [
              { name = "evil", ipc_method = "noprefix", args = "no_args" },
            ]

            [contributes.cli.arg_groups.no_args]
        "#;
        let err = parse(s).unwrap_err().to_string();
        assert!(err.contains("no namespace prefix"), "got: {err}");
    }

    #[test]
    fn manifest_duplicate_ipc_prefix_rejected() {
        let s = r#"
            manifest_version = 1
            id = "com.example.codex"
            name = "Codex"
            version = "0.1.0"
            api_version = "1"
            [entry]
            type = "process"
            command = "x"
            [[contributes.ipc_namespace]]
            prefix = "codex"
            [[contributes.ipc_namespace]]
            prefix = "codex"
        "#;
        let err = parse(s).unwrap_err().to_string();
        assert!(err.contains("declared twice"), "got: {err}");
    }

    #[test]
    fn manifest_flag_without_double_dash_rejected() {
        let s = r#"
            manifest_version = 1
            id = "com.example.codex"
            name = "Codex"
            version = "0.1.0"
            api_version = "1"

            [entry]
            type = "process"
            command = "x"

            [[contributes.ipc_namespace]]
            prefix = "codex"

            [[contributes.cli]]
            name = "codex"
            subcommands = [
              { name = "spawn", ipc_method = "codex.spawn", args = "spawn_args" },
            ]

            [contributes.cli.arg_groups.spawn_args]
            flags = [
              { name = "surface", type = "u32", flag = "-s", required = false },
            ]
        "#;
        let err = parse(s).unwrap_err().to_string();
        assert!(err.contains("must start with '--'"), "got: {err}");
    }

    #[test]
    fn lang_dir_custom() {
        let s = r#"
            manifest_version = 1
            id = "com.example.x"
            name = "X"
            version = "0.1"
            api_version = "1"
            lang_dir = "i18n"
            [entry]
            type = "process"
            command = "x"
        "#;
        let m = parse(s).expect("should parse");
        assert_eq!(m.lang_dir, "i18n");
    }
}
