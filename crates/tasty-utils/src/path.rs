//! Tasty 의 사용자 데이터 디렉토리 헬퍼.
//!
//! `tasty_home()` 이 기반 경로 (`~/.tasty/`) 를 반환한다. 도메인별 경로
//! (`themes_dir`, `memory_db_path`, `config_path` 등) 는 각 도메인 crate 가
//! 이 함수를 호출해 자체 정의한다 — utils 는 *공통 기반* 만 제공.

use std::path::PathBuf;

use directories::BaseDirs;

/// Tasty 의 사용자 데이터 디렉토리. 모든 플랫폼에서 `~/.tasty/`.
///
/// AI 에이전트가 경로를 외우기 쉽게 단일화 (Linux 규약상 `~/.config/tasty/` 가
/// 자연스럽지만, agent 접근성 우선).
pub fn tasty_home() -> Option<PathBuf> {
    BaseDirs::new().map(|dirs| dirs.home_dir().join(".tasty"))
}
