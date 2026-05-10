//! 기본 제공 플러그인 (built-in) 인프라.
//!
//! Tasty는 일부 plugin (예: explorer)을 본 바이너리와 함께 배포한다. 이들은:
//!
//! 1. **번들 위치**에서 디스커버됨 — release/dist 빌드: 실행 파일 옆 `plugins/`
//!    디렉터리, dev 빌드: `target/<profile>/builtin-plugins/` (build helper로 채움).
//! 2. **첫 실행 시** `~/.tasty/plugins/<id>/`에 복사됨 — 사용자가 손댈 수 있는
//!    실제 설치 위치는 사용자 디렉터리 한 곳뿐. `plugins.toml`의
//!    `removed_builtins`에 등록된 id는 자동 복사하지 않는다.
//! 3. **uninstall** 시 `removed_builtins`에 추가되어 다음 실행에서 재등장하지 않음.
//!
//! 외부 플러그인과의 차이는 *발생지*뿐이다. 디스커버리·실행·권한 모델은 동일하다.

use std::path::{Path, PathBuf};

use crate::plugin::manifest::Manifest;
use crate::plugin::{discovery, PluginManager};

/// 기본 제공 플러그인 id 목록. dev/release 모두 동일.
pub const BUILTIN_PLUGIN_IDS: &[&str] = &["com.tasty.explorer"];

pub fn is_builtin_plugin(id: &str) -> bool {
    BUILTIN_PLUGIN_IDS.iter().any(|b| *b == id)
}

/// 번들 plugin 디렉터리들이 있는 루트 경로.
///
/// - 첫째: `TASTY_BUILTIN_PLUGINS_DIR` 환경 변수 강제 override.
/// - 둘째: 실행 파일 옆 `plugins/` (release/dist에서 packaging 시 함께 복사).
/// - 셋째: dev 빌드일 때 workspace 자동 탐색 — `target/<profile>/builtin-plugins/`에
///   `crates/tasty-plugin-explorer/tasty-plugin.toml`과 빌드된 plugin binary를
///   매 부팅마다 mtime 비교 후 갱신. `cargo build`만 하면 자동 반영됨.
pub fn bundle_root() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("TASTY_BUILTIN_PLUGINS_DIR") {
        let path = PathBuf::from(p);
        if path.is_dir() {
            return Some(path);
        }
    }
    let exe = std::env::current_exe().ok()?;
    let exe_dir = exe.parent()?;
    let next_to_exe = exe_dir.join("plugins");
    if next_to_exe.is_dir() {
        return Some(next_to_exe);
    }
    #[cfg(debug_assertions)]
    if let Some(dev) = ensure_dev_bundle(exe_dir) {
        return Some(dev);
    }
    let dev_bundle = exe_dir.join("builtin-plugins");
    if dev_bundle.is_dir() {
        return Some(dev_bundle);
    }
    None
}

/// dev 빌드에서 workspace를 자동 탐색하여 `target/<profile>/builtin-plugins/`에
/// builtin plugin들의 manifest+binary를 동기화. mtime이 더 새것일 때만 복사하므로
/// 매 부팅 비용은 작다. workspace를 못 찾거나 plugin binary가 없으면 None.
#[cfg(debug_assertions)]
fn ensure_dev_bundle(exe_dir: &Path) -> Option<PathBuf> {
    // exe_dir = .../target/<profile>
    let target_dir = exe_dir.parent()?; // .../target
    let workspace = target_dir.parent()?; // workspace root

    let bin_name = if cfg!(windows) {
        "tasty-plugin-explorer.exe"
    } else {
        "tasty-plugin-explorer"
    };
    let plugin_bin = exe_dir.join(bin_name);
    let src_manifest = workspace
        .join("crates")
        .join("tasty-plugin-explorer")
        .join("tasty-plugin.toml");
    if !plugin_bin.exists() || !src_manifest.exists() {
        return None;
    }

    let bundle_root = exe_dir.join("builtin-plugins");
    let dest_dir = bundle_root.join("com.tasty.explorer");
    if let Err(e) = std::fs::create_dir_all(&dest_dir) {
        tracing::warn!("dev bundle: mkdir {} failed: {e}", dest_dir.display());
        return None;
    }
    if let Err(e) = copy_if_newer(&src_manifest, &dest_dir.join("tasty-plugin.toml")) {
        tracing::warn!("dev bundle: copy manifest failed: {e}");
        return None;
    }
    if let Err(e) = copy_if_newer(&plugin_bin, &dest_dir.join(bin_name)) {
        tracing::warn!("dev bundle: copy binary failed: {e}");
        return None;
    }
    Some(bundle_root)
}

/// src가 dest보다 더 최신이거나 dest가 없으면 복사. 이미 같거나 dest가 더 최신이면 no-op.
fn copy_if_newer(src: &Path, dest: &Path) -> std::io::Result<()> {
    if let (Ok(src_meta), Ok(dest_meta)) = (std::fs::metadata(src), std::fs::metadata(dest)) {
        if let (Ok(sm), Ok(dm)) = (src_meta.modified(), dest_meta.modified()) {
            if sm <= dm {
                return Ok(());
            }
        }
    }
    std::fs::copy(src, dest)?;
    Ok(())
}

/// 모든 기본 제공 플러그인을 점검: 사용자 디렉터리에 없고 `removed_builtins`
/// 목록에도 없으면 번들에서 복사. 호출자는 이후 `discover()`를 다시 돌려야 함.
///
/// 실패한 항목은 warn 로그만 남기고 계속 진행 (다른 builtin은 영향 없음).
pub fn install_builtins_if_needed(mgr: &mut PluginManager) {
    let dest_root = match discovery::plugin_root() {
        Some(p) => p,
        None => {
            tracing::warn!("install_builtins: cannot resolve plugin root");
            return;
        }
    };
    let bundle = match bundle_root() {
        Some(p) => p,
        None => {
            // 번들이 없는 환경 (예: dev 빌드에서 build helper를 아직 안 돌림).
            // 이 경우 묵묵히 넘어간다 — 외부 plugin만 사용하는 흐름과 동일.
            return;
        }
    };

    let mut installed_any = false;
    for id in BUILTIN_PLUGIN_IDS {
        let dest = dest_root.join(id);
        if dest.exists() {
            continue;
        }
        if mgr.config.is_builtin_removed(id) {
            continue;
        }
        let src = bundle.join(id);
        if !src.is_dir() {
            tracing::debug!(
                "builtin plugin '{}' not in bundle ({}), skipping",
                id,
                src.display()
            );
            continue;
        }
        if let Err(e) = std::fs::create_dir_all(&dest_root) {
            tracing::warn!("install_builtins: mkdir {} failed: {e}", dest_root.display());
            continue;
        }
        if let Err(e) = copy_dir_recursive(&src, &dest) {
            tracing::warn!("install_builtins: copy '{id}' failed: {e}");
            continue;
        }
        // 매니페스트의 모든 권한을 grant (built-in은 사용자가 명시 거부하기 전엔 신뢰).
        if let Ok(manifest) = Manifest::load(&dest) {
            mgr.config.set_granted(&manifest.id, manifest.permissions.clone());
        }
        tracing::info!("installed builtin plugin '{id}' from bundle");
        installed_any = true;
    }
    if installed_any {
        if let Err(e) = mgr.config.save() {
            tracing::warn!("install_builtins: save plugins.toml failed: {e}");
        }
    }
}

/// uninstall 흐름에서 호출 — built-in인 경우 `removed_builtins`에 등록하여
/// 다음 부팅의 `install_builtins_if_needed`가 다시 복사하지 않게 한다.
/// 외부 플러그인이면 no-op.
pub fn mark_builtin_removed(mgr: &mut PluginManager, id: &str) {
    if !is_builtin_plugin(id) {
        return;
    }
    if mgr.config.mark_builtin_removed(id) {
        if let Err(e) = mgr.config.save() {
            tracing::warn!("mark_builtin_removed: save plugins.toml failed: {e}");
        }
    }
}

fn copy_dir_recursive(src: &Path, dst: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let ty = entry.file_type()?;
        let dest_path = dst.join(entry.file_name());
        if ty.is_dir() {
            copy_dir_recursive(&entry.path(), &dest_path)?;
        } else {
            std::fs::copy(entry.path(), &dest_path)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explorer_is_builtin() {
        assert!(is_builtin_plugin("com.tasty.explorer"));
    }

    #[test]
    fn unknown_is_not_builtin() {
        assert!(!is_builtin_plugin("com.example.foo"));
    }
}
