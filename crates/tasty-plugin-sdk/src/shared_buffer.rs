//! Plugin 쪽에서 호스트와 공유하는 zero-copy 픽셀/바이트 buffer.
//!
//! [`crate::HostHandle::create_shared_buffer`]가 반환하는 타입. 내부적으로
//! [`tasty_shm::SharedMemory`] 매핑과 보조 핸들 채널의 writer를 함께 들고 있어, plugin이
//! 영역을 직접 메모리에 쓰고 [`SharedBuffer::mark_dirty`]로 호스트에 변경 영역을
//! 알릴 수 있다.
//!
//! # 동기화
//!
//! 동일 영역을 host와 plugin이 동시에 mutate하면 data race(UB). 권장 패턴은
//! "plugin이 쓴 뒤 mark_dirty, host가 읽고 다음 프레임 합성". `as_mut_slice`는
//! `unsafe`이므로 호출자가 이 동기화 책임을 진다.

use std::sync::{Arc, Mutex};

use tasty_plugin_protocol::{HandleChannelMessage, Rect, SharedBufferId};
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

    /// 매핑된 영역의 바이트 길이.
    pub fn len(&self) -> usize {
        self.mem.len()
    }

    /// 길이가 0인지 (실제로는 0-byte 영역을 만들 수 없음).
    pub fn is_empty(&self) -> bool {
        self.mem.is_empty()
    }

    /// 영역 전체를 read-only byte slice로 본다.
    ///
    /// # Safety
    ///
    /// 호스트가 같은 영역을 동시 mutate 중일 수 있으므로, 호출자가 (1) 그 시점이 안전함을
    /// 알거나 (2) 잘못 읽은 결과를 수용해야 한다. 자세한 조건은
    /// [`tasty_shm::SharedMemory::as_slice`] 문서 참조.
    pub unsafe fn as_slice(&self) -> &[u8] {
        // SAFETY: SharedMemory의 안전 조건을 호출자가 보장.
        unsafe { self.mem.as_slice() }
    }

    /// 영역 전체를 mutable byte slice로 본다. write 후 보통 [`SharedBuffer::mark_dirty`]
    /// 를 호출해 호스트에 변경 영역을 알린다.
    ///
    /// # Safety
    ///
    /// [`Self::as_slice`]의 조건에 더해, 동시 read/write가 비결정성을 일으킨다는 점을
    /// 호출자가 수용해야 한다.
    #[allow(clippy::mut_from_ref)]
    pub unsafe fn as_mut_slice(&self) -> &mut [u8] {
        // SAFETY: 위 docstring 조건을 호출자가 보장.
        unsafe { self.mem.as_mut_slice() }
    }

    /// 보조 채널로 dirty 알림을 비동기 송신. `None`이면 전체 영역.
    /// 호스트는 이를 받아 다음 프레임 합성 시 읽는다.
    pub fn mark_dirty(&self, rect: Option<Rect>) -> Result<(), PluginError> {
        let msg = HandleChannelMessage::Dirty { id: self.id, rect };
        let mut w = self
            .handle_writer
            .lock()
            .map_err(|_| PluginError::LockPoisoned("handle writer"))?;
        w.send_message(&msg)
    }
}
