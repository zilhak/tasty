//! OS 파일 관리자/기본 앱으로 경로 열기 (explorer 컨텍스트 메뉴 "Open in system")와
//! 파일 관리자에서 항목 보이기 ("Show in file manager").
//!
//! 폴더면 OS 파일 관리자에서 그 폴더를 연다. 크로스 플랫폼: Windows `explorer`,
//! macOS `open`, Linux `xdg-open`. 입력 검증/식별은 호출부 책임 — 여기선 launch 만.

use std::path::Path;
use std::process::Command;

/// `path` 를 OS 기본 핸들러로 연다 (폴더 → 파일 관리자). 실패는 Err 로 반환.
pub fn open_path(path: &Path) -> std::io::Result<()> {
    #[cfg(debug_assertions)]
    if crate::debug_os_open::intercepted("open_path", &path.to_string_lossy()) {
        return Ok(());
    }
    #[cfg(target_os = "windows")]
    {
        Command::new("explorer").arg(path).spawn()?;
        Ok(())
    }
    #[cfg(target_os = "macos")]
    {
        Command::new("open").arg(path).spawn()?;
        Ok(())
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        Command::new("xdg-open").arg(path).spawn()?;
        Ok(())
    }
    #[cfg(not(any(windows, target_os = "macos", all(unix, not(target_os = "macos")))))]
    {
        // 지원하지 않는 플랫폼 — 열 수단이 없어 path 를 쓸 곳이 없다.
        let _ = path;
        Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "open_path: unsupported platform",
        ))
    }
}

/// `path` 를 담은 폴더를 OS 파일 관리자로 열고 그 항목을 고른다. 실패는 Err 로 반환.
///
/// - Windows: `explorer /select,"<path>"`.
/// - macOS: `open -R <path>`(Finder).
/// - Linux 등: freedesktop `org.freedesktop.FileManager1.ShowItems` 를 `dbus-send` 로 부른다.
///   그 서비스가 없거나 `dbus-send` 가 없으면 고르기 없이 부모 폴더를 `xdg-open` 으로 연다.
pub fn reveal_path(path: &Path) -> std::io::Result<()> {
    #[cfg(debug_assertions)]
    if crate::debug_os_open::intercepted("reveal_path", &path.to_string_lossy()) {
        return Ok(());
    }
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        // explorer 는 `/select,` 뒤를 자체 규칙으로 읽어 일반 인자 따옴표 처리를 쓰지 않는다.
        Command::new("explorer")
            .raw_arg(format!("/select,\"{}\"", path.display()))
            .spawn()?;
        Ok(())
    }
    #[cfg(target_os = "macos")]
    {
        Command::new("open").arg("-R").arg(path).spawn()?;
        Ok(())
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let shown = Command::new("dbus-send")
            .args([
                "--session",
                "--print-reply",
                "--reply-timeout=2000",
                "--dest=org.freedesktop.FileManager1",
                "--type=method_call",
                "/org/freedesktop/FileManager1",
                "org.freedesktop.FileManager1.ShowItems",
            ])
            .arg(format!("array:string:{}", file_uri(path)))
            .arg("string:")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
        match shown {
            Ok(status) if status.success() => Ok(()),
            other => {
                tracing::info!(
                    ?other,
                    "FileManager1.ShowItems unavailable; opening the parent folder"
                );
                let parent = path.parent().unwrap_or(path);
                Command::new("xdg-open").arg(parent).spawn()?;
                Ok(())
            }
        }
    }
    #[cfg(not(any(windows, target_os = "macos", all(unix, not(target_os = "macos")))))]
    {
        // 지원하지 않는 플랫폼 — 열 수단이 없어 path 를 쓸 곳이 없다.
        let _ = path;
        Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "reveal_path: unsupported platform",
        ))
    }
}

/// 절대 경로를 `file://` URI 로 만든다. 경로 바이트를 그대로 쓰며 RFC 3986 unreserved 와 `/` 밖은
/// 퍼센트 인코딩한다(쉼표는 dbus-send 의 배열 구분자라 반드시 인코딩해야 한다).
#[cfg(all(unix, not(target_os = "macos")))]
fn file_uri(path: &Path) -> String {
    use std::os::unix::ffi::OsStrExt;
    let mut uri = String::from("file://");
    for &b in path.as_os_str().as_bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'/' | b'-' | b'.' | b'_' | b'~') {
            uri.push(char::from(b));
        } else {
            uri.push_str(&format!("%{b:02X}"));
        }
    }
    uri
}

#[cfg(all(test, unix, not(target_os = "macos")))]
mod tests {
    use super::*;

    #[test]
    fn file_uri_keeps_raw_bytes_and_escapes_separators() {
        use std::os::unix::ffi::OsStrExt;
        assert_eq!(
            file_uri(Path::new("/w/a b,c%/가.txt")),
            "file:///w/a%20b%2Cc%25/%EA%B0%80.txt"
        );
        // 분해형(NFD) 이름도 합성하지 않고 원래 바이트로 보낸다.
        let nfd = std::ffi::OsStr::from_bytes("/e\u{301}".as_bytes());
        assert_eq!(file_uri(Path::new(nfd)), "file:///e%CC%81");
    }
}
