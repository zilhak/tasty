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
/// - 첫째: 실행 파일 옆 `plugins/` (release/dist에서 packaging 시 함께 복사).
/// - 둘째: dev 환경 보조 — `target/<profile>/builtin-plugins/` (build helper가 채움).
/// - 환경 변수 `TASTY_BUILTIN_PLUGINS_DIR`로 강제 override 가능.
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
    let dev_bundle = exe_dir.join("builtin-plugins");
    if dev_bundle.is_dir() {
        return Some(dev_bundle);
    }
    None
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
