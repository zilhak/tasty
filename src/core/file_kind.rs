//! 파일 확장자를 종류로 분류한다. 탐색기의 Type 열 낱말과 Type 정렬이 같은 분류를 쓴다.
//! 낱말(번역문)은 UI 가 붙이고, 정렬은 언어와 무관하게 이 분류의 선언 순서로 묶는다.

/// 파일 종류. 선언 순서가 Type 정렬의 묶음 순서다. 표에 없는 확장자는 `Other` 다.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum FileKind {
    Archive,
    Csv,
    Html,
    Image,
    Json,
    Markdown,
    Pdf,
    Shell,
    Text,
    Toml,
    Yaml,
    Other,
}

/// 소문자 확장자의 종류. 대소문자를 섞어 받아도 같은 종류다.
pub(crate) fn file_kind(ext: &str) -> FileKind {
    let lower = ext.to_lowercase();
    match lower.as_str() {
        "md" | "markdown" => FileKind::Markdown,
        "txt" => FileKind::Text,
        "pdf" => FileKind::Pdf,
        "zip" | "tar" | "gz" | "tgz" | "bz2" | "xz" | "zst" | "7z" | "rar" => FileKind::Archive,
        "sh" | "bash" | "zsh" | "fish" | "ps1" => FileKind::Shell,
        "html" | "htm" => FileKind::Html,
        "json" => FileKind::Json,
        "toml" => FileKind::Toml,
        "yaml" | "yml" => FileKind::Yaml,
        "csv" => FileKind::Csv,
        e if is_image_ext(e) => FileKind::Image,
        _ => FileKind::Other,
    }
}

/// 확장자가 이미지 파일인지 — design 은 이미지 glyph 를 accent-info 로 강조한다.
pub(crate) fn is_image_ext(ext: &str) -> bool {
    matches!(
        ext,
        "png"
            | "jpg"
            | "jpeg"
            | "gif"
            | "webp"
            | "svg"
            | "bmp"
            | "ico"
            | "tif"
            | "tiff"
            | "avif"
            | "heic"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extensions_fall_into_kinds() {
        assert_eq!(file_kind("gz"), FileKind::Archive);
        assert_eq!(file_kind("ZIP"), FileKind::Archive);
        assert_eq!(file_kind("yml"), FileKind::Yaml);
        assert_eq!(file_kind("png"), FileKind::Image);
        assert_eq!(file_kind("rs"), FileKind::Other);
        assert_eq!(file_kind(""), FileKind::Other);
    }
}
