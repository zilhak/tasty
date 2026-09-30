//! Resolve one selected engine's immutable surface capture before requesting an activation.
use crate::core::CoreState;
use crate::core::layout_persistence::import::surface_data::SurfaceData;
use crate::runtime::journal_product::{PreparationInput, ShellRecipe};
use crate::runtime::live_projection::bootstrap::JournalPlaceholder;

pub(crate) struct RestoreInput {
    pub(crate) surface_id: u32,
    pub(crate) reference: Option<tasty_domain::DataRef>,
    from_creation_seed: bool,
    pub(crate) input: PreparationInput,
    pub(crate) plan: tasty_domain::CreationPlan,
}

pub(crate) fn describe(core: &CoreState) -> Vec<RestoreInput> {
    let shell = crate::core::state::ShellConfig::from_settings(&core.settings);
    core.local_workspaces
        .iter()
        .flat_map(|workspace| workspace.all_surface_ids())
        .filter_map(|id| {
            let placeholder = core
                .find_surface_by_id(id)?
                .as_any()
                .downcast_ref::<JournalPlaceholder>()?;
            Some(RestoreInput {
                surface_id: id,
                reference: placeholder.data.or(placeholder.creation_seed),
                from_creation_seed: placeholder.data.is_none()
                    && placeholder.creation_seed.is_some(),
                plan: tasty_domain::CreationPlan {
                    destination: tasty_domain::CreationDestination::Restore {
                        surface: id,
                        previous_activation: placeholder
                            .activation
                            .map(|activation| activation.generation),
                    },
                    surface: tasty_domain::SurfaceSpec {
                        id,
                        kind: placeholder.kind.clone(),
                        data: placeholder.data,
                    },
                    tab_name: String::new(),
                    explicit_name: None,
                },
                input: PreparationInput {
                    kind: placeholder.kind.clone(),
                    cwd: None,
                    params: serde_json::json!({}),
                    restore: None,
                    shell: (placeholder.kind == "terminal").then(|| ShellRecipe {
                        executable: shell.shell.clone(),
                        arguments: shell.args.clone(),
                        environment: shell.envs.clone(),
                        cols: core.default_cols,
                        rows: core.default_rows,
                        scrollback_lines: core.settings.general.scrollback_lines,
                        disk_scrollback: core.settings.performance.scrollback_disk_swap,
                        startup_command: core.settings.general.startup_command.clone(),
                        restore_command: None,
                    }),
                },
            })
        })
        .collect()
}

pub(crate) fn accept_payload(
    request: &mut RestoreInput,
    reference: tasty_domain::DataRef,
    bytes: &[u8],
) -> Result<(), String> {
    if request.reference != Some(reference) {
        return Err("restore payload belongs to another immutable capture".into());
    }
    if request.from_creation_seed {
        let seed: PreparationInput =
            serde_json::from_slice(bytes).map_err(|error| error.to_string())?;
        if seed.kind != request.input.kind {
            return Err("creation seed belongs to another surface kind".into());
        }
        request.input.cwd = seed.cwd;
        if request.input.kind != "terminal" {
            request.input.params = seed.params;
            request.input.restore = seed.restore;
        }
        // Terminal restart uses current shell settings and explicit restore.command, never the
        // previous activation's raw launch command, arguments or startup input.
        return Ok(());
    }
    let data = SurfaceData::decode(bytes).map_err(|error| error.to_string())?;
    match data {
        SurfaceData::Terminal {
            cwd,
            restore_command,
            ..
        } if request.input.kind == "terminal" => {
            request.input.cwd = cwd.map(std::path::PathBuf::from);
            request
                .input
                .shell
                .as_mut()
                .ok_or("terminal restore recipe missing")?
                .restore_command = restore_command;
        }
        SurfaceData::Generic { .. } if request.input.kind != "terminal" => {
            // The claim reads the existing capture. Do not resend/store a potentially large JSON
            // value through the bounded command queue merely to restore the same immutable data.
        }
        _ => return Err("restore payload kind differs from the committed surface".into()),
    }
    Ok(())
}

/// The legacy View snapshot still selects positions until its import mapping is consumed. The
/// resulting activation list is fixed to live IDs before any asynchronous preparation begins.
pub(crate) fn initial_terminal_selection(core: &CoreState) -> std::collections::HashSet<u32> {
    let active = core
        .pending_layout_restore
        .as_ref()
        .map_or(0, |saved| saved.active_workspace);
    let Some(workspace) = core
        .local_workspaces
        .get(active.min(core.local_workspaces.len().saturating_sub(1)))
    else {
        return Default::default();
    };
    let saved = core
        .pending_layout_restore
        .as_ref()
        .and_then(|saved| saved.workspaces.get(active));
    let mut selected_tabs = Vec::new();
    if let Some(saved) = saved {
        fn walk(
            node: &crate::core::layout_persistence::schema::SavedPaneNode,
            out: &mut Vec<usize>,
        ) {
            use crate::core::layout_persistence::schema::SavedPaneNode;
            match node {
                SavedPaneNode::Leaf(pane) => out.push(pane.active_tab),
                SavedPaneNode::Split { first, second, .. } => {
                    walk(first, out);
                    walk(second, out);
                }
            }
        }
        walk(&saved.pane_layout, &mut selected_tabs);
    }
    workspace
        .pane_layout()
        .all_pane_ids()
        .into_iter()
        .enumerate()
        .flat_map(|(index, id)| {
            let pane = workspace.pane_layout().find_pane(id).expect("listed pane");
            let selected = selected_tabs
                .get(index)
                .copied()
                .unwrap_or(0)
                .min(pane.tabs.len().saturating_sub(1));
            pane.tabs
                .get(selected)
                .map(|tab| tab.all_surface_ids())
                .unwrap_or_default()
        })
        .collect()
}
