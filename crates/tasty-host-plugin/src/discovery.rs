//! `~/.tasty/plugins/` 스캔 + 매니페스트 파싱. 실패한 plugin은 warn 로그 후 스킵.

use std::path::PathBuf;

use tasty_plugin_manifest::{Manifest, PluginPackage};

pub fn plugin_root() -> Option<PathBuf> {
    tasty_utils::path::tasty_home().map(|d| d.join("plugins"))
}

/// `~/.tasty/plugins/`를 스캔하여 매니페스트 파싱 성공한 plugin들을 반환.
/// id 기준 정렬, 중복은 첫 번째만 보존.
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
            Ok(manifest) => packages.push(PluginPackage { dir, manifest }),
            Err(e) => tracing::warn!("plugin '{}' rejected: {}", dir.display(), e),
        }
    }
    packages.sort_by(|a, b| a.manifest.id.cmp(&b.manifest.id));
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
