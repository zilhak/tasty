//! Resolve View-dependent notification text only after the corresponding projection is published.
use crate::core::{CoreState, host_event::PendingHostEvent};

pub(super) enum Notification {
    Ready(PendingHostEvent),
    TabName { tab_id: u32, user_direct: bool },
}

impl Notification {
    pub(super) fn resolve(
        self,
        core: &CoreState,
        presentation: &dyn crate::model::StructurePresentation,
    ) -> Option<PendingHostEvent> {
        match self {
            Self::Ready(event) => Some(event),
            Self::TabName {
                tab_id,
                user_direct,
            } => {
                let pane = core.find_pane_by_id(core.find_pane_for_tab(tab_id)?)?;
                let tab = pane.tabs.iter().find(|tab| tab.id == tab_id)?;
                Some(PendingHostEvent::TabRenamed {
                    tab_id,
                    title: tab.display_name(presentation.surface_id(tab)),
                    user_direct,
                })
            }
        }
    }
}
