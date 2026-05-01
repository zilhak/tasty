//! Linux file clipboard via wl-clipboard (Wayland) or xclip (X11).
//!
//! Uses `x-special/gnome-copied-files` MIME type which Nautilus / Thunar /
//! Dolphin / Nemo / Caja all read. Falls back to `text/uri-list` for
//! non-GNOME-aware sources (Copy/Cut distinction is lost in that case).

use std::io::Write;
use std::process::{Command, Stdio};

use super::FileClipboardOp;

const MIME_GNOME: &str = "x-special/gnome-copied-files";
const MIME_URI_LIST: &str = "text/uri-list";

fn is_wayland() -> bool {
    std::env::var_os("WAYLAND_DISPLAY").is_some()
}

fn path_to_uri(path: &str) -> String {
    format!("file://{}", percent_encode(path))
}

fn build_gnome_payload(paths: &[&str], op: FileClipboardOp) -> String {
    let header = match op {
        FileClipboardOp::Copy => "copy",
        FileClipboardOp::Cut => "cut",
    };
    let mut s = String::from(header);
    for p in paths {
        s.push('\n');
        s.push_str(&path_to_uri(p));
    }
    s
}

pub fn set_file_clipboard(paths: &[&str], op: FileClipboardOp) -> Result<(), String> {
    if paths.is_empty() {
        return Err("paths is empty".into());
    }
    let payload = build_gnome_payload(paths, op);
    let (cmd, args) = if is_wayland() {
        ("wl-copy", vec!["--type", MIME_GNOME])
    } else {
        ("xclip", vec!["-selection", "clipboard", "-t", MIME_GNOME])
    };
    write_to_stdin(cmd, &args, &payload).map_err(|e| {
        tracing::warn!("file clipboard set failed via {}: {}", cmd, e);
        format!("{} 실행 실패: {} (wl-clipboard 또는 xclip 설치 필요)", cmd, e)
    })
}

pub fn get_file_clipboard() -> Result<Option<(Vec<String>, FileClipboardOp)>, String> {
    if let Some(text) = read_clipboard(MIME_GNOME) {
        if let Some(parsed) = parse_gnome_payload(&text) {
            return Ok(Some(parsed));
        }
    }
    if let Some(text) = read_clipboard(MIME_URI_LIST) {
        let paths = parse_uri_list(&text);
        if !paths.is_empty() {
            return Ok(Some((paths, FileClipboardOp::Copy)));
        }
    }
    Ok(None)
}

fn write_to_stdin(cmd: &str, args: &[&str], data: &str) -> Result<(), String> {
    let mut child = Command::new(cmd)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("spawn: {e}"))?;

    if let Some(mut stdin) = child.stdin.take() {
        stdin
            .write_all(data.as_bytes())
            .map_err(|e| format!("write stdin: {e}"))?;
    }

    let status = child.wait().map_err(|e| format!("wait: {e}"))?;
    if !status.success() {
        return Err(format!("exit status {status}"));
    }
    Ok(())
}

fn read_clipboard(mime: &str) -> Option<String> {
    let (cmd, args) = if is_wayland() {
        ("wl-paste", vec!["--type", mime])
    } else {
        ("xclip", vec!["-selection", "clipboard", "-t", mime, "-o"])
    };
    let output = Command::new(cmd)
        .args(&args)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    if output.status.success() {
        Some(String::from_utf8_lossy(&output.stdout).into_owned())
    } else {
        None
    }
}

fn parse_gnome_payload(s: &str) -> Option<(Vec<String>, FileClipboardOp)> {
    let mut lines = s.lines();
    let header = lines.next()?.trim();
    let op = match header {
        "copy" => FileClipboardOp::Copy,
        "cut" => FileClipboardOp::Cut,
        _ => return None,
    };
    let paths: Vec<String> = lines
        .filter(|l| !l.is_empty())
        .filter_map(|l| l.strip_prefix("file://").map(percent_decode))
        .collect();
    if paths.is_empty() {
        None
    } else {
        Some((paths, op))
    }
}

fn parse_uri_list(s: &str) -> Vec<String> {
    s.lines()
        .map(|l| l.trim_end_matches('\r'))
        .filter(|l| !l.starts_with('#') && !l.is_empty())
        .filter_map(|l| l.strip_prefix("file://").map(percent_decode))
        .collect()
}

/// Percent-encode bytes that aren't safe inside a `file://` URI path.
fn percent_encode(path: &str) -> String {
    let mut out = String::with_capacity(path.len());
    for &b in path.as_bytes() {
        match b {
            b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' | b'/' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{:02X}", b)),
        }
    }
    out
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let (Some(h), Some(l)) = (hex_val(bytes[i + 1]), hex_val(bytes[i + 2])) {
                out.push((h << 4) | l);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn hex_val(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_special_chars() {
        assert_eq!(percent_encode("/tmp/Hello World.txt"), "/tmp/Hello%20World.txt");
        assert_eq!(percent_encode("/tmp/한글.txt"), "/tmp/%ED%95%9C%EA%B8%80.txt");
    }

    #[test]
    fn decode_roundtrip() {
        let original = "/tmp/Hello World.txt";
        let encoded = percent_encode(original);
        assert_eq!(percent_decode(&encoded), original);
        let kor = "/tmp/한글.txt";
        assert_eq!(percent_decode(&percent_encode(kor)), kor);
    }

    #[test]
    fn build_gnome_copy() {
        let payload = build_gnome_payload(&["/tmp/a.txt", "/tmp/b.txt"], FileClipboardOp::Copy);
        assert_eq!(payload, "copy\nfile:///tmp/a.txt\nfile:///tmp/b.txt");
    }

    #[test]
    fn build_gnome_cut() {
        let payload = build_gnome_payload(&["/tmp/a.txt"], FileClipboardOp::Cut);
        assert_eq!(payload, "cut\nfile:///tmp/a.txt");
    }

    #[test]
    fn parse_gnome_copy() {
        let s = "copy\nfile:///tmp/a.txt\nfile:///tmp/b.txt";
        let (paths, op) = parse_gnome_payload(s).unwrap();
        assert_eq!(op, FileClipboardOp::Copy);
        assert_eq!(paths, vec!["/tmp/a.txt", "/tmp/b.txt"]);
    }

    #[test]
    fn parse_gnome_cut_with_encoded() {
        let s = "cut\nfile:///tmp/Hello%20World.txt";
        let (paths, op) = parse_gnome_payload(s).unwrap();
        assert_eq!(op, FileClipboardOp::Cut);
        assert_eq!(paths, vec!["/tmp/Hello World.txt"]);
    }

    #[test]
    fn parse_gnome_invalid_header() {
        assert!(parse_gnome_payload("copye\nfile:///tmp/a.txt").is_none());
    }

    #[test]
    fn parse_uri_list_simple() {
        let s = "file:///tmp/a.txt\r\nfile:///tmp/b.txt\r\n";
        assert_eq!(parse_uri_list(s), vec!["/tmp/a.txt", "/tmp/b.txt"]);
    }

    #[test]
    fn parse_uri_list_skips_comments() {
        let s = "# RFC 2483 comment\r\nfile:///tmp/a.txt\r\n";
        assert_eq!(parse_uri_list(s), vec!["/tmp/a.txt"]);
    }
}
