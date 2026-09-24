//! PTY 출력을 줄 단위로 파싱해 observer별 memory·파일 worker에 보낸다.
//! 유한 채널이 가득 차면 해당 항목을 버린다. 등록 정보는 저장하지 않는다.

use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::sync::mpsc::{SyncSender, TrySendError, sync_channel};
use std::thread::{self, JoinHandle};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use serde_json::json;
use tasty_output::{DEFAULT_PARSER_IDS, ParsedItem, Parser, lookup};

pub type ObserverId = u64;

/// surface_id가 None이면 모든 surface를 구독한다.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ObserverSpec {
    pub surface_id: Option<u32>,
    /// 비어 있으면 기본 파서 목록을 사용한다.
    pub parsers: Vec<String>,
    /// None이면 종류로 거르지 않는다.
    pub kinds: Option<Vec<String>>,
    pub sink: SinkSpec,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SinkSpec {
    /// Global scope에 JSON을 저장한다. max_records가 0이면 이 worker의 개수 제한은 없다.
    /// 양수면 오래된 키부터 삭제를 시도하지만 삭제 실패를 재시도하지 않아 저장소에 더 남을 수 있다.
    Memory { max_records: usize },
    /// path가 없으면 Tasty 홈의 observers 디렉터리에 ID별 JSONL을 덧붙인다.
    File { path: Option<PathBuf> },
}

#[derive(Debug, Clone, Serialize)]
pub struct ObserverInfo {
    pub id: ObserverId,
    #[serde(flatten)]
    pub spec_view: SpecView,
    /// 파싱·종류 필터 뒤 채널 전송을 시도한 항목 수.
    pub total_in: u64,
    /// 채널에 들어간 수이며 저장소에 기록된 수는 아니다.
    pub total_out: u64,
    /// 채널 Full로 버린 수. 끊긴 채널이나 저장 오류로 잃은 항목은 세지 않는다.
    pub dropped: u64,
    /// 마지막 try_send 성공 시각(Unix ms).
    pub last_event_ms: Option<i64>,
}

/// 응답에는 기본 파일 경로도 실제 선택한 경로로 보여 준다.
#[derive(Debug, Clone, Serialize)]
pub struct SpecView {
    pub surface_id: Option<u32>,
    pub parsers: Vec<String>,
    pub kinds: Option<Vec<String>>,
    pub sink: SinkView,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SinkView {
    Memory { max_records: usize },
    File { path: PathBuf },
}

fn unix_ms_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

pub struct ObserverRouter {
    /// engine이 같은 발급기를 사용해 창 사이 observer ID가 중복되지 않게 한다.
    next_id: std::sync::Arc<std::sync::atomic::AtomicU64>,
    observers: HashMap<ObserverId, ObserverEntry>,
    /// 줄바꿈 전의 불완전한 줄. 길이 제한은 없다.
    line_buffers: HashMap<u32, LineBuffer>,
    /// surface를 닫는 동안 worker를 기다리지 않도록 join을 미룬다.
    /// 끝난 worker를 회수하고 나머지는 join_retired 또는 Drop에서 기다린다.
    retired: Vec<RetiredSink>,
}

struct RetiredSink {
    id: ObserverId,
    join: JoinHandle<()>,
}

struct ObserverEntry {
    spec: ObserverSpec,
    resolved_sink: SinkView,
    parser_handles: Vec<&'static dyn Parser>,
    kinds_filter: Option<Vec<String>>,
    tx: SyncSender<ParsedItem>,
    join: Option<JoinHandle<()>>,
    total_in: u64,
    total_out: u64,
    dropped: u64,
    last_event_ms: Option<i64>,
}

#[derive(Default)]
struct LineBuffer {
    next_idx: u32,
    partial: String,
}

#[derive(Debug)]
pub enum ObserverError {
    UnknownParser(String),
    InvalidPath(String),
    FileOpen(String),
    NotFound(ObserverId),
    /// worker 생성 실패도 파일 열기 실패처럼 오류로 반환한다.
    ThreadSpawn(String),
}

impl std::fmt::Display for ObserverError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ObserverError::UnknownParser(id) => write!(f, "unknown parser: {id}"),
            ObserverError::InvalidPath(p) => write!(f, "invalid sink path: {p}"),
            ObserverError::FileOpen(e) => write!(f, "failed to open sink file: {e}"),
            ObserverError::NotFound(id) => write!(f, "observer not found: {id}"),
            ObserverError::ThreadSpawn(e) => write!(f, "failed to spawn sink thread: {e}"),
        }
    }
}

impl std::error::Error for ObserverError {}

const SINK_CHANNEL_CAP: usize = 256;

impl ObserverRouter {
    pub fn new() -> Self {
        Self::with_counter(std::sync::Arc::new(std::sync::atomic::AtomicU64::new(1)))
    }

    /// 여러 engine이 공유하는 ID 발급기로 router를 만든다.
    pub fn with_counter(next_id: std::sync::Arc<std::sync::atomic::AtomicU64>) -> Self {
        Self {
            next_id,
            observers: HashMap::new(),
            line_buffers: HashMap::new(),
            retired: Vec::new(),
        }
    }

    pub fn register(
        &mut self,
        spec: ObserverSpec,
        memory: std::sync::Arc<std::sync::Mutex<dyn tasty_memory::MemoryStorage>>,
    ) -> Result<ObserverId, ObserverError> {
        let parser_ids: Vec<String> = if spec.parsers.is_empty() {
            DEFAULT_PARSER_IDS.iter().map(|s| s.to_string()).collect()
        } else {
            spec.parsers.clone()
        };
        let mut parser_handles = Vec::with_capacity(parser_ids.len());
        for id in &parser_ids {
            match lookup(id) {
                Some(p) => parser_handles.push(p),
                None => return Err(ObserverError::UnknownParser(id.clone())),
            }
        }

        let id: ObserverId = self
            .next_id
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);

        let resolved_sink = match &spec.sink {
            SinkSpec::Memory { max_records } => SinkView::Memory {
                max_records: *max_records,
            },
            SinkSpec::File { path } => {
                let resolved = match path {
                    Some(p) => p.clone(),
                    None => default_file_path(id)?,
                };
                SinkView::File { path: resolved }
            }
        };

        let (tx, rx) = sync_channel::<ParsedItem>(SINK_CHANNEL_CAP);
        let join = match &resolved_sink {
            SinkView::Memory { max_records } => {
                let cap = *max_records;
                let worker_id = id;
                let mem = memory.clone();
                Some(
                    thread::Builder::new()
                        .name(format!("tasty-observer-mem-{worker_id}"))
                        .spawn(move || run_memory_sink(worker_id, cap, rx, mem))
                        .map_err(|e| ObserverError::ThreadSpawn(e.to_string()))?,
                )
            }
            SinkView::File { path } => {
                let file = OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(path)
                    .map_err(|e| ObserverError::FileOpen(e.to_string()))?;
                let worker_id = id;
                Some(
                    thread::Builder::new()
                        .name(format!("tasty-observer-file-{worker_id}"))
                        .spawn(move || run_file_sink(worker_id, file, rx))
                        .map_err(|e| ObserverError::ThreadSpawn(e.to_string()))?,
                )
            }
        };

        let mut normalized = spec.clone();
        normalized.parsers = parser_ids;

        let kinds_filter = normalized.kinds.clone();

        self.observers.insert(
            id,
            ObserverEntry {
                spec: normalized,
                resolved_sink,
                parser_handles,
                kinds_filter,
                tx,
                join,
                total_in: 0,
                total_out: 0,
                dropped: 0,
                last_event_ms: None,
            },
        );
        Ok(id)
    }

    /// 명시적 해제는 worker 종료를 기다린다. 대기 시간에 상한은 없다.
    /// 저장 오류는 worker가 로그로 남기므로 Ok가 모든 항목의 저장 성공을 뜻하지는 않는다.
    pub fn unregister(&mut self, id: ObserverId) -> Result<(), ObserverError> {
        let entry = self
            .observers
            .remove(&id)
            .ok_or(ObserverError::NotFound(id))?;
        drop(entry.tx);
        if let Some(j) = entry.join
            && let Err(e) = j.join()
        {
            tracing::warn!("observer {id} sink thread panicked: {e:?}");
        }
        Ok(())
    }

    /// sender를 닫고 join을 미룬다. worker는 채널 잔여 항목을 받을 수 있지만
    /// 저장 오류·worker panic·프로세스 종료에 따른 손실까지 막지는 못한다.
    fn retire(&mut self, id: ObserverId, entry: ObserverEntry) {
        drop(entry.tx);
        if let Some(join) = entry.join {
            self.retired.push(RetiredSink { id, join });
        }
    }

    /// 완료로 표시된 worker만 join하고 나머지는 다음 회수로 미룬다.
    fn reap_finished(&mut self) {
        let mut still_running = Vec::with_capacity(self.retired.len());
        for r in self.retired.drain(..) {
            if r.join.is_finished() {
                if let Err(e) = r.join.join() {
                    tracing::warn!("observer {} sink thread panicked: {e:?}", r.id);
                }
            } else {
                still_running.push(r);
            }
        }
        self.retired = still_running;
    }

    /// 남은 worker를 모두 기다린다. 저장 I/O가 지연되면 이 호출도 기다린다.
    pub fn join_retired(&mut self) {
        for r in self.retired.drain(..) {
            if let Err(e) = r.join.join() {
                tracing::warn!("observer {} sink thread panicked: {e:?}", r.id);
            }
        }
    }

    #[cfg(test)]
    pub(crate) fn retired_len(&self) -> usize {
        self.retired.len()
    }

    pub fn list(&self) -> Vec<ObserverInfo> {
        self.observers
            .iter()
            .map(|(id, e)| entry_to_info(*id, e))
            .collect()
    }

    pub fn info(&self, id: ObserverId) -> Option<ObserverInfo> {
        self.observers.get(&id).map(|e| entry_to_info(id, e))
    }

    /// 출력 이벤트를 켤지 판단한다. dispatch_line과 같은 surface·전체 구독 규칙을 써야 한다.
    pub fn wants(&self, surface_id: u32) -> bool {
        self.observers.values().any(|e| match e.spec.surface_id {
            None => true,
            Some(sid) => sid == surface_id,
        })
    }

    /// 완성된 줄을 observer에 보내고 반환한다. observer가 없어도 OutputMatch hook이 이 줄을 사용할 수 있다.
    pub fn dispatch_text(&mut self, surface_id: u32, text: &str) -> Vec<String> {
        let buf = self.line_buffers.entry(surface_id).or_default();
        buf.partial.push_str(text);

        let mut completed_lines: Vec<(u32, String)> = Vec::new();
        while let Some(nl) = buf.partial.find('\n') {
            let rest = buf.partial.split_off(nl + 1);
            let mut line = std::mem::replace(&mut buf.partial, rest);
            line.pop();
            if line.ends_with('\r') {
                line.pop();
            }
            completed_lines.push((buf.next_idx, line));
            buf.next_idx = buf.next_idx.wrapping_add(1);
        }

        if completed_lines.is_empty() {
            return Vec::new();
        }

        let mut lines = Vec::with_capacity(completed_lines.len());
        for (idx, line) in completed_lines {
            if !self.observers.is_empty() {
                self.dispatch_line(surface_id, idx, &line);
            }
            lines.push(line);
        }
        lines
    }

    fn dispatch_line(&mut self, surface_id: u32, line_idx: u32, line: &str) {
        let matching_ids = self.matching_observer_ids(surface_id);
        if matching_ids.is_empty() {
            return;
        }

        for oid in matching_ids {
            let Some(entry) = self.observers.get_mut(&oid) else {
                continue;
            };
            let items = parse_and_filter_items(entry, line, line_idx);
            if items.is_empty() {
                continue;
            }
            for item in items {
                send_item_to_sink(oid, entry, item);
            }
        }
    }

    fn matching_observer_ids(&self, surface_id: u32) -> Vec<ObserverId> {
        self.observers
            .iter()
            .filter(|(_, e)| match e.spec.surface_id {
                None => true,
                Some(sid) => sid == surface_id,
            })
            .map(|(id, _)| *id)
            .collect()
    }

    /// 해당 surface에만 연결된 observer와 부분 줄을 지운다. 전체 구독은 유지한다.
    /// 여러 surface를 닫는 동안 각 worker를 기다리지 않도록 retire한다.
    pub fn drop_surface(&mut self, surface_id: u32) {
        self.line_buffers.remove(&surface_id);
        let tied: Vec<ObserverId> = self
            .observers
            .iter()
            .filter(|(_, e)| e.spec.surface_id == Some(surface_id))
            .map(|(id, _)| *id)
            .collect();
        for id in tied {
            let Some(entry) = self.observers.remove(&id) else {
                continue;
            };
            self.retire(id, entry);
        }
        self.reap_finished();
    }
}

impl Default for ObserverRouter {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for ObserverRouter {
    fn drop(&mut self) {
        let ids: Vec<ObserverId> = self.observers.keys().copied().collect();
        for id in ids {
            let _ = self.unregister(id); // 종료 중 NotFound는 추가 처리하지 않는다.
        }
        // 명시적 종료 절차를 거치지 않은 Drop도 남은 worker 종료를 기다린다.
        self.join_retired();
    }
}

fn parse_and_filter_items(entry: &ObserverEntry, line: &str, line_idx: u32) -> Vec<ParsedItem> {
    let mut items: Vec<ParsedItem> = Vec::new();
    for p in &entry.parser_handles {
        p.parse_line(line, line_idx, &mut items);
    }
    if items.is_empty() {
        return items;
    }
    if let Some(filter) = &entry.kinds_filter {
        items.retain(|it| filter.iter().any(|k| k == it.kind));
    }
    items
}

fn send_item_to_sink(oid: ObserverId, entry: &mut ObserverEntry, item: ParsedItem) {
    entry.total_in += 1;
    match entry.tx.try_send(item) {
        Ok(()) => {
            entry.total_out += 1;
            entry.last_event_ms = Some(unix_ms_now());
        }
        Err(TrySendError::Full(_)) => {
            entry.dropped += 1;
            if entry.dropped.is_multiple_of(1000) {
                tracing::warn!(
                    "observer {oid}: dropped {} items (sink backpressure)",
                    entry.dropped
                );
            }
        }
        Err(TrySendError::Disconnected(_)) => {
            tracing::warn!("observer {oid}: sink worker disconnected");
        }
    }
}

fn entry_to_info(id: ObserverId, entry: &ObserverEntry) -> ObserverInfo {
    ObserverInfo {
        id,
        spec_view: SpecView {
            surface_id: entry.spec.surface_id,
            parsers: entry.spec.parsers.clone(),
            kinds: entry.spec.kinds.clone(),
            sink: entry.resolved_sink.clone(),
        },
        total_in: entry.total_in,
        total_out: entry.total_out,
        dropped: entry.dropped,
        last_event_ms: entry.last_event_ms,
    }
}

fn default_file_path(id: ObserverId) -> Result<PathBuf, ObserverError> {
    let home = tasty_utils::path::tasty_home()
        .ok_or_else(|| ObserverError::InvalidPath("no $HOME / tasty home".to_string()))?;
    let dir = home.join("observers");
    std::fs::create_dir_all(&dir)
        .map_err(|e| ObserverError::InvalidPath(format!("create_dir {dir:?}: {e}")))?;
    Ok(dir.join(format!("{id}.jsonl")))
}

fn run_memory_sink(
    observer_id: ObserverId,
    max_records: usize,
    rx: std::sync::mpsc::Receiver<ParsedItem>,
    memory: std::sync::Arc<std::sync::Mutex<dyn tasty_memory::MemoryStorage>>,
) {
    use tasty_memory::{HOST_OWNER, MemoryValue, PutOpts, Scope};
    let mut written_keys: std::collections::VecDeque<String> =
        std::collections::VecDeque::with_capacity(max_records.min(1024));
    // 같은 밀리초의 여러 항목이 같은 키를 덮어쓰지 않도록 worker 순번을 덧붙인다.
    let mut seq: u64 = 0;
    while let Ok(item) = rx.recv() {
        let now = unix_ms_now();
        let key = format!("tasty.observer.{observer_id}.{now}.{seq:06}");
        seq += 1;
        let record = json!({
            "kind": item.kind,
            "line": item.line,
            "byte_start": item.byte_start,
            "byte_end": item.byte_end,
            "data": item.data,
            "at_ms": now,
        });
        // 같은 memory 락을 쓰는 다른 모듈과 최초 poison 보고 플래그를 공유한다.
        let mut guard = crate::poison::recover_mutex(
            memory.lock(),
            tasty_memory::STORE_LOCK_WHAT,
            &tasty_memory::STORE_LOCK_POISONED,
        );
        let put_result = guard.put(
            HOST_OWNER,
            &Scope::Global,
            &key,
            &MemoryValue::Json(record),
            &PutOpts::default(),
        );
        match put_result {
            Ok(_) => {
                if max_records > 0 {
                    written_keys.push_back(key);
                    while written_keys.len() > max_records {
                        let Some(old) = written_keys.pop_front() else {
                            break;
                        };
                        let _ = guard.delete(HOST_OWNER, &Scope::Global, &old, None); // 삭제 실패는 무시하며 이 키를 다시 삭제하지 않는다.
                    }
                }
            }
            Err(e) => {
                tracing::warn!("observer {observer_id} memory put failed: {e}");
            }
        }
    }
}

fn run_file_sink(
    observer_id: ObserverId,
    mut file: File,
    rx: std::sync::mpsc::Receiver<ParsedItem>,
) {
    while let Ok(item) = rx.recv() {
        let now = unix_ms_now();
        let record = json!({
            "kind": item.kind,
            "line": item.line,
            "byte_start": item.byte_start,
            "byte_end": item.byte_end,
            "data": item.data,
            "at_ms": now,
        });
        let line = match serde_json::to_string(&record) {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!("observer {observer_id} file sink serialize: {e}");
                continue;
            }
        };
        if let Err(e) = writeln!(file, "{line}") {
            tracing::warn!("observer {observer_id} file sink write: {e}");
            return;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn line_split_basic() {
        let mut r = ObserverRouter::new();
        r.dispatch_text(1, "hello\nworld\n");
        r.dispatch_text(1, "partial");
        r.dispatch_text(1, " line\n");
        // 이 검사는 부분 줄 처리에서 panic하지 않는지만 확인한다.
    }

    #[test]
    fn dispatch_text_returns_completed_lines_split_across_chunks() {
        let mut r = ObserverRouter::new();
        assert_eq!(r.dispatch_text(1, "partial ERR"), Vec::<String>::new());
        assert_eq!(
            r.dispatch_text(1, "OR\n"),
            vec!["partial ERROR".to_string()]
        );
    }

    #[test]
    fn dispatch_text_returns_multiple_completed_lines_in_order() {
        let mut r = ObserverRouter::new();
        assert_eq!(
            r.dispatch_text(1, "one\ntwo\nthree"),
            vec!["one".to_string(), "two".to_string()]
        );
        assert_eq!(r.dispatch_text(1, "\n"), vec!["three".to_string()]);
    }

    #[test]
    fn dispatch_text_line_buffers_are_isolated_per_surface() {
        let mut r = ObserverRouter::new();
        assert_eq!(
            r.dispatch_text(1, "surface-one partial"),
            Vec::<String>::new()
        );
        assert_eq!(
            r.dispatch_text(2, "surface-two\n"),
            vec!["surface-two".to_string()]
        );
        assert_eq!(
            r.dispatch_text(1, " completed\n"),
            vec!["surface-one partial completed".to_string()]
        );
    }

    #[test]
    fn wants_matches_observer_registrations() {
        let mut r = ObserverRouter::new();
        assert!(!r.wants(1), "no observers — gate off everywhere");

        let memory: std::sync::Arc<std::sync::Mutex<dyn tasty_memory::MemoryStorage>> =
            std::sync::Arc::new(std::sync::Mutex::new(
                tasty_memory::MemoryStore::open_in_memory().unwrap(),
            ));
        let tied = r
            .register(
                ObserverSpec {
                    surface_id: Some(1),
                    parsers: vec![],
                    kinds: None,
                    sink: SinkSpec::Memory { max_records: 10 },
                },
                memory.clone(),
            )
            .unwrap();
        assert!(r.wants(1), "surface-tied observer enables its surface");
        assert!(!r.wants(2), "other surfaces stay off");

        let wildcard = r
            .register(
                ObserverSpec {
                    surface_id: None,
                    parsers: vec![],
                    kinds: None,
                    sink: SinkSpec::Memory { max_records: 10 },
                },
                memory,
            )
            .unwrap();
        assert!(r.wants(2), "wildcard observer enables every surface");

        r.unregister(wildcard).unwrap();
        assert!(!r.wants(2), "wildcard removed — surface 2 off again");
        r.unregister(tied).unwrap();
        assert!(!r.wants(1), "all observers removed — gate off");
    }

    fn mem_store() -> std::sync::Arc<std::sync::Mutex<dyn tasty_memory::MemoryStorage>> {
        std::sync::Arc::new(std::sync::Mutex::new(
            tasty_memory::MemoryStore::open_in_memory().unwrap(),
        ))
    }

    fn register_file_observer(
        r: &mut ObserverRouter,
        surface_id: u32,
        path: &std::path::Path,
    ) -> ObserverId {
        r.register(
            ObserverSpec {
                surface_id: Some(surface_id),
                parsers: vec!["path".into()],
                kinds: None,
                sink: SinkSpec::File {
                    path: Some(path.to_path_buf()),
                },
            },
            mem_store(),
        )
        .unwrap()
    }

    #[test]
    fn drop_surface_retires_worker_without_joining() {
        let dir = tempfile::tempdir().unwrap();
        let mut r = ObserverRouter::new();
        register_file_observer(&mut r, 1, &dir.path().join("a.jsonl"));
        assert_eq!(r.retired_len(), 0);

        r.drop_surface(1);

        assert!(!r.wants(1), "옵저버는 즉시 등록 해제된다");
        assert!(r.retired_len() <= 1);
        r.join_retired();
        assert_eq!(r.retired_len(), 0, "join_retired 가 전부 회수한다");
    }

    #[test]
    fn retired_workers_flush_everything_accepted_into_the_channel() {
        let dir = tempfile::tempdir().unwrap();
        let sink = dir.path().join("flush.jsonl");
        let mut r = ObserverRouter::new();
        register_file_observer(&mut r, 7, &sink);

        // 채널이 가득 차서 버리는 경우와 구별하도록 용량보다 적게 보낸다.
        let mut expected = 0usize;
        for i in 0..64 {
            r.dispatch_text(7, &format!("/tmp/retire-probe-{i}\n"));
            expected += 1;
        }

        r.drop_surface(7);
        r.join_retired();

        let written = std::fs::read_to_string(&sink).unwrap();
        let lines: Vec<&str> = written.lines().collect();
        assert_eq!(
            lines.len(),
            expected,
            "이 파일 저장 시나리오에서 채널에 넣은 항목이 모두 기록돼야 한다"
        );
        assert!(
            lines.last().unwrap().contains("/tmp/retire-probe-63"),
            "마지막 항목까지 기록된다: {:?}",
            lines.last()
        );
    }

    #[test]
    fn dropping_the_router_reaps_retired_workers_even_without_s3b() {
        let dir = tempfile::tempdir().unwrap();
        let sink = dir.path().join("dropped.jsonl");
        let mut r = ObserverRouter::new();
        register_file_observer(&mut r, 3, &sink);

        let mut expected = 0usize;
        for i in 0..64 {
            r.dispatch_text(3, &format!("/tmp/drop-probe-{i}\n"));
            expected += 1;
        }

        r.drop_surface(3);
        drop(r); // join_retired 를 명시적으로 부르지 않는다.

        let written = std::fs::read_to_string(&sink).unwrap();
        let lines: Vec<&str> = written.lines().collect();
        assert_eq!(
            lines.len(),
            expected,
            "라우터 drop 만으로도 수락된 항목이 전부 기록된다"
        );
        assert!(
            lines.last().unwrap().contains("/tmp/drop-probe-63"),
            "마지막 항목까지 기록된다: {:?}",
            lines.last()
        );
    }

    #[test]
    fn many_surfaces_retire_and_join_once() {
        let dir = tempfile::tempdir().unwrap();
        let mut r = ObserverRouter::new();
        let sids: Vec<u32> = (1..=8).collect();
        for sid in &sids {
            register_file_observer(&mut r, *sid, &dir.path().join(format!("s{sid}.jsonl")));
        }
        for sid in &sids {
            for i in 0..16 {
                r.dispatch_text(*sid, &format!("/tmp/multi-{sid}-{i}\n"));
            }
        }
        for sid in &sids {
            r.drop_surface(*sid);
        }
        r.join_retired();

        for sid in &sids {
            let body = std::fs::read_to_string(dir.path().join(format!("s{sid}.jsonl"))).unwrap();
            let lines: Vec<&str> = body.lines().collect();
            assert_eq!(lines.len(), 16, "surface {sid}");
            assert!(
                lines
                    .last()
                    .unwrap()
                    .contains(&format!("/tmp/multi-{sid}-15")),
                "surface {sid} 마지막 항목 유실"
            );
        }
    }

    #[test]
    fn explicit_unregister_still_joins_synchronously() {
        let dir = tempfile::tempdir().unwrap();
        let sink = dir.path().join("explicit.jsonl");
        let mut r = ObserverRouter::new();
        let id = register_file_observer(&mut r, 3, &sink);
        r.dispatch_text(3, "/tmp/explicit-1\n");

        r.unregister(id).unwrap();

        assert_eq!(r.retired_len(), 0, "명시 해제는 retired 에 쌓이지 않는다");
        let body = std::fs::read_to_string(&sink).unwrap();
        assert!(body.contains("/tmp/explicit-1"), "복귀 시점에 이미 기록됨");
    }

    #[test]
    fn drop_surface_leaves_wildcard_observers_alone() {
        let dir = tempfile::tempdir().unwrap();
        let mut r = ObserverRouter::new();
        let wildcard = r
            .register(
                ObserverSpec {
                    surface_id: None,
                    parsers: vec!["path".into()],
                    kinds: None,
                    sink: SinkSpec::File {
                        path: Some(dir.path().join("wild.jsonl")),
                    },
                },
                mem_store(),
            )
            .unwrap();
        r.drop_surface(5);
        assert!(r.wants(9), "wildcard 는 surface close 로 해제되지 않는다");
        assert_eq!(r.retired_len(), 0);
        r.unregister(wildcard).unwrap();
    }

    #[test]
    fn register_unknown_parser_rejects() {
        let mut r = ObserverRouter::new();
        let memory: std::sync::Arc<std::sync::Mutex<dyn tasty_memory::MemoryStorage>> =
            std::sync::Arc::new(std::sync::Mutex::new(
                tasty_memory::MemoryStore::open_in_memory().unwrap(),
            ));
        let err = r
            .register(
                ObserverSpec {
                    surface_id: None,
                    parsers: vec!["bogus".to_string()],
                    kinds: None,
                    sink: SinkSpec::Memory { max_records: 10 },
                },
                memory,
            )
            .unwrap_err();
        assert!(matches!(err, ObserverError::UnknownParser(_)));
    }

    fn item(data: serde_json::Value) -> ParsedItem {
        ParsedItem {
            kind: "path",
            line: 0,
            byte_start: 0,
            byte_end: 1,
            data,
        }
    }

    /// 해당 observer의 data를 저장 키 순으로 읽는다. 시계 변화나 순번 자릿수 경계를 검사하는 것은 아니다.
    fn observer_records(
        memory: &std::sync::Arc<std::sync::Mutex<dyn tasty_memory::MemoryStorage>>,
        id: ObserverId,
    ) -> Vec<serde_json::Value> {
        let prefix = format!("tasty.observer.{id}.");
        memory
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .list(
                &tasty_memory::Scope::Global,
                &tasty_memory::ListOpts {
                    prefix: Some(prefix),
                    ..Default::default()
                },
            )
            .unwrap()
            .into_iter()
            .map(|e| match e.value {
                tasty_memory::MemoryValue::Json(v) => v["data"].clone(),
                other => panic!("observer 레코드는 JSON 이다: {other:?}"),
            })
            .collect()
    }

    #[test]
    fn the_memory_sink_shares_one_poison_coordinate_with_the_core() {
        assert!(std::ptr::eq(
            &tasty_memory::STORE_LOCK_POISONED,
            &crate::core::MEMORY_POISONED
        ));
        assert_eq!(tasty_memory::STORE_LOCK_WHAT, crate::core::MEMORY_WHAT);
    }

    /// put 실패는 로그만 남기고 다음 항목을 처리한다. 구독자에게 별도 gap 통지를 보내지 않는다.
    #[test]
    fn a_failed_put_drops_that_record_and_the_sink_keeps_going() {
        let config = tasty_memory::MemoryConfig {
            entry_max_bytes: 512,
            ..Default::default()
        };
        let memory: std::sync::Arc<std::sync::Mutex<dyn tasty_memory::MemoryStorage>> =
            std::sync::Arc::new(std::sync::Mutex::new(
                tasty_memory::MemoryStore::open_in_memory_with_config(config).unwrap(),
            ));
        let (tx, rx) = sync_channel::<ParsedItem>(4);
        tx.send(item(json!({ "big": "x".repeat(4096) }))).unwrap();
        tx.send(item(json!({ "small": 1 }))).unwrap();
        drop(tx);

        run_memory_sink(41, 10, rx, memory.clone());

        let records = observer_records(&memory, 41);
        assert_eq!(
            records,
            vec![json!({ "small": 1 })],
            "실패한 레코드가 쓰였거나 그 뒤 레코드가 안 쓰였다"
        );
    }

    /// 삭제가 성공하는 저장소에서 최근 N건을 남기는지 본다.
    #[test]
    fn the_sink_keeps_the_latest_records_even_within_one_millisecond() {
        let memory = mem_store();
        let (tx, rx) = sync_channel::<ParsedItem>(6);
        for n in 0..6 {
            tx.send(item(json!({ "n": n }))).unwrap();
        }
        drop(tx);
        run_memory_sink(43, 2, rx, memory.clone());
        assert_eq!(
            observer_records(&memory, 43),
            vec![json!({ "n": 4 }), json!({ "n": 5 })],
        );

        let (tx, rx) = sync_channel::<ParsedItem>(6);
        for n in 0..6 {
            tx.send(item(json!({ "n": n }))).unwrap();
        }
        drop(tx);
        run_memory_sink(44, 0, rx, memory.clone());
        assert_eq!(
            observer_records(&memory, 44),
            (0..6).map(|n| json!({ "n": n })).collect::<Vec<_>>(),
        );
    }

    #[test]
    fn a_poisoned_store_lock_is_recovered_by_the_sink() {
        let memory = mem_store();
        let holder = memory.clone();
        let poisoned = std::thread::spawn(move || {
            let _guard = holder.lock().unwrap();
            panic!("poison the store lock on purpose");
        })
        .join();
        assert!(poisoned.is_err());
        assert!(memory.is_poisoned());

        let (tx, rx) = sync_channel::<ParsedItem>(1);
        tx.send(item(json!({ "n": 1 }))).unwrap();
        drop(tx);
        run_memory_sink(42, 10, rx, memory.clone());

        assert_eq!(observer_records(&memory, 42), vec![json!({ "n": 1 })]);
        assert!(
            tasty_memory::STORE_LOCK_POISONED.load(std::sync::atomic::Ordering::Relaxed),
            "복구가 공용 store poison 플래그에 기록되지 않았다"
        );
    }
}
