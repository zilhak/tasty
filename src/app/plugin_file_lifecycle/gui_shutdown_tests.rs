use super::*;
use crate::app::{plugin_file_lifecycle::shutdown_tests as cases, preset_shutdown_tests::make_gui};
use winit::platform::x11::EventLoopBuilderExtX11;
fn start(app: &mut App, file_drain: crate::app::writer_drain::WriterDrain) {
    app.stop_plugin_file_producers();
    app.state.shutdown = Some(ShutdownState {
        exit_code: 0,
        save_final_view: false,
        phase: ShutdownPhase::StoppingPlugins,
        final_view_sequence: None,
        port_scan_deadline: None,
        profile_detection_deadline: None,
        screenshot_deadline: None,
        explorer_files_deadline: None,
        runner_stop_deadline: Instant::now(),
        observer_drain: Default::default(),
        observer_started: None,
        observer_reported: false,
        memory_drain: Default::default(),
        state_drain: Default::default(),
        file_drain,
        journal_deadline: None,
    });
}
fn drain(app: &mut App, file_drain: crate::app::writer_drain::WriterDrain) {
    start(app, file_drain);
    let end = Instant::now() + Duration::from_secs(8);
    loop {
        app.reject_pending_ipc();
        if !app.wait_for_file_writer_shutdown() {
            break;
        }
        assert!(Instant::now() < end);
        std::thread::sleep(Duration::from_millis(1));
    }
}
fn observer(app: &mut App) {
    start(app, Default::default());
    assert!(!app.wait_for_observer_shutdown());
}
#[test]
fn plugin_file_shutdown_actual_app_pipeline_final_zero_and_timeout() {
    let subscriber = tracing_subscriber::fmt()
        .with_ansi(false)
        .with_writer(std::io::stderr)
        .finish();
    let _trace = tracing::subscriber::set_default(subscriber);
    let event_loop = winit::event_loop::EventLoop::<crate::AppEvent>::with_user_event()
        .with_x11()
        .with_any_thread(true)
        .build()
        .unwrap();
    let make = || make_gui(event_loop.create_proxy());
    cases::mixed_child_webhook_and_plugin_pipeline(make, |app, session| {
        app.engines
            .insert_pending(session)
            .unwrap_or_else(|_| panic!("owned engine slot"));
        drain(app, Default::default());
    });
    crate::app::preset_shutdown_tests::run_case(make, observer, false);
    crate::app::preset_shutdown_tests::run_case(make, observer, true);
    cases::remove_pipeline(make, drain);
    cases::final_receipt(make, drain, false);
    cases::final_receipt(make, drain, true);
    cases::active_unknown_and_queued_refusal(make, drain);
    cases::deadline_rechecks_ready_receipt(make);
    cases::retirement_has_an_independent_poll_wake(make);
    crate::app::plugin_file_lifecycle::native_tests::gui_only_consumers(make);
}
