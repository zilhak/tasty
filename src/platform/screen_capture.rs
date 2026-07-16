//! OS 네이티브 인터랙티브 화면 캡처 (03 — 원격 attach 스크린샷→클립보드).
//!
//! `ui.screenshot`(`src/gfx/gpu/screenshot.rs`)은 tasty **자신이 렌더링한** 프레임만
//! 캡처한다(GPU 텍스처 readback). 이 모듈은 그와 달리 **임의 화면**(다른 앱 포함)을
//! 사용자가 인터랙티브하게 선택해 캡처하는, macOS `Cmd+Shift+4` 류 OS 자체 스크린샷과
//! 동등한 기능이다 — tasty 코드에 이런 기능이 기존에 없었다(신규).
//!
//! 플랫폼별 구현:
//! - **macOS**: `screencapture -i` — OS 표준 인터랙티브 선택 캡처.
//! - **Linux**: Wayland 면 `grim`+`slurp`(영역 선택 후 캡처), 아니면(X11)
//!   `gnome-screenshot -a` → `scrot -s` → ImageMagick `import` 순으로 설치된 도구를
//!   찾아 사용한다. 디스플레이 서버 판별은 `WAYLAND_DISPLAY` 환경변수 존재 여부.
//! - **Windows**: OS 표준 인터랙티브 선택 CLI 가 없다(Snipping Tool 의 `ms-screenclip:`
//!   프로토콜은 결과를 클립보드 **이미지**로만 내놓아 "경로 텍스트 전달" 요구사항과
//!   맞지 않는다) — PowerShell + `System.Drawing`으로 전체 가상 화면(다중 모니터 포함)을
//!   캡처한다. 인터랙티브 영역 선택은 아니지만 Windows 7+ 어디서나 추가 설치 없이
//!   동작하는 실용적 대안이다.
//!
//! 사용자가 캡처를 취소(Esc)하면 파일이 생성되지 않는다 — [`capture_interactive`]는
//! 프로세스 exit code 가 아니라 **파일 존재 여부**로 성공/취소를 판정한다(도구별로
//! 취소 시 exit code 관행이 다르므로 이게 유일하게 일관된 신호).

use std::path::{Path, PathBuf};
use std::process::Command;

/// 인터랙티브 화면 캡처를 실행해 `~/.tasty/screenshots/screenshot-<ms>.png` 에 저장하고
/// 그 경로를 반환한다. 사용자가 취소했거나(파일 미생성) 지원 도구를 못 찾으면 Err.
///
/// 블로킹(자식 프로세스 대기) — 반드시 백그라운드 스레드에서 호출해야 한다(메인
/// 루프를 막지 않기 위해). 호출부: `App::poll_screenshot_captures`.
pub fn capture_interactive() -> anyhow::Result<PathBuf> {
    let path = next_screenshot_path()?;

    capture_to_path(&path)?;

    if !path.exists() {
        anyhow::bail!("screen capture produced no file (cancelled?)");
    }
    Ok(path)
}

/// `~/.tasty/screenshots/` 를 만들고 그 안에 새 타임스탬프 파일 경로를 발급한다.
/// 실제 OS 캡처 호출과 분리해둔 이유: 이 부분만 실제 화면 캡처 도구를 실행하지
/// 않고 단위테스트로 검증하기 위함(도구 실행은 환경 의존적이고, 디스플레이가 없는
/// 헤드리스 환경에서 일부 도구는 인터랙티브 선택을 무한 대기해 테스트를 멈춘다).
fn next_screenshot_path() -> anyhow::Result<PathBuf> {
    let dir = crate::paths::tasty_home()
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
    let status = Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", &script])
        .status()?;
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
    match Command::new("grim").arg("-g").arg(&geometry).arg(path).status() {
        Ok(_) => Ok(true),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(e.into()),
    }
}

/// GNOME(X11/Wayland 공용, portal 경유): `gnome-screenshot -a -f <path>`(영역 선택).
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
        // TASTY_HOME override 로 임시 디렉터리를 격리. `next_screenshot_path` 만
        // 호출한다 — `capture_interactive`/`capture_to_path` 는 실제 OS 캡처 도구를
        // 실행하므로(headless 환경에 설치돼 있으면 디스플레이 없이 인터랙티브 선택을
        // 무한 대기할 수 있음), 단위테스트에서는 절대 실행하지 않는다.
        let tmp = std::env::temp_dir().join(format!(
            "tasty-screenshot-test-{}",
            std::process::id()
        ));
        // SAFETY: 이 테스트 프로세스 전용 임시 env 조작 — 병렬 테스트 간 공유 상태
        // 변경 위험은 std::env::set_var 의 통상적 테스트 관행과 동일 수준(단일 스레드
        // 테스트 바이너리 내에서 이 값을 읽는 다른 테스트 없음).
        unsafe {
            std::env::set_var("TASTY_HOME", &tmp);
        }
        let path = next_screenshot_path().expect("dir creation must succeed");
        assert!(path.starts_with(tmp.join("screenshots")));
        assert!(tmp.join("screenshots").is_dir());
        unsafe {
            std::env::remove_var("TASTY_HOME");
        }
        let _ = std::fs::remove_dir_all(&tmp);
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
