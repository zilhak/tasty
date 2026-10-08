//! 실행 중인 자식이 자기 report 블록에 쓰는 통로: 회차 토큰 발급, 자식 환경 변수, run stderr 의
//! 표지 줄. 규칙은 [`tasty_agent::task::report`] 에 있다.

use std::ffi::OsString;
use std::sync::{Arc, Mutex};

use tasty_agent::task::report::{
    AppendOutcome, REPORT_ENV, ReportAddress, ReportLimits, ReportSource, ReportToken,
    STDERR_MARKER,
};
use tasty_agent::{AgentError, Task, TaskStore};
use tasty_memory::{HOST_OWNER, MemoryStorage};

use super::{HostExecutor, now_ms};

/// 설정이 바꾸는 report 상한. 러너 스레드와 IPC 처리가 같은 값을 읽는다.
#[derive(Debug, Default)]
pub struct SharedReportLimits(Mutex<ReportLimits>);

impl SharedReportLimits {
    pub fn get(&self) -> ReportLimits {
        *self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    pub fn set(&self, limits: ReportLimits) {
        *self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = limits;
    }
}

/// 회차 토큰. 회차 번호와 달리 예측할 수 없다.
fn new_report_token() -> String {
    use rand::RngCore;
    let mut bytes = [0u8; 16];
    rand::rng().fill_bytes(&mut bytes);
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

impl HostExecutor {
    /// 이 dispatch 가 만들 회차의 토큰을 저장하고 `task` 에 붙인다. 저장하지 못하면 이 회차는
    /// custom 기록을 받지 않을 뿐 실행은 계속한다.
    pub(super) fn issue_report_token(&mut self, task: &mut Task) {
        let Some(next) = tasty_agent::task::attempt::next_attempt(task, 0) else {
            return;
        };
        let token = ReportToken {
            attempt: next.number,
            token: new_report_token(),
        };
        let seq = self.ctx.agent_seq.clone();
        let res = self.ctx.with_memory(|mem| {
            TaskStore::new(mem, HOST_OWNER, seq.as_ref()).issue_report_token(
                task.workspace_id,
                &task.id,
                token.clone(),
            )
        });
        match res {
            Ok(_) => task.report_token = Some(token),
            Err(e) => tracing::warn!("agent task {}: report token not stored: {e}", task.id),
        }
    }
}

/// `source` 경로로 이 회차 블록을 가리키는 주소. 토큰이 없으면(v1 task 등) 없다.
pub(crate) fn report_address(task: &Task, source: ReportSource) -> Option<ReportAddress> {
    let t = task.report_token.as_ref()?;
    Some(ReportAddress {
        workspace_id: task.workspace_id,
        task_id: task.id.clone(),
        attempt: t.attempt,
        source,
        token: t.token.clone(),
    })
}

/// 자식 환경에 더할 [`REPORT_ENV`]. 토큰이 없으면 비어 있다.
pub(crate) fn report_env(task: &Task, source: ReportSource) -> Vec<(OsString, OsString)> {
    report_address(task, source)
        .map(|a| vec![(OsString::from(REPORT_ENV), OsString::from(a.to_env_value()))])
        .unwrap_or_default()
}

/// custom 기록 한 줄을 저장한다. 러너의 stderr 표지 줄과 IPC 가 같이 쓴다. `writer_may` 는
/// 저장소 잠금 안에서 토큰·닫힘 검사 뒤에 부른다.
pub(crate) fn append(
    memory: &Mutex<dyn MemoryStorage>,
    seq: &std::sync::atomic::AtomicU64,
    limits: ReportLimits,
    addr: &ReportAddress,
    text: &str,
    cut_before: u64,
    writer_may: impl FnOnce() -> bool,
) -> Result<(AppendOutcome, u64), AgentError> {
    let mut guard = tasty_utils::poison::recover_mutex(
        memory.lock(),
        tasty_memory::STORE_LOCK_WHAT,
        &tasty_memory::STORE_LOCK_POISONED,
    );
    TaskStore::new(&mut *guard, HOST_OWNER, seq).append_report_by(
        addr,
        text,
        cut_before,
        limits,
        now_ms(),
        writer_may,
    )
}

/// 표지 줄 하나로 받는 최대 바이트. 더 긴 줄은 앞부분만 쓰고, 버린 바이트 수는 append 상한이
/// 자른 수와 합쳐 `omit_by_limit` 에 남긴다.
const MARKER_LINE_CAP: usize = 64 * 1024;

/// stderr 바이트에서 [`STDERR_MARKER`] 로 시작하는 줄을 골라낸다. 바이트는 그대로 두고 읽기만 한다.
/// 표지 줄마다 텍스트와 줄 상한을 넘어 버린 바이트 수(줄 끝 `\r` 제외)를 넘긴다.
#[derive(Default)]
pub(crate) struct MarkerScanner {
    line: Vec<u8>,
    /// 지금 줄이 상한을 넘어 버린 바이트 수.
    dropped: u64,
    /// 마지막으로 버린 바이트가 `\r` 이다. 줄 끝이면 텍스트가 아니므로 버린 수에서 뺀다.
    dropped_cr: bool,
}

impl MarkerScanner {
    pub(crate) fn feed(&mut self, chunk: &[u8], mut on_marker: impl FnMut(&str, u64)) {
        for &b in chunk {
            if b == b'\n' {
                self.emit(&mut on_marker);
                continue;
            }
            if self.line.len() < MARKER_LINE_CAP {
                self.line.push(b);
            } else {
                self.dropped += 1;
                self.dropped_cr = b == b'\r';
            }
        }
    }

    /// 마지막 줄바꿈 뒤에 남은 줄도 표지면 낸다.
    pub(crate) fn finish(&mut self, mut on_marker: impl FnMut(&str, u64)) {
        if !self.line.is_empty() {
            self.emit(&mut on_marker);
        }
    }

    fn emit(&mut self, on_marker: &mut impl FnMut(&str, u64)) {
        let line = std::mem::take(&mut self.line);
        let raw_dropped = std::mem::take(&mut self.dropped);
        let cr_dropped = std::mem::take(&mut self.dropped_cr);
        // 줄을 넘겨 버렸다면 줄 끝 `\r` 은 버린 쪽에 있다. 아니면 담아 둔 줄에서 뗀다.
        let (line, dropped) = if raw_dropped == 0 {
            (line.strip_suffix(b"\r").unwrap_or(&line), 0)
        } else {
            (&line[..], raw_dropped - u64::from(cr_dropped))
        };
        if let Some(rest) = line.strip_prefix(STDERR_MARKER.as_bytes()) {
            on_marker(&String::from_utf8_lossy(rest), dropped);
        }
    }
}

/// 저장소를 거치지 않고 건너뛴 append 수를 블록에 한 번에 더한다.
pub(crate) fn count_omitted(
    memory: &Mutex<dyn MemoryStorage>,
    seq: &std::sync::atomic::AtomicU64,
    addr: &ReportAddress,
    count: u64,
) -> Result<(), AgentError> {
    let mut guard = tasty_utils::poison::recover_mutex(
        memory.lock(),
        tasty_memory::STORE_LOCK_WHAT,
        &tasty_memory::STORE_LOCK_POISONED,
    );
    TaskStore::new(&mut *guard, HOST_OWNER, seq).count_report_omissions(addr, count)
}

/// run stderr 를 읽는 스레드가 표지 줄을 이 회차 블록에 append 한다. 실패해도 실행은 그대로다.
/// 블록에 남은 자리가 0 이 되면 그 뒤 줄은 잠금과 저장 없이 세기만 하고, stderr 를 다 읽은 뒤
/// (회차가 닫히기 전) 그 수를 `omitted_appends` 에 한 번에 더한다. 그 사이 설정에서 블록 상한을
/// 올려도 이 회차의 나머지 줄은 세기만 한다.
pub(crate) struct MarkerSink {
    memory: Arc<Mutex<dyn MemoryStorage>>,
    seq: Arc<std::sync::atomic::AtomicU64>,
    limits: Arc<SharedReportLimits>,
    addr: ReportAddress,
    scanner: MarkerScanner,
    full: bool,
    skipped: u64,
}

impl MarkerSink {
    pub(crate) fn new(
        memory: Arc<Mutex<dyn MemoryStorage>>,
        seq: Arc<std::sync::atomic::AtomicU64>,
        limits: Arc<SharedReportLimits>,
        addr: ReportAddress,
    ) -> Self {
        Self {
            memory,
            seq,
            limits,
            addr,
            scanner: MarkerScanner::default(),
            full: false,
            skipped: 0,
        }
    }

    pub(crate) fn observe(&mut self, chunk: &[u8]) {
        // 스캐너가 줄을 넘기는 동안 저장 상태(`full`·`skipped`)를 바꾸므로 잠시 꺼내 둔다.
        let mut scanner = std::mem::take(&mut self.scanner);
        scanner.feed(chunk, |text, cut| self.store(text, cut));
        self.scanner = scanner;
    }

    pub(crate) fn finish(&mut self) {
        let mut scanner = std::mem::take(&mut self.scanner);
        scanner.finish(|text, cut| self.store(text, cut));
        if self.skipped > 0
            && let Err(e) = count_omitted(&self.memory, &self.seq, &self.addr, self.skipped)
        {
            tracing::warn!(
                "agent task {}: {} skipped stderr report lines not counted: {e}",
                self.addr.task_id,
                self.skipped
            );
        }
    }

    fn store(&mut self, text: &str, cut_before: u64) {
        if self.full {
            self.skipped += 1;
            return;
        }
        let limits = self.limits.get();
        match append(
            &self.memory,
            &self.seq,
            limits,
            &self.addr,
            text,
            cut_before,
            || true,
        ) {
            Ok((_, room)) => self.full = room == 0,
            Err(e) => tracing::warn!(
                "agent task {}: stderr report line not stored: {e}",
                self.addr.task_id
            ),
        }
    }
}

#[cfg(test)]
#[path = "report_tests.rs"]
mod tests;
