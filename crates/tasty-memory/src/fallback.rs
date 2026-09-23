//! `memory.db` 를 못 열었을 때의 **in-memory 대체 저장소**와 그 까닭.
//!
//! 호스트는 부팅 때 `memory.db` 초기화가 실패해도 종료하지 않고 in-memory 저장소로
//! 계속 뜬다 — 손상된 파일로도 앱을 쓸 수 있게 하는 기존 동작이다. 문제는 그 상태가
//! **조용했다**는 것이다: 쓰기가 정상과 똑같은 `ok` 로 확인되고 재시작에 사라졌다.
//! 그래서 대체 저장소는 자기가 대체라는 사실과 원인을 들고 태어난다([`InitFallback`]).
//! 호스트는 그것을 진단 응답(`db_pragmas.memory_db`)과 쓰기 응답(`durable: false`)으로
//! 내보낸다. 근거·대안·재검토 조건은
//! `docs/design/systems/storage.md#memorydb--in-memory-대체로-계속-뜨고-degraded-로-말한다`.

use crate::{MemoryInitError, MemoryStore};

/// 이 저장소가 `memory.db` 대신 in-memory 로 열린 까닭.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InitFallback {
    /// 초기화 실패의 원인 갈래 — [`MemoryInitError::cause`] 의 이름이다.
    pub cause: &'static str,
    /// 초기화 오류 문구(로그의 `memory.db init at boot failed: …` 와 같은 값).
    pub error: String,
}

impl MemoryInitError {
    /// 밖으로 나가는 원인 이름. variant 와 1:1 이고, 사용자 안내의 i18n 키
    /// (`memory_error.<이름>`) 끝토막과 같다 — 둘이 갈리지 않는 것은 시험이 잰다.
    ///
    /// 저장 경로와 겹치는 갈래(`busy` · `disk_full` · `corrupt` · `permission_denied` ·
    /// `other`)는 [`crate::StorageFailure::as_str`] 과 같은 이름이다.
    pub fn cause(&self) -> &'static str {
        match self {
            MemoryInitError::HomeDirMissing => "home_missing",
            MemoryInitError::PermissionDenied(_) => "permission_denied",
            MemoryInitError::Busy(_) => "busy",
            MemoryInitError::DiskFull => "disk_full",
            MemoryInitError::Corrupt(_) => "corrupt",
            MemoryInitError::SchemaMismatch { .. } => "schema_mismatch",
            MemoryInitError::Other(_) => "other",
        }
    }
}

impl MemoryStore {
    /// `memory.db` 초기화가 `err` 로 실패한 뒤 쓸 in-memory 저장소를 연다.
    ///
    /// config 는 기본값이다 — 이 대체가 생긴 뒤로 줄곧 그랬고(호스트 설정의 quota 를
    /// 안 받는다), 이 함수는 그 동작을 바꾸지 않는다. 바뀐 것은 저장소가 자기가
    /// 대체라는 사실을 [`init_fallback`](Self::init_fallback) 으로 말한다는 것뿐이다.
    pub fn open_in_memory_after_init_failure(
        err: &MemoryInitError,
    ) -> std::result::Result<Self, MemoryInitError> {
        let mut store = Self::open_in_memory()?;
        store.init_fallback = Some(InitFallback {
            cause: err.cause(),
            error: err.to_string(),
        });
        Ok(store)
    }

    /// 이 저장소가 `memory.db` 초기화 실패의 대체인가. 대체면 쓰기는 프로세스와 함께
    /// 사라진다(durable 이 아니다). 파일 DB 와 시험용 in-memory 저장소는 `None`.
    pub fn init_fallback(&self) -> Option<&InitFallback> {
        self.init_fallback.as_ref()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn every_variant() -> Vec<MemoryInitError> {
        let p = || PathBuf::from("/x/memory.db");
        vec![
            MemoryInitError::HomeDirMissing,
            MemoryInitError::PermissionDenied(p()),
            MemoryInitError::Busy(p()),
            MemoryInitError::DiskFull,
            MemoryInitError::Corrupt(p()),
            MemoryInitError::SchemaMismatch {
                expected: 2,
                found: 1,
            },
            MemoryInitError::Other("boom".into()),
        ]
    }

    /// 원인 이름이 사용자 안내 키의 끝토막과 같다 — 한쪽만 바뀌면 진단 응답과 안내가
    /// 서로 다른 이름으로 같은 원인을 부른다.
    #[test]
    fn the_cause_name_is_the_suffix_of_the_user_message_key() {
        for err in every_variant() {
            let (key, _) = err.user_message_i18n();
            assert_eq!(
                key.strip_prefix("memory_error."),
                Some(err.cause()),
                "{err:?}"
            );
        }
    }

    /// 대체 저장소는 원인과 오류 문구를 들고 태어나고, 보통 in-memory 저장소는 안 든다.
    #[test]
    fn a_fallback_store_carries_its_cause_and_a_plain_one_does_not() {
        let err = MemoryInitError::Corrupt(PathBuf::from("/x/memory.db"));
        let store = MemoryStore::open_in_memory_after_init_failure(&err).expect("open");
        assert_eq!(
            store.init_fallback(),
            Some(&InitFallback {
                cause: "corrupt",
                error: "memory.db corrupted: /x/memory.db".into(),
            })
        );
        assert!(store.applied_pragmas().in_memory);

        let plain = MemoryStore::open_in_memory().expect("open");
        assert_eq!(plain.init_fallback(), None);
    }
}
