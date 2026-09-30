//! Executes an already claimed materialization. Prepared objects remain private until publication.

mod installation;

use crate::core::engine_access::EngineMut;
use crate::core::surface_registry::PublicationAction;
use crate::model::TerminalSurface;
use crate::runtime::journal_product::{ClaimedPreparation, EffectLease};
use crate::runtime::live_projection::PreparedLeaf;
use tasty_terminal::{Pty, ResourceGeneration, Terminal};

pub(crate) use installation::{Installation, Installed};

/// An application binding permits execution for this selected engine in this process only.
#[derive(Clone, Debug)]
pub(crate) struct ExecutionBinding {
    pub(crate) stream: String,
    pub(crate) runtime_epoch: u64,
    pub(crate) engine_incarnation: u64,
}

pub(crate) struct PreparedMaterialization {
    pub(crate) lease: EffectLease,
    pub(crate) leaf: PreparedLeaf,
    pub(crate) connection: Option<(Terminal, Pty)>,
    pub(crate) publication: Option<PublicationAction>,
    previous_resource: Option<ResourceGeneration>,
}

pub(crate) fn prepare(
    engine: &mut EngineMut<'_>,
    binding: &ExecutionBinding,
    claimed: ClaimedPreparation,
) -> anyhow::Result<PreparedMaterialization> {
    if binding.stream != claimed.lease.stream
        || binding.runtime_epoch != claimed.lease.runtime_epoch
        || binding.engine_incarnation != claimed.engine_incarnation
    {
        anyhow::bail!("materialization claim belongs to a different engine binding");
    }
    let surface_id = claimed.plan.surface.id;
    let previous_resource = engine.runtime.terminals.generation(surface_id);
    if !matches!(
        claimed.plan.destination,
        tasty_domain::CreationDestination::Convert { .. }
    ) && (previous_resource.is_some() || engine.find_surface_by_id(surface_id).is_some())
    {
        anyhow::bail!("reserved surface ID already has a live owner");
    }

    let input = claimed.input;
    let (surface, connection, publication) = if input.kind == "terminal" {
        let shell = input
            .shell
            .ok_or_else(|| anyhow::anyhow!("terminal preparation has no shell recipe"))?;
        let args: Vec<_> = shell.arguments.iter().map(String::as_str).collect();
        let environment: Vec<_> = shell
            .environment
            .iter()
            .map(|(key, value)| (key.as_str(), value.as_str()))
            .collect();
        let initial = shell
            .restore_command
            .as_ref()
            .map(|command| format!("{command}\r"));
        let (mut terminal, pty) = tasty_terminal::spawn_terminal(
            tasty_terminal::TerminalConfig {
                cols: shell.cols,
                rows: shell.rows,
                shell: (!shell.executable.is_empty()).then_some(shell.executable.as_str()),
                args: &args,
                extra_env: &environment,
                surface_id,
                working_dir: input.cwd.as_deref(),
                initial_input: initial.as_deref(),
            },
            engine.make_waker(surface_id),
        )?;
        terminal.set_color_palette(crate::core::terminal_store::current_terminal_palette());
        terminal.set_scrollback_limit(shell.scrollback_lines);
        if shell.disk_scrollback {
            terminal.enable_disk_scrollback(surface_id);
        }
        if !shell.startup_command.trim().is_empty() {
            terminal.send_key(&format!("{}\n", shell.startup_command.trim()));
        }
        (
            Box::new(TerminalSurface { id: surface_id }) as Box<dyn crate::model::Surface>,
            Some((terminal, pty)),
            None,
        )
    } else {
        if let Some(plugin_id) = engine.surface_registry.withdrawn_by(&input.kind) {
            return Err(crate::core::surface_registry::SurfaceKindWithdrawn {
                kind: input.kind,
                plugin_id,
            }
            .into());
        }
        let definition = engine
            .surface_registry
            .get_live(&input.kind)
            .ok_or_else(|| anyhow::anyhow!("surface kind {} is not registered", input.kind))?;
        let prepared = match input.restore {
            Some(data) => (definition.restore)(surface_id, &data)?,
            None => (definition.create)(surface_id, input.cwd.as_deref(), &input.params)?,
        };
        (prepared.surface, None, prepared.publication)
    };
    Ok(PreparedMaterialization {
        lease: claimed.lease,
        leaf: PreparedLeaf {
            logical_kind: input.kind,
            surface,
        },
        connection,
        publication,
        previous_resource,
    })
}

impl PreparedMaterialization {
    /// Split ownership only after the preparation result is durable; neither part is cloned.
    pub(crate) fn into_installation(self) -> (PreparedLeaf, Installation) {
        let id = self
            .leaf
            .surface
            .surface_id()
            .expect("prepared kind has a fixed surface ID");
        (
            self.leaf,
            Installation {
                lease: self.lease,
                surface_id: id,
                previous_resource: self.previous_resource,
                connection: self.connection,
                publication: self.publication,
            },
        )
    }
}
