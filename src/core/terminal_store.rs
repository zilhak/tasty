//! Surface 트리의 ID와 별도로 Terminal 인스턴스와 scrollback 저장 ID를 보관한다.
//! busy 상태·대기 scrollback·deferred 생성 정보는 각 수명에 맞는 다른 상태에 둔다.

use std::collections::HashMap;

use tasty_terminal::{ColorPalette, Terminal, TerminalRgb};
use tasty_type_appearance::color::HexColor;

use crate::model::SurfaceId;

fn rgb(c: HexColor) -> TerminalRgb {
    TerminalRgb::new(c.r, c.g, c.b)
}

/// OSC 색 조회용 팔레트. terminal의 focused 색과 ANSI 팔레트를 사용한다.
/// unfocused 화면의 효과까지 반영한 현재 픽셀값은 아니다.
fn current_terminal_palette() -> ColorPalette {
    let theme = crate::theme::theme();
    let surface = theme.surface("terminal");
    let fg = rgb(surface.focused_fg);
    let bg = rgb(surface.focused_bg);
    let ansi = [
        rgb(theme.ansi_black),
        rgb(theme.ansi_red),
        rgb(theme.ansi_green),
        rgb(theme.ansi_yellow),
        rgb(theme.ansi_blue),
        rgb(theme.ansi_magenta),
        rgb(theme.ansi_cyan),
        rgb(theme.ansi_white),
        rgb(theme.ansi_bright_black),
        rgb(theme.ansi_bright_red),
        rgb(theme.ansi_bright_green),
        rgb(theme.ansi_bright_yellow),
        rgb(theme.ansi_bright_blue),
        rgb(theme.ansi_bright_magenta),
        rgb(theme.ansi_bright_cyan),
        rgb(theme.ansi_bright_white),
    ];
    ColorPalette {
        foreground: fg,
        background: bg,
        cursor: fg,
        ansi,
    }
}

#[derive(Default)]
pub(crate) struct TerminalStore {
    terminals: HashMap<SurfaceId, Terminal>,

    /// Terminal 등록과 별도로 설정하는 디스크 scrollback 저장 ID.
    scrollback_persist_ids: HashMap<SurfaceId, String>,
}

impl TerminalStore {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// 기존 ID는 덮어쓰며 기존 Terminal을 drop한다. 자식 종료 처리는 Terminal이 맡는다.
    pub(crate) fn insert(&mut self, id: SurfaceId, mut terminal: Terminal) {
        terminal.set_color_palette(current_terminal_palette());
        self.terminals.insert(id, terminal);
    }

    /// Terminal을 꺼내 소유권을 반환하고 저장 ID를 지운다. 반환값을 drop할 시점은 호출자가 정한다.
    pub(crate) fn remove(&mut self, id: SurfaceId) -> Option<Terminal> {
        self.scrollback_persist_ids.remove(&id);
        self.terminals.remove(&id)
    }

    /// 새 Terminal을 넣고 기존 값을 반환한다. 없던 ID도 등록되며 None을 반환한다.
    /// scrollback 저장 ID와 레이아웃 트리는 그대로 둔다.
    pub(crate) fn replace(
        &mut self,
        id: SurfaceId,
        mut new_terminal: Terminal,
    ) -> Option<Terminal> {
        new_terminal.set_color_palette(current_terminal_palette());
        self.terminals.insert(id, new_terminal)
    }

    #[cfg(feature = "gui")]
    pub(crate) fn resync_palettes(&mut self) {
        let palette = current_terminal_palette();
        for t in self.terminals.values_mut() {
            t.set_color_palette(palette.clone());
        }
    }

    pub(crate) fn get(&self, id: SurfaceId) -> Option<&Terminal> {
        self.terminals.get(&id)
    }

    pub(crate) fn get_mut(&mut self, id: SurfaceId) -> Option<&mut Terminal> {
        self.terminals.get_mut(&id)
    }

    pub(crate) fn contains(&self, id: SurfaceId) -> bool {
        self.terminals.contains_key(&id)
    }

    pub(crate) fn iter(&self) -> impl Iterator<Item = (SurfaceId, &Terminal)> {
        self.terminals.iter().map(|(&id, t)| (id, t))
    }

    pub(crate) fn iter_mut(&mut self) -> impl Iterator<Item = (SurfaceId, &mut Terminal)> {
        self.terminals.iter_mut().map(|(&id, t)| (id, t))
    }

    pub(crate) fn scrollback_persist_id(&self, id: SurfaceId) -> Option<&str> {
        self.scrollback_persist_ids.get(&id).map(String::as_str)
    }

    pub(crate) fn set_scrollback_persist_id(&mut self, id: SurfaceId, persist_id: String) {
        self.scrollback_persist_ids.insert(id, persist_id);
    }

    /// 각 Terminal의 process 결과 중 하나라도 true이면 true다.
    pub(crate) fn process_all(&mut self) -> bool {
        let mut any = false;
        for t in self.terminals.values_mut() {
            if t.process() {
                any = true;
            }
        }
        any
    }

    pub(crate) fn process_surface(&mut self, id: SurfaceId) -> bool {
        if let Some(t) = self.terminals.get_mut(&id) {
            t.process()
        } else {
            false
        }
    }

    /// resize를 시도한 뒤에도 대기 중인 항목이 하나라도 있으면 true다.
    #[cfg(feature = "gui")]
    pub(crate) fn flush_pty_resizes(&mut self) -> bool {
        let mut any_pending = false;
        for t in self.terminals.values_mut() {
            t.flush_pty_resize();
            if t.has_pending_pty_resize() {
                any_pending = true;
            }
        }
        any_pending
    }
}
