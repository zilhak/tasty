//! 클라이언트가 조립한 mesh frame을 로컬 surface ID별로 보관한다.
//! SharedBuffer footer는 포함하지 않는다. update는 generation·frame_seq를 비교하지 않고 받은 값으로 덮어쓴다.

use std::collections::HashMap;

#[derive(Debug, Clone)]
pub(crate) struct AttachMeshFrame {
    /// mesh_wire::decode_paint에 전달할 footer 없는 payload.
    pub(crate) bytes: Vec<u8>,
    pub(crate) generation: u64,
    pub(crate) frame_seq: u64,
    pub(crate) full_textures: bool,
}

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
