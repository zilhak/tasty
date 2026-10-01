//! Surface 트리의 ID와 별도로 Terminal 인스턴스와 scrollback 저장 ID를 보관한다.
//! busy 상태·대기 scrollback·deferred 생성 정보는 각 수명에 맞는 다른 상태에 둔다.

use std::collections::HashMap;

use std::sync::{Arc, atomic::AtomicU32};
use tasty_terminal::{ColorPalette, Pty, ResourceGeneration, Terminal, TerminalRgb};
use tasty_type_appearance::color::HexColor;

use crate::model::SurfaceId;

fn rgb(c: HexColor) -> TerminalRgb {
    TerminalRgb::new(c.r, c.g, c.b)
}

/// OSC 색 조회용 팔레트. terminal의 focused 색과 ANSI 팔레트를 사용한다.
/// unfocused 화면의 효과까지 반영한 현재 픽셀값은 아니다.
pub(crate) fn current_terminal_palette() -> ColorPalette {
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

pub(crate) struct TerminalStore {
    terminals: HashMap<SurfaceId, (Terminal, Option<Pty>)>,
    next_pty_id: Arc<AtomicU32>,
    max_standalone: usize,
    idle_ttl: std::time::Duration,

    /// Terminal 등록과 별도로 설정하는 디스크 scrollback 저장 ID.
    scrollback_persist_ids: HashMap<SurfaceId, String>,
}

impl TerminalStore {
    pub(crate) fn new(next_pty_id: Arc<AtomicU32>) -> Self {
        Self {
            terminals: HashMap::new(),
            scrollback_persist_ids: HashMap::new(),
            next_pty_id,
            max_standalone: DEFAULT_MAX_CONCURRENT,
            idle_ttl: DEFAULT_IDLE_TTL,
        }
    }

    /// Each map entry is the only owner of its content and optional physical connection.
    pub(crate) fn insert(&mut self, id: SurfaceId, mut terminal: Terminal, pty: Option<Pty>) {
        if let Some(pty) = &pty {
            assert_eq!(terminal.resource_generation(), pty.generation());
        }
        terminal.set_color_palette(current_terminal_palette());
        if let Some((mut old, _pty)) = self.terminals.insert(id, (terminal, pty)) {
            old.disconnect();
        }
    }

    /// Revoke callbacks before returning old content and its physical owner for disposal.
    pub(crate) fn remove(&mut self, id: SurfaceId) -> Option<(Terminal, Option<Pty>)> {
        self.scrollback_persist_ids.remove(&id);
        let (mut terminal, pty) = self.terminals.remove(&id)?;
        terminal.disconnect();
        Some((terminal, pty))
    }

    pub(crate) fn replace(
        &mut self,
        id: SurfaceId,
        mut terminal: Terminal,
        pty: Option<Pty>,
    ) -> Option<(Terminal, Option<Pty>)> {
        if let Some(pty) = &pty {
            assert_eq!(terminal.resource_generation(), pty.generation());
        }
        terminal.set_color_palette(current_terminal_palette());
        self.terminals
            .insert(id, (terminal, pty))
            .map(|(mut old, pty)| {
                old.disconnect();
                (old, pty)
            })
    }

    /// Transfer to a committed adoption operation without revoking its content/I/O lease.
    pub(crate) fn take_standalone_for_adoption(
        &mut self,
        id: u32,
        generation: u64,
    ) -> Option<(Terminal, Pty, Option<String>)> {
        let owner = self.standalone(id)?;
        if owner.generation().value() != generation || owner.state().exit().is_some() {
            return None;
        }
        let (terminal, pty) = self.terminals.remove(&id)?;
        Some((terminal, pty?, self.scrollback_persist_ids.remove(&id)))
    }
    pub(crate) fn restore_standalone_adoption(
        &mut self,
        id: u32,
        terminal: Terminal,
        pty: Pty,
        persist_id: Option<String>,
    ) -> Result<(), (Terminal, Pty, Option<String>)> {
        if self.terminals.contains_key(&id) || pty.state().standalone().is_none() {
            return Err((terminal, pty, persist_id));
        }
        self.terminals.insert(id, (terminal, Some(pty)));
        if let Some(persist_id) = persist_id {
            self.scrollback_persist_ids.insert(id, persist_id);
        }
        Ok(())
    }

    pub(crate) fn pty(&self, id: u32) -> Option<&Pty> {
        self.terminals.get(&id)?.1.as_ref()
    }
    pub(crate) fn pty_mut(&mut self, id: u32) -> Option<&mut Pty> {
        self.terminals.get_mut(&id)?.1.as_mut()
    }
    pub(crate) fn generation(&self, id: u32) -> Option<ResourceGeneration> {
        self.get(id).map(Terminal::resource_generation)
    }
    pub(crate) fn matches_generation(&self, id: u32, generation: ResourceGeneration) -> bool {
        self.generation(id) == Some(generation)
    }
    pub(crate) fn cwd(&self, id: u32) -> Option<std::path::PathBuf> {
        self.get(id)?
            .cached_cwd()
            .or_else(|| tasty_terminal::cwd::get_cwd_of_pid(self.pty(id)?.process_id()?))
    }
    pub(crate) fn resize(&mut self, id: u32, cols: usize, rows: usize) -> bool {
        let Some((terminal, pty)) = self.terminals.get_mut(&id) else {
            return false;
        };
        if !terminal.resize(cols, rows) {
            return false;
        }
        if let Some(pty) = pty {
            pty.schedule_resize(terminal.resource_generation(), cols, rows);
        }
        true
    }

    #[cfg(feature = "gui")]
    pub(crate) fn resync_palettes(&mut self) {
        let palette = current_terminal_palette();
        for (t, _) in self.terminals.values_mut() {
            t.set_color_palette(palette.clone());
        }
    }

    pub(crate) fn get(&self, id: SurfaceId) -> Option<&Terminal> {
        self.terminals.get(&id).map(|(terminal, _)| terminal)
    }

    pub(crate) fn get_mut(&mut self, id: SurfaceId) -> Option<&mut Terminal> {
        self.terminals.get_mut(&id).map(|(terminal, _)| terminal)
    }

    pub(crate) fn contains(&self, id: SurfaceId) -> bool {
        self.terminals.contains_key(&id)
    }

    pub(crate) fn iter(&self) -> impl Iterator<Item = (SurfaceId, &Terminal)> {
        self.terminals.iter().map(|(&id, (t, _))| (id, t))
    }

    pub(crate) fn iter_mut(&mut self) -> impl Iterator<Item = (SurfaceId, &mut Terminal)> {
        self.terminals.iter_mut().map(|(&id, (t, _))| (id, t))
    }

    pub(crate) fn scrollback_persist_id(&self, id: SurfaceId) -> Option<&str> {
        self.scrollback_persist_ids.get(&id).map(String::as_str)
    }

    pub(crate) fn set_scrollback_persist_id(&mut self, id: SurfaceId, persist_id: String) {
        self.scrollback_persist_ids.insert(id, persist_id);
    }

    pub(crate) fn process_all(&mut self) -> bool {
        self.terminals
            .values_mut()
            .fold(false, |changed, (terminal, pty)| {
                Self::process_pair(terminal, pty.as_mut()) || changed
            })
    }
    pub(crate) fn process_surface(&mut self, id: SurfaceId) -> bool {
        self.terminals
            .get_mut(&id)
            .is_some_and(|(terminal, pty)| Self::process_pair(terminal, pty.as_mut()))
    }
    fn process_pair(terminal: &mut Terminal, pty: Option<&mut Pty>) -> bool {
        if let Some(pty) = pty {
            pty.force_flush_resize();
            if pty.observe_exit() && pty.state().standalone().is_none() {
                terminal.record_exit(pty.generation());
            }
        }
        terminal.process()
    }
    #[cfg(feature = "gui")]
    pub(crate) fn flush_pty_resizes(&mut self) -> bool {
        let mut pending = false;
        for (_, pty) in self.terminals.values_mut() {
            if let Some(pty) = pty {
                pty.flush_resize();
                pending |= pty.has_pending_resize();
            }
        }
        pending
    }
}

/// 터미널에서 닫은 항목 snapshot에 넣을 값을 읽는다. 줄마다 terminal mutex를 잠그지
/// 않도록 스크롤백은 한 번에 읽는다. 스크롤백은 디스크 저장 형식으로 인코딩해 넘기므로
/// 닫기 뒤 저장은 이 바이트를 그대로 쓴다.
#[cfg(test)]
pub(crate) fn closed_capture_of(
    terminal: &Terminal,
    cwd: Option<std::path::PathBuf>,
) -> crate::model::closed_item::TerminalCapture {
    let lines = terminal.scrollback_lines_all();
    crate::model::closed_item::TerminalCapture {
        cwd,
        scrollback: crate::model::closed_item::ScrollbackBlob {
            bytes: tasty_terminal::disk_scrollback::serialize_lines(&lines),
            lines: lines.len(),
        },
    }
}

mod standalone;
pub(crate) use standalone::{
    DEFAULT_IDLE_TTL, DEFAULT_MAX_CONCURRENT, PTY_ID_BASE, is_surface_id_space,
};
