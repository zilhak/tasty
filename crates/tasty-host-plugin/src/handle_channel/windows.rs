//! Windows 보조 핸들 채널 — Named Pipe 서버측 구현 (overlapped I/O).
//!
//! Unix 의 `AF_UNIX` socket + `SCM_RIGHTS` 에 대응한다. 버퍼 핸들을 ancillary data 로
//! 실을 필요는 없다 — `tasty_shm::prepare_send` 의 `DuplicateHandle` 이 이미 plugin
//! 프로세스 핸들 테이블에 HANDLE 을 복제하므로, 그 결과 u64 를 `HandleAttach.handle` 에
//! in-band 로 실어 평범한 NDJSON 라인으로 보낸다.
//!
//! **왜 overlapped I/O 인가**: 이 채널은 full-duplex 다 — host 는 HandleAttach 를 write
//! 하면서 동시에 reader 스레드가 plugin 의 Dirty 를 blocking read 한다. Windows 의
//! *동기* 파일 핸들은 같은 file object 에 대한 I/O 를 직렬화하므로(그리고 `DuplicateHandle`
//! 은 같은 file object 를 가리킴), reader 의 blocking `ReadFile` 이 `WriteFile` 을 막아
//! HandleAttach 전송이 데드락된다. `FILE_FLAG_OVERLAPPED` + per-op event 로 read/write
//! 를 비직렬화해 이를 푼다. 각 stream 은 자기 event 를 소유하므로 서로 간섭하지 않는다.

#![cfg(windows)]

use std::io::{self, Read, Write};
use std::mem;
use std::ptr;

use windows_sys::Win32::Foundation::{
    CloseHandle, DUPLICATE_SAME_ACCESS, DuplicateHandle, ERROR_BROKEN_PIPE, ERROR_IO_PENDING,
    ERROR_PIPE_CONNECTED, GetLastError, HANDLE, INVALID_HANDLE_VALUE,
};
use windows_sys::Win32::Storage::FileSystem::{
    FILE_FLAG_FIRST_PIPE_INSTANCE, FILE_FLAG_OVERLAPPED, PIPE_ACCESS_DUPLEX, ReadFile, WriteFile,
};
use windows_sys::Win32::System::IO::{GetOverlappedResult, OVERLAPPED};
use windows_sys::Win32::System::Pipes::{
    ConnectNamedPipe, CreateNamedPipeW, PIPE_READMODE_BYTE, PIPE_REJECT_REMOTE_CLIENTS,
    PIPE_TYPE_BYTE, PIPE_UNLIMITED_INSTANCES, PIPE_WAIT,
};
use windows_sys::Win32::System::Threading::{CreateEventW, GetCurrentProcess, ResetEvent};

/// 파이프 입출력 버퍼 크기 힌트. 라인 기반 소량 트래픽이라 넉넉히 4 KiB.
const PIPE_BUFFER_SIZE: u32 = 4096;

/// close-on-drop HANDLE RAII. windows-sys 는 OwnedHandle 을 주지 않으므로 자체 정의.
struct OwnedHandle(HANDLE);

impl OwnedHandle {
    fn as_raw(&self) -> HANDLE {
        self.0
    }
}

impl Drop for OwnedHandle {
    fn drop(&mut self) {
        if !self.0.is_null() && self.0 != INVALID_HANDLE_VALUE {
            // SAFETY: null/INVALID 이 아닐 때만, Drop 은 한 번만 실행되므로 double-close 없음.
            unsafe {
                CloseHandle(self.0);
            }
        }
    }
}

// SAFETY: HANDLE 은 OS 관리 정수. 스레드 간 이동/공유해도 커널이 안전 처리하며
// CloseHandle/ReadFile/WriteFile 은 thread-safe.
unsafe impl Send for OwnedHandle {}
// SAFETY: HANDLE 은 OS 관리 정수. 공유 참조로 스레드 간 공유해도 커널이 안전 처리하며
// CloseHandle/ReadFile/WriteFile 은 thread-safe(Send 와 동일 근거).
unsafe impl Sync for OwnedHandle {}

/// per-op auto-reset event 를 만든다. overlapped 완료 대기에 쓴다.
fn make_event() -> io::Result<OwnedHandle> {
    // SAFETY: Win32 CreateEventW. 수동 리셋(false)·초기 비신호(false)·무명.
    let h = unsafe { CreateEventW(ptr::null(), 0, 0, ptr::null()) };
    if h.is_null() {
        return Err(io::Error::last_os_error());
    }
    Ok(OwnedHandle(h))
}

/// Named Pipe 한 인스턴스를 감싸는 서버측 stream. `Read`/`Write` 로 라인 R/W.
///
/// overlapped I/O 라 read 와 write 가 직렬화되지 않는다. 각 stream 은 자기 완료 event 를
/// 소유한다. [`PipeServerStream::try_clone`] 은 핸들 복제 + 새 event 로 reader 스레드용
/// stream 을 만든다(같은 file object 지만 op 별 OVERLAPPED 라 안전).
pub(super) struct PipeServerStream {
    handle: OwnedHandle,
    event: OwnedHandle,
}

impl PipeServerStream {
    fn new(handle: OwnedHandle) -> io::Result<Self> {
        let event = make_event()?;
        Ok(Self { handle, event })
    }

    /// overlapped op 하나를 blocking 으로 수행한다. `start` 가 ReadFile/WriteFile 을
    /// 호출하고, ERROR_IO_PENDING 이면 GetOverlappedResult 로 완료를 기다린다. 반환은
    /// 전송 바이트 수. BROKEN_PIPE 는 EOF(0) 로 정규화.
    fn blocking_io(
        &self,
        start: impl FnOnce(HANDLE, *mut OVERLAPPED, *mut u32) -> i32,
    ) -> io::Result<usize> {
        // SAFETY: ResetEvent 는 유효 event 핸들에 대해 안전.
        unsafe {
            ResetEvent(self.event.as_raw());
        }
        // SAFETY: OVERLAPPED 은 POD 이며 all-zero 가 유효한 초기 상태다.
        let mut ov: OVERLAPPED = unsafe { mem::zeroed() };
        ov.hEvent = self.event.as_raw();
        let mut transferred: u32 = 0;
        let ok = start(self.handle.as_raw(), &mut ov, &mut transferred);
        if ok == 0 {
            // SAFETY: 부수효과 없는 last-error 조회.
            let err = unsafe { GetLastError() };
            if err == ERROR_BROKEN_PIPE {
                return Ok(0);
            }
            if err != ERROR_IO_PENDING {
                return Err(io::Error::from_raw_os_error(err as i32));
            }
            // 완료 대기(bWait=TRUE).
            // SAFETY: 유효 핸들·OVERLAPPED. transferred out param.
            let done =
                unsafe { GetOverlappedResult(self.handle.as_raw(), &ov, &mut transferred, 1) };
            if done == 0 {
                // SAFETY: 부수효과 없는 last-error 조회.
                let e2 = unsafe { GetLastError() };
                if e2 == ERROR_BROKEN_PIPE {
                    return Ok(0);
                }
                return Err(io::Error::from_raw_os_error(e2 as i32));
            }
        }
        Ok(transferred as usize)
    }

    /// 현재 프로세스 내에서 파이프 핸들을 복제한 새 stream(새 event). Unix `try_clone` 대응.
    pub(super) fn try_clone(&self) -> io::Result<Self> {
        let mut dup: HANDLE = ptr::null_mut();
        // SAFETY: 유효 핸들 → 같은 프로세스로 복제. 실패 시 rc==0.
        let rc = unsafe {
            DuplicateHandle(
                GetCurrentProcess(),
                self.handle.as_raw(),
                GetCurrentProcess(),
                &mut dup,
                0,
                0,
                DUPLICATE_SAME_ACCESS,
            )
        };
        if rc == 0 {
            return Err(io::Error::last_os_error());
        }
        Self::new(OwnedHandle(dup))
    }

    /// 파이프 인스턴스에서 클라이언트 연결을 blocking 으로 기다린다(overlapped).
    fn accept(&self) -> io::Result<()> {
        // SAFETY: ResetEvent 는 유효 event 핸들에 안전.
        unsafe {
            ResetEvent(self.event.as_raw());
        }
        // SAFETY: OVERLAPPED 은 POD 이며 all-zero 가 유효한 초기 상태다.
        let mut ov: OVERLAPPED = unsafe { mem::zeroed() };
        ov.hEvent = self.event.as_raw();
        // SAFETY: 유효 파이프 인스턴스 핸들 + overlapped.
        let rc = unsafe { ConnectNamedPipe(self.handle.as_raw(), &mut ov) };
        if rc != 0 {
            return Ok(());
        }
        // SAFETY: 부수효과 없는 last-error 조회.
        let err = unsafe { GetLastError() };
        if err == ERROR_PIPE_CONNECTED {
            return Ok(()); // accept 직전 이미 연결됨
        }
        if err != ERROR_IO_PENDING {
            return Err(io::Error::from_raw_os_error(err as i32));
        }
        let mut dummy: u32 = 0;
        // SAFETY: 유효 핸들·OVERLAPPED. 완료 대기.
        let done = unsafe { GetOverlappedResult(self.handle.as_raw(), &ov, &mut dummy, 1) };
        if done == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }
}

impl Write for PipeServerStream {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let n = self.blocking_io(|h, ov, transferred| {
            // SAFETY: 유효 핸들, buf 는 len 만큼 유효, overlapped write.
            unsafe { WriteFile(h, buf.as_ptr(), buf.len() as u32, transferred, ov) }
        })?;
        Ok(n)
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl Read for PipeServerStream {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.blocking_io(|h, ov, transferred| {
            // SAFETY: 유효 핸들, buf 는 len 만큼 유효, overlapped read.
            unsafe { ReadFile(h, buf.as_mut_ptr(), buf.len() as u32, transferred, ov) }
        })
    }
}

/// `\\.\pipe\...` 파이프 이름을 UTF-16 NUL 종단 벡터로 만든다.
fn to_wide(name: &str) -> Vec<u16> {
    name.encode_utf16().chain(std::iter::once(0)).collect()
}

/// 새 파이프 인스턴스를 만든다(overlapped). `first` 면 `FILE_FLAG_FIRST_PIPE_INSTANCE` 로
/// 같은 이름의 선점 인스턴스를 방지한다(중복 bind 조기 실패).
pub(super) fn create_pipe_instance(name: &str, first: bool) -> io::Result<PipeServerStream> {
    let wide = to_wide(name);
    let mut open_mode = PIPE_ACCESS_DUPLEX | FILE_FLAG_OVERLAPPED;
    if first {
        open_mode |= FILE_FLAG_FIRST_PIPE_INSTANCE;
    }
    // SAFETY: Win32 CreateNamedPipeW. wide 는 NUL 종단 UTF-16, 나머지는 상수.
    let raw = unsafe {
        CreateNamedPipeW(
            wide.as_ptr(),
            open_mode,
            PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
            PIPE_UNLIMITED_INSTANCES,
            PIPE_BUFFER_SIZE,
            PIPE_BUFFER_SIZE,
            0,
            ptr::null(),
        )
    };
    if raw == INVALID_HANDLE_VALUE || raw.is_null() {
        return Err(io::Error::last_os_error());
    }
    PipeServerStream::new(OwnedHandle(raw))
}

/// 파이프 인스턴스에서 클라이언트 연결을 blocking 으로 기다린다.
pub(super) fn accept(stream: &PipeServerStream) -> io::Result<()> {
    stream.accept()
}

/// 프로세스 유일 파이프 이름. Unix 의 socket path 대응(pid + 단조 seq).
pub(super) fn unique_pipe_name(seq: u64) -> String {
    format!(r"\\.\pipe\tasty-handle-{}-{seq}", std::process::id())
}
