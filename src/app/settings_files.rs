//! Settings file operations execute after the originating View returns its requests.
pub(crate) mod export;
use export::{ExportError, PendingExport};
use std::{path::PathBuf, sync::Weak};
use tasty_host_plugin::keybinding_bundle::{BundleError, DecodeEnv, DecodedBundle, decode};

pub(crate) enum SettingsFileRequest {
    Export {
        path: PathBuf,
        text: String,
        observer: Weak<()>,
    },
    Import {
        path: PathBuf,
        plugins: Vec<String>,
        scripts: Vec<String>,
    },
}

#[allow(clippy::large_enum_variant)] // reason: 내보내기·가져오기 조작 1회마다 하나를 만들어 즉시 소비한다 — 힙 할당을 더할 자리가 아니다
pub(crate) enum SettingsFileResult {
    Export {
        path: PathBuf,
        result: Result<(), ExportError>,
        observer: Weak<()>,
    },
    ExportTransport {
        observer: Weak<()>,
        error: ExportError,
    },
    Import {
        path: PathBuf,
        result: Result<DecodedBundle, Option<usize>>,
    },
}

impl SettingsFileRequest {
    fn execute(self) -> SettingsFileResult {
        match self {
            Self::Export {
                path,
                text,
                observer,
            } => {
                let result = export::write(&path, &text);
                if let Err(error) = &result {
                    tracing::error!(%error, path = %path.display(), "keybinding export failed");
                }
                SettingsFileResult::Export {
                    path,
                    result: result.map_err(ExportError::Io),
                    observer,
                }
            }
            Self::Import {
                path,
                plugins,
                scripts,
            } => {
                let result = std::fs::read_to_string(&path).map_err(|error| {
                    tracing::warn!(%error, path = %path.display(), "keybinding import read failed");
                    None
                }).and_then(|text| {
                    let env = DecodeEnv {
                        installed_plugin_ids: &plugins,
                        known_script_ids: Some(&scripts),
                    };
                    decode(&text, &env).map_err(|error| {
                        tracing::warn!(%error, path = %path.display(), "keybinding import parse failed");
                        match error {
                            BundleError::Toml(error) => error.span().map(|span| {
                                text[..span.start.min(text.len())].matches('\n').count() + 1
                            }),
                            _ => None,
                        }
                    })
                });
                SettingsFileResult::Import { path, result }
            }
        }
    }
}

impl super::App {
    pub(crate) fn process_settings_file_requests(&mut self, id: winit::window::WindowId) {
        let requests = self
            .view
            .views
            .get_mut(&id)
            .and_then(|view| {
                view.as_any_mut()
                    .downcast_mut::<crate::view::SettingsView>()
            })
            .map(crate::view::SettingsView::take_file_requests)
            .unwrap_or_default();
        for request in requests {
            if matches!(&request, SettingsFileRequest::Export { .. }) {
                let queue = self.services.file_command_queue();
                match PendingExport::submit(queue.as_ref(), request) {
                    Ok(pending) => {
                        self.state.settings_exports.push((id, pending));
                        if let Some(view) = self.view.views.get_mut(&id) {
                            view.mark_dirty();
                        }
                    }
                    Err(result) => self.deliver_settings_file_result(id, *result),
                }
            } else {
                self.deliver_settings_file_result(id, request.execute());
            }
        }
    }
    fn deliver_settings_file_result(
        &mut self,
        id: winit::window::WindowId,
        result: SettingsFileResult,
    ) {
        if let Some(view) = self.view.views.get_mut(&id).and_then(|view| {
            view.as_any_mut()
                .downcast_mut::<crate::view::SettingsView>()
        }) {
            view.accept_file_result(result);
        }
    }
    pub(crate) fn finish_settings_exports(&mut self) {
        for (id, pending) in std::mem::take(&mut self.state.settings_exports) {
            if let Some(result) = pending.poll() {
                self.deliver_settings_file_result(id, result);
            } else {
                self.state.settings_exports.push((id, pending));
            }
        }
    }
}

#[cfg(test)]
mod tests;
