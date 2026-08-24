//! attach mesh mirror — 클라이언트측 최신 frame 저장소
//! (`docs/dev-guide/attach-behavior.md` "mesh mirror 채널").
//!
//! 서버(`src/core/mesh_mirror.rs`)의 `MeshMirrorRegistry`가 "누가 무엇을 구독 중인가"
//! 라면, 이건 그 반대편 — attach client 가 TCP 로 받아 재조립한 mesh 바이트를
//! `AttachMeshSurface` local id 별로 보관한다. 로컬 `SharedBuffer`가 없으므로 footer/
//! generation atomic 이 아니라 그냥 최신 값 저장(TCP 는 신뢰·순서 보장 전송이라 tear 나
//! 재정렬을 걱정할 필요가 없다 — `mesh_stream::MeshFrameAssembler`가 이미 순서를 보장).

use std::collections::HashMap;

/// 한 attach mesh surface 의 최신 수신 frame.
#[derive(Debug, Clone)]
pub(crate) struct AttachMeshFrame {
    /// `mesh_wire::decode_paint` 가 바로 소비할 수 있는 순수 payload(footer 없음 —
    /// 서버가 `SharedBuffer` footer 를 이미 벗기고 보냈다, `headless_plugins::forward_mesh_frames`).
    pub(crate) bytes: Vec<u8>,
    pub(crate) generation: u64,
    pub(crate) frame_seq: u64,
    pub(crate) full_textures: bool,
}

/// surface_id(local) → 최신 frame. attach 세션이 붙어있는 동안만 채워진다.
#[derive(Debug, Default)]
pub(crate) struct AttachMeshFrameStore {
    frames: HashMap<u32, AttachMeshFrame>,
}

impl AttachMeshFrameStore {
    pub(crate) fn update(
        &mut self,
        surface_id: u32,
        bytes: Vec<u8>,
        generation: u64,
        frame_seq: u64,
        full_textures: bool,
    ) {
        self.frames.insert(
            surface_id,
            AttachMeshFrame {
                bytes,
                generation,
                frame_seq,
                full_textures,
            },
        );
    }

    pub(crate) fn get(&self, surface_id: u32) -> Option<&AttachMeshFrame> {
        self.frames.get(&surface_id)
    }

    /// surface 가 닫혔거나(mirror 정리) 세션이 끊겼을 때 정리.
    pub(crate) fn remove(&mut self, surface_id: u32) {
        self.frames.remove(&surface_id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn update_then_get_round_trips() {
        let mut store = AttachMeshFrameStore::default();
        store.update(1, vec![1, 2, 3], 5, 9, true);
        let f = store.get(1).unwrap();
        assert_eq!(f.bytes, vec![1, 2, 3]);
        assert_eq!(f.generation, 5);
        assert_eq!(f.frame_seq, 9);
        assert!(f.full_textures);
    }

    #[test]
    fn get_missing_is_none() {
        let store = AttachMeshFrameStore::default();
        assert!(store.get(1).is_none());
    }

    #[test]
    fn update_overwrites_previous_frame() {
        let mut store = AttachMeshFrameStore::default();
        store.update(1, vec![1], 1, 1, true);
        store.update(1, vec![2, 2], 2, 2, false);
        let f = store.get(1).unwrap();
        assert_eq!(f.bytes, vec![2, 2]);
        assert_eq!(f.generation, 2);
    }

    #[test]
    fn remove_clears_entry() {
        let mut store = AttachMeshFrameStore::default();
        store.update(1, vec![1], 1, 1, true);
        store.remove(1);
        assert!(store.get(1).is_none());
    }
}
