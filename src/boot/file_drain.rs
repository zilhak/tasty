//! File writer drain after the headless producers have stopped.
pub(super) fn drain(app: &mut crate::app::App) {
    let mut drain = crate::app::writer_drain::WriterDrain::default();
    loop {
        reject_new_calls(app.hub.ipc_server.as_deref());
        let remaining = app.services.poll_file_writer_shutdown();
        app.finish_preset_files();
        crate::intent::headless::settle_settings_saves(&mut app.services);
        app.services
            .poll_memory_replies(app.plugin_manager.as_mut());
        if !drain.observe_writer(
            remaining,
            app.services.file_writer_shutdown_report(),
            "file",
            std::time::Instant::now(),
            "headless file writer shutdown timed out; settings it has not saved may be lost",
        ) {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}

// Match GUI shutdown: a saved HTTP request may still await its injected sequence's answer.
// Reject new work rather than dispatching into already retired engines or abandoning its owner.
fn reject_new_calls(server: Option<&dyn crate::ports::ipc_server::IpcServerPort>) {
    let Some(server) = server else {
        return;
    };
    while let Ok(cmd) = server.try_recv() {
        let id = cmd.request.id.clone().unwrap_or(serde_json::Value::Null);
        crate::ipc::server::send_response(
            &cmd.response_tx,
            crate::ipc::protocol::JsonRpcResponse::error(id, -32000, "host is shutting down"),
        );
    }
}

#[cfg(test)]
mod webhook_tests {
    use std::{
        sync::Arc,
        time::{Duration, Instant},
    };
    #[test]
    fn a_saved_http_producer_gets_its_injected_call_answered_during_headless_drain() {
        let server = crate::adapters::test::mock_ipc_server::MockIpcServer::new();
        let injector =
            tasty_ipc::host_call::HostIpcInjector::new(server.tx_clone(), Arc::new(|| {}));
        let job = std::thread::spawn(move || {
            injector.dispatch(
                "notification.create",
                serde_json::json!({"body":"owned-shutdown"}),
                Duration::from_secs(5),
            )
        });
        let until = Instant::now() + Duration::from_secs(5);
        while !job.is_finished() {
            super::reject_new_calls(Some(&server));
            assert!(
                Instant::now() < until,
                "HTTP producer was left waiting during drain"
            );
            std::thread::yield_now();
        }
        assert!(
            matches!(job.join().unwrap(),Err(tasty_ipc::host_call::InjectError::Rpc {code:-32000,message}) if message=="host is shutting down")
        );
    }
}

#[cfg(all(test, target_os = "linux"))]
mod capture_file_tests {
    use crate::app::preset_shutdown_tests::{make_headless, run_case};
    #[test]
    fn app_capture_reply_is_collected_before_final_zero_return() {
        run_case(make_headless, super::drain, false);
    }
    #[test]
    fn app_capture_reply_is_collected_before_timeout_with_later_work() {
        run_case(make_headless, super::drain, true);
    }
}
