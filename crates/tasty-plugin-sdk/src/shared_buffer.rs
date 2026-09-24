//! 플러그인과 호스트가 함께 매핑하는 바이트 버퍼.
//! HostHandle::create_shared_buffer로 만들고 commit으로 변경을 알린다.
//!
//! 앞의 8바이트는 atomic generation이며 사용자 slice에서는 제외한다.
//! commit은 Release 연산으로 세대를 증가시킨 뒤 Dirty 메시지를 보낸다.
//! 세대 변경과 메시지 송신은 하나의 원자적 연산이 아니며, 송신은 실패할 수 있다.
//! 세대 일치만으로 payload를 읽는 동안 다음 쓰기를 막거나 일관된 프레임을
//! 보장하지는 않는다. Slice의 안전 조건은 호출자가 별도로 지켜야 한다.

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
        // SAFETY: 매핑 시작은 8바이트 정렬이고 길이는 footer::SIZE 이상이다.
        // footer는 AtomicU64로 접근한다. payload의 동시 접근 안전을 뜻하지는 않는다.
        #[allow(clippy::multiple_unsafe_ops_per_block)]
        unsafe {
            tasty_shm::footer::load(self.mem.as_slice(), Ordering::Acquire)
        }
    }

    /// 사용자 영역을 read-only byte slice로 본다 (footer 제외).
    ///
    /// # Safety
    ///
    /// 반환한 참조가 유효한 동안 같은 메모리에 다른 스레드나 프로세스가
    /// 쓰지 않음을 호출자가 보장해야 한다. 잘못 읽은 결과를 허용하는 것으로
    /// 이 조건을 대신할 수는 없다.
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
    /// 반환한 가변 참조가 유효한 동안 같은 메모리에 대한 다른 읽기와 쓰기를
    /// 모두 배제해야 한다. Footer를 제외한 slice라는 사실만으로 동시 접근을
    /// 막을 수는 없다.
    #[allow(clippy::mut_from_ref)]
    pub unsafe fn as_mut_slice(&self) -> &mut [u8] {
        // SAFETY: 위 docstring 조건을 호출자가 보장. footer는 분리되어 user에게 보이지 않음.
        unsafe { tasty_shm::footer::user_slice_mut(self.mem.as_mut_slice()) }
    }

    /// generation을 증가시키고 Dirty 메시지를 보낸다. rect가 None이면 전체 영역이다.
    /// 이 함수는 payload에 대한 다른 읽기·쓰기를 막지 않는다.
    pub fn commit(&self, rect: Option<PixelRect>) -> Result<(), PluginError> {
        // SAFETY: 매핑 시작은 8바이트 정렬이고 길이는 footer::SIZE 이상이다.
        // footer의 세대 값은 atomic 연산으로만 변경한다.
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

    /// generation을 바꾸지 않고 Dirty 메시지만 보낸다.
    /// 세대도 갱신하려면 Self::commit을 사용한다.
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
        // 두 조회 사이에 commit하면 generation 값이 달라지는지 확인한다.
        // 동시 읽기·쓰기의 안전성을 검사하는 시험은 아니다.
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
