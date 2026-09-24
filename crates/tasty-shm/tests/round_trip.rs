//! 한 프로세스 안에서 핸들 전달을 흉내 내고 두 매핑이 같은 메모리를 보는지 확인한다.

use tasty_shm::{PeerPid, ReceivedPayload};

#[cfg(unix)]
mod unix {
    use super::*;

    /// fd를 복제해 수신 측이 독립적으로 소유할 핸들을 만든다. 실제 IPC 전송은 하지 않는다.
    fn send_to_self(payload: tasty_shm::TransportPayload) -> ReceivedPayload {
        // payload가 들고 있는 fd를 dup해서 별개 fd를 만들어 송신 흉내.
        // SAFETY: dup syscall. payload.raw_fd()가 유효 fd.
        let dup_fd = unsafe { libc::dup(payload.raw_fd()) };
        assert!(
            dup_fd >= 0,
            "dup failed: {}",
            std::io::Error::last_os_error()
        );
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
        // SAFETY: received의 fd는 send_to_self가 방금 dup()으로 만든 새 fd — 다른
        // 곳에서 소유되지 않았다.
        let mem_b = unsafe { tasty_shm::receive(received) }.expect("receive");

        assert_eq!(mem_a.len(), size);
        assert_eq!(mem_b.len(), size);

        // SAFETY: 단일 스레드에서 다른 매핑이 동시 접근하지 않음.
        unsafe {
            let slice = mem_a.as_mut_slice();
            slice[0..4].copy_from_slice(b"test");
            slice[size - 1] = 0xAB;
        }

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
        // SAFETY: 위와 동일 — send_to_self가 dup()으로 만든 새 fd.
        let mem_b = unsafe { tasty_shm::receive(received) }.expect("receive");

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
            let payload = tasty_shm::prepare_send(handle, PeerPid::Same).expect("prepare_send");
            let received = send_to_self(payload);
            // SAFETY: 위와 동일 — send_to_self가 dup()으로 만든 새 fd.
            let mem_b = unsafe { tasty_shm::receive(received) }.expect("receive");

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
            let payload = tasty_shm::prepare_send(handle, PeerPid::Same).expect("prepare_send");
            let received = send_to_self(payload);
            // SAFETY: 위와 동일 — send_to_self가 dup()으로 만든 새 fd.
            let mem_b = unsafe { tasty_shm::receive(received) }.expect("receive");
            assert_eq!(mem_b.len(), size);
        }
    }

    #[test]
    fn drop_unmaps_without_leak() {
        // 반복 매핑·해제 중 fd 누수가 누적되면 생성이 실패할 수 있다.
        for _ in 0..256 {
            let (mem, handle) = tasty_shm::create(64).expect("create");
            let payload = tasty_shm::prepare_send(handle, PeerPid::Same).expect("prepare_send");
            let received = send_to_self(payload);
            // SAFETY: 위와 동일 — send_to_self가 dup()으로 만든 새 fd.
            let mem2 = unsafe { tasty_shm::receive(received) }.expect("receive");
            drop(mem);
            drop(mem2);
        }
    }

    #[test]
    fn into_raw_fd_then_manual_send() {
        let (_mem_a, handle) = tasty_shm::create(64).expect("create");
        let payload = tasty_shm::prepare_send(handle, PeerPid::Same).expect("prepare_send");
        let size = payload.size();
        let fd = payload.raw_fd();
        // payload의 fd를 복제해 수신 측에 독립된 소유권을 준다.
        // SAFETY: dup syscall. fd 유효.
        let dup = unsafe { libc::dup(fd) };
        assert!(dup >= 0);
        // SAFETY: dup은 방금 dup()으로 만든 새 fd — 다른 곳에서 소유되지 않았다.
        let _mem_b =
            unsafe { tasty_shm::receive(ReceivedPayload::Fd { fd: dup, size }) }.expect("receive");
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
        // SAFETY: received의 handle은 prepare_send(PeerPid::Same)가 DuplicateHandle로
        // 이 프로세스 핸들 테이블에 방금 복제한, 별도 소유 값이다.
        let mem_b = unsafe { tasty_shm::receive(received) }.expect("receive");

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
        // SAFETY: 위와 동일 — prepare_send(PeerPid::Same)가 DuplicateHandle로 복제한 값.
        let mem_b = unsafe { tasty_shm::receive(received) }.expect("receive");

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
            let payload = tasty_shm::prepare_send(handle, PeerPid::Same).expect("prepare_send");
            let received = send_to_self(payload);
            // SAFETY: 위와 동일 — prepare_send(PeerPid::Same)가 DuplicateHandle로 복제한 값.
            let mem2 = unsafe { tasty_shm::receive(received) }.expect("receive");
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
