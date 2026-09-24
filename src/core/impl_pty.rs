//! PTY 출력을 읽고 터미널 이벤트·크기 변경을 처리한다.

use super::*;

/// workspace만 읽어 목표 grid를 모은다. 이후 Terminal store를 변경할 때 borrow가 겹치지 않게 한다.
#[cfg(feature = "gui")]
fn collect_terminal_resize_targets(
    tab_bar_h: crate::model::PhysicalPx,
    engine: &crate::core::CoreState,
    terminal_rect: crate::model::PhysicalRect,
    cell_width: f32,
    cell_height: f32,
    scale_factor: f32,
) -> Vec<(u32, usize, usize)> {
    let mut out = Vec::new();
    for ws in &engine.workspaces {
        let pane_rects = ws.pane_layout().compute_rects(terminal_rect, scale_factor);
        for (pane_id, pane_rect) in pane_rects {
            let Some(pane) = ws.pane_layout().find_pane(pane_id) else {
                continue;
            };
            let content_rect = crate::model::PhysicalRect {
                x: pane_rect.x,
                y: pane_rect.y + tab_bar_h,
                width: pane_rect.width,
                height: (pane_rect.height - tab_bar_h).max(crate::model::PhysicalPx(1.0)),
            };
            for tab in &pane.tabs {
                let Some(layout) = tab.layout_opt.as_ref() else {
                    continue;
                };
                for (sid, rect) in layout.compute_rects(content_rect, scale_factor) {
                    let cols = ((rect.width.value() / cell_width.max(1.0)).floor() as usize).max(1);
                    let rows =
                        ((rect.height.value() / cell_height.max(1.0)).floor() as usize).max(1);
                    out.push((sid, cols, rows));
                }
            }
        }
    }
    out
}

impl Core {
    /// 지정 터미널의 출력을 읽고 engine에 쌓인 터미널 이벤트를 처리한다. GUI·헤드리스가 함께 사용한다.
    pub(crate) fn process_pty_output(
        &mut self,
        engine: &mut crate::core::CoreState,
        surface_id: u32,
    ) -> ProcessPtyOutcome {
        engine.process_surface(surface_id);
        let events = self.drain_terminal_events(engine);
        ProcessPtyOutcome { events }
    }

    pub(crate) fn process_all_pty_output(
        &mut self,
        engine: &mut crate::core::CoreState,
    ) -> ProcessPtyOutcome {
        engine.process_all();
        let events = self.drain_terminal_events(engine);
        ProcessPtyOutcome { events }
    }

    /// observer·명령 이력·클립보드는 여기서 처리하고 App 후속 처리가 필요한 이벤트를 반환한다.
    fn drain_terminal_events(&mut self, engine: &mut crate::core::CoreState) -> Vec<CoreEvent> {
        use tasty_terminal::TerminalEventKind;
        let raw = engine.collect_events();
        let mut out = Vec::with_capacity(raw.len());
        for ev in raw {
            let sid = ev.surface_id;
            match ev.kind {
                TerminalEventKind::OutputAppended { text } => {
                    self.handle_output_appended(engine, sid, &text, &mut out);
                }
                TerminalEventKind::PromptBoundary { phase, payload } => {
                    self.handle_prompt_boundary(engine, sid, phase, &payload, &mut out);
                }
                TerminalEventKind::ClipboardSet(text) => {
                    if let Err(e) = self.clipboard.write_text(&text) {
                        tracing::warn!("OSC 52 clipboard write failed: {e}");
                    }
                    out.push(CoreEvent::TerminalClipboardSet { surface_id: sid });
                }
                TerminalEventKind::ClipboardQuery => {
                    self.handle_clipboard_query(engine, sid);
                }
                TerminalEventKind::Notification { title, body } => {
                    out.push(CoreEvent::TerminalNotification {
                        surface_id: sid,
                        title,
                        body,
                    });
                }
                TerminalEventKind::BellRing => {
                    out.push(CoreEvent::TerminalBellRing { surface_id: sid });
                }
                TerminalEventKind::TitleChanged(title) => {
                    out.push(CoreEvent::TerminalTitleChanged {
                        surface_id: sid,
                        title,
                    });
                }
                TerminalEventKind::CwdChanged(_cwd) => {
                    out.push(CoreEvent::TerminalCwdChanged { surface_id: sid });
                }
                TerminalEventKind::ProcessExited => {
                    out.push(CoreEvent::TerminalProcessExited { surface_id: sid });
                }
            }
        }
        out
    }

    fn handle_output_appended(
        &mut self,
        engine: &mut crate::core::CoreState,
        sid: u32,
        text: &str,
        out: &mut Vec<CoreEvent>,
    ) {
        let completed_lines = engine.observer_router.dispatch_text(sid, text);
        // 청크 경계와 무관하게 완성된 줄만 OutputMatch에 넘긴다.
        if engine.hook_manager.has_output_match_hook(sid) {
            for line in completed_lines {
                out.push(CoreEvent::TerminalOutputMatch {
                    surface_id: sid,
                    text: line,
                });
            }
        }
        // 첫 출력 이후 경계 보고가 없으면 셸 통합 안내를 요청한다. 이 경로는 출력이 올 때 실행된다.
        engine.note_first_output(sid);
        if engine.take_shell_integration_hint_due(sid) {
            out.push(CoreEvent::TerminalShellIntegrationHint { surface_id: sid });
        }
    }

    fn handle_prompt_boundary(
        &mut self,
        engine: &mut crate::core::CoreState,
        sid: u32,
        phase: char,
        payload: &str,
        out: &mut Vec<CoreEvent>,
    ) {
        engine.note_prompt_boundary_seen(sid);
        let mem = engine.memory.clone();
        if let Some(cap) = engine.command_index.on_boundary(&mem, sid, phase, payload) {
            use crate::core::command_index::CommandCapEvent;
            let (title, body) = match cap {
                CommandCapEvent::SoftWarn { count, .. } => (
                    crate::i18n::t("command_index.cap.soft.title").to_string(),
                    crate::i18n::t_fmt("command_index.cap.soft.body", &count.to_string()),
                ),
                CommandCapEvent::HardBlocked { .. } => (
                    crate::i18n::t("command_index.cap.hard.title").to_string(),
                    crate::i18n::t("command_index.cap.hard.body").to_string(),
                ),
            };
            out.push(CoreEvent::TerminalNotification {
                surface_id: sid,
                title,
                body,
            });
        }
        // 명령 이력 저장 성공 여부와 무관하게 D 경계는 완료 후속 처리로 넘긴다.
        if phase == 'D' {
            let exit_code = crate::core::command_index::extract_exit_code(payload);
            out.push(CoreEvent::TerminalCommandCompleted {
                surface_id: sid,
                exit_code,
            });
        }
    }

    /// resize 제한 주기에 따라 반영하고 미처리 요청이 남았는지 반환한다.
    #[cfg(feature = "gui")]
    pub(crate) fn flush_pty_resizes(engine: &mut crate::core::CoreState) -> bool {
        engine.flush_all_pty_resizes()
    }

    /// 레이아웃의 목표 크기를 Terminal store에 적용한다. 탭 바 높이는 창 계층이 값으로 넘긴다.
    #[cfg(feature = "gui")]
    pub(crate) fn resize_all_terminals(
        tab_bar_height: crate::model::PhysicalPx,
        engine: &mut crate::core::CoreState,
        terminal_rect: crate::model::PhysicalRect,
        cell_width: f32,
        cell_height: f32,
        scale_factor: f32,
    ) {
        let targets = collect_terminal_resize_targets(
            tab_bar_height,
            engine,
            terminal_rect,
            cell_width,
            cell_height,
            scale_factor,
        );
        for (sid, cols, rows) in targets {
            // 점유 client가 정한 크기를 서버 창 레이아웃으로 덮지 않는다.
            if engine.attach.is_hard_occupied(sid) {
                continue;
            }
            if let Some(t) = engine.terminals.get_mut(sid) {
                // mirror에 먼저 크기를 적용하면 서버의 reflow 전 출력과 어긋날 수 있다.
                // 서버에 resize를 요청하고 echo를 받아 로컬 크기를 바꾼다.
                if t.is_detached() {
                    if t.cols() != cols || t.rows() != rows {
                        engine.pending_resize_forward.insert(sid, (cols, rows));
                    }
                    continue;
                }
                t.resize(cols, rows);
            }
        }
    }

    /// busy 집합이 바뀌었는지 반환해 창의 다시 그리기 여부를 정한다.
    #[cfg(feature = "gui")]
    pub(crate) fn update_busy_surfaces(engine: &mut crate::core::CoreState) -> bool {
        engine.refresh_busy_surfaces()
    }
}
