//! `~/.tasty/file-handlers.toml` 공유 파일 — `FileFormatRegistry` 와
//! `FileHandlerRegistry` 가 모두 자기 섹션을 가지고 있다.
//! Settings UI 에서 한쪽만 저장하면 다른 쪽 섹션이 사라지므로 두 export 를 합쳐
//! 단일 atomic write 로 처리한다.
//!
//! `[[detector]]` + `[[extension_priority]]` (file_format) 와 `[[handler]]`
//! (file_handler) 는 서로 다른 top-level key 라 단순 문자열 concatenate 로 합치면
//! 유효한 TOML 이 된다.

use std::io::Write;
use std::path::Path;

use crate::file::format::FileFormatRegistry;
use crate::file::handler::FileHandlerRegistry;

/// 두 registry 의 user-origin export 를 합쳐 `path` 에 atomic write.
/// 빈 결과 (양쪽 모두 user contribution 없음) 면 빈 파일로 덮어쓴다.
pub fn save_combined_user_config(
    file_format: &FileFormatRegistry,
    file_handler: &FileHandlerRegistry,
    path: &Path,
) -> std::io::Result<()> {
    let mut text = String::new();
    let fmt = file_format.export_user_config();
    let hnd = file_handler.export_user_config();
    if !fmt.is_empty() {
        text.push_str(&fmt);
    }
    if !hnd.is_empty() {
        if !text.is_empty() && !text.ends_with('\n') {
            text.push('\n');
        }
        text.push_str(&hnd);
    }
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    if !parent.exists() {
        std::fs::create_dir_all(parent)?;
    }
    let mut tmp = tempfile::NamedTempFile::new_in(parent)?;
    tmp.write_all(text.as_bytes())?;
    tmp.flush()?;
    tmp.persist(path).map_err(|e| e.error)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::file::format::FileFormatRegistry;
    use crate::file::handler::FileHandlerRegistry;

    #[test]
    fn empty_registries_write_empty_file() {
        let fmt = FileFormatRegistry::new();
        let hnd = FileHandlerRegistry::new();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("file-handlers.toml");
        save_combined_user_config(&fmt, &hnd, &path).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.is_empty());
    }

    #[test]
    fn priorities_and_handlers_round_trip() {
        let fmt = FileFormatRegistry::new();
        fmt.install_host_defaults(
            r#"[[detector]]
id = "markdown"
rule = [{ kind = "extension", values = ["md"] }]
"#,
        );
        fmt.install_host_defaults(
            r#"[[detector]]
id = "mdx"
rule = [{ kind = "extension", values = ["md"] }]
"#,
        );
        fmt.set_user_extension_priority(
            "md",
            vec![
                crate::file::format::DetectorId::new("mdx"),
                crate::file::format::DetectorId::new("markdown"),
            ],
        );
        let hnd = FileHandlerRegistry::new();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("file-handlers.toml");
        save_combined_user_config(&fmt, &hnd, &path).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("extension_priority"));
        assert!(text.contains("\"md\""));
        assert!(text.contains("mdx"));

        // round-trip — re-parse and check order preserved.
        let fmt2 = FileFormatRegistry::new();
        fmt2.install_user_config(&path);
        let order = fmt2.extension_priority_order("md").expect("present");
        assert_eq!(
            order,
            vec![
                crate::file::format::DetectorId::new("mdx"),
                crate::file::format::DetectorId::new("markdown"),
            ]
        );
    }
}
