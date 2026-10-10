//! 항목 종류를 낱말로 쓴다. Detail 의 Type 열, Properties 의 Kind, 미리보기 머리가 같은 낱말을 써서
//! 세 곳이 어긋나지 않는다. 분류는 `core::file_kind` 의 표이고, 표에 없으면 "{EXT} file" 이다.
//! 링크는 "Link to {kind}" 다. 다만 Properties 는 lstat 결과를 그대로 "Symbolic link" 로 쓴다.

use crate::core::file_kind::{FileKind, file_kind};
use crate::core::fs_list::{DirEntryInfo, EntryLink};
use crate::i18n::t;

/// 종류의 형식 이름. 번역 키를 쓰는 것과 모든 언어에서 같은 낱말을 쓰는 것이 있다.
/// 그림은 "{TYPE} image", 그 밖은 "{EXT} file" 이라 확장자를 받아야 해서 `None` 이다.
fn format_word(kind: FileKind) -> Option<String> {
    let key = match kind {
        FileKind::Markdown => "explorer.kind.markdown",
        FileKind::Text => "explorer.kind.text",
        FileKind::Pdf => "explorer.kind.pdf",
        FileKind::Archive => "explorer.kind.archive",
        FileKind::Shell => "explorer.kind.shell",
        FileKind::Html => return Some("HTML".into()),
        FileKind::Json => return Some("JSON".into()),
        FileKind::Toml => return Some("TOML".into()),
        FileKind::Yaml => return Some("YAML".into()),
        FileKind::Csv => return Some("CSV".into()),
        FileKind::Image | FileKind::Other => return None,
    };
    Some(t(key).to_string())
}

/// 링크가 아닌 항목의 종류. 폴더는 "Folder", 파일은 [`file_kind_word`] 다.
fn target_kind_word(e: &DirEntryInfo) -> String {
    if e.is_dir {
        t("explorer.type.folder").to_string()
    } else {
        file_kind_word(&e.ext)
    }
}

/// Type 열과 미리보기 머리의 종류 문구. 링크는 대상의 종류를 "Link to {kind}" 로 감싸고,
/// 대상이 없는 링크는 "Broken link" 다. 링크의 종류는 링크 이름의 확장자와 대상이 폴더인지로 정한다.
pub(crate) fn kind_word(e: &DirEntryInfo) -> String {
    match e.link {
        EntryLink::NotALink => target_kind_word(e),
        EntryLink::Valid => t("explorer.kind.link_to").replace("{kind}", &target_kind_word(e)),
        EntryLink::Broken => t("explorer.kind.broken_link").to_string(),
    }
}

/// 파일 종류를 낱말로: 표에 있으면 형식 이름("Markdown"), 그림은 "PNG image", 그 밖은 "ZIP file",
/// 확장자가 없으면 "File". 번역문은 이름 붙은 `{type}`·`{ext}` 자리로 대문자 확장자를 받는다.
pub(crate) fn file_kind_word(ext: &str) -> String {
    if ext.is_empty() {
        return t("explorer.type.file").to_string();
    }
    let upper = ext.to_uppercase();
    let kind = file_kind(ext);
    match (format_word(kind), kind) {
        (Some(word), _) => word,
        (None, FileKind::Image) => t("explorer.kind.image").replace("{type}", &upper),
        (None, _) => t("explorer.kind.ext_file").replace("{ext}", &upper),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn entry(name: &str, is_dir: bool, link: EntryLink) -> DirEntryInfo {
        DirEntryInfo {
            path: PathBuf::from("/srv").join(name),
            name: name.into(),
            is_dir,
            size: 6,
            modified: None,
            ext: name
                .rsplit_once('.')
                .map_or(String::new(), |(_, e)| e.to_string()),
            link,
            ..Default::default()
        }
    }

    #[test]
    fn kinds_read_as_words() {
        crate::i18n::init("en");
        assert_eq!(file_kind_word("png"), "PNG image");
        assert_eq!(file_kind_word("rs"), "RS file");
        assert_eq!(file_kind_word(""), "File");
    }

    #[test]
    fn the_format_table_names_common_files() {
        crate::i18n::init("en");
        for (ext, word) in [
            ("md", "Markdown"),
            ("MARKDOWN", "Markdown"),
            ("txt", "Text"),
            ("htm", "HTML"),
            ("json", "JSON"),
            ("toml", "TOML"),
            ("yml", "YAML"),
            ("csv", "CSV"),
            ("pdf", "PDF document"),
            ("gz", "Archive"),
            ("7z", "Archive"),
            ("zst", "Archive"),
            ("ps1", "Shell script"),
            ("fish", "Shell script"),
        ] {
            assert_eq!(file_kind_word(ext), word, "{ext}");
        }
    }

    #[test]
    fn links_say_what_they_point_to() {
        crate::i18n::init("en");
        assert_eq!(
            kind_word(&entry("link.md", false, EntryLink::Valid)),
            "Link to Markdown"
        );
        assert_eq!(
            kind_word(&entry("projects", true, EntryLink::Valid)),
            "Link to Folder"
        );
        assert_eq!(
            kind_word(&entry("old-config.toml", false, EntryLink::Broken)),
            "Broken link"
        );
        assert_eq!(
            kind_word(&entry("Documents", true, EntryLink::NotALink)),
            "Folder"
        );
    }
}
