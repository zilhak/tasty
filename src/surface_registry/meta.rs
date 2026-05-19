//! Per-surface metadata store.
//!
//! `surface.meta.*` IPC + 내부 호출자는 모두 이 facade 를 거쳐 `tasty-memory` 의
//! `Scope::Surface(id)` 위 `MemoryValue::Text` entry 로 저장된다. 모든 쓰기는
//! `HOST_OWNER` 로 수행되므로 plugin 이 `surface.meta.set` 으로 쓴 키도 host 가
//! 소유한다 (호환성 보존: 기존 surface.meta API 는 owner 가 없었다).
//!
//! 반환 타입은 `io::Result<()>` / `Option<String>` 그대로 유지해 기존 호출자가
//! 컴파일 변경 없이 그대로 동작한다.

use std::collections::HashMap;
use std::io;

use tasty_memory::{HOST_OWNER, MemoryError, MemoryValue, PutOpts, Scope, with_store};

fn memory_err_to_io(e: MemoryError) -> io::Error {
    io::Error::other(format!("memory: {e}"))
}

fn store_unavailable() -> io::Error {
    io::Error::other("memory store not initialised")
}

/// File-based per-surface metadata store (now forwarding to `tasty-memory`).
pub struct SurfaceMetaStore;

impl SurfaceMetaStore {
    /// Surface 생성 시 호출. memory.db 는 scope 사전 생성 개념이 없으므로 no-op.
    pub fn ensure_created(_surface_id: u32) -> io::Result<()> {
        Ok(())
    }

    /// Surface 닫힘 시 해당 스코프의 모든 key (regular+secret) 삭제.
    pub fn remove(surface_id: u32) -> io::Result<()> {
        let result = with_store(|s| s.purge_scope(&Scope::Surface(surface_id)))
            .ok_or_else(store_unavailable)?;
        result.map(|_| ()).map_err(memory_err_to_io)
    }

    /// 키 set. 값은 `text/plain` UTF-8 문자열로 저장된다.
    pub fn set(surface_id: u32, key: &str, value: &str) -> io::Result<()> {
        let result = with_store(|s| {
            s.put(
                HOST_OWNER,
                &Scope::Surface(surface_id),
                key,
                &MemoryValue::Text(value.to_string()),
                &PutOpts::default(),
            )
        })
        .ok_or_else(store_unavailable)?;
        result.map(|_| ()).map_err(memory_err_to_io)
    }

    /// 키 get. 만료/없음/비문자열 값은 `None`.
    pub fn get(surface_id: u32, key: &str) -> Option<String> {
        let entry = with_store(|s| s.get(&Scope::Surface(surface_id), key))?.ok().flatten()?;
        match entry.value {
            MemoryValue::Text(s) => Some(s),
            MemoryValue::Json(v) => {
                tracing::warn!(
                    "surface.meta.get: key '{key}' on surface {surface_id} holds JSON value; \
                     returning JSON stringified form for back-compat"
                );
                serde_json::to_string(&v).ok()
            }
            MemoryValue::Binary(_) => {
                tracing::warn!(
                    "surface.meta.get: key '{key}' on surface {surface_id} holds binary value; \
                     returning None"
                );
                None
            }
        }
    }

    /// 키 unset. 기존 파일 기반 구현은 키가 없어도 silently OK 였으므로 NotFound 무시.
    pub fn unset(surface_id: u32, key: &str) -> io::Result<()> {
        let result = with_store(|s| {
            s.delete(HOST_OWNER, &Scope::Surface(surface_id), key, None)
        })
        .ok_or_else(store_unavailable)?;
        match result {
            Ok(()) => Ok(()),
            Err(MemoryError::NotFound { .. }) => Ok(()),
            Err(e) => Err(memory_err_to_io(e)),
        }
    }

    /// 키 list. 문자열로 변환 가능한 값만 반환.
    pub fn list(surface_id: u32) -> HashMap<String, String> {
        let entries = match with_store(|s| {
            s.list(&Scope::Surface(surface_id), &tasty_memory::ListOpts::default())
        }) {
            Some(Ok(v)) => v,
            Some(Err(e)) => {
                tracing::warn!("surface.meta.list({surface_id}) failed: {e}");
                return HashMap::new();
            }
            None => return HashMap::new(),
        };
        let mut out = HashMap::with_capacity(entries.len());
        for entry in entries {
            match entry.value {
                MemoryValue::Text(s) => {
                    out.insert(entry.key, s);
                }
                MemoryValue::Json(v) => {
                    if let Ok(s) = serde_json::to_string(&v) {
                        out.insert(entry.key, s);
                    }
                }
                MemoryValue::Binary(_) => {
                    // surface.meta is string-only; binary values are skipped silently.
                }
            }
        }
        out
    }

    /// `Surface(*)` 스코프 전체를 훑어 `key=value` 인 첫 surface id 반환.
    /// 닉네임 기반 pane 조회용. 정렬 보장 없음 (memory 의 scopes() 순서를 그대로 사용).
    pub fn find_by_value(key: &str, value: &str) -> Option<u32> {
        let scopes = with_store(|s| s.scopes())?.ok()?;
        for token in scopes {
            let Ok(Scope::Surface(sid)) = Scope::parse(&token) else {
                continue;
            };
            if Self::get(sid, key).as_deref() == Some(value) {
                return Some(sid);
            }
        }
        None
    }
}
