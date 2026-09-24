//! 사용자 디렉터리 경로를 제공한다. 시험에서는 임시 디렉터리를 사용한다.

use std::path::PathBuf;

#[allow(dead_code)] // 이유: 빌더와 어댑터는 있지만 제품 코드의 호출부는 없다
pub trait HomeDirectory: Send + Sync {
    fn home(&self) -> Option<PathBuf>;
    /// 설정 디렉터리.
    fn tasty_config(&self) -> Option<PathBuf>;
    /// 데이터 디렉터리. 경로는 어댑터가 정한다.
    fn tasty_data(&self) -> Option<PathBuf>;
    /// 캐시 디렉터리.
    fn tasty_cache(&self) -> Option<PathBuf>;
}
