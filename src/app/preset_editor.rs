//! App-owned preset persistence. View drafts carry values and compare-before-write edits only.
use tasty_presets::{PresetKind, PresetStore};
use crate::view::preset::draft::{PresetDrafts, PresetEdit, PresetValue};

pub(crate) fn capture(store: &PresetStore) -> PresetDrafts {
    let workspaces = store.list(PresetKind::Workspace).into_iter()
        .filter_map(|name| store.get_workspace(&name).cloned()).map(PresetValue::Workspace);
    let tabs = store.list(PresetKind::Tab).into_iter()
        .filter_map(|name| store.get_tab(&name).cloned()).map(PresetValue::Tab);
    let panes = store.list(PresetKind::Pane).into_iter()
        .filter_map(|name| store.get_pane(&name).cloned()).map(PresetValue::Pane);
    PresetDrafts::from_values(workspaces.chain(tabs).chain(panes))
}
fn current(store: &PresetStore, kind: PresetKind, name: &str) -> Option<PresetValue> {
    match kind {
        PresetKind::Workspace => store.get_workspace(name).cloned().map(PresetValue::Workspace),
        PresetKind::Tab => store.get_tab(name).cloned().map(PresetValue::Tab),
        PresetKind::Pane => store.get_pane(name).cloned().map(PresetValue::Pane),
    }
}

/// Merge only fields edited by the View so an independent subtitle edit does not conflict with
/// layout changes. A changed layout or deleted target is never recreated from a stale draft.
fn merge(before: PresetValue, after: PresetValue, live: PresetValue) -> anyhow::Result<PresetValue> {
    macro_rules! field {
        ($before:ident, $after:ident, $live:ident, $field:ident) => {
            if $before.$field != $after.$field {
                anyhow::ensure!($live.$field == $before.$field, "preset changed while editing");
                $live.$field = $after.$field;
            }
        };
    }
    Ok(match (before, after, live) {
        (PresetValue::Workspace(before), PresetValue::Workspace(after), PresetValue::Workspace(mut live)) => {
            field!(before, after, live, subtitle);
            field!(before, after, live, description);
            field!(before, after, live, layout);
            PresetValue::Workspace(live)
        }
        (PresetValue::Tab(before), PresetValue::Tab(after), PresetValue::Tab(mut live)) => {
            // The tab name is metadata; layout-only edits preserve a concurrent title edit.
            if before.tab.layout != after.tab.layout {
                anyhow::ensure!(live.tab.layout == before.tab.layout, "preset layout changed while editing");
                live.tab.layout = after.tab.layout;
            }
            if before.tab.explicit_name != after.tab.explicit_name {
                anyhow::ensure!(live.tab.explicit_name == before.tab.explicit_name, "preset title changed while editing");
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
fn apply(store: &mut PresetStore, edit: PresetEdit) -> anyhow::Result<()> {
    match edit {
        PresetEdit::Save { before, after } => {
            let live = current(store, after.kind(), after.name());
            let overwrite = before.is_some();
            let value = match (before, live) {
                (Some(before), Some(live)) => merge(before, after, live)?,
                (None, None) => after,
                _ => anyhow::bail!("preset changed while editing"),
            };
            match value {
                PresetValue::Workspace(value) if overwrite => store.save_workspace_overwrite(value)?,
                PresetValue::Workspace(value) => store.save_workspace(value)?,
                PresetValue::Tab(value) if overwrite => store.save_tab_overwrite(value)?,
                PresetValue::Tab(value) => store.save_tab(value)?,
                PresetValue::Pane(value) if overwrite => store.save_pane_overwrite(value)?,
                PresetValue::Pane(value) => store.save_pane(value)?,
            }
        }
        PresetEdit::Delete { before } => {
            anyhow::ensure!(current(store, before.kind(), before.name()).as_ref() == Some(&before), "preset changed before deletion");
            store.delete(before.kind(), before.name())?;
        }
        PresetEdit::Rename { before, to } => {
            anyhow::ensure!(current(store, before.kind(), before.name()).as_ref() == Some(&before), "preset changed before rename");
            store.rename(before.kind(), before.name(), &to)?;
        }
    }
    Ok(())
}

impl super::App {
    pub(crate) fn refresh_preset_editor(&mut self, id: winit::window::WindowId) {
        let Some(view) = self.view.views.get_mut(&id)
            .and_then(|view| view.as_any_mut().downcast_mut::<crate::view::PresetView>())
        else { return; };
        let store = crate::poison::recover_mutex(self.services.preset_store.lock(),
            crate::core::PRESET_STORE_WHAT, &crate::core::PRESET_STORE_POISONED);
        view.refresh_drafts(capture(&store));
    }

    pub(crate) fn process_preset_edits(&mut self, id: winit::window::WindowId) {
        let Some(view) = self.view.views.get_mut(&id)
            .and_then(|view| view.as_any_mut().downcast_mut::<crate::view::PresetView>())
        else { return; };
        let edits = view.take_edits();
        if edits.is_empty() { return; }
        let mut store = crate::poison::recover_mutex(self.services.preset_store.lock(),
            crate::core::PRESET_STORE_WHAT, &crate::core::PRESET_STORE_POISONED);
        let mut error = None;
        let mut applied = Vec::new();
        for edit in edits {
            let result = edit.applied();
            if let Err(failure) = apply(&mut store, edit) {
                tracing::warn!(%failure, "preset editor save failed");
                error = Some(failure.to_string());
                break;
            }
            applied.push(result);
        }
        let drafts = capture(&store);
        drop(store);
        view.accept_edits(drafts, error, &applied);
    }
}
