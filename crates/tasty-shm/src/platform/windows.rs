//! Windows 구현: CreateFileMappingW + DuplicateHandle + MapViewOfFile.
//!
//! 핸들 전달은 송신자가 peer 프로세스를 `OpenProcess(PROCESS_DUP_HANDLE)`로 열고
//! `DuplicateHandle`로 peer의 핸들 테이블에 복제한다. 결과 HANDLE(u64)을 호출자가
//! Named Pipe로 정수로 전송하면 됨.

use std::io;
use std::ptr;

use windows_sys::Win32::Foundation::{
    CloseHandle, DUPLICATE_SAME_ACCESS, DuplicateHandle, GetHandleInformation, HANDLE,
    INVALID_HANDLE_VALUE,
};
use windows_sys::Win32::System::Memory::{
    CreateFileMappingW, FILE_MAP_ALL_ACCESS, MEMORY_MAPPED_VIEW_ADDRESS, MapViewOfFile,
    PAGE_READWRITE, UnmapViewOfFile,
};
use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcess, PROCESS_DUP_HANDLE};

use crate::{PeerPid, ReceivedPayload, SendableHandle, SharedMemory, ShmError};

use super::MAX_SIZE;

/// OwnedHandle 대용 RAII wrapper. windows-sys는 OwnedHandle을 제공하지 않으므로 자체 정의.
struct OwnedHandle(HANDLE);

impl OwnedHandle {
    fn as_raw(&self) -> HANDLE {
        self.0
    }
}

impl Drop for OwnedHandle {
    fn drop(&mut self) {
        if !self.0.is_null() && self.0 != INVALID_HANDLE_VALUE {
            // SAFETY: CloseHandle은 유효한 HANDLE에 대해 호출되어야 한다. 우리는
            // null/INVALID이 아닐 때만 호출하고 Drop은 한 번만 실행되므로 double-close
            // 위험 없음.
            unsafe {
                CloseHandle(self.0);
            }
        }
    }
}

/// `SharedMemory`의 platform 표현. mmap view + 파일 매핑 핸들.
pub(crate) struct PlatformMapping {
    view: *mut u8,
    /// RAII 보유 전용 — 읽지 않지만 Drop 이 CloseHandle 해야 매핑 수명이 유지된다.
    _handle: OwnedHandle,
}

impl Drop for PlatformMapping {
    fn drop(&mut self) {
        if !self.view.is_null() {
            // SAFETY: view는 MapViewOfFile이 반환한 유효 포인터. Drop은 한 번만 호출.
            unsafe {
                UnmapViewOfFile(MEMORY_MAPPED_VIEW_ADDRESS {
                    Value: self.view as _,
                });
            }
        }
        // handle은 OwnedHandle이 자동 close.
    }
}

pub(crate) struct PlatformSendable {
    handle: OwnedHandle,
}

pub(crate) struct PlatformPayload {
    /// peer 프로세스의 핸들 테이블에 복제된 HANDLE의 u64 표현.
    duplicated: u64,
}

impl PlatformPayload {
    pub(crate) fn serialized_handle(&self) -> u64 {
        self.duplicated
    }
}

pub(crate) fn create(size: usize) -> Result<(SharedMemory, SendableHandle), ShmError> {
    if size > MAX_SIZE {
        return Err(ShmError::TooLarge(size));
    }

    let size_u64 = size as u64;
    let size_high = (size_u64 >> 32) as u32;
    let size_low = (size_u64 & 0xFFFF_FFFF) as u32;

    // SAFETY: Win32 CreateFileMappingW. INVALID_HANDLE_VALUE를 hFile에 넘기면 system
    // paging file 기반 익명 매핑. name=null이면 이름 없는 매핑.
    let raw = unsafe {
        CreateFileMappingW(
            INVALID_HANDLE_VALUE,
            ptr::null_mut(),
            PAGE_READWRITE,
            size_high,
            size_low,
            ptr::null(),
        )
    };
    if raw.is_null() {
        return Err(ShmError::Os(io::Error::last_os_error()));
    }
    let mapping_handle = OwnedHandle(raw);

    // 송신용 두 번째 핸들을 같은 객체에 대해 만든다 (DuplicateHandle을 GetCurrentProcess →
    // GetCurrentProcess로 호출).
    let sendable_raw = duplicate_to_self(mapping_handle.as_raw())?;
    let sendable_handle = OwnedHandle(sendable_raw);

    // 현재 프로세스에 view 매핑.
    // SAFETY: MapViewOfFile. handle은 방금 만든 유효 매핑.
    let view = unsafe { MapViewOfFile(mapping_handle.as_raw(), FILE_MAP_ALL_ACCESS, 0, 0, size) };
    if view.Value.is_null() {
        return Err(ShmError::Os(io::Error::last_os_error()));
    }

    let mapping = SharedMemory {
        ptr: view.Value as *mut u8,
        len: size,
        _handle: PlatformMapping {
            view: view.Value as *mut u8,
            _handle: mapping_handle,
        },
    };
    let handle = SendableHandle {
        inner: PlatformSendable {
            handle: sendable_handle,
        },
        size,
    };
    Ok((mapping, handle))
}

pub(crate) fn prepare_send(
    sendable: PlatformSendable,
    _size: usize,
    peer: PeerPid,
) -> Result<PlatformPayload, ShmError> {
    let peer_pid = match peer {
        PeerPid::Same => std::process::id(),
        PeerPid::Other(pid) => pid,
    };

    // peer 프로세스를 PROCESS_DUP_HANDLE 권한으로 연다.
    // SAFETY: Win32 OpenProcess. peer_pid는 사용자가 넘긴 값. 실패 시 null 반환.
    let peer_handle = unsafe { OpenProcess(PROCESS_DUP_HANDLE, 0, peer_pid) };
    if peer_handle.is_null() {
        return Err(ShmError::PeerUnreachable(peer_pid));
    }
    let peer_owned = OwnedHandle(peer_handle);

    let mut duplicated: HANDLE = ptr::null_mut();
    // SAFETY: Win32 DuplicateHandle. 모든 핸들이 유효함이 위에서 보장.
    let rc = unsafe {
        DuplicateHandle(
            GetCurrentProcess(),
            sendable.handle.as_raw(),
            peer_owned.as_raw(),
            &mut duplicated,
            0,
            0,
            DUPLICATE_SAME_ACCESS,
        )
    };
    if rc == 0 {
        return Err(ShmError::Os(io::Error::last_os_error()));
    }

    // peer_owned는 자동 close. sendable.handle도 자동 close (우리 측 사본).
    // duplicated는 peer가 close해야 함 — 호출자가 receive 시 OwnedHandle로 감싼다.

    Ok(PlatformPayload {
        duplicated: duplicated as u64,
    })
}

/// # Safety
/// 상위 `tasty_shm::receive`의 계약과 동일 — `handle`은 `DuplicateHandle`로 이
/// 프로세스의 핸들 테이블에 방금 복제되어, 아직 소유되지 않은 값이어야 한다.
///
/// 아래 `GetHandleInformation` 검증은 "이 정수가 현재 프로세스 핸들 테이블에
/// 유효하게 존재하는가"만 확인한다 — Unix 쪽(`fcntl`+`fstat`으로 open 여부와
/// 객체 **타입**을 모두 검증)보다 약한 방어다. Win32엔 표준 문서화 API로 "이
/// HANDLE이 file-mapping 객체인가"를 물을 방법이 마땅치 않아(비공식
/// `NtQueryObject` 정도), 이미 유효하지만 다른 용도인 핸들(로그 파일 HANDLE 등)이
/// 실수로 넘어오는 경우까지는 걸러내지 못한다 — `MapViewOfFile` 자체가 타입
/// 불일치 시 실패하는 것에 의존한다.
pub(crate) unsafe fn receive(payload: ReceivedPayload) -> Result<SharedMemory, ShmError> {
    let ReceivedPayload::Handle { handle, size } = payload;
    let raw = handle as HANDLE;
    if size == 0 {
        // SAFETY: handle은 계약상 이미 우리 소유 — 조기 반환 전에 명시적으로 닫는다.
        close_received_handle(raw);
        return Err(ShmError::ZeroSize);
    }
    if size > MAX_SIZE {
        // SAFETY: 위와 동일 이유.
        close_received_handle(raw);
        return Err(ShmError::TooLarge(size));
    }

    if raw.is_null() {
        return Err(ShmError::Os(io::Error::new(
            io::ErrorKind::InvalidInput,
            "received null handle",
        )));
    }

    // 방어 코드: handle이 현재 프로세스 핸들 테이블에 유효하게 존재하는지 형태 검증
    // (Unix의 fcntl(F_GETFD)에 대응). 무효/이미 닫힌 값을 소유권 편입 전에 걸러내
    // UB 대신 Err로 실패시킨다. 위 doc 참조 — 객체 타입까지는 검증하지 못한다.
    let mut flags: u32 = 0;
    // SAFETY: GetHandleInformation은 조회 전용, handle 소유권에 영향 없음.
    if unsafe { GetHandleInformation(raw, &mut flags) } == 0 {
        let err = io::Error::last_os_error();
        close_received_handle(raw);
        return Err(ShmError::Os(err));
    }

    let owned = OwnedHandle(raw);

    // SAFETY: MapViewOfFile. handle은 위에서 유효성을 확인했고, 호출자가 상위
    // `receive`의 `# Safety` 계약을 지켰다는 전제.
    let view = unsafe { MapViewOfFile(owned.as_raw(), FILE_MAP_ALL_ACCESS, 0, 0, size) };
    if view.Value.is_null() {
        return Err(ShmError::Os(io::Error::last_os_error()));
    }

    Ok(SharedMemory {
        ptr: view.Value as *mut u8,
        len: size,
        _handle: PlatformMapping {
            view: view.Value as *mut u8,
            _handle: owned,
        },
    })
}

/// 검증 실패 경로에서 계약상 이미 우리 소유인 handle을 명시적으로 닫는다(leak
/// 방지). null/INVALID는 닫을 대상이 아니므로 건너뛴다.
fn close_received_handle(raw: HANDLE) {
    if !raw.is_null() && raw != INVALID_HANDLE_VALUE {
        // SAFETY: null/INVALID가 아님을 위에서 확인. 이 함수는 검증 실패 후 1회만
        // 호출되므로 double-close 위험 없음.
        unsafe {
            CloseHandle(raw);
        }
    }
}

fn duplicate_to_self(source: HANDLE) -> Result<HANDLE, ShmError> {
    let mut out: HANDLE = ptr::null_mut();
    // SAFETY: Win32 DuplicateHandle with same source/target process. GetCurrentProcess는
    // 항상 유효 pseudo-handle 반환.
    let rc = unsafe {
        DuplicateHandle(
            GetCurrentProcess(),
            source,
            GetCurrentProcess(),
            &mut out,
            0,
            0,
            DUPLICATE_SAME_ACCESS,
        )
    };
    if rc == 0 {
        return Err(ShmError::Os(io::Error::last_os_error()));
    }
    Ok(out)
}

impl PlatformPayload {
    /// 핸들 값을 회수. 일반적으로는 호출 불요 (Drop이 없음 — peer 소유라서).
    /// unix 판 `into_raw_fd` 와의 대칭 API 표면 유지 — 현재 windows 송신 경로는
    /// `serialized_handle` 만 쓴다.
    #[allow(dead_code)]
    pub(crate) fn into_raw(self) -> u64 {
        let h = self.duplicated;
        std::mem::forget(self);
        h
    }
}

impl Drop for PlatformPayload {
    fn drop(&mut self) {
        // PlatformPayload는 peer 프로세스의 핸들 테이블에 있는 HANDLE 값만 보관한다.
        // 우리 프로세스에는 그 핸들이 등록돼 있지 않으므로 CloseHandle을 호출하면 안 된다
        // (잘못된 핸들로 인한 손상 위험). 그러므로 Drop은 no-op.
    }
}

// extern HANDLE은 Send/Sync 자동 구현되지 않으므로 명시.
// SAFETY: HANDLE은 OS-managed 정수. 다른 스레드로 이동하거나 공유해도 OS가 안전하게 처리.
unsafe impl Send for OwnedHandle {}
// SAFETY: 위와 동일. CloseHandle은 thread-safe.
unsafe impl Sync for OwnedHandle {}
// SAFETY: PlatformPayload는 u64만 보관하며 OS 핸들 lifecycle을 직접 관리하지 않음.
unsafe impl Send for PlatformPayload {}
