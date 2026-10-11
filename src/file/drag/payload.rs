//! Immutable local paths; never canonicalize symlinks or read a remote filesystem.
use std::path::PathBuf;

pub(super) const MAX_BYTES: usize = 1024 * 1024;

pub(super) fn validate(paths: &[PathBuf]) -> Result<(), String> {
    if paths.is_empty()
        || paths
            .iter()
            .any(|p| !p.is_absolute() || p.as_os_str().as_encoded_bytes().contains(&0))
    {
        return Err("file drag requires nonempty absolute local paths without NUL".into());
    }
    if paths
        .iter()
        .map(|p| {
            p.as_os_str()
                .as_encoded_bytes()
                .len()
                .saturating_mul(3)
                .saturating_add(9)
        })
        .sum::<usize>()
        > MAX_BYTES
    {
        return Err("file drag path payload exceeds 1 MiB".into());
    }
    Ok(())
}

#[cfg(target_os = "linux")]
pub(super) fn uri_list(paths: &[PathBuf]) -> Vec<u8> {
    use std::os::unix::ffi::OsStrExt;
    let mut out = String::new();
    for path in paths {
        out.push_str("file://");
        for &byte in path.as_os_str().as_bytes() {
            if byte.is_ascii_alphanumeric() || matches!(byte, b'/' | b'-' | b'.' | b'_' | b'~') {
                out.push(byte as char);
            } else {
                use std::fmt::Write;
                // Writing to a String cannot fail.
                write!(out, "%{byte:02X}").expect("String formatting");
            }
        }
        out.push_str("\r\n");
    }
    out.into_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_invalid_and_oversized_payloads() {
        assert!(validate(&[]).is_err());
        assert!(validate(&[PathBuf::from("relative")]).is_err());
        assert!(validate(&[PathBuf::from("/a\0b")]).is_err());
        assert!(validate(&[PathBuf::from(format!("/{}", "a".repeat(MAX_BYTES)))]).is_err());
    }
    #[cfg(target_os = "linux")]
    #[test]
    fn preserves_bytes_and_link_names_without_filesystem_access() {
        use std::{ffi::OsString, os::unix::ffi::OsStringExt};
        let paths = [
            PathBuf::from(OsString::from_vec(b"/space #\n%\xff".to_vec())),
            PathBuf::from("/broken-link"),
        ];
        assert!(validate(&paths).is_ok());
        assert_eq!(
            uri_list(&paths),
            b"file:///space%20%23%0A%25%FF\r\nfile:///broken-link\r\n"
        );
    }
}
