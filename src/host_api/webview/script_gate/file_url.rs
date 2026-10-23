//! Local URL decoding stays separate from received-byte responses.
use std::path::PathBuf;

/// `file://` URL을 로컬 경로로 바꾼다. query·fragment는 버리고 percent escape를 바이트로 복원한다.
pub fn file_url_to_path(url: &str) -> Option<PathBuf> {
    let rest = url
        .get(..7)
        .filter(|s| s.eq_ignore_ascii_case("file://"))
        .map(|_| &url[7..])?;
    let rest = rest.split(['?', '#']).next().unwrap_or("");
    let (host, path) = match rest.find('/') {
        Some(i) => (&rest[..i], &rest[i..]),
        None => (rest, ""),
    };
    let bytes = percent_decode(path)?;
    let host = if host.eq_ignore_ascii_case("localhost") {
        ""
    } else {
        host
    };
    path_from_parts(host, bytes)
}

fn percent_decode(s: &str) -> Option<Vec<u8>> {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' {
            let hex = s.get(i + 1..i + 3)?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    Some(out)
}

#[cfg(unix)]
fn path_from_parts(host: &str, bytes: Vec<u8>) -> Option<PathBuf> {
    use std::os::unix::ffi::OsStringExt;
    // 다른 호스트의 파일은 로컬 경로로 읽지 않는다.
    if !host.is_empty() || bytes.first() != Some(&b'/') {
        return None;
    }
    Some(PathBuf::from(std::ffi::OsString::from_vec(bytes)))
}

#[cfg(windows)]
fn path_from_parts(host: &str, bytes: Vec<u8>) -> Option<PathBuf> {
    let path = String::from_utf8(bytes).ok()?;
    if !host.is_empty() {
        // UNC: file://server/share/a.html -> \\server\share\a.html
        return Some(PathBuf::from(format!(
            r"\\{host}{}",
            path.replace('/', "\\")
        )));
    }
    // file:///C:/a.html -> C:\a.html
    let trimmed = path.strip_prefix('/').unwrap_or(&path);
    Some(PathBuf::from(trimmed.replace('/', "\\")))
}
