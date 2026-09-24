//! 캐시를 만들거나 저장한 시점의 레이아웃. 저장 직전 충돌 검사와 보기 모드 갱신에 사용한다.
//! 저장은 레이아웃만 교체하므로 이름·부제 변경은 충돌로 보지 않는다.
//! 배경: docs/adr/0038-preset-drafts-and-store-conflicts.md.

use tasty_presets::{PresetKind, PresetPane, PresetPaneNode, PresetStore, PresetSurfaceLayout};

/// persist_layout이 교체하는 레이아웃의 기준값.
#[derive(Debug, Clone, PartialEq)]
pub(super) enum LayoutBase {
    /// `WorkspacePreset::layout`.
    Workspace(PresetPaneNode),
    /// `TabPreset::tab.layout`.
    Tab(PresetSurfaceLayout),
    /// `PanePreset::pane`.
    Pane(PresetPane),
}

impl LayoutBase {
    /// 현재 저장소 값. 프리셋이 없으면 None이다.
    pub(super) fn current(store: &PresetStore, kind: PresetKind, name: &str) -> Option<Self> {
        match kind {
            PresetKind::Workspace => store
                .get_workspace(name)
                .map(|p| Self::Workspace(p.layout.clone())),
            PresetKind::Tab => store.get_tab(name).map(|p| Self::Tab(p.tab.layout.clone())),
            PresetKind::Pane => store.get_pane(name).map(|p| Self::Pane(p.pane.clone())),
        }
    }
}
