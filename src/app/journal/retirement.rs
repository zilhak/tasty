//! A discarded slot is durably retired before its process owners are released or reused.
use super::*;
use crate::runtime::journal_product::EngineBinding;

pub(super) struct PendingRetirement {
    binding: EngineBinding,
    ticket: Option<u64>,
    release_owner: bool,
    preserve_stream: bool,
}

#[derive(Default)]
pub(super) struct Retirements {
    pub(super) pending: HashMap<EngineId, PendingRetirement>,
    completed: Vec<EngineId>,
}

impl JournalApplication {
    pub(crate) fn begin_first_gui_engine(
        &mut self,
        session: &EngineSession,
        selection: EngineSelection,
    ) -> Result<(), String> {
        self.begin_engine(session, selection)?;
        self.opening
            .get_mut(&session.id)
            .expect("opening first engine")
            .select_available_slot = true;
        Ok(())
    }

    pub(crate) fn known_layout_slots(&self) -> impl Iterator<Item = u32> + '_ {
        self.known_slots
            .iter()
            .filter_map(|(slot, retired)| (!retired).then_some(*slot))
    }

    pub(crate) fn layout_slot_retired(&self, slot: u32) -> bool {
        self.known_slots.get(&slot).copied().unwrap_or(false)
    }

    pub(crate) fn retire_engine(
        &mut self,
        id: EngineId,
        binding: EngineBinding,
        release_owner: bool,
    ) {
        self.restorations.retire(id);
        self.view_writes.cancel(&binding.stream);
        self.retirements
            .pending
            .entry(id)
            .or_insert(PendingRetirement {
                binding,
                ticket: None,
                release_owner,
                preserve_stream: false,
            });
        (self.wake)();
    }

    pub(crate) fn has_pending_retirements(&self) -> bool {
        !self.retirements.pending.is_empty() && !self.is_halted()
    }

    pub(crate) fn take_retired_engines(&mut self) -> Vec<EngineId> {
        std::mem::take(&mut self.retirements.completed)
    }

    /// Failed View creation must not delete a successfully resumed slot. Only its process owner ends.
    pub(crate) fn release_failed_opening(&mut self, id: EngineId, binding: EngineBinding) {
        self.retire_engine(id, binding, true);
        if let Some(pending) = self.retirements.pending.get_mut(&id) {
            pending.preserve_stream = true;
        }
    }

    pub(crate) fn forget_released_engine(&mut self, id: EngineId) {
        self.opening.remove(&id);
        self.restored_views.remove(&id);
        self.restorations.forget(id);
    }

    // Drain accepted work while its exact owner remains hidden and alive.
    pub(super) fn submit_retirements(&mut self) -> Result<(), String> {
        let ready: Vec<_> = self
            .retirements
            .pending
            .iter()
            .filter_map(|(id, pending)| {
                (pending.ticket.is_none()
                    && !self.has_pending_engine_effects(*id)
                    && !self.restorations.has_reads(*id))
                .then_some(*id)
            })
            .take(8)
            .collect();
        for id in ready {
            if self
                .retirements
                .pending
                .get(&id)
                .is_some_and(|pending| pending.preserve_stream)
            {
                self.retirements.pending.remove(&id);
                self.retirements.completed.push(id);
                continue;
            }
            let retirement = self
                .retirements
                .pending
                .get_mut(&id)
                .ok_or("retiring owner disappeared")?;
            let ticket = self.next_ticket;
            match self.worker.submit(Request {
                ticket,
                work: Work::RetireEngine(retirement.binding.clone()),
            }) {
                Ok(()) => {
                    retirement.ticket = Some(ticket);
                    self.next_ticket += 1;
                }
                Err(crate::runtime::journal_product::SubmitError::Busy) => break,
                Err(error) => return Err(format!("retirement submission failed: {error:?}")),
            }
        }
        Ok(())
    }

    pub(super) fn finish_retirement(
        &mut self,
        ticket: u64,
        result: &Result<ResultValue, String>,
    ) -> Result<bool, String> {
        let Some(id) = self
            .retirements
            .pending
            .iter()
            .find_map(|(id, pending)| (pending.ticket == Some(ticket)).then_some(*id))
        else {
            return Ok(false);
        };
        match result {
            Ok(ResultValue::Executed(_)) => {}
            Err(error) => return Err(format!("engine retirement failed: {error}")),
            _ => return Err("engine retirement returned another completion".into()),
        }
        let pending = self
            .retirements
            .pending
            .remove(&id)
            .expect("retirement ticket");
        if pending.release_owner {
            self.retirements.completed.push(id);
        }
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    fn pump(
        journal: &mut JournalApplication,
        session: &mut EngineSession,
        ready: impl Fn(&JournalApplication) -> bool,
    ) {
        let until = Instant::now() + Duration::from_secs(10);
        loop {
            journal.poll_bootstrap(&mut [session], None).unwrap();
            journal.poll_restore_bootstrap(session).unwrap();
            if ready(journal) {
                return;
            }
            assert!(Instant::now() < until, "slot lifecycle stalled");
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    #[test]
    fn first_boot_prefers_a_live_journal_slot_and_explicit_reuse_advances_retired_incarnation() {
        let mut settings = crate::settings::Settings::default();
        settings.general.shell = "/bin/sh".into();
        settings.general.startup_command = "exec sleep 60".into();
        let mut session = EngineSession::new_with_ids_and_settings(
            80,
            24,
            Arc::new(|| {}),
            None,
            Some(1),
            Arc::new(std::sync::Mutex::new(
                tasty_memory::testing::InMemoryStorage::new(),
            )),
            Arc::new(tasty_task_runtime::RunnerRegistry::new()),
            settings,
        )
        .unwrap();
        let home = tasty_utils::path::tasty_home().unwrap();
        std::fs::create_dir(home.join("layouts")).unwrap();
        for slot in [1, 2] {
            let layout = serde_json::json!({"version":2,"active_workspace":0,"workspaces":[{"name":format!("slot-{slot}"),"subtitle":"","description":"","focused_pane_index":0,"pane_layout":{"Leaf":{"tabs":[{"name":"empty","explicit_name":null,"surface":{"Leaf":{"Generic":{"kind":"empty","data":null}}}}],"active_tab":0}}}]});
            std::fs::write(
                home.join(format!("layouts/{slot:02}.json")),
                layout.to_string(),
            )
            .unwrap();
        }
        let id = session.id;
        let mut bindings = Vec::new();
        let mut surface_ids = Vec::new();
        for slot in [1, 2] {
            let mut journal = JournalApplication::new(Arc::new(|| {})).unwrap();
            session.persistence.slot = Some(slot);
            journal
                .begin_engine(&session, EngineSelection::Slot { slot, resume: true })
                .unwrap();
            pump(&mut journal, &mut session, |j| j.is_ready(id));
            bindings.push(session.journal_binding.clone().unwrap());
            surface_ids.push(session.core_state.local_workspaces()[0].all_surface_ids()[0]);
            if slot == 1 {
                journal.retire_engine(id, bindings[0].clone(), true);
                assert!(
                    journal.take_retired_engines().is_empty(),
                    "submission is not durable retirement"
                );
                pump(&mut journal, &mut session, |j| !j.has_pending_retirements());
                assert_eq!(journal.take_retired_engines(), vec![id]);
                assert!(journal.layout_slot_retired(1));
                assert!(!home.join("layouts/01.json").exists());
            }
            drop(journal);
            // Reset only the test projection; retained runtime owners still belong to this session.
            session.core_state = crate::core::CoreState::new_base();
            session.journal_binding = None;
        }
        std::fs::remove_dir_all(home.join("layouts")).unwrap();
        session.persistence.slot = Some(1); // Early legacy-file selection has no journal data yet.
        let mut journal = JournalApplication::new(Arc::new(|| {})).unwrap();
        journal
            .begin_first_gui_engine(
                &session,
                EngineSelection::Slot {
                    slot: 1,
                    resume: true,
                },
            )
            .unwrap();
        pump(&mut journal, &mut session, |j| j.is_ready(id));
        assert_eq!(session.persistence.slot, Some(2));
        assert_eq!(
            session.journal_binding.as_ref().unwrap().stream,
            "structure:slot-2"
        );
        assert_eq!(
            session.journal_binding.as_ref().unwrap().incarnation,
            bindings[1].incarnation
        );
        assert_eq!(
            session.core_state.local_workspaces()[0].all_surface_ids(),
            vec![surface_ids[1]]
        );
        assert_eq!(journal.known_layout_slots().collect::<Vec<_>>(), vec![2]);
        drop(journal);
        // Reset only the test projection; retained runtime owners still belong to this session.
        session.core_state = crate::core::CoreState::new_base();
        session.journal_binding = None;
        session.persistence.slot = Some(1);
        let mut journal = JournalApplication::new(Arc::new(|| {})).unwrap();
        journal
            .begin_engine(
                &session,
                EngineSelection::Slot {
                    slot: 1,
                    resume: true,
                },
            )
            .unwrap();
        pump(&mut journal, &mut session, |j| j.is_ready(id));
        assert_eq!(
            session.journal_binding.as_ref().unwrap().incarnation,
            bindings[0].incarnation + 1
        );
        assert!(!session.core_state.has_surface(surface_ids[0]));
        assert_eq!(
            session.runtime.terminals.iter().count(),
            1,
            "fresh default materializes only after reuse commit"
        );
    }

    #[test]
    fn retirement_drains_an_accepted_materialization_before_releasing_its_owner() {
        let mut settings = crate::settings::Settings::default();
        settings.general.shell = "/bin/sh".into();
        settings.general.startup_command = "exec sleep 60".into();
        let mut session = EngineSession::new_with_ids_and_settings(
            80,
            24,
            Arc::new(|| {}),
            None,
            Some(1),
            Arc::new(std::sync::Mutex::new(
                tasty_memory::testing::InMemoryStorage::new(),
            )),
            Arc::new(tasty_task_runtime::RunnerRegistry::new()),
            settings,
        )
        .unwrap();
        let id = session.id;
        let mut journal = JournalApplication::new(Arc::new(|| {})).unwrap();
        journal
            .begin_engine(
                &session,
                EngineSelection::Slot {
                    slot: 1,
                    resume: false,
                },
            )
            .unwrap();
        pump(&mut journal, &mut session, |j| j.is_ready(id));
        let ticket = journal.next_ticket;
        journal.next_ticket += 1;
        let creation =
            creation::Creation::default_workspace(ticket, &session, &journal.worker).unwrap();
        journal.creations.insert((id, ticket), creation);
        journal.retire_engine(id, session.journal_binding.clone().unwrap(), true);
        journal.submit_retirements().unwrap();
        assert!(journal.retirements.pending[&id].ticket.is_none());
        assert!(!journal.activate_restored_surface(&session, 999).unwrap());
        assert!(journal.take_retired_engines().is_empty());
        pump(&mut journal, &mut session, |j| !j.has_pending_retirements());
        assert!(session.pending_materializations.is_empty());
        assert!(!journal.has_creation(id));
        assert_eq!(
            session.runtime.terminals.iter().count(),
            2,
            "both accepted owners survive until retirement publication"
        );
        assert!(journal.layout_slot_retired(1));
        assert_eq!(journal.take_retired_engines(), vec![id]);
    }
}
