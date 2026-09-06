//! 기본 제공 플러그인 (built-in) 인프라.
//!
//! Tasty는 일부 plugin (예: explorer, codex)을 본 바이너리와 함께 배포한다. 이들은:
//!
//! 1. **번들 위치**에서 디스커버됨 — 배포 패키지(release/dist): 실행 파일 옆
//!    `plugins/` 디렉터리, workspace 빌드(debug/release 무관): workspace 를 자동
//!    탐색해 `target/<profile>/builtin-plugins/`를 채움 (`ensure_dev_bundle`).
//! 2. **첫 실행 시** `~/.tasty/plugins/<id>/`에 복사됨 — 사용자가 손댈 수 있는
//!    실제 설치 위치는 사용자 디렉터리 한 곳뿐. `plugins.toml`의
//!    `removed_builtins`에 등록된 id는 자동 복사하지 않는다.
//! 3. **uninstall** 시 `removed_builtins`에 추가되어 다음 실행에서 재등장하지 않음.
//!
//! 외부 플러그인과의 차이는 *발생지*뿐이다. 디스커버리·실행·권한 모델은 동일하다.
//!
//! 위 2 는 **첫 설치**만 설명한다. 첫 복사 이후 플러그인 코드/매니페스트를 고쳤을
//! 때 **호스트 재시작 없이** 실행 중 tasty 에 반영하는 절차(재빌드 → `upgrade-builtins`
//! 재sync → `disable`/`enable` respawn)는 `docs/dev-guide/plugin-development.md` §9.1
//! 참조. 재sync 는 매니페스트 `version` 을 올렸을 때만 일어난다(same-version skip).

use std::collections::HashSet;
use std::ffi::OsString;
use std::path::{Path, PathBuf};

use crate::{PluginManager, PluginsConfig, discovery};
use tasty_plugin_manifest::Manifest;

/// 한 builtin plugin의 패키지 메타 — id, dev workspace crate 경로, plugin 바이너리 이름.
struct BuiltinSpec {
    id: &'static str,
    /// `crates/<crate_dir>/` — workspace 빌드의 매니페스트/lang 원본 위치.
    /// workspace 가 실제로 존재할 때만 dev sync 에 사용된다 (배포 패키지에서는 미사용).
    crate_dir: &'static str,
    /// `target/<profile>/<bin_name>` — workspace 빌드된 plugin 실행 바이너리 이름.
    bin_name: &'static str,
}

// explorer 는 T11 에서 host builtin surface 로 승격됨 — plugin 번들에서 제거.
#[cfg(windows)]
const BUILTINS: &[BuiltinSpec] = &[
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
        id: "com.tasty.markdown",
        crate_dir: "tasty-plugin-markdown",
        bin_name: "tasty-plugin-markdown.exe",
    },
    BuiltinSpec {
        id: "com.tasty.clipboard-viewer",
        crate_dir: "tasty-plugin-clipboard-viewer",
        bin_name: "tasty-plugin-clipboard-viewer.exe",
    },
    BuiltinSpec {
        id: "com.tasty.html",
        crate_dir: "tasty-plugin-html",
        bin_name: "tasty-plugin-html.exe",
    },
    BuiltinSpec {
        id: "com.tasty.git-viewer",
        crate_dir: "tasty-plugin-git-viewer",
        bin_name: "tasty-plugin-git-viewer.exe",
    },
    BuiltinSpec {
        id: "com.tasty.mesh-demo",
        crate_dir: "tasty-plugin-mesh-demo",
        bin_name: "tasty-plugin-mesh-demo.exe",
    },
    BuiltinSpec {
        id: "com.tasty.agent-stream",
        crate_dir: "tasty-plugin-agent-stream",
        bin_name: "tasty-plugin-agent-stream.exe",
    },
];

#[cfg(not(windows))]
const BUILTINS: &[BuiltinSpec] = &[
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
        id: "com.tasty.markdown",
        crate_dir: "tasty-plugin-markdown",
        bin_name: "tasty-plugin-markdown",
    },
    BuiltinSpec {
        id: "com.tasty.clipboard-viewer",
        crate_dir: "tasty-plugin-clipboard-viewer",
        bin_name: "tasty-plugin-clipboard-viewer",
    },
    BuiltinSpec {
        id: "com.tasty.html",
        crate_dir: "tasty-plugin-html",
        bin_name: "tasty-plugin-html",
    },
    BuiltinSpec {
        id: "com.tasty.git-viewer",
        crate_dir: "tasty-plugin-git-viewer",
        bin_name: "tasty-plugin-git-viewer",
    },
    BuiltinSpec {
        id: "com.tasty.mesh-demo",
        crate_dir: "tasty-plugin-mesh-demo",
        bin_name: "tasty-plugin-mesh-demo",
    },
    BuiltinSpec {
        id: "com.tasty.agent-stream",
        crate_dir: "tasty-plugin-agent-stream",
        bin_name: "tasty-plugin-agent-stream",
    },
];

pub fn is_builtin_plugin(id: &str) -> bool {
    BUILTINS.iter().any(|b| b.id == id)
}

/// 자동 builtin upgrade 시 *어떤* 분기로 처리할지 결정한 결과.
///
/// `install_builtins_if_needed` 의 already-present 분기와 신규 `upgrade_builtins`
/// 양쪽에서 공유. mtime 비신뢰성을 피하기 위해 manifest 의 `version` (semver)
/// 을 1차 기준으로 사용한다.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum BuiltinUpgradeDecision {
    /// 변경 없음 (installed >= bundle, 또는 bundle 매니페스트 corrupt).
    Skip,
    /// 같은 semver — mtime 기반 idempotent sync 만 수행 (dev workspace hotfix).
    ResyncSameVersion,
    /// bundle > installed — 디렉토리 전체를 mtime 무시하고 덮어씀.
    UpgradeVersion {
        from: semver::Version,
        to: semver::Version,
    },
    /// 사용자가 `--force` 로 명시 요청 — 동일/하위 버전도 강제 재설치.
    ForceOverwrite,
}

/// dest 의 `tasty-plugin.toml` 에서 semver 만 읽음. corrupt/parse-fail → None.
fn read_installed_version(dest: &Path) -> Option<semver::Version> {
    Manifest::load(dest)
        .ok()
        .and_then(|m| semver::Version::parse(&m.version).ok())
}

/// bundle 의 `tasty-plugin.toml` 에서 semver 만 읽음. corrupt/parse-fail → None.
fn read_bundle_version(src: &Path) -> Option<semver::Version> {
    Manifest::load(src)
        .ok()
        .and_then(|m| semver::Version::parse(&m.version).ok())
}

/// installed / bundle / force 입력으로 upgrade 의사 결정.
///
/// 규칙 (verify §5.5 반영):
/// - force=true → 무조건 `ForceOverwrite`
/// - bundle 매니페스트 파싱 실패 → `Skip` (corrupt bundle 보호)
/// - installed 파싱 실패 + bundle ok → `ResyncSameVersion` (사용자 dir 손상 복구)
/// - 둘 다 ok → bundle vs installed semver 비교
pub(crate) fn decide_builtin_upgrade(
    installed: Option<&semver::Version>,
    bundle: Option<&semver::Version>,
    force: bool,
) -> BuiltinUpgradeDecision {
    if force {
        return BuiltinUpgradeDecision::ForceOverwrite;
    }
    let bundle = match bundle {
        Some(b) => b,
        None => return BuiltinUpgradeDecision::Skip,
    };
    let installed = match installed {
        Some(i) => i,
        None => return BuiltinUpgradeDecision::ResyncSameVersion,
    };
    match bundle.cmp(installed) {
        std::cmp::Ordering::Greater => BuiltinUpgradeDecision::UpgradeVersion {
            from: installed.clone(),
            to: bundle.clone(),
        },
        std::cmp::Ordering::Equal => BuiltinUpgradeDecision::ResyncSameVersion,
        std::cmp::Ordering::Less => BuiltinUpgradeDecision::Skip,
    }
}

/// dest 에 있는데 src 에 없는 항목을 제거한다 — **이 층만** 본다(재귀는 호출자가 한다).
///
/// **복사 정책과 독립이다.** 이 단계의 값은 src 목록과 dest 목록의 *차집합*이라 복사를
/// 얼마나 하든 같다. 한 함수에 묶여 있으면 복사 정책을 바꿀 때 청소가 함께 사라지는데,
/// 그 소실은 조용하다 — 옛 잔존 파일은 어떤 단정도 안 건드리고 사용자 홈에서만 쌓인다.
fn prune_dest_not_in_src(src: &Path, dest: &Path) -> std::io::Result<()> {
    let src_names: HashSet<OsString> = std::fs::read_dir(src)?
        .filter_map(|e| e.ok())
        .map(|e| e.file_name())
        .collect();
    for entry in std::fs::read_dir(dest)? {
        let entry = entry?;
        if !src_names.contains(&entry.file_name()) {
            if entry.file_type()?.is_dir() {
                std::fs::remove_dir_all(entry.path())?;
            } else {
                std::fs::remove_file(entry.path())?;
            }
        }
    }
    Ok(())
}

/// dest 의 모든 파일을 src 기준으로 강제 교체 (mtime 무시).
/// src 에 없는 dest 의 파일/디렉토리는 *제거* — 옛 버전의 잔존 파일 정리
/// ([`prune_dest_not_in_src`], 층마다 돈다).
fn overwrite_builtin_dir(src: &Path, dest: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dest)?;
    prune_dest_not_in_src(src, dest)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let dest_path = dest.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            overwrite_builtin_dir(&entry.path(), &dest_path)?;
        } else {
            copy_atomic(&entry.path(), &dest_path)?;
        }
    }
    Ok(())
}

/// 번들 plugin 디렉터리들이 있는 루트 경로.
///
/// - 첫째: `TASTY_BUILTIN_PLUGINS_DIR` 환경 변수 강제 override.
/// - 둘째 (macOS 한정): `.app` 번들의 `Contents/Resources/plugins/`.
/// - 셋째: 실행 파일 옆 `plugins/` (release/dist에서 packaging 시 함께 복사,
///   portable .tar.gz / .zip 설치).
/// - 넷째: workspace 빌드일 때 자동 탐색 — `target/<profile>/builtin-plugins/`에
///   각 builtin plugin의 매니페스트와 빌드된 바이너리를 mtime 비교 후 갱신.
///   `cargo build [--release] --workspace` 후 실행하면 자동 반영됨. workspace 가
///   없는 배포 패키지에서는 앞선 항목에서 이미 반환되어 여기 도달하지 않는다.
/// - 다섯째 (linux 한정): FHS 표준 경로 `/usr/lib/tasty/plugins/`,
///   `/usr/share/tasty/plugins/` — `.deb` / `.rpm` 패키지가 `/usr/bin/tasty` 옆이
///   아닌 FHS 친화 위치에 plugin 을 설치한다. exe-relative 보다 우선순위가
///   낮음 — 같은 머신에 portable archive 가 풀려있으면 그쪽이 항상 우선이라
///   두 설치 형태가 충돌하지 않는다.
pub fn bundle_root() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("TASTY_BUILTIN_PLUGINS_DIR") {
        let path = PathBuf::from(p);
        if path.is_dir() {
            return Some(path);
        }
    }
    if let Some(found) = bundle_root_exe_relative() {
        return Some(found);
    }
    #[cfg(target_os = "linux")]
    {
        for sys_path in ["/usr/lib/tasty/plugins", "/usr/share/tasty/plugins"] {
            let path = PathBuf::from(sys_path);
            if path.is_dir() {
                return Some(path);
            }
        }
    }
    None
}

/// exe-relative 경로들에서 bundle root 검색. 추출 사유: FHS fallback 진입 전
/// `current_exe()` 실패 (테스트 환경 등) 가 전체 None 으로 단락되지 않도록.
fn bundle_root_exe_relative() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let exe_dir = exe.parent()?;
    #[cfg(target_os = "macos")]
    {
        // .app 번들: `Contents/Resources/plugins/`. exe 옆(`Contents/MacOS/`)이
        // 아닌 이유는 codesign 이 `Contents/MacOS/` 하위의 실행 파일 디렉터리를
        // nested bundle 로 파싱하려다 실패하기 때문(bundle format unrecognized).
        // 아래 exe-relative 보다 먼저 본다 — 구 배치가 남아 있어도 새 것 우선.
        // exe 가 `Contents/MacOS/` 안에 있을 때로 한정한다. 그 확인이 없으면 번들이
        // 아닌 배치(예: `target/<profile>/tasty`)에서 우연히 `../Resources/plugins`
        // 가 존재할 때 exe 옆 `plugins/` 를 가로챈다.
        let bundle_contents = exe_dir
            .parent()
            .filter(|_| exe_dir.file_name().is_some_and(|n| n == "MacOS"));
        if let Some(contents) = bundle_contents {
            let in_resources = contents.join("Resources").join("plugins");
            if in_resources.is_dir() {
                return Some(in_resources);
            }
        }
    }
    let next_to_exe = exe_dir.join("plugins");
    if next_to_exe.is_dir() {
        return Some(next_to_exe);
    }
    if let Some(dev) = ensure_dev_bundle(exe_dir) {
        return Some(dev);
    }
    let dev_bundle = exe_dir.join("builtin-plugins");
    if dev_bundle.is_dir() {
        return Some(dev_bundle);
    }
    None
}

/// workspace 빌드(debug/release 무관)에서 workspace를 자동 탐색하여
/// `target/<profile>/builtin-plugins/`에 등록된 builtin plugin들의
/// manifest+binary+lang을 동기화. mtime이 더 새것일 때만 복사하므로 매 부팅 비용은
/// 작다. 한 plugin이라도 동기화에 성공했으면 Some(bundle_root). workspace를 못 찾으면
/// (= 배포 패키지처럼 `crates/`가 없으면) `sync_builtin_dev`가 전부 false 를 반환하여
/// None. 따라서 진짜 배포본에서는 자연히 no-op.
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
/// workspace에 없으면 (예: codex만 빌드 안 됨, 또는 배포 패키지라 `crates/`가 아예
/// 없음) false 반환. 이 가드가 release 빌드에서도 배포본을 안전하게 만든다.
fn sync_builtin_dev(
    workspace: &Path,
    exe_dir: &Path,
    bundle_root: &Path,
    spec: &BuiltinSpec,
) -> bool {
    let plugin_bin = exe_dir.join(spec.bin_name);
    let crate_dir = workspace.join("crates").join(spec.crate_dir);
    let src_manifest = crate_dir.join("tasty-plugin.toml");
    if !plugin_bin.exists() || !src_manifest.exists() {
        return false;
    }

    let dest_dir = bundle_root.join(spec.id);
    if !sync_builtin_dev_required(&plugin_bin, &src_manifest, &dest_dir, spec) {
        return false;
    }
    sync_builtin_dev_sig(&crate_dir, &dest_dir, spec);
    sync_builtin_dev_lang(&crate_dir, &dest_dir, spec);
    true
}

/// mkdir + 매니페스트 + 바이너리 복사 — 이 중 하나라도 실패하면 dev bundle
/// 자체가 무효(fatal). sig/lang 은 별도 best-effort 스텝으로 분리되어 있다.
/// 세 스텝을 `&&`로 묶어 첫 실패에서 단락 — 순서·조건은 원본과 동일.
fn sync_builtin_dev_required(
    plugin_bin: &Path,
    src_manifest: &Path,
    dest_dir: &Path,
    spec: &BuiltinSpec,
) -> bool {
    dev_sync_step(
        || std::fs::create_dir_all(dest_dir),
        || format!("dev bundle: mkdir {} failed", dest_dir.display()),
    ) && dev_sync_step(
        || copy_if_newer(src_manifest, &dest_dir.join("tasty-plugin.toml")),
        || format!("dev bundle: copy manifest for {} failed", spec.id),
    ) && dev_sync_step(
        || copy_if_newer(plugin_bin, &dest_dir.join(spec.bin_name)),
        || format!("dev bundle: copy binary for {} failed", spec.id),
    )
}

/// 한 fallible 스텝 실행 — 실패 시 `<msg>: <e>` 형식으로 warn 후 `false`.
fn dev_sync_step(op: impl FnOnce() -> std::io::Result<()>, msg: impl FnOnce() -> String) -> bool {
    if let Err(e) = op() {
        tracing::warn!("{}: {e}", msg());
        return false;
    }
    true
}

/// 매니페스트 서명 sidecar(.sig)가 있으면 함께 동기화한다. 없으면 (미서명 dev
/// workspace) skip — debug 빌드는 trust gate 를 우회하므로 무방하지만,
/// release/dist 산출물을 *직접 실행* 할 때는 sign-bundle.sh 로 생성된 .sig 가
/// bundle 에 있어야 install 후 trust gate(임베드 dev-pubkey)를 통과한다.
/// 복사 실패는 비치명 (debug 는 어차피 우회) 이므로 warn 만 남긴다.
fn sync_builtin_dev_sig(crate_dir: &Path, dest_dir: &Path, spec: &BuiltinSpec) {
    let src_sig = crate_dir.join("tasty-plugin.toml.sig");
    if src_sig.exists()
        && let Err(e) = copy_if_newer(&src_sig, &dest_dir.join("tasty-plugin.toml.sig"))
    {
        tracing::warn!("dev bundle: copy sig for {} failed: {e}", spec.id);
    }
}

/// plugin lang/ 디렉토리도 함께 동기화 (i18n 키 호스트 머지에 필요) — best-effort.
fn sync_builtin_dev_lang(crate_dir: &Path, dest_dir: &Path, spec: &BuiltinSpec) {
    let src_lang = crate_dir.join("lang");
    if src_lang.is_dir() {
        let dest_lang = dest_dir.join("lang");
        if let Err(e) = sync_dir_if_newer(&src_lang, &dest_lang) {
            tracing::warn!("dev bundle: copy lang for {} failed: {e}", spec.id);
        }
    }
}

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
    if let (Ok(src_meta), Ok(dest_meta)) = (std::fs::metadata(src), std::fs::metadata(dest))
        && let (Ok(sm), Ok(dm)) = (src_meta.modified(), dest_meta.modified())
        && sm <= dm
    {
        return Ok(());
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
///
/// macOS: rename 성공 후 `dest`의 `com.apple.quarantine` xattr을 best-effort로
/// 제거한다(`strip_quarantine`) — dist(.app) 번들은 Finder가 quarantine된 DMG에서
/// 드래그/추출될 때 내부 파일에도 재귀적으로 quarantine xattr을 남기고, 이 상태로
/// 남은 plugin 바이너리는 ad-hoc 서명(비공증)이라 Gatekeeper가 exec 자체를 막는다.
/// plugin 서브프로세스가 조용히 spawn 실패하면 host는 frame을 영원히 못 받아
/// surface가 완전히 빈 채로 멈춘다 — 이미 앱 자체의 Gatekeeper 승인을 통과해
/// 실행 중인 이 프로세스가 자신이 번들에서 방금 풀어낸 리소스의 quarantine을
/// 스스로 제거하는 것은 안전하다.
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
        // tmp가 부분 생성됐을 수 있으니 best-effort 정리. NotFound는 정상.
        if let Err(re) = std::fs::remove_file(&tmp)
            && re.kind() != std::io::ErrorKind::NotFound
        {
            tracing::trace!("builtin install tmp {} cleanup failed: {re}", tmp.display());
        }
        return Err(e);
    }
    if let Err(e) = std::fs::rename(&tmp, dest) {
        if let Err(re) = std::fs::remove_file(&tmp)
            && re.kind() != std::io::ErrorKind::NotFound
        {
            tracing::trace!("builtin install tmp {} cleanup failed: {re}", tmp.display());
        }
        return Err(e);
    }
    #[cfg(target_os = "macos")]
    strip_quarantine(dest);
    Ok(())
}

/// `dest`의 `com.apple.quarantine` xattr을 제거한다(있으면). 속성이 애초에 없는
/// 경우(`ENOATTR`)는 정상 상태(quarantine되지 않은 파일)이므로 조용히 넘어가고,
/// 그 외 실패는 warn만 남긴다 — 이 정리가 실패해도 파일 자체는 정상 복사됐으니
/// 설치 흐름을 막을 이유가 없다(best-effort).
#[cfg(target_os = "macos")]
fn strip_quarantine(dest: &Path) {
    use std::os::unix::ffi::OsStrExt;

    let Ok(c_path) = std::ffi::CString::new(dest.as_os_str().as_bytes()) else {
        return;
    };
    // SAFETY: c_path/c_attr 모두 유효한 NUL-terminated 버퍼이고, removexattr은
    // 이를 읽기만 한다(dest 파일 자체는 변경하지 않고 xattr만 제거).
    let ret = unsafe { libc::removexattr(c_path.as_ptr(), c"com.apple.quarantine".as_ptr(), 0) };
    if ret != 0 {
        let err = std::io::Error::last_os_error();
        if err.raw_os_error() != Some(libc::ENOATTR) {
            tracing::warn!(
                "strip quarantine xattr for {} failed: {err}",
                dest.display()
            );
        }
    }
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

        // Step 1: 번들에서 복사. 항목별 실패는 warn 후 다음 spec 으로 (Step 2 skip).
        if install_builtin_bundle_step(
            mgr,
            spec,
            &dest_root,
            &dest,
            already_present,
            bundle.as_deref(),
        ) {
            continue;
        }

        // Step 2: dest가 존재하면 매니페스트 권한을 grant entry에 반영.
        if install_builtin_grant_step(mgr, spec, &dest) {
            config_dirty = true;
        }
    }
    if config_dirty && let Err(e) = mgr.config.save() {
        tracing::warn!("install_builtins: save plugins.toml failed: {e}");
    }
}

/// `install_builtins_if_needed` Step 1 — 번들에서 복사/갱신.
///
/// builtin은 호스트 소유 리소스이므로(사용자가 직접 편집하는 용도가 아님) 버전/mtime
/// 비교 없이 번들본으로 **항상 무조건 덮어쓴다** — dev(`just run`)·배포(dmg) 모두 동일
/// 정책이라, 새 빌드/설치를 띄우면 plugin 버전 bump 여부와 무관하게 최신 번들이 반영된다.
/// 단 사용자가 명시 제거한 builtin(`is_builtin_removed`)은 복원하지 않는다.
///
/// 반환값: `true` 면 항목별 실패(warn 후 계속) — caller 는 Step 2 를 건너뛰고 다음 spec.
fn install_builtin_bundle_step(
    mgr: &PluginManager,
    spec: &BuiltinSpec,
    dest_root: &Path,
    dest: &Path,
    already_present: bool,
    bundle: Option<&Path>,
) -> bool {
    if mgr.config.is_builtin_removed(spec.id) {
        return false;
    }
    let Some(bundle) = bundle else {
        return false;
    };
    let src = bundle.join(spec.id);
    if !src.is_dir() {
        tracing::debug!(
            "builtin plugin '{}' not in bundle ({}), skipping",
            spec.id,
            src.display()
        );
        return false;
    }
    if let Err(e) = std::fs::create_dir_all(dest_root) {
        tracing::warn!(
            "install_builtins: mkdir {} failed: {e}",
            dest_root.display()
        );
        return true;
    }
    if already_present {
        install_builtin_overwrite_present(spec, &src, dest)
    } else {
        install_builtin_fresh_copy(spec, &src, dest)
    }
}

/// 사용자 디렉터리에 아직 없는 builtin 을 번들에서 신규 복사.
/// 반환값: `true` 면 실패(warn 후 계속) — caller 는 Step 2 skip.
fn install_builtin_fresh_copy(spec: &BuiltinSpec, src: &Path, dest: &Path) -> bool {
    if let Err(e) = copy_dir_recursive(src, dest) {
        tracing::warn!("install_builtins: copy '{}' failed: {e}", spec.id);
        return true;
    }
    tracing::info!("installed builtin plugin '{}' from bundle", spec.id);
    false
}

/// already-present builtin 을 번들 대비 갱신.
///
/// **force 를 넘기지 않는다.** 예전에는 여기서 `true` 를 박아 판정이 항상 `ForceOverwrite` 로
/// 떨어졌고, 그래서 같은 번들로 재부팅해도 번들 전량(debug 45 파일 ≈ 1.1 GB)을 다시 썼다. 그때
/// 쓰기를 정당화한 것은 "mtime 이 거짓일 수 있다" 였는데, 지금은 같은 버전 갈래가 mtime 이 아니라
/// **내용**으로 판정하므로([`sync_dir_by_content`]) 그 보험이 필요 없다.
///
/// 반환값: `true` 면 실패(warn 후 계속) — caller 는 Step 2 skip.
fn install_builtin_overwrite_present(spec: &BuiltinSpec, src: &Path, dest: &Path) -> bool {
    let installed_v = read_installed_version(dest);
    let bundle_v = read_bundle_version(src);
    match decide_builtin_upgrade(installed_v.as_ref(), bundle_v.as_ref(), false) {
        BuiltinUpgradeDecision::Skip => {
            log_builtin_up_to_date(spec.id, installed_v.as_ref(), bundle_v.as_ref());
            false
        }
        BuiltinUpgradeDecision::ResyncSameVersion => run_dir_sync_step(
            || sync_dir_by_content(src, dest).map(|_| ()),
            spec.id,
            "resync",
        ),
        BuiltinUpgradeDecision::UpgradeVersion { from, to } => {
            tracing::info!("upgrading builtin '{}' v{} → v{}", spec.id, from, to);
            run_dir_sync_step(|| overwrite_builtin_dir(src, dest), spec.id, "upgrade")
        }
        BuiltinUpgradeDecision::ForceOverwrite => {
            // `tasty plugin upgrade-builtins --force` 만 여기 온다 — 버전·내용 무시하고
            // 통째로 덮어쓴다. 부팅 경로는 force 를 안 넘기므로 이 팔에 오지 않는다.
            log_builtin_force_overwrite(spec.id, installed_v.as_ref(), bundle_v.as_ref());
            run_dir_sync_step(
                || overwrite_builtin_dir(src, dest),
                spec.id,
                "force-overwrite",
            )
        }
    }
}

fn log_builtin_up_to_date(
    id: &str,
    installed_v: Option<&semver::Version>,
    bundle_v: Option<&semver::Version>,
) {
    tracing::debug!(
        "builtin '{}' up-to-date (installed v{:?}, bundle v{:?})",
        id,
        installed_v.map(|v| v.to_string()),
        bundle_v.map(|v| v.to_string()),
    );
}

fn log_builtin_force_overwrite(
    id: &str,
    installed_v: Option<&semver::Version>,
    bundle_v: Option<&semver::Version>,
) {
    tracing::info!(
        "force-overwriting builtin '{}' (installed v{:?}, bundle v{:?})",
        id,
        installed_v.map(|v| v.to_string()),
        bundle_v.map(|v| v.to_string()),
    );
}

/// 디렉터리 동기화 스텝 실행 공통 헬퍼 — 실패 시 `install_builtins: <verb> '<id>' failed`
/// 형식으로 warn 후 `true`(실패) 반환, 성공 시 `false`.
fn run_dir_sync_step(op: impl FnOnce() -> std::io::Result<()>, id: &str, verb: &str) -> bool {
    if let Err(e) = op() {
        tracing::warn!("install_builtins: {verb} '{id}' failed: {e}");
        return true;
    }
    false
}

/// `install_builtins_if_needed` Step 2 — dest 존재 시 매니페스트 권한을 grant entry에 반영.
///   - grant entry 없음 → 매니페스트 권한 전체를 set (최초 install).
///   - grant entry 있음 → 매니페스트 신규 추가분만 증분 grant (기존 사용자 대상으로 새
///     버전 builtin이 추가한 permission을 자동 수용).
///
/// 기존에 grant된 토큰은 *제거하지 않는다*. 사용자가 명시적 deny한 경우는 본 helper 책임 밖.
/// 매니페스트에서 사라진 token은 다음 install 시점에 set_granted로 덮어쓰일 때만 정리된다.
///
/// 반환값: config 를 변경했으면 `true` (caller 가 dirty 마킹).
fn install_builtin_grant_step(mgr: &mut PluginManager, spec: &BuiltinSpec, dest: &Path) -> bool {
    if !dest.exists() {
        return false;
    }
    // F.B.11-4: bridge::validate_bin_extras 는 본 바이너리 chain — discover 와 동일하게
    // install/add 경로의 caller 가 chain 한다.
    let Ok(manifest) = Manifest::load(dest) else {
        return false;
    };
    if manifest.permissions.is_empty() {
        return false;
    }
    if !mgr.config.grants.contains_key(spec.id) {
        mgr.config
            .set_granted(spec.id, manifest.permissions.clone());
        tracing::info!(
            "auto-granted manifest permissions for builtin '{}'",
            spec.id
        );
        true
    } else if apply_builtin_permission_diff(&mut mgr.config, spec.id, &manifest.permissions) {
        tracing::info!(
            "auto-granted new manifest permissions for builtin '{}'",
            spec.id
        );
        true
    } else {
        false
    }
}

/// 한 builtin plugin 이 본 실행에서 처리된 결과. CLI/IPC 응답에 포함된다.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct BuiltinUpgradeItem {
    pub id: String,
    pub action: BuiltinUpgradeAction,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum BuiltinUpgradeAction {
    /// 변경 없이 통과 — installed >= bundle, 동일 버전 mtime resync, 사용자 제거 등.
    Skipped {
        installed_version: Option<String>,
        bundle_version: Option<String>,
        reason: String,
    },
    /// bundle > installed — 디렉토리 교체 성공.
    Upgraded {
        from: String,
        to: String,
        /// `--restart-running` 경로에서 swap restart 가 성공했는지. 기존 client
        /// 호환을 위해 `#[serde(default)]`.
        #[serde(default)]
        was_restarted: bool,
        /// swap restart 가 실패한 경우 에러 메시지. 성공 / 미시도면 `None`.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        restart_error: Option<String>,
    },
    /// force=true 또는 신규 install — 강제 (재)설치 성공. version 은 bundle 의 값.
    Reinstalled {
        version: String,
        #[serde(default)]
        was_restarted: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        restart_error: Option<String>,
    },
    /// bundle 디렉토리 자체에 해당 plugin 이 없음 (dev partial build 등).
    NotInBundle,
    /// 파일 IO 실패 — Windows sharing violation 등.
    Failed { reason: String },
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct BuiltinUpgradeReport {
    pub items: Vec<BuiltinUpgradeItem>,
}

/// 사용자/AI 가 명시 호출하는 builtin 재설치 진입점.
///
/// `install_builtins_if_needed` 와의 차이:
/// - `force` 입력으로 동일/하위 버전도 덮어쓸 수 있음 (recovery).
/// - 각 plugin 당 한 줄 report 항목 반환 — CLI/IPC 응답으로 노출.
/// - 변경된 항목이 있으면 PluginManager 의 packages/extensions 를 재계산.
///
/// 실행 중 plugin process 의 binary 가 교체될 수 있다 (POSIX 는 inode 교체로 안전,
/// Windows 는 sharing violation 가능). 본 함수는 *process 재시작을 수행하지 않는다*
/// — 새 binary 는 다음 plugin restart (또는 부팅) 후 효과 발생.
pub fn upgrade_builtins(
    mgr: &mut PluginManager,
    force: bool,
    restore_removed: &[String],
    restore_all: bool,
    restart_running: bool,
) -> BuiltinUpgradeReport {
    let dest_root = match discovery::plugin_root() {
        Some(p) => p,
        None => {
            tracing::warn!("upgrade_builtins: cannot resolve plugin root");
            return BuiltinUpgradeReport {
                items: vec![BuiltinUpgradeItem {
                    id: String::new(),
                    action: BuiltinUpgradeAction::Failed {
                        reason: "cannot resolve plugin root".into(),
                    },
                }],
            };
        }
    };
    let bundle = bundle_root();
    let mut items = Vec::with_capacity(BUILTINS.len());
    let mut changed_ids: Vec<String> = Vec::new();

    restore_removed_builtins(mgr, restore_removed, restore_all);

    for spec in BUILTINS {
        let outcome = process_builtin_upgrade(
            mgr,
            spec,
            &dest_root,
            bundle.as_deref(),
            force,
            restart_running,
        );
        if outcome.changed {
            changed_ids.push(spec.id.into());
        }
        items.push(outcome.item);
    }

    if !changed_ids.is_empty() {
        finalize_builtin_upgrades(mgr, &dest_root, &changed_ids);
    }

    BuiltinUpgradeReport { items }
}

/// §2.E.1 의사결정: `restore_all` 이 true 이면 전체 clear (restore_removed 무시).
/// 그렇지 않으면 명시된 id 만 unmark. 변경이 있었으면 plugins.toml 저장.
fn restore_removed_builtins(
    mgr: &mut PluginManager,
    restore_removed: &[String],
    restore_all: bool,
) {
    let removed_cleared = apply_restore_removed(mgr, restore_removed, restore_all);
    if removed_cleared && let Err(e) = mgr.config.save() {
        tracing::warn!("upgrade_builtins: save plugins.toml after unmark failed: {e}");
    }
}

/// `restore_all` 이면 전체 clear(명시된 id 무시), 아니면 명시된 id 만 unmark.
/// 반환값: 하나라도 실제로 바뀌었으면 `true`(caller 가 save 여부 판단).
fn apply_restore_removed(
    mgr: &mut PluginManager,
    restore_removed: &[String],
    restore_all: bool,
) -> bool {
    if restore_all {
        let cleared = mgr.config.clear_removed_builtins();
        if cleared {
            tracing::info!("upgrade_builtins: cleared all removed_builtins (restore_all)");
        }
        return cleared;
    }
    let mut removed_cleared = false;
    for id in restore_removed {
        if mgr.config.unmark_builtin_removed(id) {
            removed_cleared = true;
            tracing::info!("upgrade_builtins: restored '{id}' from removed_builtins");
        }
    }
    removed_cleared
}

/// 한 builtin spec 의 upgrade 처리 결과. `changed` 면 caller 가 changed_ids 에 push.
struct SpecUpgrade {
    item: BuiltinUpgradeItem,
    changed: bool,
}

/// UpgradeVersion / ForceOverwrite 공통 스텝: (선택적 swap-shutdown) → overwrite →
/// (선택적 respawn). 순서·조건을 원본 그대로 보존한다.
/// Ok(SwapResult) 성공, Err(item) 은 Failed report 항목 (caller 는 그대로 push + skip).
struct SwapResult {
    was_restarted: bool,
    restart_error: Option<String>,
}

fn swap_overwrite_respawn(
    mgr: &mut PluginManager,
    spec: &BuiltinSpec,
    src: &Path,
    dest: &Path,
    restart_running: bool,
) -> Result<SwapResult, BuiltinUpgradeItem> {
    let should_swap = restart_running && mgr.is_running(spec.id);
    if should_swap && let Err(e) = mgr.swap_shutdown_internal(spec.id) {
        return Err(BuiltinUpgradeItem {
            id: spec.id.into(),
            action: BuiltinUpgradeAction::Failed {
                reason: format!("swap-shutdown-failed: {e}"),
            },
        });
    }
    if let Err(e) = overwrite_builtin_dir(src, dest) {
        if should_swap && let Err(re) = mgr.swap_respawn_internal(spec.id) {
            tracing::warn!(
                "respawn after failed overwrite of '{}' failed: {re}",
                spec.id
            );
        }
        return Err(BuiltinUpgradeItem {
            id: spec.id.into(),
            action: BuiltinUpgradeAction::Failed {
                reason: e.to_string(),
            },
        });
    }
    let restart_error = if should_swap {
        mgr.swap_respawn_internal(spec.id)
            .err()
            .map(|e| e.to_string())
    } else {
        None
    };
    Ok(SwapResult {
        was_restarted: should_swap && restart_error.is_none(),
        restart_error,
    })
}

fn not_in_bundle_item(spec: &BuiltinSpec) -> BuiltinUpgradeItem {
    BuiltinUpgradeItem {
        id: spec.id.into(),
        action: BuiltinUpgradeAction::NotInBundle,
    }
}

/// release 빌드 한정: bundle manifest 의 ed25519 detached signature 검증.
/// sidecar 누락 / 변조 / 잘못된 서명 → Err(Skipped) 로 차단.
/// debug 빌드는 dev workspace bundle 이 unsigned 라 warn 로깅 후 Ok 통과.
fn verify_builtin_bundle_trust(
    spec: &BuiltinSpec,
    src: &Path,
    dest: &Path,
) -> Result<(), BuiltinUpgradeItem> {
    #[cfg(not(debug_assertions))]
    {
        use crate::bundle_sig::{TrustDecision, verify_bundle_signature};
        match verify_bundle_signature(src) {
            Ok(TrustDecision::Trusted) => Ok(()),
            Ok(TrustDecision::Untrusted {
                plugin_id,
                fingerprint,
                reason,
                ..
            }) => {
                // 임베드 + known_plugins.toml 모두 거부. release 빌드의 builtin 은
                // *반드시* 임베드 키로 통과되어야 하므로, Untrusted = 사실상 차단.
                // trust 모달은 외부 plugin 경로용(0.7+ marketplace) 이며 builtin
                // 흐름엔 노출하지 않는다.
                tracing::warn!(
                    "builtin '{}' is untrusted (reason: {reason:?}, fp: {fingerprint})",
                    plugin_id
                );
                Err(BuiltinUpgradeItem {
                    id: spec.id.into(),
                    action: BuiltinUpgradeAction::Skipped {
                        installed_version: read_installed_version(dest).map(|v| v.to_string()),
                        bundle_version: None,
                        reason: format!("untrusted: {reason:?}"),
                    },
                })
            }
            Err(e) => {
                tracing::warn!("builtin '{}' bundle signature check failed: {e}", spec.id);
                Err(BuiltinUpgradeItem {
                    id: spec.id.into(),
                    action: BuiltinUpgradeAction::Skipped {
                        installed_version: read_installed_version(dest).map(|v| v.to_string()),
                        bundle_version: None,
                        reason: format!("signature-invalid: {e}"),
                    },
                })
            }
        }
    }
    #[cfg(debug_assertions)]
    {
        // debug 빌드는 서명 검증 결과를 로그로만 남기고 설치를 건너뛰지 않으므로
        // 설치 대상 경로를 읽을 일이 없다 — release 갈래에서만 쓰인다.
        let _ = dest;
        match crate::bundle_sig::verify_bundle_signature(src) {
            Ok(_) => {}
            Err(e) => {
                tracing::debug!(
                    "dev bundle '{}' signature check: {e} (debug build = ignored)",
                    spec.id
                );
            }
        }
        Ok(())
    }
}

/// dest 미존재 시 번들에서 신규 설치. 성공 → Reinstalled(changed), 실패 → Failed.
fn install_new_builtin(
    spec: &BuiltinSpec,
    src: &Path,
    dest: &Path,
    dest_root: &Path,
    bundle_v: Option<&semver::Version>,
) -> SpecUpgrade {
    if let Err(e) = std::fs::create_dir_all(dest_root).and_then(|_| copy_dir_recursive(src, dest)) {
        return SpecUpgrade {
            item: BuiltinUpgradeItem {
                id: spec.id.into(),
                action: BuiltinUpgradeAction::Failed {
                    reason: e.to_string(),
                },
            },
            changed: false,
        };
    }
    tracing::info!("installed builtin plugin '{}' from bundle", spec.id);
    SpecUpgrade {
        item: BuiltinUpgradeItem {
            id: spec.id.into(),
            action: BuiltinUpgradeAction::Reinstalled {
                version: bundle_v.map(|v| v.to_string()).unwrap_or_default(),
                was_restarted: false,
                restart_error: None,
            },
        },
        changed: true,
    }
}

/// dest 존재 시 decide_builtin_upgrade 판정 → 해당 분기 적용.
fn apply_builtin_upgrade_decision(
    mgr: &mut PluginManager,
    spec: &BuiltinSpec,
    src: &Path,
    dest: &Path,
    installed_v: Option<semver::Version>,
    bundle_v: Option<semver::Version>,
    force: bool,
    restart_running: bool,
) -> SpecUpgrade {
    match decide_builtin_upgrade(installed_v.as_ref(), bundle_v.as_ref(), force) {
        BuiltinUpgradeDecision::Skip => SpecUpgrade {
            item: BuiltinUpgradeItem {
                id: spec.id.into(),
                action: BuiltinUpgradeAction::Skipped {
                    installed_version: installed_v.map(|v| v.to_string()),
                    bundle_version: bundle_v.map(|v| v.to_string()),
                    reason: "installed >= bundle".into(),
                },
            },
            changed: false,
        },
        BuiltinUpgradeDecision::ResyncSameVersion => {
            let wrote = match sync_dir_by_content(src, dest) {
                Ok(w) => w,
                Err(e) => {
                    return SpecUpgrade {
                        item: BuiltinUpgradeItem {
                            id: spec.id.into(),
                            action: BuiltinUpgradeAction::Failed {
                                reason: e.to_string(),
                            },
                        },
                        changed: false,
                    };
                }
            };
            SpecUpgrade {
                item: BuiltinUpgradeItem {
                    id: spec.id.into(),
                    action: BuiltinUpgradeAction::Skipped {
                        installed_version: installed_v.map(|v| v.to_string()),
                        bundle_version: bundle_v.map(|v| v.to_string()),
                        reason: if wrote {
                            "same-version (content resync: files rewritten)".into()
                        } else {
                            "same-version (content resync: nothing to write)".into()
                        },
                    },
                },
                // 버전은 그대로여도 **파일이 바뀌었으면 바뀐 것**이다. 이 값이 false 로 고정돼
                // 있으면 같은 버전으로 내용만 고친 plugin 이 재기동 대상에서 조용히 빠진다.
                changed: wrote,
            }
        }
        BuiltinUpgradeDecision::UpgradeVersion { from, to } => {
            tracing::info!("upgrading builtin '{}' v{} → v{}", spec.id, from, to);
            match swap_overwrite_respawn(mgr, spec, src, dest, restart_running) {
                Ok(swap) => SpecUpgrade {
                    item: BuiltinUpgradeItem {
                        id: spec.id.into(),
                        action: BuiltinUpgradeAction::Upgraded {
                            from: from.to_string(),
                            to: to.to_string(),
                            was_restarted: swap.was_restarted,
                            restart_error: swap.restart_error,
                        },
                    },
                    changed: true,
                },
                Err(item) => SpecUpgrade {
                    item,
                    changed: false,
                },
            }
        }
        BuiltinUpgradeDecision::ForceOverwrite => {
            tracing::info!(
                "force-reinstalling builtin '{}' (installed v{:?}, bundle v{:?})",
                spec.id,
                installed_v.as_ref().map(|v| v.to_string()),
                bundle_v.as_ref().map(|v| v.to_string()),
            );
            match swap_overwrite_respawn(mgr, spec, src, dest, restart_running) {
                Ok(swap) => SpecUpgrade {
                    item: BuiltinUpgradeItem {
                        id: spec.id.into(),
                        action: BuiltinUpgradeAction::Reinstalled {
                            version: bundle_v.as_ref().map(|v| v.to_string()).unwrap_or_default(),
                            was_restarted: swap.was_restarted,
                            restart_error: swap.restart_error,
                        },
                    },
                    changed: true,
                },
                Err(item) => SpecUpgrade {
                    item,
                    changed: false,
                },
            }
        }
    }
}

/// 한 builtin spec 의 upgrade 전체 처리 (removed → not-in-bundle → 서명 → install/decide).
fn process_builtin_upgrade(
    mgr: &mut PluginManager,
    spec: &BuiltinSpec,
    dest_root: &Path,
    bundle: Option<&Path>,
    force: bool,
    restart_running: bool,
) -> SpecUpgrade {
    let dest = dest_root.join(spec.id);

    if mgr.config.is_builtin_removed(spec.id) {
        return SpecUpgrade {
            item: BuiltinUpgradeItem {
                id: spec.id.into(),
                action: BuiltinUpgradeAction::Skipped {
                    installed_version: read_installed_version(&dest).map(|v| v.to_string()),
                    bundle_version: None,
                    reason: "user-removed".into(),
                },
            },
            changed: false,
        };
    }

    let Some(bundle_root) = bundle else {
        return SpecUpgrade {
            item: not_in_bundle_item(spec),
            changed: false,
        };
    };
    let src = bundle_root.join(spec.id);
    if !src.is_dir() {
        return SpecUpgrade {
            item: not_in_bundle_item(spec),
            changed: false,
        };
    }

    if let Err(item) = verify_builtin_bundle_trust(spec, &src, &dest) {
        return SpecUpgrade {
            item,
            changed: false,
        };
    }

    let installed_v = read_installed_version(&dest);
    let bundle_v = read_bundle_version(&src);

    if !dest.exists() {
        return install_new_builtin(spec, &src, &dest, dest_root, bundle_v.as_ref());
    }

    apply_builtin_upgrade_decision(
        mgr,
        spec,
        &src,
        &dest,
        installed_v,
        bundle_v,
        force,
        restart_running,
    )
}

/// changed builtin 들에 대해 PluginManager state 재계산 + 매니페스트 신규 permission
/// 자동 grant (install_builtins_if_needed step 2 와 동일 규칙).
fn finalize_builtin_upgrades(mgr: &mut PluginManager, dest_root: &Path, changed_ids: &[String]) {
    // 디스크가 바뀌었으니 PluginManager state 재계산.
    mgr.refresh_packages();
    mgr.recompute_extensions();
    let mut config_dirty = false;
    for id in changed_ids {
        let dest = dest_root.join(id);
        if !dest.exists() {
            continue;
        }
        if let Ok(manifest) = Manifest::load(&dest) {
            if manifest.permissions.is_empty() {
                continue;
            }
            if !mgr.config.grants.contains_key(id) {
                mgr.config.set_granted(id, manifest.permissions.clone());
                config_dirty = true;
            } else if apply_builtin_permission_diff(&mut mgr.config, id, &manifest.permissions) {
                config_dirty = true;
            }
        }
    }
    if config_dirty && let Err(e) = mgr.config.save() {
        tracing::warn!("upgrade_builtins: save plugins.toml failed: {e}");
    }
}

/// 기존 builtin grant entry에 매니페스트 신규 permission 만 증분 추가.
///
/// 기존 사용자의 `plugins.toml`에 이미 plugin entry가 있는 경우 (`grants` map에
/// 키가 있는 상태), `install_builtins_if_needed`의 step 2 첫 번째 분기 (entry
/// 없을 때만 set_granted)는 동작하지 않는다. 새 버전 builtin이 매니페스트에
/// permission을 추가했을 때 이 helper로 신규 token만 증분 grant 한다.
///
/// 기존 grant token은 제거하지 않는다 (사용자가 명시적으로 deny 했을 가능성).
/// 반환값: 신규 token이 하나라도 추가되면 true.
fn apply_builtin_permission_diff(
    config: &mut PluginsConfig,
    id: &str,
    manifest_permissions: &[String],
) -> bool {
    let mut changed = false;
    for token in manifest_permissions {
        if config.grant(id, token) {
            changed = true;
        }
    }
    changed
}

/// uninstall 흐름에서 호출 — built-in인 경우 `removed_builtins`에 등록하여
/// 다음 부팅의 `install_builtins_if_needed`가 다시 복사하지 않게 한다.
/// 외부 플러그인이면 no-op.
pub fn mark_builtin_removed(mgr: &mut PluginManager, id: &str) {
    if !is_builtin_plugin(id) {
        return;
    }
    if mgr.config.mark_builtin_removed(id)
        && let Err(e) = mgr.config.save()
    {
        tracing::warn!("mark_builtin_removed: save plugins.toml failed: {e}");
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
/// 번들 → 설치 디렉터리 동기화. **내용이 다른 파일만** 쓰고, src 에 없는 dest 항목은 층마다
/// 지운다([`prune_dest_not_in_src`]).
///
/// **mtime 을 판정에 쓰지 않는다.** 예전에는 이 자리가 mtime 비교였고, 그러면 번들 쪽 mtime 이
/// 거짓일 때(`cp -p`·아카이브 해제처럼 mtime 을 보존하는 경로) **새 파일을 건너뛴다.** 그
/// 위험 때문에 부팅 경로는 아예 무조건 덮어쓰기를 골랐었고, 그 대가가 같은 번들로 재부팅해도
/// 번들 전량을 다시 쓰는 것이었다. 내용으로 판정하면 둘 다 필요 없다.
///
/// 반환값은 **무엇이든 썼는가**다. 호출자가 그 값으로 "같은 버전인데 내용이 바뀌었다" 를
/// 보고·재기동 판정에 쓴다 — 안 돌려주면 그 사실이 이 함수 안에서 사라진다.
fn sync_dir_by_content(src: &Path, dst: &Path) -> std::io::Result<bool> {
    std::fs::create_dir_all(dst)?;
    prune_dest_not_in_src(src, dst)?;
    let mut wrote = false;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let ty = entry.file_type()?;
        let dest_path = dst.join(entry.file_name());
        if ty.is_dir() {
            wrote |= sync_dir_by_content(&entry.path(), &dest_path)?;
        } else {
            wrote |= copy_file_if_content_differs(&entry.path(), &dest_path)?;
        }
    }
    Ok(wrote)
}

/// 내용이 다를 때만 복사. 판정 순서가 **비용 순서**이고, 어느 단계에도 mtime 이 없다.
///
/// 1. dest 가 없다 → 복사
/// 2. 크기가 다르다 → 복사 (해시 불필요 — 재빌드는 대개 여기서 걸린다)
/// 3. 크기가 같다 → 양쪽 sha256 비교
///
/// "크기와 mtime 이 둘 다 같으면 해시를 생략" 하는 지름길은 **넣지 않는다.** 그 한 줄이
/// mtime 을 판정에 도로 들여서, 번들 mtime 이 거짓이고 크기까지 같은 경우에 이 함수가 사려던
/// 내성이 사라진다 — 그리고 그 사라짐은 조용하다(거짓 mtime 이 실제로 올 때까지 증상이 없다).
fn copy_file_if_content_differs(src: &Path, dst: &Path) -> std::io::Result<bool> {
    if !file_content_differs(src, dst)? {
        return Ok(false);
    }
    copy_atomic(src, dst)?;
    Ok(true)
}

/// [`copy_file_if_content_differs`] 의 판정부. dest 를 못 읽으면 "다르다" 로 답한다 —
/// 그래야 읽기 실패가 **안 쓰는 쪽**이 아니라 쓰는 쪽으로 물러난다.
fn file_content_differs(src: &Path, dst: &Path) -> std::io::Result<bool> {
    let (Ok(src_meta), Ok(dst_meta)) = (std::fs::metadata(src), std::fs::metadata(dst)) else {
        return Ok(true);
    };
    if src_meta.len() != dst_meta.len() {
        return Ok(true);
    }
    Ok(file_sha256(src)? != file_sha256(dst)?)
}

/// 파일 하나의 sha256. 통째로 읽지 않고 버퍼로 흘려 넣는다 — 번들 바이너리는 debug 에서
/// 파일 하나가 수백 MB 다.
fn file_sha256(path: &Path) -> std::io::Result<[u8; 32]> {
    use sha2::{Digest, Sha256};
    let mut file = std::fs::File::open(path)?;
    let mut hasher = Sha256::new();
    std::io::copy(&mut file, &mut hasher)?;
    Ok(hasher.finalize().into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explorer_is_not_builtin_plugin() {
        // T11: explorer 는 host builtin surface 로 승격되어 plugin 번들에서 제거됨.
        assert!(!is_builtin_plugin("com.tasty.explorer"));
    }

    #[test]
    fn claude_design_is_not_builtin_plugin() {
        // docs/adr/0057-remove-claude-design-plugin.md: 별도 프로젝트로 분리하며
        // tasty 본체 번들에서 완전히 제거됨.
        assert!(!is_builtin_plugin("com.tasty.claude-design"));
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
    fn markdown_is_builtin() {
        assert!(is_builtin_plugin("com.tasty.markdown"));
    }

    #[test]
    fn clipboard_viewer_is_builtin() {
        assert!(is_builtin_plugin("com.tasty.clipboard-viewer"));
    }

    #[test]
    fn unknown_is_not_builtin() {
        assert!(!is_builtin_plugin("com.example.foo"));
    }

    #[test]
    fn permission_diff_appends_new_tokens_only() {
        let mut cfg = PluginsConfig::default();
        cfg.set_granted(
            "com.tasty.image",
            vec!["surface.read".into(), "surface.write".into()],
        );

        let manifest = vec![
            "surface.read".into(),
            "surface.write".into(),
            "file_handler.define".into(),
            "file_handler.handle:image".into(),
        ];

        let changed = apply_builtin_permission_diff(&mut cfg, "com.tasty.image", &manifest);
        assert!(changed);

        let granted = cfg.granted_permissions("com.tasty.image");
        assert!(granted.contains("surface.read"));
        assert!(granted.contains("surface.write"));
        assert!(granted.contains("file_handler.define"));
        assert!(granted.contains("file_handler.handle:image"));
    }

    #[test]
    fn permission_diff_is_noop_when_manifest_already_covered() {
        let mut cfg = PluginsConfig::default();
        cfg.set_granted(
            "com.tasty.image",
            vec!["surface.read".into(), "surface.write".into()],
        );

        let manifest = vec!["surface.read".into(), "surface.write".into()];

        let changed = apply_builtin_permission_diff(&mut cfg, "com.tasty.image", &manifest);
        assert!(!changed);
        assert_eq!(cfg.granted_permissions("com.tasty.image").len(), 2);
    }

    fn v(s: &str) -> semver::Version {
        semver::Version::parse(s).unwrap()
    }

    #[test]
    fn decide_returns_skip_when_bundle_is_lower() {
        let installed = v("1.2.0");
        let bundle = v("1.1.0");
        assert_eq!(
            decide_builtin_upgrade(Some(&installed), Some(&bundle), false),
            BuiltinUpgradeDecision::Skip
        );
    }

    #[test]
    fn decide_returns_upgrade_when_bundle_is_higher() {
        let installed = v("1.0.0");
        let bundle = v("1.1.0");
        assert_eq!(
            decide_builtin_upgrade(Some(&installed), Some(&bundle), false),
            BuiltinUpgradeDecision::UpgradeVersion {
                from: installed,
                to: bundle,
            }
        );
    }

    #[test]
    fn decide_returns_resync_when_versions_equal() {
        let installed = v("1.0.0");
        let bundle = v("1.0.0");
        assert_eq!(
            decide_builtin_upgrade(Some(&installed), Some(&bundle), false),
            BuiltinUpgradeDecision::ResyncSameVersion
        );
    }

    #[test]
    fn decide_returns_force_overwrite_when_force_true() {
        let installed = v("9.9.9");
        let bundle = v("1.0.0");
        assert_eq!(
            decide_builtin_upgrade(Some(&installed), Some(&bundle), true),
            BuiltinUpgradeDecision::ForceOverwrite
        );
        // 입력이 None 이어도 force 우선
        assert_eq!(
            decide_builtin_upgrade(None, None, true),
            BuiltinUpgradeDecision::ForceOverwrite
        );
    }

    #[test]
    fn decide_returns_skip_when_bundle_version_unparsable() {
        let installed = v("1.0.0");
        assert_eq!(
            decide_builtin_upgrade(Some(&installed), None, false),
            BuiltinUpgradeDecision::Skip
        );
    }

    #[test]
    fn decide_returns_resync_when_installed_version_unparsable_but_bundle_ok() {
        let bundle = v("1.0.0");
        assert_eq!(
            decide_builtin_upgrade(None, Some(&bundle), false),
            BuiltinUpgradeDecision::ResyncSameVersion
        );
    }

    /// 파일의 mtime 을 강제로 세운다 — `cp -p`·아카이브 해제가 하는 일과 같은 결과.
    fn set_mtime(path: &std::path::Path, t: std::time::SystemTime) {
        let f = std::fs::OpenOptions::new().write(true).open(path).unwrap();
        f.set_modified(t).unwrap();
    }

    fn mtimes(dir: &std::path::Path) -> Vec<(std::ffi::OsString, std::time::SystemTime)> {
        let mut v: Vec<_> = std::fs::read_dir(dir)
            .unwrap()
            .map(|e| {
                let e = e.unwrap();
                (e.file_name(), e.metadata().unwrap().modified().unwrap())
            })
            .collect();
        v.sort_by(|a, b| a.0.cmp(&b.0));
        v
    }

    #[test]
    fn second_sync_of_the_same_bundle_writes_nothing() {
        // 이 시험이 이 변경의 값 자체다 — 같은 번들로 두 번 돌면 두 번째는 아무것도 안 쓴다.
        // 판정은 mtime 스냅샷 차분이다(쓰였으면 mtime 이 움직인다).
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("src");
        let dest = tmp.path().join("dest");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(src.join("bin"), "content").unwrap();
        std::fs::write(src.join("tasty-plugin.toml"), "version = \"0.1.0\"").unwrap();

        sync_dir_by_content(&src, &dest).unwrap();
        let first = mtimes(&dest);
        sync_dir_by_content(&src, &dest).unwrap();

        assert_eq!(first, mtimes(&dest), "두 번째 동기화가 파일을 다시 썼다");
    }

    #[test]
    fn the_sync_reports_whether_it_wrote() {
        // 이 반환값이 `upgrade-builtins` 의 보고문과 재기동 판정을 정한다. 늘 false 면
        // 같은 버전으로 내용만 고친 plugin 이 재기동 대상에서 조용히 빠진다.
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("src");
        let dest = tmp.path().join("dest");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(src.join("bin"), "one").unwrap();

        assert!(
            sync_dir_by_content(&src, &dest).unwrap(),
            "첫 설치가 false 를 냈다"
        );
        assert!(
            !sync_dir_by_content(&src, &dest).unwrap(),
            "안 쓴 회차가 true 를 냈다"
        );

        std::fs::write(src.join("bin"), "two").unwrap();
        assert!(
            sync_dir_by_content(&src, &dest).unwrap(),
            "내용이 바뀐 회차가 false 를 냈다"
        );
    }

    #[test]
    fn only_the_changed_file_is_rewritten() {
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("src");
        let dest = tmp.path().join("dest");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(src.join("changed"), "before").unwrap();
        std::fs::write(src.join("untouched"), "same").unwrap();
        sync_dir_by_content(&src, &dest).unwrap();
        let before = mtimes(&dest);

        std::fs::write(src.join("changed"), "after!").unwrap(); // 같은 크기, 다른 내용
        sync_dir_by_content(&src, &dest).unwrap();

        let after = mtimes(&dest);
        assert_eq!(
            std::fs::read_to_string(dest.join("changed")).unwrap(),
            "after!"
        );
        let m = |v: &[(std::ffi::OsString, std::time::SystemTime)], n: &str| {
            v.iter().find(|(k, _)| k == n).unwrap().1
        };
        assert_ne!(m(&before, "changed"), m(&after, "changed"));
        assert_eq!(
            m(&before, "untouched"),
            m(&after, "untouched"),
            "안 바뀐 파일이 다시 쓰였다"
        );
    }

    #[test]
    fn a_lying_mtime_does_not_hide_a_different_file() {
        // ★ 이 변경의 존재 이유. src 가 dest 보다 **오래된** mtime 을 갖고 크기까지 같은데
        // 내용이 다르다 — mtime 판정이면 건너뛰고, 내용 판정이면 복사한다.
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("src");
        let dest = tmp.path().join("dest");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(src.join("bin"), "aaaa").unwrap();
        sync_dir_by_content(&src, &dest).unwrap();

        std::fs::write(src.join("bin"), "bbbb").unwrap(); // 같은 크기
        let old = std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_000_000);
        set_mtime(&src.join("bin"), old);

        sync_dir_by_content(&src, &dest).unwrap();

        assert_eq!(
            std::fs::read_to_string(dest.join("bin")).unwrap(),
            "bbbb",
            "src 가 더 오래된 mtime 이라고 새 내용을 건너뛰었다"
        );
    }

    #[test]
    fn content_sync_still_removes_what_the_bundle_dropped() {
        // 청소가 복사 정책과 함께 사라지지 않았는지 — 정책이 바뀐 자리에서 다시 묻는다.
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("src");
        let dest = tmp.path().join("dest");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(src.join("keep"), "keep").unwrap();
        std::fs::write(src.join("dropped"), "gone soon").unwrap();
        sync_dir_by_content(&src, &dest).unwrap();
        assert!(dest.join("dropped").exists());

        std::fs::remove_file(src.join("dropped")).unwrap();
        sync_dir_by_content(&src, &dest).unwrap();

        assert!(
            !dest.join("dropped").exists(),
            "번들에서 빠진 파일이 남았다"
        );
        assert!(dest.join("keep").exists());
    }

    #[test]
    fn prune_removes_only_what_src_lacks_and_copies_nothing() {
        // 청소 **단독** 단정. 이 시험이 없으면 "복사 정책을 바꿨더니 청소가 조용히
        // 사라졌다" 를 아무것도 못 잡는다 — 잔존 파일은 다른 단정을 안 건드린다.
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("src");
        let dest = tmp.path().join("dest");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::create_dir_all(dest.join("gone-dir")).unwrap();
        std::fs::write(src.join("keep"), "src-side").unwrap();
        std::fs::write(dest.join("keep"), "dest-side").unwrap();
        std::fs::write(dest.join("stale"), "remove me").unwrap();
        std::fs::write(dest.join("gone-dir/inner"), "remove me too").unwrap();

        prune_dest_not_in_src(&src, &dest).unwrap();

        assert!(!dest.join("stale").exists(), "src 에 없는 파일이 남았다");
        assert!(
            !dest.join("gone-dir").exists(),
            "src 에 없는 디렉터리가 남았다"
        );
        // 청소는 **복사를 하지 않는다** — 이름이 겹치는 파일의 내용은 그대로여야 한다.
        assert_eq!(
            std::fs::read_to_string(dest.join("keep")).unwrap(),
            "dest-side"
        );
    }

    #[test]
    fn overwrite_builtin_dir_removes_stale_files() {
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("src");
        let dest = tmp.path().join("dest");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::create_dir_all(&dest).unwrap();
        std::fs::write(src.join("a"), "a-new").unwrap();
        std::fs::write(src.join("b"), "b-new").unwrap();
        std::fs::write(dest.join("a"), "a-old").unwrap();
        std::fs::write(dest.join("b"), "b-old").unwrap();
        std::fs::write(dest.join("stale.txt"), "remove me").unwrap();

        overwrite_builtin_dir(&src, &dest).unwrap();

        assert_eq!(std::fs::read_to_string(dest.join("a")).unwrap(), "a-new");
        assert_eq!(std::fs::read_to_string(dest.join("b")).unwrap(), "b-new");
        assert!(!dest.join("stale.txt").exists());
    }

    #[test]
    fn overwrite_builtin_dir_recreates_nested_dirs() {
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("src");
        let dest = tmp.path().join("dest");
        std::fs::create_dir_all(src.join("lang")).unwrap();
        std::fs::create_dir_all(dest.join("lang")).unwrap();
        std::fs::write(src.join("lang/en.toml"), "en=new").unwrap();
        std::fs::write(dest.join("lang/en.toml"), "en=old").unwrap();
        std::fs::write(dest.join("lang/old.toml"), "stale").unwrap();

        overwrite_builtin_dir(&src, &dest).unwrap();

        assert_eq!(
            std::fs::read_to_string(dest.join("lang/en.toml")).unwrap(),
            "en=new"
        );
        assert!(!dest.join("lang/old.toml").exists());
    }

    #[test]
    fn permission_diff_preserves_existing_extra_tokens() {
        // 사용자가 명시적으로 추가했을 수도 있는 매니페스트 외 토큰은 제거하지 않는다.
        let mut cfg = PluginsConfig::default();
        cfg.set_granted(
            "com.tasty.image",
            vec!["surface.read".into(), "user.extra.token".into()],
        );

        let manifest = vec!["surface.read".into(), "file_handler.define".into()];

        let changed = apply_builtin_permission_diff(&mut cfg, "com.tasty.image", &manifest);
        assert!(changed);

        let granted = cfg.granted_permissions("com.tasty.image");
        assert!(granted.contains("surface.read"));
        assert!(granted.contains("user.extra.token")); // preserved
        assert!(granted.contains("file_handler.define")); // newly added
    }
}
