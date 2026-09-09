//! View-local tour transitions. Geometry, input routing and persistence are adapters.
use super::catalog::{Requirement, all_topics};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ActiveTutorial {
    pub topic: usize,
    pub step: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PracticeContext {
    pub workspace: u32,
    pub pane: u32,
    pub tab: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PracticeEvent {
    SplitSurface {
        workspace: u32,
        pane: u32,
        tab: u32,
    },
    NewTab {
        workspace: u32,
        pane: u32,
        tab: u32,
    },
    SwitchTab {
        workspace: u32,
        pane: u32,
        from: u32,
        to: u32,
    },
    SplitPane {
        workspace: u32,
        original: u32,
        new_pane: u32,
    },
    OpenPalette,
}

#[derive(Clone, Debug, Default)]
pub struct Progress {
    pub completed: bool,
    pub started: bool,
    pub resume: usize,
    pub row_version: i64,
    pub dirty: bool,
}

pub struct TutorialRuntime {
    pub active: Option<ActiveTutorial>,
    pub pending_start: Option<usize>,
    pub popup_selected: usize,
    pub progress: Vec<Progress>,
    pub practice: Option<PracticeContext>,
    pub preparing: bool,
    pub keyboard_focus: bool,
    pub callout_rect: Option<egui::Rect>,
    pub save_error: bool,
    pub setup_error: bool,
    pub catalog_loaded: bool,
    fulfilled: Vec<bool>,
}

impl Default for TutorialRuntime {
    fn default() -> Self {
        Self {
            active: None,
            pending_start: None,
            popup_selected: 0,
            progress: vec![Progress::default(); all_topics().len()],
            practice: None,
            preparing: false,
            keyboard_focus: false,
            callout_rect: None,
            save_error: false,
            setup_error: false,
            catalog_loaded: false,
            fulfilled: Vec::new(),
        }
    }
}

impl TutorialRuntime {
    pub fn request_start(&mut self, topic: usize) {
        if topic < all_topics().len() {
            self.pending_start = Some(topic);
        }
    }

    pub fn start_pending(&mut self) {
        let Some(topic) = self.pending_start.take() else {
            return;
        };
        let def = &all_topics()[topic];
        // Runtime object IDs cannot be resumed after returning to the catalog.
        let step = if def.id == "layout-practice" {
            0
        } else {
            self.progress[topic].resume.min(def.steps.len() - 1)
        };
        self.active = Some(ActiveTutorial { topic, step });
        self.progress[topic].started = true;
        self.fulfilled = vec![false; def.steps.len()];
        self.practice = None;
        self.preparing = false;
        self.setup_error = false;
        self.keyboard_focus = true;
    }

    pub fn ready(&self) -> bool {
        self.active.is_some_and(|a| {
            all_topics()[a.topic].steps[a.step].requirement == Requirement::Read
                || self.fulfilled[a.step]
        })
    }

    /// True only when the final step completes. A disabled Next never advances.
    pub fn next(&mut self) -> bool {
        let Some(mut active) = self.active else {
            return false;
        };
        if !self.ready() {
            return false;
        }
        if active.step + 1 == all_topics()[active.topic].steps.len() {
            self.progress[active.topic].completed = true;
            self.interrupt();
            self.progress[active.topic].resume = 0;
            return true;
        }
        active.step += 1;
        self.progress[active.topic].resume = active.step;
        self.progress[active.topic].dirty = true;
        self.active = Some(active);
        false
    }

    pub fn back(&mut self) {
        if let Some(ref mut a) = self.active {
            a.step = a.step.saturating_sub(1);
            self.progress[a.topic].resume = a.step;
            self.progress[a.topic].dirty = true;
        }
    }

    pub fn interrupt(&mut self) {
        if let Some(a) = self.active.take() {
            self.popup_selected = a.topic;
            self.progress[a.topic].resume = a.step;
            self.progress[a.topic].dirty = true;
        }
        self.preparing = false;
        self.practice = None;
        self.keyboard_focus = false;
        self.callout_rect = None;
    }

    pub fn prepared(&mut self, context: PracticeContext) {
        if !self.preparing {
            return;
        }
        self.preparing = false;
        self.practice = Some(context);
        if let Some(a) = self.active {
            self.fulfilled[a.step] = true;
        }
    }

    pub fn observe(&mut self, event: PracticeEvent) {
        let Some(a) = self.active else {
            return;
        };
        let req = all_topics()[a.topic].steps[a.step].requirement;
        let matched = match (req, self.practice, event) {
            (Requirement::OpenPalette, _, PracticeEvent::OpenPalette) => true,
            (
                Requirement::SplitSurface,
                Some(c),
                PracticeEvent::SplitSurface {
                    workspace,
                    pane,
                    tab,
                },
            ) => (c.workspace, c.pane, c.tab) == (workspace, pane, tab),
            (
                Requirement::NewTab,
                Some(c),
                PracticeEvent::NewTab {
                    workspace,
                    pane,
                    tab,
                },
            ) => c.workspace == workspace && c.pane == pane && c.tab != tab,
            (
                Requirement::SwitchTab,
                Some(c),
                PracticeEvent::SwitchTab {
                    workspace,
                    pane,
                    from,
                    to,
                },
            ) => c.workspace == workspace && c.pane == pane && from != to && to == c.tab,
            (
                Requirement::SplitPane,
                Some(c),
                PracticeEvent::SplitPane {
                    workspace,
                    original,
                    ..
                },
            ) => c.workspace == workspace && c.pane == original,
            _ => false,
        };
        if matched {
            self.fulfilled[a.step] = true;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn completion_survives_replay_and_interruption() {
        let mut r = TutorialRuntime::default();
        r.request_start(0);
        r.start_pending();
        for _ in 0..all_topics()[0].steps.len() - 1 {
            assert!(!r.next());
        }
        assert!(r.next());
        assert!(r.progress[0].completed);
        assert!(r.active.is_none());
        r.request_start(0);
        r.start_pending();
        r.next();
        r.interrupt();
        assert!(r.progress[0].completed);
        assert_eq!(r.progress[0].resume, 1);
    }
    #[test]
    fn practice_needs_the_right_object_and_does_not_auto_advance() {
        let mut r = TutorialRuntime::default();
        r.request_start(1);
        r.start_pending();
        assert!(!r.next());
        assert_eq!(r.active.unwrap().step, 0);
        r.preparing = true;
        r.prepared(PracticeContext {
            workspace: 1,
            pane: 2,
            tab: 3,
        });
        r.next();
        r.observe(PracticeEvent::SplitSurface {
            workspace: 9,
            pane: 2,
            tab: 3,
        });
        assert!(!r.ready());
        r.observe(PracticeEvent::SplitSurface {
            workspace: 1,
            pane: 2,
            tab: 3,
        });
        assert!(r.ready());
        assert_eq!(r.active.unwrap().step, 1);
        r.next();
        assert!(!r.ready());
        r.back();
        assert!(r.ready());
    }
    #[test]
    fn interrupted_setup_cannot_reenter() {
        let mut r = TutorialRuntime::default();
        r.request_start(1);
        r.start_pending();
        r.preparing = true;
        r.interrupt();
        r.prepared(PracticeContext {
            workspace: 1,
            pane: 2,
            tab: 3,
        });
        assert!(r.active.is_none());
        assert!(r.practice.is_none());
    }
}
