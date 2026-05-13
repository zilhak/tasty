//! Linux 구현: memfd_create + mmap.
//!
//! 핸들 전달은 호출자가 Unix socket의 SCM_RIGHTS cmsg에 fd를 끼우는 방식. 이 모듈은
//! fd를 노출하기만 한다.

use std::ffi::CString;
use std::io;
use std::os::fd::{AsRawFd, FromRawFd, IntoRawFd, OwnedFd, RawFd};
use std::ptr;

use crate::{PeerPid, ReceivedPayload, SendableHandle, SharedMemory, ShmError};

use super::MAX_SIZE;

/// `SharedMemory`의 platform-specific cleanup. Drop 시 munmap + close.
pub(crate) struct PlatformMapping {
    ptr: *mut u8,
    len: usize,
    fd: OwnedFd,
}

impl Drop for PlatformMapping {
    fn drop(&mut self) {
        if !self.ptr.is_null() {
            // SAFETY: ptr/len은 mmap이 반환한 유효한 영역. Drop은 한 번만 호출되므로
            // double-munmap 위험 없음.
            unsafe {
                libc::munmap(self.ptr as *mut libc::c_void, self.len);
            }
        }
        // fd는 OwnedFd가 자동으로 close.
        let _ = &self.fd;
    }
}

/// `SendableHandle`의 platform 표현. fd 그 자체.
pub(crate) struct PlatformSendable {
    fd: OwnedFd,
}

/// `TransportPayload`의 platform 표현. fd 소유권을 유지 (호출자가 sendmsg할 동안 살아있어야).
pub(crate) struct PlatformPayload {
    fd: OwnedFd,
}

impl PlatformPayload {
    pub(crate) fn raw_fd(&self) -> RawFd {
        self.fd.as_raw_fd()
    }
}

pub(crate) fn create(size: usize) -> Result<(SharedMemory, SendableHandle), ShmError> {
    if size > MAX_SIZE {
        return Err(ShmError::TooLarge(size));
    }

    let name = CString::new("tasty-shm").expect("static name has no nul");

    // SAFETY: memfd_create syscall. name pointer는 위에서 만든 CString이 유효.
    // 반환값은 새 fd 또는 -1 (errno 설정).
    let raw_fd = unsafe {
        libc::syscall(
            libc::SYS_memfd_create,
            name.as_ptr(),
            libc::MFD_CLOEXEC,
        )
    };
    if raw_fd < 0 {
        return Err(ShmError::Os(io::Error::last_os_error()));
    }
    // SAFETY: memfd_create가 반환한 raw_fd는 유효한 새 fd. OwnedFd가 소유권을 가져간다.
    let fd_for_map = unsafe { OwnedFd::from_raw_fd(raw_fd as RawFd) };

    // 크기 설정.
    // SAFETY: ftruncate는 fd에 길이를 설정. 우리가 막 만든 fd는 유효.
    let rc = unsafe { libc::ftruncate(fd_for_map.as_raw_fd(), size as libc::off_t) };
    if rc < 0 {
        return Err(ShmError::Os(io::Error::last_os_error()));
    }

    // 현재 프로세스에 매핑.
    let ptr = mmap_shared(fd_for_map.as_raw_fd(), size)?;

    // 같은 fd를 두 개의 OwnedFd로 쪼개려면 dup이 필요하다 — 하나는 매핑 유지용
    // (Drop 시 close), 하나는 송신용. dup으로 별도 fd를 만들어 송신 측에 보관한다.
    // SAFETY: dup syscall. fd가 유효함은 위에서 보장.
    let dup_fd = unsafe { libc::dup(fd_for_map.as_raw_fd()) };
    if dup_fd < 0 {
        // 매핑은 정리되어야 함. ptr/len을 들고 cleanup.
        // SAFETY: 방금 mmap한 유효 영역.
        unsafe { libc::munmap(ptr as *mut libc::c_void, size) };
        return Err(ShmError::Os(io::Error::last_os_error()));
    }
    // SAFETY: dup이 성공해서 새 유효 fd 반환.
    let sendable_fd = unsafe { OwnedFd::from_raw_fd(dup_fd) };

    // CLOEXEC을 dup된 fd에도 설정 (dup은 CLOEXEC을 복사하지 않음).
    set_cloexec(sendable_fd.as_raw_fd())?;

    let mapping = SharedMemory {
        ptr,
        len: size,
        _handle: PlatformMapping {
            ptr,
            len: size,
            fd: fd_for_map,
        },
    };
    let handle = SendableHandle {
        inner: PlatformSendable { fd: sendable_fd },
        size,
    };
    Ok((mapping, handle))
}

pub(crate) fn prepare_send(
    sendable: PlatformSendable,
    size: usize,
    _peer: PeerPid,
) -> Result<PlatformPayload, ShmError> {
    // Unix에선 peer PID가 필요 없다 (SCM_RIGHTS가 커널 매개).
    let _ = size;
    Ok(PlatformPayload { fd: sendable.fd })
}

pub(crate) fn receive(payload: ReceivedPayload) -> Result<SharedMemory, ShmError> {
    let ReceivedPayload::Fd { fd, size } = payload;
    if size == 0 {
        return Err(ShmError::ZeroSize);
    }
    if size > MAX_SIZE {
        return Err(ShmError::TooLarge(size));
    }

    // SAFETY: 호출자가 socketmsg cmsg에서 받은 유효한 fd라고 약속. 소유권이 우리에게 이전.
    let owned = unsafe { OwnedFd::from_raw_fd(fd) };
    let ptr = mmap_shared(owned.as_raw_fd(), size)?;
    Ok(SharedMemory {
        ptr,
        len: size,
        _handle: PlatformMapping {
            ptr,
            len: size,
            fd: owned,
        },
    })
}

fn mmap_shared(fd: RawFd, size: usize) -> Result<*mut u8, ShmError> {
    // SAFETY: mmap syscall. fd가 유효한 file descriptor임을 호출자가 보장 (위
    // create/receive 경로에서 모두 직전에 검증).
    let ptr = unsafe {
        libc::mmap(
            ptr::null_mut(),
            size,
            libc::PROT_READ | libc::PROT_WRITE,
            libc::MAP_SHARED,
            fd,
            0,
        )
    };
    if ptr == libc::MAP_FAILED {
        return Err(ShmError::Os(io::Error::last_os_error()));
    }
    Ok(ptr as *mut u8)
}

fn set_cloexec(fd: RawFd) -> Result<(), ShmError> {
    // SAFETY: fcntl syscall. fd가 유효함은 호출자가 보장.
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFD) };
    if flags < 0 {
        return Err(ShmError::Os(io::Error::last_os_error()));
    }
    // SAFETY: fcntl syscall.
    let rc = unsafe { libc::fcntl(fd, libc::F_SETFD, flags | libc::FD_CLOEXEC) };
    if rc < 0 {
        return Err(ShmError::Os(io::Error::last_os_error()));
    }
    Ok(())
}

impl PlatformPayload {
    /// 호출자가 sendmsg 후 fd 소유권을 명시적으로 회수하고 싶을 때.
    /// 일반적으로는 Drop으로 자동 close되므로 사용 불요.
    #[allow(dead_code)]
    pub(crate) fn into_raw_fd(self) -> RawFd {
        self.fd.into_raw_fd()
    }
}
