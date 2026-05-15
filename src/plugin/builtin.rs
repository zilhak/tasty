//! 기본 제공 플러그인 (built-in) 인프라.
//!
//! Tasty는 일부 plugin (예: explorer, codex)을 본 바이너리와 함께 배포한다. 이들은:
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

/// 한 builtin plugin의 패키지 메타 — id, dev workspace crate 경로, plugin 바이너리 이름.
struct BuiltinSpec {
    id: &'static str,
    /// `crates/<crate_dir>/` — dev 빌드의 매니페스트/lang 원본 위치.
    crate_dir: &'static str,
    /// `target/<profile>/<bin_name>` — dev 빌드된 plugin 실행 바이너리 이름.
    bin_name: &'static str,
}

#[cfg(windows)]
const BUILTINS: &[BuiltinSpec] = &[
    BuiltinSpec {
        id: "com.tasty.explorer",
        crate_dir: "tasty-plugin-explorer",
        bin_name: "tasty-plugin-explorer.exe",
    },
    BuiltinSpec {
        id: "com.tasty.codex",
        crate_dir: "tasty-plugin-codex",
        bin_name: "tasty-plugin-codex.exe",
    },
    BuiltinSpec {
        id: "com.tasty.claude",
        crate_dir: "tasty-plugin-claude",
        bin_name: "tasty-plugin-claude.exe",
    },
    BuiltinSpec {
        id: "com.tasty.image",
        crate_dir: "tasty-plugin-image",
        bin_name: "tasty-plugin-image.exe",
    },
    BuiltinSpec {
        id: "com.tasty.clipboard-history",
        crate_dir: "tasty-plugin-clipboard-history",
        bin_name: "tasty-plugin-clipboard-history.exe",
    },
];

#[cfg(not(windows))]
const BUILTINS: &[BuiltinSpec] = &[
    BuiltinSpec {
        id: "com.tasty.explorer",
        crate_dir: "tasty-plugin-explorer",
        bin_name: "tasty-plugin-explorer",
    },
    BuiltinSpec {
        id: "com.tasty.codex",
        crate_dir: "tasty-plugin-codex",
        bin_name: "tasty-plugin-codex",
    },
    BuiltinSpec {
        id: "com.tasty.claude",
        crate_dir: "tasty-plugin-claude",
        bin_name: "tasty-plugin-claude",
    },
    BuiltinSpec {
        id: "com.tasty.image",
        crate_dir: "tasty-plugin-image",
        bin_name: "tasty-plugin-image",
    },
    BuiltinSpec {
        id: "com.tasty.clipboard-history",
        crate_dir: "tasty-plugin-clipboard-history",
        bin_name: "tasty-plugin-clipboard-history",
    },
];

pub fn is_builtin_plugin(id: &str) -> bool {
    BUILTINS.iter().any(|b| b.id == id)
}

/// 번들 plugin 디렉터리들이 있는 루트 경로.
///
/// - 첫째: `TASTY_BUILTIN_PLUGINS_DIR` 환경 변수 강제 override.
/// - 둘째: 실행 파일 옆 `plugins/` (release/dist에서 packaging 시 함께 복사).
/// - 셋째: dev 빌드일 때 workspace 자동 탐색 — `target/<profile>/builtin-plugins/`에
///   각 builtin plugin의 매니페스트와 빌드된 바이너리를 mtime 비교 후 갱신.
///   `cargo build`만 하면 자동 반영됨.
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
/// 등록된 builtin plugin들의 manifest+binary+lang을 동기화. mtime이 더 새것일
/// 때만 복사하므로 매 부팅 비용은 작다. 한 plugin이라도 동기화에 성공했으면
/// Some(bundle_root). workspace를 못 찾으면 None.
#[cfg(debug_assertions)]
fn ensure_dev_bundle(exe_dir: &Path) -> Option<PathBuf> {
    // exe_dir = .../target/<profile>
    let target_dir = exe_dir.parent()?; // .../target
    let workspace = target_dir.parent()?; // workspace root

    let bundle_root = exe_dir.join("builtin-plugins");
    let mut any_synced = false;
    for spec in BUILTINS {
        if sync_builtin_dev(workspace, exe_dir, &bundle_root, spec) {
            any_synced = true;
        }
    }
    if any_synced {
        Some(bundle_root)
    } else {
        // bundle_root가 이미 존재할 수도 있다 (이전 부팅에서 동기화됨).
        // 그 경우엔 bundle_root() 호출자가 별도 분기에서 fallback으로 발견.
        if bundle_root.is_dir() {
            Some(bundle_root)
        } else {
            None
        }
    }
}

/// 한 builtin plugin을 dev bundle로 동기화. 바이너리 또는 매니페스트가
/// workspace에 없으면 (예: codex만 빌드 안 됨) false 반환.
#[cfg(debug_assertions)]
fn sync_builtin_dev(
    workspace: &Path,
    exe_dir: &Path,
    bundle_root: &Path,
    spec: &BuiltinSpec,
) -> bool {
    let plugin_bin = exe_dir.join(spec.bin_name);
    let src_manifest = workspace
        .join("crates")
        .join(spec.crate_dir)
        .join("tasty-plugin.toml");
    if !plugin_bin.exists() || !src_manifest.exists() {
        return false;
    }

    let dest_dir = bundle_root.join(spec.id);
    if let Err(e) = std::fs::create_dir_all(&dest_dir) {
        tracing::warn!("dev bundle: mkdir {} failed: {e}", dest_dir.display());
        return false;
    }
    if let Err(e) = copy_if_newer(&src_manifest, &dest_dir.join("tasty-plugin.toml")) {
        tracing::warn!("dev bundle: copy manifest for {} failed: {e}", spec.id);
        return false;
    }
    if let Err(e) = copy_if_newer(&plugin_bin, &dest_dir.join(spec.bin_name)) {
        tracing::warn!("dev bundle: copy binary for {} failed: {e}", spec.id);
        return false;
    }
    // plugin lang/ 디렉토리도 함께 동기화 (i18n 키 호스트 머지에 필요).
    let src_lang = workspace.join("crates").join(spec.crate_dir).join("lang");
    if src_lang.is_dir() {
        let dest_lang = dest_dir.join("lang");
        if let Err(e) = sync_dir_if_newer(&src_lang, &dest_lang) {
            tracing::warn!("dev bundle: copy lang for {} failed: {e}", spec.id);
        }
    }
    true
}

#[cfg(debug_assertions)]
fn sync_dir_if_newer(src: &Path, dst: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let ty = entry.file_type()?;
        let dest_path = dst.join(entry.file_name());
        if ty.is_dir() {
            sync_dir_if_newer(&entry.path(), &dest_path)?;
        } else {
            copy_if_newer(&entry.path(), &dest_path)?;
        }
    }
    Ok(())
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
    copy_atomic(src, dest)
}

/// `std::fs::copy`의 안전 대체 — temp 파일에 쓰고 atomic rename으로 dest에 swap.
///
/// macOS에서 같은 경로에 binary를 in-place 덮어쓰면 kernel이 캐시한 code signature가
/// invalid로 판정되어 다음 exec 시 `SIGKILL (Code Signature Invalid)`로 죽는다
/// (Taskgated). rename은 inode를 교체하므로 kernel이 새 시그니처를 다시 읽는다.
///
/// Linux/Windows에서도 동일하게 동작 — partial-write race 방지 효과까지 덤으로 얻는다.
fn copy_atomic(src: &Path, dest: &Path) -> std::io::Result<()> {
    let parent = dest.parent().ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "destination path has no parent",
        )
    })?;
    // 같은 디렉터리 안에 temp를 만들어야 rename이 cross-filesystem 에러를 안 낸다.
    let pid = std::process::id();
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);
    let file_name = dest.file_name().and_then(|s| s.to_str()).unwrap_or("file");
    let tmp = parent.join(format!(".{file_name}.tmp.{pid}.{nanos:x}"));

    if let Err(e) = std::fs::copy(src, &tmp) {
        // best-effort temp 정리. copy 실패 시 src/dest 권한 문제가 원인일 확률이 높고
        // remove_file도 함께 실패해도 알릴 곳이 없으므로 의도적으로 무시.
        let _ = std::fs::remove_file(&tmp);
        return Err(e);
    }
    if let Err(e) = std::fs::rename(&tmp, dest) {
        // 위와 동일: rename 실패 시 temp 잔여물은 best-effort로만 정리.
        let _ = std::fs::remove_file(&tmp);
        return Err(e);
    }
    Ok(())
}

/// 모든 기본 제공 플러그인을 점검:
/// 1. 사용자 디렉터리에 없고 `removed_builtins` 목록에도 없으면 번들에서 복사 +
///    매니페스트 권한을 자동 grant.
/// 2. 이미 사용자 디렉터리에 있지만 `plugins.toml`에 grant 엔트리가 한 번도
///    기록된 적 없는 builtin은 매니페스트 권한을 자동 grant (이전 버전에서
///    builtin으로 인식되지 않은 채 설치된 plugin 복구). 사용자가 명시적으로
///    빈 리스트로 둔 경우(`granted = []`)는 entry는 있으니 건드리지 않는다.
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
    let bundle = bundle_root();

    let mut config_dirty = false;
    for spec in BUILTINS {
        let dest = dest_root.join(spec.id);
        let already_present = dest.exists();

        // Step 1: 번들에서 복사. 신규 설치 + 기존 설치된 builtin의 manifest/binary
        // 갱신을 동일 경로에서 처리한다. builtin은 호스트 소유 리소스이므로 사용자
        // 디렉터리에 있더라도 번들이 더 새것이면 덮어쓴다 (사용자가 직접 편집하는
        // 용도가 아님).
        if !mgr.config.is_builtin_removed(spec.id) {
            if let Some(bundle) = bundle.as_ref() {
                let src = bundle.join(spec.id);
                if !src.is_dir() {
                    tracing::debug!(
                        "builtin plugin '{}' not in bundle ({}), skipping",
                        spec.id,
                        src.display()
                    );
                } else {
                    if let Err(e) = std::fs::create_dir_all(&dest_root) {
                        tracing::warn!(
                            "install_builtins: mkdir {} failed: {e}",
                            dest_root.display()
                        );
                        continue;
                    }
                    if already_present {
                        if let Err(e) = sync_dir_recursive_if_newer(&src, &dest) {
                            tracing::warn!("install_builtins: sync '{}' failed: {e}", spec.id);
                            continue;
                        }
                    } else {
                        if let Err(e) = copy_dir_recursive(&src, &dest) {
                            tracing::warn!("install_builtins: copy '{}' failed: {e}", spec.id);
                            continue;
                        }
                        tracing::info!("installed builtin plugin '{}' from bundle", spec.id);
                    }
                }
            }
        }

        // Step 2: dest가 존재하고 grant entry가 없으면 매니페스트 권한 자동 grant.
        if dest.exists() && !mgr.config.grants.contains_key(spec.id) {
            if let Ok(manifest) = Manifest::load(&dest) {
                if !manifest.permissions.is_empty() {
                    mgr.config
                        .set_granted(spec.id, manifest.permissions.clone());
                    config_dirty = true;
                    tracing::info!(
                        "auto-granted manifest permissions for builtin '{}'",
                        spec.id
                    );
                }
            }
        }
    }
    if config_dirty {
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
            copy_atomic(&entry.path(), &dest_path)?;
        }
    }
    Ok(())
}

/// `copy_dir_recursive`의 idempotent 버전. `src`의 각 파일이 `dst`보다 더 새것일
/// 때만 복사한다. 사용자 디렉터리에 이미 설치된 builtin을 번들 최신본으로 갱신할
/// 때 사용 — 동일한 manifest는 건너뛰고, 매니페스트/바이너리 변경분만 반영한다.
fn sync_dir_recursive_if_newer(src: &Path, dst: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let ty = entry.file_type()?;
        let dest_path = dst.join(entry.file_name());
        if ty.is_dir() {
            sync_dir_recursive_if_newer(&entry.path(), &dest_path)?;
        } else {
            copy_file_if_newer(&entry.path(), &dest_path)?;
        }
    }
    Ok(())
}

fn copy_file_if_newer(src: &Path, dst: &Path) -> std::io::Result<()> {
    if let (Ok(src_meta), Ok(dst_meta)) = (std::fs::metadata(src), std::fs::metadata(dst)) {
        if let (Ok(sm), Ok(dm)) = (src_meta.modified(), dst_meta.modified()) {
            if sm <= dm {
                return Ok(());
            }
        }
    }
    copy_atomic(src, dst)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explorer_is_builtin() {
        assert!(is_builtin_plugin("com.tasty.explorer"));
    }

    #[test]
    fn codex_is_builtin() {
        assert!(is_builtin_plugin("com.tasty.codex"));
    }

    #[test]
    fn claude_is_builtin() {
        assert!(is_builtin_plugin("com.tasty.claude"));
    }

    #[test]
    fn image_is_builtin() {
        assert!(is_builtin_plugin("com.tasty.image"));
    }

    #[test]
    fn clipboard_history_is_builtin() {
        assert!(is_builtin_plugin("com.tasty.clipboard-history"));
    }

    #[test]
    fn unknown_is_not_builtin() {
        assert!(!is_builtin_plugin("com.example.foo"));
    }
}
