//! 문서 원본을 한 번 읽어 스크립트 감지와 파일 전체 지문을 함께 구한다.
//!
//! 감지는 상한까지만 하고 지문은 파일 끝까지 구한다(ADR-0053). 상한 뒤에만 있는 스크립트는
//! 감지하지 못하며, 이때는 JS가 꺼진 채 배너가 뜨지 않는다.

use std::io::Read;

use sha2::{Digest, Sha256};

use super::{Fingerprint, ScriptDetection, ScriptScan};

/// 감지 스캔의 상한. 지문에는 적용하지 않는다.
pub const SCAN_LIMIT_BYTES: usize = 4 * 1024 * 1024;

const READ_CHUNK: usize = 64 * 1024;

/// `reader`를 끝까지 읽어 지문을 구하고, 앞의 `limit` 바이트로 스크립트를 감지한다.
pub fn scan_reader(mut reader: impl Read, limit: usize) -> std::io::Result<ScriptScan> {
    let mut hasher = Sha256::new();
    let mut head = Vec::new();
    let mut buf = vec![0u8; READ_CHUNK];
    loop {
        let n = match reader.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => n,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        };
        let chunk = &buf[..n];
        hasher.update(chunk);
        if head.len() < limit {
            let take = (limit - head.len()).min(n);
            head.extend_from_slice(&chunk[..take]);
        }
    }
    Ok(ScriptScan {
        fingerprint: Fingerprint(hasher.finalize().into()),
        detection: detect_scripts(&head),
    })
}

/// 정규 파일을 열어 [`scan_reader`]로 스캔한다.
///
/// FIFO·장치 파일은 읽기가 끝나지 않거나 쓰는 쪽을 기다리며 막힐 수 있어 거절한다.
/// 경로를 연 뒤 같은 핸들로 종류를 확인하므로 확인과 읽기 사이에 파일이 바뀌지 않는다.
pub fn scan_file(path: &std::path::Path) -> std::io::Result<ScriptScan> {
    let file = open_without_blocking(path)?;
    if !file.metadata()?.is_file() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "not a regular file",
        ));
    }
    scan_reader(file, SCAN_LIMIT_BYTES)
}

/// FIFO를 열 때 쓰는 쪽을 기다리지 않도록 unix에서는 `O_NONBLOCK`으로 연다.
/// 정규 파일 읽기에는 영향이 없다.
#[cfg(unix)]
fn open_without_blocking(path: &std::path::Path) -> std::io::Result<std::fs::File> {
    use std::os::unix::fs::OpenOptionsExt;
    std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NONBLOCK)
        .open(path)
}

#[cfg(not(unix))]
fn open_without_blocking(path: &std::path::Path) -> std::io::Result<std::fs::File> {
    std::fs::File::open(path)
}

/// HTML 원본에서 실행 가능한 스크립트를 찾는다. 주석과 원문 텍스트 요소의 내용은 보지 않는다.
pub fn detect_scripts(src: &[u8]) -> ScriptDetection {
    let mut remote_only = false;
    let mut i = 0;
    while i < src.len() {
        if src[i] != b'<' {
            i += 1;
            continue;
        }
        if src[i..].starts_with(b"<!--") {
            i = find(src, i + 4, b"-->").map_or(src.len(), |e| e + 3);
            continue;
        }
        let Some(tag) = parse_tag(src, i) else {
            i += 1;
            continue;
        };
        i = tag.end;
        if tag.closing {
            continue;
        }
        if tag.attrs.iter().any(|a| is_event_handler(&a.name)) {
            return ScriptDetection::Scripts;
        }
        if tag
            .attrs
            .iter()
            .any(|a| is_url_attribute(&a.name) && a.value.as_deref().is_some_and(is_javascript_url))
        {
            return ScriptDetection::Scripts;
        }
        if tag.name == "script" {
            if is_executable_script(&tag.attrs) {
                if script_src(&tag.attrs).is_some_and(is_remote_url) {
                    remote_only = true;
                } else {
                    return ScriptDetection::Scripts;
                }
            }
            i = skip_raw_text(src, i, b"script");
        } else if let Some(name) = RAW_TEXT_ELEMENTS.iter().find(|n| **n == tag.name) {
            i = skip_raw_text(src, i, name.as_bytes());
        }
    }
    if remote_only {
        ScriptDetection::ScriptsRemoteOnly
    } else {
        ScriptDetection::None
    }
}

/// 내용을 태그로 해석하지 않는 요소. `script`는 따로 처리한다.
const RAW_TEXT_ELEMENTS: &[&str] = &["style", "textarea", "title", "xmp", "noembed", "noframes"];

/// HTML 명세가 JavaScript로 해석하는 MIME 형식.
const JAVASCRIPT_MIME_TYPES: &[&str] = &[
    "application/ecmascript",
    "application/javascript",
    "application/x-ecmascript",
    "application/x-javascript",
    "text/ecmascript",
    "text/javascript",
    "text/javascript1.0",
    "text/javascript1.1",
    "text/javascript1.2",
    "text/javascript1.3",
    "text/javascript1.4",
    "text/javascript1.5",
    "text/jscript",
    "text/livescript",
    "text/x-ecmascript",
    "text/x-javascript",
];

const URL_ATTRIBUTES: &[&str] = &["href", "src", "action", "formaction", "xlink:href"];

struct Attr {
    name: String,
    value: Option<String>,
}

struct Tag {
    name: String,
    closing: bool,
    attrs: Vec<Attr>,
    /// `>` 다음 위치. 태그가 닫히지 않았으면 원본 끝이다.
    end: usize,
}

fn find(src: &[u8], from: usize, needle: &[u8]) -> Option<usize> {
    if from >= src.len() {
        return None;
    }
    src[from..]
        .windows(needle.len())
        .position(|w| w == needle)
        .map(|p| p + from)
}

fn is_name_byte(b: u8) -> bool {
    !(b.is_ascii_whitespace() || b == b'/' || b == b'>' || b == b'=')
}

/// `start`의 `<`에서 태그 하나를 읽는다. 태그 이름이 영문자로 시작하지 않으면 `None`.
fn parse_tag(src: &[u8], start: usize) -> Option<Tag> {
    let mut i = start + 1;
    let closing = src.get(i) == Some(&b'/');
    if closing {
        i += 1;
    }
    if !src.get(i).is_some_and(|b| b.is_ascii_alphabetic()) {
        return None;
    }
    let name_start = i;
    while i < src.len() && is_name_byte(src[i]) {
        i += 1;
    }
    let name = String::from_utf8_lossy(&src[name_start..i]).to_ascii_lowercase();
    let mut attrs = Vec::new();
    loop {
        while i < src.len() && (src[i].is_ascii_whitespace() || src[i] == b'/') {
            i += 1;
        }
        if i >= src.len() {
            break;
        }
        if src[i] == b'>' {
            i += 1;
            break;
        }
        let attr_start = i;
        // `=`로 시작하는 이상한 이름도 한 글자로 소비해 진행을 보장한다.
        i += 1;
        while i < src.len() && is_name_byte(src[i]) {
            i += 1;
        }
        let attr_name = String::from_utf8_lossy(&src[attr_start..i]).to_ascii_lowercase();
        let mut j = i;
        while j < src.len() && src[j].is_ascii_whitespace() {
            j += 1;
        }
        let mut value = None;
        if src.get(j) == Some(&b'=') {
            j += 1;
            while j < src.len() && src[j].is_ascii_whitespace() {
                j += 1;
            }
            let (v, next) = read_attr_value(src, j);
            value = Some(v);
            i = next;
        }
        attrs.push(Attr {
            name: attr_name,
            value,
        });
    }
    Some(Tag {
        name,
        closing,
        attrs,
        end: i,
    })
}

fn read_attr_value(src: &[u8], start: usize) -> (String, usize) {
    match src.get(start) {
        Some(&q) if q == b'"' || q == b'\'' => {
            let end = src[start + 1..]
                .iter()
                .position(|&b| b == q)
                .map_or(src.len(), |p| start + 1 + p);
            let value = String::from_utf8_lossy(&src[start + 1..end]).into_owned();
            (value, (end + 1).min(src.len()))
        }
        _ => {
            let mut end = start;
            while end < src.len() && !src[end].is_ascii_whitespace() && src[end] != b'>' {
                end += 1;
            }
            (String::from_utf8_lossy(&src[start..end]).into_owned(), end)
        }
    }
}

/// `</name`이 나올 때까지 건너뛴다. 닫는 태그가 없으면 원본 끝까지다.
fn skip_raw_text(src: &[u8], from: usize, name: &[u8]) -> usize {
    let mut i = from;
    while let Some(lt) = find(src, i, b"</") {
        let after = lt + 2;
        let candidate = src.get(after..after + name.len());
        let boundary = src.get(after + name.len()).copied();
        if candidate.is_some_and(|c| c.eq_ignore_ascii_case(name))
            && boundary.is_none_or(|b| !is_name_byte(b))
        {
            return lt;
        }
        i = after;
    }
    src.len()
}

fn is_event_handler(name: &str) -> bool {
    name.len() > 2 && name.starts_with("on")
}

fn is_url_attribute(name: &str) -> bool {
    URL_ATTRIBUTES.contains(&name)
}

/// 브라우저처럼 앞뒤 공백·제어 문자와 중간의 탭·줄바꿈을 무시하고 스킴을 비교한다.
fn is_javascript_url(value: &str) -> bool {
    let cleaned: String = value
        .trim_matches(|c: char| c <= ' ')
        .chars()
        .filter(|c| !matches!(c, '\t' | '\n' | '\r'))
        .take("javascript:".len())
        .collect();
    cleaned.eq_ignore_ascii_case("javascript:")
}

fn is_executable_script(attrs: &[Attr]) -> bool {
    let Some(ty) = attrs.iter().find(|a| a.name == "type") else {
        return true;
    };
    let ty = ty
        .value
        .as_deref()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    if ty.is_empty() || ty == "module" {
        return true;
    }
    // 명세상 매개변수가 붙은 형식은 실행되지 않지만, 놓치는 쪽보다 배너가 뜨는 쪽을 택해 매개변수를 뺀다.
    let essence = ty.split(';').next().unwrap_or("").trim();
    JAVASCRIPT_MIME_TYPES.contains(&essence)
}

fn script_src(attrs: &[Attr]) -> Option<&str> {
    attrs
        .iter()
        .find(|a| a.name == "src" || a.name == "href" || a.name == "xlink:href")
        .and_then(|a| a.value.as_deref())
}

/// `file://` 문서에서 `//host` 형태는 `file://host`로 해석되므로 원격으로 보지 않는다.
fn is_remote_url(url: &str) -> bool {
    let url = url.trim().to_ascii_lowercase();
    url.starts_with("http://") || url.starts_with("https://")
}

#[cfg(test)]
mod tests;
