//! Per-window display positions. Terminal resources remain in Core's existing store.
use std::collections::HashMap;
use tasty_terminal::{ContentCut, TerminalViewport};

use crate::core::CoreState;

#[derive(Default)]
struct SurfaceViewports {
    local: TerminalViewport,
    readonly: TerminalViewport,
}

#[derive(Default)]
pub(crate) struct TerminalViewports(HashMap<u32, SurfaceViewports>);

impl TerminalViewports {
    pub fn get(&self, engine: &CoreState, surface_id: u32) -> TerminalViewport {
        self.0
            .get(&surface_id)
            .map_or(TerminalViewport::LIVE, |entry| {
                if engine.attach.is_hard_occupied(surface_id) {
                    entry.readonly
                } else {
                    entry.local
                }
            })
    }
    pub fn update(
        &mut self,
        engine: &CoreState,
        surface_id: u32,
        apply: impl FnOnce(&mut TerminalViewport, ContentCut),
    ) {
        let Some(terminal) = engine.visible_terminal(surface_id) else {
            return;
        };
        let entry = self.0.entry(surface_id).or_default();
        let viewport = if engine.attach.is_hard_occupied(surface_id) {
            &mut entry.readonly
        } else {
            &mut entry.local
        };
        let current = *viewport;
        terminal.with_view(&current, |view| apply(viewport, view.cut()));
    }
    pub fn remove(&mut self, surface_id: u32) {
        self.0.remove(&surface_id);
    }
    pub fn retain(&mut self, engine: &CoreState) {
        self.0.retain(|id, _| engine.has_surface(*id));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn independent_windows_keep_anchors_and_retire_surface_entries() {
        let (mut state, mut engine) = crate::state::tests::test_state();
        let sid = state.focused_surface_id(&engine).unwrap();
        let mut terminal = tasty_terminal::Terminal::new_detached(20, 3);
        terminal.feed_bytes(b"zero\r\none\r\ntwo\r\nthree\r\nfour");
        engine.runtime.terminals.insert(sid, terminal);
        let second_window = TerminalViewports::default();
        state
            .terminal_views
            .update(&engine, sid, |viewport, cut| viewport.scroll_up(cut, 1));
        engine
            .find_terminal_by_id_mut(sid)
            .unwrap()
            .feed_bytes(b"\r\nfive");
        let cut = engine.find_terminal_by_id(sid).unwrap().content_cut();
        assert_eq!(
            state.terminal_views.get(&engine, sid).resolve(cut).top_row,
            1
        );
        assert_eq!(
            second_window.get(&engine, sid).resolve(cut).scroll_offset(),
            0
        );
        state
            .terminal_views
            .0
            .insert(u32::MAX, SurfaceViewports::default());
        state.reconcile_presentation(&engine);
        assert!(!state.terminal_views.0.contains_key(&u32::MAX));
        state.release_surface_views(sid);
        assert!(state.terminal_views.0.is_empty());
    }
}
