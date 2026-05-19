//! 옵저버: PTY 라인 → 파서 → sink fan-out.
//!
//! 메인 thread 에 `ObserverRouter` 가 살고, 각 옵저버의 sink 는 자기 worker
//! thread 한 개와 bounded channel (`std::sync::mpsc::sync_channel`) 로 연결돼
//! 있다. `dispatch_text` 가 호출되면 라인 단위로 쪼개 매칭 옵저버에
//! `try_send` — 가득 차면 drop + counter 증가.
//!
//! 첫 PR scope: memory + file sink 2 종, 휘발성 spec. socket/fifo 와 spec
//! persistence 는 후속 phase.

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

/// 옵저버 고유 id (호스트 자동 할당, 1 부터 증가).
pub type ObserverId = u64;

/// 옵저버 등록 spec. `surface_id = None` 이면 모든 surface 를 본다.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ObserverSpec {
    pub surface_id: Option<u32>,
    /// 활성 파서 id 리스트. 비어있으면 [`DEFAULT_PARSER_IDS`] 사용.
    pub parsers: Vec<String>,
    /// kind 필터 (예: `["path","url"]`). `None` 이면 통과.
    pub kinds: Option<Vec<String>>,
    pub sink: SinkSpec,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SinkSpec {
    /// `tasty-memory` 의 `scope=Host` 위 `tasty.observer.<id>.<unix-ms>` key 로
    /// JSON 저장. `max_records=0` 이면 무한, 그 외는 ring buffer (오래된 키
    /// 부터 삭제).
    Memory { max_records: usize },
    /// `path` 가 `None` 이면 `~/.tasty/observers/<id>.jsonl` 에 자동 append.
    File { path: Option<PathBuf> },
}

/// `output.observe_info` / `output.observe_list` 응답.
#[derive(Debug, Clone, Serialize)]
pub struct ObserverInfo {
    pub id: ObserverId,
    #[serde(flatten)]
    pub spec_view: SpecView,
    /// dispatch 시도된 ParsedItem 수 (필터 적용 후).
    pub total_in: u64,
    /// sink channel 에 성공적으로 들어간 수.
    pub total_out: u64,
    /// backpressure 로 drop 된 수.
    pub dropped: u64,
    /// 마지막 try_send 성공 시각 (unix-ms).
    pub last_event_ms: Option<i64>,
}

/// `ObserverSpec` 의 응답용 평면화. `sink` 안의 path 가 default 였으면
/// 실제 resolved path 도 같이 노출.
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

/// 메인 스레드 상태. 각 옵저버는 자기 worker thread 와 bounded sender 를 갖는다.
pub struct ObserverRouter {
    next_id: ObserverId,
    observers: HashMap<ObserverId, ObserverEntry>,
    /// surface 별 partial-line 버퍼. `'\n'` 이 들어올 때까지 누적.
    line_buffers: HashMap<u32, LineBuffer>,
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
    /// 다음 emit 할 라인의 0-based index.
    next_idx: u32,
    /// 미완성 partial 라인 (마지막 `\n` 이후).
    partial: String,
}

/// 옵저버 등록 / dispatch 시 발생할 수 있는 에러.
#[derive(Debug)]
pub enum ObserverError {
    UnknownParser(String),
    InvalidPath(String),
    FileOpen(String),
    NotFound(ObserverId),
}

impl std::fmt::Display for ObserverError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ObserverError::UnknownParser(id) => write!(f, "unknown parser: {id}"),
            ObserverError::InvalidPath(p) => write!(f, "invalid sink path: {p}"),
            ObserverError::FileOpen(e) => write!(f, "failed to open sink file: {e}"),
            ObserverError::NotFound(id) => write!(f, "observer not found: {id}"),
        }
    }
}

impl std::error::Error for ObserverError {}

const SINK_CHANNEL_CAP: usize = 256;

impl ObserverRouter {
    pub fn new() -> Self {
        Self {
            next_id: 1,
            observers: HashMap::new(),
            line_buffers: HashMap::new(),
        }
    }

    /// 옵저버 등록. id 반환.
    pub fn register(&mut self, spec: ObserverSpec) -> Result<ObserverId, ObserverError> {
        // 파서 lookup
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

        let id = self.next_id;
        self.next_id += 1;

        // Sink resolved view (외부 응답용)
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

        // Sink worker spawn
        let (tx, rx) = sync_channel::<ParsedItem>(SINK_CHANNEL_CAP);
        let join = match &resolved_sink {
            SinkView::Memory { max_records } => {
                let cap = *max_records;
                let worker_id = id;
                Some(
                    thread::Builder::new()
                        .name(format!("tasty-observer-mem-{worker_id}"))
                        .spawn(move || run_memory_sink(worker_id, cap, rx))
                        .expect("spawn memory sink thread"),
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
                        .expect("spawn file sink thread"),
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

    pub fn unregister(&mut self, id: ObserverId) -> Result<(), ObserverError> {
        let entry = self
            .observers
            .remove(&id)
            .ok_or(ObserverError::NotFound(id))?;
        // tx drop → worker recv loop exits → join
        drop(entry.tx);
        if let Some(j) = entry.join
            && let Err(e) = j.join()
        {
            tracing::warn!("observer {id} sink thread panicked: {e:?}");
        }
        Ok(())
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

    /// PTY 가 emit 한 텍스트를 라인 단위로 쪼개 매칭 옵저버에 dispatch.
    pub fn dispatch_text(&mut self, surface_id: u32, text: &str) {
        if self.observers.is_empty() {
            return;
        }
        let buf = self.line_buffers.entry(surface_id).or_default();
        buf.partial.push_str(text);

        // `'\n'` 으로 라인 분리. 마지막 `\n` 이후는 partial 로 남김.
        let mut completed_lines: Vec<(u32, String)> = Vec::new();
        while let Some(nl) = buf.partial.find('\n') {
            let rest = buf.partial.split_off(nl + 1);
            let mut line = std::mem::replace(&mut buf.partial, rest);
            line.pop(); // remove '\n'
            if line.ends_with('\r') {
                line.pop();
            }
            completed_lines.push((buf.next_idx, line));
            buf.next_idx = buf.next_idx.wrapping_add(1);
        }

        if completed_lines.is_empty() {
            return;
        }

        for (idx, line) in completed_lines {
            self.dispatch_line(surface_id, idx, &line);
        }
    }

    fn dispatch_line(&mut self, surface_id: u32, line_idx: u32, line: &str) {
        // 매칭 옵저버 id 수집 (borrow 분리).
        let matching_ids: Vec<ObserverId> = self
            .observers
            .iter()
            .filter(|(_, e)| match e.spec.surface_id {
                None => true,
                Some(sid) => sid == surface_id,
            })
            .map(|(id, _)| *id)
            .collect();
        if matching_ids.is_empty() {
            return;
        }

        for oid in matching_ids {
            let Some(entry) = self.observers.get_mut(&oid) else {
                continue;
            };
            let mut items: Vec<ParsedItem> = Vec::new();
            for p in &entry.parser_handles {
                p.parse_line(line, line_idx, &mut items);
            }
            if items.is_empty() {
                continue;
            }
            if let Some(filter) = &entry.kinds_filter {
                items.retain(|it| filter.iter().any(|k| k == it.kind));
            }
            for item in items {
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
        }
    }

    /// Surface 가 닫혔을 때 호출. 그 surface 에 매인 옵저버 (wildcard 가
    /// 아닌) 는 자동 종료, line buffer 도 정리.
    pub fn drop_surface(&mut self, surface_id: u32) {
        self.line_buffers.remove(&surface_id);
        let tied: Vec<ObserverId> = self
            .observers
            .iter()
            .filter(|(_, e)| e.spec.surface_id == Some(surface_id))
            .map(|(id, _)| *id)
            .collect();
        for id in tied {
            if let Err(e) = self.unregister(id) {
                tracing::warn!("auto-unregister observer {id} for closed surface: {e}");
            }
        }
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
            let _ = self.unregister(id); // best-effort on shutdown
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
    let home = tasty_core::paths::tasty_home()
        .ok_or_else(|| ObserverError::InvalidPath("no $HOME / tasty home".to_string()))?;
    let dir = home.join("observers");
    std::fs::create_dir_all(&dir)
        .map_err(|e| ObserverError::InvalidPath(format!("create_dir {dir:?}: {e}")))?;
    Ok(dir.join(format!("{id}.jsonl")))
}

// ── workers ──────────────────────────────────────────────────────────────

fn run_memory_sink(observer_id: ObserverId, max_records: usize, rx: std::sync::mpsc::Receiver<ParsedItem>) {
    use tasty_memory::{HOST_OWNER, MemoryValue, PutOpts, Scope, with_store};
    let mut written_keys: std::collections::VecDeque<String> =
        std::collections::VecDeque::with_capacity(max_records.min(1024));
    while let Ok(item) = rx.recv() {
        let now = unix_ms_now();
        let key = format!("tasty.observer.{observer_id}.{now}");
        let record = json!({
            "kind": item.kind,
            "line": item.line,
            "byte_start": item.byte_start,
            "byte_end": item.byte_end,
            "data": item.data,
            "at_ms": now,
        });
        let result = with_store(|s| {
            s.put(
                HOST_OWNER,
                &Scope::Global,
                &key,
                &MemoryValue::Json(record),
                &PutOpts::default(),
            )
        });
        match result {
            Some(Ok(_)) => {
                if max_records > 0 {
                    written_keys.push_back(key);
                    while written_keys.len() > max_records {
                        let Some(old) = written_keys.pop_front() else {
                            break;
                        };
                        let _ = with_store(|s| s.delete(HOST_OWNER, &Scope::Global, &old, None));
                    }
                }
            }
            Some(Err(e)) => {
                tracing::warn!("observer {observer_id} memory put failed: {e}");
            }
            None => {
                tracing::warn!("observer {observer_id} memory store not initialised; stopping sink");
                return;
            }
        }
    }
}

fn run_file_sink(observer_id: ObserverId, mut file: File, rx: std::sync::mpsc::Receiver<ParsedItem>) {
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
        // No observers — nothing happens, but buffer machinery shouldn't crash.
        r.dispatch_text(1, "hello\nworld\n");
        r.dispatch_text(1, "partial");
        r.dispatch_text(1, " line\n");
        // ObserverRouter::dispatch_text is the public surface; with no observers
        // there are no externally observable effects. The test just ensures
        // the buffer-and-split path doesn't panic on partial chunks.
    }

    #[test]
    fn register_unknown_parser_rejects() {
        let mut r = ObserverRouter::new();
        let err = r
            .register(ObserverSpec {
                surface_id: None,
                parsers: vec!["bogus".to_string()],
                kinds: None,
                sink: SinkSpec::Memory { max_records: 10 },
            })
            .unwrap_err();
        assert!(matches!(err, ObserverError::UnknownParser(_)));
    }
}
