//! Retire headless observer producers before closing their memory writer.
use std::time::{Duration, Instant};

pub(super) fn drain(
    app: &mut crate::app::App,
    session: &mut crate::runtime::engine_session::EngineSession,
) {
    let started = Instant::now();
    let mut drain = crate::app::writer_drain::WriterDrain::default();
    session.observer_router.begin_shutdown();
    let mut reported_wait = false;
    loop {
        let progress = session.observer_router.poll_shutdown();
        if progress.pending != 0 && !std::mem::replace(&mut reported_wait, true) {
            tracing::info!(target: "tasty::shutdown", pending = progress.pending, "observer_sink_wait_started");
        }
        app.services.poll_memory_writer();
        super::file_drain::collect(app, &mut || {
            session.borrow_mut().poll_child_registry_effects()
        });
        if let Some(ipc) = app.hub.ipc_server.as_ref() {
            while let Ok(command) = ipc.try_recv() {
                let id = command
                    .request
                    .id
                    .clone()
                    .unwrap_or(serde_json::Value::Null);
                crate::ipc::server::send_response(
                    &command.response_tx,
                    crate::ipc::protocol::JsonRpcResponse::error(
                        id,
                        -32000,
                        "host is shutting down",
                    ),
                );
            }
        }
        if !drain.keep_waiting(
            progress.pending,
            Instant::now(),
            "headless observer shutdown timed out; sink workers remain unjoined",
        ) {
            session.observer_router.fence_submissions();
            progress.record_outcome(started.elapsed());
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use crate::app::preset_shutdown_tests::{make_headless, run_case};

    #[test]
    fn observer_drain_collects_original_preset_reply_before_return() {
        run_case(
            make_headless,
            |app| {
                let mut session = crate::boot::file_drain::composition_tests::engine(app);
                super::drain(app, &mut session);
            },
            false,
        );
    }

    #[test]
    fn observer_drain_collects_ready_preset_reply_with_later_file_work() {
        run_case(
            make_headless,
            |app| {
                let mut session = crate::boot::file_drain::composition_tests::engine(app);
                super::drain(app, &mut session);
            },
            true,
        );
    }
}
