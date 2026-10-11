//! Copy-only outbound file sessions. Starting a session does not report a completed drop.
#[cfg(target_os = "linux")]
mod linux;
mod payload;
#[cfg(target_os = "linux")]
mod wayland;
#[cfg(target_os = "linux")]
mod x11;

use std::{path::PathBuf, sync::Arc};
use winit::{dpi::PhysicalPosition, window::Window};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Outcome {
    /// The receiver reported accepting Copy; its filesystem work is owned by the receiver.
    Copied,
    Cancelled,
    /// The peer disappeared or never reported its terminal outcome.
    Unknown,
}

pub(crate) struct Context {
    #[cfg(target_os = "linux")]
    wayland: Option<wayland::Context>,
}
impl Context {
    pub(crate) fn new(
        window: Arc<Window>,
        display_owner: Option<winit::event_loop::OwnedDisplayHandle>,
    ) -> Self {
        #[cfg(target_os = "linux")]
        {
            let wayland = match wayland::Context::new(window, display_owner) {
                Ok(context) => context,
                Err(error) => {
                    tracing::warn!(%error, "native drag context unavailable");
                    None
                }
            };
            Self { wayland }
        }
        #[cfg(not(target_os = "linux"))]
        {
            drop((window, display_owner));
            Self {}
        }
    }
    pub(crate) fn poll(&mut self) {
        #[cfg(target_os = "linux")]
        if let Some(context) = self.wayland.as_mut()
            && let Err(error) = context.poll()
        {
            tracing::warn!(%error, "native drag input context failed");
            self.wayland = None;
        }
    }
}

#[derive(Debug)]
pub(crate) enum StartError {
    #[cfg(not(target_os = "linux"))]
    Unsupported,
    Failed(String),
}

impl std::fmt::Display for StartError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            #[cfg(not(target_os = "linux"))]
            Self::Unsupported => {
                f.write_str("native outbound drag is unavailable on this platform")
            }
            Self::Failed(error) => f.write_str(error),
        }
    }
}

pub(crate) struct Session {
    #[cfg(target_os = "linux")]
    backend: linux::Session,
    // Keep the native window/display owner alive until all native resources have been released.
    _window: Arc<Window>,
}
impl Session {
    pub(crate) fn start(
        window: Arc<Window>,
        paths: &[PathBuf],
        context: &mut Context,
    ) -> Result<Self, StartError> {
        payload::validate(paths).map_err(StartError::Failed)?;
        #[cfg(target_os = "linux")]
        {
            let backend = linux::Session::start(&window, paths, context.wayland.as_mut())
                .map_err(StartError::Failed)?;
            Ok(Self {
                backend,
                _window: window,
            })
        }
        #[cfg(not(target_os = "linux"))]
        {
            drop((window, context));
            Err(StartError::Unsupported)
        }
    }
    pub(crate) fn motion(&mut self, _position: PhysicalPosition<f64>) -> Result<(), String> {
        #[cfg(target_os = "linux")]
        return self.backend.motion(_position);
        #[cfg(not(target_os = "linux"))]
        {
            Ok(())
        }
    }
    pub(crate) fn release(&mut self) -> Result<(), String> {
        #[cfg(target_os = "linux")]
        return self.backend.release();
        #[cfg(not(target_os = "linux"))]
        Ok(())
    }
    pub(crate) fn poll(&mut self) -> Result<Option<Outcome>, String> {
        #[cfg(target_os = "linux")]
        return self.backend.poll();
        #[cfg(not(target_os = "linux"))]
        Ok(Some(Outcome::Cancelled))
    }
    pub(crate) fn accepted(&self) -> bool {
        #[cfg(target_os = "linux")]
        return self.backend.accepted();
        #[cfg(not(target_os = "linux"))]
        false
    }
}
