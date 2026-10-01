//! Resolve one selected engine's immutable surface capture before requesting an activation.
use crate::core::CoreState;
use crate::core::layout_persistence::import::surface_data::SurfaceData;
use crate::runtime::journal_product::{PreparationInput, ShellRecipe};
use crate::model::Surface;
/// Exists only behind the bootstrap read/render barrier. SurfaceRestorer replaces this with the
/// selected kind or its ordinary lazy/plugin placeholder after reading the referenced payload.
pub(crate) struct JournalPlaceholder {
    pub(crate) id: u32,
    pub(crate) kind: String,
    pub(crate) data: Option<tasty_domain::DataRef>,
    pub(crate) creation_seed: Option<tasty_domain::DataRef>,
    pub(crate) activation: Option<tasty_domain::Activation>,
}

impl Surface for JournalPlaceholder {
    tasty_model::impl_surface_any!();
    fn kind(&self) -> &'static str {
        "empty"
    }
    fn type_name(&self) -> &'static str {
        "Pending"
    }
    fn surface_id(&self) -> Option<u32> {
        Some(self.id)
    }
    fn source_cwd(&self) -> Option<std::path::PathBuf> {
        None
    }
}



pub(crate) struct RestoreInput {
    pub(crate) surface_id: u32,
    pub(crate) reference: Option<tasty_domain::DataRef>,
    from_creation_seed: bool,
    pub(crate) input: PreparationInput,
    pub(crate) plan: tasty_domain::CreationPlan,
}

pub(crate) fn describe(engine: &crate::runtime::engine_access::EngineRef<'_>) -> Vec<RestoreInput> {
    let core=engine.core;
    let shell = crate::core::state::ShellConfig::from_settings(&core.settings);
    core.local_workspaces
        .iter()
        .flat_map(|workspace| workspace.all_surface_ids())
        .filter_map(|id| {
            let placeholder = engine
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

pub(crate) fn from_saved(id:u32,value:&tasty_domain::Surface,shell:ShellRecipe)->RestoreInput {
    RestoreInput {
        surface_id:id,reference:value.data.or(value.creation_seed),from_creation_seed:value.data.is_none() && value.creation_seed.is_some(),
        plan:tasty_domain::CreationPlan {destination:tasty_domain::CreationDestination::Restore {surface:id,previous_activation:None},surface:tasty_domain::SurfaceSpec {id,kind:value.kind.clone(),data:value.data},tab_name:String::new(),explicit_name:None},
        input:PreparationInput {kind:value.kind.clone(),cwd:None,params:serde_json::json!({}),restore:None,shell:(value.kind=="terminal").then_some(shell)},
    }
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

/// Initial activation follows explicit imported/restored View IDs, never a mutable tree index.
pub(crate) fn initial_terminal_selection(
    core: &CoreState,
    presentation: Option<&crate::model::RestoredPresentation>,
) -> std::collections::HashSet<u32> {
    use crate::model::StructurePresentation;
    let Some(workspace) = presentation
        .and_then(|view| view.active_workspace)
        .and_then(|id| {
            core.local_workspaces
                .iter()
                .find(|workspace| workspace.id == id)
        })
        .or_else(|| core.local_workspaces.first())
    else {
        return Default::default();
    };
    workspace
        .pane_layout()
        .all_pane_ids()
        .into_iter()
        .flat_map(|id| {
            let pane = workspace.pane_layout().find_pane(id).expect("listed pane");
            let index = presentation.map_or(0, |view| view.selection.tab_index(pane));
            pane.tabs
                .get(index)
                .map(|tab| tab.all_surface_ids())
                .unwrap_or_default()
        })
        .collect()
}

/// Install only logical placeholders; selected activation is a separate committed operation.
pub(crate) fn initialize_instances(session:&mut crate::runtime::engine_session::EngineSession,model:&tasty_domain::JournalModel) {
    for (id,surface) in &model.surfaces {
        session.runtime.surfaces.entry(*id).or_insert_with(||Box::new(JournalPlaceholder {
            id:*id,kind:surface.kind.clone(),data:surface.data,creation_seed:surface.creation_seed,activation:surface.activation,
        }));
    }
}
