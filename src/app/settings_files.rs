//! Settings file operations execute after the originating View returns its requests.
use std::path::PathBuf;
use tasty_host_plugin::keybinding_bundle::{BundleError, DecodeEnv, DecodedBundle, decode};

pub(crate) enum SettingsFileRequest {
    Export {
        path: PathBuf,
        text: String,
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
        result: std::io::Result<()>,
    },
    Import {
        path: PathBuf,
        result: Result<DecodedBundle, Option<usize>>,
    },
}

impl SettingsFileRequest {
    fn execute(self) -> SettingsFileResult {
        match self {
            Self::Export { path, text } => {
                let result = std::fs::write(&path, text);
                if let Err(error) = &result {
                    tracing::error!(%error, path = %path.display(), "keybinding export failed");
                }
                SettingsFileResult::Export { path, result }
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
    /// Synchronous execution keeps results attached to this exact settings View; no worker can
    /// deliver into a replacement modal after Cancel or close.
    pub(crate) fn process_settings_file_requests(&mut self, id: winit::window::WindowId) {
        let Some(view) = self.view.views.get_mut(&id).and_then(|view| {
            view.as_any_mut()
                .downcast_mut::<crate::view::SettingsView>()
        }) else {
            return;
        };
        for request in view.take_file_requests() {
            view.accept_file_result(request.execute());
        }
    }
}
