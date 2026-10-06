//! 후처리 프로세스 하나를 실행하고 보고를 만든다.
//!
//! - 명령은 셸을 거치지 않고 직접 실행한다. stdin 에는 JSON 문서 하나를 쓰고 닫는다.
//! - stdin 쓰기·stdout·stderr 읽기는 각자 스레드에서 해 파이프가 차서 서로 기다리지 않는다.
//! - 제한 시간은 프로세스 종료와 상속된 파이프의 EOF 까지 포함한다. 시간이 지나거나 취소되면
//!   이 실행이 만든 프로세스 그룹(Windows 는 job)만 종료하고, 직접 자식을 회수한 뒤 보고한다.

use std::io::{self, Read, Write};
use std::path::PathBuf;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::thread;
use std::time::{Duration, Instant};

use tasty_agent::task::postprocess::{
    MAX_POSTPROCESS_STDOUT_BYTES, POSTPROCESS_STDERR_TAIL_BYTES, PostprocessCause,
    PostprocessOutcome, PostprocessReport, StdoutSpec, collect_stdout,
};

/// 종료를 확인하는 간격.
const WAIT_STEP: Duration = Duration::from_millis(10);
/// 프로세스 그룹을 종료한 뒤 파이프 EOF 를 기다리는 시간. 그룹 밖으로 나간 프로세스가 파이프를
/// 쥐고 있으면 이 시간 뒤 읽기를 포기한다.
const KILL_DRAIN_GRACE: Duration = Duration::from_secs(2);

/// 실행 요청.
pub(crate) struct ProcessRequest {
    pub(crate) command: Vec<String>,
    pub(crate) cwd: Option<PathBuf>,
    pub(crate) stdin: Vec<u8>,
    pub(crate) stdout: StdoutSpec,
    pub(crate) timeout: Duration,
    pub(crate) run: u32,
}

/// 시작한 프로세스. `wait` 가 끝날 때까지 프로세스 그룹을 소유한다.
pub(crate) struct Started {
    pub(crate) pid: u32,
    child: Child,
    group: ProcessGroup,
    stdin_rx: Receiver<io::Result<()>>,
    stdout_rx: Receiver<StdoutCapture>,
    stderr_rx: Receiver<StderrCapture>,
    stdout: StdoutSpec,
    deadline: Instant,
    run: u32,
}

#[derive(Default)]
struct StdoutCapture {
    data: Vec<u8>,
    overflowed: bool,
    error: Option<String>,
}

#[derive(Default)]
struct StderrCapture {
    tail: Vec<u8>,
    truncated: bool,
}

/// 프로세스를 시작한다. 실패하면 그 실행의 보고를 돌려준다.
pub(crate) fn spawn(req: ProcessRequest) -> Result<Started, PostprocessReport> {
    let deadline = Instant::now() + req.timeout;
    let fail = |cause, message: String| PostprocessReport::failed(req.run, cause, message);
    let Some((program, args)) = req.command.split_first() else {
        return Err(fail(PostprocessCause::Spawn, "empty command".into()));
    };
    let mut cmd = Command::new(program);
    tasty_utils::process::hide_console(&mut cmd);
    cmd.args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(dir) = &req.cwd {
        cmd.current_dir(dir);
    }
    ProcessGroup::configure(&mut cmd);
    let mut child = cmd
        .spawn()
        .map_err(|e| fail(PostprocessCause::Spawn, format!("spawn '{program}': {e}")))?;
    let pid = child.id();
    let mut group = ProcessGroup::adopt(pid);

    let (Some(stdin), Some(stdout), Some(stderr)) =
        (child.stdin.take(), child.stdout.take(), child.stderr.take())
    else {
        group.kill(&mut child);
        reap(&mut child);
        return Err(fail(
            PostprocessCause::Spawn,
            "child pipes were not created".into(),
        ));
    };
    let spawned = (|| -> io::Result<_> {
        let stdin_rx = spawn_worker(format!("agent-postprocess-stdin-pid{pid}"), {
            let payload = req.stdin;
            move || write_stdin(stdin, &payload)
        })?;
        let stdout_rx = spawn_worker(format!("agent-postprocess-stdout-pid{pid}"), move || {
            read_stdout(stdout)
        })?;
        let stderr_rx = spawn_worker(format!("agent-postprocess-stderr-pid{pid}"), move || {
            read_stderr(stderr)
        })?;
        Ok((stdin_rx, stdout_rx, stderr_rx))
    })();
    let (stdin_rx, stdout_rx, stderr_rx) = match spawned {
        Ok(rx) => rx,
        Err(e) => {
            group.kill(&mut child);
            reap(&mut child);
            return Err(fail(PostprocessCause::Spawn, format!("pipe worker: {e}")));
        }
    };
    Ok(Started {
        pid,
        child,
        group,
        stdin_rx,
        stdout_rx,
        stderr_rx,
        stdout: req.stdout,
        deadline,
        run: req.run,
    })
}

impl Started {
    /// 종료와 출력 수집을 기다려 보고를 만든다. `stop` 이 [`STOP_NONE`] 이 아니면 중단한다.
    pub(crate) fn wait(mut self, stop: &AtomicU8) -> PostprocessReport {
        loop {
            if let Some(message) = stop_message(stop) {
                return self.stop(PostprocessCause::Cancelled, message.into());
            }
            match self.group.exited(&mut self.child) {
                Ok(true) => break,
                Ok(false) => {}
                Err(e) => {
                    let message = format!("wait: {e}");
                    return self.stop(PostprocessCause::StdoutRead, message);
                }
            }
            if Instant::now() >= self.deadline {
                let message = timeout_message("exit");
                return self.stop(PostprocessCause::Timeout, message);
            }
            thread::sleep(WAIT_STEP);
        }
        // 직접 자식이 끝나도 자손이 파이프를 쥐고 있으면 EOF 가 오지 않는다. 같은 기한 안에서 기다린다.
        let stdout = match self.recv_until_deadline(&self.stdout_rx, stop) {
            Ok(c) => c,
            Err((cause, message)) => return self.stop(cause, message),
        };
        let stderr = match self.recv_until_deadline(&self.stderr_rx, stop) {
            Ok(c) => c,
            Err((cause, message)) => return self.stop(cause, message),
        };
        // 남은 그룹 구성원은 이 실행이 만든 프로세스다. 함께 정리하고 직접 자식을 회수한다.
        self.group.kill(&mut self.child);
        let Some(status) = reap(&mut self.child) else {
            return PostprocessReport::failed(
                self.run,
                PostprocessCause::StdoutRead,
                "could not collect the exit status",
            );
        };
        // stdin 은 읽지 않고 끝난 자식이면 쓰기가 파이프 끊김으로 끝난다. 그 밖의 오류만 실패다.
        let stdin_error = match self.stdin_rx.recv_timeout(WAIT_STEP) {
            Ok(Err(e)) if e.kind() != io::ErrorKind::BrokenPipe => Some(e.to_string()),
            _ => None,
        };
        report_for(self.run, &self.stdout, status, stdout, stderr, stdin_error)
    }

    /// 직접 자식이 끝난 뒤 파이프 EOF 를 같은 기한 안에서 기다린다.
    fn recv_until_deadline<T>(
        &self,
        rx: &Receiver<T>,
        stop: &AtomicU8,
    ) -> Result<T, (PostprocessCause, String)> {
        loop {
            if let Some(message) = stop_message(stop) {
                return Err((PostprocessCause::Cancelled, message.into()));
            }
            let now = Instant::now();
            if now >= self.deadline {
                return Err((
                    PostprocessCause::Timeout,
                    timeout_message("output EOF (a child process may hold the pipe)"),
                ));
            }
            match rx.recv_timeout(WAIT_STEP.min(self.deadline - now)) {
                Ok(v) => return Ok(v),
                Err(RecvTimeoutError::Timeout) => continue,
                Err(RecvTimeoutError::Disconnected) => {
                    return Err((
                        PostprocessCause::StdoutRead,
                        "the pipe reader ended without a result".into(),
                    ));
                }
            }
        }
    }

    /// 그룹을 종료하고 직접 자식을 회수한 뒤 실패 보고를 만든다.
    fn stop(mut self, cause: PostprocessCause, message: String) -> PostprocessReport {
        self.group.kill(&mut self.child);
        let status = reap(&mut self.child);
        // 종료 뒤 남은 stderr 를 진단으로 남긴다. 그룹 밖 프로세스가 쥐고 있으면 기다리지 않는다.
        let stderr = self.stderr_rx.recv_timeout(KILL_DRAIN_GRACE).ok();
        let mut report = PostprocessReport::failed(self.run, cause, message);
        report.exit_code = status.and_then(|s| s.code());
        if let Some(e) = stderr {
            report.stderr = Some(String::from_utf8_lossy(&e.tail).into_owned());
            report.stderr_truncated = e.truncated;
        }
        report
    }
}

impl Drop for Started {
    /// 보고를 만들지 못하고 버려지면(작업 스레드 생성 실패 등) 프로세스를 남기지 않는다.
    fn drop(&mut self) {
        if matches!(self.child.try_wait(), Ok(None)) {
            self.group.kill(&mut self.child);
            reap(&mut self.child);
        }
    }
}

/// 중단 요청 없음.
pub(crate) const STOP_NONE: u8 = 0;
/// task 가 취소됐다.
pub(crate) const STOP_CANCELLED: u8 = 1;
/// runner 가 멈췄다(workspace 정리·앱 종료).
pub(crate) const STOP_RUNNER: u8 = 2;

fn stop_message(stop: &AtomicU8) -> Option<&'static str> {
    match stop.load(Ordering::Acquire) {
        STOP_CANCELLED => Some("task was cancelled"),
        STOP_RUNNER => Some("the runner stopped before the postprocess finished"),
        _ => None,
    }
}

fn timeout_message(what: &str) -> String {
    format!("no {what} before the timeout")
}

fn report_for(
    run: u32,
    spec: &StdoutSpec,
    status: ExitStatus,
    stdout: StdoutCapture,
    stderr: StderrCapture,
    stdin_error: Option<String>,
) -> PostprocessReport {
    let failed = |cause, message: String| PostprocessOutcome::Failed { cause, message };
    let outcome = match status.code() {
        Some(0) => {
            if let Some(e) = stdout.error {
                failed(PostprocessCause::StdoutRead, format!("stdout read: {e}"))
            } else if let Some(e) = stdin_error {
                failed(PostprocessCause::StdinWrite, format!("stdin write: {e}"))
            } else {
                match collect_stdout(spec, &stdout.data, stdout.overflowed) {
                    Ok(stdout) => PostprocessOutcome::Collected { stdout },
                    Err((cause, message)) => failed(cause, message),
                }
            }
        }
        Some(code) => failed(
            PostprocessCause::NonzeroExit,
            format!("exited with code {code}"),
        ),
        None => failed(
            PostprocessCause::Signal,
            format!("exited without an exit code ({status})"),
        ),
    };
    PostprocessReport {
        run,
        exit_code: status.code(),
        stderr: Some(String::from_utf8_lossy(&stderr.tail).into_owned()),
        stderr_truncated: stderr.truncated,
        outcome,
    }
}

fn spawn_worker<T: Send + 'static>(
    name: String,
    f: impl FnOnce() -> T + Send + 'static,
) -> io::Result<Receiver<T>> {
    let (tx, rx) = mpsc::sync_channel(1);
    thread::Builder::new().name(name).spawn(move || {
        // 받는 쪽이 이미 포기했으면 결과를 버린다.
        if tx.send(f()).is_err() {
            tracing::debug!("postprocess pipe worker finished after the run was reported");
        }
    })?;
    Ok(rx)
}

fn write_stdin(mut pipe: std::process::ChildStdin, payload: &[u8]) -> io::Result<()> {
    pipe.write_all(payload)?;
    pipe.flush()
    // pipe 를 닫아 EOF 를 보낸다.
}

/// 상한까지 모으고, 넘으면 표시만 하고 EOF 까지 버린다(자식이 쓰기에서 멈추지 않게).
fn read_stdout(mut pipe: impl Read) -> StdoutCapture {
    let mut cap = StdoutCapture::default();
    let mut chunk = [0u8; 8192];
    loop {
        match pipe.read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => {
                if cap.overflowed {
                    continue;
                }
                if cap.data.len() + n > MAX_POSTPROCESS_STDOUT_BYTES {
                    cap.overflowed = true;
                    cap.data = Vec::new();
                } else {
                    cap.data.extend_from_slice(&chunk[..n]);
                }
            }
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => {
                cap.error = Some(e.to_string());
                break;
            }
        }
    }
    cap
}

fn read_stderr(mut pipe: impl Read) -> StderrCapture {
    let mut cap = StderrCapture::default();
    let mut chunk = [0u8; 8192];
    loop {
        match pipe.read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => {
                cap.tail.extend_from_slice(&chunk[..n]);
                if cap.tail.len() > POSTPROCESS_STDERR_TAIL_BYTES {
                    let excess = cap.tail.len() - POSTPROCESS_STDERR_TAIL_BYTES;
                    cap.tail.drain(..excess);
                    cap.truncated = true;
                }
            }
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            // stderr 는 진단용이라 읽기 오류로 실행을 실패시키지 않는다.
            Err(_) => break,
        }
    }
    cap
}

/// 직접 자식을 회수한다. 이미 회수했거나 오류면 상태가 없다.
fn reap(child: &mut Child) -> Option<ExitStatus> {
    match child.wait() {
        Ok(s) => Some(s),
        Err(e) => {
            tracing::warn!("postprocess reap pid {}: {e}", child.id());
            None
        }
    }
}

/// 이 실행이 만든 프로세스 묶음.
///
/// Unix 는 자식을 새 프로세스 그룹의 리더로 만들고 그룹 전체에 SIGKILL 을 보낸다. 자식이 스스로
/// 새 세션·그룹으로 옮긴 프로세스는 포함되지 않는다. Linux 는 종료한 리더를 회수하지 않은 채
/// 관찰해(`WNOWAIT`) 그룹을 종료할 때까지 그룹 id 가 다른 프로세스에 재사용되지 않게 한다.
/// 다른 Unix 는 리더를 회수한 뒤에는 그룹을 종료하지 않는다.
/// Windows 는 `KILL_ON_JOB_CLOSE` job 에 자식을 넣고 job 을 닫아 종료한다. job 에 넣기 전에
/// 자식이 만든 프로세스는 포함되지 않는다.
struct ProcessGroup {
    #[cfg(unix)]
    pgid: i32,
    /// 리더를 이미 회수했다. 그 뒤로는 그룹 id 를 믿지 않는다.
    #[cfg(unix)]
    leader_reaped: bool,
    #[cfg(windows)]
    job: Option<tasty_reaper::JobObject>,
}

impl ProcessGroup {
    fn configure(cmd: &mut Command) {
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            cmd.process_group(0);
        }
        #[cfg(not(unix))]
        let _unused = cmd;
    }

    fn adopt(pid: u32) -> Self {
        #[cfg(unix)]
        {
            Self {
                pgid: i32::try_from(pid).unwrap_or(0),
                leader_reaped: false,
            }
        }
        #[cfg(windows)]
        {
            let job = match tasty_reaper::JobObject::new() {
                Ok(job) => match job.assign_pid(pid) {
                    Ok(()) => Some(job),
                    Err(e) => {
                        tracing::warn!("postprocess job assign pid {pid}: {e}");
                        None
                    }
                },
                Err(e) => {
                    tracing::warn!("postprocess job create: {e}");
                    None
                }
            };
            Self { job }
        }
        #[cfg(not(any(unix, windows)))]
        {
            let _unused = pid;
            Self {}
        }
    }

    /// 직접 자식이 끝났는가. Linux 는 회수하지 않고 관찰한다.
    fn exited(&mut self, child: &mut Child) -> io::Result<bool> {
        #[cfg(target_os = "linux")]
        {
            let _unused = child;
            // SAFETY: siginfo_t 는 POD 이며 0 으로 채운 값이 유효하다. waitid 는 이 구조체만 쓴다.
            let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
            // SAFETY: info 는 유효한 포인터다. WNOWAIT 라 자식을 회수하지 않는다.
            let rc = unsafe {
                libc::waitid(
                    libc::P_PID,
                    self.pgid as libc::id_t,
                    &mut info,
                    libc::WEXITED | libc::WNOHANG | libc::WNOWAIT,
                )
            };
            if rc != 0 {
                return Err(io::Error::last_os_error());
            }
            // SAFETY: waitid 가 성공했으므로 si_pid 를 읽을 수 있다. 상태 변화가 없으면 0 이다.
            Ok(unsafe { info.si_pid() } != 0)
        }
        #[cfg(not(target_os = "linux"))]
        {
            let done = child.try_wait()?.is_some();
            #[cfg(unix)]
            if done {
                self.leader_reaped = true;
            }
            Ok(done)
        }
    }

    /// 묶음 전체를 종료한다. 직접 자식에도 종료 신호를 보낸다(회수는 호출자가 한다).
    fn kill(&mut self, child: &mut Child) {
        #[cfg(unix)]
        if self.pgid > 0 && !self.leader_reaped {
            // SAFETY: kill(2) 은 메모리를 건드리지 않는다. 음수 pid 는 그 그룹의 모든 프로세스다.
            // 리더를 아직 회수하지 않았으므로 그룹 id 가 다른 그룹에 재사용되지 않는다.
            let rc = unsafe { libc::kill(-self.pgid, libc::SIGKILL) };
            if rc != 0 {
                let e = io::Error::last_os_error();
                if e.raw_os_error() != Some(libc::ESRCH) {
                    tracing::warn!("postprocess kill group {}: {e}", self.pgid);
                }
            }
        }
        #[cfg(windows)]
        {
            // job 을 닫으면 KILL_ON_JOB_CLOSE 로 job 의 모든 프로세스가 끝난다.
            self.job.take();
        }
        if let Err(e) = child.kill()
            && e.kind() != io::ErrorKind::InvalidInput
        {
            tracing::debug!("postprocess kill pid {}: {e}", child.id());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Broken;

    impl Read for Broken {
        fn read(&mut self, _buf: &mut [u8]) -> io::Result<usize> {
            Err(io::Error::other("device went away"))
        }
    }

    #[test]
    fn a_stdout_read_error_fails_the_run_instead_of_using_partial_output() {
        let cap = read_stdout(io::Cursor::new(b"{\"a\":".to_vec()).chain(Broken));
        assert_eq!(cap.error.as_deref(), Some("device went away"));
        #[cfg(unix)]
        {
            use std::os::unix::process::ExitStatusExt;
            let r = report_for(
                1,
                &StdoutSpec::default(),
                ExitStatus::from_raw(0),
                cap,
                StderrCapture::default(),
                None,
            );
            assert_eq!(r.cause(), Some(PostprocessCause::StdoutRead));
        }
    }
}
