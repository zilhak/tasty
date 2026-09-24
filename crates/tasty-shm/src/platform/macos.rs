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
    // 이유: 매핑을 해제할 때까지 fd를 소유하고 Drop에서 닫는다.
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

    /// 빌린 raw_fd와 달리 fd 소유권을 호출자에게 넘긴다.
    // 이유: Linux와 같은 핸들 소유권 회수 API를 유지한다.
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

    // 이름 충돌은 O_EXCL로 거절한다.
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

    // fd의 열림 상태와 파일 타입만 검사한다. 다른 용도의 유효한 fd는 구분하지
    // 못하므로 호출자가 receive의 소유권 조건을 보장해야 한다.
    // SAFETY: fcntl(F_GETFD)는 fd 값 자체는 아직 소유하지 않은 채 조회만 한다.
    if unsafe { libc::fcntl(fd, libc::F_GETFD) } < 0 {
        let err = io::Error::last_os_error();
        // SAFETY: 검증 실패 fd도 계약상 이미 우리 소유 — 닫아야 leak 되지 않는다.
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
    // macOS shm_open 객체는 fstat의 타입 비트가 0일 수 있으므로 0과 S_IFREG를
    // 허용한다. 소켓·파이프·tty·디렉터리 등 다른 타입은 거절한다.
    let ifmt = st.st_mode & libc::S_IFMT;
    if ifmt != 0 && ifmt != libc::S_IFREG {
        // SAFETY: 형태 불일치로 거부하는 fd도 계약상 이미 우리 소유 — leak 방지.
        unsafe { libc::close(fd) };
        return Err(ShmError::Os(io::Error::new(
            io::ErrorKind::InvalidInput,
            "received fd is not a shared-memory object (shm_open backing expected)",
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
