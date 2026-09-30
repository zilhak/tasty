//! Installation consumes the private candidate after its structural result is committed.

use super::*;
use tasty_terminal::{PtyPhase, PtyRetirement};

pub(crate) struct Installation {
    pub(super) lease: EffectLease,
    pub(super) surface_id: u32,
    pub(super) previous_resource: Option<ResourceGeneration>,
    pub(super) connection: Option<(Terminal, Pty)>,
    pub(super) publication: Option<PublicationAction>,
}

pub(crate) struct Installed {
    pub(crate) lease: EffectLease,
    retirement: Option<PtyRetirement>,
}

impl Installed {
    /// Signalling/removing an old owner is not evidence that the OS child was reaped.
    pub(crate) fn cleanup_complete(&self) -> anyhow::Result<bool> {
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
    /// The caller keeps the entire affected batch hidden until the completion commit is applied.
    /// A failure after external registration requires halt/reconciliation, never legacy fallback.
    pub(crate) fn install(
        self,
        engine: &mut EngineMut<'_>,
        mut plugins: Option<&mut crate::plugin::PluginManager>,
        retired_kind: Option<&dyn crate::model::Surface>,
    ) -> anyhow::Result<Installed> {
        if engine.runtime.terminals.generation(self.surface_id) != self.previous_resource {
            anyhow::bail!("prepared installation would replace a different physical owner");
        }
        if let Some(old) = retired_kind {
            if old.surface_id() != Some(self.surface_id) {
                anyhow::bail!("retired kind belongs to another surface");
            }
            if let Some(remote) = old
                .as_any()
                .downcast_ref::<crate::plugin_bridge::remote_surface::RemoteSurface>()
            {
                let manager = plugins
                    .as_deref_mut()
                    .ok_or_else(|| anyhow::anyhow!("remote retirement has no plugin host"))?;
                manager
                    .enqueue_bound_remote_retirement(self.surface_id, remote.handles().binding())
                    .map_err(anyhow::Error::msg)?;
            } else if let Some(manager) = plugins.as_deref_mut() {
                manager.destroy_remote_surface(self.surface_id, Some(old.kind()));
            }
        }
        // Destroy of the old registration precedes enqueueing Created/Restored for the new one.
        if let Some(publication) = self.publication {
            publication()?;
        }
        let previous = match self.connection {
            Some((terminal, pty)) => {
                engine
                    .runtime
                    .terminals
                    .replace(self.surface_id, terminal, Some(pty))
            }
            None => engine.runtime.terminals.remove(self.surface_id),
        };
        let retirement = previous.and_then(|(terminal, pty)| {
            drop(terminal);
            pty.map(Pty::retire)
        });
        // A candidate may have become quiet or exited before its earlier wake was handled.
        // Processing the installed owner preserves its queued OSC/output events for the ordinary drain.
        engine.runtime.terminals.process_surface(self.surface_id);
        Ok(Installed {
            lease: self.lease,
            retirement,
        })
    }
}
