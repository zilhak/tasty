//! Shared file consumers use the existing file writer without depending on settings revisions.
#[cfg(all(test, feature = "gui"))]
mod wake_tests;

use std::sync::Arc;

use tasty_utils::batch_writer::{Handle, RequestReceipt};

use super::file_writer::{self, FileExecutor, FileRequest};
use crate::core::file_snapshot::{PendingSave, Snapshot, Submit};
use crate::runtime::registries::RuntimeRegistries;

struct Pending(RequestReceipt<FileExecutor>);
impl PendingSave for Pending {
    fn poll(&self) -> Option<Result<(), String>> {
        self.0
            .try_take()
            .map(|outcome| file_writer::saved_of(&outcome))
    }
}

pub(crate) fn sink(queue: Handle<FileExecutor>) -> Submit {
    Arc::new(move |snapshot: Snapshot| {
        queue
            .submit(FileRequest::Snapshot(snapshot))
            .map(|receipt| Box::new(Pending(receipt)) as Box<dyn PendingSave>)
            .map_err(|e| e.to_string())
    })
}

pub(crate) fn attach(registries: &RuntimeRegistries, queue: Handle<FileExecutor>) {
    let sink = sink(queue);
    if let Some(rules) = &registries.file_rules {
        rules.install_sink(sink.clone());
    }
    #[cfg(feature = "gui")]
    {
        registries.explorer_favorites.install_sink(sink.clone());
        registries.port_favorites.install_sink(sink);
    }
}

pub(crate) fn poll(registries: &RuntimeRegistries) {
    if let Some(rules) = &registries.file_rules {
        report("file-handlers.toml", rules.poll());
    }
    #[cfg(feature = "gui")]
    {
        report(
            "explorer-favorites.toml",
            registries.explorer_favorites.poll(),
        );
        report("port-favorites.toml", registries.port_favorites.poll());
    }
}

pub(crate) fn report(file: &str, errors: Vec<String>) {
    for error in errors {
        tracing::warn!(file, %error, "file snapshot was not saved");
        crate::system_notice::post(crate::system_notice::SystemNotice::warning(
            if file == "file-handlers.toml" {
                "settings-file-writer"
            } else {
                "file-writer"
            },
            crate::i18n::t("storage_notice.file_save_title").into(),
            crate::i18n::t_fmt2("storage_notice.file_save_body", file, &error),
        ));
    }
}

pub(crate) fn before_shutdown(registries: &RuntimeRegistries) {
    poll(registries);
    if let Some(rules) = &registries.file_rules {
        rules.before_shutdown();
    }
    #[cfg(feature = "gui")]
    {
        registries.explorer_favorites.retry_latest_before_shutdown();
        registries.port_favorites.retry_latest_before_shutdown();
    }
}

pub(crate) fn after_shutdown(registries: &RuntimeRegistries) {
    poll(registries);
    if let Some(rules) = &registries.file_rules
        && let Some((visible, committed)) = rules.finish_shutdown()
    {
        tracing::warn!(
            file = "file-handlers.toml",
            visible,
            committed,
            "file remains unsaved at shutdown"
        );
    }
    #[cfg(feature = "gui")]
    for (name, unsaved) in [
        (
            "explorer-favorites.toml",
            registries.explorer_favorites.finish_shutdown(),
        ),
        (
            "port-favorites.toml",
            registries.port_favorites.finish_shutdown(),
        ),
    ] {
        if let Some((visible, committed)) = unsaved {
            tracing::warn!(
                file = name,
                visible,
                committed,
                "favorites remain unsaved at shutdown"
            );
        }
    }
}
