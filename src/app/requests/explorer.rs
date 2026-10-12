//! 탐색기 패널을 바꾸는 실행 요청. 메뉴·주소 오류·새로고침·파일 열기·파일 작업은 화면 쪽이 처리하므로
//! 이 값으로 표현할 수 없다. 요청 출처는 이 값을 싣는 `Intent` 봉투가 보존한다.

use std::path::PathBuf;

use tasty_model::{ExplorerViewMode, SortColumn};

use crate::runtime::surface_binding::SurfaceBinding;

/// 활성 내부 탭 또는 내부 탭 목록에 적용할 변경. 내부 탭은 index로 고른다.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum ExplorerCommand {
    Navigate(PathBuf),
    Back,
    Forward,
    Up,
    ViewMode(ExplorerViewMode),
    /// 같은 컬럼이면 방향을 뒤집는다.
    Sort(SortColumn),
    NewTab,
    CloseTab(usize),
    SelectTab(usize),
}

/// 요청을 만들 때 잡은 surface 세대에 묶인 명령. 대상이 바뀌었으면 적용하지 않는다.
#[derive(Clone, Debug)]
pub(crate) struct BoundExplorerCommand {
    target: SurfaceBinding,
    command: ExplorerCommand,
}

impl BoundExplorerCommand {
    pub(crate) fn new(target: SurfaceBinding, command: ExplorerCommand) -> Self {
        Self { target, command }
    }
    pub(crate) fn target(&self) -> &SurfaceBinding {
        &self.target
    }
    pub(crate) fn command(&self) -> &ExplorerCommand {
        &self.command
    }
}

/// 실행 결과. 거절된 요청은 패널과 저장 dirty 표시를 바꾸지 않는다.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ExplorerCommandOutcome {
    Applied,
    /// surface가 닫혔거나 다른 세대로 바뀌었다.
    StaleTarget,
    /// 같은 세대의 surface지만 탐색기 패널이 아니다.
    NotExplorer,
}
