//! macOS 구현: POSIX shm_open + 즉시 shm_unlink로 unnamed 효과.
//!
//! 핸들 전달은 Linux와 동일하게 SCM_RIGHTS 경로 (BSD 호환). 이름 충돌은 PID +
//! 단조 카운터 + nanosec timestamp로 unique.

use std::ffi::CString;
use std::io;
use std::os::fd::{AsRawFd, FromRawFd, IntoRawFd, OwnedFd, RawFd};
use std::ptr;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::{PeerPid, ReceivedPayload, SendableHandle, SharedMemory, ShmError};

use super::MAX_SIZE;

pub(crate) struct PlatformMapping {
    ptr: *mut u8,
    len: usize,
    /// fd는 mmap 영역의 backing — munmap 이후 자동 close. 명시 사용처는 없다.
    // 이유: RAII 가드 — 읽히지 않지만 munmap 까지 fd 를 살려둬야 함(삭제 시 즉시 close 버그).
    #[allow(dead_code)]
    fd: OwnedFd,
}

impl Drop for PlatformMapping {
    fn drop(&mut self) {
        if !self.ptr.is_null() {
            // SAFETY: ptr/len은 mmap이 반환한 유효한 영역. Drop은 한 번만 호출.
            unsafe {
                libc::munmap(self.ptr as *mut libc::c_void, self.len);
            }
        }
    }
}

pub(crate) struct PlatformSendable {
    fd: OwnedFd,
}

pub(crate) struct PlatformPayload {
    fd: OwnedFd,
}

impl PlatformPayload {
    pub(crate) fn raw_fd(&self) -> RawFd {
        self.fd.as_raw_fd()
    }

    /// fd 소유권을 호출자로 명시 이양 — `raw_fd`(빌림)와 짝. `round_trip` 통합
    /// 테스트가 이 경로(호출자 fd 소유권 회수)를 검증한다.
    // 이유: linux.rs 의 동명 메서드와 플랫폼 대칭 API (한쪽만 삭제 시 분기). 판단필요 — conductor 검토.
    #[allow(dead_code)]
    pub(crate) fn into_raw_fd(self) -> RawFd {
        self.fd.into_raw_fd()
    }
}

fn unique_shm_name() -> CString {
    let pid = std::process::id();
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let mono = crate::next_unique_id();
    // POSIX shm 이름은 leading `/`로 시작해야 하고, macOS는 최대 31자(NAME_MAX).
    // pid(10) + nanos 하위(10) + mono(6) ≈ 27자 + 헤더 4자.
    let name = format!(
        "/tsty{:x}{:x}{:x}",
        pid,
        (nanos as u64) & 0xFFFF_FFFF,
        mono & 0xFFFF
    );
    let truncated = if name.len() > 30 {
        name[..30].to_string()
    } else {
        name
    };
    CString::new(truncated).expect("name has no nul")
}

pub(crate) fn create(size: usize) -> Result<(SharedMemory, SendableHandle), ShmError> {
    if size > MAX_SIZE {
        return Err(ShmError::TooLarge(size));
    }

    let name = unique_shm_name();

    // O_EXCL로 이름 충돌 시 실패하게 — 충돌은 카운터 race로 매우 드물지만 명시적.
    // SAFETY: shm_open syscall. name pointer가 위에서 만든 유효 CString.
    let raw_fd = unsafe {
        libc::shm_open(
            name.as_ptr(),
            libc::O_RDWR | libc::O_CREAT | libc::O_EXCL,
            0o600,
        )
    };
    if raw_fd < 0 {
        return Err(ShmError::Os(io::Error::last_os_error()));
    }
    // SAFETY: shm_open이 반환한 유효 fd.
    let fd_for_map = unsafe { OwnedFd::from_raw_fd(raw_fd) };

    // 즉시 이름 회수 — 이미 열린 fd는 유효함을 유지하고 다른 프로세스가 이름으로
    // 새로 열 수 없게 만든다 (unnamed 효과).
    // SAFETY: shm_unlink syscall. name이 유효.
    let rc = unsafe { libc::shm_unlink(name.as_ptr()) };
    if rc < 0 {
        return Err(ShmError::Os(io::Error::last_os_error()));
    }

    // 크기 설정.
    // SAFETY: ftruncate. fd 유효.
    let rc = unsafe { libc::ftruncate(fd_for_map.as_raw_fd(), size as libc::off_t) };
    if rc < 0 {
        return Err(ShmError::Os(io::Error::last_os_error()));
    }

    let ptr = mmap_shared(fd_for_map.as_raw_fd(), size)?;

    // dup으로 송신용 fd 분리.
    // SAFETY: dup syscall. fd 유효.
    let dup_fd = unsafe { libc::dup(fd_for_map.as_raw_fd()) };
    if dup_fd < 0 {
        // SAFETY: 방금 mmap한 유효 영역 정리.
        unsafe { libc::munmap(ptr as *mut libc::c_void, size) };
        return Err(ShmError::Os(io::Error::last_os_error()));
    }
    // SAFETY: dup이 성공해서 새 유효 fd.
    let sendable_fd = unsafe { OwnedFd::from_raw_fd(dup_fd) };
    set_cloexec(sendable_fd.as_raw_fd())?;
    set_cloexec(fd_for_map.as_raw_fd())?;

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
    _size: usize,
    _peer: PeerPid,
) -> Result<PlatformPayload, ShmError> {
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

    // SAFETY: 호출자가 SCM_RIGHTS에서 받은 유효 fd라고 약속. 소유권 이전.
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
    // SAFETY: mmap syscall. fd가 유효함을 호출자가 보장.
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
    // SAFETY: fcntl syscall. fd 유효함은 호출자 보장.
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
