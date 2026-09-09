//! 단축키 이식 번들 — 한 환경의 단축키 구성 **전량**을 파일 하나로 옮긴다.
//!
//! 옮길 대상이 두 파일에 흩어져 있다.
//!
//! - `~/.tasty/config.toml` 의 `[keybindings]` — [`KeybindingSettings`]
//! - `~/.tasty/plugins.toml` 의 `keybindings` — [`crate::registry_state::PluginsConfig::shortcut_overrides`]
//!
//! 그래서 번들 타입은 두 타입이 **모두 보이는 가장 낮은 지점**인 이 크레이트에 있다
//! (`tasty-host-plugin` → `tasty-settings` 단방향 의존이라 반대쪽에는 못 둔다).
//! 결정의 근거·대안·재검토 조건은
//! `docs/adr/0255-the-keybinding-bundle-is-a-toml-file-with-a-schema-tag.md`,
//! 지금 어떻게 동작하는지는 `docs/features/keybindings/index.md` 의 "이식 번들" 절.
//!
//! ## 왜 `PluginShortcutSnapshot` 이 export 원본이 아닌가
//!
//! 설정 창이 가진 스냅샷은 `command_registry.iter_all()` 로 만들어져 **등록된 command
//! 만** 담는다. 비활성·미등록 plugin 의 override 는 거기 없으므로, 그것을 원본으로
//! 쓰면 export 에서 조용히 빠진다. 원본은 언제나 `PluginsConfig.keybindings` 자체다.

use std::collections::BTreeMap;
use std::fmt;

use serde::{Deserialize, Serialize};
use tasty_settings::KeybindingSettings;

use crate::registry_state::ShortcutOverride;

/// plugin id → command id → 사용자 override.
pub type PluginShortcutOverrides = BTreeMap<String, BTreeMap<String, ShortcutOverride>>;

/// 번들 스키마 식별자. 사용자가 아무 TOML 이나 고를 수 있으므로 이 값이 최상위에
/// 없거나 다르면 "단축키 번들이 아니다" 로 거절한다.
pub const BUNDLE_SCHEMA: &str = "tasty.keybindings";

/// 이 빌드가 쓰는 번들 스키마 버전.
pub const BUNDLE_VERSION: u32 = 1;

/// 번들의 최상위 키 전량. [`decode`] 가 "모르는 최상위 키" 를 가리는 데 쓴다.
///
/// [`KeybindingBundle`] 의 필드와 짝이 맞아야 하고, 그 정합은
/// `top_level_keys_match_the_struct` 테스트가 직렬화 결과로 대조한다.
const BUNDLE_KEYS: &[&str] = &["schema", "version", "keybindings", "plugin_keybindings"];

/// 직렬화되는 번들 본체.
///
/// 필드 순서가 곧 TOML 출력 순서다 — 스칼라(`schema`/`version`)를 테이블보다 앞에 둔다.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeybindingBundle {
    pub schema: String,
    pub version: u32,
    pub keybindings: KeybindingSettings,
    #[serde(default)]
    pub plugin_keybindings: PluginShortcutOverrides,
}

/// [`decode`] 가 "이 환경에 무엇이 있는가" 를 판정하는 데 쓰는 입력.
#[derive(Debug, Clone, Copy, Default)]
pub struct DecodeEnv<'a> {
    /// 이 환경에 설치된 plugin id 전량. 여기 없는 plugin 의 override 는 버린다.
    pub installed_plugin_ids: &'a [String],
    /// 이 환경이 아는 script id 전량. `None` 이면 script 존재 판정을 **건너뛴다** —
    /// 레지스트리를 못 보는 호출자가 빈 슬라이스를 넘겨 전량을 잃는 사고를 막는다.
    pub known_script_ids: Option<&'a [String]>,
}

/// decode 결과 — 복원된 값과, 조용히 버려지지 않았음을 보이는 경고 목록.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecodedBundle {
    pub keybindings: KeybindingSettings,
    pub plugin_keybindings: PluginShortcutOverrides,
    pub warnings: Vec<BundleWarning>,
}

/// 번들을 아예 읽을 수 없는 경우. 값을 하나라도 복원할 수 있으면 에러가 아니라
/// [`BundleWarning`] 이다.
#[derive(Debug, thiserror::Error)]
pub enum BundleError {
    #[error("TOML 로 읽을 수 없다: {0}")]
    Toml(#[from] toml::de::Error),
    /// 최상위 `schema` 가 없거나 [`BUNDLE_SCHEMA`] 가 아니다.
    #[error("단축키 번들이 아니다 — 최상위 schema 가 \"{expected}\" 여야 하는데 {found}")]
    NotABundle {
        expected: &'static str,
        found: SchemaFound,
    },
    #[error("번들 직렬화 실패: {0}")]
    Serialize(#[from] toml::ser::Error),
}

/// 거절 사유를 사람이 읽을 수 있게 — 키가 아예 없었는지, 다른 값이었는지.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SchemaFound {
    Missing,
    Other(String),
}

impl fmt::Display for SchemaFound {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Missing => write!(f, "그 키가 없다"),
            Self::Other(s) => write!(f, "\"{s}\" 였다"),
        }
    }
}

/// decode 중 버리거나 기본값으로 되돌린 것 — 조용히 사라지지 않게 전부 여기 담는다.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BundleWarning {
    /// 번들 버전이 이 빌드가 아는 것보다 높다 — 아는 만큼만 복원했다.
    NewerVersion { found: u64, known: u32 },
    /// 최상위에 모르는 키가 있어 무시했다.
    UnknownTopLevelKey { key: String },
    /// `[keybindings]` 안에 이 빌드가 모르는 필드가 있어 무시했다.
    UnknownKeybindingField { field: String },
    /// 필드 값의 모양이 이 빌드의 것과 달라(타입 불일치·고정 배열 길이 차이) 그
    /// 필드만 기본값으로 복원했다.
    KeybindingFieldShapeMismatch { field: String, reason: String },
    /// `[keybindings]` 자체가 테이블이 아니거나 통째로 읽히지 않아 전량 기본값이다.
    KeybindingsUnreadable { reason: String },
    /// 이 환경에 없는 plugin 의 override 를 버렸다.
    DroppedUninstalledPlugin { plugin_id: String, commands: usize },
    /// override 항목 하나를 읽지 못해 버렸다.
    UnreadablePluginOverride {
        plugin_id: String,
        command_id: String,
        reason: String,
    },
    /// `plugin_keybindings` 자체를 읽지 못해 전량 버렸다.
    PluginOverridesUnreadable { reason: String },
    /// 이 환경에 없는 script 를 가리키는 바인딩을 버렸다.
    DroppedUnknownScriptBinding { script_id: String, combo: String },
}

impl fmt::Display for BundleWarning {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NewerVersion { found, known } => write!(
                f,
                "번들 버전 {found} 은 이 빌드가 아는 {known} 보다 높다 — 아는 만큼만 복원했다"
            ),
            Self::UnknownTopLevelKey { key } => {
                write!(f, "모르는 최상위 키 \"{key}\" 를 무시했다")
            }
            Self::UnknownKeybindingField { field } => {
                write!(f, "모르는 단축키 필드 \"{field}\" 를 무시했다")
            }
            Self::KeybindingFieldShapeMismatch { field, reason } => write!(
                f,
                "단축키 필드 \"{field}\" 의 모양이 달라 기본값으로 복원했다: {reason}"
            ),
            Self::KeybindingsUnreadable { reason } => {
                write!(f, "[keybindings] 를 읽지 못해 전량 기본값이다: {reason}")
            }
            Self::DroppedUninstalledPlugin {
                plugin_id,
                commands,
            } => write!(
                f,
                "설치되지 않은 plugin \"{plugin_id}\" 의 단축키 {commands} 개를 버렸다"
            ),
            Self::UnreadablePluginOverride {
                plugin_id,
                command_id,
                reason,
            } => write!(
                f,
                "plugin \"{plugin_id}\" 의 \"{command_id}\" 단축키를 읽지 못해 버렸다: {reason}"
            ),
            Self::PluginOverridesUnreadable { reason } => {
                write!(f, "plugin 단축키 절을 읽지 못해 전량 버렸다: {reason}")
            }
            Self::DroppedUnknownScriptBinding { script_id, combo } => write!(
                f,
                "이 환경에 없는 script \"{script_id}\" 의 바인딩 \"{combo}\" 를 버렸다"
            ),
        }
    }
}

/// 현재 구성을 번들 TOML 로 직렬화한다. 사용자가 열어 읽고 고칠 수 있는 형태다.
pub fn encode(
    keybindings: &KeybindingSettings,
    plugin_keybindings: &PluginShortcutOverrides,
) -> Result<String, BundleError> {
    let bundle = KeybindingBundle {
        schema: BUNDLE_SCHEMA.to_string(),
        version: BUNDLE_VERSION,
        keybindings: keybindings.clone(),
        plugin_keybindings: plugin_keybindings.clone(),
    };
    Ok(toml::to_string_pretty(&bundle)?)
}

/// 번들 TOML 을 읽어 복원한다.
///
/// 스키마 식별자가 없거나 다르면 거절하고, 그 밖의 이상(모르는 필드·모양 불일치·
/// 미설치 plugin·모르는 script)은 **경고로 돌려주고 계속한다** — import 는 사용자가
/// 고른 파일을 다루는 일이라 한 항목 때문에 전체가 죽으면 안 된다.
pub fn decode(text: &str, env: &DecodeEnv<'_>) -> Result<DecodedBundle, BundleError> {
    let table: toml::Table = toml::from_str(text)?;

    match table.get("schema").and_then(toml::Value::as_str) {
        Some(BUNDLE_SCHEMA) => {}
        Some(other) => {
            return Err(BundleError::NotABundle {
                expected: BUNDLE_SCHEMA,
                found: SchemaFound::Other(other.to_string()),
            });
        }
        None => {
            return Err(BundleError::NotABundle {
                expected: BUNDLE_SCHEMA,
                found: SchemaFound::Missing,
            });
        }
    }

    let mut warnings = Vec::new();

    if let Some(found) = table.get("version").and_then(toml::Value::as_integer)
        && found > i64::from(BUNDLE_VERSION)
    {
        warnings.push(BundleWarning::NewerVersion {
            found: found.unsigned_abs(),
            known: BUNDLE_VERSION,
        });
    }

    for key in table.keys() {
        if !BUNDLE_KEYS.contains(&key.as_str()) {
            warnings.push(BundleWarning::UnknownTopLevelKey { key: key.clone() });
        }
    }

    let mut keybindings = decode_keybindings(table.get("keybindings"), &mut warnings);
    let plugin_keybindings =
        decode_plugin_overrides(table.get("plugin_keybindings"), env, &mut warnings);

    if let Some(known) = env.known_script_ids {
        keybindings.script_bindings.retain(|b| {
            let kept = known.iter().any(|id| id == &b.script_id);
            if !kept {
                warnings.push(BundleWarning::DroppedUnknownScriptBinding {
                    script_id: b.script_id.clone(),
                    combo: b.combo.clone(),
                });
            }
            kept
        });
    }

    Ok(DecodedBundle {
        keybindings,
        plugin_keybindings,
        warnings,
    })
}

/// `[keybindings]` 를 필드 단위로 복원한다.
///
/// 기본값 테이블에서 출발해 번들의 필드를 하나씩 얹고, 얹은 뒤 전체가 역직렬화되지
/// 않으면 그 필드만 되돌린다. 이 방식이라 **필드 명부를 손으로 들지 않는다** — 무엇이
/// 있는 필드인지도, 어느 필드가 고정 길이 배열인지도 `KeybindingSettings` 자신의
/// 직렬화 결과가 답한다. 필드가 늘거나 배열 길이가 바뀌어도 여기는 안 고친다.
fn decode_keybindings(
    value: Option<&toml::Value>,
    warnings: &mut Vec<BundleWarning>,
) -> KeybindingSettings {
    let defaults = KeybindingSettings::default();
    let default_table = match toml::Value::try_from(&defaults) {
        Ok(toml::Value::Table(t)) => t,
        // `KeybindingSettings` 는 struct 라 항상 테이블이다. 그래도 여기서 패닉하지
        // 않는다 — import 경로는 사용자 파일을 다루므로 어떤 갈래도 살아 나가야 한다.
        _ => return defaults,
    };

    let Some(value) = value else {
        return defaults;
    };
    let toml::Value::Table(incoming) = value else {
        warnings.push(BundleWarning::KeybindingsUnreadable {
            reason: format!("테이블이 아니라 {} 였다", value.type_str()),
        });
        return defaults;
    };

    let mut acc = default_table.clone();
    for (field, v) in incoming {
        let Some(fallback) = default_table.get(field) else {
            warnings.push(BundleWarning::UnknownKeybindingField {
                field: field.clone(),
            });
            continue;
        };
        acc.insert(field.clone(), v.clone());
        if let Err(e) = KeybindingSettings::deserialize(toml::Value::Table(acc.clone())) {
            warnings.push(BundleWarning::KeybindingFieldShapeMismatch {
                field: field.clone(),
                reason: e.to_string(),
            });
            acc.insert(field.clone(), fallback.clone());
        }
    }

    match KeybindingSettings::deserialize(toml::Value::Table(acc)) {
        Ok(kb) => kb,
        Err(e) => {
            // 필드마다 검증하며 쌓았으므로 여기 오는 갈래는 없다. 그래도 값으로 받는다.
            warnings.push(BundleWarning::KeybindingsUnreadable {
                reason: e.to_string(),
            });
            defaults
        }
    }
}

/// `plugin_keybindings` 를 복원하며 **미설치 plugin 의 항목을 버린다**.
///
/// 버리는 것이 맞는 이유: 그 override 는 이 환경에서 가리킬 command 가 없어 발화도
/// 표시도 되지 않는데, 남겨 두면 `plugins.toml` 에 영원히 고이면서 나중에 그 plugin 을
/// 설치했을 때 사용자가 기억 못 하는 설정이 되살아난다.
fn decode_plugin_overrides(
    value: Option<&toml::Value>,
    env: &DecodeEnv<'_>,
    warnings: &mut Vec<BundleWarning>,
) -> PluginShortcutOverrides {
    let Some(value) = value else {
        return PluginShortcutOverrides::new();
    };
    let raw: BTreeMap<String, BTreeMap<String, toml::Value>> = match value.clone().try_into() {
        Ok(raw) => raw,
        Err(e) => {
            warnings.push(BundleWarning::PluginOverridesUnreadable {
                reason: e.to_string(),
            });
            return PluginShortcutOverrides::new();
        }
    };

    let mut out = PluginShortcutOverrides::new();
    for (plugin_id, commands) in raw {
        if !env.installed_plugin_ids.iter().any(|id| id == &plugin_id) {
            warnings.push(BundleWarning::DroppedUninstalledPlugin {
                commands: commands.len(),
                plugin_id,
            });
            continue;
        }
        let mut kept = BTreeMap::new();
        for (command_id, v) in commands {
            match v.try_into::<ShortcutOverride>() {
                Ok(ov) => {
                    kept.insert(command_id, ov);
                }
                Err(e) => warnings.push(BundleWarning::UnreadablePluginOverride {
                    plugin_id: plugin_id.clone(),
                    command_id,
                    reason: e.to_string(),
                }),
            }
        }
        // 전부 못 읽었으면 빈 맵을 남기지 않는다 — `PluginsConfig` 는 마지막 항목이
        // 지워지면 plugin 키 자체를 지우므로(`clear_shortcut_override`) 빈 그룹은
        // 애초에 만들어지지 않는 상태다.
        if !kept.is_empty() {
            out.insert(plugin_id, kept);
        }
    }
    out
}

/// 이식 시 `option` 바인딩 판정과 대체 적용 — 번들이 실어 나르는 두 값 위에서 돈다.
pub mod option_migration;

#[cfg(test)]
#[path = "keybinding_bundle/tests.rs"]
mod tests;
