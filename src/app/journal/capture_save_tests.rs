use super::*;
use std::{
    sync::{Arc, Mutex, mpsc},
    time::{Duration, Instant},
};
use tasty_utils::{batch_writer, file_replace};

#[test]
fn journal_capture_handoff_waits_for_file_receipt_before_original_response() {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir(root.path().join("workspace")).unwrap();
    let path = root.path().join("workspace/captured.toml");
    let cache = Arc::new(Mutex::new(tasty_presets::PresetStore::load_from(
        root.path().into(),
    )));
    let writer = crate::app::file_writer::start(
        Arc::new(tasty_settings::FileSettingsStorage),
        batch_writer::Limits::default(),
        Arc::new(batch_writer::SystemClock),
        Arc::new(|| {}),
    )
    .unwrap();
    let mut core = crate::adapters::ipc::handler::cli_entry_tests::test_core_builder()
        .with_preset_store(cache.clone())
        .with_file_writer(writer)
        .build()
        .unwrap();
    let held = file_replace::lock(&path).unwrap();
    let (sender, receiver) = mpsc::sync_channel(2);
    let completion = PresetCaptureCompletion {
        #[cfg(feature = "gui")]
        engine: EngineId::for_test(),
        #[cfg(feature = "gui")]
        binding: crate::runtime::journal_product::EngineBinding {
            journal_id: "test".into(),
            stream: "test".into(),
            incarnation: 1,
            runtime_epoch: 1,
            published_cut: None,
            revision: None,
        },
        reply: PresetCaptureReply::Ipc {
            id: serde_json::json!(47),
            sender,
            name: Some("captured".into()),
        },
        result: Ok((
            crate::app::preset_commands::tests::value("captured"),
            "base".into(),
        )),
    };
    let mut pending = completion.save(&core);
    assert!(pending.poll().is_none());
    assert!(!pending.is_finished());
    assert!(receiver.try_recv().is_err());
    assert!(cache.lock().unwrap().get_workspace("captured").is_none());
    drop(held);
    let deadline = Instant::now() + Duration::from_secs(5);
    while !pending.is_finished() {
        core.poll_file_writer();
        assert!(pending.poll().is_none());
        assert!(Instant::now() < deadline, "capture save did not finish");
        std::thread::yield_now();
    }
    let response = receiver.try_recv().unwrap();
    assert_eq!(response.id, serde_json::json!(47));
    assert_eq!(
        response.result.unwrap(),
        serde_json::json!({"name":"captured"})
    );
    assert_eq!(
        cache
            .lock()
            .unwrap()
            .get_workspace("captured")
            .unwrap()
            .subtitle,
        "captured"
    );
    assert!(path.is_file());
    assert!(pending.poll().is_none());
    assert!(receiver.try_recv().is_err());
    while core.poll_file_writer_shutdown() != 0 {
        assert!(Instant::now() < deadline);
        std::thread::yield_now();
    }
}
