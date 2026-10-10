//! OS 파일 클립보드 형식의 인코딩과 해석. 플랫폼 호출 없이 바이트만 다루므로 모든 OS 에서 시험한다.
//!
//! - Linux: `text/uri-list`(RFC 2483, 줄 끝 CRLF), GNOME `x-special/gnome-copied-files`(첫 줄 `copy`·`cut`,
//!   뒤 줄마다 URI), KDE `application/x-kde-cutselection`(`1` 이면 잘라내기).
//! - Windows: `CF_HDROP` 의 `DROPFILES` 구조와 `Preferred DropEffect` DWORD.
//! - macOS: pasteboard 항목마다의 URL 문자열.

#[cfg(any(target_os = "linux", all(test, unix)))]
use std::path::Path;
use std::path::PathBuf;

#[cfg(any(target_os = "linux", target_os = "macos", all(test, unix)))]
use super::{OsClip, OsFiles};

/// `Preferred DropEffect` 값. 복사는 `DROPEFFECT_COPY`, 잘라내기는 `DROPEFFECT_MOVE` 다.
#[cfg(any(windows, test))]
pub(crate) const DROPEFFECT_COPY: u32 = 1;
#[cfg(any(windows, test))]
pub(crate) const DROPEFFECT_MOVE: u32 = 2;

/// 로컬 절대 경로를 `file://` URI 로 바꾼다. RFC 3986 의 비예약 문자와 `/` 만 그대로 둔다.
#[cfg(any(target_os = "linux", all(test, unix)))]
pub(crate) fn file_uri(path: &Path) -> String {
    use std::os::unix::ffi::OsStrExt;
    let mut out = String::from("file://");
    for &b in path.as_os_str().as_bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'/' | b'-' | b'.' | b'_' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// `text/uri-list` 본문. 줄 끝은 RFC 2483 대로 CRLF 다.
#[cfg(any(target_os = "linux", all(test, unix)))]
pub(crate) fn uri_list(paths: &[PathBuf]) -> String {
    paths
        .iter()
        .map(|p| file_uri(p) + "\r\n")
        .collect::<String>()
}

/// GNOME `x-special/gnome-copied-files` 본문. 첫 줄이 동작이고 줄 끝은 LF 다.
#[cfg(any(target_os = "linux", all(test, unix)))]
pub(crate) fn gnome_copied_files(files: &OsFiles) -> String {
    let mut out = String::from(if files.cut { "cut" } else { "copy" });
    for p in &files.paths {
        out.push('\n');
        out.push_str(&file_uri(p));
    }
    out
}

/// URI 줄들을 로컬 경로로 읽는다. 주석(`#`)·빈 줄은 건너뛴다. 로컬 파일이 아닌 URI(다른 호스트,
/// `sftp://` 등)가 하나라도 있으면 [`OsClip::NotLocal`] 이다 — 일부만 붙여넣지 않는다.
#[cfg(any(target_os = "linux", target_os = "macos", all(test, unix)))]
fn uri_lines<'a>(lines: impl Iterator<Item = &'a str>, cut: bool) -> OsClip {
    let mut paths = Vec::new();
    for line in lines {
        let line = line.trim_end_matches('\r').trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        match crate::host_api::webview::script_gate::file_url_to_path(line) {
            Some(path) => paths.push(path),
            None => return OsClip::NotLocal,
        }
    }
    if paths.is_empty() {
        OsClip::Other
    } else {
        OsClip::Files(OsFiles { paths, cut })
    }
}

/// `text/uri-list` 를 읽는다. 잘라내기 여부는 KDE 표지가 정한다.
#[cfg(any(target_os = "linux", all(test, unix)))]
pub(crate) fn parse_uri_list(text: &str, kde_cut: Option<&[u8]>) -> OsClip {
    let cut = kde_cut.is_some_and(|v| v.trim_ascii() == b"1");
    uri_lines(text.lines(), cut)
}

/// GNOME `x-special/gnome-copied-files` 를 읽는다. 첫 줄이 `copy`·`cut` 이 아니면 파일 목록이 아니다.
#[cfg(any(target_os = "linux", all(test, unix)))]
pub(crate) fn parse_gnome_copied_files(text: &str) -> OsClip {
    let mut lines = text.lines();
    let cut = match lines.next().map(|l| l.trim()) {
        Some("cut") => true,
        Some("copy") => false,
        _ => return OsClip::Other,
    };
    uri_lines(lines, cut)
}

/// macOS pasteboard 항목들의 URL. 파일 URL 이 아닌 항목(웹 주소 등)이 하나라도 있으면
/// [`OsClip::NotLocal`] 이다. pasteboard 에는 잘라내기 표지가 없어 늘 복사다.
#[cfg(any(target_os = "macos", all(test, unix)))]
pub(crate) fn parse_pasteboard_urls(urls: &[String]) -> OsClip {
    uri_lines(urls.iter().map(String::as_str), false)
}

/// `DROPFILES` 머리 크기. pFiles(u32) · pt(i32 두 개) · fNC(i32) · fWide(i32).
#[cfg(any(windows, test))]
const DROPFILES_HEADER: usize = 20;

/// `CF_HDROP` 메모리 내용. 머리 뒤에 UTF-16 경로를 NUL 로 나누고 NUL 하나를 더 붙인다.
#[cfg(any(windows, test))]
pub(crate) fn dropfiles(paths: &[PathBuf]) -> Vec<u8> {
    let mut out = Vec::with_capacity(DROPFILES_HEADER + paths.len() * 64);
    out.extend_from_slice(&(DROPFILES_HEADER as u32).to_le_bytes());
    out.extend_from_slice(&0i32.to_le_bytes());
    out.extend_from_slice(&0i32.to_le_bytes());
    out.extend_from_slice(&0i32.to_le_bytes());
    out.extend_from_slice(&1i32.to_le_bytes());
    for p in paths {
        for unit in p.to_string_lossy().encode_utf16() {
            out.extend_from_slice(&unit.to_le_bytes());
        }
        out.extend_from_slice(&0u16.to_le_bytes());
    }
    out.extend_from_slice(&0u16.to_le_bytes());
    out
}

/// `CF_HDROP` 메모리 내용을 경로 목록으로 읽는다. 구조가 맞지 않으면 None.
#[cfg(any(windows, test))]
pub(crate) fn parse_dropfiles(bytes: &[u8]) -> Option<Vec<PathBuf>> {
    let word = |at: usize| -> Option<u32> {
        Some(u32::from_le_bytes(bytes.get(at..at + 4)?.try_into().ok()?))
    };
    let start = word(0)? as usize;
    let wide = word(16)? != 0;
    let body = bytes.get(start..)?;
    let names: Vec<String> = if wide {
        let units: Vec<u16> = body
            .chunks_exact(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .collect();
        units
            .split(|&u| u == 0)
            .take_while(|s| !s.is_empty())
            .map(String::from_utf16_lossy)
            .collect()
    } else {
        body.split(|&b| b == 0)
            .take_while(|s| !s.is_empty())
            .map(|s| String::from_utf8_lossy(s).into_owned())
            .collect()
    };
    Some(names.into_iter().map(PathBuf::from).collect())
}

/// `Preferred DropEffect` 가 잘라내기인가. 복사 비트가 함께 있으면 복사로 본다.
#[cfg(any(windows, test))]
pub(crate) fn drop_effect_is_cut(effect: u32) -> bool {
    effect & DROPEFFECT_MOVE != 0 && effect & DROPEFFECT_COPY == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    fn files(paths: &[&str], cut: bool) -> OsFiles {
        OsFiles {
            paths: paths.iter().map(PathBuf::from).collect(),
            cut,
        }
    }

    #[cfg(unix)]
    #[test]
    fn a_path_with_spaces_and_hangul_round_trips_through_a_uri() {
        let path = PathBuf::from("/home/u/My Doc 한.txt");
        let uri = file_uri(&path);
        assert_eq!(uri, "file:///home/u/My%20Doc%20%ED%95%9C.txt");
        assert_eq!(
            parse_uri_list(&uri_list(std::slice::from_ref(&path)), None),
            OsClip::Files(files(&["/home/u/My Doc 한.txt"], false))
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_web_url_on_the_pasteboard_refuses_the_whole_list() {
        let file = "file:///Users/u/a%20b.txt".to_string();
        assert_eq!(
            parse_pasteboard_urls(std::slice::from_ref(&file)),
            OsClip::Files(files(&["/Users/u/a b.txt"], false))
        );
        assert_eq!(
            parse_pasteboard_urls(&[file, "https://example.com/x".into()]),
            OsClip::NotLocal
        );
        assert_eq!(parse_pasteboard_urls(&[]), OsClip::Other);
    }

    #[cfg(unix)]
    #[test]
    fn the_gnome_list_carries_cut_and_copy() {
        for cut in [false, true] {
            let f = files(&["/a/b", "/a/c d"], cut);
            assert_eq!(
                parse_gnome_copied_files(&gnome_copied_files(&f)),
                OsClip::Files(f)
            );
        }
        // Nautilus 는 끝에 줄바꿈을 붙이지 않고 다른 앱은 붙인다. 둘 다 같다.
        assert_eq!(
            parse_gnome_copied_files("cut\nfile:///a/b\n"),
            OsClip::Files(files(&["/a/b"], true))
        );
        assert_eq!(parse_gnome_copied_files("hello\n/a/b"), OsClip::Other);
    }

    #[cfg(unix)]
    #[test]
    fn the_kde_mark_makes_a_uri_list_a_cut() {
        let text = "file:///a/b\r\n";
        assert_eq!(
            parse_uri_list(text, Some(b"1")),
            OsClip::Files(files(&["/a/b"], true))
        );
        assert_eq!(
            parse_uri_list(text, Some(b"0")),
            OsClip::Files(files(&["/a/b"], false))
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_remote_uri_makes_the_whole_list_not_local() {
        // 다른 호스트·sftp 의 경로를 같은 문자열의 로컬 파일로 읽지 않는다.
        assert_eq!(
            parse_uri_list("file:///a/b\r\nsftp://host/a/b\r\n", None),
            OsClip::NotLocal
        );
        assert_eq!(
            parse_gnome_copied_files("copy\nfile://server/share/a"),
            OsClip::NotLocal
        );
        assert_eq!(parse_uri_list("# comment\r\n\r\n", None), OsClip::Other);
    }

    #[test]
    fn dropfiles_round_trip_and_the_drop_effect_names_a_cut() {
        let paths = vec![PathBuf::from(r"C:\a b\c.txt"), PathBuf::from(r"D:\한")];
        let bytes = dropfiles(&paths);
        assert_eq!(&bytes[..4], &20u32.to_le_bytes());
        assert_eq!(parse_dropfiles(&bytes), Some(paths));
        assert_eq!(parse_dropfiles(&[1, 2]), None);
        assert!(drop_effect_is_cut(DROPEFFECT_MOVE));
        assert!(!drop_effect_is_cut(DROPEFFECT_COPY));
        // Explorer 의 복사는 COPY|LINK(5)다.
        assert!(!drop_effect_is_cut(5));
        assert!(!drop_effect_is_cut(DROPEFFECT_COPY | DROPEFFECT_MOVE));
    }
}
