//! OS 도구로 화면을 캡처한다. macOS는 screencapture -i, Linux는 grim/slurp 또는
//! gnome-screenshot/scrot/import를 사용하며 Windows는 PowerShell로 가상 화면 전체를 캡처한다.
//! Windows 경로는 대화형 영역 선택이 아니다.
//!
//! 결과는 파일 존재·종료 상태·도구의 취소 메시지를 함께 보고 분류한다. 도구가 취소를
//! 구별 가능한 신호로 알리지 않는 결과(정상 종료인데 파일이 없음 등)는 [`CaptureError::NoOutput`]
//! 으로 남기고 사용자 취소로 확정하지 않는다. 분류 표는
//! `docs/features/remote-screenshot-clipboard/index.md` 의 "결과 분류" 절에 있다.

use std::path::{Path, PathBuf};
#[cfg(any(target_os = "macos", target_os = "windows"))]
use std::process::Command;

/// 캡처를 완료하지 못한 이유.
#[derive(Debug)]
pub enum CaptureError {
    /// 화면 기록 권한 미승인 (macOS). 시스템 설정에서 사용자가 켜야 한다.
    PermissionDenied,
    /// 도구가 사용자 취소를 구별 가능한 신호(종료 상태와 취소 메시지)로 알렸다.
    Cancelled,
    /// 도구는 끝났지만 파일이 없다. 취소인지 출력 없는 실패인지 도구가 구별해 주지 않는다.
    /// 다른 선택 UI를 다시 열지 않으려고 다음 후보로 넘어가지 않는다.
    NoOutput(String),
    /// 이 플랫폼에서 지원하는 캡처 도구를 하나도 찾지 못했다.
    Unavailable(String),
    /// 저장 위치를 준비하거나 저장된 캡처 파일을 읽지 못했다.
    Storage(anyhow::Error),
    /// 도구 실행이 실패했거나 손상된 파일을 남겼다. 폴백 후보가 있었다면 모두 실패했다.
    Tool(anyhow::Error),
}

impl std::fmt::Display for CaptureError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::PermissionDenied => {
                write!(f, "screen recording permission is not granted")
            }
            Self::Cancelled => write!(f, "screen capture was cancelled by the user"),
            Self::NoOutput(detail) => {
                write!(f, "screen capture produced no file ({detail})")
            }
            Self::Unavailable(detail) => write!(f, "no screen capture tool: {detail}"),
            Self::Storage(err) => write!(f, "screen capture storage failed: {err:#}"),
            Self::Tool(err) => write!(f, "screen capture tool failed: {err:#}"),
        }
    }
}

impl std::error::Error for CaptureError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Storage(err) | Self::Tool(err) => Some(err.as_ref()),
            _ => None,
        }
    }
}

/// 인터랙티브 화면 캡처를 실행해 `~/.tasty/screenshots/screenshot-<ms>.png` 에 저장하고
/// 그 경로를 반환한다. 성공한 경로는 완전한 PNG 파일이다.
///
/// 블로킹(자식 프로세스 대기) — 반드시 백그라운드 스레드에서 호출해야 한다(메인
/// 루프를 막지 않기 위해). 호출부: `App::poll_screenshot_captures`.
pub fn capture_interactive() -> Result<PathBuf, CaptureError> {
    // 권한 조회는 캡처 **직전**에 한다 — 부팅 시점 값을 캐시해두면 그 사이 사용자가
    // 시스템 설정에서 권한을 바꾼 경우를 잘못 판정한다. 비-macOS 는 항상 true.
    if !crate::macos_permissions::screen_recording_authorized() {
        return Err(CaptureError::PermissionDenied);
    }

    let path = next_screenshot_path().map_err(CaptureError::Storage)?;

    capture_to_path(&path)?;
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

/// 도구가 남긴 파일이 끝까지 쓰인 PNG 인지 확인한다. 서명과 마지막 IEND 청크만 보므로
/// 픽셀 디코딩 오류까지 잡지는 않는다. 잘린 파일·다른 형식은 잡는다.
fn validate_png(path: &Path) -> anyhow::Result<()> {
    use std::io::{Read, Seek, SeekFrom};
    const SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    const IEND: [u8; 12] = [0, 0, 0, 0, b'I', b'E', b'N', b'D', 0xAE, 0x42, 0x60, 0x82];
    let mut file = std::fs::File::open(path)?;
    let len = file.metadata()?.len();
    if len < (SIGNATURE.len() + IEND.len()) as u64 {
        anyhow::bail!("{} is too short to be a PNG ({len} bytes)", path.display());
    }
    let mut head = [0u8; 8];
    file.read_exact(&mut head)?;
    let mut tail = [0u8; 12];
    file.seek(SeekFrom::End(-(IEND.len() as i64)))?;
    file.read_exact(&mut tail)?;
    if head != SIGNATURE {
        anyhow::bail!("{} is not a PNG file", path.display());
    }
    if tail != IEND {
        anyhow::bail!("{} is an incomplete PNG (no IEND chunk)", path.display());
    }
    Ok(())
}

/// 실패 원인으로 보여 줄 표준 오류의 마지막 부분. 메시지 길이를 제한한다.
fn stderr_tail(stderr: &[u8]) -> String {
    const LIMIT: usize = 400;
    let text = String::from_utf8_lossy(stderr);
    let text = text.trim();
    let start = text
        .char_indices()
        .rev()
        .nth(LIMIT.saturating_sub(1))
        .map_or(0, |(idx, _)| idx);
    text[start..].to_string()
}

/// 캡처 도구가 실패했을 때 남은 부분 파일을 지운다. 지우지 못해도 결과 분류는 바뀌지 않는다.
fn discard_partial(path: &Path) {
    if let Err(error) = std::fs::remove_file(path)
        && error.kind() != std::io::ErrorKind::NotFound
    {
        tracing::warn!(%error, path = %path.display(), "discarding a partial screen capture failed");
    }
}

#[cfg(target_os = "macos")]
fn capture_to_path(path: &Path) -> Result<(), CaptureError> {
    // `-i` = interactive(영역/윈도우 선택). 취소의 종료 상태가 macOS 버전마다 달라
    // 파일 없는 결과는 취소로 확정하지 않는다. 대체 도구가 없어 폴백도 없다.
    let output = match Command::new("screencapture").arg("-i").arg(path).output() {
        Ok(output) => output,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Err(CaptureError::Unavailable("screencapture not found".into()));
        }
        Err(e) => {
            return Err(CaptureError::Tool(
                anyhow::Error::new(e).context("screencapture"),
            ));
        }
    };
    if !path.exists() {
        return Err(CaptureError::NoOutput(format!(
            "screencapture exited with {}",
            output.status
        )));
    }
    if !output.status.success() {
        discard_partial(path);
        return Err(CaptureError::Tool(anyhow::anyhow!(
            "screencapture exited with {}: {}",
            output.status,
            stderr_tail(&output.stderr)
        )));
    }
    validate_png(path).map_err(|e| {
        discard_partial(path);
        CaptureError::Tool(e)
    })
}

#[cfg(target_os = "windows")]
fn capture_to_path(path: &Path) -> Result<(), CaptureError> {
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
    // 전체 화면 캡처라 선택 UI와 취소가 없다. 파일이 없거나 비정상 종료면 실패다.
    let output = match cmd.output() {
        Ok(output) => output,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Err(CaptureError::Unavailable("powershell not found".into()));
        }
        Err(e) => {
            return Err(CaptureError::Tool(
                anyhow::Error::new(e).context("powershell"),
            ));
        }
    };
    if !output.status.success() {
        discard_partial(path);
        return Err(CaptureError::Tool(anyhow::anyhow!(
            "powershell screen capture exited with {}: {}",
            output.status,
            stderr_tail(&output.stderr)
        )));
    }
    if !path.exists() {
        return Err(CaptureError::Tool(anyhow::anyhow!(
            "powershell screen capture exited successfully without writing a file"
        )));
    }
    validate_png(path).map_err(|e| {
        discard_partial(path);
        CaptureError::Tool(e)
    })
}

#[cfg(all(unix, not(target_os = "macos")))]
fn capture_to_path(path: &Path) -> Result<(), CaptureError> {
    let wayland = std::env::var_os("WAYLAND_DISPLAY").is_some();
    let search = std::env::var_os("PATH").unwrap_or_default();
    linux::run_candidates(linux::candidates(wayland), path, &search)
}

#[cfg(all(unix, not(target_os = "macos")))]
mod linux {
    //! Linux 후보 도구를 차례로 시도한다. 도구가 없거나 실패하면 다음 후보로 넘어가고,
    //! 취소나 구별할 수 없는 무출력 결과에서는 멈춘다. 다른 선택 UI를 연달아 열지 않기 위해서다.

    use super::{CaptureError, discard_partial, stderr_tail, validate_png};
    use std::ffi::OsStr;
    use std::path::{Path, PathBuf};
    use std::process::Command;

    #[derive(Clone, Copy, Debug)]
    pub(super) enum Candidate {
        GrimSlurp,
        GnomeScreenshot,
        Scrot,
        Import,
    }

    /// 한 후보를 실행한 결과.
    #[derive(Debug)]
    enum Attempt {
        /// 정상 종료하고 파일을 남겼다. 내용 검사는 호출부가 한다.
        Captured,
        Cancelled,
        NoOutput(String),
        /// 필요한 실행 파일이 없다. 선택 UI를 열지 않았다.
        Unavailable,
        Failed(anyhow::Error),
    }

    pub(super) fn candidates(wayland: bool) -> &'static [Candidate] {
        if wayland {
            &[Candidate::GrimSlurp, Candidate::GnomeScreenshot]
        } else {
            &[
                Candidate::GnomeScreenshot,
                Candidate::Scrot,
                Candidate::Import,
            ]
        }
    }

    impl Candidate {
        fn label(self) -> &'static str {
            match self {
                Self::GrimSlurp => "grim+slurp",
                Self::GnomeScreenshot => "gnome-screenshot",
                Self::Scrot => "scrot",
                Self::Import => "import",
            }
        }

        fn attempt(self, path: &Path, search: &OsStr) -> Attempt {
            let path_arg = path.as_os_str();
            match self {
                Self::GrimSlurp => grim_slurp(path, search),
                Self::GnomeScreenshot => run_tool(
                    "gnome-screenshot",
                    &[OsStr::new("-a"), OsStr::new("-f"), path_arg],
                    path,
                    search,
                    None,
                ),
                // scrot 은 Esc 취소에 종료 코드 1과 이 메시지를 낸다(scrot 1.10 실측).
                Self::Scrot => run_tool(
                    "scrot",
                    &[OsStr::new("-s"), path_arg],
                    path,
                    search,
                    Some("aborting shot"),
                ),
                Self::Import => run_tool("import", &[path_arg], path, search, None),
            }
        }
    }

    pub(super) fn run_candidates(
        candidates: &[Candidate],
        path: &Path,
        search: &OsStr,
    ) -> Result<(), CaptureError> {
        let mut failures: Vec<String> = Vec::new();
        for &candidate in candidates {
            let failure = match candidate.attempt(path, search) {
                Attempt::Captured => match validate_png(path) {
                    Ok(()) => return Ok(()),
                    Err(error) => error,
                },
                Attempt::Cancelled => return Err(CaptureError::Cancelled),
                Attempt::NoOutput(detail) => return Err(CaptureError::NoOutput(detail)),
                Attempt::Unavailable => continue,
                Attempt::Failed(error) => error,
            };
            discard_partial(path);
            tracing::debug!(
                tool = candidate.label(),
                "screen capture candidate failed: {failure:#}"
            );
            failures.push(format!("{}: {failure:#}", candidate.label()));
        }
        if failures.is_empty() {
            let names: Vec<_> = candidates.iter().map(|c| c.label()).collect();
            return Err(CaptureError::Unavailable(format!(
                "install one of {}",
                names.join(", ")
            )));
        }
        Err(CaptureError::Tool(anyhow::anyhow!(failures.join("; "))))
    }

    /// `PATH` 와 같은 형식의 `search` 에서 실행 가능한 `name` 을 찾는다.
    fn find_program(name: &str, search: &OsStr) -> Option<PathBuf> {
        use std::os::unix::fs::PermissionsExt;
        std::env::split_paths(search)
            .map(|dir| dir.join(name))
            .find(|candidate| {
                std::fs::metadata(candidate)
                    .is_ok_and(|meta| meta.is_file() && meta.permissions().mode() & 0o111 != 0)
            })
    }

    fn run_tool(
        name: &str,
        args: &[&OsStr],
        path: &Path,
        search: &OsStr,
        cancel_marker: Option<&str>,
    ) -> Attempt {
        let Some(program) = find_program(name, search) else {
            return Attempt::Unavailable;
        };
        let output = match Command::new(&program).args(args).output() {
            Ok(output) => output,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Attempt::Unavailable,
            Err(e) => {
                return Attempt::Failed(anyhow::Error::new(e).context(format!("starting {name}")));
            }
        };
        classify_exit(name, &output, path, cancel_marker)
    }

    fn classify_exit(
        name: &str,
        output: &std::process::Output,
        path: &Path,
        cancel_marker: Option<&str>,
    ) -> Attempt {
        let exists = path.exists();
        if output.status.success() {
            return if exists {
                Attempt::Captured
            } else {
                Attempt::NoOutput(format!("{name} exited successfully without writing a file"))
            };
        }
        let stderr = stderr_tail(&output.stderr);
        if !exists && cancel_marker.is_some_and(|marker| stderr.contains(marker)) {
            return Attempt::Cancelled;
        }
        Attempt::Failed(anyhow::anyhow!("exited with {}: {stderr}", output.status))
    }

    /// Wayland: `slurp` 로 영역을 고르고 `grim -g <geometry>` 로 캡처한다. 둘 중 하나라도 없으면
    /// slurp 의 선택 UI를 열지 않고 다음 후보로 넘어간다.
    fn grim_slurp(path: &Path, search: &OsStr) -> Attempt {
        let (Some(slurp), Some(_grim)) =
            (find_program("slurp", search), find_program("grim", search))
        else {
            return Attempt::Unavailable;
        };
        let selection = match Command::new(&slurp).output() {
            Ok(output) => output,
            Err(e) => return Attempt::Failed(anyhow::Error::new(e).context("starting slurp")),
        };
        if !selection.status.success() {
            let stderr = stderr_tail(&selection.stderr);
            // slurp 은 Esc 취소에 종료 코드 1과 "selection cancelled" 를 낸다(소스 확인).
            if stderr.contains("selection cancelled") {
                return Attempt::Cancelled;
            }
            return Attempt::Failed(anyhow::anyhow!(
                "slurp exited with {}: {stderr}",
                selection.status
            ));
        }
        let geometry = String::from_utf8_lossy(&selection.stdout)
            .trim()
            .to_string();
        if geometry.is_empty() {
            return Attempt::NoOutput("slurp returned no selection".into());
        }
        let geometry = OsStr::new(&geometry);
        match run_tool(
            "grim",
            &[OsStr::new("-g"), geometry, path.as_os_str()],
            path,
            search,
            None,
        ) {
            Attempt::Failed(error) => Attempt::Failed(error.context("grim")),
            other => other,
        }
    }
}

#[cfg(all(test, unix, not(target_os = "macos")))]
mod linux_tests;

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

    #[test]
    fn validate_png_accepts_complete_and_rejects_truncated_or_foreign_files() {
        let dir = tempfile::tempdir().expect("tempdir");
        let good = dir.path().join("good.png");
        std::fs::write(&good, png_fixture()).expect("write");
        validate_png(&good).expect("complete PNG");

        let truncated = dir.path().join("truncated.png");
        let bytes = png_fixture();
        std::fs::write(&truncated, &bytes[..bytes.len() - 4]).expect("write");
        assert!(validate_png(&truncated).is_err());

        let foreign = dir.path().join("foreign.png");
        std::fs::write(&foreign, b"this is definitely not a png file").expect("write");
        assert!(validate_png(&foreign).is_err());

        assert!(validate_png(&dir.path().join("missing.png")).is_err());
    }

    #[test]
    fn stderr_tail_keeps_the_last_characters() {
        let long = "가".repeat(500) + "end";
        let tail = stderr_tail(long.as_bytes());
        assert!(tail.ends_with("end"));
        assert_eq!(tail.chars().count(), 400);
        assert_eq!(stderr_tail(b"  short \n"), "short");
    }

    /// 서명과 IEND 를 가진 최소 PNG 형태의 바이트. validate_png 는 이 두 부분만 본다.
    pub(crate) fn png_fixture() -> Vec<u8> {
        let mut bytes = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
        bytes.extend_from_slice(b"body");
        bytes.extend_from_slice(&[0, 0, 0, 0, b'I', b'E', b'N', b'D', 0xAE, 0x42, 0x60, 0x82]);
        bytes
    }
}
