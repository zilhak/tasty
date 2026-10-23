use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;
use tasty_utils::batch_writer::ManualClock;

#[derive(Default)]
struct Control {
    refuse: bool,
    pending: usize,
    ready: Vec<(String, u64, RecentOutcome)>,
}
struct Sink {
    control: Arc<Mutex<Control>>,
    polls: Arc<AtomicUsize>,
}
impl RecentSink for Sink {
    fn submit(&mut self, _: RecentSnapshot) -> Result<(), String> {
        let mut state = self.control.lock().unwrap();
        if state.refuse {
            return Err("queue refused".into());
        }
        state.pending += 1;
        Ok(())
    }
    fn reap(&mut self) -> Vec<(String, u64, RecentOutcome)> {
        self.polls.fetch_add(1, Ordering::Relaxed);
        let mut state = self.control.lock().unwrap();
        let ready = std::mem::take(&mut state.ready);
        state.pending -= ready.len();
        ready
    }
    fn pending(&self) -> usize {
        self.control.lock().unwrap().pending
    }
}
struct Fixture {
    recent: RecentFiles,
    clock: Arc<ManualClock>,
    control: Arc<Mutex<Control>>,
    polls: Arc<AtomicUsize>,
}
fn fixture() -> Fixture {
    let clock = Arc::new(ManualClock::default());
    let control = Arc::new(Mutex::new(Control::default()));
    let polls = Arc::new(AtomicUsize::new(0));
    let recent = RecentFiles::with_clock(
        HashMap::new(),
        Box::new(Sink {
            control: control.clone(),
            polls: polls.clone(),
        }),
        clock.clone(),
    );
    Fixture {
        recent,
        clock,
        control,
        polls,
    }
}
fn complete(f: &Fixture, kind: &str, revision: u64, outcome: RecentOutcome) {
    f.control
        .lock()
        .unwrap()
        .ready
        .push((kind.into(), revision, outcome));
    f.recent.settle();
}
#[test]
fn first_applied_period_survives_overlap_old_success_failure_and_new_period() {
    let mut f = fixture();
    assert_eq!(f.recent.save_snapshot().dirty_age_us, None);
    f.recent.add("markdown", "/one.md".into());
    assert_eq!(f.recent.save_snapshot().dirty_age_us, Some(0));
    f.clock.advance(Duration::from_micros(10));
    f.recent.add("markdown", "/two.md".into());
    f.clock.advance(Duration::from_micros(20));
    complete(&f, "markdown", 1, RecentOutcome::Committed);
    assert_eq!(
        f.recent.revisions("markdown"),
        Revisions {
            visible: 2,
            committed: 1
        }
    );
    assert_eq!(f.recent.save_snapshot().dirty_age_us, Some(30));
    complete(
        &f,
        "markdown",
        2,
        RecentOutcome::NotCommitted("save failed".into()),
    );
    assert_eq!(f.recent.pending(), 0);
    f.clock.advance(Duration::from_micros(40));
    assert_eq!(f.recent.save_snapshot().dirty_age_us, Some(70));
    f.recent.add("markdown", "/three.md".into());
    assert_eq!(f.recent.save_snapshot().dirty_age_us, Some(70));
    complete(&f, "markdown", 3, RecentOutcome::Committed);
    assert_eq!(
        f.recent.save_snapshot(),
        RecentSaveSnapshot {
            dirty: false,
            dirty_age_us: None
        }
    );
    f.clock.advance(Duration::from_micros(999));
    f.recent.add("markdown", "/four.md".into());
    assert_eq!(f.recent.save_snapshot().dirty_age_us, Some(0));
}
#[test]
fn admission_refusal_and_unknown_with_no_pending_work_preserve_first_age() {
    let mut f = fixture();
    f.control.lock().unwrap().refuse = true;
    f.recent.add("markdown", "/refused.md".into());
    f.clock.advance(Duration::from_micros(13));
    assert_eq!(f.recent.pending(), 0);
    assert_eq!(f.recent.save_snapshot().dirty_age_us, Some(13));
    f.control.lock().unwrap().refuse = false;
    f.recent.add("markdown", "/unknown.md".into());
    complete(
        &f,
        "markdown",
        2,
        RecentOutcome::NotCommitted("outcome unknown".into()),
    );
    f.clock.advance(Duration::from_micros(7));
    assert_eq!(f.recent.pending(), 0);
    assert_eq!(f.recent.save_snapshot().dirty_age_us, Some(20));
    assert_eq!(f.recent.unsaved().len(), 1);
}
#[test]
fn snapshots_never_reap_ready_results_and_all_kinds_must_confirm() {
    let mut f = fixture();
    let mut second_view = f.recent.clone();
    f.recent.add("markdown", "/one.md".into());
    f.clock.advance(Duration::from_micros(10));
    second_view.add("directory", "/folder".into());
    f.control
        .lock()
        .unwrap()
        .ready
        .push(("markdown".into(), 1, RecentOutcome::Committed));
    f.clock.advance(Duration::from_micros(7));
    assert_eq!(f.recent.save_snapshot(), second_view.save_snapshot());
    assert_eq!(f.polls.load(Ordering::Relaxed), 0);
    assert_eq!(f.recent.revisions("markdown").committed, 0);
    assert_eq!(f.recent.save_snapshot().dirty_age_us, Some(17));
    f.recent.settle();
    assert_eq!(f.recent.revisions("markdown").committed, 1);
    assert_eq!(f.recent.save_snapshot().dirty_age_us, Some(17));
    complete(&f, "directory", 1, RecentOutcome::Committed);
    assert_eq!(f.recent.save_snapshot().dirty_age_us, None);
    assert!(f.recent.unsaved().is_empty());
}
