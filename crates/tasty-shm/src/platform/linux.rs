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
    // 이유: RAII 가드 — 읽히지 않지만 munmap 까지 fd 를 살려둬야 함(삭제 시 즉시 close 버그).
    #[allow(dead_code)]
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
    let raw_fd = unsafe { libc::syscall(libc::SYS_memfd_create, name.as_ptr(), libc::MFD_CLOEXEC) };
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
    _size: usize,
    _peer: PeerPid,
) -> Result<PlatformPayload, ShmError> {
    // Unix에선 peer PID가 필요 없다 (SCM_RIGHTS가 커널 매개).
    Ok(PlatformPayload { fd: sendable.fd })
}

/// # Safety
/// 상위 `tasty_shm::receive`의 계약과 동일 — `fd`는 방금 커널이 이 프로세스로
/// 전달한, 아직 소유되지 않은 fd여야 한다.
pub(crate) unsafe fn receive(payload: ReceivedPayload) -> Result<SharedMemory, ShmError> {
    let ReceivedPayload::Fd { fd, size } = payload;
    if size == 0 {
        // SAFETY: fd는 계약상 이미 우리 소유 — 조기 반환 전에 명시적으로 닫는다(leak 방지).
        unsafe { libc::close(fd) };
        return Err(ShmError::ZeroSize);
    }
    if size > MAX_SIZE {
        // SAFETY: 위와 동일 이유.
        unsafe { libc::close(fd) };
        return Err(ShmError::TooLarge(size));
    }

    // 방어 코드: fd가 현재 프로세스에서 열려 있고, memfd_create/shm_open이 만드는
    // backing과 같은 타입(regular file)인지 형태 검증. 무작위/닫힌/타입불일치 fd를
    // 소유권 편입 전에 걸러내 UB 대신 Err로 실패시킨다. 완전한 증명은 아니다 — 다른
    // 목적의 regular-file fd까지는 걸러내지 못한다(상위 `receive`의 `# Safety` 참조).
    // SAFETY: fcntl(F_GETFD)는 fd 값 자체는 아직 소유하지 않은 채 조회만 한다.
    if unsafe { libc::fcntl(fd, libc::F_GETFD) } < 0 {
        let err = io::Error::last_os_error();
        // SAFETY: 검증 실패 fd도 계약상 이미 우리 소유 — 닫아야 leak 되지 않는다.
        // fcntl 자체가 실패했다는 건 fd가 이미 무효(닫힘)했을 가능성이 높지만, close는
        // EBADF를 반환할 뿐 안전하므로 무조건 시도한다.
        unsafe { libc::close(fd) };
        return Err(ShmError::Os(err));
    }
    // SAFETY: libc::stat은 all-zero가 유효한 초기값(fstat이 모든 필드를 채운다).
    let mut st: libc::stat = unsafe { std::mem::zeroed() };
    // SAFETY: fstat도 조회 전용, fd 소유권에 영향 없음.
    if unsafe { libc::fstat(fd, &mut st) } < 0 {
        let err = io::Error::last_os_error();
        // SAFETY: 위와 동일 — 검증 실패 경로에서 leak 방지.
        unsafe { libc::close(fd) };
        return Err(ShmError::Os(err));
    }
    if st.st_mode & libc::S_IFMT != libc::S_IFREG {
        // SAFETY: 형태 불일치로 거부하는 fd도 계약상 이미 우리 소유 — leak 방지.
        unsafe { libc::close(fd) };
        return Err(ShmError::Os(io::Error::new(
            io::ErrorKind::InvalidInput,
            "received fd is not a regular file (memfd/shm backing expected)",
        )));
    }

    // SAFETY: 호출자가 상위 `receive`의 `# Safety` 계약을 지켰고, 위 형태 검증도 통과한 fd.
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
    // 이유: macos.rs 의 동명 메서드와 플랫폼 대칭 API (한쪽만 삭제 시 분기). 판단필요 — conductor 검토.
    #[allow(dead_code)]
    pub(crate) fn into_raw_fd(self) -> RawFd {
        self.fd.into_raw_fd()
    }
}
