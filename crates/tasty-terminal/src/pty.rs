//! OS PTY 연결. 자식 프로세스 생성, raw read/write, OS resize 적용, 종료 관측·회수를 맡는다.
//!
//! Pty는 VT 해석과 grid를 모른다. 읽은 raw byte는 [`PtyOutput`] 계약으로 넘기고,
//! 쓸 byte는 [`OutputSink`] 채널로 받는다. reader worker가 받은 byte를 바로
//! Terminal에 ingest하더라도 그 상태와 lock은 Terminal 쪽 소유다.

use std::io::{Read, Write};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, mpsc};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::Result;
use portable_pty::{CommandBuilder, NativePtySystem, PtySize, PtySystem};

use crate::TerminalConfig;
use crate::sink::{OutputSink, WriteProgress, lock_write_progress, new_write_progress};

/// `Pty::drop` 누적 소요(ns) — [`pty_drop_totals`] 참조.
static PTY_DROP_NANOS: AtomicU64 = AtomicU64::new(0);
/// `Pty::drop` 누적 횟수 — [`pty_drop_totals`] 참조.
static PTY_DROP_COUNT: AtomicU64 = AtomicU64::new(0);

/// PTY Drop의 누적 시간과 횟수. 특정 종료 구간의 비용은 호출 전후 차이로 계산한다.
pub fn pty_drop_totals() -> (Duration, u64) {
    (
        Duration::from_nanos(PTY_DROP_NANOS.load(Ordering::Relaxed)),
        PTY_DROP_COUNT.load(Ordering::Relaxed),
    )
}

/// Minimum interval between child-alive `try_wait` syscalls.
pub(crate) const ALIVE_CHECK_INTERVAL: Duration = Duration::from_millis(500);

/// Initial post-EOF wake interval, doubled up to ALIVE_CHECK_INTERVAL until exit is handled.
pub(crate) const EOF_REWAKE_FIRST: Duration = Duration::from_millis(10);

/// Throttle interval for OS resize notifications.
const RESIZE_THROTTLE: Duration = Duration::from_millis(100);

/// reader worker가 읽은 raw byte를 받는 쪽의 계약.
pub(crate) trait PtyOutput: Send + 'static {
    /// 읽은 청크를 처리한다. 받는 쪽이 사라졌으면 false를 반환해 reader를 멈춘다.
    fn on_bytes(&self, data: &[u8]) -> bool;
    /// EOF/읽기 오류를 알린다. 이 호출 전에 [`PtyState::reader_eof`]가 설정된다.
    fn on_eof(&self);
    /// 종료 관측을 재촉하도록 받는 쪽을 깨운다.
    fn wake(&self);
    /// 받는 쪽이 아직 살아 있는지. EOF 뒤 재촉 반복의 종료 조건이다.
    fn is_attached(&self) -> bool;
}

/// Pty의 현재 runtime 상태. OS 자원 자체는 [`Pty`]가 따로 소유한다.
pub(crate) struct PtyState {
    /// grid는 이미 바뀌었고 OS에 아직 알리지 않은 크기.
    pub(crate) pending_resize: Option<(usize, usize)>,
    /// 마지막 OS resize flush 시각. throttle에 사용한다.
    pub(crate) last_resize_flush: Instant,
    /// 마지막 `try_wait` 시각.
    pub(crate) last_alive_check: Instant,
    /// 자식 종료를 이미 관측했는지. 관측은 한 번만 보고한다.
    pub(crate) exit_observed: bool,
    /// reader worker가 EOF/오류를 만났는지. 다음 관측이 throttle 없이 확인한다.
    pub(crate) reader_eof: Arc<AtomicBool>,
    /// 종료를 관측했거나 자식을 넘겨 EOF 뒤 재촉이 더는 필요 없는지.
    pub(crate) exit_settled: Arc<AtomicBool>,
}

impl PtyState {
    fn new() -> Self {
        Self {
            pending_resize: None,
            last_resize_flush: Instant::now(),
            // Start in the past so the first observation always checks immediately.
            last_alive_check: Instant::now() - ALIVE_CHECK_INTERVAL,
            exit_observed: false,
            reader_eof: Arc::new(AtomicBool::new(false)),
            exit_settled: Arc::new(AtomicBool::new(false)),
        }
    }
}

/// spawn 직후 아직 worker를 시작하지 않은 PTY 읽기 끝.
pub(crate) struct PtyReader(Box<dyn Read + Send>);

/// OS PTY와 자식 프로세스의 소유자.
pub(crate) struct Pty {
    pub(crate) state: PtyState,
    _writer_thread: thread::JoinHandle<()>,
    /// Drop의 계측 구간 안에서 master를 take해 해제한다. 살아 있는 동안은 Some이다.
    master: Option<Box<dyn portable_pty::MasterPty + Send>>,
    /// 외부 exit watcher가 take_child로 가져가기 전까지 소유하는 자식.
    /// Surface 터미널은 자식을 넘기지 않고 자체 kill/reap을 사용한다.
    child: Option<Box<dyn portable_pty::Child + Send + Sync>>,
    /// raw read worker. [`Pty::start_reader`] 전에는 None이다.
    _reader_thread: Option<thread::JoinHandle<()>>,
}

impl Pty {
    /// PTY를 열고 자식을 실행한 뒤 writer worker를 시작한다. 반환한 sink가 writer로 가는
    /// 유일한 입력 연결이며, 모든 sink가 사라지면 writer가 끝난다. reader는 호출자가
    /// 받는 쪽을 준비한 뒤 [`start_reader`](Self::start_reader)로 시작한다.
    pub(crate) fn spawn(config: &TerminalConfig<'_>) -> Result<(Self, PtyReader, OutputSink)> {
        let pair = NativePtySystem::default().openpty(pty_size(config.cols, config.rows))?;

        let shell = match config.shell {
            Some(s) if !s.is_empty() => s.to_string(),
            _ => default_shell(),
        };
        let cmd = build_shell_command(
            &shell,
            config.args,
            config.surface_id,
            config.working_dir,
            config.extra_env,
        );

        let child = pair.slave.spawn_command(cmd)?;
        drop(pair.slave);

        // Windows Job Object에 자식 등록을 시도한다. 미초기화·다른 OS에서는 동작하지 않는다.
        tasty_reaper::adopt_pid(child.process_id());

        let mut pty_writer = pair.master.take_writer()?;
        let pty_reader = pair.master.try_clone_reader()?;

        // writer 스레드가 시작되기 전에 초기 입력을 쓴다. 자식은 이미 실행 중이므로
        // 셸의 tcflush/TCSAFLUSH 등으로 입력이 사라질 수 있다.
        if let Some(input) = config.initial_input
            && !input.is_empty()
        {
            if let Err(e) = pty_writer.write_all(input.as_bytes()) {
                tracing::warn!("initial_input write_all failed: {e}");
            } else if let Err(e) = pty_writer.flush() {
                tracing::warn!("initial_input flush failed: {e}");
            }
        }

        let write_progress = new_write_progress();
        let progress_for_writer = Arc::clone(&write_progress);
        let (write_tx, write_rx) = mpsc::channel::<Vec<u8>>();
        let writer_thread =
            thread::spawn(move || run_writer_loop(pty_writer, write_rx, progress_for_writer));
        let sink = OutputSink::with_progress(write_tx, write_progress);

        let pty = Self {
            state: PtyState::new(),
            _writer_thread: writer_thread,
            master: Some(pair.master),
            child: Some(child),
            _reader_thread: None,
        };
        Ok((pty, PtyReader(pty_reader), sink))
    }

    /// raw read worker를 시작한다. 청크마다 `output.on_bytes`를 호출하고 EOF 뒤에는
    /// 종료를 관측하거나 자식을 넘기거나 받는 쪽이 사라질 때까지 간격을 늘려 깨운다.
    pub(crate) fn start_reader(&mut self, reader: PtyReader, output: impl PtyOutput) {
        let mut reader = reader.0;
        let eof = Arc::clone(&self.state.reader_eof);
        let settled = Arc::clone(&self.state.exit_settled);
        self._reader_thread = Some(thread::spawn(move || {
            let mut buf = [0u8; 8192];
            loop {
                match reader.read(&mut buf) {
                    Ok(0) => break,
                    Ok(n) => {
                        if !output.on_bytes(&buf[..n]) {
                            return;
                        }
                    }
                    Err(_) => break,
                }
            }
            // PTY EOF/error: the next observation checks immediately (bypassing the throttle).
            eof.store(true, Ordering::Release);
            output.on_eof();
            // EOF can precede a waitable child exit. Keep waking with increasing intervals
            // until the exit is observed, the child is transferred, or the receiver is gone.
            let mut gap = EOF_REWAKE_FIRST;
            loop {
                output.wake();
                thread::sleep(gap);
                if settled.load(Ordering::Acquire) || !output.is_attached() {
                    break;
                }
                gap = (gap * 2).min(ALIVE_CHECK_INTERVAL);
            }
        }));
    }

    /// grid 변경 뒤 OS resize를 예약한다. 실제 적용은 flush 경로에서 한다.
    pub(crate) fn schedule_resize(&mut self, cols: usize, rows: usize) {
        self.state.pending_resize = Some((cols, rows));
    }

    pub(crate) fn has_pending_resize(&self) -> bool {
        self.state.pending_resize.is_some()
    }

    /// 예약된 OS resize를 throttle에 맞춰 적용한다. 적용했으면 true, 예약이 없거나
    /// throttle에 걸렸으면 false다(throttle이면 예약은 유지된다).
    pub(crate) fn flush_resize(&mut self) -> bool {
        if self.state.pending_resize.is_none() {
            return false;
        }
        if self.state.last_resize_flush.elapsed() < RESIZE_THROTTLE {
            return false;
        }
        self.force_flush_resize();
        true
    }

    /// throttle과 무관하게 예약된 OS resize를 적용한다.
    pub(crate) fn force_flush_resize(&mut self) {
        if let Some((cols, rows)) = self.state.pending_resize.take() {
            if let Err(e) = self.apply_os_resize(cols, rows) {
                tracing::warn!("PTY resize failed: {e}");
            }
            self.state.last_resize_flush = Instant::now();
        }
    }

    /// OS PTY에 크기를 바로 알린다. master가 없으면 아무 일도 하지 않는다.
    pub(crate) fn apply_os_resize(&self, cols: usize, rows: usize) -> Result<()> {
        if let Some(master) = self.master.as_ref() {
            master.resize(pty_size(cols, rows))?;
        }
        Ok(())
    }

    /// 자식 종료를 관측한다. 이번 호출에서 처음 종료를 확인했으면 true다.
    /// `try_wait`는 ALIVE_CHECK_INTERVAL마다 한 번이며 reader EOF 뒤에는 즉시 확인한다.
    pub(crate) fn observe_exit(&mut self) -> bool {
        if self.state.exit_observed {
            return false;
        }
        let reader_gone = self.state.reader_eof.load(Ordering::Acquire);
        if !reader_gone && self.state.last_alive_check.elapsed() < ALIVE_CHECK_INTERVAL {
            return false;
        }
        self.state.last_alive_check = Instant::now();
        if self.check_alive() {
            return false;
        }
        self.state.exit_observed = true;
        self.state.exit_settled.store(true, Ordering::Release);
        true
    }

    /// OS PTY에 현재 적용된 크기 `(cols, rows)`.
    #[cfg(test)]
    pub(crate) fn os_size(&self) -> Option<(usize, usize)> {
        let size = self.master.as_ref()?.get_size().ok()?;
        Some((usize::from(size.cols), usize::from(size.rows)))
    }

    pub(crate) fn process_id(&self) -> Option<u32> {
        self.child.as_ref()?.process_id()
    }

    /// 자식이 아직 실행 중인지. 소유한 자식이 없으면 true다.
    pub(crate) fn is_alive(&mut self) -> bool {
        match self.child.as_mut() {
            Some(child) => child.try_wait().ok().flatten().is_none(),
            None => true,
        }
    }

    /// 자식 종료를 확인하면 false다. 소유한 자식이 없거나 조회가 실패하면 true다.
    pub(crate) fn check_alive(&mut self) -> bool {
        match self.child.as_mut() {
            Some(child) => !matches!(child.try_wait(), Ok(Some(_status))),
            None => true,
        }
    }

    /// 자식의 kill/wait 소유권을 넘긴다. 이후 EOF 뒤 재촉과 Drop의 종료 처리는 하지 않는다.
    pub(crate) fn take_child(&mut self) -> Option<Box<dyn portable_pty::Child + Send + Sync>> {
        self.state.exit_settled.store(true, Ordering::Release);
        self.child.take()
    }
}

impl Drop for Pty {
    /// 정상 Drop에서는 자식 종료를 시도한다. Windows의 비정상 종료 처리는
    /// Job Object에 등록된 자식에 한해 tasty_reaper가 맡는다.
    fn drop(&mut self) {
        // take_child로 넘긴 자식의 종료·회수는 새 소유자가 맡는다.
        let Some(child) = self.child.as_mut() else {
            return;
        };
        let t_drop = Instant::now();
        // Unix는 SIGHUP만 보내고 대기·강제 종료·회수는 아래 스레드에 맡긴다.
        // portable-pty kill의 동기 유예 대기를 메인 스레드에서 반복하지 않기 위해서다.
        // Windows는 기존 kill 경로를 사용한다.
        #[cfg(unix)]
        let signalled_pid = child.process_id();
        #[cfg(unix)]
        match signalled_pid {
            // SAFETY: kill syscall. 대상은 본 프로세스의 자식 pid 이고, 아직
            // 회수 전(아래 reap 스레드가 회수한다)이라 pid 재사용이 일어날 수 없다.
            Some(pid) => unsafe {
                if libc::kill(pid as i32, libc::SIGHUP) != 0 {
                    tracing::trace!(
                        "pty child SIGHUP on drop failed (already exited?): {}",
                        std::io::Error::last_os_error()
                    );
                }
            },
            // pid 를 못 얻는 예외 경로에서만 blocking kill 로 폴백한다.
            None => {
                if let Err(e) = child.kill() {
                    tracing::trace!("pty child kill on drop failed (already exited?): {e}");
                }
            }
        }
        #[cfg(not(unix))]
        if let Err(e) = child.kill() {
            tracing::trace!("pty child kill on drop failed (already exited?): {e}");
        }
        // 메인 스레드 밖에서 유예 대기 후 필요하면 SIGKILL을 보내고 자식을 회수한다.
        #[cfg(unix)]
        if let Some(pid) = signalled_pid {
            thread::spawn(move || {
                let pid = pid as i32;
                let mut status = 0i32;
                for _ in 0..40 {
                    // SAFETY: waitpid 는 본 프로세스의 자식 pid 에만 매칭된다. 이미
                    // 다른 곳에서 회수됐으면 -1(ECHILD) 로 즉시 반환된다.
                    match unsafe { libc::waitpid(pid, &raw mut status, libc::WNOHANG) } {
                        0 => thread::sleep(Duration::from_millis(5)),
                        _ => return, // >0 회수 완료, -1 이미 회수됨/자식 아님
                    }
                }
                // 유예가 끝나면 SIGKILL을 보내고 회수한다. waitpid의 완료 시간은 보장하지 않는다.
                // SAFETY: kill syscall. pid 는 아직 미회수 자식(위 waitpid 가 0 반환)
                // 이므로 재사용될 수 없다.
                unsafe {
                    libc::kill(pid, libc::SIGKILL);
                }
                // SAFETY: waitpid syscall — 본 프로세스의 자식 pid 에만 매칭.
                unsafe {
                    libc::waitpid(pid, &raw mut status, 0);
                }
            });
        }
        // master 해제를 계측 구간 안으로 끌어들인다(위 필드 주석 참조).
        drop(self.master.take());
        PTY_DROP_NANOS.fetch_add(
            u64::try_from(t_drop.elapsed().as_nanos()).unwrap_or(u64::MAX),
            Ordering::Relaxed,
        );
        PTY_DROP_COUNT.fetch_add(1, Ordering::Relaxed);
    }
}

fn pty_size(cols: usize, rows: usize) -> PtySize {
    PtySize {
        rows: rows as u16,
        cols: cols as u16,
        pixel_width: 0,
        pixel_height: 0,
    }
}

/// PTY writer 스레드 본체 — 큐에 들어오는 write 를 순서대로 PTY 에
/// write_all+flush 하고, 각 성공마다 `progress` 카운터를 올려 `\r` 를 별도로
/// write 로 보내는 IPC 핸들러(`send_text_to_surface_with_ack`)가 `WriteAck` 로 실제 flush 완료를 확인할 수
/// 있게 한다.
pub(crate) fn run_writer_loop(
    mut pty_writer: Box<dyn Write + Send>,
    write_rx: mpsc::Receiver<Vec<u8>>,
    progress: WriteProgress,
) {
    while let Ok(data) = write_rx.recv() {
        if pty_writer.write_all(&data).is_err() {
            break;
        }
        if pty_writer.flush().is_err() {
            break;
        }
        let (count, cvar) = &*progress;
        *lock_write_progress(count) += 1;
        cvar.notify_all();
    }
}

fn default_shell() -> String {
    #[cfg(windows)]
    {
        std::env::var("COMSPEC").unwrap_or_else(|_| "cmd.exe".to_string())
    }
    #[cfg(not(windows))]
    {
        std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string())
    }
}

/// 자식 셸의 인자·환경변수·작업 디렉터리를 구성한다.
fn build_shell_command(
    shell: &str,
    args: &[&str],
    surface_id: u32,
    working_dir: Option<&std::path::Path>,
    extra_env: &[(&str, &str)],
) -> CommandBuilder {
    let mut cmd = CommandBuilder::new(shell);
    // Launch as interactive login shell so .zshrc/.bashrc and themes are loaded —
    // *unless* `args` already carries an explicit `--rcfile`-based bash startup
    // (`GeneralSettings::effective_shell_args`): bash's `--rcfile` is
    // silently ignored for login shells (`bash(1)`: it only applies to
    // interactive *non-login* shells), so login mode must be dropped in that
    // case — the args themselves append `-i` to keep the shell interactive.
    #[cfg(not(windows))]
    if !args.contains(&"--rcfile") {
        cmd.arg("-li");
    }
    for arg in args {
        if !arg.is_empty() {
            cmd.arg(arg);
        }
    }
    for (key, value) in extra_env {
        cmd.env(key, value);
    }
    cmd.env("TERM", "xterm-256color");
    cmd.env("TASTY_SURFACE_ID", surface_id.to_string());
    // 자식이 부모의 데이터 경로를 찾도록 정보용 TASTY_PARENT_HOME을 전달한다.
    // TASTY_HOME으로 주입하면 자식 Tasty의 debug/release별 경로 선택을 덮어쓰게 된다.
    if let Some(home) = tasty_utils::path::tasty_home() {
        cmd.env("TASTY_PARENT_HOME", &home);
    }

    // Remove CMUX_* environment variables so cmux CLI doesn't work inside tasty terminals.
    for (key, _) in std::env::vars() {
        if key.starts_with("CMUX_") {
            cmd.env_remove(&key);
        }
    }

    // Add tasty's own binary directory to PATH so `tasty` CLI works inside the
    // terminal. hook_handler::trigger::spawn_shell 와 동일한 보강을 공유
    // 헬퍼로 적용해 두 경로의 동작을 일치시킨다(패키징된 macOS `.app` 의
    // 최소 PATH 에서 `tasty` self 호출 해결).
    if let Some(new_path) = tasty_utils::process::path_prepending_self_dir(std::env::var_os("PATH"))
    {
        cmd.env("PATH", new_path);
    }

    if let Some(dir) = working_dir {
        cmd.cwd(dir);
    }
    cmd
}

#[cfg(all(test, unix))]
mod tests {
    use std::sync::Mutex;

    use super::*;

    /// VT 해석 없이 raw byte와 EOF만 기록하는 받는 쪽.
    #[derive(Clone, Default)]
    struct Recorder {
        bytes: Arc<Mutex<Vec<u8>>>,
        eof: Arc<AtomicBool>,
        wakes: Arc<AtomicU64>,
        detached: Arc<AtomicBool>,
    }

    impl PtyOutput for Recorder {
        fn on_bytes(&self, data: &[u8]) -> bool {
            self.bytes.lock().expect("recorder").extend_from_slice(data);
            true
        }
        fn on_eof(&self) {
            self.eof.store(true, Ordering::Release);
        }
        fn wake(&self) {
            self.wakes.fetch_add(1, Ordering::Relaxed);
        }
        fn is_attached(&self) -> bool {
            !self.detached.load(Ordering::Acquire)
        }
    }

    fn spawn_sh(args: &[&str]) -> (Pty, PtyReader, OutputSink) {
        Pty::spawn(&TerminalConfig {
            cols: 80,
            rows: 24,
            shell: Some("/bin/sh"),
            args,
            surface_id: 0,
            working_dir: None,
            initial_input: None,
            extra_env: &[],
        })
        .expect("pty spawn")
    }

    fn wait_until(what: &str, mut cond: impl FnMut() -> bool) {
        let deadline = Instant::now() + Duration::from_secs(10);
        while !cond() {
            assert!(
                Instant::now() < deadline,
                "{what}: 10초 안에 일어나지 않았다"
            );
            thread::sleep(Duration::from_millis(10));
        }
    }

    /// Terminal 없이도 raw 출력·EOF·종료 관측이 계약대로 전달된다.
    #[test]
    fn raw_output_eof_and_exit_reach_a_non_terminal_receiver() {
        let (mut pty, reader, _sink) = spawn_sh(&["-c", "printf tasty-pty-raw"]);
        let rec = Recorder::default();
        pty.start_reader(reader, rec.clone());

        wait_until("EOF", || rec.eof.load(Ordering::Acquire));
        assert!(pty.state.reader_eof.load(Ordering::Acquire));
        let out = String::from_utf8_lossy(&rec.bytes.lock().expect("recorder")).into_owned();
        assert!(out.contains("tasty-pty-raw"), "raw 출력: {out:?}");

        wait_until("exit", || pty.observe_exit());
        assert!(pty.state.exit_settled.load(Ordering::Acquire));
        assert!(!pty.observe_exit(), "종료 관측은 한 번만 보고한다");
    }

    /// sink로 보낸 byte가 자식의 stdin에 도달하고 writer 완료 카운터가 오른다.
    #[test]
    fn sink_bytes_reach_the_child_and_advance_progress() {
        let (mut pty, reader, sink) = spawn_sh(&["-c", "read line; printf \"got:%s\" \"$line\""]);
        let rec = Recorder::default();
        pty.start_reader(reader, rec.clone());

        sink.send(b"ping\r".to_vec()).expect("writer alive");
        wait_until("writer flush", || {
            *lock_write_progress(&sink.progress().0) >= 1
        });
        wait_until("echo", || {
            String::from_utf8_lossy(&rec.bytes.lock().expect("recorder")).contains("got:ping")
        });
    }

    /// take_child 뒤에는 EOF 재촉을 멈추고 Drop은 넘긴 자식을 건드리지 않는다.
    #[test]
    fn take_child_settles_and_transfers_reaping() {
        let (mut pty, reader, _sink) = spawn_sh(&["-c", "exit 0"]);
        pty.start_reader(reader, Recorder::default());
        let mut child = pty.take_child().expect("owned child");
        assert!(pty.state.exit_settled.load(Ordering::Acquire));
        assert_eq!(pty.process_id(), None);
        assert!(pty.check_alive(), "넘긴 뒤에는 자식이 없어 alive로 본다");
        drop(pty);
        let status = child.wait().expect("새 소유자가 회수한다");
        assert!(status.success());
    }
}
