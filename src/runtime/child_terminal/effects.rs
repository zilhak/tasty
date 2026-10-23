//! File completion and live occupancy completion have separate owners.
use super::*;
use crate::core::{CoreState, live::LiveDomainState};
use crate::runtime::terminal_store::TerminalStore;
use tasty_terminal::ResourceGeneration;

pub(super) enum Effect {
    Adopt {
        parent: ParentKey,
        child: u32,
        label: Option<String>,
        generation: Option<ResourceGeneration>,
    },
    Release {
        parent: ParentKey,
        child: u32,
    },
}
pub(super) struct PendingEffect {
    receipt: Ticket,
    application: Arc<Mutex<transport::Application>>,
    effect: Effect,
    bytes: usize,
}
impl Drop for PendingEffect {
    fn drop(&mut self) {
        let mut state = self.application.lock().unwrap();
        if matches!(*state, transport::Application::Pending) {
            *state = transport::Application::Retired;
        }
    }
}
impl ChildTerminalRegistry {
    pub(crate) fn adopt_with_effect(
        &mut self,
        parent: ParentKey,
        child: ChildEntry,
        generation: Option<ResourceGeneration>,
    ) -> Result<Ticket, String> {
        let label = child.nickname.clone().or_else(|| child.role.clone());
        let bytes = crate::app::file_writer::command::value_bytes(&parent)
            .saturating_add(std::mem::size_of::<PendingEffect>())
            .saturating_add(label.as_ref().map_or(0, String::len));
        self.check_effect_budget(bytes)?;
        let effect = Effect::Adopt {
            parent: parent.clone(),
            child: child.child_surface_id,
            label,
            generation,
        };
        let receipt = self.adopt(parent, child)?;
        Ok(self.retain_effect(receipt, effect, bytes))
    }
    pub(crate) fn release_with_effect(
        &mut self,
        parent: ParentKey,
        child: u32,
    ) -> Result<Ticket, String> {
        let bytes = crate::app::file_writer::command::value_bytes(&parent)
            .saturating_add(std::mem::size_of::<PendingEffect>());
        self.check_effect_budget(bytes)?;
        let receipt = self.submit()?;
        Ok(self.retain_effect(receipt, Effect::Release { parent, child }, bytes))
    }
    fn check_effect_budget(&self, bytes: usize) -> Result<(), String> {
        let limits = tasty_utils::batch_writer::Limits::default();
        let used = self
            .effects
            .iter()
            .map(|effect| effect.bytes)
            .fold(0usize, usize::saturating_add);
        if self.effects.len() >= limits.queue_items
            || bytes > limits.queue_bytes.saturating_sub(used)
        {
            return Err("child registry live continuation budget exceeded".into());
        }
        Ok(())
    }
    fn retain_effect(&mut self, receipt: Ticket, effect: Effect, bytes: usize) -> Ticket {
        let (public, application) = receipt.with_application();
        self.effects.push_back(PendingEffect {
            receipt,
            application,
            effect,
            bytes,
        });
        public
    }
    pub(crate) fn poll_effects(
        &mut self,
        core: &CoreState,
        live: &mut LiveDomainState,
        terminals: &TerminalStore,
    ) {
        self.hub.poll();
        while let Some(pending) = self.effects.front() {
            let Some(result) = pending.receipt.observation() else {
                break;
            };
            let pending = self.effects.pop_front().unwrap();
            let applied = match result {
                Err(error) => Err(error),
                Ok(_) => {
                    match &pending.effect {
                        Effect::Adopt {
                            parent,
                            child,
                            label,
                            generation,
                        } => {
                            if !core.live_surface_ids().contains(child)
                                || terminals.generation(*child) != *generation
                                || self.parent_of_child(*child).as_ref() != Some(parent)
                            {
                                Err("relationship saved but live adoption target retired or changed".into())
                            } else {
                                live.occupancy.acquire_soft(*child,parent.clone(),label.clone())
                                .map_err(|error|format!("relationship saved but occupancy install failed: {error:?}"))
                            }
                        }
                        Effect::Release { parent, child } => {
                            if let Err(error) = live.occupancy.release_soft(*child, parent) {
                                tracing::warn!(
                                    ?error,
                                    "child relationship removed; previous soft occupancy unavailable"
                                );
                            }
                            Ok(())
                        }
                    }
                }
            };
            *pending.application.lock().unwrap() = transport::Application::Applied(applied);
        }
    }
}
