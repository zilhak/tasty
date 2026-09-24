//! Detector TOML schema + 파서.
//!
//! `DetectorRuleDecl` 은 manual `Deserialize` 구현으로 미지의 `kind` 도
//! payload (`toml::Value`) 를 손실 없이 보존한다 (forward-compat).

use serde::de::{self, MapAccess, Visitor};
use serde::{Deserialize, Deserializer};
use std::fmt;

/// 호스트 기본값·사용자 TOML의 확장자별 우선순위. 플러그인 매니페스트에는 이 항목이 없다.
///
/// ```toml
/// [[extension_priority]]
/// extension = "md"
/// order = ["com.example.mdx/mdx-strict", "markdown"]
/// ```
///
/// `order` 에 적힌 detector 가 우선. 표에 없는 detector 는 `install_order` 오름차순으로
/// 뒤에 붙는다 (`identify_by_extension_priority` 참조). 미설치 detector id 는 silently skip.
#[derive(Debug, Clone, Deserialize)]
pub struct ExtensionPriorityDecl {
    pub extension: String,
    #[serde(default)]
    pub order: Vec<String>,
}

/// 같은 ID의 선언은 규칙을 합치고 명시한 메타데이터를 덮어쓴다. ensure_finalized에서 병합한다.
#[derive(Debug, Clone, Deserialize)]
pub struct DetectorDecl {
    pub id: String,
    #[serde(default)]
    pub display_name_i18n_key: Option<String>,
    #[serde(default)]
    pub icon: Option<String>,
    /// 적지 않으면 `None`. host · plugin 은 `true` 만 뜻을 갖고(끈다), user 는 `false` 도
    /// 뜻을 갖는다(다른 출처가 끈 detector 를 켠다) — 해석은 registry 의 `install_one`.
    #[serde(default)]
    pub disabled: Option<bool>,
    #[serde(default)]
    pub rule: Vec<DetectorRuleDecl>,
}

/// 알려진 kind를 파싱하고 미지의 kind는 원문과 함께 Unknown으로 보존한다.
/// Lua도 여기서는 파싱한다. 플러그인 Lua 규칙의 제거는 install_plugin_detectors가 담당한다.
#[derive(Debug, Clone)]
pub enum DetectorRuleDecl {
    Extension { values: Vec<String> },
    PathGlob { pattern: String },
    Mime { types: Vec<String> },
    Magic { offset: usize, bytes_hex: String },
    IsDirectory,
    Lua { script: String },
    StructureCheck { spec: String },
    Unknown { kind_name: String, raw: toml::Value },
}

impl<'de> Deserialize<'de> for DetectorRuleDecl {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_map(DetectorRuleDeclVisitor)
    }
}

struct DetectorRuleDeclVisitor;

impl<'de> Visitor<'de> for DetectorRuleDeclVisitor {
    type Value = DetectorRuleDecl;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("a detector rule table with a `kind` field")
    }

    fn visit_map<A>(self, mut map: A) -> Result<DetectorRuleDecl, A::Error>
    where
        A: MapAccess<'de>,
    {
        let mut table = toml::value::Table::new();
        while let Some(key) = map.next_key::<String>()? {
            let value: toml::Value = map.next_value()?;
            table.insert(key, value);
        }

        let kind = table
            .get("kind")
            .and_then(|v| v.as_str())
            .ok_or_else(|| de::Error::missing_field("kind"))?
            .to_string();

        match kind.as_str() {
            "extension" => {
                let values = take_string_array(&table, "values")
                    .ok_or_else(|| de::Error::missing_field("values"))?;
                Ok(DetectorRuleDecl::Extension { values })
            }
            "path-glob" | "path_glob" => {
                let pattern = take_string(&table, "pattern")
                    .ok_or_else(|| de::Error::missing_field("pattern"))?;
                Ok(DetectorRuleDecl::PathGlob { pattern })
            }
            "mime" => {
                let types = take_string_array(&table, "types")
                    .ok_or_else(|| de::Error::missing_field("types"))?;
                Ok(DetectorRuleDecl::Mime { types })
            }
            "magic" => {
                let offset = table
                    .get("offset")
                    .and_then(|v| v.as_integer())
                    .map(|n| n as usize)
                    .unwrap_or(0);
                let bytes_hex = take_string(&table, "bytes_hex")
                    .ok_or_else(|| de::Error::missing_field("bytes_hex"))?;
                Ok(DetectorRuleDecl::Magic { offset, bytes_hex })
            }
            "is-directory" | "is_directory" => Ok(DetectorRuleDecl::IsDirectory),
            "lua" => {
                let script = take_string(&table, "script")
                    .ok_or_else(|| de::Error::missing_field("script"))?;
                Ok(DetectorRuleDecl::Lua { script })
            }
            "structure-check" | "structure_check" => {
                let spec =
                    take_string(&table, "spec").ok_or_else(|| de::Error::missing_field("spec"))?;
                Ok(DetectorRuleDecl::StructureCheck { spec })
            }
            other => Ok(DetectorRuleDecl::Unknown {
                kind_name: other.to_string(),
                raw: toml::Value::Table(table),
            }),
        }
    }
}

fn take_string(table: &toml::value::Table, key: &str) -> Option<String> {
    table.get(key).and_then(|v| v.as_str()).map(String::from)
}

fn take_string_array(table: &toml::value::Table, key: &str) -> Option<Vec<String>> {
    table.get(key).and_then(|v| v.as_array()).map(|arr| {
        arr.iter()
            .filter_map(|v| v.as_str().map(String::from))
            .collect()
    })
}

/// Decl 단계 schema 검증. install 단계 cross-ref 와는 별개로, 자기 자신만 검사.
#[derive(Debug, Clone)]
pub enum DetectorDeclError {
    InvalidId(String),
    ReservedIdFromPlugin(String),
    EmptyExtensionValues {
        detector: String,
    },
    BadMagicHex {
        detector: String,
        got: String,
    },
    EmptyPathGlob {
        detector: String,
    },
    InvalidPathGlob {
        detector: String,
        pattern: String,
        reason: String,
    },
    StructureCheckEscape {
        detector: String,
    },
    UnknownRuleKind {
        detector: String,
        kind: String,
    },
}

impl fmt::Display for DetectorDeclError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidId(id) => write!(f, "invalid detector id '{id}'"),
            Self::ReservedIdFromPlugin(id) => write!(
                f,
                "plugin cannot define reserved (`$`-prefixed) detector id '{id}'"
            ),
            Self::EmptyExtensionValues { detector } => write!(
                f,
                "detector '{detector}': extension rule values must not be empty"
            ),
            Self::BadMagicHex { detector, got } => write!(
                f,
                "detector '{detector}': magic bytes_hex must be even length hex-only (got '{got}')"
            ),
            Self::EmptyPathGlob { detector } => write!(
                f,
                "detector '{detector}': path-glob pattern must not be empty"
            ),
            Self::InvalidPathGlob {
                detector,
                pattern,
                reason,
            } => write!(
                f,
                "detector '{detector}': path-glob pattern '{pattern}' is invalid: {reason}"
            ),
            Self::StructureCheckEscape { detector } => write!(
                f,
                "detector '{detector}': structure-check spec path must not escape manifest dir"
            ),
            Self::UnknownRuleKind { detector, kind } => write!(
                f,
                "detector '{detector}': unsupported rule kind '{kind}' (skipped at runtime)"
            ),
        }
    }
}

impl std::error::Error for DetectorDeclError {}

/// 플러그인의 $ 예약 ID를 거절한다. 미지의 kind는 경고로 반환한다.
pub fn validate_detector_decl(
    decl: &DetectorDecl,
    from_plugin: bool,
) -> Result<Vec<DetectorDeclError>, DetectorDeclError> {
    use super::types::is_valid_detector_id;

    if !is_valid_detector_id(&decl.id) {
        return Err(DetectorDeclError::InvalidId(decl.id.clone()));
    }
    if from_plugin && decl.id.starts_with('$') {
        return Err(DetectorDeclError::ReservedIdFromPlugin(decl.id.clone()));
    }

    let mut warnings = Vec::new();
    for rule in &decl.rule {
        match rule {
            DetectorRuleDecl::Extension { values } => {
                if values.is_empty() {
                    return Err(DetectorDeclError::EmptyExtensionValues {
                        detector: decl.id.clone(),
                    });
                }
            }
            DetectorRuleDecl::PathGlob { pattern } => {
                if pattern.is_empty() {
                    return Err(DetectorDeclError::EmptyPathGlob {
                        detector: decl.id.clone(),
                    });
                }
                // 평가 때 쓰는 / 정규화와 같은 방식으로 glob을 미리 컴파일해 잘못된 문법을 거절한다.
                let normalized = tasty_utils::path::to_slash(pattern);
                if let Err(e) = globset::Glob::new(&normalized) {
                    return Err(DetectorDeclError::InvalidPathGlob {
                        detector: decl.id.clone(),
                        pattern: pattern.clone(),
                        reason: e.to_string(),
                    });
                }
            }
            DetectorRuleDecl::Magic { bytes_hex, .. } => {
                if bytes_hex.len() % 2 != 0 || !bytes_hex.chars().all(|c| c.is_ascii_hexdigit()) {
                    return Err(DetectorDeclError::BadMagicHex {
                        detector: decl.id.clone(),
                        got: bytes_hex.clone(),
                    });
                }
            }
            DetectorRuleDecl::StructureCheck { spec } => {
                if spec.contains("..") {
                    return Err(DetectorDeclError::StructureCheckEscape {
                        detector: decl.id.clone(),
                    });
                }
            }
            DetectorRuleDecl::Unknown { kind_name, .. } => {
                warnings.push(DetectorDeclError::UnknownRuleKind {
                    detector: decl.id.clone(),
                    kind: kind_name.clone(),
                });
            }
            _ => {}
        }
    }
    Ok(warnings)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Deserialize)]
    struct DetectorWrap {
        #[serde(rename = "detector")]
        detectors: Vec<DetectorDecl>,
    }

    fn parse(toml: &str) -> Vec<DetectorDecl> {
        toml::from_str::<DetectorWrap>(toml)
            .expect("parse")
            .detectors
    }

    #[test]
    fn parses_known_kinds() {
        let t = r#"
            [[detector]]
            id = "pdf"
            display_name_i18n_key = "format.pdf"
            icon = "file"

            [[detector.rule]]
            kind = "extension"
            values = ["pdf"]

            [[detector.rule]]
            kind = "magic"
            offset = 0
            bytes_hex = "25504446"

            [[detector.rule]]
            kind = "path-glob"
            pattern = "Dockerfile"

            [[detector.rule]]
            kind = "mime"
            types = ["application/pdf"]

            [[detector.rule]]
            kind = "is-directory"
        "#;
        let d = parse(t);
        assert_eq!(d.len(), 1);
        assert_eq!(d[0].id, "pdf");
        assert_eq!(d[0].rule.len(), 5);
        matches!(d[0].rule[0], DetectorRuleDecl::Extension { .. });
        matches!(d[0].rule[1], DetectorRuleDecl::Magic { .. });
        matches!(d[0].rule[2], DetectorRuleDecl::PathGlob { .. });
        matches!(d[0].rule[3], DetectorRuleDecl::Mime { .. });
        matches!(d[0].rule[4], DetectorRuleDecl::IsDirectory);
    }

    #[test]
    fn preserves_unknown_kind_payload() {
        let t = r#"
            [[detector]]
            id = "exotic"

            [[detector.rule]]
            kind = "future-thing"
            magic_field = 42
            another = "yes"
        "#;
        let d = parse(t);
        assert_eq!(d.len(), 1);
        match &d[0].rule[0] {
            DetectorRuleDecl::Unknown { kind_name, raw } => {
                assert_eq!(kind_name, "future-thing");
                let tbl = raw.as_table().unwrap();
                assert_eq!(
                    tbl.get("magic_field").and_then(|v| v.as_integer()),
                    Some(42)
                );
                assert_eq!(tbl.get("another").and_then(|v| v.as_str()), Some("yes"));
            }
            other => panic!("expected Unknown, got {other:?}"),
        }
    }

    #[test]
    fn plugin_reserved_id_rejected() {
        let decl = DetectorDecl {
            id: "$something".into(),
            display_name_i18n_key: None,
            icon: None,
            disabled: None,
            rule: vec![],
        };
        let res = validate_detector_decl(&decl, true);
        assert!(matches!(
            res,
            Err(DetectorDeclError::ReservedIdFromPlugin(_))
        ));
    }

    #[test]
    fn host_reserved_id_allowed() {
        let decl = DetectorDecl {
            id: "$directory".into(),
            display_name_i18n_key: None,
            icon: None,
            disabled: None,
            rule: vec![DetectorRuleDecl::IsDirectory],
        };
        assert!(validate_detector_decl(&decl, false).is_ok());
    }

    #[test]
    fn magic_bad_hex_rejected() {
        let decl = DetectorDecl {
            id: "x".into(),
            display_name_i18n_key: None,
            icon: None,
            disabled: None,
            rule: vec![DetectorRuleDecl::Magic {
                offset: 0,
                bytes_hex: "ZZZ".into(),
            }],
        };
        assert!(matches!(
            validate_detector_decl(&decl, false),
            Err(DetectorDeclError::BadMagicHex { .. })
        ));
    }

    #[test]
    fn empty_extension_rejected() {
        let decl = DetectorDecl {
            id: "x".into(),
            display_name_i18n_key: None,
            icon: None,
            disabled: None,
            rule: vec![DetectorRuleDecl::Extension { values: vec![] }],
        };
        assert!(matches!(
            validate_detector_decl(&decl, false),
            Err(DetectorDeclError::EmptyExtensionValues { .. })
        ));
    }

    #[test]
    fn invalid_path_glob_pattern_rejected_at_registration() {
        let decl = DetectorDecl {
            id: "x".into(),
            display_name_i18n_key: None,
            icon: None,
            disabled: None,
            rule: vec![DetectorRuleDecl::PathGlob {
                pattern: "[abc".into(),
            }],
        };
        let err = validate_detector_decl(&decl, false).expect_err("must reject");
        assert!(matches!(err, DetectorDeclError::InvalidPathGlob { .. }));
        let msg = err.to_string();
        assert!(
            msg.contains("[abc"),
            "message should name the pattern: {msg}"
        );
    }

    #[test]
    fn path_glob_with_standard_glob_syntax_accepted() {
        for pattern in ["file?.txt", "[abc]*.rs", "**/*.rs", "*.config.json"] {
            let decl = DetectorDecl {
                id: "x".into(),
                display_name_i18n_key: None,
                icon: None,
                disabled: None,
                rule: vec![DetectorRuleDecl::PathGlob {
                    pattern: pattern.into(),
                }],
            };
            assert!(
                validate_detector_decl(&decl, false).is_ok(),
                "pattern '{pattern}' should be accepted"
            );
        }
    }

    #[test]
    fn structure_check_escape_rejected() {
        let decl = DetectorDecl {
            id: "x".into(),
            display_name_i18n_key: None,
            icon: None,
            disabled: None,
            rule: vec![DetectorRuleDecl::StructureCheck {
                spec: "../etc/passwd".into(),
            }],
        };
        assert!(matches!(
            validate_detector_decl(&decl, false),
            Err(DetectorDeclError::StructureCheckEscape { .. })
        ));
    }

    #[test]
    fn unknown_kind_returns_warning_not_error() {
        let decl = DetectorDecl {
            id: "x".into(),
            display_name_i18n_key: None,
            icon: None,
            disabled: None,
            rule: vec![DetectorRuleDecl::Unknown {
                kind_name: "future".into(),
                raw: toml::Value::String("dummy".into()),
            }],
        };
        let warnings = validate_detector_decl(&decl, false).expect("ok");
        assert_eq!(warnings.len(), 1);
    }
}
