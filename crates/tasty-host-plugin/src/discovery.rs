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
    let root = match plugin_root() {
        Some(r) => r,
        None => return Vec::new(),
    };
    if !root.exists() {
        return Vec::new();
    }
    let read_dir = match std::fs::read_dir(&root) {
        Ok(rd) => rd,
        Err(e) => {
            tracing::warn!("plugin discovery: cannot read {}: {}", root.display(), e);
            return Vec::new();
        }
    };
    let mut packages = Vec::new();
    for entry in read_dir.flatten() {
        let dir = entry.path();
        if !dir.is_dir() {
            continue;
        }
        if !dir.join("tasty-plugin.toml").exists() {
            continue;
        }
        // F.B.11-4: host file 도메인 결합 (bridge::validate_bin_extras) 은 본
        // 바이너리 caller (App::plugin_install / cli::plugin / view::add) 가 chain.
        // discover 자체는 schema 검증만으로 충분 — 잘못된 detector/handler 는
        // install_plugin_handlers 시점에 거부된다.
        match Manifest::load(&dir) {
            Ok(manifest) => {
                if !is_trusted(&dir, &manifest.id) {
                    continue;
                }
                packages.push(PluginPackage { dir, manifest });
            }
            Err(e) => tracing::warn!("plugin '{}' rejected: {}", dir.display(), e),
        }
    }
    packages.sort_by(|a, b| a.manifest.id.cmp(&b.manifest.id));

    // (정렬 + dedup 은 trust gate 와 무관 — 위에서 이미 filter 완료.)
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
    packages
}

/// 매니페스트 sig + trust DB 로 *이 디렉토리 plugin 이 자동 로드해도 되는지*
/// 판정. debug 빌드는 항상 true (dev workspace bundle 미서명).
///
/// release 빌드:
/// - `verify_bundle_signature` → `TrustDecision::Trusted` 면 true
/// - `Untrusted` (UnknownKey / PermissionsChanged) 면 false + warn
/// - `Err` (sidecar missing / read error / invalid length) 도 false + warn
fn is_trusted(dir: &std::path::Path, plugin_id: &str) -> bool {
    #[cfg(debug_assertions)]
    {
        let _ = (dir, plugin_id); // debug 빌드 sig-skip — 인자는 release 분기 전용.
        true
    }
    #[cfg(not(debug_assertions))]
    {
        use crate::bundle_sig::{TrustDecision, verify_bundle_signature};
        match verify_bundle_signature(dir) {
            Ok(TrustDecision::Trusted) => true,
            Ok(TrustDecision::Untrusted { reason, .. }) => {
                tracing::warn!(
                    "plugin '{}' not auto-loaded (untrusted: {:?}). Use \
                     Settings → Plugins → Add to trust.",
                    plugin_id,
                    reason
                );
                false
            }
            Err(e) => {
                tracing::warn!("plugin '{}' signature check failed: {e}", plugin_id);
                false
            }
        }
    }
}
