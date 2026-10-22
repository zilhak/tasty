use super::*;
use std::sync::{Mutex, mpsc};
use std::time::{Duration, Instant};

struct Gated {
    entered: mpsc::Sender<u64>,
    release: Mutex<mpsc::Receiver<bool>>,
}
impl SettingsStorage for Gated {
    fn load(&self) -> Settings {
        Settings::default()
    }
    fn save(&self, settings: &Settings) -> anyhow::Result<()> {
        self.entered.send(settings.remote_transfer.max_mb).unwrap();
        if !self
            .release
            .lock()
            .unwrap()
            .recv_timeout(Duration::from_secs(5))?
        {
            anyhow::bail!("replacement failed");
        }
        Ok(())
    }
}
fn settings(value: u64) -> Settings {
    let mut settings = Settings::default();
    settings.remote_transfer.max_mb = value;
    settings
}
fn wait_saves(saves: &mut SettingsSaves, n: usize) -> Vec<SettledSave> {
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut results = Vec::new();
    while results.len() < n {
        results.extend(saves.reap());
        assert!(Instant::now() < deadline, "receipt missing");
        std::thread::yield_now();
    }
    results
}
#[test]
fn older_completion_does_not_clear_newer_dirty_and_failure_keeps_it() {
    let (entered, received) = mpsc::channel();
    let (release, released) = mpsc::channel();
    let storage = Arc::new(Gated {
        entered,
        release: Mutex::new(released),
    });
    let mut parts = start(
        storage.clone(),
        Limits::default(),
        Arc::new(batch_writer::SystemClock),
        Arc::new(|| {}),
    )
    .unwrap();
    let mut saves = SettingsSaves::new(Some(parts.writer.handle()), storage);
    saves.submit(settings(1), None, None);
    assert_eq!(received.recv_timeout(Duration::from_secs(5)).unwrap(), 1);
    saves.submit(settings(2), None, None);
    release.send(true).unwrap();
    assert_eq!(received.recv_timeout(Duration::from_secs(5)).unwrap(), 2);
    assert!(wait_saves(&mut saves, 1)[0].saved.is_ok());
    assert_eq!(saves.unsaved(), Some((2, 1)));
    release.send(false).unwrap();
    assert!(wait_saves(&mut saves, 1)[0].saved.is_err());
    assert_eq!(saves.unsaved(), Some((2, 1)));
    saves.retry_latest_before_shutdown();
    assert_eq!(received.recv_timeout(Duration::from_secs(5)).unwrap(), 2);
    release.send(true).unwrap();
    assert!(wait_saves(&mut saves, 1)[0].saved.is_ok());
    assert_eq!(saves.unsaved(), None);
    let deadline = Instant::now() + Duration::from_secs(5);
    while parts.writer.poll_shutdown() != 0 {
        assert!(Instant::now() < deadline);
        std::thread::yield_now();
    }
    assert_eq!(parts.stats.snapshot().replaced, 2);
    assert_eq!(parts.stats.snapshot().failed, 1);
}
#[test]
fn dynamic_settings_payload_counts_toward_admission_budget() {
    use batch_writer::Request;
    let small = FileRequest::Settings {
        revision: 1,
        settings: Box::new(settings(1)),
    };
    let mut large = settings(2);
    large.remote_transfer.dir = "x".repeat(100_000);
    let large = FileRequest::Settings {
        revision: 2,
        settings: Box::new(large),
    };
    assert!(large.bytes() > small.bytes() + 99_000);
}
