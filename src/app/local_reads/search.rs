//! 탐색기의 하위 폴더 검색. 시작 폴더부터 너비 우선으로 읽으며 이름이 맞는 항목을 흘려보낸다.
//! 받는 쪽(View 의 검색 영수증)이 사라지거나 Stop 을 누르면 다음 폴더를 읽기 전에 멈춘다.
//! 링크로 된 폴더는 따라 들어가지 않는다. 같은 폴더를 두 번 읽거나 끝없이 도는 것을 막기 위해서다.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, mpsc};
use std::time::{Duration, Instant};

use crate::core::fs_list::{DirEntryInfo, EntryLink, read_dir_entries};

/// 결과를 모아 보내는 간격. 폴더마다 깨우면 화면을 너무 자주 다시 그린다.
const FLUSH_EVERY: Duration = Duration::from_millis(100);
/// 하위 폴더 검색이 모으는 결과 수. 이만큼 찾으면 멈추고 결과는 남긴다.
pub(crate) const SEARCH_MAX_HITS: usize = 5_000;

pub(crate) enum SearchEvent {
    Hits(Vec<DirEntryInfo>),
    /// 읽지 못한 하위 폴더.
    Skipped(PathBuf),
    /// 시작 폴더를 읽지 못했다. OS 문구 그대로다.
    Failed(String),
    /// 끝났다. Stop 으로 멈췄으면 `stopped`, 결과가 `SEARCH_MAX_HITS` 에 닿아 멈췄으면 `capped`.
    Done {
        stopped: bool,
        capped: bool,
    },
}

pub(crate) struct SearchSpec {
    pub(super) root: PathBuf,
    pub(super) query: String,
    pub(super) stop: Arc<AtomicBool>,
}

/// View 가 가진 검색 영수증. 버리면 worker 가 멈춘다.
pub(crate) struct SearchQuery {
    request: Option<super::Request>,
    receiver: mpsc::Receiver<SearchEvent>,
    stop: Arc<AtomicBool>,
}

pub(crate) fn search(root: PathBuf, query: String) -> SearchQuery {
    let (sender, receiver) = mpsc::channel();
    let stop = Arc::new(AtomicBool::new(false));
    SearchQuery {
        request: Some(super::Request::Search(
            SearchSpec {
                root,
                query,
                stop: Arc::clone(&stop),
            },
            sender,
        )),
        receiver,
        stop,
    }
}

impl SearchQuery {
    /// 아직 시작하지 않았으면 worker 에 넘기고, 그사이 온 사건을 모두 꺼낸다.
    pub(crate) fn poll(&mut self, requests: &mut super::ReadRequests) -> Vec<SearchEvent> {
        if let Some(request) = self.request.take()
            && let Err(request) = requests.push(request)
        {
            self.request = Some(request);
        }
        self.receiver.try_iter().collect()
    }
    pub(crate) fn stop(&self) {
        self.stop.store(true, Ordering::Release);
    }
}

impl Drop for SearchQuery {
    fn drop(&mut self) {
        self.stop();
    }
}

/// worker 에서 실행한다. 보낼 곳이 닫혔으면 바로 멈춘다.
pub(super) fn run(spec: SearchSpec, sender: mpsc::Sender<SearchEvent>, wake: &dyn Fn()) {
    let stopped = || spec.stop.load(Ordering::Acquire);
    let mut queue = VecDeque::from([spec.root.clone()]);
    let mut batch = Vec::new();
    let mut last_flush = Instant::now();
    let mut hits = Hits::default();
    let flush = |batch: &mut Vec<DirEntryInfo>| {
        batch.is_empty()
            || sender
                .send(SearchEvent::Hits(std::mem::take(batch)))
                .is_ok()
    };
    while let Some(dir) = queue.pop_front() {
        if stopped() {
            break;
        }
        if hits.full() {
            hits.capped = true;
            break;
        }
        let entries = match read_dir_entries(&dir) {
            Ok(entries) => entries,
            Err(error) if dir == spec.root => {
                if sender.send(SearchEvent::Failed(error.to_string())).is_ok() {
                    wake();
                }
                return;
            }
            Err(_) => {
                if sender.send(SearchEvent::Skipped(dir)).is_err() {
                    return;
                }
                continue;
            }
        };
        for entry in entries {
            if entry.is_dir && entry.link == EntryLink::NotALink {
                queue.push_back(entry.path.clone());
            }
            if matches(&entry.name, &spec.query) {
                hits.take(entry, &mut batch);
            }
        }
        if last_flush.elapsed() >= FLUSH_EVERY {
            if !flush(&mut batch) {
                return;
            }
            last_flush = Instant::now();
            wake();
        }
    }
    if !flush(&mut batch)
        || sender
            .send(SearchEvent::Done {
                stopped: stopped(),
                capped: hits.capped,
            })
            .is_err()
    {
        tracing::debug!("explorer search receipt dropped before the search finished");
    }
}

/// 모은 결과 수와 상한 도달 여부.
#[derive(Default)]
struct Hits {
    found: usize,
    /// 상한에 닿은 뒤 더 맞는 항목이 있거나 읽지 않은 폴더가 남았다.
    capped: bool,
}

impl Hits {
    fn full(&self) -> bool {
        self.found >= SEARCH_MAX_HITS
    }

    /// 상한 안이면 묶음에 넣고, 넘으면 버리고 상한에 닿았다고 남긴다.
    fn take(&mut self, entry: DirEntryInfo, batch: &mut Vec<DirEntryInfo>) {
        if self.full() {
            self.capped = true;
        } else {
            self.found += 1;
            batch.push(entry);
        }
    }
}

fn matches(name: &str, query: &str) -> bool {
    tasty_ui_widgets::match_range(name, query).is_some()
}

/// 결과가 든 폴더를 시작 폴더 기준 상대 경로로. 시작 폴더 자신은 ".".
/// 화면에 보이는 로컬 경로라 OS 구분자를 그대로 쓴다. 저장소 상대 경로가 아니다.
pub(crate) fn relative_folder(start: &Path, hit: &Path) -> String {
    let parent = hit.parent().unwrap_or(hit);
    match parent.strip_prefix(start) {
        Ok(rel) if rel.as_os_str().is_empty() => ".".to_string(),
        Ok(rel) => rel.display().to_string(),
        Err(_) => parent.display().to_string(),
    }
}

#[cfg(test)]
mod tests;
