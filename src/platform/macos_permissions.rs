//! macOS 권한 프롬프트 pre-warm — 파일 TCC + 화면 기록.
//!
//! macOS 는 보호 리소스에 **실제로 접근하는 그 순간**에만 권한 프롬프트를 띄운다.
//! 파일 계열 TCC 서비스에는 "미리 물어보는" API 가 없으므로, 프롬프트 시점을 앞당기는
//! 유일한 방법은 앱이 부팅 직후 그 리소스를 스스로 한 번 건드리는 것이다
//! (디렉터리 1 회 `read_dir`). PTY 자식 프로세스(zsh, 그 안의 AI 에이전트)가 보호
//! 폴더에 접근하면 macOS 가 그 접근의 responsible process 를 부모 GUI 앱으로 귀속
//! 시키므로, 터미널 작업 *도중에* 프롬프트가 떠서 자율 진행이 멈추는 것이 원래
//! 증상이다. 부팅 직후로 몰아두면 그 중단이 사라진다.
//!
//! 이미 허용/거부가 결정된 항목에는 프롬프트가 뜨지 않으므로 매 부팅 반복해도
//! 무해하다 — "첫 실행" 플래그를 따로 두지 않는 이유다. 새 마운트는 실행할 때마다
//! 달라져 1 회로는 못 덮고, 플래그만 남고 TCC 가 초기화된 상태(재설치·`tccutil reset`
//! 이후)에서는 pre-warm 이 영영 안 도는 어긋남이 생긴다.
//!
//! 이 모듈은 **목록 결정**(순수, 전 플랫폼 컴파일·테스트 가능)과 **실제 접근**
//! (`#[cfg(all(target_os = "macos", feature = "gui"))]`) 을 분리한다. cfg 로 잘린
//! 코드는 rustc 가 타입체크 전에 걷어내므로, 로직을 순수부에 몰아둘수록 비-macOS
//! 에서도 검증되는 면적이 넓어진다.
//!
//! 기능 문서: `docs/features/macos-permissions/index.md`.

// 비-macOS / headless 빌드에서는 결정 로직을 호출하는 실행부가 cfg 로 잘려 나간다.
// 그래도 로직 자체는 컴파일한다 — cfg 로 잘린 코드는 타입체크조차 되지 않으므로,
// 다른 플랫폼에서 검증 가능한 면적을 남겨두는 것이 이 분리의 목적이다.
#![cfg_attr(not(all(target_os = "macos", feature = "gui")), allow(dead_code))]

use std::path::{Path, PathBuf};

/// pre-warm 할 홈 하위 폴더 — `SystemPolicy{Downloads,Documents,Desktop}Folder` 대응.
/// 순서가 곧 프롬프트가 뜨는 순서다.
const HOME_SUBDIRS: [&str; 3] = ["Downloads", "Documents", "Desktop"];

/// 마운트 루트. 이동식(`SystemPolicyRemovableVolumes`)·네트워크
/// (`SystemPolicyNetworkVolumes`) 볼륨이 모두 여기 하위에 붙는다.
const VOLUMES_ROOT: &str = "/Volumes";

/// 목록 결정에 필요한 파일시스템 조회. 실제 IO 없이 결정 로직만 검증할 수 있도록
/// 추상화한다 — TCC 가 없는 CI 에서 `read_dir` 을 돌리면 헤드리스 러너가 프롬프트를
/// 기다리며 멈출 수 있고, 그 환경 의존성을 테스트에 들이지 않기 위함이다.
pub(crate) trait FsProbe {
    /// 디렉터리로 존재하는가. 없는 폴더는 읽어봐야 프롬프트가 안 뜨므로 건너뛴다.
    fn is_dir(&self, path: &Path) -> bool;

    /// depth-1 나열. 실패(권한·부재)는 빈 목록으로 접는다 — 마운트 목록을 못 읽는 것은
    /// pre-warm 을 중단할 사유가 아니다.
    fn list_dir(&self, path: &Path) -> Vec<PathBuf>;
}

/// 실제 파일시스템. pre-warm 실행부와 같은 조건으로만 컴파일한다.
#[cfg(all(target_os = "macos", feature = "gui"))]
pub(crate) struct RealFs;

#[cfg(all(target_os = "macos", feature = "gui"))]
impl FsProbe for RealFs {
    fn is_dir(&self, path: &Path) -> bool {
        path.is_dir()
    }

    fn list_dir(&self, path: &Path) -> Vec<PathBuf> {
        match std::fs::read_dir(path) {
            Ok(entries) => entries.filter_map(Result::ok).map(|e| e.path()).collect(),
            // 마운트 루트를 못 읽어도 홈 폴더 pre-warm 은 그대로 진행한다.
            Err(err) => {
                tracing::debug!(path = %path.display(), %err, "prewarm: 마운트 루트 나열 실패");
                Vec::new()
            }
        }
    }
}

/// pre-warm 대상 경로를 **순서대로** 결정한다.
///
/// 홈 폴더 3 곳이 먼저고, 마운트된 볼륨이 마지막이다. 네트워크 볼륨의 `read_dir` 은
/// 응답 없는 마운트에서 수 초~수십 초 걸리거나 영영 안 끝날 수 있어, 앞에 두면 사용자가
/// 실제로 겪는 홈 폴더 프롬프트가 그만큼 늦어진다. 볼륨은 `/Volumes` 를 depth-1 로만
/// 나열해 항목당 한 번씩만 건드린다.
///
/// 존재하지 않는 경로는 빠진다. `home` 이 `None` 이면 홈 항목 전체가 빠진다.
pub(crate) fn prewarm_targets(home: Option<&Path>, fs: &dyn FsProbe) -> Vec<PathBuf> {
    let mut targets = Vec::new();

    if let Some(home) = home {
        for sub in HOME_SUBDIRS {
            let path = home.join(sub);
            if fs.is_dir(&path) {
                targets.push(path);
            }
        }
    }

    let volumes_root = Path::new(VOLUMES_ROOT);
    if fs.is_dir(volumes_root) {
        let mut volumes: Vec<PathBuf> = fs
            .list_dir(volumes_root)
            .into_iter()
            .filter(|p| fs.is_dir(p))
            .collect();
        // `read_dir` 순서는 파일시스템 마음이라 프롬프트 순서가 실행마다 달라진다.
        // 경로로 정렬해 사용자가 보는 순서를 고정한다.
        volumes.sort();
        targets.extend(volumes);
    }

    targets
}

// CoreGraphics 의 화면 기록 권한 API. 파일 계열 TCC 와 달리 "미리 물어보는" 공개
// API 가 있어서, 리소스를 몰래 건드려 유도할 필요 없이 정식으로 요청할 수 있다.
// 새 크레이트를 들이지 않고 두 함수만 직접 선언한다 — `surface.raw_key` 의
// CoreGraphics 선언(`src/adapters/ipc/handler/input_source.rs`)과 같은 방식.
//
// - `CGPreflightScreenCaptureAccess`: 현재 승인 상태만 조회한다. 프롬프트를 띄우지 않는다.
// - `CGRequestScreenCaptureAccess`: 미결정 상태면 시스템 프롬프트를 띄운다. 이미 거부된
//   상태면 프롬프트 없이 즉시 false 를 반환한다(사용자가 시스템 설정에서 직접 켜야 한다).
//
// 둘 다 macOS 10.15+ 이고 번들의 `LSMinimumSystemVersion` 은 11.0 이라 항상 존재한다.
#[cfg(all(target_os = "macos", feature = "gui"))]
#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {
    fn CGPreflightScreenCaptureAccess() -> bool;
    fn CGRequestScreenCaptureAccess() -> bool;
}

/// 화면 기록 권한이 지금 승인돼 있는가. 프롬프트를 띄우지 않는 순수 조회다.
///
/// **캡처 직전에 부른다** — 부팅 시점 값을 캐시해두면 그 사이 사용자가 시스템 설정에서
/// 권한을 바꾼 경우를 잘못 판정한다.
#[cfg(all(target_os = "macos", feature = "gui"))]
pub(crate) fn screen_recording_authorized() -> bool {
    // SAFETY: 인자도 반환 포인터도 없는 CoreGraphics C 함수 호출 — 포인터 수명/해제
    // 책임이 발생하지 않고, panic 을 가로지르는 상태도 남기지 않는다. 내부적으로
    // TCC 데몬에 현재 앱의 승인 상태를 묻기만 하며 AppKit 을 건드리지 않아
    // main thread 한정이 아니다(캡처 워커 스레드에서도 호출된다).
    unsafe { CGPreflightScreenCaptureAccess() }
}

/// 비-macOS / headless — 화면 기록 권한이라는 개념이 없으므로 "승인됨"으로 답한다.
/// 그래야 캡처 경로가 다른 플랫폼에서 기존과 똑같이 동작한다.
#[cfg(not(all(target_os = "macos", feature = "gui")))]
pub(crate) fn screen_recording_authorized() -> bool {
    true
}

/// 화면 기록 권한을 **부팅당 1 회** 요청한다. 이미 승인돼 있으면 아무것도 하지 않는다.
///
/// 거부된 상태에서 다시 불러도 프롬프트는 뜨지 않고 즉시 false 가 돌아오므로, 재시도
/// 루프를 두지 않는다 — 그 상태를 되돌리는 것은 시스템 설정에서 사용자가 할 일이다.
#[cfg(all(target_os = "macos", feature = "gui"))]
fn prewarm_screen_recording() {
    if screen_recording_authorized() {
        tracing::debug!("prewarm: 화면 기록 권한 이미 승인됨");
        return;
    }
    // SAFETY: `screen_recording_authorized` 의 preflight 호출과 같은 근거 — 인자/반환
    // 포인터가 없는 CoreGraphics C 함수다. 미결정 상태에서만 프롬프트를 띄우고 사용자
    // 응답까지 블록할 수 있어 워커 스레드에서만 호출한다(메인 루프를 막지 않는다).
    let granted = unsafe { CGRequestScreenCaptureAccess() };
    tracing::debug!(granted, "prewarm: 화면 기록 권한 요청 결과");
}

/// 홈 디렉터리 — `DirectoriesHome` 과 같은 해석(`directories::BaseDirs`).
#[cfg(all(target_os = "macos", feature = "gui"))]
fn home_dir() -> Option<PathBuf> {
    directories::BaseDirs::new().map(|b| b.home_dir().to_path_buf())
}

/// 부팅 직후 권한 프롬프트를 순차로 발화한다. 호출 즉시 반환한다.
///
/// **반드시 워커 스레드**다. 프롬프트가 떠 있는 동안 `read_dir`(과 화면 기록 요청)은
/// 사용자가 응답할 때까지 리턴하지 않으므로, 메인 스레드(winit 이벤트 루프)에서
/// 호출하면 프롬프트가 떠 있는 내내 UI 가 얼어붙고 `boot_total` 계측도 사용자 응답
/// 시간만큼 부풀려진다.
///
/// **하나의 스레드에서 하나씩 순차로** 처리한다. 동시에 건드리면 프롬프트가 겹쳐 뜬다 —
/// 순차면 앞의 것을 닫아야 다음이 뜬다. 파일 폴더가 먼저고 화면 기록이 마지막이다.
#[cfg(all(target_os = "macos", feature = "gui"))]
pub(crate) fn spawn_prewarm() {
    std::thread::spawn(|| {
        let targets = prewarm_targets(home_dir().as_deref(), &RealFs);
        tracing::debug!(count = targets.len(), "prewarm: 파일 TCC 대상 결정");
        for path in targets {
            // 결과는 버린다 — 거부는 사용자의 정당한 선택이라 정상 결과이고, 성공해도
            // 엔트리를 쓸 데가 없다. 목적은 접근 시도 자체(= 프롬프트 발화)뿐이다.
            match std::fs::read_dir(&path) {
                Ok(_) => tracing::debug!(path = %path.display(), "prewarm: 접근 허용"),
                Err(err) => {
                    tracing::debug!(path = %path.display(), %err, "prewarm: 접근 불가(거부 또는 부재)")
                }
            }
        }
        prewarm_screen_recording();
    });
}

/// 비-macOS / headless 는 no-op — 호출부에 `#[cfg]` 를 흩뿌리지 않기 위한 짝.
/// headless 에는 프롬프트를 띄울 GUI 주체가 없으므로 macOS 여도 돌지 않는다.
#[cfg(not(all(target_os = "macos", feature = "gui")))]
pub(crate) fn spawn_prewarm() {}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    /// 경로 집합만 들고 있는 가짜 파일시스템. `dirs` 는 디렉터리, `files` 는 그 외
    /// 엔트리 — `read_dir` 이 디렉터리가 아닌 것도 돌려준다는 사실을 재현한다.
    struct FakeFs {
        dirs: BTreeSet<PathBuf>,
        files: BTreeSet<PathBuf>,
    }

    impl FakeFs {
        fn new<'a>(dirs: impl IntoIterator<Item = &'a str>) -> Self {
            Self {
                dirs: dirs.into_iter().map(PathBuf::from).collect(),
                files: BTreeSet::new(),
            }
        }

        fn with_files<'a>(mut self, files: impl IntoIterator<Item = &'a str>) -> Self {
            self.files = files.into_iter().map(PathBuf::from).collect();
            self
        }
    }

    impl FsProbe for FakeFs {
        fn is_dir(&self, path: &Path) -> bool {
            self.dirs.contains(path)
        }

        fn list_dir(&self, path: &Path) -> Vec<PathBuf> {
            // 실제 `read_dir` 처럼 순서를 보장하지 않는 소스를 흉내내려 역순으로 준다 —
            // 정렬 책임이 `prewarm_targets` 에 있는지 확인하기 위함.
            let mut out: Vec<PathBuf> = self
                .dirs
                .iter()
                .chain(self.files.iter())
                .filter(|p| p.parent() == Some(path))
                .cloned()
                .collect();
            out.sort();
            out.reverse();
            out
        }
    }

    #[test]
    fn home_folders_come_first_in_fixed_order() {
        let fs = FakeFs::new([
            "/Users/t/Downloads",
            "/Users/t/Documents",
            "/Users/t/Desktop",
        ]);
        let targets = prewarm_targets(Some(Path::new("/Users/t")), &fs);
        assert_eq!(
            targets,
            vec![
                PathBuf::from("/Users/t/Downloads"),
                PathBuf::from("/Users/t/Documents"),
                PathBuf::from("/Users/t/Desktop"),
            ]
        );
    }

    #[test]
    fn missing_paths_are_skipped() {
        // Documents 만 없는 홈.
        let fs = FakeFs::new(["/Users/t/Downloads", "/Users/t/Desktop"]);
        let targets = prewarm_targets(Some(Path::new("/Users/t")), &fs);
        assert_eq!(
            targets,
            vec![
                PathBuf::from("/Users/t/Downloads"),
                PathBuf::from("/Users/t/Desktop"),
            ]
        );
    }

    #[test]
    fn no_home_yields_no_home_targets() {
        let fs = FakeFs::new(["/Users/t/Downloads"]);
        assert!(prewarm_targets(None, &fs).is_empty());
    }

    #[test]
    fn absent_volumes_root_contributes_nothing() {
        let fs = FakeFs::new(["/Users/t/Desktop"]);
        let targets = prewarm_targets(Some(Path::new("/Users/t")), &fs);
        assert_eq!(targets, vec![PathBuf::from("/Users/t/Desktop")]);
    }

    #[test]
    fn empty_volumes_root_contributes_nothing() {
        let fs = FakeFs::new(["/Users/t/Desktop", "/Volumes"]);
        let targets = prewarm_targets(Some(Path::new("/Users/t")), &fs);
        assert_eq!(targets, vec![PathBuf::from("/Users/t/Desktop")]);
    }

    #[test]
    fn mounted_volumes_come_last_and_are_sorted() {
        let fs = FakeFs::new([
            "/Users/t/Downloads",
            "/Volumes",
            "/Volumes/Backup",
            "/Volumes/Archive",
        ]);
        let targets = prewarm_targets(Some(Path::new("/Users/t")), &fs);
        assert_eq!(
            targets,
            vec![
                PathBuf::from("/Users/t/Downloads"),
                PathBuf::from("/Volumes/Archive"),
                PathBuf::from("/Volumes/Backup"),
            ]
        );
    }

    #[test]
    fn volumes_are_probed_shallowly() {
        // 볼륨 하위 디렉터리는 목록에 들어가지 않는다 — 마운트당 한 번만 건드린다.
        let fs = FakeFs::new(["/Volumes", "/Volumes/Backup", "/Volumes/Backup/nested"]);
        let targets = prewarm_targets(None, &fs);
        assert_eq!(targets, vec![PathBuf::from("/Volumes/Backup")]);
    }

    #[test]
    fn non_directory_volume_entries_are_skipped() {
        // `/Volumes` 하위의 비-디렉터리(예: `.DS_Store`)는 대상이 아니다 — 파일을
        // `read_dir` 해봐야 볼륨 프롬프트가 뜨지 않는다.
        let fs = FakeFs::new(["/Volumes", "/Volumes/Backup"]).with_files(["/Volumes/.DS_Store"]);
        assert_eq!(
            prewarm_targets(None, &fs),
            vec![PathBuf::from("/Volumes/Backup")]
        );
    }
}
