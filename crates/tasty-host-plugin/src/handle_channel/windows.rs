//! Windows 보조 핸들 채널 — Named Pipe 서버측 구현.
//!
//! Unix 의 `AF_UNIX` socket + `SCM_RIGHTS` 에 대응한다. 다만 Windows 는 버퍼 핸들을
//! ancillary data 로 실어 보낼 필요가 없다 — `tasty_shm::prepare_send` 의
//! `DuplicateHandle` 이 이미 plugin 프로세스 핸들 테이블에 HANDLE 을 복제해 넣으므로,
//! 그 결과 u64 를 [`HandleChannelMessage::HandleAttach`] 의 `handle` 필드에 in-band 로
//! 실어 평범한 NDJSON 라인으로 보내면 된다.
//!
//! 따라서 이 모듈은 (1) Named Pipe 서버 인스턴스 생성/accept 와 (2) 라인 R/W 만
//! 담당하고, 핸들 복제 자체는 상위(`manager::buffer`)가 `tasty_shm` 으로 수행한다.

#![cfg(windows)]

use std::io::{self, Read, Write};

use windows_sys::Win32::Foundation::{
    CloseHandle, DUPLICATE_SAME_ACCESS, DuplicateHandle, ERROR_PIPE_CONNECTED, GetLastError, HANDLE,
    INVALID_HANDLE_VALUE,
};
use windows_sys::Win32::Storage::FileSystem::{
    FILE_FLAG_FIRST_PIPE_INSTANCE, PIPE_ACCESS_DUPLEX, ReadFile, WriteFile,
};
use windows_sys::Win32::System::Pipes::{
    ConnectNamedPipe, CreateNamedPipeW, DisconnectNamedPipe, PIPE_READMODE_BYTE,
    PIPE_REJECT_REMOTE_CLIENTS, PIPE_TYPE_BYTE, PIPE_UNLIMITED_INSTANCES, PIPE_WAIT,
};
use windows_sys::Win32::System::Threading::GetCurrentProcess;

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
unsafe impl Sync for OwnedHandle {}

/// Named Pipe 한 인스턴스를 감싸는 서버측 stream. `Read`/`Write` 로 라인 R/W.
///
/// [`HandleStream`](super::HandleStream) 의 Windows inner. 송신 스레드가 이 stream 을
/// 잡고, [`PipeServerStream::try_clone`] 이 만든 사본을 reader 스레드가 잡는다(양방향
/// duplex 파이프라 read/write 를 별도 스레드에서 동시에 해도 안전).
pub(super) struct PipeServerStream {
    handle: OwnedHandle,
}

impl PipeServerStream {
    fn from_owned(handle: OwnedHandle) -> Self {
        Self { handle }
    }

    /// 현재 프로세스 내에서 파이프 핸들을 복제한 새 stream. Unix `try_clone` 대응.
    pub(super) fn try_clone(&self) -> io::Result<Self> {
        let mut dup: HANDLE = std::ptr::null_mut();
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
        Ok(Self::from_owned(OwnedHandle(dup)))
    }
}

impl Write for PipeServerStream {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let mut written: u32 = 0;
        // SAFETY: 유효 파이프 핸들, buf 는 len 만큼 유효, written 은 out param.
        let rc = unsafe {
            WriteFile(
                self.handle.as_raw(),
                buf.as_ptr(),
                buf.len() as u32,
                &mut written,
                std::ptr::null_mut(),
            )
        };
        if rc == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(written as usize)
    }

    fn flush(&mut self) -> io::Result<()> {
        // 동기 파이프라 WriteFile 이 반환하면 커널 버퍼에 이미 들어감. 별도 flush 불요.
        Ok(())
    }
}

impl Read for PipeServerStream {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let mut read: u32 = 0;
        // SAFETY: 유효 파이프 핸들, buf 는 len 만큼 유효, read 는 out param.
        let rc = unsafe {
            ReadFile(
                self.handle.as_raw(),
                buf.as_mut_ptr(),
                buf.len() as u32,
                &mut read,
                std::ptr::null_mut(),
            )
        };
        if rc == 0 {
            let err = io::Error::last_os_error();
            // 파이프 반대편이 닫히면 BrokenPipe — EOF(0) 로 정규화해 상위 라인 reader 가
            // 연결 종료로 처리하게 한다.
            if err.kind() == io::ErrorKind::BrokenPipe {
                return Ok(0);
            }
            return Err(err);
        }
        Ok(read as usize)
    }
}

/// `\\.\pipe\...` 파이프 이름을 UTF-16 NUL 종단 벡터로 만든다.
fn to_wide(name: &str) -> Vec<u16> {
    name.encode_utf16().chain(std::iter::once(0)).collect()
}

/// 새 파이프 인스턴스를 만든다. `first` 면 `FILE_FLAG_FIRST_PIPE_INSTANCE` 로 같은
/// 이름의 선점 인스턴스를 방지한다(중복 bind 조기 실패).
pub(super) fn create_pipe_instance(name: &str, first: bool) -> io::Result<PipeServerStream> {
    let wide = to_wide(name);
    let mut open_mode = PIPE_ACCESS_DUPLEX;
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
            std::ptr::null(),
        )
    };
    if raw == INVALID_HANDLE_VALUE || raw.is_null() {
        return Err(io::Error::last_os_error());
    }
    Ok(PipeServerStream::from_owned(OwnedHandle(raw)))
}

/// 파이프 인스턴스에서 클라이언트 연결을 blocking 으로 기다린다.
///
/// `ConnectNamedPipe` 가 실패(0) 하면서 `ERROR_PIPE_CONNECTED` 면 accept 직전에 이미
/// 클라이언트가 붙은 것 — 성공으로 취급한다.
pub(super) fn accept(stream: &PipeServerStream) -> io::Result<()> {
    // SAFETY: 유효 파이프 인스턴스 핸들. overlapped 없이 동기 대기.
    let rc = unsafe { ConnectNamedPipe(stream.handle.as_raw(), std::ptr::null_mut()) };
    if rc != 0 {
        return Ok(());
    }
    // SAFETY: 부수효과 없는 last-error 조회.
    let err = unsafe { GetLastError() };
    if err == ERROR_PIPE_CONNECTED {
        return Ok(());
    }
    Err(io::Error::from_raw_os_error(err as i32))
}

/// 한 클라이언트 처리 후 인스턴스를 재사용 가능 상태로 되돌린다(현재는 인스턴스를
/// 매번 새로 만들므로 사용하지 않지만, 향후 인스턴스 풀 도입 시 재연결 경로).
#[allow(dead_code)]
pub(super) fn disconnect(stream: &PipeServerStream) {
    // SAFETY: 유효 핸들. 실패는 무시 가능(이미 끊긴 경우 등).
    unsafe {
        DisconnectNamedPipe(stream.handle.as_raw());
    }
}

/// 프로세스 유일 파이프 이름. Unix 의 socket path 대응(pid + 단조 seq).
pub(super) fn unique_pipe_name(seq: u64) -> String {
    format!(r"\\.\pipe\tasty-handle-{}-{seq}", std::process::id())
}
