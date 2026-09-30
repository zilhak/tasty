//! 파일 처리기 선택 이력의 읽기·저장. 저장 실패는 로그만 남기고 화면 동작을 계속한다.

use super::RequestContext;

/// 사용자 처리기 선택 이력. 홈을 못 찾으면 공용 임시 경로에도 읽기·쓰기를 시도한다.
fn file_handler_recent_path() -> std::path::PathBuf {
    tasty_utils::path::tasty_home()
        .map(|d| d.join("file-handler-recent.json"))
        // 이유: 사용자 선택 이력의 공유 폴백이며 인스턴스별로 격리하지 않는다.
        .unwrap_or_else(|| std::env::temp_dir().join("tasty-file-handler-recent.json"))
}

pub(super) fn load() -> crate::file::handler::recent::RecentPicks {
    crate::file::handler::recent::RecentPicks::load(&file_handler_recent_path())
}

impl RequestContext {
    pub(crate) fn record_file_handler_pick(&mut self, id: &crate::file::handler::HandlerId) {
        self.file_handler_recent.record(id);
        let path = file_handler_recent_path();
        if let Err(e) = self.file_handler_recent.save_atomic(&path) {
            tracing::warn!(
                path = %path.display(),
                error = %e,
                "file_handler_recent: atomic save failed",
            );
        }
    }
}
