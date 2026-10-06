//! A close that reaches its receipt wait no longer pauses observation, so the remote retirements
//! its owner enqueued must already be applied when observation resumes.
use super::*;
use crate::plugin_bridge::host_cmd::HostCmd;
use crate::plugin_bridge::remote_surface::RemoteSurface;

const PLUGIN: &str = "com.test.receipt-wait";

#[test]
fn entering_the_receipt_wait_applies_the_remote_retirement_before_observation_resumes() {
    let (mut session, mut journal) = boot();
    let rx = send(
        &mut journal,
        request(
            "workspace.create",
            serde_json::json!({"name":"remote-owner"}),
            None,
            901,
        ),
    );
    assert!(finish(&mut journal, &mut session, &rx).error.is_none());
    let surface = session
        .core_state
        .workspaces()
        .iter()
        .last()
        .unwrap()
        .all_surface_ids()[0];

    // Swap the materialized owner for a remote one published to a stub that never acknowledges.
    let mut plugins = crate::plugin::PluginManager::with_registries(
        std::sync::Arc::new(tasty_terminal::waker_factory::NoopWakerFactory),
        std::sync::Arc::new(crate::file::format::FileFormatRegistry::new()),
        std::sync::Arc::new(crate::file::handler::FileHandlerRegistry::new()),
    );
    let stub = plugins.attach_namespace_stub_for_test(PLUGIN, "receipt_wait");
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

    let rx = send(
        &mut journal,
        request(
            "surface.close",
            serde_json::json!({"surface_id":surface}),
            None,
            902,
        ),
    );
    let mut stall = StallBudget::new(&journal);
    let mut sent = 0;
    loop {
        journal
            .poll_bootstrap(&mut [&mut session], Some(&mut plugins))
            .unwrap();
        for (ticket, _) in journal.requests_needing_resolution() {
            journal.resolve_ipc_for_engine(ticket, &session);
        }
        sent += stub.drain_invokes().len();
        // No plugin pump runs in this loop. Once the cleanup runs without pausing observation,
        // only the claim-to-running transition can have sent the destroy request.
        if journal.cleanup_awaits_any_receipts() && !journal.pauses_observation() {
            assert_eq!(
                sent, 1,
                "the destroy request was sent when the cleanup started running"
            );
        }
        if rx.try_recv().is_ok() {
            // The stub never acknowledges, so the reply itself is not asserted here.
            assert_eq!(sent, 1, "the destroy request preceded the reply");
            return;
        }
        stall.nap("close receipt wait");
    }
}
