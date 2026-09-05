//! Plugin extension 상태 관리.
//!
//! `[extends]` 블록을 선언한 plugin은 다른 plugin(target)의 IPC/이벤트 흐름을
//! 가로채는 *확장 plugin*이 된다. 본 모듈은 어떤 extension이 어떤 target에 대해
//! 활성 상태인지를 추적한다.
//!
//! **상태 머신**:
//!
//! ```text
//! ┌─────────┐  user enable          ┌────────┐
//! │Disabled │ ───────────────────▶ │Pending │ ─┐  target compatible &
//! └─────────┘                       └────────┘ │  not conflicting
//!     ▲                                  ▲    ▼
//!     │  user disable                    │ ┌────────┐
//!     │                                  └─│ Active │
//!     │                                    └────────┘
//!     │  user disable                       │
//!     └─────────────────────────────────────┘
//!
//! Conflict: 같은 target을 잡은 다른 active extension이 이미 있을 때.
//! ```
//!
//! 본 PR(2/7)은 *상태 추적*만 한다. 실제 hook 실행은 후속 PR(4 event, 5 ipc)에서
//! 이 registry의 `active_extension_for_target`을 조회해 dispatch한다.

use std::collections::HashMap;

use tasty_plugin_manifest::{ExtendsDecl, Manifest};

/// 한 extension plugin의 현재 상태.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExtensionState {
    /// 활성. target도 존재하고 호환되며 사용자도 enable해 둠.
    Active {
        target_id: String,
        target_version: String,
    },
    /// target 부재 또는 호환성 깨짐. hook 등록되지 않음.
    Pending(PendingReason),
    /// 사용자가 명시적으로 비활성화. recompute에서 Active로 자동 승격되지 않는다.
    Disabled,
    /// 다른 extension이 동일 target을 이미 점유. 우선순위 규칙은
    /// `recompute`의 정의 참조.
    Conflict { other_extension_id: String },
}

/// `Pending` 상태의 세부 사유.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PendingReason {
    /// target plugin이 설치 자체가 안 됨.
    TargetMissing,
    /// target은 설치돼 있으나 사용자가 disable.
    TargetDisabled,
    /// target.version이 extension의 `version_req`를 만족하지 않음.
    VersionMismatch {
        target_version: String,
        required: String,
    },
    /// target.version이 semver 형식이 아님 (target plugin의 매니페스트 오류).
    InvalidTargetVersion { target_version: String },
    /// 매니페스트는 `ext:<target>` 권한을 declare했으나 사용자가 아직 grant 안 함.
    PermissionNotGranted,
}

/// extension 등록 상태 머신. PluginManager가 소유.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct ExtensionRegistry {
    /// extension plugin id → 현재 상태. extends 블록이 없는 plugin은 등록되지 않음.
    states: HashMap<String, ExtensionState>,
}

impl ExtensionRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// extension plugin의 현재 상태를 조회. 등록되지 않은 plugin은 `None`.
    pub fn state(&self, extension_id: &str) -> Option<&ExtensionState> {
        self.states.get(extension_id)
    }

    /// `target_id`를 확장하는 *active* extension plugin id (있다면).
    /// hook dispatch 시 사용 — 단일 A+ per A 제약이라 0 또는 1개만 반환.
    pub fn active_extension_for_target(&self, target_id: &str) -> Option<&str> {
        self.states.iter().find_map(|(ext_id, state)| match state {
            ExtensionState::Active { target_id: t, .. } if t == target_id => Some(ext_id.as_str()),
            _ => None,
        })
    }

    /// 모든 extension 상태를 (id, state) iterator로 반환. UI/디버그용.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &ExtensionState)> {
        self.states.iter().map(|(k, v)| (k.as_str(), v))
    }

    /// 전체 상태를 재계산. plugin 디스커버리/enable/disable/install/remove 후
    /// 매번 호출한다.
    ///
    /// 입력:
    /// - `packages`: 현재 설치된 모든 plugin의 매니페스트
    /// - `is_disabled`: plugin id → 사용자가 disable했는가
    /// - `has_extension_grant`: (extension_id, target_id) → 사용자가 `ext:<target>` 권한을
    ///   grant했는가. extension은 매니페스트에 토큰을 declare해도 grant 전엔 Pending 유지.
    ///
    /// 충돌 결정 규칙(단일 A+ per A): 같은 target을 잡은 후보 extension이 둘 이상이면
    /// **plugin id의 사전식 순서 최솟값**이 Active를 차지하고 나머지는 Conflict.
    /// 결정적·재현 가능한 정책으로 1.0에서는 충분. 사용자가 우선순위를 조정하고
    /// 싶으면 충돌하는 한쪽을 disable한다.
    pub fn recompute(
        &mut self,
        manifests: &[&Manifest],
        is_disabled: &dyn Fn(&str) -> bool,
        has_extension_grant: &dyn Fn(&str, &str) -> bool,
    ) {
        // 1) 빈 상태로 시작 — extends 블록이 있는 plugin만 새로 채워넣는다.
        let mut next: HashMap<String, ExtensionState> = HashMap::new();

        // 2) 모든 extension 후보를 모은다. (extension_id, &decl)
        let extensions: Vec<(&str, &ExtendsDecl)> = manifests
            .iter()
            .filter_map(|m| m.extends.as_ref().map(|d| (m.id.as_str(), d)))
            .collect();

        // 3) 각 extension별 1차 평가 — Disabled / Pending / Active-후보로 분류.
        //    Conflict는 이 단계에서 결정할 수 없다 (다른 후보와 비교해야).
        //    target lookup은 manifests에서 plugin_id 매칭.
        let target_for = |target_id: &str| -> Option<&&Manifest> {
            manifests.iter().find(|m| m.id == target_id)
        };

        // (ext_id, decl, classification) — Active-후보면 classification은 None.
        //                                   확정된 state면 Some(state).
        type Classification = Option<ExtensionState>;
        let mut classified: Vec<(&str, &ExtendsDecl, Classification)> = Vec::new();

        for (ext_id, decl) in &extensions {
            if is_disabled(ext_id) {
                classified.push((ext_id, decl, Some(ExtensionState::Disabled)));
                continue;
            }
            if !has_extension_grant(ext_id, &decl.plugin_id) {
                classified.push((
                    ext_id,
                    decl,
                    Some(ExtensionState::Pending(PendingReason::PermissionNotGranted)),
                ));
                continue;
            }
            let Some(target) = target_for(&decl.plugin_id) else {
                classified.push((
                    ext_id,
                    decl,
                    Some(ExtensionState::Pending(PendingReason::TargetMissing)),
                ));
                continue;
            };
            if is_disabled(&target.id) {
                classified.push((
                    ext_id,
                    decl,
                    Some(ExtensionState::Pending(PendingReason::TargetDisabled)),
                ));
                continue;
            }
            // version 매칭. target.version, decl.version_req 모두 검증.
            let target_ver = match semver::Version::parse(&target.version) {
                Ok(v) => v,
                Err(_) => {
                    classified.push((
                        ext_id,
                        decl,
                        Some(ExtensionState::Pending(
                            PendingReason::InvalidTargetVersion {
                                target_version: target.version.clone(),
                            },
                        )),
                    ));
                    continue;
                }
            };
            let req = match semver::VersionReq::parse(&decl.version_req) {
                Ok(r) => r,
                Err(_) => {
                    // 매니페스트 검증에서 이미 걸렀어야 하지만 안전망.
                    classified.push((
                        ext_id,
                        decl,
                        Some(ExtensionState::Pending(PendingReason::VersionMismatch {
                            target_version: target.version.clone(),
                            required: decl.version_req.clone(),
                        })),
                    ));
                    continue;
                }
            };
            if !req.matches(&target_ver) {
                classified.push((
                    ext_id,
                    decl,
                    Some(ExtensionState::Pending(PendingReason::VersionMismatch {
                        target_version: target.version.clone(),
                        required: decl.version_req.clone(),
                    })),
                ));
                continue;
            }
            // Active 후보.
            classified.push((ext_id, decl, None));
        }

        // 4) 충돌 해결. target_id → 그 target에 대한 Active 후보 ext_id 일람.
        let mut candidates_by_target: HashMap<&str, Vec<&str>> = HashMap::new();
        for (ext_id, decl, cls) in &classified {
            if cls.is_none() {
                candidates_by_target
                    .entry(decl.plugin_id.as_str())
                    .or_default()
                    .push(ext_id);
            }
        }
        // 각 target에 대해 사전식 최솟값을 winner로 선택.
        let mut winners: HashMap<&str, &str> = HashMap::new();
        for (target, mut cands) in candidates_by_target {
            cands.sort_unstable();
            if let Some(winner) = cands.first() {
                winners.insert(target, *winner);
            }
        }

        // 5) 최종 상태 확정.
        for (ext_id, decl, cls) in classified {
            let state = match cls {
                Some(s) => s,
                None => {
                    let target =
                        target_for(&decl.plugin_id).expect("target present in active candidate");
                    let winner = winners.get(decl.plugin_id.as_str()).copied();
                    if winner == Some(ext_id) {
                        ExtensionState::Active {
                            target_id: decl.plugin_id.clone(),
                            target_version: target.version.clone(),
                        }
                    } else {
                        ExtensionState::Conflict {
                            other_extension_id: winner.unwrap_or("").to_string(),
                        }
                    }
                }
            };
            next.insert(ext_id.to_string(), state);
        }

        self.states = next;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tasty_plugin_manifest::{
        Contributes, Entry, EventHookDecl, ExtendsDecl, HookMode, IpcHookDecl,
    };

    fn mk_manifest(id: &str, version: &str, extends: Option<ExtendsDecl>) -> Manifest {
        Manifest {
            manifest_version: 1,
            id: id.to_string(),
            name: id.to_string(),
            version: version.to_string(),
            authors: vec![],
            description: String::new(),
            homepage: String::new(),
            api_version: "1".to_string(),
            entry: Entry::Process {
                command: "x".to_string(),
                args: vec![],
            },
            surface_kinds: vec![],
            permissions: vec![],
            event_subscribe: vec![],
            event_publish: vec![],
            events_emitted: vec![],
            contributes: Contributes::default(),
            extends,
            lang_dir: "lang".to_string(),
            bundle: true,
        }
    }

    fn mk_extends(target: &str, req: &str) -> ExtendsDecl {
        ExtendsDecl {
            plugin_id: target.to_string(),
            version_req: req.to_string(),
            api_version: "1".to_string(),
            pre_event: vec![],
            post_event: vec![],
            pre_ipc: vec![IpcHookDecl {
                method: "foo.bar".to_string(),
                modifies: vec!["entry".into()],
                mode: HookMode::Transform,
                timeout_ms: 100,
            }],
            post_ipc: vec![],
        }
    }

    fn never_disabled(_: &str) -> bool {
        false
    }

    fn always_granted(_: &str, _: &str) -> bool {
        true
    }

    #[test]
    fn empty_manifests_yield_empty_registry() {
        let mut reg = ExtensionRegistry::new();
        reg.recompute(&[], &never_disabled, &always_granted);
        assert_eq!(reg.iter().count(), 0);
    }

    #[test]
    fn non_extension_plugin_is_not_tracked() {
        let m = mk_manifest("com.a.target", "1.0.0", None);
        let mut reg = ExtensionRegistry::new();
        reg.recompute(&[&m], &never_disabled, &always_granted);
        assert!(reg.state("com.a.target").is_none());
    }

    #[test]
    fn active_when_target_compatible_and_enabled() {
        let target = mk_manifest("com.a.target", "1.2.0", None);
        let ext = mk_manifest(
            "com.b.ext",
            "0.1.0",
            Some(mk_extends("com.a.target", ">=1.0.0, <2.0.0")),
        );
        let mut reg = ExtensionRegistry::new();
        reg.recompute(&[&target, &ext], &never_disabled, &always_granted);
        match reg.state("com.b.ext") {
            Some(ExtensionState::Active {
                target_id,
                target_version,
            }) => {
                assert_eq!(target_id, "com.a.target");
                assert_eq!(target_version, "1.2.0");
            }
            other => panic!("expected Active, got {other:?}"),
        }
        assert_eq!(
            reg.active_extension_for_target("com.a.target"),
            Some("com.b.ext")
        );
    }

    #[test]
    fn pending_when_target_missing() {
        let ext = mk_manifest(
            "com.b.ext",
            "0.1.0",
            Some(mk_extends("com.a.target", ">=1.0.0")),
        );
        let mut reg = ExtensionRegistry::new();
        reg.recompute(&[&ext], &never_disabled, &always_granted);
        assert_eq!(
            reg.state("com.b.ext"),
            Some(&ExtensionState::Pending(PendingReason::TargetMissing))
        );
    }

    #[test]
    fn pending_when_target_disabled() {
        let target = mk_manifest("com.a.target", "1.0.0", None);
        let ext = mk_manifest(
            "com.b.ext",
            "0.1.0",
            Some(mk_extends("com.a.target", ">=1.0.0")),
        );
        let mut reg = ExtensionRegistry::new();
        reg.recompute(
            &[&target, &ext],
            &|id| id == "com.a.target",
            &always_granted,
        );
        assert_eq!(
            reg.state("com.b.ext"),
            Some(&ExtensionState::Pending(PendingReason::TargetDisabled))
        );
    }

    #[test]
    fn pending_when_version_mismatch() {
        let target = mk_manifest("com.a.target", "0.5.0", None);
        let ext = mk_manifest(
            "com.b.ext",
            "0.1.0",
            Some(mk_extends("com.a.target", ">=1.0.0")),
        );
        let mut reg = ExtensionRegistry::new();
        reg.recompute(&[&target, &ext], &never_disabled, &always_granted);
        match reg.state("com.b.ext") {
            Some(ExtensionState::Pending(PendingReason::VersionMismatch {
                target_version,
                required,
            })) => {
                assert_eq!(target_version, "0.5.0");
                assert_eq!(required, ">=1.0.0");
            }
            other => panic!("expected VersionMismatch, got {other:?}"),
        }
    }

    #[test]
    fn pending_when_target_version_not_semver() {
        let target = mk_manifest("com.a.target", "not-a-version", None);
        let ext = mk_manifest(
            "com.b.ext",
            "0.1.0",
            Some(mk_extends("com.a.target", ">=1.0.0")),
        );
        let mut reg = ExtensionRegistry::new();
        reg.recompute(&[&target, &ext], &never_disabled, &always_granted);
        match reg.state("com.b.ext") {
            Some(ExtensionState::Pending(PendingReason::InvalidTargetVersion {
                target_version,
            })) => assert_eq!(target_version, "not-a-version"),
            other => panic!("expected InvalidTargetVersion, got {other:?}"),
        }
    }

    #[test]
    fn disabled_when_extension_user_disabled() {
        let target = mk_manifest("com.a.target", "1.0.0", None);
        let ext = mk_manifest(
            "com.b.ext",
            "0.1.0",
            Some(mk_extends("com.a.target", ">=1.0.0")),
        );
        let mut reg = ExtensionRegistry::new();
        reg.recompute(&[&target, &ext], &|id| id == "com.b.ext", &always_granted);
        assert_eq!(reg.state("com.b.ext"), Some(&ExtensionState::Disabled));
        assert!(reg.active_extension_for_target("com.a.target").is_none());
    }

    #[test]
    fn conflict_uses_lexicographic_winner() {
        let target = mk_manifest("com.a.target", "1.0.0", None);
        let b1 = mk_manifest(
            "com.b.alpha",
            "0.1.0",
            Some(mk_extends("com.a.target", ">=1.0.0")),
        );
        let b2 = mk_manifest(
            "com.b.beta",
            "0.1.0",
            Some(mk_extends("com.a.target", ">=1.0.0")),
        );
        let mut reg = ExtensionRegistry::new();
        reg.recompute(&[&target, &b1, &b2], &never_disabled, &always_granted);
        // alpha < beta alphabetically — alpha wins.
        assert!(matches!(
            reg.state("com.b.alpha"),
            Some(ExtensionState::Active { .. })
        ));
        match reg.state("com.b.beta") {
            Some(ExtensionState::Conflict { other_extension_id }) => {
                assert_eq!(other_extension_id, "com.b.alpha");
            }
            other => panic!("expected Conflict, got {other:?}"),
        }
        assert_eq!(
            reg.active_extension_for_target("com.a.target"),
            Some("com.b.alpha")
        );
    }

    #[test]
    fn disabling_target_demotes_active_to_pending() {
        let target = mk_manifest("com.a.target", "1.0.0", None);
        let ext = mk_manifest(
            "com.b.ext",
            "0.1.0",
            Some(mk_extends("com.a.target", ">=1.0.0")),
        );
        let mut reg = ExtensionRegistry::new();
        reg.recompute(&[&target, &ext], &never_disabled, &always_granted);
        assert!(matches!(
            reg.state("com.b.ext"),
            Some(ExtensionState::Active { .. })
        ));
        // user disables target
        reg.recompute(
            &[&target, &ext],
            &|id| id == "com.a.target",
            &always_granted,
        );
        assert_eq!(
            reg.state("com.b.ext"),
            Some(&ExtensionState::Pending(PendingReason::TargetDisabled))
        );
        // re-enable target
        reg.recompute(&[&target, &ext], &never_disabled, &always_granted);
        assert!(matches!(
            reg.state("com.b.ext"),
            Some(ExtensionState::Active { .. })
        ));
    }

    #[test]
    fn event_hook_extension_also_supported() {
        let target = mk_manifest("com.a.target", "1.0.0", None);
        let mut decl = mk_extends("com.a.target", ">=1.0.0");
        decl.pre_ipc.clear();
        decl.pre_event.push(EventHookDecl {
            event: "com.a.target.something".into(),
            modifies: vec!["payload".into()],
            mode: HookMode::Transform,
            timeout_ms: 100,
        });
        let ext = mk_manifest("com.b.ext", "0.1.0", Some(decl));
        let mut reg = ExtensionRegistry::new();
        reg.recompute(&[&target, &ext], &never_disabled, &always_granted);
        assert!(matches!(
            reg.state("com.b.ext"),
            Some(ExtensionState::Active { .. })
        ));
    }

    #[test]
    fn pending_when_extension_permission_not_granted() {
        let target = mk_manifest("com.a.target", "1.0.0", None);
        let ext = mk_manifest(
            "com.b.ext",
            "0.1.0",
            Some(mk_extends("com.a.target", ">=1.0.0")),
        );
        let mut reg = ExtensionRegistry::new();
        let no_grant = |_: &str, _: &str| false;
        reg.recompute(&[&target, &ext], &never_disabled, &no_grant);
        assert_eq!(
            reg.state("com.b.ext"),
            Some(&ExtensionState::Pending(
                PendingReason::PermissionNotGranted
            ))
        );
        assert!(reg.active_extension_for_target("com.a.target").is_none());
    }

    #[test]
    fn granting_permission_promotes_pending_to_active() {
        let target = mk_manifest("com.a.target", "1.0.0", None);
        let ext = mk_manifest(
            "com.b.ext",
            "0.1.0",
            Some(mk_extends("com.a.target", ">=1.0.0")),
        );
        let mut reg = ExtensionRegistry::new();
        // 처음: grant 없음.
        let no_grant = |_: &str, _: &str| false;
        reg.recompute(&[&target, &ext], &never_disabled, &no_grant);
        assert!(matches!(
            reg.state("com.b.ext"),
            Some(ExtensionState::Pending(PendingReason::PermissionNotGranted))
        ));
        // grant 후: active 승격.
        let grant_b_for_a = |ext_id: &str, target_id: &str| -> bool {
            ext_id == "com.b.ext" && target_id == "com.a.target"
        };
        reg.recompute(&[&target, &ext], &never_disabled, &grant_b_for_a);
        assert!(matches!(
            reg.state("com.b.ext"),
            Some(ExtensionState::Active { .. })
        ));
    }
}
