//! `FileFormatRegistry` — 등록된 detector 들을 관리하고 file 을 identify 한다.
//!
//! 출처별 contribution 을 따로 보관해 plugin uninstall 시 원본 복원 가능.
//! finalize 는 incremental — install/uninstall 호출 후 dirty 표시 + identify 시 1회.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::RwLock;

use tracing::warn;

use super::config::{validate_detector_decl, DetectorDecl, DetectorRuleDecl};
use super::evaluator::{evaluate_cheap, evaluate_deep, DeepCtx};
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
    ///
    /// - `DetectDepth::Cheap`: file IO 없음. 확장자/glob/is-directory 만.
    /// - `DetectDepth::Deep`: 같은 cheap rule 들 + magic/MIME 까지 평가. 한 호출 동안
    ///   `DeepCtx` 로 head/MIME 캐시 → detector 가 여러 magic rule 을 가져도 head 는 1회만 read.
    pub fn identify(&self, target: &FileTarget, depth: DetectDepth) -> Option<DetectorId> {
        self.ensure_finalized();
        let inner = self.inner.read().ok()?;
        let is_dir = target.is_directory();

        let mut deep_ctx = match depth {
            DetectDepth::Deep => Some(DeepCtx::new()),
            DetectDepth::Cheap => None,
        };

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
            for rule in &det.rules {
                let matched = match deep_ctx.as_mut() {
                    Some(ctx) => evaluate_deep(&rule.kind, target, ctx),
                    None => evaluate_cheap(&rule.kind, target),
                };
                if matched {
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
            // Plugin 출처의 Lua detector rule 은 차단: host/user 만 사용자 권한으로
            // 실행되는 신뢰 영역. plugin 의 임의 Lua 는 sandbox 가 있어도 정책상 금지.
            let mut decl = decl.clone();
            let before = decl.rule.len();
            decl.rule
                .retain(|r| !matches!(r, DetectorRuleDecl::Lua { .. }));
            let dropped = before - decl.rule.len();
            if dropped > 0 {
                warn!(
                    plugin = plugin_id,
                    detector = %decl.id,
                    dropped,
                    "file_format: dropping Lua detector rules from plugin (host/user only)",
                );
            }
            if decl.rule.is_empty() {
                // 모든 rule 이 Lua 였다면 detector 자체 install 의미 없음.
                continue;
            }
            install_one(
                &mut inner,
                decl,
                RuleOrigin::Plugin(plugin_id.to_string()),
                true,
            );
        }
        inner.dirty = true;
    }

    /// 사용자 설정 파일을 다시 읽어 user origin contribution 만 교체. host + plugin 은 그대로.
    ///
    /// **Transactional**: 파일 read/parse 단계에서 실패하면 기존 user contribution 을 보존한다
    /// (write lock 잡기 전에 검증). 파일이 없으면 user contribution 만 제거 (= 설정 삭제).
    pub fn reload_user_config(&self, path: &Path) {
        let decls = match std::fs::read_to_string(path) {
            Ok(text) => match parse_detector_section(&text) {
                Ok(d) => d,
                Err(e) => {
                    warn!(
                        path = %path.display(),
                        error = %e,
                        "file_format: reload aborted — parse failed, keeping previous user config",
                    );
                    return;
                }
            },
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Vec::new(),
            Err(e) => {
                warn!(
                    path = %path.display(),
                    error = %e,
                    "file_format: reload aborted — read failed, keeping previous user config",
                );
                return;
            }
        };
        let mut inner = match self.inner.write() {
            Ok(g) => g,
            Err(_) => return,
        };
        // 기존 user origin contribution 모두 제거.
        let mut empty_ids = Vec::new();
        for (id, contribs) in inner.contributions.iter_mut() {
            contribs.retain(|c| !matches!(c.origin, RuleOrigin::User));
            if contribs.is_empty() {
                empty_ids.push(id.clone());
            }
        }
        for id in empty_ids {
            inner.contributions.remove(&id);
        }
        // 새 user contribution install.
        for decl in decls {
            install_one(&mut inner, decl, RuleOrigin::User, false);
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
        DetectorRuleDecl::Lua { script } => DetectorRuleKind::Lua { script },
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
    fn plugin_lua_rule_dropped_with_warn() {
        let reg = FileFormatRegistry::new();
        // plugin 이 Lua 와 Extension 을 섞어서 제공. Lua 만 drop 되고 Extension 은 유지.
        let decls = vec![DetectorDecl {
            id: "weird-fmt".into(),
            display_name_i18n_key: None,
            icon: None,
            disabled: false,
            rule: vec![
                DetectorRuleDecl::Lua {
                    script: "return true".into(),
                },
                DetectorRuleDecl::Extension {
                    values: vec!["wf".into()],
                },
            ],
        }];
        reg.install_plugin_detectors("com.example.weird", &decls);
        // Lua drop 후에도 Extension rule 이 살아 있어 매칭 가능.
        let id = reg.identify(&target("x.wf"), DetectDepth::Deep);
        assert_eq!(id, Some(DetectorId("weird-fmt".into())));
    }

    #[test]
    fn plugin_lua_only_detector_skipped() {
        let reg = FileFormatRegistry::new();
        // Lua 만 들어있는 detector — install 자체가 무의미해서 skip.
        let decls = vec![DetectorDecl {
            id: "lua-only".into(),
            display_name_i18n_key: None,
            icon: None,
            disabled: false,
            rule: vec![DetectorRuleDecl::Lua {
                script: "return true".into(),
            }],
        }];
        reg.install_plugin_detectors("com.example.lua-only", &decls);
        // detector 자체가 등록되지 않으므로 어떤 파일에도 안 잡힘.
        assert_eq!(
            reg.identify(&target("anything"), DetectDepth::Deep),
            None
        );
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
    fn identify_deep_matches_magic_when_cheap_misses() {
        let reg = FileFormatRegistry::new();
        // 호스트 default 는 사용 안 함 — 확장자가 없는 파일이 magic byte 로 매칭되는지 확인.
        // 사용자 정의 detector: extension 매칭 실패해도 magic 으로 매칭.
        let user_toml = r#"
            [[detector]]
            id = "png"
            [[detector.rule]]
            kind = "extension"
            values = ["png"]
            [[detector.rule]]
            kind = "magic"
            offset = 0
            bytes_hex = "89504E470D0A1A0A"
        "#;
        let dir = tempfile::tempdir().unwrap();
        let cfg = dir.path().join("file-handlers.toml");
        std::fs::write(&cfg, user_toml).unwrap();
        reg.install_user_config(&cfg);

        // 확장자가 .dat 인 PNG 파일.
        let png_sig = [0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A];
        let img_path = dir.path().join("masquerade.dat");
        std::fs::write(&img_path, png_sig).unwrap();
        let t = FileTarget::new(img_path);

        // Cheap → 확장자 안 맞음, magic 평가 안 함 → None
        assert_eq!(reg.identify(&t, DetectDepth::Cheap), None);
        // Deep → magic 매칭 → Some("png")
        assert_eq!(
            reg.identify(&t, DetectDepth::Deep),
            Some(DetectorId("png".into()))
        );
    }

    #[test]
    fn reload_user_config_replaces_user_entries_keeps_host() {
        let reg = FileFormatRegistry::new();
        reg.install_host_defaults(
            include_str!("defaults/default-file-format.toml"),
        );
        // 1차: 사용자가 pdf detector 추가.
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("file-handlers.toml");
        std::fs::write(
            &p,
            r#"
                [[detector]]
                id = "pdf"
                [[detector.rule]]
                kind = "extension"
                values = ["pdf"]
            "#,
        )
        .unwrap();
        reg.install_user_config(&p);
        assert_eq!(
            reg.identify(&target("a/b.pdf"), DetectDepth::Cheap),
            Some(DetectorId("pdf".into()))
        );

        // 2차: 사용자가 pdf 를 빼고 csv 추가 → reload.
        std::fs::write(
            &p,
            r#"
                [[detector]]
                id = "csv"
                [[detector.rule]]
                kind = "extension"
                values = ["csv"]
            "#,
        )
        .unwrap();
        reg.reload_user_config(&p);

        // pdf 는 host default 에 없으므로 (user 만) 사라져야 함.
        assert_eq!(reg.identify(&target("a/b.pdf"), DetectDepth::Cheap), None);
        // csv 는 새로 잡힘.
        assert_eq!(
            reg.identify(&target("a/b.csv"), DetectDepth::Cheap),
            Some(DetectorId("csv".into()))
        );
        // host default markdown 은 그대로.
        assert_eq!(
            reg.identify(&target("a/b.md"), DetectDepth::Cheap),
            Some(DetectorId("markdown".into()))
        );
    }

    #[test]
    fn reload_user_config_missing_file_clears_user_entries() {
        let reg = FileFormatRegistry::new();
        reg.install_host_defaults(
            include_str!("defaults/default-file-format.toml"),
        );
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("file-handlers.toml");
        std::fs::write(
            &p,
            r#"
                [[detector]]
                id = "pdf"
                [[detector.rule]]
                kind = "extension"
                values = ["pdf"]
            "#,
        )
        .unwrap();
        reg.install_user_config(&p);
        assert!(reg.detector(&DetectorId("pdf".into())).is_some());

        // 파일 삭제 후 reload → user origin 제거.
        std::fs::remove_file(&p).unwrap();
        reg.reload_user_config(&p);
        assert!(reg.detector(&DetectorId("pdf".into())).is_none());
        // host markdown 은 보존.
        assert!(reg.detector(&DetectorId("markdown".into())).is_some());
    }

    #[test]
    fn reload_user_config_parse_error_keeps_previous_state() {
        let reg = FileFormatRegistry::new();
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("file-handlers.toml");
        std::fs::write(
            &p,
            r#"
                [[detector]]
                id = "pdf"
                [[detector.rule]]
                kind = "extension"
                values = ["pdf"]
            "#,
        )
        .unwrap();
        reg.install_user_config(&p);
        assert!(reg.detector(&DetectorId("pdf".into())).is_some());

        // 파일을 의도적으로 깨뜨림 → reload 는 거부, 기존 user 항목 보존.
        std::fs::write(&p, "[[detector\n id = broken").unwrap();
        reg.reload_user_config(&p);
        assert!(reg.detector(&DetectorId("pdf".into())).is_some());
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
