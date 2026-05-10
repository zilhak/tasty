//! Plugin 매니페스트 정의 + 파싱 + 검증.
//!
//! `~/.tasty/plugins/<plugin-id>/tasty-plugin.toml` 형식.

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
}

#[derive(Debug, Clone, Deserialize)]
pub struct CommandDecl {
    pub id: String,
    pub title_i18n_key: String,
    #[serde(default)]
    pub default_keybinding: Option<String>,
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
        Ok(())
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
            permissions = ["fs.read", "ipc.surface.list"]

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
    }
}
