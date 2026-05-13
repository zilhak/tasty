//! Round-trip 검증: 한쪽에서 쓴 바이트를 다른쪽에서 읽어 일치하는가.
//!
//! cross-process IPC 통합은 Step 02에서 검증한다. 여기서는 같은 프로세스 내에서
//! 핸들 전달 메커니즘만 흉내내 양쪽 매핑이 같은 메모리를 본다는 사실을 검증.

use tasty_shm::{PeerPid, ReceivedPayload};

#[cfg(unix)]
mod unix {
    use super::*;

    /// 호출자가 fd를 transport에 실어 보낸 것을 흉내내기 위해, 같은 프로세스 내에서
    /// raw fd 정수를 그대로 ReceivedPayload로 만들어 넘긴다. 실제 transport (sendmsg)는
    /// Step 02에서 SDK 통합 시 검증.
    fn send_to_self(payload: tasty_shm::TransportPayload) -> ReceivedPayload {
        // payload가 들고 있는 fd를 dup해서 별개 fd를 만들어 송신 흉내.
        // SAFETY: dup syscall. payload.raw_fd()가 유효 fd.
        let dup_fd = unsafe { libc::dup(payload.raw_fd()) };
        assert!(dup_fd >= 0, "dup failed: {}", std::io::Error::last_os_error());
        // payload는 여기서 drop되며 자기 fd를 close. dup_fd만 남는다.
        ReceivedPayload::Fd {
            fd: dup_fd,
            size: payload.size(),
        }
    }

    #[test]
    fn create_and_receive_share_same_memory() {
        let size = 4096;
        let (mem_a, handle) = tasty_shm::create(size).expect("create");
        let payload = tasty_shm::prepare_send(handle, PeerPid::Same).expect("prepare_send");
        let received = send_to_self(payload);
        let mem_b = tasty_shm::receive(received).expect("receive");

        assert_eq!(mem_a.len(), size);
        assert_eq!(mem_b.len(), size);

        // mem_a 측에서 쓴다.
        // SAFETY: 단일 스레드에서 다른 매핑이 동시 접근하지 않음.
        unsafe {
            let slice = mem_a.as_mut_slice();
            slice[0..4].copy_from_slice(b"test");
            slice[size - 1] = 0xAB;
        }

        // mem_b 측에서 읽어 일치 확인.
        // SAFETY: 위 쓰기가 끝난 뒤 단일 스레드에서 읽음.
        unsafe {
            let view = mem_b.as_slice();
            assert_eq!(&view[0..4], b"test");
            assert_eq!(view[size - 1], 0xAB);
        }
    }

    #[test]
    fn write_from_b_visible_in_a() {
        let (mem_a, handle) = tasty_shm::create(8).expect("create");
        let payload = tasty_shm::prepare_send(handle, PeerPid::Same).expect("prepare_send");
        let received = send_to_self(payload);
        let mem_b = tasty_shm::receive(received).expect("receive");

        // mem_b가 쓴 값이 mem_a에서 보이는가.
        // SAFETY: 단일 스레드.
        unsafe {
            mem_b.as_mut_slice()[0..3].copy_from_slice(b"hi!");
        }
        // SAFETY: 단일 스레드.
        unsafe {
            assert_eq!(&mem_a.as_slice()[0..3], b"hi!");
        }
    }

    #[test]
    fn page_boundary_sizes() {
        // 4096(정확 1페이지), 4097(경계 + 1), 16384(여러 페이지)
        for &size in &[4096usize, 4097, 16384] {
            let (mem_a, handle) = tasty_shm::create(size).expect("create");
            let payload =
                tasty_shm::prepare_send(handle, PeerPid::Same).expect("prepare_send");
            let received = send_to_self(payload);
            let mem_b = tasty_shm::receive(received).expect("receive");

            assert_eq!(mem_a.len(), size);
            assert_eq!(mem_b.len(), size);

            // SAFETY: 단일 스레드.
            unsafe {
                let s = mem_a.as_mut_slice();
                s[0] = 0xDE;
                s[size - 1] = 0xAD;
            }
            // SAFETY: 단일 스레드.
            unsafe {
                let v = mem_b.as_slice();
                assert_eq!(v[0], 0xDE);
                assert_eq!(v[size - 1], 0xAD);
            }
        }
    }

    #[test]
    fn unaligned_small_size() {
        // 1, 3, 17 — 모두 페이지 경계 미만. mmap이 페이지로 올림하지만 슬라이스 길이는
        // 요청한 size여야.
        for &size in &[1usize, 3, 17] {
            let (mem_a, handle) = tasty_shm::create(size).expect("create");
            assert_eq!(mem_a.len(), size);
            let payload =
                tasty_shm::prepare_send(handle, PeerPid::Same).expect("prepare_send");
            let received = send_to_self(payload);
            let mem_b = tasty_shm::receive(received).expect("receive");
            assert_eq!(mem_b.len(), size);
        }
    }

    #[test]
    fn drop_unmaps_without_leak() {
        // 매핑/언맵을 반복해도 fd가 새지 않음을 검증 (rlimit으로 잡힐 만큼 많이).
        for _ in 0..256 {
            let (mem, handle) = tasty_shm::create(64).expect("create");
            let payload =
                tasty_shm::prepare_send(handle, PeerPid::Same).expect("prepare_send");
            let received = send_to_self(payload);
            let mem2 = tasty_shm::receive(received).expect("receive");
            drop(mem);
            drop(mem2);
        }
    }

    #[test]
    fn into_raw_fd_then_manual_send() {
        // PlatformPayload::into_raw_fd 경로 (호출자가 fd 소유권을 명시 회수)도 동작.
        let (_mem_a, handle) = tasty_shm::create(64).expect("create");
        let payload = tasty_shm::prepare_send(handle, PeerPid::Same).expect("prepare_send");
        let size = payload.size();
        let fd = payload.raw_fd();
        // dup으로 별도 fd 만들어 receive 측에 쓰고 원본은 payload가 drop하며 close.
        // SAFETY: dup syscall. fd 유효.
        let dup = unsafe { libc::dup(fd) };
        assert!(dup >= 0);
        let _mem_b = tasty_shm::receive(ReceivedPayload::Fd { fd: dup, size })
            .expect("receive");
        // payload는 함수 끝에서 drop → 원본 fd close. dup된 fd는 _mem_b가 들고 있어 살아있음.
        drop(payload);
    }

}

#[cfg(windows)]
mod win {
    use super::*;

    /// Windows에선 자기 자신 PID로 DuplicateHandle을 호출하면 같은 프로세스 핸들 테이블에
    /// 별도 핸들이 만들어진다. payload.serialized_handle()이 곧 받을 핸들 값.
    fn send_to_self(payload: tasty_shm::TransportPayload) -> ReceivedPayload {
        let h = payload.serialized_handle();
        let size = payload.size();
        // payload는 drop돼도 우리쪽 HANDLE은 없으므로 무관 (Windows PlatformPayload::Drop은 no-op).
        std::mem::forget(payload);
        ReceivedPayload::Handle { handle: h, size }
    }

    #[test]
    fn create_and_receive_share_same_memory() {
        let size = 4096;
        let (mem_a, handle) = tasty_shm::create(size).expect("create");
        let payload = tasty_shm::prepare_send(handle, PeerPid::Same).expect("prepare_send");
        let received = send_to_self(payload);
        let mem_b = tasty_shm::receive(received).expect("receive");

        assert_eq!(mem_a.len(), size);
        assert_eq!(mem_b.len(), size);

        // SAFETY: 단일 스레드.
        unsafe {
            let slice = mem_a.as_mut_slice();
            slice[0..4].copy_from_slice(b"test");
            slice[size - 1] = 0xAB;
        }
        // SAFETY: 단일 스레드.
        unsafe {
            let view = mem_b.as_slice();
            assert_eq!(&view[0..4], b"test");
            assert_eq!(view[size - 1], 0xAB);
        }
    }

    #[test]
    fn write_from_b_visible_in_a() {
        let (mem_a, handle) = tasty_shm::create(8).expect("create");
        let payload = tasty_shm::prepare_send(handle, PeerPid::Same).expect("prepare_send");
        let received = send_to_self(payload);
        let mem_b = tasty_shm::receive(received).expect("receive");

        // SAFETY: 단일 스레드.
        unsafe {
            mem_b.as_mut_slice()[0..3].copy_from_slice(b"hi!");
        }
        // SAFETY: 단일 스레드.
        unsafe {
            assert_eq!(&mem_a.as_slice()[0..3], b"hi!");
        }
    }

    #[test]
    fn drop_unmaps_without_leak() {
        for _ in 0..256 {
            let (mem, handle) = tasty_shm::create(64).expect("create");
            let payload =
                tasty_shm::prepare_send(handle, PeerPid::Same).expect("prepare_send");
            let received = send_to_self(payload);
            let mem2 = tasty_shm::receive(received).expect("receive");
            drop(mem);
            drop(mem2);
        }
    }
}

#[test]
fn zero_size_is_rejected() {
    match tasty_shm::create(0) {
        Err(tasty_shm::ShmError::ZeroSize) => {}
        Err(e) => panic!("expected ZeroSize, got {e:?}"),
        Ok(_) => panic!("expected ZeroSize error"),
    }
}

#[test]
fn too_large_is_rejected() {
    match tasty_shm::create(usize::MAX) {
        Err(tasty_shm::ShmError::TooLarge(_)) => {}
        Err(e) => panic!("expected TooLarge, got {e:?}"),
        Ok(_) => panic!("expected TooLarge error"),
    }
}
