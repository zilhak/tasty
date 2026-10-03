//! App-owned bounded local reads. Views keep receipts; discarded receipts cannot update a new View.
use crate::core::fs_list::DirEntryInfo;
use crate::state::branch::HeadState;
use crate::view::ui::View;
use std::{
    io,
    path::PathBuf,
    sync::{Arc, mpsc},
    thread::JoinHandle,
};

const MAX_RUNNING: usize = 4;

pub(crate) struct Query<T> {
    request: Option<Request>,
    receiver: mpsc::Receiver<io::Result<T>>,
}
impl<T> Query<T> {
    fn new(make: impl FnOnce(mpsc::SyncSender<io::Result<T>>) -> Request) -> Self {
        let (sender, receiver) = mpsc::sync_channel(1);
        Self {
            request: Some(make(sender)),
            receiver,
        }
    }
    pub(crate) fn poll(&mut self, owner: &mut LocalReads) -> Option<io::Result<T>> {
        if let Some(request) = self.request.take() {
            if let Err(request) = owner.submit(request) {
                self.request = Some(request);
            }
        }
        match self.receiver.try_recv() {
            Ok(result) => Some(result),
            Err(mpsc::TryRecvError::Empty) => None,
            Err(mpsc::TryRecvError::Disconnected) => {
                Some(Err(io::Error::other("local read worker disconnected")))
            }
        }
    }
}

enum Request {
    Directory(PathBuf, mpsc::SyncSender<io::Result<Vec<DirEntryInfo>>>),
    Git(PathBuf, mpsc::SyncSender<io::Result<Option<HeadState>>>),
    Script(PathBuf, mpsc::SyncSender<io::Result<ScriptSource>>),
    #[cfg(test)]
    Blocked(mpsc::Receiver<()>, mpsc::SyncSender<io::Result<()>>),
}
impl Request {
    fn run(self) {
        let delivered = match self {
            #[cfg(test)]
            Self::Blocked(wait, sender) => {
                sender.send(wait.recv().map_err(io::Error::other)).is_ok()
            }
            Self::Directory(path, sender) => sender
                .send(crate::core::fs_list::read_dir_entries(&path))
                .is_ok(),
            Self::Git(path, sender) => sender.send(Ok(git_branch(&path))).is_ok(),
            Self::Script(path, sender) => {
                use std::io::Read;
                const LIMIT: u64 =
                    crate::adapters::production::tcp_ipc_server::MAX_REQUEST_LINE_BYTES as u64;
                let result = std::fs::File::open(&path).and_then(|file| {
                    let mut text = String::new();
                    file.take(LIMIT + 1).read_to_string(&mut text)?;
                    if text.len() as u64 > LIMIT {
                        return Err(io::Error::other("script exceeds 8 MiB"));
                    }
                    Ok(ScriptSource {
                        hash: tasty_settings::hash_bytes(text.as_bytes()),
                        source: text,
                    })
                });
                sender.send(result).is_ok()
            }
        };
        if !delivered {
            tracing::debug!("discarded local read result for a closed request");
        }
    }
}
pub(crate) fn directory(path: PathBuf) -> Query<Vec<DirEntryInfo>> {
    Query::new(|sender| Request::Directory(path, sender))
}
pub(crate) fn git(path: PathBuf) -> Query<Option<HeadState>> {
    Query::new(|sender| Request::Git(path, sender))
}
pub(crate) fn script(path: PathBuf) -> Query<ScriptSource> {
    Query::new(|sender| Request::Script(path, sender))
}

pub(crate) struct ScriptSource {
    pub(crate) source: String,
    pub(crate) hash: String,
}
pub(crate) struct PendingScript {
    pub(crate) script_id: String,
    pub(crate) name: String,
    pub(crate) stored_hash: String,
    pub(crate) query: Query<ScriptSource>,
}

pub(crate) struct LocalReads {
    jobs: Vec<JoinHandle<()>>,
    wake: Arc<dyn Fn() + Send + Sync>,
    stopping: bool,
}
impl Default for LocalReads {
    fn default() -> Self {
        Self::new(Arc::new(|| {}))
    }
}
impl LocalReads {
    pub(crate) fn new(wake: Arc<dyn Fn() + Send + Sync>) -> Self {
        Self {
            jobs: Vec::new(),
            wake,
            stopping: false,
        }
    }
    fn submit(&mut self, request: Request) -> Result<(), Request> {
        if self.stopping || self.jobs.len() >= MAX_RUNNING {
            return Err(request);
        }
        let wake = self.wake.clone();
        match std::thread::Builder::new()
            .name("local-read".into())
            .spawn(move || {
                request.run();
                wake();
            }) {
            Ok(job) => self.jobs.push(job),
            Err(error) => tracing::warn!(%error, "local read worker could not start"),
        }
        Ok(())
    }
    pub(crate) fn reap(&mut self) {
        let mut i = 0;
        while i < self.jobs.len() {
            if self.jobs[i].is_finished() {
                if self.jobs.swap_remove(i).join().is_err() {
                    tracing::warn!("local read worker panicked");
                }
            } else {
                i += 1;
            }
        }
    }
    pub(crate) fn poll_shutdown(&mut self) -> usize {
        self.stopping = true;
        self.reap();
        self.jobs.len()
    }
    #[cfg(test)]
    pub(crate) fn finish<T>(&mut self, query: &mut Query<T>) -> io::Result<T> {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            self.reap();
            if let Some(result) = query.poll(self) {
                return result;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "local query did not complete"
            );
            std::thread::yield_now();
        }
    }
}
impl Drop for LocalReads {
    fn drop(&mut self) {
        let remaining = self.poll_shutdown();
        if remaining != 0 {
            tracing::warn!(remaining, "local read workers remain unjoined at App drop");
        }
    }
}

pub(crate) fn git_branch(cwd: &std::path::Path) -> Option<HeadState> {
    let mut dir = Some(cwd);
    while let Some(d) = dir {
        let dot_git = d.join(".git");
        if let Ok(content) = std::fs::read_to_string(dot_git.join("HEAD")) {
            return crate::state::branch::parse_head(&content);
        }
        if let Ok(content) = std::fs::read_to_string(&dot_git)
            && let Some(gitdir) = crate::state::branch::resolve_gitdir_file(d, &content)
            && let Ok(head) = std::fs::read_to_string(gitdir.join("HEAD"))
        {
            return crate::state::branch::parse_head(&head);
        }
        dir = d.parent();
    }
    None
}

impl super::App {
    pub(crate) fn poll_local_reads(&mut self) {
        self.local_reads.reap();
        for view in self.view.views.values_mut() {
            view.poll_local_reads(&mut self.local_reads);
        }
    }
}

impl crate::view::MainView {
    pub(crate) fn poll_local_read_results(
        &mut self,
        owner: &mut crate::app::local_reads::LocalReads,
    ) {
        let mut changed = self.state.explorer_views.poll_local_reads(owner);
        changed |= self.state.poll_branch_read(owner);
        changed |=
            crate::adapters::ui::popup::file_picker::poll_local_reads(&mut self.state, owner);
        if self.state.dialogs.pending_script_confirm.is_none()
            && let Some(result) = self
                .state
                .script_reads
                .front_mut()
                .and_then(|request| request.query.poll(owner))
        {
            let request = self.state.script_reads.pop_front().expect("script receipt");
            match result {
                Ok(read) if read.hash == request.stored_hash => {
                    if self
                        .proxy
                        .send_event(crate::AppEvent::RunLuaScript {
                            source: read.source,
                            name: request.name,
                        })
                        .is_err()
                    {
                        tracing::warn!("script execution request after shutdown");
                    }
                }
                Ok(read) => {
                    self.state.dialogs.pending_script_confirm =
                        Some(crate::state::PendingScriptConfirm {
                            script_id: request.script_id,
                            name: request.name,
                            source: read.source,
                            new_hash: read.hash,
                            result: None,
                        });
                    self.state.dispatch_intent(
                        crate::intent::UiIntent::OpenPopup {
                            id: "script_changed_confirm",
                            mode: crate::intent::OpenPopupMode::CenteredFocused,
                        }
                        .from_user_menu("script_tofu_gate"),
                    );
                }
                Err(error) => tracing::warn!(%error, "script read failed"),
            }
            changed = true;
        }
        if changed {
            self.mark_dirty();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn replaced_directory_receipt_cannot_publish_its_old_result() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a");
        let b = dir.path().join("b");
        std::fs::create_dir(&a).unwrap();
        std::fs::create_dir(&b).unwrap();
        std::fs::write(a.join("old"), b"old").unwrap();
        std::fs::write(b.join("new"), b"new").unwrap();
        let mut owner = LocalReads::default();
        let mut query = directory(a);
        let _result = query.poll(&mut owner);
        query = directory(b);
        let result = owner.finish(&mut query).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].name, "new");
        while owner.poll_shutdown() != 0 {
            std::thread::yield_now();
        }
    }
    #[test]
    fn slow_reads_do_not_exceed_the_worker_limit() {
        let mut owner = LocalReads::default();
        let mut releases = Vec::new();
        let mut queries = Vec::new();
        for _ in 0..MAX_RUNNING + 1 {
            let (release, wait) = mpsc::channel();
            let mut query = Query::new(|reply| Request::Blocked(wait, reply));
            assert!(query.poll(&mut owner).is_none());
            releases.push(release);
            queries.push(query);
        }
        assert_eq!(owner.jobs.len(), MAX_RUNNING);
        assert!(queries.last().unwrap().request.is_some());
        for release in releases {
            release.send(()).unwrap();
        }
        for query in &mut queries {
            owner.finish(query).unwrap();
        }
        while owner.poll_shutdown() != 0 {
            std::thread::yield_now();
        }
    }
}
