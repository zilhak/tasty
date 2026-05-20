//! GPU shared buffer 매핑 관리. plugin 별 buffer id 발급 + dirty rect drain.

use std::collections::HashMap;
use std::sync::atomic::Ordering;

use tasty_plugin_protocol::{HandleChannelMessage, SharedBufferCreateResult, SharedBufferId};
#[cfg(unix)]
use tasty_shm::PeerPid;
use tasty_shm::SharedMemory;

use super::PluginManager;

impl PluginManager {
    pub fn take_plugin_dirty_rects(
        &self,
        plugin_id: &str,
    ) -> HashMap<SharedBufferId, Option<tasty_plugin_protocol::Rect>> {
        self.processes
            .get(plugin_id)
            .map(|p| p.take_dirty_rects())
            .unwrap_or_default()
    }

    /// (plugin_id, buffer_id)에 해당하는 호스트 측 매핑된 [`SharedMemory`] 참조.
    /// 캔버스 렌더 시 atomic generation + user data를 읽기 위해 사용.
    pub fn plugin_buffer(
        &self,
        plugin_id: &str,
        buffer_id: SharedBufferId,
    ) -> Option<&SharedMemory> {
        self.plugin_buffers.get(plugin_id)?.get(&buffer_id)
    }

    /// 활성 plugin 목록 (`SharedBuffer`를 보유 중인 plugin만). canvas prepare가 iterating
    /// 시 사용.
    pub fn plugin_ids_with_buffers(&self) -> Vec<String> {
        self.plugin_buffers.keys().cloned().collect()
    }

    /// `host.shared_buffer.create` 처리. 새 공유 메모리 영역을 만들어
    /// 메인 채널 결과(`SharedBufferCreateResult`)와 보조 채널 핸들(`HandleAttach`)을
    /// 양쪽 모두 전송한다.
    ///
    /// - main 채널 응답은 caller(`App::process_plugin_ipc_calls`)가
    ///   `send_ipc_result`로 회신.
    /// - 보조 채널 핸들은 본 메서드가 직접 송신 (plugin SDK는 같은 call_id로
    ///   매칭되는 `HandleAttach`를 기다린다).
    ///
    /// 핸들 전송이 실패하면 SharedMemory를 등록하지 않고 에러를 반환한다 — plugin은
    /// `host_call_timeout` 등으로 인식한다.
    #[cfg(unix)]
    pub fn create_shared_buffer_for(
        &mut self,
        plugin_id: &str,
        call_id: u64,
        size: u64,
    ) -> Result<SharedBufferCreateResult, String> {
        const MAX_BYTES: u64 = 1 << 30; // 1 GiB. manifest 권한 도입 전 임시 상한.
        if size == 0 {
            return Err("shared_buffer.create: size must be > 0".into());
        }
        if size > MAX_BYTES {
            return Err(format!(
                "shared_buffer.create: size {size} exceeds host cap {MAX_BYTES}"
            ));
        }
        let proc = self
            .processes
            .get(plugin_id)
            .ok_or_else(|| format!("plugin '{plugin_id}' is not running"))?;
        // 보조 채널이 없으면 핸들 전송이 불가능. 즉시 거절.
        if self.handle_listener.is_none() {
            return Err("shared_buffer.create: host handle channel not available".into());
        }

        let (mem, sendable) = tasty_shm::create(size as usize)
            .map_err(|e| format!("shared_buffer.create: tasty_shm::create failed: {e}"))?;
        // Unix는 peer pid를 무시하지만 의도 명시를 위해 child pid를 넘긴다.
        let peer = match proc.child_pid() {
            Some(pid) => PeerPid::Other(pid),
            None => PeerPid::Same,
        };
        let payload = tasty_shm::prepare_send(sendable, peer)
            .map_err(|e| format!("shared_buffer.create: prepare_send failed: {e}"))?;

        let id = SharedBufferId(self.next_buffer_id.fetch_add(1, Ordering::Relaxed));
        let actual_size = mem.len() as u64;
        let msg = HandleChannelMessage::HandleAttach {
            request_id: call_id,
            id,
            size: actual_size,
        };
        let raw_fd = payload.raw_fd();
        let send_result = proc.with_handle_stream(|stream| stream.send_handle(&msg, raw_fd));
        match send_result {
            Some(Ok(())) => {}
            Some(Err(e)) => {
                return Err(format!(
                    "shared_buffer.create: handle channel send failed: {e}"
                ));
            }
            None => {
                return Err("shared_buffer.create: plugin handle channel not connected".into());
            }
        }
        // SharedMemory를 매니저가 보관 — Drop이 일어나면 OS region이 회수되므로
        // plugin이 매핑을 잡고 있는 한 살아있어야 한다. payload는 method 끝에서
        // Drop되며 송신 fd가 닫힌다(매핑 fd는 mem 안에 별도로 보존).
        self.plugin_buffers
            .entry(plugin_id.to_string())
            .or_default()
            .insert(id, mem);
        drop(payload);
        Ok(SharedBufferCreateResult {
            id,
            size: actual_size,
        })
    }

    /// Windows는 보조 채널 핸들 전송이 02c에서 미구현. plugin SDK 측이 미리
    /// `HandleChannelUnavailable`을 반환하므로 실제로 호출되지 않지만, 호스트
    /// 측에서도 명시적으로 거절한다.
    #[cfg(windows)]
    pub fn create_shared_buffer_for(
        &mut self,
        _plugin_id: &str,
        _call_id: u64,
        _size: u64,
    ) -> Result<SharedBufferCreateResult, String> {
        Err("shared_buffer.create: windows host-side not implemented".into())
    }
}
