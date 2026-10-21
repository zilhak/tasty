//! HTML address input normalization, independent of UI, environment and filesystem I/O.
//! A caller supplies the committed document, surface cwd and home. Local targets must pass
//! an external lookup before navigation; this check cannot guarantee a later load succeeds.

use std::path::{Path, PathBuf};
use url::Url;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AddressError {
    Empty,
    NoBase,
    Invalid,
    UnsupportedScheme,
    MissingFile(PathBuf),
    NotFile(PathBuf),
    LookupFailed(PathBuf),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AddressTarget {
    pub url: Url,
    /// Lookup uses a decoded OS path, without the URL query or fragment.
    pub local_path: Option<PathBuf>,
}

/// Only a successful file lookup admits a local navigation. The host owns asynchronous
/// lookup, error localization and stale-result rejection; no I/O runs in the view.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LocalFileStatus {
    File,
    Missing,
    Other,
    Unavailable,
}

impl AddressTarget {
    pub fn validate_local(
        &self,
        lookup: impl FnOnce(&Path) -> LocalFileStatus,
    ) -> Result<(), AddressError> {
        let Some(path) = &self.local_path else {
            return Ok(());
        };
        match lookup(path) {
            LocalFileStatus::File => Ok(()),
            LocalFileStatus::Missing => Err(AddressError::MissingFile(path.clone())),
            LocalFileStatus::Other => Err(AddressError::NotFile(path.clone())),
            LocalFileStatus::Unavailable => Err(AddressError::LookupFailed(path.clone())),
        }
    }

    pub fn display(&self, home: Option<&Path>) -> String {
        let Some(path) = &self.local_path else {
            return self.url.to_string();
        };
        let mut text = home
            .and_then(|home| path.strip_prefix(home).ok())
            .map_or_else(
                || path.to_string_lossy().into_owned(),
                |relative| {
                    if relative.as_os_str().is_empty() {
                        "~".into()
                    } else {
                        format!("~/{}", relative.to_string_lossy())
                    }
                },
            );
        if let Some(query) = self.url.query() {
            text.push('?');
            text.push_str(query);
        }
        if let Some(fragment) = self.url.fragment() {
            text.push('#');
            text.push_str(fragment);
        }
        text
    }
}

fn target(url: Url) -> Result<AddressTarget, AddressError> {
    let local_path = match url.scheme() {
        "http" | "https" if url.host_str().is_some() => None,
        "file" => Some(url.to_file_path().map_err(|()| AddressError::Invalid)?),
        _ => return Err(AddressError::UnsupportedScheme),
    };
    Ok(AddressTarget { url, local_path })
}

fn local(path: &Path) -> Result<AddressTarget, AddressError> {
    target(Url::from_file_path(path).map_err(|()| AddressError::Invalid)?)
}

/// Surrounding whitespace is ignored; interior whitespace is preserved/URL-encoded.
/// Absolute OS paths and explicit URLs take precedence. Relative input uses the current
/// local document's folder or web URL; only an empty tab may fall back to surface cwd.
/// Bare host names are relative inputs, never an implicit web search or https URL.
pub fn resolve_html_address(
    input: &str,
    document: Option<&str>,
    cwd: Option<&Path>,
    home: Option<&Path>,
) -> Result<AddressTarget, AddressError> {
    let input = input.trim();
    if input.is_empty() {
        return Err(AddressError::Empty);
    }
    if input.chars().any(char::is_control) {
        return Err(AddressError::Invalid);
    }
    // A displayed local URL may carry a query/fragment after its filesystem path.
    // Resubmitting that unchanged display keeps the URL rather than looking up a
    // different filename containing the suffix. Edited filesystem names stay literal.
    if let Some(current) = document
        .and_then(|s| Url::parse(s).ok())
        .and_then(|url| target(url).ok())
        .filter(|current| current.local_path.is_some() && current.display(home) == input)
    {
        return Ok(current);
    }
    if input == "~" || input.starts_with("~/") {
        return local(
            &home
                .ok_or(AddressError::NoBase)?
                .join(input.strip_prefix("~/").unwrap_or("")),
        );
    }
    let path = Path::new(input);
    if path.is_absolute() {
        return local(path);
    }
    if let Ok(url) = Url::parse(input) {
        return target(url);
    }
    // A syntactically explicit but invalid URL must not silently become a local filename.
    if input.contains("://") {
        return Err(AddressError::Invalid);
    }
    if let Some(document) = document.filter(|s| !s.is_empty()) {
        let base = Url::parse(document).map_err(|_| AddressError::Invalid)?;
        match base.scheme() {
            "http" | "https" => {
                return target(base.join(input).map_err(|_| AddressError::Invalid)?);
            }
            "file" => {
                // URL-only fragment/query edits preserve the current document. Other local
                // input is a filesystem name: spaces, '%' and '#' are literal, not URL syntax.
                if input.starts_with('#') || input.starts_with('?') {
                    return target(base.join(input).map_err(|_| AddressError::Invalid)?);
                }
                let file = base.to_file_path().map_err(|()| AddressError::Invalid)?;
                return local(&file.parent().ok_or(AddressError::NoBase)?.join(path));
            }
            _ => return Err(AddressError::UnsupportedScheme),
        }
    }
    local(
        &cwd.filter(|p| p.is_absolute())
            .ok_or(AddressError::NoBase)?
            .join(path),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    fn root() -> PathBuf {
        if cfg!(windows) {
            PathBuf::from("C:/work")
        } else {
            PathBuf::from("/work")
        }
    }
    #[test]
    fn local_document_wins_over_cwd() {
        let doc = Url::from_file_path(root().join("docs/a.html")).unwrap();
        let got = resolve_html_address("b.html", Some(doc.as_str()), Some(&root()), None).unwrap();
        assert_eq!(got.local_path.unwrap(), root().join("docs/b.html"));
    }
    #[test]
    fn empty_tab_uses_absolute_cwd_or_rejects() {
        assert_eq!(
            resolve_html_address("a.html", None, Some(&root()), None)
                .unwrap()
                .local_path,
            Some(root().join("a.html"))
        );
        assert_eq!(
            resolve_html_address("a.html", None, None, None),
            Err(AddressError::NoBase)
        );
        assert_eq!(
            resolve_html_address("a.html", None, Some(Path::new("relative")), None),
            Err(AddressError::NoBase)
        );
    }
    #[test]
    fn web_relative_fragment_and_parent_join() {
        for (s, expected) in [
            ("b.html", "https://example.com/a/b.html"),
            ("../c.html", "https://example.com/c.html"),
            ("#part", "https://example.com/a/#part"),
        ] {
            assert_eq!(
                resolve_html_address(s, Some("https://example.com/a/"), Some(&root()), None)
                    .unwrap()
                    .url
                    .as_str(),
                expected
            );
        }
    }
    #[test]
    fn absolute_file_and_file_url_round_trip() {
        let path = root().join("a space%#.html");
        let a = resolve_html_address(&path.to_string_lossy(), None, None, None).unwrap();
        let b = resolve_html_address(a.url.as_str(), None, None, None).unwrap();
        assert_eq!(a, b);
        assert_eq!(a.local_path, Some(path));
    }
    #[test]
    fn local_relative_names_are_not_url_escapes() {
        let doc = Url::from_file_path(root().join("a.html")).unwrap();
        let got = resolve_html_address("a%20b#.html", Some(doc.as_str()), None, None).unwrap();
        assert_eq!(got.local_path, Some(root().join("a%20b#.html")));
    }
    #[test]
    fn explicit_schemes_and_whitespace() {
        assert_eq!(
            resolve_html_address(" \t ", None, None, None),
            Err(AddressError::Empty)
        );
        assert_eq!(
            resolve_html_address("javascript:alert(1)", None, None, None),
            Err(AddressError::UnsupportedScheme)
        );
        assert_eq!(
            resolve_html_address("https://[", None, Some(&root()), None),
            Err(AddressError::Invalid)
        );
        assert_eq!(
            resolve_html_address(" https://example.com/a b ", None, None, None)
                .unwrap()
                .url
                .as_str(),
            "https://example.com/a%20b"
        );
        assert_eq!(
            resolve_html_address("a\nb", None, Some(&root()), None),
            Err(AddressError::Invalid)
        );
    }
    #[test]
    fn home_expansion_and_display_use_component_boundary() {
        let a = resolve_html_address("~/a.html", None, None, Some(&root())).unwrap();
        assert_eq!(a.display(Some(&root())), "~/a.html");
        let sibling = root().with_file_name("workspace").join("a.html");
        assert!(
            !local(&sibling)
                .unwrap()
                .display(Some(&root()))
                .starts_with('~')
        );
        assert_eq!(
            resolve_html_address("~/a.html", None, None, None),
            Err(AddressError::NoBase)
        );
    }
    #[test]
    fn local_preflight_rejects_missing_directory_and_lookup_errors() {
        let a = local(&root().join("a.html")).unwrap();
        assert_eq!(a.validate_local(|_| LocalFileStatus::File), Ok(()));
        assert!(matches!(
            a.validate_local(|_| LocalFileStatus::Missing),
            Err(AddressError::MissingFile(_))
        ));
        assert!(matches!(
            a.validate_local(|_| LocalFileStatus::Other),
            Err(AddressError::NotFile(_))
        ));
        assert!(matches!(
            a.validate_local(|_| LocalFileStatus::Unavailable),
            Err(AddressError::LookupFailed(_))
        ));
    }
    #[test]
    fn web_does_not_lookup_local_files() {
        resolve_html_address("https://example.com", None, None, None)
            .unwrap()
            .validate_local(|_| panic!("web target must not stat"))
            .unwrap();
    }
    #[test]
    fn local_url_fragment_is_not_part_of_lookup_path() {
        let mut url = Url::from_file_path(root().join("a.html")).unwrap();
        url.set_fragment(Some("part"));
        let a = resolve_html_address(url.as_str(), None, None, None).unwrap();
        assert_eq!(a.local_path, Some(root().join("a.html")));
        assert_eq!(a.display(Some(&root())), "~/a.html#part");
    }
    #[test]
    fn unchanged_local_display_preserves_query_and_fragment() {
        let mut url = Url::from_file_path(root().join("a.html")).unwrap();
        url.set_query(Some("mode=1"));
        url.set_fragment(Some("part"));
        let current = target(url).unwrap();
        for home in [None, Some(root())] {
            let shown = current.display(home.as_deref());
            assert_eq!(
                resolve_html_address(&shown, Some(current.url.as_str()), None, home.as_deref())
                    .unwrap(),
                current
            );
        }
    }
}
