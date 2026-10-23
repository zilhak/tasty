//! File writer drain after the headless producers have stopped.
pub(super) fn drain(app: &mut crate::app::App) {
    let mut drain = crate::app::writer_drain::WriterDrain::default();
    loop {
        let remaining = app.services.poll_file_writer_shutdown();
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
