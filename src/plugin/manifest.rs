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
    /// Event Bus 구독 허용 패턴 일람. 정확한 키 또는 `<namespace>.*` 와일드카드.
    /// 비어 있으면 plugin은 이벤트를 구독할 수 없다.
    #[serde(default)]
    pub event_subscribe: Vec<String>,
    /// Event Bus 발화 허용 패턴 일람. plugin이 자기 namespace로 publish할 때 필요.
    /// 예약 네임스페이스(`surface.*`, `system.*` 등)는 호스트만 publish 가능 — plugin이
    /// 적어도 매니페스트 검증 단계에서 거부된다.
    #[serde(default)]
    pub event_publish: Vec<String>,
    /// plugin이 publish하는 이벤트 카탈로그. 검토 항목 #6 — 1.0에 포함.
    /// `event_publish` 권한 패턴이 *허용 범위*라면, `events_emitted`는 그 안에서
    /// 실제로 어떤 정확 키를 발화하는지 plugin이 사전 선언하는 *카탈로그*다.
    /// 외부 tool(`tasty plugin show`)이 어떤 이벤트가 나오는지 확인할 수 있게 하고,
    /// extension plugin이 hook 대상으로 참조 가능하게 한다.
    #[serde(default, rename = "events_emitted")]
    pub events_emitted: Vec<EventEmittedDecl>,
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
    // WASM entry는 1.0 이후 재검토. 보류 이유는 docs/dev-guide/plugin-ecosystem.md
    // §1 참조 (강제 가능한 sandbox 가치 vs. 1.0 전 보안/도구체인 비용).
}

/// Plugin이 매니페스트에 선언할 수 있는 권한 카테고리.
///
/// 평면 enum — `fs.write`는 `fs.read`를 자동 포함하지 않는다.
/// 매니페스트에 두 권한이 모두 필요하면 명시적으로 선언해야 한다.
///
/// `IpcInvoke(prefix)`는 동적 토큰을 보유하므로 `Copy`를 derive할 수 없다.
/// 정적 enum variant도 함께 `Clone`만 derive하여 일관성을 유지한다.
#[derive(Debug, Clone, Eq, PartialEq, Hash)]
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
    /// 다른 plugin이 점유한 IPC namespace prefix의 메서드 호출.
    /// 토큰 형식: `ipc.invoke:<prefix>` (예: `ipc.invoke:codex`).
    IpcInvoke(String),
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
            other => {
                let prefix = other.strip_prefix("ipc.invoke:")?;
                if !is_valid_ipc_prefix(prefix) || is_reserved_ipc_prefix(prefix) {
                    return None;
                }
                return Some(Self::IpcInvoke(prefix.to_string()));
            }
        })
    }

    /// 권한의 토큰 문자열 형태. `IpcInvoke`는 prefix를 포함하므로 owned `String`을
    /// 반환한다. 비교/저장에는 `&token`을 그대로 사용하면 된다.
    pub fn as_token(&self) -> String {
        match self {
            Self::SurfaceRead => "surface.read".into(),
            Self::SurfaceWrite => "surface.write".into(),
            Self::Notification => "notification".into(),
            Self::ClipboardRead => "clipboard.read".into(),
            Self::ClipboardWrite => "clipboard.write".into(),
            Self::FsRead => "fs.read".into(),
            Self::FsWrite => "fs.write".into(),
            Self::ProcessSpawn => "process.spawn".into(),
            Self::TerminalSpawn => "terminal.spawn".into(),
            Self::TerminalWrite => "terminal.write".into(),
            Self::TerminalRead => "terminal.read".into(),
            Self::Network => "network".into(),
            Self::IpcInvoke(prefix) => format!("ipc.invoke:{prefix}"),
        }
    }
}

/// plugin이 publish할 이벤트 카탈로그 항목. `events_emitted = [...]`로 선언.
///
/// - `key`: 정확한 이벤트 키 (와일드카드 불가, 예약 네임스페이스 불가).
///   `event_publish` 권한 패턴 안에 포함되어야 한다 (그렇지 않으면 publish 시점에 호스트가 거부).
/// - `description`: 사람용 짧은 설명.
/// - `stability`: 이벤트 안정성 등급. 기본 `stable`. 새 이벤트를 도입할 때 plugin 작성자가
///   `experimental`로 표기해 호환성 약속을 약화시킬 수 있다.
/// - `payload_schema`: 옵션. 페이로드 JSON Schema 파일의 매니페스트 디렉터리 기준 상대 경로.
///   1.0에서는 호스트가 검증에 사용하지 않고 카탈로그용으로만 보유한다.
#[derive(Debug, Clone, Deserialize)]
pub struct EventEmittedDecl {
    pub key: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub stability: EventStability,
    #[serde(default)]
    pub payload_schema: Option<String>,
}

/// `events_emitted` 항목의 안정성 등급. `event-catalog.md`의 정책을 따른다.
#[derive(Debug, Clone, Copy, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EventStability {
    #[default]
    Stable,
    Experimental,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SurfaceKindDecl {
    pub kind: String,
    pub display_name_i18n_key: String,
    #[serde(default)]
    pub icon: Option<String>,
    /// surface 렌더링을 호스트 GUI가 직접 담당하는지(`Host`), plugin이 UiNode tree를
    /// 보내는 일반 remote 방식인지(`Remote`). 호스트가 화이트리스트로 등록한 kind에만
    /// `Host`를 허용한다. 기본 `Remote`.
    #[serde(default)]
    pub rendering: SurfaceKindRendering,
}

/// surface kind의 렌더링 방식. plugin 매니페스트 `rendering = "host" | "remote"`.
#[derive(Debug, Clone, Copy, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum SurfaceKindRendering {
    /// plugin이 UiNode tree로 화면을 그린다 (일반 plugin surface). 기본값.
    #[default]
    Remote,
    /// 호스트 본문이 직접 egui로 그린다. 매니페스트는 등록(메타데이터)만 담당하고
    /// 픽셀 처리는 호스트가 한다. 호스트 화이트리스트 매칭이 필요하다.
    Host,
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
    /// CLI help용 한 줄 설명. plain text는 plugin manager 없이 동작하는 CLI 클라이언트
    /// 진입 경로(`tasty <plugin> --help`)에서도 곧장 사용할 수 있다.
    #[serde(default)]
    pub description: Option<String>,
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
    pub description: Option<String>,
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
    /// 한 줄 도움말. clap의 `Arg::help`에 그대로 전달된다.
    #[serde(default)]
    pub help: Option<String>,
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
        self.validate_event_patterns()?;
        self.validate_events_emitted()?;
        Ok(())
    }

    /// `events_emitted` 카탈로그 검증.
    ///
    /// - key는 정확한 이벤트 키여야 한다 (와일드카드 불가).
    /// - key는 예약 네임스페이스를 쓸 수 없다.
    /// - key는 매니페스트의 `event_publish` 패턴 중 하나에 의해 *cover*되어야 한다.
    ///   (실제 publish 시점에도 같은 검사가 적용되므로 일관성 보장)
    /// - 같은 key를 두 번 선언하면 거부 (의미 없는 중복).
    fn validate_events_emitted(&self) -> anyhow::Result<()> {
        let mut seen: HashSet<&str> = HashSet::new();
        for decl in &self.events_emitted {
            if !is_valid_event_key(&decl.key) {
                anyhow::bail!(
                    "invalid events_emitted key '{}': must be a concrete event key (no '*')",
                    decl.key
                );
            }
            let ns = event_pattern_namespace(&decl.key);
            if is_reserved_event_namespace(ns) {
                anyhow::bail!(
                    "events_emitted key '{}' uses reserved namespace '{}' — \
                     only the host may publish in this namespace",
                    decl.key,
                    ns
                );
            }
            let covered = self
                .event_publish
                .iter()
                .any(|p| event_pattern_covers(p, &decl.key));
            if !covered {
                anyhow::bail!(
                    "events_emitted key '{}' is not covered by any event_publish pattern",
                    decl.key
                );
            }
            if !seen.insert(decl.key.as_str()) {
                anyhow::bail!("events_emitted key '{}' declared twice", decl.key);
            }
        }
        Ok(())
    }

    /// `event_subscribe`/`event_publish` 패턴 검증.
    ///
    /// 규칙:
    /// - 빈 문자열, 단독 `"*"` 거부 (모든 이벤트 일괄 매칭 금지)
    /// - 와일드카드는 끝의 `.<segment>` 자리에만 허용 (`foo.*`, `foo.bar.*`)
    /// - 중간/시작 와일드카드(`*.bar`, `f*`) 거부
    /// - 각 세그먼트: 소문자 ascii + 숫자 + `_`. 알파벳으로 시작.
    /// - `event_publish`는 예약 네임스페이스(`surface`, `system`, `tab`, ...)를 거부.
    fn validate_event_patterns(&self) -> anyhow::Result<()> {
        for p in &self.event_subscribe {
            if !is_valid_event_pattern(p) {
                anyhow::bail!(
                    "invalid event_subscribe pattern '{}': must be a key or '<ns>.*' \
                     (segments: lowercase ascii + digits + '_', start with a letter)",
                    p
                );
            }
        }
        for p in &self.event_publish {
            if !is_valid_event_pattern(p) {
                anyhow::bail!(
                    "invalid event_publish pattern '{}': must be a key or '<ns>.*'",
                    p
                );
            }
            let ns = event_pattern_namespace(p);
            if is_reserved_event_namespace(ns) {
                anyhow::bail!(
                    "event_publish pattern '{}' uses reserved namespace '{}' — \
                     only the host may publish in this namespace",
                    p,
                    ns
                );
            }
        }
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
        "plugin"
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
        "plugin"
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

/// Event Bus 패턴 검증. 정확한 키 또는 `<namespace>(.<segment>)*.*` 형태.
///
/// - `surface.created`: 정확한 key → 허용
/// - `surface.*`: namespace 와일드카드 → 허용
/// - `surface.lifecycle.*`: 깊이 2 와일드카드 → 허용
/// - `*`, `*.bar`, `foo.*.bar`, `foo*`, 빈 문자열 → 거부
fn is_valid_event_pattern(s: &str) -> bool {
    if s.is_empty() {
        return false;
    }
    let segments: Vec<&str> = s.split('.').collect();
    if segments.len() < 2 {
        // 모든 이벤트는 `<namespace>.<name>` 최소 2 세그먼트.
        return false;
    }
    for (i, seg) in segments.iter().enumerate() {
        let is_last = i + 1 == segments.len();
        if *seg == "*" {
            if !is_last {
                return false;
            }
            continue;
        }
        if !is_valid_event_segment(seg) {
            return false;
        }
    }
    true
}

/// 와일드카드를 허용하지 않는 정확 이벤트 키 검증. `events_emitted.key`에 사용.
fn is_valid_event_key(s: &str) -> bool {
    if s.is_empty() {
        return false;
    }
    let segments: Vec<&str> = s.split('.').collect();
    if segments.len() < 2 {
        return false;
    }
    segments.iter().all(|seg| is_valid_event_segment(seg))
}

/// publish 패턴이 정확 키를 cover하는지. 매니페스트 검증된 패턴만 받는다.
///
/// - 패턴이 정확 키와 같으면 cover.
/// - 패턴이 `<prefix>.*`이고 키가 `<prefix>.<segment>` 형태면 cover.
fn event_pattern_covers(pattern: &str, key: &str) -> bool {
    if pattern == key {
        return true;
    }
    if let Some(prefix) = pattern.strip_suffix(".*") {
        if let Some(rest) = key.strip_prefix(prefix) {
            return rest.starts_with('.') && rest.len() > 1;
        }
    }
    false
}

fn is_valid_event_segment(s: &str) -> bool {
    if s.is_empty() {
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

/// 패턴의 최상위 namespace 세그먼트. 검증 통과 후 호출하면 절대 빈 값을 반환하지 않는다.
fn event_pattern_namespace(s: &str) -> &str {
    s.split('.').next().unwrap_or("")
}

/// 호스트만 publish 가능한 예약 네임스페이스.
/// plugin은 자기 도메인의 namespace로만 발화할 수 있다.
fn is_reserved_event_namespace(ns: &str) -> bool {
    matches!(
        ns,
        "surface"
            | "tab"
            | "pane"
            | "split"
            | "workspace"
            | "window"
            | "clipboard"
            | "plugin"
            | "extension"
            | "tool"
            | "command"
            | "ime"
            | "theme"
            | "language"
            | "notification"
            | "hook"
            | "process"
            | "system"
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
    fn permission_ipc_invoke_token_round_trip() {
        let p = Permission::from_token("ipc.invoke:codex").expect("should parse");
        match &p {
            Permission::IpcInvoke(prefix) => assert_eq!(prefix, "codex"),
            _ => panic!("expected IpcInvoke"),
        }
        assert_eq!(p.as_token(), "ipc.invoke:codex");
    }

    #[test]
    fn permission_ipc_invoke_empty_prefix_rejected() {
        assert!(Permission::from_token("ipc.invoke:").is_none());
    }

    #[test]
    fn permission_ipc_invoke_invalid_prefix_rejected() {
        // 대문자 거부 (lowercase ascii only)
        assert!(Permission::from_token("ipc.invoke:Codex").is_none());
        // '.' 포함 거부
        assert!(Permission::from_token("ipc.invoke:co.dex").is_none());
        // 숫자 시작 거부
        assert!(Permission::from_token("ipc.invoke:1codex").is_none());
    }

    #[test]
    fn permission_ipc_invoke_reserved_prefix_rejected() {
        // 호스트 예약 prefix는 plugin이 점유할 수 없으므로 토큰도 거부.
        assert!(Permission::from_token("ipc.invoke:surface").is_none());
        assert!(Permission::from_token("ipc.invoke:pane").is_none());
    }

    #[test]
    fn manifest_accepts_ipc_invoke_permission() {
        let s = r#"
            manifest_version = 1
            id = "com.example.codex-helper"
            name = "Helper"
            version = "0.1.0"
            api_version = "1"
            permissions = ["ipc.invoke:codex", "surface.read"]
            [entry]
            type = "process"
            command = "x"
        "#;
        let m = parse(s).expect("should parse");
        let perms = m.parsed_permissions().expect("resolve");
        assert!(perms.contains(&Permission::IpcInvoke("codex".into())));
        assert!(perms.contains(&Permission::SurfaceRead));
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

    /// 번들된 com.tasty.image plugin의 실제 매니페스트가 파서를 통과하고
    /// surface_kind가 host-rendered로 인식되는지 확인.
    #[test]
    fn bundled_image_plugin_manifest_validates() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("crates")
            .join("tasty-plugin-image");
        let m = Manifest::load(&path).expect("image plugin manifest should load");
        assert_eq!(m.id, "com.tasty.image");
        assert_eq!(m.surface_kinds.len(), 1);
        assert_eq!(m.surface_kinds[0].kind, "image");
        assert_eq!(
            m.surface_kinds[0].rendering,
            SurfaceKindRendering::Host
        );
        // ipc_namespace prefix가 "image"여야 하고 cli 매핑이 모두 image.* 메서드.
        assert!(m
            .contributes
            .ipc_namespace
            .iter()
            .any(|n| n.prefix == "image"));
        assert!(m.contributes.cli.iter().any(|c| c.name == "image"));
    }

    #[test]
    fn surface_kind_rendering_defaults_to_remote() {
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
            kind = "explorer"
            display_name_i18n_key = "k"
        "#;
        let m = parse(s).expect("should parse");
        assert_eq!(m.surface_kinds[0].rendering, SurfaceKindRendering::Remote);
    }

    #[test]
    fn surface_kind_rendering_host_parses() {
        let s = r#"
            manifest_version = 1
            id = "com.tasty.image"
            name = "Image"
            version = "0.1"
            api_version = "1"
            [entry]
            type = "process"
            command = "x"
            [[surface_kinds]]
            kind = "image"
            display_name_i18n_key = "surface.kind.image"
            rendering = "host"
        "#;
        let m = parse(s).expect("should parse");
        assert_eq!(m.surface_kinds[0].rendering, SurfaceKindRendering::Host);
    }

    #[test]
    fn surface_kind_rendering_unknown_value_rejected() {
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
            kind = "explorer"
            display_name_i18n_key = "k"
            rendering = "exotic"
        "#;
        // serde가 lowercase enum의 알 수 없는 variant를 reject.
        assert!(parse(s).is_err());
    }

    #[test]
    fn event_subscribe_accepts_exact_key_and_wildcard() {
        let s = r#"
            manifest_version = 1
            id = "com.example.x"
            name = "X"
            version = "0.1"
            api_version = "1"
            event_subscribe = ["surface.created", "surface.*", "command.invoked"]
            [entry]
            type = "process"
            command = "x"
        "#;
        let m = parse(s).expect("should parse");
        assert_eq!(m.event_subscribe.len(), 3);
    }

    #[test]
    fn event_subscribe_rejects_bare_wildcard() {
        let s = r#"
            manifest_version = 1
            id = "com.example.x"
            name = "X"
            version = "0.1"
            api_version = "1"
            event_subscribe = ["*"]
            [entry]
            type = "process"
            command = "x"
        "#;
        let err = parse(s).unwrap_err().to_string();
        assert!(err.contains("invalid event_subscribe pattern"), "got: {err}");
    }

    #[test]
    fn event_subscribe_rejects_leading_wildcard() {
        let s = r#"
            manifest_version = 1
            id = "com.example.x"
            name = "X"
            version = "0.1"
            api_version = "1"
            event_subscribe = ["*.created"]
            [entry]
            type = "process"
            command = "x"
        "#;
        assert!(parse(s).is_err());
    }

    #[test]
    fn event_subscribe_rejects_middle_wildcard() {
        let s = r#"
            manifest_version = 1
            id = "com.example.x"
            name = "X"
            version = "0.1"
            api_version = "1"
            event_subscribe = ["surface.*.created"]
            [entry]
            type = "process"
            command = "x"
        "#;
        assert!(parse(s).is_err());
    }

    #[test]
    fn event_subscribe_rejects_single_segment() {
        let s = r#"
            manifest_version = 1
            id = "com.example.x"
            name = "X"
            version = "0.1"
            api_version = "1"
            event_subscribe = ["surface"]
            [entry]
            type = "process"
            command = "x"
        "#;
        assert!(parse(s).is_err());
    }

    #[test]
    fn event_subscribe_rejects_partial_wildcard() {
        let s = r#"
            manifest_version = 1
            id = "com.example.x"
            name = "X"
            version = "0.1"
            api_version = "1"
            event_subscribe = ["surf*.created"]
            [entry]
            type = "process"
            command = "x"
        "#;
        assert!(parse(s).is_err());
    }

    #[test]
    fn event_publish_rejects_reserved_namespace() {
        let s = r#"
            manifest_version = 1
            id = "com.example.evil"
            name = "Evil"
            version = "0.1"
            api_version = "1"
            event_publish = ["surface.created"]
            [entry]
            type = "process"
            command = "x"
        "#;
        let err = parse(s).unwrap_err().to_string();
        assert!(err.contains("reserved namespace"), "got: {err}");
    }

    #[test]
    fn event_publish_accepts_plugin_namespace() {
        let s = r#"
            manifest_version = 1
            id = "com.example.claude"
            name = "Claude"
            version = "0.1"
            api_version = "1"
            event_publish = ["claude.activity.changed", "claude.session.*"]
            [entry]
            type = "process"
            command = "x"
        "#;
        let m = parse(s).expect("should parse");
        assert_eq!(m.event_publish.len(), 2);
    }

    #[test]
    fn event_subscribe_accepts_reserved_namespace() {
        // subscribe는 어떤 namespace도 허용 (예약은 publish 전용 제약).
        let s = r#"
            manifest_version = 1
            id = "com.example.x"
            name = "X"
            version = "0.1"
            api_version = "1"
            event_subscribe = ["surface.*", "system.shutdown"]
            [entry]
            type = "process"
            command = "x"
        "#;
        let m = parse(s).expect("should parse");
        assert_eq!(m.event_subscribe.len(), 2);
    }

    #[test]
    fn events_emitted_parses_and_defaults_stable() {
        let s = r#"
            manifest_version = 1
            id = "com.example.x"
            name = "X"
            version = "0.1"
            api_version = "1"
            event_publish = ["com.example.x.*"]
            [[events_emitted]]
            key = "com.example.x.child_state_changed"
            description = "child state"
            [entry]
            type = "process"
            command = "x"
        "#;
        let m = parse(s).expect("should parse");
        assert_eq!(m.events_emitted.len(), 1);
        let decl = &m.events_emitted[0];
        assert_eq!(decl.key, "com.example.x.child_state_changed");
        assert_eq!(decl.stability, EventStability::Stable);
        assert!(decl.payload_schema.is_none());
    }

    #[test]
    fn events_emitted_rejects_wildcard_key() {
        let s = r#"
            manifest_version = 1
            id = "com.example.x"
            name = "X"
            version = "0.1"
            api_version = "1"
            event_publish = ["com.example.x.*"]
            [[events_emitted]]
            key = "com.example.x.*"
            [entry]
            type = "process"
            command = "x"
        "#;
        let err = parse(s).unwrap_err().to_string();
        assert!(err.contains("invalid events_emitted key"), "got: {err}");
    }

    #[test]
    fn events_emitted_rejects_reserved_namespace() {
        let s = r#"
            manifest_version = 1
            id = "com.example.x"
            name = "X"
            version = "0.1"
            api_version = "1"
            event_publish = ["surface.created"]
            [[events_emitted]]
            key = "surface.created"
            [entry]
            type = "process"
            command = "x"
        "#;
        // event_publish 검증이 먼저 reserved를 잡지만, 다른 검증 단계라도 결국 거부됨.
        assert!(parse(s).is_err());
    }

    #[test]
    fn events_emitted_rejects_uncovered_key() {
        let s = r#"
            manifest_version = 1
            id = "com.example.x"
            name = "X"
            version = "0.1"
            api_version = "1"
            event_publish = ["com.example.x.foo.*"]
            [[events_emitted]]
            key = "com.example.x.bar"
            [entry]
            type = "process"
            command = "x"
        "#;
        let err = parse(s).unwrap_err().to_string();
        assert!(err.contains("not covered by"), "got: {err}");
    }

    #[test]
    fn events_emitted_rejects_duplicate_key() {
        let s = r#"
            manifest_version = 1
            id = "com.example.x"
            name = "X"
            version = "0.1"
            api_version = "1"
            event_publish = ["com.example.x.*"]
            [[events_emitted]]
            key = "com.example.x.foo"
            [[events_emitted]]
            key = "com.example.x.foo"
            [entry]
            type = "process"
            command = "x"
        "#;
        let err = parse(s).unwrap_err().to_string();
        assert!(err.contains("declared twice"), "got: {err}");
    }

    #[test]
    fn events_emitted_accepts_experimental_stability() {
        let s = r#"
            manifest_version = 1
            id = "com.example.x"
            name = "X"
            version = "0.1"
            api_version = "1"
            event_publish = ["com.example.x.*"]
            [[events_emitted]]
            key = "com.example.x.alpha"
            stability = "experimental"
            payload_schema = "schemas/alpha.json"
            [entry]
            type = "process"
            command = "x"
        "#;
        let m = parse(s).expect("should parse");
        let decl = &m.events_emitted[0];
        assert_eq!(decl.stability, EventStability::Experimental);
        assert_eq!(decl.payload_schema.as_deref(), Some("schemas/alpha.json"));
    }
}
