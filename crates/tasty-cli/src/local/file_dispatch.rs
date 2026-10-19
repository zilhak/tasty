//! `file-handler dispatch`: 접수 뒤 `run.result`로 최종 결과를 기다린다.
//! 출력·종료 코드·대기 상한 처리는 [`super::run_wait`]를 따른다.

use std::time::Duration;

use anyhow::Result;

use crate::Commands;
use crate::dispatch::{ClientCommand, ClientCtx};

/// 파일 열기는 식별·탭 생성·플러그인 응답을 기다린다. 플러그인 IPC 응답에 상한을 두지 않으므로
/// CLI가 이 시간까지만 기다린다.
const FILE_WAIT: Duration = Duration::from_secs(30);

pub struct FileDispatch<'a> {
    pub command: &'a Commands,
}

impl ClientCommand for FileDispatch<'_> {
    fn run(self: Box<Self>, ctx: &ClientCtx<'_>) -> Result<()> {
        let request = crate::request::command_to_request(self.command);
        super::run_wait::dispatch_and_wait(ctx.port_file, &request, "run.result", |_| FILE_WAIT)
    }
}
