//! detector·extension_priority·handler의 사용자 설정을 합쳐 한 번에 저장한다.
//! 한 레지스트리만 저장해 다른 설정을 지우지 않도록 서로 다른 TOML 최상위 절을 합친다.

use std::io::Write;
use std::path::Path;

use crate::FileHandlerRegistry;
use tasty_file_format::FileFormatRegistry;

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
    use crate::FileHandlerRegistry;
    use tasty_file_format::FileFormatRegistry;

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
                tasty_file_format::DetectorId::new("mdx"),
                tasty_file_format::DetectorId::new("markdown"),
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

        let fmt2 = FileFormatRegistry::new();
        fmt2.install_user_config(&path);
        let order = fmt2.extension_priority_order("md").expect("present");
        assert_eq!(
            order,
            vec![
                tasty_file_format::DetectorId::new("mdx"),
                tasty_file_format::DetectorId::new("markdown"),
            ]
        );
    }
}
