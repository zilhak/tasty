//! File writer drain after the headless producers have stopped.
pub(super) fn drain(app: &mut crate::app::App) {
    let mut drain = crate::app::writer_drain::WriterDrain::default();
    loop {
        let remaining = app.services.poll_file_writer_shutdown();
        app.finish_preset_files();
        crate::intent::headless::settle_settings_saves(&mut app.services);
        app.services
            .poll_memory_replies(app.plugin_manager.as_mut());
        if !drain.observe_writer(
            remaining,
            app.services.file_writer_shutdown_observation(),
            "file",
            std::time::Instant::now(),
            "headless file writer shutdown timed out; settings it has not saved may be lost",
        ) {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
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
