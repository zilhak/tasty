//! `FileFormatRegistry` — 등록된 detector 들을 관리하고 file 을 identify 한다.
//!
//! 출처별 contribution 을 따로 보관해 plugin uninstall 시 원본 복원 가능.
//! finalize 는 incremental — install/uninstall 호출 후 dirty 표시 + identify 시 1회.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::RwLock;

use tracing::warn;

use super::config::{validate_detector_decl, DetectorDecl, DetectorRuleDecl};
use super::evaluator::evaluate_cheap;
use super::types::{
    DetectDepth, DetectorId, DetectorRule, DetectorRuleKind, FileFormatDetector, FileTarget,
    RuleOrigin,
};

/// 한 출처가 단일 detector 에 기여한 내용.
#[derive(Debug, Clone)]
struct DetectorContribution {
    origin: RuleOrigin,
    display_name_i18n_key: Option<String>,
    icon: Option<String>,
    /// `disabled` 는 명시된 출처만 적용 (patch). `None` 이면 무시.
    disabled_override: Option<bool>,
    rules: Vec<DetectorRuleKind>,
}

struct Inner {
    /// detector id → 출처별 contribution. install 순서대로 push (host → plugin → user).
    contributions: BTreeMap<DetectorId, Vec<DetectorContribution>>,
    /// finalize 결과 cache. dirty 시 lazy 재계산.
    finalized: BTreeMap<DetectorId, FileFormatDetector>,
    dirty: bool,
}

pub struct FileFormatRegistry {
    inner: RwLock<Inner>,
}

impl FileFormatRegistry {
    pub fn new() -> Self {
        Self {
            inner: RwLock::new(Inner {
                contributions: BTreeMap::new(),
                finalized: BTreeMap::new(),
                dirty: false,
            }),
        }
    }

    /// detector 조회 — clone 반환.
    pub fn detector(&self, id: &DetectorId) -> Option<FileFormatDetector> {
        self.ensure_finalized();
        let inner = self.inner.read().ok()?;
        inner.finalized.get(id).cloned()
    }

    pub fn list_detectors(&self) -> Vec<DetectorId> {
        self.ensure_finalized();
        let inner = match self.inner.read() {
            Ok(g) => g,
            Err(_) => return Vec::new(),
        };
        inner.finalized.keys().cloned().collect()
    }

    /// `target` 에 매칭되는 detector id 결정. 매칭 실패 시 `None` (= unknown).
    pub fn identify(&self, target: &FileTarget, depth: DetectDepth) -> Option<DetectorId> {
        self.ensure_finalized();
        let inner = self.inner.read().ok()?;
        let is_dir = target.is_directory();

        for (id, det) in inner.finalized.iter() {
            if det.disabled {
                continue;
            }
            // pre-filter: 디렉토리면 IsDirectory rule 가진 detector 만 평가.
            // 파일이면 IsDirectory rule 가진 detector 는 제외 (즉 file vs dir 단순 분리).
            let has_is_dir = det
                .rules
                .iter()
                .any(|r| matches!(r.kind, DetectorRuleKind::IsDirectory));
            if is_dir != has_is_dir {
                continue;
            }
            // 매칭: OR — 하나라도 match 면 detector 매칭.
            // Phase A 는 cheap 만. depth=Deep 도 같은 evaluator (B/D 단계에 확장).
            let _ = depth;
            for rule in &det.rules {
                if evaluate_cheap(&rule.kind, target) {
                    return Some(id.clone());
                }
            }
        }
        None
    }

    // ── install / uninstall ────────────────────────────────────────────

    pub fn install_host_defaults(&self, toml_text: &str) {
        let decls = match parse_detector_section(toml_text) {
            Ok(d) => d,
            Err(e) => {
                warn!(error = %e, "file_format: failed to parse host defaults");
                return;
            }
        };
        let mut inner = match self.inner.write() {
            Ok(g) => g,
            Err(_) => return,
        };
        for decl in decls {
            install_one(&mut inner, decl, RuleOrigin::HostDefault, false);
        }
        inner.dirty = true;
    }

    pub fn install_user_config(&self, path: &Path) {
        let text = match std::fs::read_to_string(path) {
            Ok(s) => s,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return,
            Err(e) => {
                warn!(path = %path.display(), error = %e, "file_format: user config read failed");
                return;
            }
        };
        let decls = match parse_detector_section(&text) {
            Ok(d) => d,
            Err(e) => {
                warn!(path = %path.display(), error = %e, "file_format: user config parse failed");
                return;
            }
        };
        let mut inner = match self.inner.write() {
            Ok(g) => g,
            Err(_) => return,
        };
        for decl in decls {
            install_one(&mut inner, decl, RuleOrigin::User, false);
        }
        inner.dirty = true;
    }

    pub fn install_plugin_detectors(&self, plugin_id: &str, decls: &[DetectorDecl]) {
        let mut inner = match self.inner.write() {
            Ok(g) => g,
            Err(_) => return,
        };
        for decl in decls {
            install_one(
                &mut inner,
                decl.clone(),
                RuleOrigin::Plugin(plugin_id.to_string()),
                true,
            );
        }
        inner.dirty = true;
    }

    pub fn uninstall_plugin(&self, plugin_id: &str) {
        let mut inner = match self.inner.write() {
            Ok(g) => g,
            Err(_) => return,
        };
        let mut empty_ids = Vec::new();
        for (id, contribs) in inner.contributions.iter_mut() {
            contribs.retain(|c| !matches!(&c.origin, RuleOrigin::Plugin(p) if p == plugin_id));
            if contribs.is_empty() {
                empty_ids.push(id.clone());
            }
        }
        for id in empty_ids {
            inner.contributions.remove(&id);
        }
        inner.dirty = true;
    }

    fn ensure_finalized(&self) {
        let needs_finalize = self
            .inner
            .read()
            .map(|g| g.dirty)
            .unwrap_or(false);
        if !needs_finalize {
            return;
        }
        let mut inner = match self.inner.write() {
            Ok(g) => g,
            Err(_) => return,
        };
        if !inner.dirty {
            return;
        }
        let mut next = BTreeMap::new();
        for (id, contribs) in inner.contributions.iter() {
            let mut display = None;
            let mut icon = None;
            let mut disabled = false;
            let mut rules: Vec<DetectorRule> = Vec::new();
            for c in contribs {
                if c.display_name_i18n_key.is_some() {
                    display = c.display_name_i18n_key.clone();
                }
                if c.icon.is_some() {
                    icon = c.icon.clone();
                }
                if let Some(d) = c.disabled_override {
                    disabled = d;
                }
                for kind in &c.rules {
                    let candidate = DetectorRule {
                        kind: kind.clone(),
                        origin: c.origin.clone(),
                    };
                    // dedupe — 동일 kind 두 번 등록되면 처음 origin 만 보존.
                    if !rules.iter().any(|r| rule_kind_eq(&r.kind, &candidate.kind)) {
                        rules.push(candidate);
                    }
                }
            }
            next.insert(
                id.clone(),
                FileFormatDetector {
                    id: id.clone(),
                    display_name_i18n_key: display,
                    icon,
                    rules,
                    disabled,
                },
            );
        }
        inner.finalized = next;
        inner.dirty = false;
    }
}

impl Default for FileFormatRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// 같은 id 의 contribution 을 append. 기존 entry 가 있으면 patch semantics 로 메타데이터
/// 가 마지막 출처에 의해 덮어써진다 (finalize 단계에서 적용).
fn install_one(inner: &mut Inner, decl: DetectorDecl, origin: RuleOrigin, from_plugin: bool) {
    // schema 검증
    let validation = validate_detector_decl(&decl, from_plugin);
    let warnings = match validation {
        Ok(w) => w,
        Err(e) => {
            warn!(error = %e, "file_format: rejecting detector decl");
            return;
        }
    };
    for w in warnings {
        warn!(warning = %w, "file_format: detector decl warning");
    }

    let id = DetectorId(decl.id.clone());
    let entry = inner.contributions.entry(id).or_default();
    // 같은 origin 으로 재install (예: 사용자 설정 reload) 인 경우 기존 동일 origin 제거 후 push.
    entry.retain(|c| c.origin != origin);
    let rule_kinds: Vec<DetectorRuleKind> = decl
        .rule
        .into_iter()
        .filter_map(|r| decl_rule_to_kind(r))
        .collect();
    entry.push(DetectorContribution {
        origin,
        display_name_i18n_key: decl.display_name_i18n_key,
        icon: decl.icon,
        // decl.disabled 가 명시되었는지 schema 상 알 수 없으므로(`#[serde(default)]`),
        // patch semantics 를 위해 false 면 None 으로 취급 (= 끄지 않음). 사용자가 명시적으로
        // disable 하려면 다른 출처가 disabled = true 를 적어 last-writer-wins.
        disabled_override: if decl.disabled { Some(true) } else { None },
        rules: rule_kinds,
    });
}

fn decl_rule_to_kind(decl: DetectorRuleDecl) -> Option<DetectorRuleKind> {
    Some(match decl {
        DetectorRuleDecl::Extension { values } => DetectorRuleKind::Extension {
            values: values
                .into_iter()
                .map(|s| s.trim_start_matches('.').to_ascii_lowercase())
                .filter(|s| !s.is_empty())
                .collect(),
        },
        DetectorRuleDecl::PathGlob { pattern } => DetectorRuleKind::PathGlob { pattern },
        DetectorRuleDecl::Mime { types } => DetectorRuleKind::Mime { types },
        DetectorRuleDecl::Magic { offset, bytes_hex } => {
            let bytes = hex_to_bytes(&bytes_hex)?;
            DetectorRuleKind::Magic { offset, bytes }
        }
        DetectorRuleDecl::IsDirectory => DetectorRuleKind::IsDirectory,
        DetectorRuleDecl::Lua { script } => DetectorRuleKind::Lua {
            script_path: PathBuf::from(script),
        },
        DetectorRuleDecl::StructureCheck { spec } => DetectorRuleKind::StructureCheck {
            spec_path: PathBuf::from(spec),
        },
        DetectorRuleDecl::Unknown { kind_name, raw } => DetectorRuleKind::Unknown {
            kind_name,
            raw,
        },
    })
}

fn hex_to_bytes(hex: &str) -> Option<Vec<u8>> {
    if hex.len() % 2 != 0 {
        return None;
    }
    (0..hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).ok())
        .collect()
}

fn rule_kind_eq(a: &DetectorRuleKind, b: &DetectorRuleKind) -> bool {
    // Unknown 의 raw 비교는 toml::Value PartialEq 가 있어 가능.
    a == b
}

/// host default / user config 공통 표면: `[[detector]]` 섹션을 가진 TOML.
fn parse_detector_section(toml_text: &str) -> Result<Vec<DetectorDecl>, toml::de::Error> {
    #[derive(serde::Deserialize)]
    struct Wrap {
        #[serde(default, rename = "detector")]
        detectors: Vec<DetectorDecl>,
    }
    let w: Wrap = toml::from_str(toml_text)?;
    Ok(w.detectors)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn target(p: &str) -> FileTarget {
        FileTarget::new(PathBuf::from(p))
    }

    #[test]
    fn host_default_loads_and_identifies_markdown() {
        let reg = FileFormatRegistry::new();
        reg.install_host_defaults(
            include_str!("defaults/default-file-format.toml"),
        );
        let id = reg.identify(&target("a/b.md"), DetectDepth::Cheap);
        assert_eq!(id, Some(DetectorId("markdown".into())));
        let id = reg.identify(&target("a/b.MARKDOWN"), DetectDepth::Cheap);
        assert_eq!(id, Some(DetectorId("markdown".into())));
        let id = reg.identify(&target("a/b.html"), DetectDepth::Cheap);
        assert_eq!(id, Some(DetectorId("html".into())));
        let id = reg.identify(&target("a/b.png"), DetectDepth::Cheap);
        assert_eq!(id, Some(DetectorId("image".into())));
        let id = reg.identify(&target("a/b.unknownext"), DetectDepth::Cheap);
        assert_eq!(id, None);
    }

    #[test]
    fn plugin_extends_existing_detector() {
        let reg = FileFormatRegistry::new();
        reg.install_host_defaults(
            include_str!("defaults/default-file-format.toml"),
        );
        // plugin 이 mdx 확장자 추가
        let decls = vec![DetectorDecl {
            id: "markdown".into(),
            display_name_i18n_key: None,
            icon: None,
            disabled: false,
            rule: vec![DetectorRuleDecl::Extension {
                values: vec!["mdx".into()],
            }],
        }];
        reg.install_plugin_detectors("com.example.mdx", &decls);
        let id = reg.identify(&target("a/b.mdx"), DetectDepth::Cheap);
        assert_eq!(id, Some(DetectorId("markdown".into())));
        // 기존 md 매칭 유지
        let id = reg.identify(&target("a/b.md"), DetectDepth::Cheap);
        assert_eq!(id, Some(DetectorId("markdown".into())));
    }

    #[test]
    fn uninstall_plugin_removes_only_its_rules() {
        let reg = FileFormatRegistry::new();
        reg.install_host_defaults(
            include_str!("defaults/default-file-format.toml"),
        );
        let decls = vec![DetectorDecl {
            id: "markdown".into(),
            display_name_i18n_key: None,
            icon: None,
            disabled: false,
            rule: vec![DetectorRuleDecl::Extension {
                values: vec!["mdx".into()],
            }],
        }];
        reg.install_plugin_detectors("com.example.mdx", &decls);
        assert_eq!(
            reg.identify(&target("a/b.mdx"), DetectDepth::Cheap),
            Some(DetectorId("markdown".into()))
        );
        reg.uninstall_plugin("com.example.mdx");
        // 호스트의 md 는 유지
        assert_eq!(
            reg.identify(&target("a/b.md"), DetectDepth::Cheap),
            Some(DetectorId("markdown".into()))
        );
        // plugin 의 mdx 는 사라짐
        assert_eq!(
            reg.identify(&target("a/b.mdx"), DetectDepth::Cheap),
            None
        );
    }

    #[test]
    fn directory_prefilter() {
        let reg = FileFormatRegistry::new();
        reg.install_host_defaults(
            include_str!("defaults/default-file-format.toml"),
        );
        // tempfile 같은 디렉토리 만들기보다 root path 사용 — 디렉토리 매칭 동작만 확인.
        let dir = std::env::temp_dir();
        let t = FileTarget::new(dir);
        assert_eq!(
            reg.identify(&t, DetectDepth::Cheap),
            Some(DetectorId("$directory".into()))
        );
        // 파일 (확장자 없는 가짜 path) → IsDirectory 매칭 제외, 다른 detector 도 안 맞아 None
        let t = target("/nonexistent/file.no-such-ext");
        assert_eq!(reg.identify(&t, DetectDepth::Cheap), None);
    }

    #[test]
    fn user_disabled_overrides_host() {
        let reg = FileFormatRegistry::new();
        reg.install_host_defaults(
            include_str!("defaults/default-file-format.toml"),
        );
        // 사용자가 markdown detector 를 disable
        let user_toml = r#"
            [[detector]]
            id = "markdown"
            disabled = true
        "#;
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("file-handlers.toml");
        std::fs::write(&p, user_toml).unwrap();
        reg.install_user_config(&p);
        assert_eq!(reg.identify(&target("a/b.md"), DetectDepth::Cheap), None);
    }
}
