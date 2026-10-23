//! Explorer 이름 거르기와 하위 검색의 같은 입력 문법. 확장자 조건만 공백 토큰으로 읽는다.

use super::DirEntryInfo;

#[derive(Default)]
pub(crate) struct FindQuery {
    text: String,
    extensions: Vec<String>,
}

impl FindQuery {
    pub(crate) fn parse(query: &str) -> Self {
        let mut extensions = Vec::new();
        let mut text = Vec::new();
        for token in query.split_whitespace() {
            let extension = token
                .split_once(':')
                .filter(|(key, _)| key.eq_ignore_ascii_case("ext"))
                .map(|(_, value)| value.strip_prefix('.').unwrap_or(value))
                .filter(|value| {
                    !value.is_empty()
                        && value
                            .chars()
                            .all(|c| c.is_alphanumeric() || c == '_' || c == '-')
                });
            match extension {
                Some(ext) => extensions.push(ext.to_lowercase()),
                None => text.push(token),
            }
        }
        Self {
            // 연산자가 없으면 공백까지 기존 부분 문자열 검색 그대로다.
            text: if extensions.is_empty() {
                query.to_owned()
            } else {
                text.join(" ")
            },
            extensions,
        }
    }

    pub(crate) fn text(&self) -> &str {
        &self.text
    }

    fn text_matches(&self, name: &str) -> bool {
        if self.text.is_empty() {
            return true;
        }
        if name.is_ascii() && self.text.is_ascii() {
            return name
                .as_bytes()
                .windows(self.text.len())
                .any(|part| part.eq_ignore_ascii_case(self.text.as_bytes()));
        }
        tasty_ui_widgets::match_range(name, &self.text).is_some()
    }

    pub(crate) fn matches(&self, entry: &DirEntryInfo) -> bool {
        self.text_matches(&entry.name)
            && (self.extensions.is_empty()
                || (!entry.is_dir
                    && self
                        .extensions
                        .iter()
                        .any(|ext| entry.ext.to_lowercase() == *ext)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(name: &str, is_dir: bool) -> DirEntryInfo {
        DirEntryInfo {
            name: name.into(),
            is_dir,
            ext: std::path::Path::new(name)
                .extension()
                .map_or(String::new(), |s| s.to_string_lossy().into_owned()),
            ..Default::default()
        }
    }

    #[test]
    fn extensions_are_alternatives_combined_with_the_existing_name_substring() {
        let q = FindQuery::parse("Note ext:rs EXT:.Md");
        assert_eq!(q.text(), "Note");
        for (name, dir, expected) in [
            ("notes.RS", false, true),
            ("my-NOTE.md", false, true),
            ("other.rs", false, false),
            ("note.rst", false, false),
            ("note.tar.rs", false, true),
            ("note.rs", true, false),
            (".rs", false, false),
        ] {
            assert_eq!(q.matches(&entry(name, dir)), expected, "{name}, dir={dir}");
        }
        assert!(FindQuery::parse("ext:rs").matches(&entry("main.RS", false)));
        assert!(FindQuery::parse("ext:RS ext:rs").matches(&entry("main.rs", false)));
        assert!(!FindQuery::parse("ext:rs").matches(&entry("README", false)));
    }

    #[test]
    fn plain_names_and_incomplete_or_malformed_operators_stay_literal() {
        for text in [
            "two  words",
            "ext:",
            "ext:.",
            "ext:*.rs",
            "ext:rs,md",
            "ext:a/b",
            "xext:rs",
        ] {
            let q = FindQuery::parse(text);
            assert_eq!(q.text(), text);
            assert!(q.matches(&entry(&format!("prefix {text} suffix"), false)));
            assert!(!q.matches(&entry("main.rs", false)));
        }
        assert!(FindQuery::parse("").matches(&entry("any", true)));
        assert!(FindQuery::parse("notes here ext:rs").matches(&entry("my notes here.rs", false)));
        assert!(
            !FindQuery::parse("notes here ext:rs")
                .matches(&entry("notes elsewhere here.rs", false))
        );
    }
    #[test]
    fn borrowed_ascii_comparison_preserves_unicode_and_literal_substring_matching() {
        for name in [
            "item000099.TXT",
            "two  words",
            "Kelvin",
            "İmage",
            "Straße",
            "a/b.rs",
            "",
        ] {
            for query in [
                "",
                "ITEM000099",
                "txt",
                " ",
                "two  ",
                "k",
                "i",
                "i\u{307}",
                "ß",
                "ss",
                "/b",
                "no match",
            ] {
                assert_eq!(
                    FindQuery::parse(query).matches(&entry(name, false)),
                    tasty_ui_widgets::match_range(name, query).is_some() || query.is_empty(),
                    "{name:?} / {query:?}"
                );
            }
        }
    }
}
