//! OS 도구로 화면을 캡처한다. macOS는 screencapture -i, Linux는 grim/slurp 또는
//! gnome-screenshot/scrot/import를 사용하며 Windows는 PowerShell로 가상 화면 전체를 캡처한다.
//! Windows 경로는 대화형 영역 선택이 아니다.
//! macOS는 실행 전 화면 기록 권한을 조회한다. 그 뒤 결과 파일이 없으면 Cancelled로 분류한다.
//! 일부 도구는 비정상 종료도 파일 부재로만 판정하므로 Cancelled가 사용자 취소만을 증명하지는 않는다.

use std::path::{Path, PathBuf};
use std::process::Command;

/// 캡처를 완료하지 못한 이유.
#[derive(Debug)]
pub enum CaptureError {
    /// 화면 기록 권한 미승인 (macOS). 시스템 설정에서 사용자가 켜야 한다.
    PermissionDenied,
    /// 도구 실행 뒤 결과 파일이 없어 취소로 분류했다.
    Cancelled,
    /// 권한 조회 이후 준비·도구 실행에서 반환된 오류. 비정상 종료를 검사하는 경로도 포함한다.
    Tool(anyhow::Error),
}

impl std::fmt::Display for CaptureError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::PermissionDenied => {
                write!(f, "screen recording permission is not granted")
            }
            Self::Cancelled => write!(f, "screen capture produced no file (cancelled)"),
            Self::Tool(err) => write!(f, "screen capture tool failed: {err}"),
        }
    }
}

impl std::error::Error for CaptureError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Tool(err) => Some(err.as_ref()),
            _ => None,
        }
    }
}

/// 인터랙티브 화면 캡처를 실행해 `~/.tasty/screenshots/screenshot-<ms>.png` 에 저장하고
/// 그 경로를 반환한다.
///
/// 블로킹(자식 프로세스 대기) — 반드시 백그라운드 스레드에서 호출해야 한다(메인
/// 루프를 막지 않기 위해). 호출부: `App::poll_screenshot_captures`.
pub fn capture_interactive() -> Result<PathBuf, CaptureError> {
    // 권한 조회는 캡처 **직전**에 한다 — 부팅 시점 값을 캐시해두면 그 사이 사용자가
    // 시스템 설정에서 권한을 바꾼 경우를 잘못 판정한다. 비-macOS 는 항상 true.
    if !crate::macos_permissions::screen_recording_authorized() {
        return Err(CaptureError::PermissionDenied);
    }

    let path = next_screenshot_path().map_err(CaptureError::Tool)?;

    capture_to_path(&path).map_err(CaptureError::Tool)?;

    if !path.exists() {
        return Err(CaptureError::Cancelled);
    }
    Ok(path)
}

/// 저장 디렉터리와 타임스탬프 경로를 만든다. 실제 화면 도구 없이 이 단계만 시험할 수 있다.
fn next_screenshot_path() -> anyhow::Result<PathBuf> {
    let dir = tasty_utils::path::tasty_home()
        .ok_or_else(|| anyhow::anyhow!("no tasty home directory (TASTY_HOME/HOME unresolved)"))?
        .join("screenshots");
    std::fs::create_dir_all(&dir)?;
    let millis = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    Ok(dir.join(format!("screenshot-{millis}.png")))
}

#[cfg(target_os = "macos")]
fn capture_to_path(path: &Path) -> anyhow::Result<()> {
    // `-i` = interactive(영역/윈도우 선택). 취소 시에도 exit code 0 인 macOS 버전이
    // 있어 status 를 강제하지 않는다 — 성공 판정은 호출부의 파일 존재 확인.
    Command::new("screencapture").arg("-i").arg(path).status()?;
    Ok(())
}

#[cfg(target_os = "windows")]
fn capture_to_path(path: &Path) -> anyhow::Result<()> {
    // PowerShell 인용 규칙: 단일따옴표 문자열 안의 `'` 는 `''` 로 이스케이프.
    let path_escaped = path.to_string_lossy().replace('\'', "''");
    let script = format!(
        "Add-Type -AssemblyName System.Windows.Forms,System.Drawing; \
         $b = [System.Windows.Forms.SystemInformation]::VirtualScreen; \
         $bmp = New-Object System.Drawing.Bitmap $b.Width, $b.Height; \
         $g = [System.Drawing.Graphics]::FromImage($bmp); \
         $g.CopyFromScreen($b.Location, [System.Drawing.Point]::Empty, $b.Size); \
         $bmp.Save('{path_escaped}', [System.Drawing.Imaging.ImageFormat]::Png); \
         $g.Dispose(); $bmp.Dispose()"
    );
    let mut cmd = Command::new("powershell");
    cmd.args(["-NoProfile", "-NonInteractive", "-Command", &script]);
    tasty_utils::process::hide_console(&mut cmd);
    let status = cmd.status()?;
    if !status.success() {
        anyhow::bail!("powershell screen capture exited with {status}");
    }
    Ok(())
}

#[cfg(all(unix, not(target_os = "macos")))]
fn capture_to_path(path: &Path) -> anyhow::Result<()> {
    let wayland = std::env::var_os("WAYLAND_DISPLAY").is_some();
    if wayland {
        if try_grim_slurp(path)? {
            return Ok(());
        }
        if try_gnome_screenshot(path)? {
            return Ok(());
        }
        anyhow::bail!(
            "no supported Wayland screen capture tool found — install grim+slurp or gnome-screenshot"
        );
    }
    if try_gnome_screenshot(path)? {
        return Ok(());
    }
    if try_scrot(path)? {
        return Ok(());
    }
    if try_import(path)? {
        return Ok(());
    }
    anyhow::bail!(
        "no supported X11 screen capture tool found — install gnome-screenshot, scrot, or ImageMagick (import)"
    );
}

/// `cmd`를 spawn 해 완료를 기다린다. 바이너리가 없으면(`NotFound`) `Ok(false)`(다음
/// 후보로 폴백), 그 외 실행 자체는 exit code 와 무관하게 `Ok(true)`(취소는 호출부의
/// 파일 존재 확인으로 판정), 스폰 자체의 다른 실패는 `Err`.
#[cfg(all(unix, not(target_os = "macos")))]
fn try_command_capture(cmd: &str, args: &[&str]) -> anyhow::Result<bool> {
    match Command::new(cmd).args(args).status() {
        Ok(_status) => Ok(true),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(e.into()),
    }
}

/// Wayland: `slurp` 로 영역을 인터랙티브 선택받아 `grim -g <geometry>` 로 캡처.
/// 어느 한쪽 바이너리라도 없으면 `Ok(false)`(다음 후보로 폴백).
#[cfg(all(unix, not(target_os = "macos")))]
fn try_grim_slurp(path: &Path) -> anyhow::Result<bool> {
    let slurp = match Command::new("slurp").output() {
        Ok(o) => o,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(e) => return Err(e.into()),
    };
    // slurp 취소(Esc)/실패 — 선택 없음. 이 경우도 "도구는 있었다" 로 취급해 다음
    // 후보로 넘기지 않는다(파일 미생성 → 호출부가 취소로 판정).
    if !slurp.status.success() {
        return Ok(true);
    }
    let geometry = String::from_utf8_lossy(&slurp.stdout).trim().to_string();
    if geometry.is_empty() {
        return Ok(true);
    }
    match Command::new("grim")
        .arg("-g")
        .arg(&geometry)
        .arg(path)
        .status()
    {
        Ok(_) => Ok(true),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(e.into()),
    }
}

/// GNOME 도구로 영역을 선택한다: gnome-screenshot -a -f <path>.
#[cfg(all(unix, not(target_os = "macos")))]
fn try_gnome_screenshot(path: &Path) -> anyhow::Result<bool> {
    let path_str = path.to_string_lossy().to_string();
    try_command_capture("gnome-screenshot", &["-a", "-f", &path_str])
}

/// X11: `scrot -s <path>`(인터랙티브 영역/윈도우 선택).
#[cfg(all(unix, not(target_os = "macos")))]
fn try_scrot(path: &Path) -> anyhow::Result<bool> {
    let path_str = path.to_string_lossy().to_string();
    try_command_capture("scrot", &["-s", &path_str])
}

/// X11: ImageMagick `import <path>`(인자 없이 실행하면 클릭/드래그로 인터랙티브 선택).
#[cfg(all(unix, not(target_os = "macos")))]
fn try_import(path: &Path) -> anyhow::Result<bool> {
    let path_str = path.to_string_lossy().to_string();
    try_command_capture("import", &[&path_str])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capture_interactive_dir_is_under_tasty_home() {
        // 경로 생성만 확인한다. 사용자 화면을 캡처하거나 대화형 OS 도구를 실행하지 않는다.
        let home = tasty_test_support::TastyHomeGuard::new();
        let path = next_screenshot_path().expect("dir creation must succeed");
        assert!(path.starts_with(home.path().join("screenshots")));
        assert!(home.path().join("screenshots").is_dir());
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    #[test]
    fn try_command_capture_missing_binary_returns_false() {
        assert_eq!(
            try_command_capture("tasty-definitely-not-a-real-binary", &[]).unwrap(),
            false
        );
    }
}
