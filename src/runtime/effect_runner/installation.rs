//! Install an authorized candidate before publishing its final structural result.

use super::*;
use tasty_terminal::{PtyPhase, PtyRetirement};

pub(crate) struct Installation {
    pub(crate) lease: EffectLease,
    pub(super) surface_id: u32,
    pub(super) previous_resource: Option<ResourceGeneration>,
    pub(super) connection: Option<(Terminal, Pty)>,
    pub(super) publication: Option<PublicationAction>,
    pub(super) unpublished_remote: Option<crate::plugin_bridge::host_cmd::SurfaceBinding>,
    pub(super) registration: Option<KindRegistration>,
    pub(super) scrollback_persist_id: Option<String>,
    pub(super) metadata: Vec<(String, String)>,
    pub(super) adoption: Option<crate::runtime::journal_product::AdoptRecipe>,
    pub(super) child: Option<crate::runtime::journal_product::ChildRecipe>,
    pub(super) one_shot_input: Option<String>,
    pub(super) retirement: Option<PtyRetirement>,
    pub(super) remote_retirements: Vec<crate::plugin_bridge::host_cmd::RemoteRetirementReceipt>,
}

pub(crate) struct RetiringKind {
    surface_id: u32,
    mesh: Option<crate::plugin_bridge::host_cmd::MeshBinding>,
    remote: Option<crate::plugin_bridge::host_cmd::SurfaceBinding>,
}

impl RetiringKind {
    pub(crate) fn capture(surface: &dyn crate::model::Surface) -> Self {
        Self {
            surface_id: surface.surface_id().expect("retiring leaf has an ID"),
            mesh: surface
                .as_any()
                .downcast_ref::<crate::runtime::egui_mesh_surface::EguiMeshSurface>()
                .map(|mesh| mesh.retirement_binding.clone()),
            remote: surface
                .as_any()
                .downcast_ref::<crate::plugin_bridge::remote_surface::RemoteSurface>()
                .map(|remote| remote.handles().binding()),
        }
    }
}

pub(crate) struct Installed {
    pub(crate) lease: EffectLease,
    #[cfg(feature = "gui")]
    pub(crate) previous_resource: Option<ResourceGeneration>,
    pub(crate) surface_id: u32,
    retirement: Option<PtyRetirement>,
    remote_retirements: Vec<crate::plugin_bridge::host_cmd::RemoteRetirementReceipt>,
    installed_generation: Option<ResourceGeneration>,
    input: Option<PendingSubmit>,
}

struct PendingSubmit {
    surface: u32,
    generation: ResourceGeneration,
    ack: tasty_terminal::WriteAck,
    started: std::time::Instant,
    settled: Option<std::time::Instant>,
}

impl Installed {
    pub(crate) fn reconciliation_evidence(
        &self,
        engine: &crate::runtime::engine_access::EngineRef<'_>,
        incarnation: u64,
    ) -> Option<Vec<u8>> {
        if !matches!(self.cleanup_complete(), Ok(true))
            || engine.runtime.terminals.generation(self.surface_id) != self.installed_generation
        {
            return None;
        }
        serde_json::to_vec(&serde_json::json!({"version":1,"source":"owned-preparation-receipts","runtime_epoch":self.lease.runtime_epoch,"engine_incarnation":incarnation,"lease":self.lease,"installation_complete":true,"cleanup_complete":true,"surface":self.surface_id,"physical_generation":self.installed_generation.map(|generation|generation.value()),"remote_destroy_acknowledged":self.remote_retirements.len()})).ok()
    }
    pub(crate) fn waiting_input(&self) -> bool {
        self.input.is_some()
    }
    pub(crate) fn poll_input(&mut self, engine: &mut EngineMut<'_>) -> anyhow::Result<()> {
        let Some(input) = self.input.as_mut() else {
            return Ok(());
        };
        if !engine
            .runtime
            .terminals
            .matches_generation(input.surface, input.generation)
            || engine.live.occupancy.is_hard_occupied(input.surface)
        {
            anyhow::bail!("child input target changed after body enqueue; late submit discarded");
        }
        if input.settled.is_none()
            && (input.ack.is_complete()
                || input.started.elapsed() >= std::time::Duration::from_secs(5))
        {
            input.settled = Some(std::time::Instant::now());
        }
        if input
            .settled
            .is_some_and(|settled| settled.elapsed() >= std::time::Duration::from_millis(20))
        {
            engine
                .runtime
                .terminals
                .get_mut(input.surface)
                .ok_or_else(|| anyhow::anyhow!("child input owner disappeared"))?
                .try_send_key_with_ack("\r")
                .map_err(|_| anyhow::anyhow!("child submit queue closed"))?;
            self.input = None;
        }
        Ok(())
    }

    /// Signalling/removing an old owner is not evidence that the OS child was reaped.
    pub(crate) fn cleanup_complete(&self) -> anyhow::Result<bool> {
        if self.input.is_some() {
            return Ok(false);
        }
        self.release_complete()
    }

    /// Disposal can finish without claiming that an interrupted one-shot input was delivered.
    pub(crate) fn release_complete(&self) -> anyhow::Result<bool> {
        for receipt in &self.remote_retirements {
            match receipt.observation() {
                Some(Ok(())) => {}
                Some(Err(reason)) => anyhow::bail!(reason),
                None => return Ok(false),
            }
        }
        match self
            .retirement
            .as_ref()
            .map(|receipt| receipt.observation().phase)
        {
            None | Some(PtyPhase::Reaped) => Ok(true),
            Some(PtyPhase::WaitFailed) => {
                anyhow::bail!("old PTY owner could not confirm child reap")
            }
            Some(_) => Ok(false),
        }
    }
}

impl Installation {
    pub(crate) fn surface_id(&self) -> u32 {
        self.surface_id
    }

    /// The caller keeps the entire affected batch hidden until the completion commit is applied.
    /// A failure after external registration requires halt/reconciliation, never legacy fallback.
    pub(crate) fn install(
        &mut self,
        engine: &mut EngineMut<'_>,
        mut plugins: Option<&mut crate::plugin::PluginManager>,
        retired_kind: Option<RetiringKind>,
    ) -> anyhow::Result<Installed> {
        self.validate_installation_owner(engine)?;
        if let Some(old) = retired_kind {
            if old.surface_id != self.surface_id {
                anyhow::bail!("retired kind belongs to another surface");
            }
            if let Some(binding) = old.remote {
                let manager = plugins
                    .as_deref_mut()
                    .ok_or_else(|| anyhow::anyhow!("remote retirement has no plugin host"))?;
                self.remote_retirements.push(
                    manager
                        .enqueue_observed_remote_retirement(self.surface_id, binding)
                        .map_err(anyhow::Error::msg)?,
                );
            } else if let Some(binding) = old.mesh {
                let manager = plugins
                    .as_deref_mut()
                    .ok_or_else(|| anyhow::anyhow!("mesh retirement has no plugin host"))?;
                self.remote_retirements.push(
                    manager
                        .enqueue_observed_mesh_retirement(self.surface_id, &binding)
                        .map_err(anyhow::Error::msg)?,
                );
            }
        }
        // Destroy of the old registration precedes enqueueing Created/Restored for the new one.
        if let Some(publication) = self.publication.take() {
            publication()?;
        }
        let previous = match self.connection.take() {
            Some((mut terminal, mut pty)) => {
                if let Some(adoption) = &self.adoption {
                    if pty.generation().value() != adoption.resource_generation {
                        anyhow::bail!("standalone installation has another physical owner");
                    }
                    pty.adopt();
                    terminal.rewire_waker(engine.make_waker(self.surface_id));
                    if let Some(factory) = &engine.runtime.waker_factory {
                        factory.forget_surface(adoption.pty_id);
                    }
                }
                engine
                    .runtime
                    .terminals
                    .replace(self.surface_id, terminal, Some(pty))
            }
            None => engine.runtime.terminals.remove(self.surface_id),
        };
        // Own the old receipt before any later metadata/input step can fail.
        self.retirement = previous.and_then(|(terminal, pty)| {
            drop(terminal);
            pty.map(Pty::retire)
        });
        if let Some(persist_id) = self.scrollback_persist_id.take() {
            engine
                .runtime
                .terminals
                .set_scrollback_persist_id(self.surface_id, persist_id);
        }
        self.install_metadata(engine);
        let submit = self.child.as_ref().is_some_and(|child| !child.replacing);
        if let Some(child) = self.child.take() {
            if child.replacing {
                engine
                    .runtime
                    .child_terminals
                    .update_child(child.parent, child.index, |entry| {
                        entry.cwd = child.cwd;
                        entry.role = child.role;
                        entry.nickname = child.nickname;
                    });
                engine
                    .runtime
                    .child_terminals
                    .set_idle(self.surface_id, false);
            } else {
                let label = child.nickname.clone().or_else(|| child.role.clone());
                engine
                    .occupy_soft(self.surface_id, child.parent, label)
                    .map_err(|error| {
                        anyhow::anyhow!("child occupancy install failed: {error:?}")
                    })?;
                engine.runtime.child_terminals.register_child(
                    child.parent,
                    crate::runtime::child_terminal::ChildEntry {
                        child_surface_id: self.surface_id,
                        index: child.index,
                        cwd: child.cwd,
                        role: child.role,
                        nickname: child.nickname,
                    },
                );
            }
            engine.runtime.child_terminals.save();
        }
        let input = if let Some(body) = self.one_shot_input.take() {
            let generation = engine
                .runtime
                .terminals
                .generation(self.surface_id)
                .ok_or_else(|| anyhow::anyhow!("child input has no PTY owner"))?;
            let ack = engine
                .runtime
                .terminals
                .get_mut(self.surface_id)
                .ok_or_else(|| anyhow::anyhow!("child input has no terminal"))?
                .try_send_key_with_ack(&body)
                .map_err(|_| anyhow::anyhow!("child body queue closed after installation"))?;
            submit.then_some(PendingSubmit {
                surface: self.surface_id,
                generation,
                ack,
                started: std::time::Instant::now(),
                settled: None,
            })
        } else {
            None
        };
        // A candidate may have become quiet or exited before its earlier wake was handled.
        // Processing the installed owner preserves its queued OSC/output events for the ordinary drain.
        engine.runtime.terminals.process_surface(self.surface_id);
        Ok(Installed {
            lease: self.lease.clone(),
            #[cfg(feature = "gui")]
            previous_resource: self.previous_resource,
            surface_id: self.surface_id,
            installed_generation: engine.runtime.terminals.generation(self.surface_id),
            retirement: self.retirement.take(),
            remote_retirements: std::mem::take(&mut self.remote_retirements),
            input,
        })
    }
}

impl Installation {
    pub(crate) fn retire_for_release(
        mut self,
        release: &mut crate::runtime::resource_retirement::EngineRelease,
    ) {
        if let Some((terminal, pty)) = self.connection.take() {
            drop(terminal);
            release.retain_pty(pty.retire());
        }
        if self.publication.take().is_some() {
            if let Some(binding) = self.unpublished_remote.take() {
                let (receipt, completion) =
                    crate::plugin_bridge::host_cmd::RemoteRetirementReceipt::pending(
                        self.surface_id,
                        binding,
                    );
                completion.finish(Ok(()));
                release.retain_remote(receipt);
            }
        }
        if let Some(receipt) = self.retirement.take() {
            release.retain_pty(receipt);
        }
        for receipt in self.remote_retirements {
            release.retain_remote(receipt);
        }
        // A consumed publication or one-shot input is never reconstructed or resent here.
    }
}

impl Installed {
    pub(crate) fn retire_for_release(
        mut self,
        release: &mut crate::runtime::resource_retirement::EngineRelease,
    ) {
        if let Some(receipt) = self.retirement.take() {
            release.retain_pty(receipt);
        }
        for receipt in self.remote_retirements {
            release.retain_remote(receipt);
        }
        // The input receipt remains an unknown delivery outcome; disposing its exact PTY is a
        // separate process-owner result and never causes this input to be sent again.
    }
}

impl Installation {
    fn validate_installation_owner(&self, engine: &EngineMut<'_>) -> anyhow::Result<()> {
        if let Some(registration) = &self.registration {
            registration.validate(&engine.runtime.surface_registry)?;
        }
        if engine.runtime.terminals.generation(self.surface_id) != self.previous_resource {
            anyhow::bail!("prepared installation would replace a different physical owner");
        }
        if let Some(child) = &self.child {
            let existing = engine
                .runtime
                .child_terminals
                .find_child(child.parent, child.index);
            if if child.replacing {
                existing.is_none_or(|entry| entry.child_surface_id != self.surface_id)
            } else {
                existing.is_some()
                    || engine
                        .live
                        .occupancy
                        .occupancy_of(self.surface_id)
                        .is_some()
            } {
                anyhow::bail!("reserved child relation or occupancy was replaced");
            }
        }
        Ok(())
    }
    fn install_metadata(&self, engine: &EngineMut<'_>) {
        if !self.metadata.is_empty() {
            let mut memory = crate::poison::recover_mutex(
                engine.runtime.memory.lock(),
                crate::core::MEMORY_WHAT,
                &crate::core::MEMORY_POISONED,
            );
            for (key, value) in &self.metadata {
                if let Err(error) = crate::surface_meta::SurfaceMetaStore::set(
                    &mut *memory,
                    self.surface_id,
                    key,
                    value,
                ) {
                    tracing::warn!(
                        surface = self.surface_id,
                        "surface_meta set failed for key '{key}': {error}"
                    );
                }
            }
        }
    }
}
