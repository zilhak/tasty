//! Clipboard access failures are distinct from an offer without the requested content.

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum ClipboardError {
    #[cfg(feature = "gui")]
    UnsupportedNativeWayland(String),
    BackendUnavailable(String),
    TransferFailed(String),
}

impl ClipboardError {
    #[cfg(feature = "gui")]
    pub(crate) fn for_display(self, display: DisplayBackend) -> Self {
        match self {
            Self::BackendUnavailable(reason) if display == DisplayBackend::Wayland => {
                Self::UnsupportedNativeWayland(reason)
            }
            error => error,
        }
    }

    #[cfg(feature = "gui")]
    pub(crate) fn from_arboard(error: arboard::Error) -> Self {
        match error {
            arboard::Error::ClipboardNotSupported => Self::BackendUnavailable(error.to_string()),
            _ => Self::TransferFailed(error.to_string()),
        }
    }

    #[cfg(feature = "gui")]
    pub(crate) fn feedback(&self) -> (&'static str, crate::adapters::ui::ToastKind) {
        use crate::adapters::ui::ToastKind;
        match self {
            Self::UnsupportedNativeWayland(_) => {
                ("toast.clipboard_wayland_unsupported", ToastKind::Info)
            }
            Self::BackendUnavailable(_) => ("toast.clipboard_unavailable", ToastKind::Info),
            Self::TransferFailed(_) => ("toast.clipboard_failed", ToastKind::Error),
        }
    }
}

/// The GUI supplies the actual display handle; service-only callers leave it unknown.
#[cfg(feature = "gui")]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum DisplayBackend {
    #[default]
    Unknown,
    Wayland,
    Other,
}

#[cfg(feature = "gui")]
impl DisplayBackend {
    pub(crate) fn of(window: &winit::window::Window) -> Self {
        use winit::raw_window_handle::{HasDisplayHandle, RawDisplayHandle};
        match window.display_handle().map(|h| h.as_raw()) {
            Ok(RawDisplayHandle::Wayland(_)) => Self::Wayland,
            Ok(_) => Self::Other,
            Err(_) => Self::Unknown,
        }
    }
}

impl From<String> for ClipboardError {
    fn from(reason: String) -> Self {
        Self::TransferFailed(reason)
    }
}
impl From<&str> for ClipboardError {
    fn from(reason: &str) -> Self {
        Self::TransferFailed(reason.into())
    }
}

impl std::fmt::Display for ClipboardError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let (label, reason) = match self {
            #[cfg(feature = "gui")]
            Self::UnsupportedNativeWayland(reason) => (
                "native Wayland clipboard is unsupported; X11 clipboard unavailable",
                reason,
            ),
            Self::BackendUnavailable(reason) => ("clipboard backend unavailable", reason),
            Self::TransferFailed(reason) => ("clipboard transfer failed", reason),
        };
        write!(f, "{label}: {reason}")
    }
}
impl std::error::Error for ClipboardError {}
