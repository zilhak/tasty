//! App-owned preset persistence. View drafts carry values and compare-before-write edits only.
#[cfg(test)]
mod tests;

use super::preset_commands::{Outcome, Ticket};
use crate::intent::preset::PresetMutationError;
use crate::view::preset::draft::{PresetApplied, PresetDrafts, PresetEdit, PresetValue};
use tasty_presets::{PresetKind, PresetStore};

pub(crate) fn capture(store: &PresetStore) -> PresetDrafts {
    let workspaces = store
        .list(PresetKind::Workspace)
        .into_iter()
        .filter_map(|name| store.get_workspace(&name).cloned())
        .map(PresetValue::Workspace);
    let tabs = store
        .list(PresetKind::Tab)
        .into_iter()
        .filter_map(|name| store.get_tab(&name).cloned())
        .map(PresetValue::Tab);
    let panes = store
        .list(PresetKind::Pane)
        .into_iter()
        .filter_map(|name| store.get_pane(&name).cloned())
        .map(PresetValue::Pane);
    PresetDrafts::from_values(workspaces.chain(tabs).chain(panes))
}
fn current(store: &PresetStore, kind: PresetKind, name: &str) -> Option<PresetValue> {
    match kind {
        PresetKind::Workspace => store
            .get_workspace(name)
            .cloned()
            .map(PresetValue::Workspace),
        PresetKind::Tab => store.get_tab(name).cloned().map(PresetValue::Tab),
        PresetKind::Pane => store.get_pane(name).cloned().map(PresetValue::Pane),
    }
}

/// Merge only fields edited by the View so an independent subtitle edit does not conflict with
/// layout changes. A changed layout or deleted target is never recreated from a stale draft.
fn merge(
    before: PresetValue,
    after: PresetValue,
    live: PresetValue,
) -> anyhow::Result<PresetValue> {
    macro_rules! field {
        ($before:ident, $after:ident, $live:ident, $field:ident) => {
            if $before.$field != $after.$field {
                anyhow::ensure!(
                    $live.$field == $before.$field,
                    "preset changed while editing"
                );
                $live.$field = $after.$field;
            }
        };
    }
    Ok(match (before, after, live) {
        (
            PresetValue::Workspace(before),
            PresetValue::Workspace(after),
            PresetValue::Workspace(mut live),
        ) => {
            field!(before, after, live, subtitle);
            field!(before, after, live, description);
            field!(before, after, live, layout);
            PresetValue::Workspace(live)
        }
        (PresetValue::Tab(before), PresetValue::Tab(after), PresetValue::Tab(mut live)) => {
            // The tab name is metadata; layout-only edits preserve a concurrent title edit.
            if before.tab.layout != after.tab.layout {
                anyhow::ensure!(
                    live.tab.layout == before.tab.layout,
                    "preset layout changed while editing"
                );
                live.tab.layout = after.tab.layout;
            }
            if before.tab.explicit_name != after.tab.explicit_name {
                anyhow::ensure!(
                    live.tab.explicit_name == before.tab.explicit_name,
                    "preset title changed while editing"
                );
                live.tab.explicit_name = after.tab.explicit_name;
            }
            PresetValue::Tab(live)
        }
        (PresetValue::Pane(before), PresetValue::Pane(after), PresetValue::Pane(mut live)) => {
            field!(before, after, live, pane);
            PresetValue::Pane(live)
        }
        _ => anyhow::bail!("preset kind changed while editing"),
    })
}
fn owned(value: PresetValue) -> tasty_presets::OwnedPreset {
    match value {
        PresetValue::Workspace(value) => tasty_presets::OwnedPreset::Workspace(value),
        PresetValue::Tab(value) => tasty_presets::OwnedPreset::Tab(value),
        PresetValue::Pane(value) => tasty_presets::OwnedPreset::Pane(value),
    }
}
fn conflict(message: impl Into<String>) -> Outcome<()> {
    Outcome::new(
        Vec::new(),
        Err(PresetMutationError::Conflict(message.into())),
    )
}
fn save_edit(
    store: &mut PresetStore,
    before: Option<PresetValue>,
    after: PresetValue,
) -> Outcome<()> {
    let live = current(store, after.kind(), after.name());
    let overwrite = before.is_some();
    let value = match (before, live) {
        (Some(before), Some(live)) => match merge(before, after, live) {
            Ok(value) => value,
            Err(error) => return conflict(error.to_string()),
        },
        (None, None) => after,
        _ => return conflict("preset changed while editing"),
    };
    let name = value.name().to_owned();
    match store.save_command("", Some(&name), overwrite, owned(value)) {
        Ok(Some(change)) => Outcome::new(vec![change], Ok(())),
        Ok(None) => conflict("preset changed while editing"),
        Err(error) => Outcome::new(Vec::new(), Err(PresetMutationError::Store(error))),
    }
}
fn apply(store: &mut PresetStore, edit: PresetEdit) -> Outcome<()> {
    match edit {
        PresetEdit::Save { before, after } => save_edit(store, before, after),
        PresetEdit::Delete { before } => {
            if current(store, before.kind(), before.name()).as_ref() != Some(&before) {
                return conflict("preset changed before deletion");
            }
            let output = store.delete_command(before.kind(), before.name());
            Outcome::new(
                output.changes,
                output.result.map_err(PresetMutationError::Store),
            )
        }
        PresetEdit::Rename { before, to } => {
            if current(store, before.kind(), before.name()).as_ref() != Some(&before) {
                return conflict("preset changed before rename");
            }
            let output = store.rename_command(before.kind(), before.name(), &to);
            Outcome::new(
                output.changes,
                output.result.map_err(PresetMutationError::Store),
            )
        }
    }
}
fn value_bytes(value: &PresetValue) -> usize {
    use super::file_writer::command::value_bytes;
    match value {
        PresetValue::Workspace(value) => value_bytes(value),
        PresetValue::Tab(value) => value_bytes(value),
        PresetValue::Pane(value) => value_bytes(value),
    }
}
fn edit_bytes(edit: &PresetEdit) -> usize {
    let bytes = match edit {
        PresetEdit::Save { before, after } => before
            .as_ref()
            .map_or(0, value_bytes)
            .saturating_add(value_bytes(after)),
        PresetEdit::Delete { before } => value_bytes(before),
        PresetEdit::Rename { before, to } => value_bytes(before).saturating_add(to.len()),
    };
    bytes.saturating_add(128)
}
#[derive(serde::Serialize)]
pub(crate) struct EditReport {
    applied: usize,
    error: Option<String>,
    partial: bool,
}
fn run_edits(store: &mut PresetStore, edits: Vec<PresetEdit>) -> Outcome<EditReport> {
    let mut report = EditReport {
        applied: 0,
        error: None,
        partial: false,
    };
    let mut changes = Vec::new();
    for edit in edits {
        let output = apply(store, edit);
        changes.extend(output.changes);
        if let Err(error) = output.result {
            report.partial = report.applied > 0 || output.partial;
            report.error = Some(error.to_string());
            break;
        }
        report.applied += 1;
    }
    let saved = report.error.is_none();
    let partial = report.partial;
    let completed_edits = report.applied;
    let mut output = Outcome::new(changes, Ok(report));
    output.saved = saved;
    output.partial = partial;
    output.completed_edits = completed_edits;
    output
}
pub(crate) struct PendingEdit {
    window: winit::window::WindowId,
    identity: std::sync::Weak<()>,
    ticket: Ticket<EditReport>,
    applied: Vec<PresetApplied>,
}

impl super::App {
    pub(crate) fn refresh_preset_editor(&mut self, id: winit::window::WindowId) {
        let Some(view) = self
            .view
            .views
            .get_mut(&id)
            .and_then(|view| view.as_any_mut().downcast_mut::<crate::view::PresetView>())
        else {
            return;
        };
        let store = crate::poison::recover_mutex(
            self.services.preset_store.lock(),
            crate::core::PRESET_STORE_WHAT,
            &crate::core::PRESET_STORE_POISONED,
        );
        view.refresh_drafts(capture(&store));
    }

    pub(crate) fn process_preset_edits(&mut self, id: winit::window::WindowId) {
        let Some(view) = self
            .view
            .views
            .get_mut(&id)
            .and_then(|view| view.as_any_mut().downcast_mut::<crate::view::PresetView>())
        else {
            return;
        };
        let edits = view.take_edits();
        if edits.is_empty() {
            return;
        }
        let identity = view.base.state.identity();
        let applied = edits.iter().map(PresetEdit::applied).collect();
        let bytes = edits
            .iter()
            .fold(0usize, |bytes, edit| bytes.saturating_add(edit_bytes(edit)));
        let ticket = self
            .services
            .preset_files
            .submit(bytes, move |store| run_edits(store, edits));
        self.preset_io.edits.push(PendingEdit {
            window: id,
            identity,
            ticket,
            applied,
        });
        self.finish_preset_edits();
    }

    pub(crate) fn finish_preset_edits(&mut self) {
        let pending = std::mem::take(&mut self.preset_io.edits);
        for entry in pending {
            let Some(result) = entry.ticket.poll() else {
                self.preset_io.edits.push(entry);
                continue;
            };
            let (applied, error) = match result {
                Ok(report) => (report.applied, report.error),
                Err(error) => {
                    let applied = match &error {
                        PresetMutationError::Cache {
                            completed_edits, ..
                        } => *completed_edits,
                        _ => 0,
                    };
                    (applied, Some(error.to_string()))
                }
            };
            if let Some(error) = &error {
                tracing::warn!(%error, "preset editor command failed");
            }
            let Some(view) = self
                .view
                .views
                .get_mut(&entry.window)
                .and_then(|view| view.as_any_mut().downcast_mut::<crate::view::PresetView>())
                .filter(|view| view.base.state.matches_identity(&entry.identity))
            else {
                continue;
            };
            let store = crate::poison::recover_mutex(
                self.services.preset_store.lock(),
                crate::core::PRESET_STORE_WHAT,
                &crate::core::PRESET_STORE_POISONED,
            );
            let drafts = capture(&store);
            drop(store);
            view.accept_edits(
                drafts,
                error,
                &entry.applied[..applied.min(entry.applied.len())],
            );
            view.base.mark_dirty();
        }
    }
}
