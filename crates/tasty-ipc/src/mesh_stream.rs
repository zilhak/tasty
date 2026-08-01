//! attach mesh mirror(plugin egui-mesh surface)의 바이너리 chunk 프로토콜.
//!
//! mesh 프레임(`mesh_wire::encode_paint` 가 만든 POD 바이트, `tasty-plugin-protocol`)은
//! 큰 텍스처(512×512 RGBA 급이면 ~1MiB)를 포함할 수 있어 [`crate::stream::MAX_FRAME_LEN`]
//! (1MiB)을 쉽게 초과한다. JSON(`StreamControl`)+base64 는 33% 오버헤드를 더하므로
//! 배제하고, 여기서는 opaque 바이트를 청크로 쪼개 [`crate::stream::StreamTag::MeshData`]
//! 프레임으로 나른다 — **이 crate 는 mesh 바이트를 디코드하지 않는다**(`mesh_wire` 는
//! egui-mesh feature 에 묶여 있어 non-GUI 빌드에 새는 것을 막는다).
//!
//! 헤더 레이아웃 (전부 big-endian, [`MESH_CHUNK_HEADER_LEN`] 바이트):
//! `[surface_id:u32][frame_id:u64][chunk_index:u32][chunk_count:u32][total_len:u32]
//!  [generation:u64][frame_seq:u64][full_textures:u8]` + 이어서 chunk payload 바이트.

use std::collections::HashMap;

/// 청크 헤더의 고정 와이어 크기(바이트).
pub const MESH_CHUNK_HEADER_LEN: usize = 4 + 8 + 4 + 4 + 4 + 8 + 8 + 1;

/// 한 [`crate::stream::StreamTag::MeshData`] 프레임에 실을 수 있는 chunk payload 최대
/// 길이 — frame 전체(헤더+payload)가 [`crate::stream::MAX_FRAME_LEN`] 을 넘지 않도록.
pub const MESH_CHUNK_MAX_PAYLOAD: usize =
    (crate::stream::MAX_FRAME_LEN as usize) - MESH_CHUNK_HEADER_LEN;

/// mesh 바이트 청크 하나의 메타.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MeshChunkMeta {
    pub surface_id: u32,
    /// 이 청크들이 속한 mesh frame 의 단조 id — 서버가 프레임마다 1씩 증가시켜
    /// 발급한다(`frame_seq`/`generation` 과 별개, 순수 chunk 재조립용 키).
    pub frame_id: u64,
    pub chunk_index: u32,
    pub chunk_count: u32,
    /// 이 frame_id 에 속한 전체 payload 길이(모든 청크 합산).
    pub total_len: u32,
    pub generation: u64,
    pub frame_seq: u64,
    pub full_textures: bool,
}

/// 청크 하나를 [`crate::stream::StreamTag::MeshData`] 프레임 payload 로 인코드.
pub fn encode_mesh_chunk(meta: &MeshChunkMeta, chunk: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(MESH_CHUNK_HEADER_LEN + chunk.len());
    out.extend_from_slice(&meta.surface_id.to_be_bytes());
    out.extend_from_slice(&meta.frame_id.to_be_bytes());
    out.extend_from_slice(&meta.chunk_index.to_be_bytes());
    out.extend_from_slice(&meta.chunk_count.to_be_bytes());
    out.extend_from_slice(&meta.total_len.to_be_bytes());
    out.extend_from_slice(&meta.generation.to_be_bytes());
    out.extend_from_slice(&meta.frame_seq.to_be_bytes());
    out.push(meta.full_textures as u8);
    out.extend_from_slice(chunk);
    out
}

/// [`encode_mesh_chunk`] 의 역연산. 헤더보다 짧으면 `None`(잘린 프레임).
pub fn decode_mesh_chunk(buf: &[u8]) -> Option<(MeshChunkMeta, &[u8])> {
    if buf.len() < MESH_CHUNK_HEADER_LEN {
        return None;
    }
    let surface_id = u32::from_be_bytes(buf[0..4].try_into().ok()?);
    let frame_id = u64::from_be_bytes(buf[4..12].try_into().ok()?);
    let chunk_index = u32::from_be_bytes(buf[12..16].try_into().ok()?);
    let chunk_count = u32::from_be_bytes(buf[16..20].try_into().ok()?);
    let total_len = u32::from_be_bytes(buf[20..24].try_into().ok()?);
    let generation = u64::from_be_bytes(buf[24..32].try_into().ok()?);
    let frame_seq = u64::from_be_bytes(buf[32..40].try_into().ok()?);
    let full_textures = buf[40] != 0;
    let meta = MeshChunkMeta {
        surface_id,
        frame_id,
        chunk_index,
        chunk_count,
        total_len,
        generation,
        frame_seq,
        full_textures,
    };
    Some((meta, &buf[MESH_CHUNK_HEADER_LEN..]))
}

/// 한 mesh frame(opaque 바이트)을 [`MESH_CHUNK_MAX_PAYLOAD`] 이하 청크들로 쪼개,
/// 각각 [`encode_mesh_chunk`] 로 이미 인코드된 [`crate::stream::StreamTag::MeshData`]
/// 프레임 payload 목록을 반환한다(호출자는 `write_frame(w, StreamTag::MeshData, payload)`
/// 로 순서대로 내보내면 된다). `bytes` 가 비어 있어도 최소 1개 청크(빈 payload)를
/// 만들어 chunk_count=1 불변식을 지킨다.
pub fn split_mesh_frame(
    surface_id: u32,
    frame_id: u64,
    generation: u64,
    frame_seq: u64,
    full_textures: bool,
    bytes: &[u8],
) -> Vec<Vec<u8>> {
    let total_len = bytes.len() as u32;
    let chunk_count = bytes.len().div_ceil(MESH_CHUNK_MAX_PAYLOAD).max(1) as u32;
    (0..chunk_count)
        .map(|i| {
            let start = i as usize * MESH_CHUNK_MAX_PAYLOAD;
            let end = (start + MESH_CHUNK_MAX_PAYLOAD).min(bytes.len());
            let meta = MeshChunkMeta {
                surface_id,
                frame_id,
                chunk_index: i,
                chunk_count,
                total_len,
                generation,
                frame_seq,
                full_textures,
            };
            encode_mesh_chunk(&meta, &bytes[start..end])
        })
        .collect()
}

/// 재조립 실패 사유.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MeshAssembleError {
    /// 헤더가 잘렸거나 파싱 불가.
    Malformed,
    /// `total_len`이 [`MESH_CHUNK_MAX_PAYLOAD`] * `chunk_count` 로 도달 가능한 최댓값을
    /// 초과 — 손상되었거나 악의적인 length 메타로 간주해 즉시 거부(과할당 방지).
    LengthMismatch,
}

/// 진행 중인(아직 chunk_count 만큼 다 안 모인) frame 재조립 상태.
struct InFlight {
    chunk_count: u32,
    total_len: u32,
    generation: u64,
    frame_seq: u64,
    full_textures: bool,
    received: HashMap<u32, Vec<u8>>,
}

/// client 가 여러 mesh surface 의 청크를 뒤섞어 받아도(같은 스트림, 여러 surface_id)
/// frame_id 단위로 올바르게 재조립하는 상태 머신. surface 당 하나씩 두는 것을 권장
/// (다른 surface 의 frame_id 재조립 상태를 서로 침범하지 않도록 `surface_id` 로도 키잉).
#[derive(Default)]
pub struct MeshFrameAssembler {
    inflight: HashMap<(u32, u64), InFlight>,
}

impl MeshFrameAssembler {
    pub fn new() -> Self {
        Self::default()
    }

    /// 청크 하나를 투입. 그 frame_id 의 마지막 청크였으면 조립된 전체 바이트를
    /// `Ok(Some((meta, bytes)))` 로 반환, 아직 미완이면 `Ok(None)`. 손상/악의적 메타는
    /// `Err` — 호출자는 이 청크(그리고 필요하면 해당 frame_id 의 진행 상태)를 버려야 한다.
    pub fn push_chunk(
        &mut self,
        buf: &[u8],
    ) -> Result<Option<(MeshChunkMeta, Vec<u8>)>, MeshAssembleError> {
        let (meta, payload) = decode_mesh_chunk(buf).ok_or(MeshAssembleError::Malformed)?;
        if meta.chunk_count == 0 || meta.chunk_index >= meta.chunk_count {
            return Err(MeshAssembleError::Malformed);
        }
        // 방어: chunk_count 개가 최대 payload 크기로 채워도 total_len 에 못 미치면
        // (즉 total_len 이 도달 불가능하게 큼) 손상/악의적 메타로 거부한다. 실제
        // payload 초과 확인은 각 청크 삽입 시점에도 별도로 한다(아래).
        let max_possible = (meta.chunk_count as u64) * (MESH_CHUNK_MAX_PAYLOAD as u64);
        if meta.total_len as u64 > max_possible {
            return Err(MeshAssembleError::LengthMismatch);
        }

        let key = (meta.surface_id, meta.frame_id);
        let slot = self.inflight.entry(key).or_insert_with(|| InFlight {
            chunk_count: meta.chunk_count,
            total_len: meta.total_len,
            generation: meta.generation,
            frame_seq: meta.frame_seq,
            full_textures: meta.full_textures,
            received: HashMap::new(),
        });
        // 동일 frame_id 재사용 시 메타 불일치는 손상으로 취급.
        if slot.chunk_count != meta.chunk_count || slot.total_len != meta.total_len {
            self.inflight.remove(&key);
            return Err(MeshAssembleError::Malformed);
        }
        if payload.len() > MESH_CHUNK_MAX_PAYLOAD {
            self.inflight.remove(&key);
            return Err(MeshAssembleError::LengthMismatch);
        }
        slot.received.insert(meta.chunk_index, payload.to_vec());

        if slot.received.len() as u32 == slot.chunk_count {
            let slot = self.inflight.remove(&key).expect("just inserted above");
            let mut assembled = Vec::with_capacity(slot.total_len as usize);
            for i in 0..slot.chunk_count {
                let Some(part) = slot.received.get(&i) else {
                    // 인덱스 구멍(중복 삽입으로 다른 인덱스가 덮여씀) — 손상.
                    return Err(MeshAssembleError::Malformed);
                };
                assembled.extend_from_slice(part);
            }
            if assembled.len() as u32 != slot.total_len {
                return Err(MeshAssembleError::LengthMismatch);
            }
            let out_meta = MeshChunkMeta {
                surface_id: meta.surface_id,
                frame_id: meta.frame_id,
                chunk_index: 0,
                chunk_count: slot.chunk_count,
                total_len: slot.total_len,
                generation: slot.generation,
                frame_seq: slot.frame_seq,
                full_textures: slot.full_textures,
            };
            return Ok(Some((out_meta, assembled)));
        }
        Ok(None)
    }

    /// 특정 surface 의 진행 중이던 재조립 상태를 모두 버린다 — detach/재연결 시
    /// stale in-flight frame 이 다음 세션에 섞여 들어가지 않도록.
    pub fn forget_surface(&mut self, surface_id: u32) {
        self.inflight.retain(|(sid, _), _| *sid != surface_id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stream::{StreamTag, read_frame, write_frame};
    use std::io::Cursor;

    #[test]
    fn chunk_header_roundtrip() {
        let meta = MeshChunkMeta {
            surface_id: 7,
            frame_id: 99,
            chunk_index: 1,
            chunk_count: 3,
            total_len: 12345,
            generation: 42,
            frame_seq: 8,
            full_textures: true,
        };
        let encoded = encode_mesh_chunk(&meta, b"hello");
        let (decoded, payload) = decode_mesh_chunk(&encoded).unwrap();
        assert_eq!(decoded, meta);
        assert_eq!(payload, b"hello");
    }

    #[test]
    fn decode_rejects_truncated_header() {
        assert!(decode_mesh_chunk(&[0u8; MESH_CHUNK_HEADER_LEN - 1]).is_none());
    }

    #[test]
    fn split_and_reassemble_large_frame() {
        // MAX_FRAME_LEN(1MiB) 을 초과하는 합성 mesh 바이트.
        let bytes: Vec<u8> = (0..(crate::stream::MAX_FRAME_LEN as usize * 2 + 777))
            .map(|i| (i % 256) as u8)
            .collect();
        let chunks = split_mesh_frame(3, 1, 10, 20, false, &bytes);
        assert!(
            chunks.len() > 1,
            "must actually split across multiple chunks"
        );

        // 각 청크를 실제 StreamTag::MeshData 프레임으로 왕복(프레이밍 계층까지 포함)
        // 시켜 MAX_FRAME_LEN 위반이 없는지도 함께 검증한다.
        let mut assembler = MeshFrameAssembler::new();
        let mut result = None;
        for chunk in &chunks {
            let mut buf = Vec::new();
            write_frame(&mut buf, StreamTag::MeshData, chunk).unwrap();
            let mut cur = Cursor::new(buf);
            let frame = read_frame(&mut cur).unwrap();
            assert_eq!(frame.tag, StreamTag::MeshData);
            if let Some(r) = assembler.push_chunk(&frame.payload).unwrap() {
                result = Some(r);
            }
        }
        let (meta, assembled) = result.expect("all chunks delivered — must assemble");
        assert_eq!(assembled, bytes);
        assert_eq!(meta.surface_id, 3);
        assert_eq!(meta.frame_id, 1);
        assert_eq!(meta.generation, 10);
        assert_eq!(meta.frame_seq, 20);
        assert!(!meta.full_textures);
    }

    #[test]
    fn split_empty_frame_yields_one_chunk() {
        let chunks = split_mesh_frame(1, 1, 0, 0, true, &[]);
        assert_eq!(chunks.len(), 1);
        let mut assembler = MeshFrameAssembler::new();
        let (meta, assembled) = assembler
            .push_chunk(&chunks[0])
            .unwrap()
            .expect("single chunk completes immediately");
        assert!(assembled.is_empty());
        assert!(meta.full_textures);
    }

    #[test]
    fn interleaved_surfaces_do_not_cross_contaminate() {
        let bytes_a = vec![0xAAu8; MESH_CHUNK_MAX_PAYLOAD + 10];
        let bytes_b = vec![0xBBu8; MESH_CHUNK_MAX_PAYLOAD + 10];
        let chunks_a = split_mesh_frame(1, 1, 0, 0, false, &bytes_a);
        let chunks_b = split_mesh_frame(2, 1, 0, 0, false, &bytes_b);
        assert_eq!(chunks_a.len(), 2);
        assert_eq!(chunks_b.len(), 2);

        let mut assembler = MeshFrameAssembler::new();
        // interleave: a0, b0, a1, b1
        assert!(assembler.push_chunk(&chunks_a[0]).unwrap().is_none());
        assert!(assembler.push_chunk(&chunks_b[0]).unwrap().is_none());
        let (meta_a, assembled_a) = assembler.push_chunk(&chunks_a[1]).unwrap().unwrap();
        let (meta_b, assembled_b) = assembler.push_chunk(&chunks_b[1]).unwrap().unwrap();
        assert_eq!(meta_a.surface_id, 1);
        assert_eq!(assembled_a, bytes_a);
        assert_eq!(meta_b.surface_id, 2);
        assert_eq!(assembled_b, bytes_b);
    }

    #[test]
    fn malicious_total_len_is_rejected() {
        // chunk_count=1 인데 total_len 이 실제 도달 가능한 최댓값을 초과.
        let meta = MeshChunkMeta {
            surface_id: 1,
            frame_id: 1,
            chunk_index: 0,
            chunk_count: 1,
            total_len: u32::MAX,
            generation: 0,
            frame_seq: 0,
            full_textures: false,
        };
        let encoded = encode_mesh_chunk(&meta, b"short");
        let mut assembler = MeshFrameAssembler::new();
        let err = assembler.push_chunk(&encoded).unwrap_err();
        assert_eq!(err, MeshAssembleError::LengthMismatch);
    }

    #[test]
    fn total_len_not_matching_actual_assembled_is_rejected() {
        // 단일 청크인데 total_len 이 실제 payload 길이와 다름(각 청크는 유효 범위
        // 내지만 합산이 안 맞는 손상 케이스).
        let meta = MeshChunkMeta {
            surface_id: 1,
            frame_id: 1,
            chunk_index: 0,
            chunk_count: 1,
            total_len: 999, // 실제론 5바이트
            generation: 0,
            frame_seq: 0,
            full_textures: false,
        };
        let encoded = encode_mesh_chunk(&meta, b"short");
        let mut assembler = MeshFrameAssembler::new();
        let err = assembler.push_chunk(&encoded).unwrap_err();
        assert_eq!(err, MeshAssembleError::LengthMismatch);
    }

    #[test]
    fn forget_surface_drops_inflight_state() {
        let bytes = vec![0u8; MESH_CHUNK_MAX_PAYLOAD + 1];
        let chunks = split_mesh_frame(5, 1, 0, 0, false, &bytes);
        assert_eq!(chunks.len(), 2);
        let mut assembler = MeshFrameAssembler::new();
        assert!(assembler.push_chunk(&chunks[0]).unwrap().is_none());
        assembler.forget_surface(5);
        // 남은 청크만 넣으면 인덱스가 다 안 모여 여전히 None (구 상태가 안 남아있단 방증).
        assert!(assembler.push_chunk(&chunks[1]).unwrap().is_none());
    }
}
