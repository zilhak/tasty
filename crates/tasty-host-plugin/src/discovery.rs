//! `~/.tasty/plugins/` 스캔 + 매니페스트 파싱. 실패한 plugin은 warn 로그 후 스킵.
//!
//! ## Trust gate (Step H 대안)
//!
//! discover 결과는 *trusted* plugin 만 포함한다. 즉:
//! - 임베드 키 (`TRUSTED_PUBKEYS`) 통과
//! - `~/.tasty/known-plugins.toml` 의 사용자 trust 항목 통과
//!
//! 둘 다 실패 (`TrustDecision::Untrusted`) 하거나 서명 검증 자체가 에러
//! (`SigVerifyError::SidecarMissing` 등) 인 plugin 은 *list 에 안 들어간다*.
//! 따라서 silent install (`~/.tasty/plugins/` 에 임의로 디렉토리 떨어뜨려도
//! 자동 로드 X) 이 차단된다. 사용자가 "Add plugin" 탭에서 명시적으로 추가하면,
//! 그 시점에 사용자 trust 가 known-plugins.toml 에 기록되고, 다음 discover
//! 부터 자동 통과한다.
//!
//! debug 빌드 (dev workspace bundle 이 unsigned) 는 trust gate 를 우회하여
//! 모든 매니페스트-파싱 통과 plugin 을 로드한다 — release 의 silent-install
//! 방어와 dev 의 빠른 iteration 둘 다 보장.

use std::path::PathBuf;

use tasty_plugin_manifest::{Manifest, PluginPackage};

pub fn plugin_root() -> Option<PathBuf> {
    tasty_utils::path::tasty_home().map(|d| d.join("plugins"))
}

/// `~/.tasty/plugins/`를 스캔하여 매니페스트 파싱 + (release) trust gate 통과한
/// plugin들을 반환. id 기준 정렬, 중복은 첫 번째만 보존.
pub fn discover() -> Vec<PluginPackage> {
    discover_with_rejections().0
}

/// `discover` 와 동일하지만, trust gate 에서 거부된 plugin(서명 미신뢰 / 검증
/// 실패 / 권한 변경)도 함께 수집해 `(통과, 거부)` 로 반환한다. UI 의 "확인 필요"
/// 목록과 사이드바 경고 배지가 이 거부 목록을 소비한다. debug 빌드는 trust gate
/// 를 우회하므로(`trust_outcome` 가 항상 Trusted) 거부 목록은 항상 비어 있다.
pub fn discover_with_rejections() -> (Vec<PluginPackage>, Vec<RejectedPlugin>) {
    let root = match plugin_root() {
        Some(r) => r,
        None => return (Vec::new(), Vec::new()),
    };
    if !root.exists() {
        return (Vec::new(), Vec::new());
    }
    let read_dir = match std::fs::read_dir(&root) {
        Ok(rd) => rd,
        Err(e) => {
            tracing::warn!("plugin discovery: cannot read {}: {}", root.display(), e);
            return (Vec::new(), Vec::new());
        }
    };
    let mut packages = Vec::new();
    let mut rejected = Vec::new();
    for entry in read_dir.flatten() {
        classify_discovery_entry(entry.path(), &mut packages, &mut rejected);
    }
    packages.sort_by(|a, b| a.manifest.id.cmp(&b.manifest.id));
    // (정렬 + dedup 은 trust gate 와 무관 — 위에서 이미 filter 완료.)
    dedup_packages_by_id(&mut packages);
    (packages, rejected)
}

/// 한 디렉토리 항목을 검사해 trust gate 통과 여부에 따라 packages/rejected 에 분류.
/// plugin 디렉토리가 아니거나 매니페스트가 없으면 조용히 skip.
fn classify_discovery_entry(
    dir: PathBuf,
    packages: &mut Vec<PluginPackage>,
    rejected: &mut Vec<RejectedPlugin>,
) {
    if !dir.is_dir() || !dir.join("tasty-plugin.toml").exists() {
        return;
    }
    // F.B.11-4: host file 도메인 결합 (bridge::validate_bin_extras) 은 본
    // 바이너리 caller (App::plugin_install / cli::plugin / view::add) 가 chain.
    // discover 자체는 schema 검증만으로 충분 — 잘못된 detector/handler 는
    // install_plugin_handlers 시점에 거부된다.
    match Manifest::load(&dir) {
        Ok(manifest) => match trust_outcome(&dir, &manifest) {
            TrustOutcome::Trusted => packages.push(PluginPackage { dir, manifest }),
            TrustOutcome::Rejected(rej) => {
                tracing::warn!("plugin '{}' not auto-loaded ({:?})", rej.id, rej.reason);
                rejected.push(rej);
            }
        },
        Err(e) => tracing::warn!("plugin '{}' rejected: {}", dir.display(), e),
    }
}

/// 정렬된 packages 에서 중복 id 를 제거 — 먼저 나온(정렬 후 첫) 항목만 보존.
fn dedup_packages_by_id(packages: &mut Vec<PluginPackage>) {
    let mut seen = std::collections::HashSet::new();
    packages.retain(|pkg| {
        if seen.contains(&pkg.manifest.id) {
            tracing::warn!(
                "duplicate plugin id '{}' at {} — keeping first",
                pkg.manifest.id,
                pkg.dir.display()
            );
            false
        } else {
            seen.insert(pkg.manifest.id.clone());
            true
        }
    });
}

/// trust gate 에서 거부된 plugin 한 건. UI "확인 필요" 탭 + 사이드바 경고 배지
/// 가 소비한다. 매니페스트는 파싱에 성공했으나 서명/신뢰 검증에서 떨어진
/// 경우만 기록한다 (매니페스트 파싱 실패는 식별 정보가 없어 제외).
#[derive(Debug, Clone)]
pub struct RejectedPlugin {
    pub id: String,
    pub name: String,
    pub version: String,
    pub authors: Vec<String>,
    /// builtin(번들) plugin 이면 true — UI 가 "built-in" 태그를 붙인다.
    pub builtin: bool,
    pub reason: RejectionReason,
    /// 서명 키 지문 (UnknownKey/PermissionsChanged 일 때). 표시용.
    pub fingerprint: Option<String>,
    /// PermissionsChanged 일 때 신뢰 시점 대비 새로 요구된 권한.
    pub permissions_added: Vec<String>,
    /// PermissionsChanged 일 때 더 이상 쓰지 않는 권한.
    pub permissions_removed: Vec<String>,
}

/// plugin 이 자동 로드에서 거부된 사유. `bundle_sig::TrustDecision` 을 UI 친화적
/// 으로 단순화한 것.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RejectionReason {
    /// 신뢰 목록에 없는 키로 서명됨 → 등록 거부.
    UnknownKey,
    /// 서명 누락/손상/검증 실패 → 등록 거부.
    SignatureInvalid,
    /// 한 번 신뢰했으나 매니페스트 권한이 바뀜 → 재승인 필요.
    PermissionsChanged,
}

// debug 빌드는 trust gate 를 우회해 `Rejected` 를 절대 만들지 않는다 — 그쪽에선
// dead_code 경고가 정상이므로 억제한다 (release 에선 정상 사용).
// 이유: debug 는 trust gate 를 우회해 `Rejected` 를 만들지 않는다(위) — release 에선 정상 사용.
#[cfg_attr(debug_assertions, allow(dead_code))]
enum TrustOutcome {
    Trusted,
    Rejected(RejectedPlugin),
}

/// 매니페스트 sig + trust DB 로 *이 디렉토리 plugin 이 자동 로드해도 되는지* 판정.
/// debug 빌드는 항상 Trusted (dev workspace bundle 미서명).
///
/// release 빌드:
/// - `verify_bundle_signature` → `Trusted` 면 통과
/// - `Untrusted` (UnknownKey / PermissionsChanged) → 사유와 함께 Rejected
/// - `Err` (sidecar missing / read error / invalid length) → SignatureInvalid Rejected
fn trust_outcome(dir: &std::path::Path, manifest: &Manifest) -> TrustOutcome {
    #[cfg(debug_assertions)]
    {
        let _ = (dir, manifest); // debug 빌드 sig-skip — 인자는 release 분기 전용.
        TrustOutcome::Trusted
    }
    #[cfg(not(debug_assertions))]
    {
        use crate::bundle_sig::{TrustDecision, UntrustedReason, verify_bundle_signature};
        let mk = |reason, fingerprint, added: Vec<String>, removed: Vec<String>| {
            TrustOutcome::Rejected(RejectedPlugin {
                id: manifest.id.clone(),
                name: manifest.name.clone(),
                version: manifest.version.clone(),
                authors: manifest.authors.clone(),
                builtin: crate::builtin::is_builtin_plugin(&manifest.id),
                reason,
                fingerprint,
                permissions_added: added,
                permissions_removed: removed,
            })
        };
        match verify_bundle_signature(dir) {
            Ok(TrustDecision::Trusted) => TrustOutcome::Trusted,
            Ok(TrustDecision::Untrusted {
                fingerprint,
                manifest_permissions,
                reason,
                ..
            }) => match reason {
                UntrustedReason::PermissionsChanged => {
                    let (added, removed) = perm_diff(&manifest.id, &manifest_permissions);
                    mk(
                        RejectionReason::PermissionsChanged,
                        Some(fingerprint),
                        added,
                        removed,
                    )
                }
                UntrustedReason::UnknownKey => mk(
                    RejectionReason::UnknownKey,
                    Some(fingerprint),
                    vec![],
                    vec![],
                ),
            },
            Err(e) => {
                tracing::warn!("plugin '{}' signature check failed: {e}", manifest.id);
                mk(RejectionReason::SignatureInvalid, None, vec![], vec![])
            }
        }
    }
}

/// 신뢰 시점(known-plugins.toml)의 권한 대비 새 매니페스트 권한의 added/removed.
#[cfg(not(debug_assertions))]
fn perm_diff(plugin_id: &str, new_perms: &[String]) -> (Vec<String>, Vec<String>) {
    let old: Vec<String> = crate::known_plugins::KnownPlugins::load()
        .ok()
        .and_then(|k| k.lookup(plugin_id).map(|e| e.permissions.clone()))
        .unwrap_or_default();
    let added = new_perms
        .iter()
        .filter(|p| !old.contains(p))
        .cloned()
        .collect();
    let removed = old
        .iter()
        .filter(|p| !new_perms.contains(p))
        .cloned()
        .collect();
    (added, removed)
}
