//! 사용자 파일 형식·처리기 설정을 다시 읽는다. picker의 GUI 결과 처리는 file::dispatch::picker_apply에 있다.

use std::path::PathBuf;

use crate::app::services::AppServices;

pub(crate) struct ReloadFileHandlersOutcome {
    pub(crate) path: PathBuf,
    pub(crate) exists: bool,
    /// 이번 reload에서 적용하지 않은 사용자 항목.
    pub(crate) rejected: Vec<tasty_file_handler::RejectedUserHandler>,
}

impl AppServices {
    pub(crate) fn reload_file_handlers(&self) -> Result<ReloadFileHandlersOutcome, String> {
        crate::app::file_consumers::poll(&self.registries);
        let rules = self
            .registries
            .file_rules
            .as_ref()
            .ok_or("file handler path is unavailable")?;
        let rejected = rules.reload(&self.registries.file_format, &self.registries.file_handler)?;
        let path = rules.path().to_owned();
        let exists = path.exists();
        Ok(ReloadFileHandlersOutcome {
            path,
            exists,
            rejected,
        })
    }
}
