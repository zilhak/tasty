//! OS 공유 메모리와 핸들 전달에 필요한 기능.
//!
//! `create`로 영역과 송신 핸들을 만들고 `prepare_send`로 전달할 값을 얻는다.
//! 호출자가 IPC로 값을 전달하면 수신 측은 `receive`로 같은 영역을 매핑한다.
//! 실제 전송과 동기화는 호출자가 맡는다.
//!
//! # 안전성
//!
//! 반환된 슬라이스를 사용하는 동안 다른 스레드·프로세스의 접근도 조율해야 한다.
//! generation이나 dirty 알림만으로 동시 접근이 안전해지는 것은 아니다.
//! 공유 내용은 픽셀·오디오·바이트 데이터로만 사용하며 코드로 실행하지 않는다.
#![deny(missing_docs)]

mod error;
mod platform;

pub mod footer;

#[cfg(target_os = "macos")]
use std::sync::atomic::{AtomicU64, Ordering};

pub use error::ShmError;

/// 수신 측 프로세스. Windows에서 핸들을 복제할 대상을 지정한다. Unix는 무시한다.
#[derive(Debug, Clone, Copy)]
pub enum PeerPid {
    /// 자기 자신 프로세스(테스트용).
    Same,
    /// 다른 프로세스의 OS PID.
    Other(u32),
}

/// 현재 프로세스에 매핑한 공유 메모리. Drop에서 매핑과 소유 핸들을 해제한다.
pub struct SharedMemory {
    ptr: *mut u8,
    len: usize,
    /// Drop에서 매핑·핸들을 해제하는 플랫폼별 상태.
    _handle: platform::PlatformMapping,
}

// SAFETY: 매핑의 이동은 메모리 접근을 만들지 않는다. 슬라이스 접근은 unsafe이며
// 호출자가 스레드·프로세스 사이의 동기화를 보장한다.
unsafe impl Send for SharedMemory {}
// SAFETY: 매핑 메타데이터를 공유할 수 있다. 실제 메모리 접근의 동기화는
// unsafe 슬라이스 메서드의 호출자가 보장한다.
unsafe impl Sync for SharedMemory {}

impl SharedMemory {
    /// 매핑된 영역의 바이트 길이.
    pub fn len(&self) -> usize {
        self.len
    }

    /// 길이가 0인지 (실제로는 0-byte 영역을 만들 수 없으므로 항상 false).
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// 매핑된 영역을 raw byte slice로 본다.
    ///
    /// # Safety
    ///
    /// 반환된 참조가 유효한 동안 매핑이 유지되어야 하며, 다른 스레드·프로세스나
    /// 별도 매핑에서 같은 영역에 쓰면 안 된다. 잘못된 값을 허용하는 것으로
    /// 동시 접근에 필요한 조건을 대신할 수 없다.
    pub unsafe fn as_slice(&self) -> &[u8] {
        // SAFETY: create/receive가 만든 유효 영역이며 &self 동안 매핑을 유지한다.
        // 다른 접근과의 동기화는 호출자가 위 조건에 따라 보장한다.
        unsafe { std::slice::from_raw_parts(self.ptr, self.len) }
    }

    /// 매핑된 영역을 raw mutable byte slice로 본다.
    ///
    /// # Safety
    ///
    /// 반환된 참조가 유효한 동안 매핑을 유지하고 해당 영역에 독점적으로 접근해야 한다.
    /// 같은 프로세스의 다른 참조·매핑과 다른 프로세스 모두 동시에 읽거나 쓰면 안 된다.
    #[allow(clippy::mut_from_ref)]
    pub unsafe fn as_mut_slice(&self) -> &mut [u8] {
        // SAFETY: 유효한 매핑이며 호출자가 반환된 참조의 수명 동안 독점 접근을 보장한다.
        unsafe { std::slice::from_raw_parts_mut(self.ptr, self.len) }
    }
}

/// 다른 프로세스로 보낼 수 있는 공유 메모리 핸들.
///
/// `prepare_send`를 호출하면 transport-ready 페이로드로 변환되고 본 핸들은 소비된다.
pub struct SendableHandle {
    inner: platform::PlatformSendable,
    size: usize,
}

impl SendableHandle {
    /// 영역 크기 (peer가 받을 mmap에 사용해야 하는 값).
    pub fn size(&self) -> usize {
        self.size
    }
}

/// `prepare_send` 결과. OS-native transport에 실어 보낼 수 있는 형태.
///
/// 이 crate는 transport를 다루지 않으므로, 호출자가 페이로드를 자기 IPC 채널에
/// 적절히 직렬화/전송해야 한다.
///
/// - Unix: `raw_fd()`로 fd를 얻어 sendmsg의 SCM_RIGHTS cmsg에 끼운다.
/// - Windows: `serialized_handle()`로 u64를 얻어 Named Pipe write로 보낸다.
pub struct TransportPayload {
    inner: platform::PlatformPayload,
    size: usize,
}

impl TransportPayload {
    /// 영역 크기.
    pub fn size(&self) -> usize {
        self.size
    }

    /// Unix: 송신할 raw file descriptor. sendmsg cmsg에 끼워 보낸다.
    /// payload는 *소유권을 유지*하므로 fd를 sendmsg에 끼우는 동안 살아있어야 한다.
    #[cfg(unix)]
    pub fn raw_fd(&self) -> std::os::fd::RawFd {
        self.inner.raw_fd()
    }

    /// Windows: 송신할 직렬화된 HANDLE 값(u64). Named Pipe write로 그대로 보낸다.
    #[cfg(windows)]
    pub fn serialized_handle(&self) -> u64 {
        self.inner.serialized_handle()
    }
}

/// 받은 transport 페이로드를 재구성하기 위한 양식.
///
/// 호출자가 자기 transport에서 fd 또는 u64를 꺼내 이 enum으로 wrapping해 넘긴다.
#[cfg(unix)]
pub enum ReceivedPayload {
    /// SCM_RIGHTS cmsg에서 받은 fd + 영역 크기.
    Fd {
        /// 받은 raw file descriptor. `receive`가 호출되면 소유권이 이전된다.
        fd: std::os::fd::RawFd,
        /// 영역 크기.
        size: usize,
    },
}

/// 받은 transport 페이로드를 재구성하기 위한 양식 (Windows).
#[cfg(windows)]
pub enum ReceivedPayload {
    /// peer가 DuplicateHandle로 자신의 핸들 테이블에 복제해준 HANDLE의 u64 표현.
    Handle {
        /// 받은 HANDLE u64. `receive`가 호출되면 소유권이 이전된다.
        handle: u64,
        /// 영역 크기.
        size: usize,
    },
}

/// 단일 프로세스 내 단조 카운터 (macOS의 shm_open 이름 충돌 방지용 — macOS 전용).
#[cfg(target_os = "macos")]
static MONO_COUNTER: AtomicU64 = AtomicU64::new(0);

#[cfg(target_os = "macos")]
pub(crate) fn next_unique_id() -> u64 {
    MONO_COUNTER.fetch_add(1, Ordering::Relaxed)
}

/// 새 공유 메모리 영역을 만든다.
///
/// 반환된 `SharedMemory`는 현재 프로세스에 즉시 매핑되어 있고, `SendableHandle`은
/// 다른 프로세스에 핸들을 넘기기 위한 자료. `size`는 0보다 커야 한다.
///
/// 한 OS 영역에 대해 송신자(=호출자)와 수신자가 각각 별도 `SharedMemory` 객체를 갖게
/// 된다. Drop은 독립적으로 일어나며, 마지막 매핑이 해제될 때 OS 영역이 회수된다.
pub fn create(size: usize) -> Result<(SharedMemory, SendableHandle), ShmError> {
    if size == 0 {
        return Err(ShmError::ZeroSize);
    }
    platform::create(size)
}

/// `SendableHandle`을 transport-ready 페이로드로 변환한다.
///
/// 호출 후 본 핸들은 소비되어 재사용 불가. peer는 Windows에서만 의미를 가진다
/// (`DuplicateHandle` 대상 PID).
pub fn prepare_send(handle: SendableHandle, peer: PeerPid) -> Result<TransportPayload, ShmError> {
    platform::prepare_send(handle.inner, handle.size, peer).map(|inner| TransportPayload {
        inner,
        size: handle.size,
    })
}

/// 받은 transport 페이로드로부터 현재 프로세스에 영역을 매핑한다.
///
/// # Safety
///
/// `payload`가 담은 fd(unix) 또는 handle(windows)은 대응하는 `prepare_send`가 만든
/// 페이로드로부터 **SCM_RIGHTS recvmsg(unix) 또는 `DuplicateHandle`(windows)로 이
/// 프로세스에 방금 전달되어, 아직 어디에도 소유되지 않은 값**이어야 한다. 임의의
/// 정수, 이미 다른 목적으로 열려 있는 fd/handle, 또는 이미 close/receive된 값을
/// 넘기면 소유권 이중화(double-close, use-after-close, fd/handle aliasing) UB가
/// 발생한다. 호출자는 이 값의 출처를 직접 추적할 수 있는 코드에서만 호출해야 한다.
///
/// fd/handle 형태(열려 있는지, regular file/매핑 객체인지)는 내부적으로 검증해
/// 명백히 잘못된 값은 `Err`로 걸러내지만, "다른 목적으로 쓰이는 유효한 값"까지는
/// 구분하지 못한다 — 이 계약은 형태 검증으로 대체되지 않는다.
pub unsafe fn receive(payload: ReceivedPayload) -> Result<SharedMemory, ShmError> {
    // SAFETY: 호출자가 위 계약을 지켰다는 전제 하에 platform::receive에 위임.
    unsafe { platform::receive(payload) }
}

#[cfg(all(unix, test))]
mod tests {
    use super::*;

    #[test]
    fn receive_rejects_invalid_fd() {
        // 이미 닫혔거나 존재한 적 없는 fd 번호 — fcntl(F_GETFD)에서 EBADF로 걸려야 한다.
        // SAFETY: 이 테스트 자체가 "계약 위반(무효 fd)을 넘겼을 때 UB 대신 Err가
        // 나오는가"를 검증하는 목적 — fcntl/fstat 형태 검증이 mmap 전에 걸러내므로
        // 실제로는 UB가 발생하지 않는다.
        let payload = ReceivedPayload::Fd {
            fd: 99999,
            size: 4096,
        };
        // SAFETY: 위와 동일 — 계약 위반(무효 fd)을 형태 검증이 걸러낸다.
        let result = unsafe { receive(payload) };
        assert!(result.is_err());
    }

    #[test]
    fn receive_rejects_non_regular_file_fd() {
        // stdin(fd 0)은 열려는 있지만 memfd/shm backing과 다른 타입 — fstat의
        // S_IFREG 검증에 걸려야 한다.
        // SAFETY: 위와 동일 — 계약 위반(타입 불일치 fd)을 형태 검증이 걸러낸다.
        let payload = ReceivedPayload::Fd { fd: 0, size: 4096 };
        // SAFETY: 위와 동일 — 계약 위반(타입 불일치 fd)을 형태 검증이 걸러낸다.
        let result = unsafe { receive(payload) };
        assert!(result.is_err());
    }
}
