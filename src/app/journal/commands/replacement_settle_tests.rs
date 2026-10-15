//! A replacement publishes its structure without waiting for the replaced owner to retire, and an
//! installation failure leaves only its own target unavailable instead of pausing observation.
//! Every scenario runs the public respawn through the production journal pump.
use super::*;
use crate::plugin_bridge::host_cmd::HostCmd;
use crate::plugin_bridge::remote_surface::RemoteSurface;
use crate::runtime::effect_runner::{INSTALL_FAULT, InstallFault};

const PLUGIN: &str = "com.test.replacement-settle";

fn terminal_workspace(journal: &mut JournalApplication, session: &mut EngineSession) -> u32 {
    let rx = send(
        journal,
        request(
            "workspace.create",
            serde_json::json!({"name":"replaced"}),
            None,
            700,
        ),
    );
    assert!(finish(journal, session, &rx).error.is_none());
    session
        .core_state
        .workspaces()
        .iter()
        .last()
        .unwrap()
        .all_surface_ids()[0]
}

fn respawn(journal: &mut JournalApplication, surface: u32, id: u64) -> Receiver {
    send(
        journal,
        request(
            "surface.respawn_terminal",
            serde_json::json!({"surface_id":surface}),
            None,
            id,
        ),
    )
}

type Receiver = std::sync::mpsc::Receiver<JsonRpcResponse>;

/// One production pump turn: the worker pump, then the window's resolution of new requests.
fn turn(
    journal: &mut JournalApplication,
    session: &mut EngineSession,
    plugins: Option<&mut crate::plugin::PluginManager>,
    services: &crate::app::services::AppServices,
) {
    journal
        .poll_bootstrap(&mut [&mut *session], plugins)
        .unwrap();
    for (ticket, request) in journal.requests_needing_resolution() {
        match request.method.as_str() {
            "surface.respawn_terminal" => {
                journal.resolve_public_creation(ticket, session, services)
            }
            "workspace.create" => journal.resolve_workspace_creation(ticket, session, None),
            _ => journal.resolve_ipc_for_engine(ticket, session),
        }
    }
}

fn reply(
    journal: &mut JournalApplication,
    session: &mut EngineSession,
    rx: &Receiver,
    mut observe: impl FnMut(&JournalApplication, &EngineSession),
) -> JsonRpcResponse {
    let services = crate::ipc::handler::cli_entry_tests::test_core();
    let mut stall = StallBudget::new(journal);
    loop {
        turn(journal, session, None, &services);
        observe(journal, session);
        if let Ok(response) = rx.try_recv() {
            return response;
        }
        stall.nap("replacement reply");
    }
}

fn inject(fault: InstallFault) {
    INSTALL_FAULT.with(|slot| slot.set(Some(fault)));
}

fn injected() -> Option<InstallFault> {
    INSTALL_FAULT.with(|slot| slot.get())
}

fn activation(session: &EngineSession, surface: u32) -> Option<u64> {
    session
        .core_state
        .find_surface_by_id(surface)
        .and_then(|descriptor| descriptor.activation_generation)
}

#[cfg(unix)]
fn process_exists(pid: u32) -> bool {
    // SAFETY: signal 0 only checks that the PID exists; it delivers nothing.
    unsafe { libc::kill(pid as i32, 0) == 0 }
}

#[test]
fn a_failure_before_any_owner_changes_rejects_the_candidate_and_keeps_the_old_owner() {
    let (mut session, mut journal) = boot();
    let surface = terminal_workspace(&mut journal, &mut session);
    let old = session.runtime.terminals.generation(surface).unwrap();
    let before = activation(&session, surface);
    inject(InstallFault::BeforeChange);
    let rx = respawn(&mut journal, surface, 701);
    let response = reply(&mut journal, &mut session, &rx, |_, _| {});
    assert_eq!(
        injected(),
        None,
        "the production installation reached the fault"
    );
    let error = response.error.expect("a known rejection is a failed reply");
    assert!(error.message.contains("injected"), "{error:?}");
    assert_ne!(
        error.code,
        crate::ipc::protocol::ERR_OPERATION_RECOVERY_REQUIRED
    );
    assert_eq!(session.runtime.terminals.generation(surface), Some(old));
    assert_eq!(activation(&session, surface), before);
    assert!(session.pending_materializations.is_empty());
    assert!(!journal.pauses_observation());
    assert!(!journal.is_halted());
}

#[test]
fn a_failure_after_the_swap_withdraws_the_candidate_and_marks_only_its_target() {
    let (mut session, mut journal) = boot();
    let surface = terminal_workspace(&mut journal, &mut session);
    let other = terminal_workspace(&mut journal, &mut session);
    let other_generation = session.runtime.terminals.generation(other);
    let before = activation(&session, surface);
    inject(InstallFault::AfterSwap);
    let rx = respawn(&mut journal, surface, 702);
    let mut uncertain_seen = false;
    let response = reply(&mut journal, &mut session, &rx, |journal, _| {
        if injected().is_none() {
            uncertain_seen = true;
            // The withdrawn replacement never pauses observation while it reconciles.
            assert!(!journal.pauses_observation());
        }
    });
    assert!(uncertain_seen);
    let error = response.error.expect("the withdrawn replacement failed");
    assert!(error.message.contains("injected"), "{error:?}");
    assert_eq!(
        activation(&session, surface),
        before,
        "the structure is unchanged"
    );
    assert!(
        session.runtime.terminals.generation(surface).is_none(),
        "neither the retired owner nor the withdrawn candidate is live"
    );
    assert!(
        crate::runtime::surface_restorer::recovery_blocked_reason(&session, surface).is_some(),
        "the target whose owner is gone is shown as unavailable"
    );
    assert_eq!(
        session.runtime.terminals.generation(other),
        other_generation
    );
    assert!(session.pending_materializations.is_empty());
    assert!(!journal.is_halted());
    let next = send(
        &mut journal,
        request("workspace.create", serde_json::json!({}), None, 703),
    );
    assert!(finish(&mut journal, &mut session, &next).error.is_none());
}

#[cfg(unix)]
#[test]
fn the_reply_of_a_published_respawn_follows_the_reap_of_the_replaced_process() {
    let (mut session, mut journal) = boot();
    let surface = terminal_workspace(&mut journal, &mut session);
    let old = session.runtime.terminals.generation(surface).unwrap();
    let pid = session
        .runtime
        .terminals
        .pty(surface)
        .and_then(|pty| pty.process_id())
        .expect("old shell process");
    let rx = respawn(&mut journal, surface, 704);
    let response = reply(&mut journal, &mut session, &rx, |_, _| {});
    assert!(response.error.is_none(), "{response:?}");
    assert_ne!(session.runtime.terminals.generation(surface), Some(old));
    assert!(
        !process_exists(pid),
        "the reply is not delivered before the replaced process is reaped"
    );
    assert!(!journal.pauses_observation());
    assert!(session.engine_release.is_none());
}

#[test]
fn an_unacknowledged_plugin_destroy_neither_pauses_observation_nor_reports_success() {
    let (mut session, mut journal) = boot();
    let surface = terminal_workspace(&mut journal, &mut session);
    // Bind the published surface to a remote owner whose stub never acknowledges a destroy.
    let mut plugins = crate::plugin::PluginManager::with_registries(
        std::sync::Arc::new(tasty_terminal::waker_factory::NoopWakerFactory),
        std::sync::Arc::new(crate::file::format::FileFormatRegistry::new()),
        std::sync::Arc::new(crate::file::handler::FileHandlerRegistry::new()),
    );
    let stub = plugins.attach_namespace_stub_for_test(PLUGIN, "replacement_settle");
    let remote = RemoteSurface::new(surface, "terminal", PLUGIN.into(), "remote".into());
    plugins
        .host_cmd_tx
        .send(HostCmd::RemoteSurfaceCreated {
            surface_id: surface,
            plugin_id: PLUGIN.into(),
            kind: "terminal".into(),
            cwd: None,
            params: serde_json::json!({}),
            handles: remote.handles(),
        })
        .unwrap();
    session.runtime.surfaces.insert(surface, Box::new(remote));
    plugins.pump(Instant::now());
    assert_eq!(stub.drain_invokes().len(), 1, "the create was published");
    let before = activation(&session, surface);

    let rx = respawn(&mut journal, surface, 705);
    let services = crate::ipc::handler::cli_entry_tests::test_core();
    let mut stall = StallBudget::new(&journal);
    let mut published_while_retiring = false;
    let response = loop {
        turn(&mut journal, &mut session, Some(&mut plugins), &services);
        if activation(&session, surface) != before {
            // The new activation is published while the old destroy is still unanswered.
            assert!(!journal.pauses_observation());
            published_while_retiring = true;
        }
        if let Ok(response) = rx.try_recv() {
            break response;
        }
        stall.nap("unacknowledged replaced owner");
    };
    assert!(published_while_retiring);
    let error = response
        .error
        .expect("an unconfirmed retirement is not a success");
    assert_eq!(
        error.code,
        crate::ipc::protocol::ERR_OPERATION_RECOVERY_REQUIRED
    );
    assert!(session.runtime.terminals.generation(surface).is_some());
    assert!(
        session.engine_release.is_some(),
        "the unconfirmed receipt has one later owner"
    );
    assert!(!journal.has_creation(session.id));
    assert!(!journal.pauses_observation());
}
