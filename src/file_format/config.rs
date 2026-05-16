//! Detector TOML schema + 파서. M2 에서 manual `Deserialize` 본격 구현.

use serde::Deserialize;

/// detector 정의 TOML entry. M2 에서 manual `Deserialize` 로 교체.
#[derive(Debug, Clone, Deserialize)]
pub struct DetectorDecl {
    pub id: String,
    #[serde(default)]
    pub display_name_i18n_key: Option<String>,
    #[serde(default)]
    pub icon: Option<String>,
    #[serde(default)]
    pub disabled: bool,
    #[serde(default)]
    pub rule: Vec<DetectorRuleDecl>,
}

/// detector rule 정의. M2 에서 manual `Deserialize` 로 교체해 Unknown variant 보존.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DetectorRuleDecl {
    Extension {
        values: Vec<String>,
    },
    PathGlob {
        pattern: String,
    },
    Mime {
        types: Vec<String>,
    },
    Magic {
        #[serde(default)]
        offset: Option<usize>,
        bytes_hex: String,
    },
    IsDirectory,
    Lua {
        script: String,
    },
    StructureCheck {
        spec: String,
    },
}
