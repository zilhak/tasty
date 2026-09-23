//! macOS 보호 폴더·화면 기록·손쉬운 사용의 권한 요청과 Full Disk Access 추정.
//! 요청은 사용자가 설정 > 일반 > 권한 의 [모든 권한 요청하기] 를 눌렀을 때만 워커에서
//! 돈다. 부팅 직후 자동 발화는 하지 않는다 — 결정의 근거·대안·재검토 조건은
//! `docs/adr/0052-permission-prompts-are-raised-on-request-not-at-boot.md`.
//! 매 부팅 현재 상태를 확인하며 승인·표시 여부 자체는 OS가 결정한다.
//! 이미 허용/거부가 결정된 항목에는 프롬프트가 뜨지 않으므로 몇 번을 눌러도 무해하다.
//! 경로 목록과 상태 분류는 OS 접근과 분리해 다른 플랫폼에서도 시험한다.

#[cfg(any(target_os = "macos", test))]
use std::path::{Path, PathBuf};

/// 홈 보호 폴더의 접근 순서.
#[cfg(any(target_os = "macos", test))]
const HOME_SUBDIRS: [&str; 3] = ["Downloads", "Documents", "Desktop"];

/// 마운트 루트. 이동식(`SystemPolicyRemovableVolumes`)·네트워크
/// (`SystemPolicyNetworkVolumes`) 볼륨이 모두 여기 하위에 붙는다.
#[cfg(any(target_os = "macos", test))]
const VOLUMES_ROOT: &str = "/Volumes";

/// 권한 프롬프트나 실제 파일 접근 없이 경로 선택을 시험하기 위한 인터페이스.
#[cfg(any(target_os = "macos", test))]
trait FsProbe {
    /// 디렉터리로 존재하는가. 없는 폴더는 읽어봐야 프롬프트가 안 뜨므로 건너뛴다.
    fn is_dir(&self, path: &Path) -> bool;

    /// depth-1 나열. 실패(권한·부재)는 빈 목록으로 접는다 — 마운트 목록을 못 읽는 것은
    /// pre-warm 을 중단할 사유가 아니다.
    fn list_dir(&self, path: &Path) -> Vec<PathBuf>;
}

/// 실제 파일시스템. pre-warm 실행부와 같은 조건으로만 컴파일한다.
#[cfg(all(target_os = "macos", feature = "gui"))]
struct RealFs;

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

/// 홈 세 폴더 다음에 정렬한 /Volumes 하위 디렉터리를 반환한다. 없는 경로는 제외한다.
/// 목록을 만드는 파일시스템 조회도 지연될 수 있으므로 이 함수 자체가 즉시 끝난다고 보장하지 않는다.
#[cfg(any(target_os = "macos", test))]
fn prewarm_targets(home: Option<&Path>, fs: &dyn FsProbe) -> Vec<PathBuf> {
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
        // 파일시스템 열거 순서와 무관하게 접근 목록을 정렬한다.
        volumes.sort();
        targets.extend(volumes);
    }

    targets
}

// CoreGraphics의 화면 기록 승인 조회·요청 API. 요청 결과와 실제 캡처 성공은 별개다.
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
pub fn screen_recording_authorized() -> bool {
    // SAFETY: 인자도 반환 포인터도 없는 CoreGraphics C 함수 호출 — 포인터 수명/해제
    // 책임이 발생하지 않고, panic 을 가로지르는 상태도 남기지 않는다. 내부적으로
    // TCC 데몬에 현재 앱의 승인 상태를 묻기만 하며 AppKit 을 건드리지 않아
    // main thread 한정이 아니다(캡처 워커 스레드에서도 호출된다).
    unsafe { CGPreflightScreenCaptureAccess() }
}

/// macOS GUI 이외에서는 이 TCC 검사를 생략해 true를 반환한다. 캡처 성공 보장은 아니다.
#[cfg(not(all(target_os = "macos", feature = "gui")))]
pub fn screen_recording_authorized() -> bool {
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

// 손쉬운 사용 권한은 debug 전용 OS 키 주입에서 필요하다. 상태 조회는 안내를 띄우지 않고,
// 요청은 시스템 설정으로 이동할 안내를 띄운다. 실제 승인은 사용자가 설정해야 한다.
// 소비자와 같은 debug cfg로 FFI 선언을 제한한다.
#[cfg(all(debug_assertions, target_os = "macos", feature = "gui"))]
#[link(name = "ApplicationServices", kind = "framework")]
unsafe extern "C" {
    fn AXIsProcessTrusted() -> bool;
}

// 권한 요청에서만 사용하는 debug 전용 심볼.
#[cfg(all(debug_assertions, target_os = "macos", feature = "gui"))]
#[link(name = "ApplicationServices", kind = "framework")]
unsafe extern "C" {
    fn AXIsProcessTrustedWithOptions(options: *const std::ffi::c_void) -> bool;
    /// 옵션 딕셔너리의 키(`CFStringRef` 전역). 문자열 값을 직접 만들지 않고 프레임워크가
    /// 내보내는 심볼을 그대로 쓴다 — 값이 바뀌어도 따라간다.
    static kAXTrustedCheckOptionPrompt: *const std::ffi::c_void;
}

// CoreFoundation 쪽도 전량 `prewarm_accessibility` 전용이다(위와 같은 이유로 debug 한정).
#[cfg(all(debug_assertions, target_os = "macos", feature = "gui"))]
#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    static kCFBooleanTrue: *const std::ffi::c_void;
    /// CFType 표준 콜백. 이걸 넘겨야 딕셔너리가 키를 **CFEqual 로** 비교하고
    /// retain/release 를 관리한다. null 콜백은 포인터 동일성 비교라 여기선 부적절하다.
    static kCFTypeDictionaryKeyCallBacks: std::ffi::c_void;
    static kCFTypeDictionaryValueCallBacks: std::ffi::c_void;
    fn CFDictionaryCreate(
        alloc: *const std::ffi::c_void,
        keys: *const *const std::ffi::c_void,
        values: *const *const std::ffi::c_void,
        count: isize,
        key_callbacks: *const std::ffi::c_void,
        value_callbacks: *const std::ffi::c_void,
    ) -> *const std::ffi::c_void;
    fn CFRelease(cf: *const std::ffi::c_void);
}

/// debug OS 키 주입용 손쉬운 사용 승인 상태를 현재 시점에 조회한다.
/// 부팅 값을 캐시하지 않으며 안내창은 띄우지 않는다.
#[cfg(all(debug_assertions, target_os = "macos", feature = "gui"))]
pub fn accessibility_trusted() -> bool {
    // SAFETY: 인자도 반환 포인터도 없는 ApplicationServices C 함수 호출 — 포인터
    // 수명/해제 책임이 생기지 않는다. 현재 프로세스의 TCC 승인 상태를 묻기만 하고
    // 프롬프트를 띄우지 않으므로 블록하지 않으며, AppKit 을 건드리지 않아 main
    // thread 한정이 아니다(IPC 핸들러 스레드에서도 호출된다).
    unsafe { AXIsProcessTrusted() }
}

/// macOS GUI 이외의 debug 빌드는 이 TCC 검사를 생략해 true를 반환한다.
#[cfg(all(debug_assertions, not(all(target_os = "macos", feature = "gui"))))]
pub fn accessibility_trusted() -> bool {
    true
}

/// OS 키 주입의 승인 여부를 실제 FFI와 분리해 판정한다. 미승인이면 주입하지 않는다.
/// 소비자는 debug 전용이며 순수 규칙 시험은 test 빌드에서도 사용한다.
#[cfg(any(debug_assertions, test))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RawKeyDecision {
    /// 승인됨 — 그대로 주입한다.
    Inject,
    /// 미승인 — 주입하지 않고 권한 부재를 에러로 돌려준다.
    PermissionDenied,
}

/// 위 판정의 **순수** 규칙. FFI 호출과 분리해 두면 macOS 밖에서도 검증된다.
#[cfg(any(debug_assertions, test))]
pub fn raw_key_decision(accessibility_trusted: bool) -> RawKeyDecision {
    if accessibility_trusted {
        RawKeyDecision::Inject
    } else {
        RawKeyDecision::PermissionDenied
    }
}

/// debug 부팅에서 손쉬운 사용 권한 안내를 요청한다. 이미 승인됐으면 생략한다.
/// 실제 권한을 사용하는 OS 키 주입이 release에 없어 release에서는 요청하지 않는다.
#[cfg(all(debug_assertions, target_os = "macos", feature = "gui"))]
fn prewarm_accessibility() {
    if accessibility_trusted() {
        tracing::debug!("prewarm: 손쉬운 사용 권한 이미 승인됨");
        return;
    }
    // SAFETY: `kAXTrustedCheckOptionPrompt: kCFBooleanTrue` 딕셔너리를 만들어 넘기는
    // 표준 호출 시퀀스다.
    // - 키/값은 프레임워크가 소유하는 전역 CF 객체라 이쪽에 해제 책임이 없다.
    // - CFType 표준 콜백을 넘겨 딕셔너리가 키를 CFEqual 로 비교하고 retain 을 관리한다.
    // - `CFDictionaryCreate` 는 +1 retain 으로 돌아오므로 사용 직후 `CFRelease` 로 짝을
    //   맞춘다. 그 사이에 조기 반환이나 panic 지점이 없다.
    // - 프롬프트가 뜨는 동안 블록할 수 있어 워커 스레드에서만 호출한다.
    // CF 시퀀스가 한 트랜잭션이라 분할하면 retain/release 짝이 흩어진다.
    #[allow(clippy::multiple_unsafe_ops_per_block)]
    let granted = unsafe {
        let keys = [kAXTrustedCheckOptionPrompt];
        let values = [kCFBooleanTrue];
        let options = CFDictionaryCreate(
            std::ptr::null(),
            keys.as_ptr(),
            values.as_ptr(),
            1,
            &kCFTypeDictionaryKeyCallBacks as *const _ as *const std::ffi::c_void,
            &kCFTypeDictionaryValueCallBacks as *const _ as *const std::ffi::c_void,
        );
        let trusted = AXIsProcessTrustedWithOptions(options);
        CFRelease(options);
        trusted
    };
    // 프롬프트는 "설정을 열겠느냐" 안내라, 여기서 false 여도 사용자가 이제부터 켤 수 있다.
    tracing::debug!(granted, "prewarm: 손쉬운 사용 권한 요청 결과");
}

// Full Disk Access는 사용자가 시스템 설정에서 직접 부여한다.
// 여기서는 보호 경로 접근으로 상태를 추정하고 설정 패널 안내만 제공한다.

/// 시스템 설정의 전체 디스크 접근 권한 패널 딥링크.
#[cfg(all(target_os = "macos", feature = "gui"))]
pub const FULL_DISK_ACCESS_SETTINGS_URL: &str =
    "x-apple.systempreferences:com.apple.preference.security?Privacy_AllFiles";

/// FDA 추정에 사용하는 보호 경로. 하나라도 열리면 Granted로 분류한다.
/// OS의 경로·보호 정책에 의존하는 추정이며 공개 승인 조회 API의 결과가 아니다.
#[cfg(any(target_os = "macos", test))]
fn fda_probe_paths(home: Option<&Path>) -> Vec<PathBuf> {
    let mut paths = vec![PathBuf::from(
        "/Library/Application Support/com.apple.TCC/TCC.db",
    )];
    // 보조 경로 — 시스템 경로의 보호 정책이 바뀌었을 때의 완화책. 사용자별 TCC 저장소도
    // 같은 보호를 받으므로 판정 신호로 쓸 수 있다.
    if let Some(home) = home {
        paths.push(home.join("Library/Application Support/com.apple.TCC/TCC.db"));
    }
    paths
}

/// 보호 경로 조회로 추정한 FDA 상태. 하나라도 열리면 Granted, 열린 곳 없이
/// PermissionDenied가 있으면 Denied, 그 외에는 Unknown이다. 경로 부재를 거부로 단정하지 않는다.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FullDiskAccess {
    /// 프로브 경로가 열렸다 — 보유로 본다.
    Granted,
    /// 열린 경로가 없고 거부가 있었다 — 미보유로 본다.
    Denied,
    /// 열리지도 거부되지도 않았다(경로 없음 등) — 판정할 근거가 없다.
    Unknown,
}

/// 경로별 프로브 결과에서 판정을 뽑는 **순수** 규칙. `None` 이 열림, `Some(kind)` 가
/// 그 오류로 실패했다는 뜻이다.
#[cfg(any(test, all(target_os = "macos", feature = "gui")))]
fn decide_full_disk_access(probes: &[Option<std::io::ErrorKind>]) -> FullDiskAccess {
    if probes.iter().any(Option::is_none) {
        FullDiskAccess::Granted
    } else if probes.contains(&Some(std::io::ErrorKind::PermissionDenied)) {
        FullDiskAccess::Denied
    } else {
        FullDiskAccess::Unknown
    }
}

/// 확인 가능한 권한 중 하나라도 미승인으로 확인되면 부팅 안내를 표시한다.
/// 과거 표시 여부 대신 매번 현재 상태를 사용한다.
/// FDA 와 화면 기록만 본다. 파일 폴더 권한은 상태를 묻는 API 가 없고 재는 것 자체가
/// 프롬프트라 판정에 넣지 않으며, 그래서 둘 다 허용한 사용자에게는 안내가 뜨지 않는
/// 사각이 남는다. 손쉬운 사용은 release 에 소비자가 없어 제외한다.
/// Unknown 은 미승인으로 세지 않는다. 근거가 사라졌을 뿐이며 승인을 가진 사용자에게
/// 매 부팅 오탐을 띄우게 된다.
#[cfg(any(test, all(target_os = "macos", feature = "gui")))]
fn should_show_permission_notice(full_disk_access: FullDiskAccess, screen_recording: bool) -> bool {
    matches!(full_disk_access, FullDiskAccess::Denied) || !screen_recording
}

/// 보호 경로를 열어 FDA 상태를 추정한다.
#[cfg(all(target_os = "macos", feature = "gui"))]
pub fn full_disk_access_state() -> FullDiskAccess {
    let probes: Vec<Option<std::io::ErrorKind>> = fda_probe_paths(home_dir().as_deref())
        .iter()
        .map(|p| std::fs::File::open(p).err().map(|e| e.kind()))
        .collect();
    decide_full_disk_access(&probes)
}

/// 부팅 시 권한 안내를 띄워야 하는가. **이때 표시용 스냅샷을 새로 재서 보관한다** —
/// 부팅 판정과 권한 화면이 같은 측정 1 회를 공유한다. 비-macOS / headless 에서는
/// 스냅샷이 `Unknown` + 화면 기록 승인이라 안내하지 않는다.
pub fn wants_permission_notice() -> bool {
    let snapshot = refresh_permission_snapshot();
    should_show_permission_notice(snapshot.full_disk_access, snapshot.screen_recording)
}

// ── 표시용 권한 상태 스냅샷 ────────────────────────────────────────────────────
//
// 설정 > 일반 > 권한 탭은 상태를 **재지 않고 읽는다.** 측정은 파일 열기 syscall 과 TCC
// 데몬 IPC 라, draw(렌더) 경로에 두면 tccd 응답이 늦는 만큼 설정 창이 멈추고, egui 가
// repaint 를 요구하는 입력(마우스 이동·호버)이 이어지는 동안 그 횟수만큼 반복된다.
// 값이 필요한 시점은 프레임이 아니라 "상태가 바뀔 수 있었던 시점" 이다.
//
// **캐시와 갱신은 세트다.** FDA 는 앱이 요청할 수 없어 사용자가 시스템 설정에 다녀오는
// 왕복이 반드시 생기고(위 "Full Disk Access" 주석), 화면 기록도 거부 이후에는 시스템
// 설정에서만 되돌릴 수 있다. 부팅 값만 들고 있으면 그 왕복 결과가 화면에 영영 반영되지
// 않는다. 그래서 갱신 트리거를 함께 둔다 — 부팅 1 회(`wants_full_disk_access_notice`),
// 권한 화면 진입(`apply_l2_select`), 설정 창 포커스 복귀(`SettingsView::handle_event`).
//
// **캡처·주입 경로는 이 스냅샷을 쓰지 않는다.** `screen_capture.rs` 의
// `screen_recording_authorized()` 와 `input_source.rs` 의 `accessibility_trusted()` 는
// 그 동작 직전 실측이 의도된 정책이다(각 함수의 주석). 두 소비처를 같은 캐시로 묶으면
// 캡처·주입 판정이 낡은 값을 보게 된다.

/// 권한 화면이 표시하는 상태 한 벌. **측정 시점의 값**이며, 그 뒤 사용자가 시스템
/// 설정에서 바꾼 것은 다음 갱신 전까지 반영되지 않는다.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PermissionSnapshot {
    /// Full Disk Access 추정 3 상태.
    pub full_disk_access: FullDiskAccess,
    /// 화면 기록 승인 여부.
    pub screen_recording: bool,
    /// 손쉬운 사용 승인 여부. **debug 빌드에만 있다** — release 에는 이 권한을 소비하는
    /// 코드가 없어 표시할 행 자체가 없다(`accessibility_trusted` 참고).
    #[cfg(debug_assertions)]
    pub accessibility: bool,
}

/// 마지막으로 잰 값. `None` 은 아직 한 번도 재지 않았다는 뜻이다.
static PERMISSION_SNAPSHOT: std::sync::RwLock<Option<PermissionSnapshot>> =
    std::sync::RwLock::new(None);

/// 보관된 값을 읽는다 — **측정하지 않는다.** 표시 경로(draw)가 부르는 쪽이다.
///
/// 아직 한 번도 재지 않았으면 그 자리에서 1 회 잰다. 부팅이 먼저 재므로 정상 흐름에서는
/// 일어나지 않고, 측정 없이 "허용 안 됨" 을 표시해 승인을 가진 사용자에게 거짓을 말하는
/// 것보다 1 회 측정이 낫다.
pub fn permission_snapshot() -> PermissionSnapshot {
    if let Some(snapshot) = PERMISSION_SNAPSHOT.read().ok().and_then(|g| *g) {
        return snapshot;
    }
    refresh_permission_snapshot()
}

/// 지금 상태를 다시 재서 보관하고 그 값을 돌려준다. 위 "갱신 트리거" 에서만 부른다.
pub fn refresh_permission_snapshot() -> PermissionSnapshot {
    let snapshot = measure_permissions();
    // 측정이 실제로 여기서만 일어나는지(= draw 경로에서 빠졌는지) 세는 자리다.
    tracing::debug!(?snapshot, "권한 상태 스냅샷 갱신");
    match PERMISSION_SNAPSHOT.write() {
        Ok(mut guard) => *guard = Some(snapshot),
        // 보관만 실패한 것이라 이번 측정값은 그대로 쓴다 — 다음 갱신에서 다시 시도한다.
        Err(err) => tracing::warn!(%err, "권한 상태 스냅샷 보관 실패"),
    }
    snapshot
}

/// 실제 측정. 여기서만 TCC 를 건드린다.
#[cfg(all(target_os = "macos", feature = "gui"))]
fn measure_permissions() -> PermissionSnapshot {
    PermissionSnapshot {
        full_disk_access: full_disk_access_state(),
        screen_recording: screen_recording_authorized(),
        #[cfg(debug_assertions)]
        accessibility: accessibility_trusted(),
    }
}

/// 비-macOS / headless — 항목별 동명 조회와 같은 답을 낸다(권한 개념이 없으므로 제약
/// 없음). FDA 만 `Unknown` 이다: 없는 권한을 "보유" 로 적으면 안내 판정이 그 값을 근거로
/// 쓰게 된다.
#[cfg(not(all(target_os = "macos", feature = "gui")))]
fn measure_permissions() -> PermissionSnapshot {
    PermissionSnapshot {
        full_disk_access: FullDiskAccess::Unknown,
        screen_recording: screen_recording_authorized(),
        #[cfg(debug_assertions)]
        accessibility: accessibility_trusted(),
    }
}

/// 시스템 설정의 전체 디스크 접근 권한 패널을 연다. `open(1)` 로 띄운다 —
/// `x-apple.systempreferences:` 는 브라우저가 아니라 OS 기본 핸들러가 처리한다.
/// 프로세스를 기다리지 않는다(렌더 경로에서 호출될 수 있다).
#[cfg(all(target_os = "macos", feature = "gui"))]
pub fn open_full_disk_access_settings() {
    #[cfg(debug_assertions)]
    if crate::debug_os_open::intercepted("open_settings", FULL_DISK_ACCESS_SETTINGS_URL) {
        return;
    }
    if let Err(err) = std::process::Command::new("open")
        .arg(FULL_DISK_ACCESS_SETTINGS_URL)
        .spawn()
    {
        tracing::warn!(%err, "전체 디스크 접근 권한 설정 패널 열기 실패");
    }
}

/// 홈 디렉터리 — `DirectoriesHome` 과 같은 해석(`directories::BaseDirs`).
#[cfg(all(target_os = "macos", feature = "gui"))]
fn home_dir() -> Option<PathBuf> {
    directories::BaseDirs::new().map(|b| b.home_dir().to_path_buf())
}

/// 요청 시퀀스가 도는 중인가. 버튼 재진입을 막고 진행 표시를 켜는 좌변이다.
static REQUEST_RUNNING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// 요청 시퀀스가 지금 도는 중인가. draw 경로가 매 프레임 읽어도 되는 원자 로드다.
pub fn permission_request_running() -> bool {
    REQUEST_RUNNING.load(std::sync::atomic::Ordering::Acquire)
}

/// 워커를 시작해 목록의 폴더, 화면 기록, debug 손쉬운 사용 순으로 요청한다.
/// 파일 접근과 시스템 요청이 사용자 응답이나 네트워크를 기다릴 수 있어 메인 루프에서 실행하지 않는다.
/// 하나씩 순차로 요청한다. 동시에 건드리면 프롬프트가 겹쳐 뜬다.
/// 이미 돌고 있으면 아무것도 하지 않고 `false` 를 돌려준다.
/// 끝나면 표시용 스냅샷을 갱신하고 `on_finished` 로 호출자를 깨운다.
#[cfg(all(target_os = "macos", feature = "gui"))]
pub fn request_all_permissions(on_finished: impl FnOnce() + Send + 'static) -> bool {
    use std::sync::atomic::Ordering;
    if REQUEST_RUNNING
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        tracing::debug!("권한 요청: 이미 도는 중이라 무시한다");
        return false;
    }
    std::thread::spawn(move || {
        let targets = prewarm_targets(home_dir().as_deref(), &RealFs);
        tracing::debug!(count = targets.len(), "권한 요청: 파일 TCC 대상 결정");
        for path in targets {
            // 접근 시도가 목적이다. 반환된 항목은 쓰지 않으며 거부도 오류 로그 대신 진단으로 남긴다.
            match std::fs::read_dir(&path) {
                Ok(_) => tracing::debug!(path = %path.display(), "권한 요청: 접근 허용"),
                Err(err) => {
                    tracing::debug!(path = %path.display(), %err, "권한 요청: 접근 불가(거부 또는 부재)")
                }
            }
        }
        prewarm_screen_recording();
        // 손쉬운 사용은 debug 빌드에서만 요청한다 — release 에는 소비자가 없다.
        #[cfg(debug_assertions)]
        prewarm_accessibility();
        // 표시용 상태를 먼저 갱신하고 나서 깃발을 내린다 — 반대로 하면 호출자가
        // "끝났다" 를 보고 낡은 스냅샷을 읽을 수 있다.
        refresh_permission_snapshot();
        REQUEST_RUNNING.store(false, Ordering::Release);
        on_finished();
    });
    true
}

/// 비-macOS / headless — 요청할 권한이라는 개념이 없다. 호출부에 `#[cfg]` 를 흩뿌리지
/// 않기 위한 짝이고, 시퀀스를 시작하지 않았으므로 `false` 다. headless 에는 프롬프트를
/// 띄울 GUI 주체가 없으므로 macOS 여도 돌지 않는다.
#[cfg(not(all(target_os = "macos", feature = "gui")))]
pub fn request_all_permissions(_on_finished: impl FnOnce() + Send + 'static) -> bool {
    false
}

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
    fn raw_key_injects_only_when_accessibility_is_trusted() {
        assert_eq!(raw_key_decision(true), RawKeyDecision::Inject);
        // 미승인 상태에서 주입하면 CGEventPost 가 조용히 무시된다 — 성공으로 답하면 안 된다.
        assert_eq!(raw_key_decision(false), RawKeyDecision::PermissionDenied);
    }

    /// 요청 시퀀스가 끝까지 돌고, 끝나면 깃발을 내리고 호출자를 깨우는가.
    ///
    /// **실제 TCC 를 건드리므로 `#[ignore]` 다.** 미결정 항목이 있으면 프롬프트가 떠서
    /// 사용자 응답까지 멈추는데, 헤드리스 러너에는 누를 사람이 없다 — 모듈 최상단의
    /// `FsProbe` 주석이 테스트에 실 IO 를 들이지 않는 이유로 적어 둔 바로 그 함정이다.
    /// 어느 워크플로도 `--ignored` 를 쓰지 않으므로 이 시험은 사람이 부를 때만 돈다.
    /// 재는 법: `cargo test -p tasty-platform --features gui -- --ignored request_sequence`
    ///
    /// 재진입 거부(두 번째 호출이 `false`)는 여기서 재지 않는다 — 워커가 언제 끝나는지에
    /// 달려 있어 결정적이지 않다. 그 규칙은 `REQUEST_RUNNING` 의 원자 교환 한 줄이다.
    #[cfg(all(target_os = "macos", feature = "gui"))]
    #[test]
    #[ignore]
    fn request_sequence_finishes_and_clears_the_running_flag() {
        use std::time::Duration;
        let (tx, rx) = std::sync::mpsc::channel();
        assert!(
            request_all_permissions(move || {
                tx.send(())
                    .expect("완료 통지를 기다리는 수신자가 있어야 한다");
            }),
            "도는 시퀀스가 없으면 요청은 시작돼야 한다"
        );
        rx.recv_timeout(Duration::from_secs(120))
            .expect("시퀀스가 끝나면 완료 통지가 와야 한다");
        assert!(
            !permission_request_running(),
            "완료 통지 시점에는 깃발이 이미 내려가 있어야 한다"
        );

        // 끝난 뒤에는 다시 시작할 수 있다 — 깃발이 걸린 채 남지 않는다.
        let (tx2, rx2) = std::sync::mpsc::channel();
        assert!(
            request_all_permissions(move || {
                tx2.send(())
                    .expect("완료 통지를 기다리는 수신자가 있어야 한다");
            }),
            "앞 시퀀스가 끝났으면 다시 시작할 수 있어야 한다"
        );
        rx2.recv_timeout(Duration::from_secs(120))
            .expect("두 번째 시퀀스도 끝나야 한다");
    }

    /// 읽기는 보관된 값을 그대로 돌려준다 — draw 가 매번 재지 않아도 되는 근거.
    #[test]
    fn snapshot_read_returns_last_refreshed_value() {
        let refreshed = refresh_permission_snapshot();
        assert_eq!(permission_snapshot(), refreshed);
    }

    /// 프로브 결과 → 3 상태 매핑. 안내 판정과 분리돼 있어 따로 고정한다.
    #[test]
    fn full_disk_access_is_denied_only_on_a_confirmed_refusal() {
        use std::io::ErrorKind;

        // 하나라도 열리면 보유다 — 앞쪽 경로가 없어도 뒤쪽이 열리면 보유.
        assert_eq!(decide_full_disk_access(&[None]), FullDiskAccess::Granted);
        assert_eq!(
            decide_full_disk_access(&[Some(ErrorKind::NotFound), None]),
            FullDiskAccess::Granted
        );
        // 거부가 있으면 미보유다.
        assert_eq!(
            decide_full_disk_access(&[Some(ErrorKind::PermissionDenied)]),
            FullDiskAccess::Denied
        );
        // 모든 경로가 없으면 거부가 아닌 판정 불가다.
        assert_eq!(
            decide_full_disk_access(&[Some(ErrorKind::NotFound), Some(ErrorKind::NotFound)]),
            FullDiskAccess::Unknown
        );
    }

    /// 안내 판정의 6 갈래를 전부 고정한다. `Granted` + 화면 기록 미승인이 이 규칙의
    /// 핵심 갈래다 — FDA 만 보던 때에는 그 사용자에게 아무 안내도 뜨지 않았다.
    #[test]
    fn notice_shows_when_any_checkable_permission_is_missing() {
        use FullDiskAccess::*;
        assert!(should_show_permission_notice(Denied, false));
        assert!(should_show_permission_notice(Denied, true));
        // FDA 를 이미 가진 사용자도 화면 기록이 없으면 알아야 한다.
        assert!(should_show_permission_notice(Granted, false));
        assert!(!should_show_permission_notice(Granted, true));
        // 판정 근거가 없는 것을 미승인으로 접지 않는다.
        assert!(!should_show_permission_notice(Unknown, true));
        // 다만 화면 기록 쪽 근거는 확실하므로 그것만으로 띄운다.
        assert!(should_show_permission_notice(Unknown, false));
    }

    #[test]
    fn fda_probe_includes_system_store_first_then_user_store() {
        let paths = fda_probe_paths(Some(Path::new("/Users/t")));
        assert_eq!(
            paths,
            vec![
                PathBuf::from("/Library/Application Support/com.apple.TCC/TCC.db"),
                PathBuf::from("/Users/t/Library/Application Support/com.apple.TCC/TCC.db"),
            ]
        );
    }

    #[test]
    fn fda_probe_without_home_keeps_the_system_store() {
        assert_eq!(
            fda_probe_paths(None),
            vec![PathBuf::from(
                "/Library/Application Support/com.apple.TCC/TCC.db"
            )]
        );
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
        // /Volumes의 일반 파일은 디렉터리 접근 대상에서 제외한다.
        let fs = FakeFs::new(["/Volumes", "/Volumes/Backup"]).with_files(["/Volumes/.DS_Store"]);
        assert_eq!(
            prewarm_targets(None, &fs),
            vec![PathBuf::from("/Volumes/Backup")]
        );
    }
}
