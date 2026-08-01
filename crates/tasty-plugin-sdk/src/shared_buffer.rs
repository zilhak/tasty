//! Plugin 쪽에서 호스트와 공유하는 zero-copy 픽셀/바이트 buffer.
//!
//! [`crate::HostHandle::create_shared_buffer`]가 반환하는 타입. 내부적으로
//! [`tasty_shm::SharedMemory`] 매핑과 보조 핸들 채널의 writer를 함께 들고 있어, plugin이
//! 영역을 직접 메모리에 쓰고 [`SharedBuffer::commit`]로 호스트에 변경 영역을
//! 알릴 수 있다.
//!
//! # 동기화 — atomic generation footer
//!
//! 영역의 시작 8바이트는 [`tasty_shm::footer`]의 `AtomicU64 generation`이고,
//! plugin 사용자에게는 보이지 않는다 (`as_mut_slice`/`as_slice`/`len`은 모두 user
//! 영역만 반환). 권장 패턴:
//!
//! 1. `as_mut_slice()`로 user 영역에 픽셀을 쓴다.
//! 2. `commit(rect)`를 호출하면 `fetch_add(1, Release)` + Dirty 메시지가 한 묶음으로
//!    호스트에 송신된다.
//! 3. 호스트는 매 frame `generation`을 Acquire load + 픽셀 read + 다시 Acquire load
//!    하여 두 값이 같을 때만 안정된 frame으로 인정한다.
//!
//! 이 규약을 따르면 plugin의 부분 write 도중 호스트가 읽는 race(half-painted frame)가
//! 발생하지 않는다. 단, 단일 frame 내 read가 자주 실패(skip)할 만큼 빠르게 commit하면
//! 호스트가 frame을 건너뛸 수 있다 — 그 경우 다음 frame에서 회복.

use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};

use tasty_plugin_protocol::{HandleChannelMessage, PixelRect, SharedBufferId};
use tasty_shm::SharedMemory;

use crate::error::PluginError;
use crate::handle_channel::HandleClient;

/// 한 plugin 인스턴스가 호스트와 공유하는 buffer.
///
/// Drop 시 SharedMemory 매핑이 해제된다. 호스트 측 동일 buffer는 호스트 자체 lifecycle을
/// 따른다(plugin이 죽으면 호스트가 cleanup).
pub struct SharedBuffer {
    id: SharedBufferId,
    mem: SharedMemory,
    handle_writer: Arc<Mutex<HandleClient>>,
}

impl SharedBuffer {
    pub(crate) fn new(
        id: SharedBufferId,
        mem: SharedMemory,
        handle_writer: Arc<Mutex<HandleClient>>,
    ) -> Self {
        Self {
            id,
            mem,
            handle_writer,
        }
    }

    /// 호스트가 부여한 buffer id.
    pub fn id(&self) -> SharedBufferId {
        self.id
    }

    /// 사용자 영역의 바이트 길이 (= OS 영역 길이 - footer).
    pub fn len(&self) -> usize {
        tasty_shm::footer::user_len(self.mem.len())
    }

    /// 길이가 0인지 (footer만 있고 user data가 0바이트인 경우 포함).
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// 현재 generation 값을 Acquire load. 호스트와 디버깅 시 사용.
    pub fn generation(&self) -> u64 {
        // SAFETY: SharedMemory 영역의 시작 8바이트는 mmap 페이지 정렬로 8B aligned.
        // tasty_shm::footer의 SAFETY 조건을 충족.
        // mem.as_slice() 와 footer::load 가 한 read 흐름이라 분할 불필요.
        #[allow(clippy::multiple_unsafe_ops_per_block)]
        unsafe {
            tasty_shm::footer::load(self.mem.as_slice(), Ordering::Acquire)
        }
    }

    /// 사용자 영역을 read-only byte slice로 본다 (footer 제외).
    ///
    /// # Safety
    ///
    /// 호스트가 같은 영역을 동시 mutate 중일 수 있으므로, 호출자가 (1) 그 시점이 안전함을
    /// 알거나 (2) 잘못 읽은 결과를 수용해야 한다. 자세한 조건은
    /// [`tasty_shm::SharedMemory::as_slice`] 문서 참조.
    pub unsafe fn as_slice(&self) -> &[u8] {
        // SAFETY: SharedMemory의 안전 조건을 호출자가 보장.
        unsafe { tasty_shm::footer::user_slice(self.mem.as_slice()) }
    }

    /// 사용자 영역을 mutable byte slice로 본다 (footer 제외).
    ///
    /// write 후 보통 [`SharedBuffer::commit`]를 호출해 generation을 증가시키고
    /// 호스트에 dirty 영역을 알린다.
    ///
    /// # Safety
    ///
    /// [`Self::as_slice`]의 조건에 더해, 동시 read/write가 비결정성을 일으킨다는 점을
    /// 호출자가 수용해야 한다. footer 8바이트는 본 슬라이스에 포함되지 않으므로 plugin이
    /// 의도치 않게 atomic을 손상시킬 수 없다.
    #[allow(clippy::mut_from_ref)]
    pub unsafe fn as_mut_slice(&self) -> &mut [u8] {
        // SAFETY: 위 docstring 조건을 호출자가 보장. footer는 분리되어 user에게 보이지 않음.
        unsafe { tasty_shm::footer::user_slice_mut(self.mem.as_mut_slice()) }
    }

    /// generation을 1 증가시키고 호스트에 Dirty 메시지를 송신한다.
    ///
    /// 권장 호출 순서:
    ///
    /// 1. `as_mut_slice()`로 픽셀을 모두 쓴다.
    /// 2. 본 함수를 호출한다. atomic increment(Release)로 호스트가 일관된 frame을 볼
    ///    수 있게 되고, 보조 채널로 dirty rect가 송신된다.
    ///
    /// `rect`가 `None`이면 전체 영역이 dirty.
    pub fn commit(&self, rect: Option<PixelRect>) -> Result<(), PluginError> {
        // SAFETY: footer atomic 접근. SharedMemory 영역은 mmap 페이지 정렬이라 8B aligned.
        // mem.as_slice() 와 footer::fetch_add 가 한 atomic 흐름이라 분할 불필요.
        #[allow(clippy::multiple_unsafe_ops_per_block)]
        unsafe {
            tasty_shm::footer::fetch_add(self.mem.as_slice(), 1, Ordering::Release);
        }
        let msg = HandleChannelMessage::Dirty { id: self.id, rect };
        let mut w = self
            .handle_writer
            .lock()
            .map_err(|_| PluginError::LockPoisoned("handle writer"))?;
        w.send_message(&msg)
    }

    /// (호환용) generation 증가 없이 dirty 메시지만 송신.
    ///
    /// 새 코드는 [`commit`]을 사용해 atomic 동기화를 함께 받아야 한다. 본 함수는
    /// generation footer를 사용하지 않는 시나리오(예: 단순 디버깅, 단일 frame 후
    /// 즉시 종료)나, plugin이 직접 generation을 관리하는 고급 사용처를 위한 escape
    /// hatch다.
    ///
    /// [`commit`]: Self::commit
    pub fn mark_dirty(&self, rect: Option<PixelRect>) -> Result<(), PluginError> {
        let msg = HandleChannelMessage::Dirty { id: self.id, rect };
        let mut w = self
            .handle_writer
            .lock()
            .map_err(|_| PluginError::LockPoisoned("handle writer"))?;
        w.send_message(&msg)
    }
}

#[cfg(all(unix, test))]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader};
    use std::os::unix::net::UnixStream;

    /// SharedBuffer 한 쌍을 만든다: plugin 측에서 보는 SharedBuffer + host 측에서 같은
    /// OS 영역을 매핑한 raw SharedMemory + host가 받는 dirty 메시지 reader.
    ///
    /// socketpair로 plugin↔host의 보조 채널을 모의. plugin commit으로 송신된 Dirty
    /// 메시지를 reader가 NDJSON으로 받아서 검증할 수 있다.
    fn make_pair(user_size: usize) -> (SharedBuffer, SharedMemory, BufReader<UnixStream>) {
        let total = user_size + tasty_shm::footer::SIZE;
        let (plugin_mem, sendable) = tasty_shm::create(total).expect("create shm");
        let payload =
            tasty_shm::prepare_send(sendable, tasty_shm::PeerPid::Same).expect("prepare_send");
        // prepare_send가 raw_fd를 빌려준다. 같은 fd를 receive에 넘기면 새 매핑.
        // dup으로 별도 매핑을 만들어 host 측 mem으로 사용 — payload는 drop해도 plugin
        // 측 mem이 fd를 소유하지 않는 형태이므로 dup이 필요.
        let fd = payload.raw_fd();
        // SAFETY: fd는 payload가 살아있는 동안 유효하다. libc::dup은 OS 콜로 새 fd를 반환.
        let dup_fd = unsafe { libc::dup(fd) };
        assert!(dup_fd >= 0, "dup failed");
        // SAFETY: dup_fd는 방금 dup()으로 만든 새 fd — 다른 어디에도 소유되지 않았다.
        let host_mem = unsafe {
            tasty_shm::receive(tasty_shm::ReceivedPayload::Fd {
                fd: dup_fd,
                size: total,
            })
        }
        .expect("host receive");
        // payload는 더 필요 없음.
        drop(payload);

        let (plugin_sock, host_sock) = UnixStream::pair().expect("socketpair");
        let writer = Arc::new(Mutex::new(
            crate::handle_channel::HandleClient::from_unix_stream(plugin_sock),
        ));
        let buffer = SharedBuffer::new(SharedBufferId(42), plugin_mem, writer);
        let host_reader = BufReader::new(host_sock);
        (buffer, host_mem, host_reader)
    }

    #[test]
    fn user_len_excludes_footer() {
        let (buf, _host, _r) = make_pair(4096);
        assert_eq!(buf.len(), 4096);
    }

    #[test]
    fn generation_starts_at_zero() {
        let (buf, _host, _r) = make_pair(64);
        assert_eq!(buf.generation(), 0);
    }

    #[test]
    fn commit_increments_generation_and_emits_dirty() {
        let (buf, host_mem, mut reader) = make_pair(64);
        // SAFETY: 단일 thread, 동시 mutate 없음.
        unsafe {
            buf.as_mut_slice()[0] = 0xAB;
        }
        let rect = Some(PixelRect {
            x: 0,
            y: 0,
            w: 8,
            h: 8,
        });
        buf.commit(rect).expect("commit");

        assert_eq!(buf.generation(), 1);
        // host 측에서도 같은 atomic 값과 user 데이터를 본다.
        // SAFETY: plugin이 write를 마쳤고 후속 동시 mutate 없음.
        let host_raw = unsafe { host_mem.as_slice() };
        // SAFETY: host_raw는 8-aligned mmap 시작이고 길이 >= footer::SIZE.
        let host_gen = unsafe { tasty_shm::footer::load(host_raw, Ordering::Acquire) };
        assert_eq!(host_gen, 1);
        assert_eq!(tasty_shm::footer::user_slice(host_raw)[0], 0xAB);

        // host_reader는 Dirty NDJSON 한 줄을 받는다.
        let mut line = String::new();
        reader.read_line(&mut line).expect("read dirty");
        let msg: HandleChannelMessage = serde_json::from_str(line.trim()).expect("decode");
        assert_eq!(
            msg,
            HandleChannelMessage::Dirty {
                id: SharedBufferId(42),
                rect,
            }
        );
    }

    #[test]
    fn double_load_detects_in_flight_write() {
        // tear 감지 패턴: plugin이 commit 직전(fetch_add 호출 전)에 write 중이면,
        // host가 before/after load 사이에 plugin이 commit하면 두 값이 다르다.
        let (buf, host_mem, _r) = make_pair(64);
        // SAFETY: 단일 thread 시나리오.
        unsafe {
            buf.as_mut_slice()[0] = 0x11;
        }
        // SAFETY: plugin이 write를 끝낸 후 동시 mutate 없음.
        let host_raw = unsafe { host_mem.as_slice() };
        // SAFETY: host_raw는 8-aligned이고 길이 >= footer::SIZE.
        let before = unsafe { tasty_shm::footer::load(host_raw, Ordering::Acquire) };
        buf.commit(None).expect("commit");
        // SAFETY: 위와 동일.
        let after = unsafe { tasty_shm::footer::load(host_raw, Ordering::Acquire) };
        assert_ne!(before, after, "commit increments generation");
    }
}
