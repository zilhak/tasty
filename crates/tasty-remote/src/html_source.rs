//! Requested source is separate from persisted snapshots and native commit observations.
use tasty_ipc::html_transfer::HtmlSource;
#[derive(Debug, Default)]
pub struct HtmlSourceState {
    pub generation: u64,
    pub source: Option<HtmlSource>,
}
impl HtmlSourceState {
    pub fn set(&mut self, raw: Option<&str>, cwd: Option<&std::path::Path>) {
        self.replace(classify(raw, cwd));
    }
    /// The WebView setter loads every non-navigable string as markup, including plain text.
    /// Only create/restore parameters may contain an unqualified filesystem path.
    pub fn set_webview(&mut self, raw: Option<&str>) {
        let source = match raw.filter(|s| !s.is_empty()) {
            None => HtmlSource::Unsupported,
            Some(raw)
                if raw.starts_with("file://")
                    || raw.starts_with("http://")
                    || raw.starts_with("https://") =>
            {
                classify(Some(raw), None)
            }
            Some(_) => HtmlSource::Inline,
        };
        self.replace(source);
    }
    fn replace(&mut self, source: HtmlSource) {
        if self.source.as_ref() == Some(&source) {
            return;
        }
        let Some(next) = self.generation.checked_add(1) else {
            self.source = None;
            tracing::error!("HTML source generation exhausted; source disabled");
            return;
        };
        self.generation = next;
        self.source = Some(source);
    }
}
pub fn classify(raw: Option<&str>, cwd: Option<&std::path::Path>) -> HtmlSource {
    let Some(raw) = raw.filter(|s| !s.is_empty()) else {
        return HtmlSource::Unsupported;
    };
    if raw.trim_start().starts_with('<') {
        return HtmlSource::Inline;
    }
    if let Ok(url) = url::Url::parse(raw) {
        match url.scheme() {
            "http" | "https" => {
                return HtmlSource::Http {
                    url: url.to_string(),
                };
            }
            "file" => {
                return url
                    .to_file_path()
                    .ok()
                    .filter(|p| p.is_absolute())
                    .and_then(|p| file_source(p, Some(&url)))
                    .unwrap_or(HtmlSource::Unsupported);
            }
            _ if !std::path::Path::new(raw).is_absolute() => return HtmlSource::Unsupported,
            _ => {}
        }
    }
    let path = std::path::Path::new(raw);
    let path = if path.is_absolute() {
        path.to_path_buf()
    } else if let Some(cwd) = cwd {
        cwd.join(path)
    } else {
        return HtmlSource::Unsupported;
    };
    file_source(path, None).unwrap_or(HtmlSource::Unsupported)
}
fn file_source(path: std::path::PathBuf, original: Option<&url::Url>) -> Option<HtmlSource> {
    let mut url = url::Url::from_file_path(&path).ok()?;
    if let Some(original) = original {
        url.set_query(original.query());
        url.set_fragment(original.fragment());
    }
    Some(HtmlSource::File {
        // A string DTO must reject unrepresentable paths rather than open a different lossy name.
        path: path.to_str()?.to_owned(),
        url: url.into(),
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn typed_sources_never_open_http_or_markup_as_paths() {
        assert!(matches!(
            classify(Some("https://localhost:3000/a"), None),
            HtmlSource::Http { .. }
        ));
        assert_eq!(
            classify(Some("<html>file:///etc/passwd</html>"), None),
            HtmlSource::Inline
        );
        assert_eq!(
            classify(Some("data:text/html,x"), None),
            HtmlSource::Unsupported
        );
        // Lexical inputs only: this classifier performs no filesystem access.
        let root = std::path::Path::new(if cfg!(windows) {
            r"C:\html-source-fixture"
        } else {
            "/html-source-fixture"
        });
        let a = root.join("a b.html");
        let uri = url::Url::from_file_path(&a).unwrap();
        assert_eq!(
            classify(Some(uri.as_str()), None),
            HtmlSource::File {
                path: a.to_string_lossy().into_owned(),
                url: uri.to_string(),
            }
        );
        let mut state = HtmlSourceState::default();
        state.set(a.to_str(), None);
        state.set(Some(uri.as_str()), None);
        assert_eq!(state.generation, 1);
        state.set(root.join("b.html").to_str(), None);
        assert_eq!(state.generation, 2);
        let mut queried = uri.clone();
        queried.set_query(Some("mode=preview"));
        queried.set_fragment(Some("section"));
        let HtmlSource::File { path, url } = classify(Some(queried.as_str()), None) else {
            panic!("file source")
        };
        assert_eq!(path, a.to_string_lossy());
        assert_eq!(url, queried.as_str());
        for raw in ["plain text", "\u{feff}<html>body</html>", "/not-a-file"] {
            state.set_webview(Some(raw));
            assert_eq!(state.source, Some(HtmlSource::Inline));
        }
        state.set_webview(Some(uri.as_str()));
        assert_eq!(state.source, Some(classify(Some(uri.as_str()), None)));
        #[cfg(unix)]
        assert_eq!(
            classify(Some("file:///html-source-fixture/%FF.html"), None),
            HtmlSource::Unsupported
        );
    }
}
