//! memory.db 초기화 실패 뒤 사용하는 임시 저장소와 실패 원인.
//! 호스트는 앱을 계속 실행하며 db_pragmas.memory_db와 durable:false로 비영속 상태를 알린다.

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
    /// 사용자 안내의 memory_error.<이름>과 같은 원인 이름.
    /// 저장 오류와 공통인 원인은 StorageFailure::as_str과도 같다.
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
    /// 초기화 실패 뒤 기본 MemoryConfig로 임시 저장소를 연다. 호스트 quota 설정은 받지 않는다.
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

    /// 초기화 진단과 사용자 안내 키의 원인 이름을 대조한다.
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

    /// 초기화 실패로 만든 임시 저장소에만 원인과 오류 문구가 기록된다.
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
