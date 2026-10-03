//! One App-owned writer for tutorial history. Accepted saves survive closing their originating View.
use crate::store::tutorial_progress::Record;
use std::{
    collections::BTreeMap,
    sync::{
        Arc, Weak,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread::JoinHandle,
};

pub(crate) type Receipt = mpsc::Receiver<Result<Reply, String>>;
pub(crate) enum Reply {
    Loaded(Vec<Record>),
    Saved { record: Record, updated: bool },
}
#[derive(Clone)]
pub(crate) struct Save {
    pub(crate) topic: &'static str,
    pub(crate) revision: u32,
    pub(crate) step: &'static str,
    pub(crate) completed: bool,
    pub(crate) expected: i64,
}
enum Work {
    Load,
    Save(Save),
}
struct Request {
    owner: Arc<()>,
    work: Work,
    reply: mpsc::SyncSender<Result<Reply, String>>,
}
enum Message {
    Request(Request),
    Stop,
}
#[derive(Default)]
pub(crate) struct Client {
    sender: Option<mpsc::SyncSender<Message>>,
    closed: Arc<AtomicBool>,
    identity: Arc<()>,
}
impl Client {
    fn submit(&self, work: Work) -> Result<Receipt, String> {
        if self.closed.load(Ordering::Acquire) {
            return Err("tutorial writer stopped".into());
        }
        let sender = self.sender.as_ref().ok_or("tutorial writer unavailable")?;
        let (reply, receipt) = mpsc::sync_channel(1);
        sender
            .try_send(Message::Request(Request {
                owner: self.identity.clone(),
                work,
                reply,
            }))
            .map_err(|error| format!("tutorial write admission: {error}"))?;
        Ok(receipt)
    }
    pub(crate) fn load(&self) -> Result<Receipt, String> {
        self.submit(Work::Load)
    }
    pub(crate) fn save(&self, save: Save) -> Result<Receipt, String> {
        self.submit(Work::Save(save))
    }
}

type Versions = BTreeMap<(&'static str, u32), (i64, i64)>;
#[derive(Default)]
struct Chains(BTreeMap<usize, (Weak<()>, Versions)>);
impl Chains {
    fn execute(
        &mut self,
        conn: &mut rusqlite::Connection,
        owner: &Arc<()>,
        work: Work,
    ) -> rusqlite::Result<Reply> {
        self.0.retain(|_, (owner, _)| owner.strong_count() != 0);
        let (_, versions) = self
            .0
            .entry(Arc::as_ptr(owner) as usize)
            .or_insert_with(|| (Arc::downgrade(owner), BTreeMap::new()));
        match work {
            Work::Load => {
                versions.clear();
                let records = crate::adapters::ui::tutorial::all_topics()
                    .iter()
                    .map(|topic| {
                        crate::store::tutorial_progress::load(conn, topic.id, topic.revision)
                    })
                    .collect::<rusqlite::Result<Vec<_>>>()?;
                Ok(Reply::Loaded(records))
            }
            Work::Save(save) => {
                let key = (save.topic, save.revision);
                // Rapid actions from one View may still carry its last acknowledged version.
                // Only advance through this View's own successful writes, never another writer.
                let chain = versions
                    .get(&key)
                    .copied()
                    .filter(|(base, last)| save.expected >= *base && save.expected <= *last);
                let expected = chain.map_or(save.expected, |(_, last)| last);
                let (record, updated) = crate::store::tutorial_progress::save(
                    conn,
                    save.topic,
                    save.revision,
                    save.step,
                    save.completed,
                    expected,
                )?;
                if updated {
                    versions.insert(
                        key,
                        (
                            chain.map_or(save.expected, |(base, _)| base),
                            record.version,
                        ),
                    );
                } else {
                    versions.remove(&key);
                }
                Ok(Reply::Saved { record, updated })
            }
        }
    }
}

pub(crate) struct TutorialProgress {
    sender: mpsc::SyncSender<Message>,
    closed: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
}
impl TutorialProgress {
    pub(crate) fn new(wake: Arc<dyn Fn() + Send + Sync>) -> std::io::Result<Self> {
        Self::with_executor(wake, |chains, owner, work| {
            crate::db::with_state_db(|db| chains.execute(&mut db.conn, owner, work))
                .ok_or_else(|| "tutorial database unavailable".to_owned())
                .and_then(|result| result.map_err(|error| error.to_string()))
        })
    }
    fn with_executor(
        wake: Arc<dyn Fn() + Send + Sync>,
        mut execute: impl FnMut(&mut Chains, &Arc<()>, Work) -> Result<Reply, String> + Send + 'static,
    ) -> std::io::Result<Self> {
        let (sender, receiver) = mpsc::sync_channel::<Message>(64);
        let closed = Arc::new(AtomicBool::new(false));
        let stopped = closed.clone();
        let worker = std::thread::Builder::new()
            .name("tutorial-progress".into())
            .spawn(move || {
                let mut chains = Chains::default();
                loop {
                    let message = if stopped.load(Ordering::Acquire) {
                        match receiver.try_recv() {
                            Ok(message) => message,
                            Err(_) => break,
                        }
                    } else {
                        match receiver.recv() {
                            Ok(message) => message,
                            Err(_) => break,
                        }
                    };
                    let Message::Request(request) = message else {
                        continue;
                    };
                    let result = execute(&mut chains, &request.owner, request.work);
                    if let Err(error) = &result {
                        tracing::warn!(%error, "tutorial persistence failed");
                    }
                    if request.reply.send(result).is_err() {
                        tracing::debug!("tutorial save completed after its View closed");
                    }
                    wake();
                }
            })?;
        Ok(Self {
            sender,
            closed,
            worker: Some(worker),
        })
    }
    pub(crate) fn client(&self) -> Client {
        Client {
            sender: Some(self.sender.clone()),
            closed: self.closed.clone(),
            identity: Arc::new(()),
        }
    }
    pub(crate) fn stop(&self) {
        if !self.closed.swap(true, Ordering::AcqRel) {
            match self.sender.try_send(Message::Stop) {
                Ok(()) | Err(mpsc::TrySendError::Disconnected(_)) => {}
                // A full queue already wakes the worker; the closed flag makes it drain and exit.
                Err(mpsc::TrySendError::Full(_)) => {}
            }
        }
    }
    pub(crate) fn poll_shutdown(&mut self) -> usize {
        self.stop();
        if self
            .worker
            .as_ref()
            .is_some_and(|worker| worker.is_finished())
        {
            if self.worker.take().expect("finished worker").join().is_err() {
                tracing::warn!("tutorial writer panicked");
            }
        }
        usize::from(self.worker.is_some())
    }
}
impl Drop for TutorialProgress {
    fn drop(&mut self) {
        if self.poll_shutdown() != 0 {
            tracing::warn!("tutorial writer remains unjoined at App drop");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rapid_own_writes_advance_but_another_views_conflict_is_not_adopted() {
        let mut conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch(crate::store::tutorial_progress::SCHEMA)
            .unwrap();
        let owner = Arc::new(());
        let other = Arc::new(());
        let mut chains = Chains::default();
        let save = |step, expected| Save {
            topic: "basics",
            revision: 1,
            step,
            completed: false,
            expected,
        };
        for (index, step) in ["first", "second", "third"].into_iter().enumerate() {
            let Reply::Saved { record, updated } = chains
                .execute(&mut conn, &owner, Work::Save(save(step, 0)))
                .unwrap()
            else {
                panic!("save");
            };
            assert!(updated);
            assert_eq!(record.version, index as i64 + 1);
            assert_eq!(record.resume_step.as_deref(), Some(step));
        }
        let Reply::Saved { record, updated } = chains
            .execute(&mut conn, &owner, Work::Save(save("fourth", 1)))
            .unwrap()
        else {
            panic!("save");
        };
        assert!(updated);
        assert_eq!(record.version, 4);
        chains
            .execute(&mut conn, &other, Work::Save(save("other", 4)))
            .unwrap();
        let Reply::Saved { record, updated } = chains
            .execute(&mut conn, &owner, Work::Save(save("late", 0)))
            .unwrap()
        else {
            panic!("save");
        };
        assert!(!updated);
        assert_eq!(record.resume_step.as_deref(), Some("other"));
    }
    #[test]
    fn accepted_save_survives_closed_view_and_writer_joins_after_draining() {
        let mut conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch(crate::store::tutorial_progress::SCHEMA)
            .unwrap();
        let mut writer =
            TutorialProgress::with_executor(Arc::new(|| {}), move |chains, owner, work| {
                chains
                    .execute(&mut conn, owner, work)
                    .map_err(|error| error.to_string())
            })
            .unwrap();
        let client = writer.client();
        let topic = &crate::adapters::ui::tutorial::all_topics()[0];
        let receipt = client
            .save(Save {
                topic: topic.id,
                revision: topic.revision,
                step: topic.steps[0].id,
                completed: true,
                expected: 0,
            })
            .unwrap();
        drop(client);
        drop(receipt);
        let observer = writer.client();
        let receipt = observer.load().unwrap();
        writer.stop();
        let Reply::Loaded(records) = receipt
            .recv_timeout(std::time::Duration::from_secs(5))
            .unwrap()
            .unwrap()
        else {
            panic!("loaded");
        };
        assert!(records[0].completed);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while writer.poll_shutdown() != 0 {
            assert!(std::time::Instant::now() < deadline);
            std::thread::yield_now();
        }
        assert!(observer.load().is_err());
    }
}
