//! Result-aware host text/image clipboard. Keep the owner alive after a write.

use super::error::{ClipboardError, DisplayBackend};

pub(crate) struct ClipboardContext {
    inner: Result<arboard::Clipboard, ClipboardError>,
}

pub(crate) enum PasteContent {
    Text(String),
    Image(arboard::ImageData<'static>),
}

impl ClipboardContext {
    pub(crate) fn read_paste(&mut self) -> Result<Option<PasteContent>, ClipboardError> {
        read_paste(self.get_text(), || self.get_image())
    }

    #[cfg(test)]
    pub(crate) fn unavailable(error: ClipboardError) -> Self {
        Self { inner: Err(error) }
    }

    pub(crate) fn new(display: DisplayBackend) -> Self {
        Self {
            inner: arboard::Clipboard::new().map_err(|error| {
                ClipboardError::BackendUnavailable(error.to_string()).for_display(display)
            }),
        }
    }

    fn inner(&mut self) -> Result<&mut arboard::Clipboard, ClipboardError> {
        self.inner.as_mut().map_err(|error| error.clone())
    }

    pub(crate) fn get_text(&mut self) -> Result<Option<String>, ClipboardError> {
        content(self.inner()?.get_text())
    }

    pub(crate) fn get_image(
        &mut self,
    ) -> Result<Option<arboard::ImageData<'static>>, ClipboardError> {
        content(self.inner()?.get_image())
    }

    pub(crate) fn set_text(&mut self, text: &str) -> Result<(), ClipboardError> {
        self.inner()?
            .set_text(text)
            .map_err(ClipboardError::from_arboard)
    }

    /// Linux primary selection; other platforms use their ordinary clipboard.
    pub(crate) fn set_text_primary(&mut self, text: &str) -> Result<(), ClipboardError> {
        #[cfg(target_os = "linux")]
        {
            use arboard::{LinuxClipboardKind, SetExtLinux};
            self.inner()?
                .set()
                .clipboard(LinuxClipboardKind::Primary)
                .text(text)
                .map_err(ClipboardError::from_arboard)
        }
        #[cfg(not(target_os = "linux"))]
        self.set_text(text)
    }
}

fn read_paste(
    text: Result<Option<String>, ClipboardError>,
    image: impl FnOnce() -> Result<Option<arboard::ImageData<'static>>, ClipboardError>,
) -> Result<Option<PasteContent>, ClipboardError> {
    match text? {
        Some(text) if text.is_empty() => Ok(None),
        Some(text) => Ok(Some(PasteContent::Text(text))),
        None => Ok(image()?.map(PasteContent::Image)),
    }
}

fn content<T>(result: Result<T, arboard::Error>) -> Result<Option<T>, ClipboardError> {
    match result {
        Ok(value) => Ok(Some(value)),
        Err(arboard::Error::ContentNotAvailable) => Ok(None),
        Err(error) => Err(ClipboardError::from_arboard(error)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paste_only_tries_an_image_after_absent_text() {
        for text in [
            Ok(Some(String::new())),
            Ok(Some("fixture".into())),
            Err(ClipboardError::BackendUnavailable("fixture".into())),
        ] {
            let _outcome = read_paste(text, || panic!("must not read image"));
        }
        let mut called = false;
        assert!(
            read_paste(Ok(None), || {
                called = true;
                Ok(None)
            })
            .unwrap()
            .is_none()
        );
        assert!(called);
        assert!(
            read_paste(
                Err(ClipboardError::TransferFailed("fixture".into())),
                || panic!("no partial paste on transfer failure")
            )
            .is_err()
        );
    }

    #[test]
    fn missing_content_is_not_an_access_failure() {
        assert_eq!(
            content::<String>(Err(arboard::Error::ContentNotAvailable)),
            Ok(None)
        );
        assert_eq!(content(Ok(String::new())), Ok(Some(String::new())));
        assert!(matches!(
            content::<String>(Err(arboard::Error::ClipboardOccupied)),
            Err(ClipboardError::TransferFailed(_))
        ));
    }

    #[test]
    fn initialization_failure_survives_every_operation() {
        let error = ClipboardError::BackendUnavailable("fixture".into())
            .for_display(DisplayBackend::Wayland);
        let mut clipboard = ClipboardContext {
            inner: Err(error.clone()),
        };
        assert_eq!(clipboard.get_text(), Err(error.clone()));
        assert_eq!(clipboard.get_image().unwrap_err(), error);
        assert_eq!(clipboard.set_text("fixture"), Err(error.clone()));
        assert_eq!(clipboard.set_text_primary("fixture"), Err(error));
        assert!(matches!(
            ClipboardError::BackendUnavailable("fixture".into()).for_display(DisplayBackend::Other),
            ClipboardError::BackendUnavailable(_)
        ));
    }

    #[test]
    #[ignore = "writes the OS clipboard; run on an isolated display"]
    fn the_os_clipboard_round_trips_text() {
        let _guard = crate::clipboard::files::os_clipboard_test_lock();
        let mut owner = ClipboardContext::new(DisplayBackend::Other);
        owner.set_text("Tasty clipboard fixture 한글").unwrap();
        let mut reader = ClipboardContext::new(DisplayBackend::Other);
        assert_eq!(
            reader.get_text().unwrap().as_deref(),
            Some("Tasty clipboard fixture 한글")
        );
    }
}
