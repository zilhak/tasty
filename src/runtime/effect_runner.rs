//! Executes an already claimed materialization. Prepared objects remain private until publication.

mod installation;

use crate::runtime::engine_access::EngineMut;
use crate::runtime::surface_registry::PublicationAction;
use crate::model::TerminalSurface;
use crate::runtime::journal_product::{ClaimedPreparation, EffectLease};
pub(crate) struct PreparedLeaf {
    pub(crate) logical_kind:String,
    pub(crate) surface:Box<dyn crate::model::Surface>,
}
use tasty_terminal::{Pty, ResourceGeneration, Terminal};

pub(crate) use installation::{Installation, Installed, RetiringKind};

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
    registration: Option<KindRegistration>,
    installed: bool,
    deferred:bool,
    adoption:Option<crate::runtime::journal_product::AdoptRecipe>,
    scrollback_persist_id: Option<String>,
    metadata:Vec<(String,String)>,
    child:Option<crate::runtime::journal_product::ChildRecipe>,
    one_shot_input:Option<String>,
}

#[derive(Clone)]
pub(super) struct KindRegistration {
    kind: String,
    definition: std::sync::Weak<crate::runtime::surface_registry::SurfaceKindDef>,
}

impl KindRegistration {
    fn validate(&self, registry: &crate::runtime::surface_registry::SurfaceKindRegistry) -> anyhow::Result<()> {
        if let Some(plugin_id) = registry.withdrawn_by(&self.kind) {
            return Err(crate::runtime::surface_registry::SurfaceKindWithdrawn {
                kind: self.kind.clone(),
                plugin_id,
            }
            .into());
        }
        let current = registry
            .get_live(&self.kind)
            .ok_or_else(|| anyhow::anyhow!("prepared kind is no longer registered"))?;
        if !self.definition.ptr_eq(&std::sync::Arc::downgrade(&current)) {
            anyhow::bail!("prepared kind registration was replaced before installation");
        }
        Ok(())
    }
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
            | tasty_domain::CreationDestination::Restore { .. }
    ) && (previous_resource.is_some() || engine.find_surface_by_id(surface_id).is_some())
    {
        anyhow::bail!("reserved surface ID already has a live owner");
    }

    let mut scrollback_persist_id = None;
    let metadata=if matches!(claimed.plan.destination,tasty_domain::CreationDestination::Pane {..}|tasty_domain::CreationDestination::Split {..}) {
        claimed.input.params.get("meta").and_then(|value|value.as_object()).into_iter().flatten().filter_map(|(key,value)|value.as_str().map(|value|(key.clone(),value.to_owned()))).collect()
    } else {Vec::new()};
    let input = claimed.input;
    let adoption=input.adopt.clone();
    let child=input.child.clone();
    if child.as_ref().is_some_and(|child|child.runtime_epoch!=binding.runtime_epoch) {anyhow::bail!("child creation belongs to an earlier runtime; one-shot input cannot be replayed");}
    let deferred=matches!(claimed.plan.destination,tasty_domain::CreationDestination::Assembly {..}) && input.kind!="terminal" && engine.runtime.surface_registry.get_live(&input.kind).is_none();
    let (surface, connection, publication, registration) = if let Some(adoption)=&adoption {
        if input.kind!="terminal" || adoption.runtime_epoch!=binding.runtime_epoch {anyhow::bail!("standalone transfer belongs to another runtime or kind");}
        let (terminal,pty,persist_id)=engine.runtime.terminals.take_standalone_for_adoption(adoption.pty_id,adoption.resource_generation).ok_or_else(||anyhow::anyhow!("standalone owner exited or changed before transfer"))?;
        scrollback_persist_id=persist_id;
        (Box::new(TerminalSurface {id:surface_id}) as Box<dyn crate::model::Surface>,Some((terminal,pty)),None,None)
    } else if input.kind == "terminal" {
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
        terminal.set_color_palette(crate::runtime::terminal_store::current_terminal_palette());
        terminal.set_scrollback_limit(shell.scrollback_lines);
        if shell.disk_scrollback {
            terminal.enable_disk_scrollback(surface_id);
        }
        if let Some(bytes) = &claimed.capture {
            match crate::core::layout_persistence::import::surface_data::SurfaceData::decode(bytes)?
            {
                crate::core::layout_persistence::import::surface_data::SurfaceData::Terminal {
                    scrollback,
                    scrollback_ref,
                    ..
                } => {
                    scrollback_persist_id = scrollback_ref;
                    if let Some(blob) = scrollback {
                        if let Some(lines) =
                            tasty_terminal::disk_scrollback::deserialize_lines(&blob)
                        {
                            if !lines.is_empty() {
                                terminal.inject_scrollback(lines);
                                let prefill = terminal.rows() / 2;
                                terminal.prefill_visible_from_scrollback(prefill);
                            }
                        } else {
                            tracing::warn!(
                                surface_id,
                                "stored scrollback could not be decoded; keeping its immutable capture"
                            );
                        }
                    }
                }
                _ => anyhow::bail!("terminal activation capture belongs to another kind"),
            }
        }
        if !shell.startup_command.trim().is_empty() {
            terminal.send_key(&format!("{}\n", shell.startup_command.trim()));
        }
        (
            Box::new(TerminalSurface { id: surface_id }) as Box<dyn crate::model::Surface>,
            Some((terminal, pty)),
            None,
            None,
        )
    } else if deferred {
        (Box::new(crate::runtime::surface_restorer::JournalPlaceholder {id:surface_id,kind:input.kind.clone(),data:claimed.plan.surface.data,creation_seed:None,activation:Some(tasty_domain::Activation {generation:claimed.lease.resource_generation,phase:tasty_domain::ActivationPhase::Deferred})}) as Box<dyn crate::model::Surface>,None,None,None)
    } else {
        if let Some(plugin_id) = engine.runtime.surface_registry.withdrawn_by(&input.kind) {
            return Err(crate::runtime::surface_registry::SurfaceKindWithdrawn {
                kind: input.kind,
                plugin_id,
            }
            .into());
        }
        let definition = engine.runtime.surface_registry
            .get_live(&input.kind)
            .ok_or_else(|| anyhow::anyhow!("surface kind {} is not registered", input.kind))?;
        let restore = match claimed.capture.as_deref() {
            Some(bytes) => match crate::core::layout_persistence::import::surface_data::SurfaceData::decode(bytes)? {
                crate::core::layout_persistence::import::surface_data::SurfaceData::Generic { data } => Some(data),
                _ => anyhow::bail!("generic activation capture belongs to another kind"),
            },
            None => input.restore,
        };
        let prepared = match restore {
            Some(data) => (definition.restore)(surface_id, &data)?,
            None => (definition.create)(surface_id, input.cwd.as_deref(), &input.params)?,
        };
        let registration = KindRegistration {
            kind: input.kind.clone(),
            definition: std::sync::Arc::downgrade(&definition),
        };
        (
            prepared.surface,
            None,
            prepared.publication,
            Some(registration),
        )
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
        registration,
        installed: false,deferred,adoption,
        scrollback_persist_id,metadata,child,one_shot_input:None,
    })
}

impl PreparedMaterialization {
    /// Installation consumes external handles while the engine keeps the unpublished kind leaf.
    pub(crate) fn set_one_shot_input(&mut self,input:Option<String>) {self.one_shot_input=input;}
    pub(crate) fn is_deferred(&self)->bool {self.deferred}
    pub(crate) fn surface_id(&self)->Option<u32> {self.leaf.surface.surface_id()}

    pub(crate) fn begin_installation(
        &mut self,
        registry: &crate::runtime::surface_registry::SurfaceKindRegistry,
    ) -> anyhow::Result<Installation> {
        if self.installed {
            anyhow::bail!("prepared materialization was already installed");
        }
        if let Some(registration) = &self.registration {
            registration.validate(registry)?;
        }
        if let Some(adoption)=&self.adoption {
            let (_,pty)=self.connection.as_mut().ok_or_else(||anyhow::anyhow!("standalone transfer has no physical owner"))?;
            if pty.generation().value()!=adoption.resource_generation || pty.state().standalone().is_none() {
                anyhow::bail!("standalone transfer binding changed before installation");
            }
            // check_alive records reap without consuming the once-only exit notification.
            // Rejection keeps this exact pair available for rollback to its standalone key.
            if !pty.check_alive() || pty.state().observation().phase==tasty_terminal::PtyPhase::WaitFailed {
                anyhow::bail!("standalone owner exited or could not be observed before installation");
            }
        }
        if self.child.as_ref().is_some_and(|child|child.has_command) && self.one_shot_input.is_none() {
            anyhow::bail!("child command continuation is unavailable; input cannot be replayed");
        }
        self.installed = true;
        Ok(Installation {
            lease: self.lease.clone(),
            surface_id: self
                .leaf
                .surface
                .surface_id()
                .expect("prepared surface has a fixed ID"),
            previous_resource: self.previous_resource,
            connection: self.connection.take(),
            publication: self.publication.take(),
            registration: self.registration.take(),
            scrollback_persist_id: self.scrollback_persist_id.take(),
            metadata:std::mem::take(&mut self.metadata),
            adoption:self.adoption.clone(),child:self.child.clone(),one_shot_input:self.one_shot_input.take(),
        })
    }

    pub(crate) fn discard(mut self,engine:&mut EngineMut<'_>)->Result<Option<tasty_terminal::PtyRetirement>,(Self,String)> {
        let Some((terminal,pty))=self.connection.take() else {return Ok(None);};
        if let Some(adoption)=&self.adoption {
            if let Err(pair)=engine.runtime.terminals.restore_standalone_adoption(adoption.pty_id,terminal,pty,self.scrollback_persist_id.take()) {
                self.connection=Some((pair.0,pair.1));self.scrollback_persist_id=pair.2;
                return Err((self,"original standalone binding cannot be restored; reconciliation required".into()));
            }
            return Ok(None);
        }
        drop(terminal);Ok(Some(pty.retire()))
    }

    pub(crate) fn into_leaf(self, installed: &Installed) -> anyhow::Result<PreparedLeaf> {
        if self.lease != installed.lease
            || !installed.cleanup_complete()?
            || !self.installed
            || self.connection.is_some()
            || self.publication.is_some()
        {
            anyhow::bail!("uninstalled materialization cannot be published");
        }
        Ok(self.leaf)
    }
}
