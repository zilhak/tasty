//! Terminal title observations never mutate the committed structure.
use super::*;
impl AppServices {
    #[cfg(any(feature="gui",test))]
    pub(super) fn apply_update_tab_name(engine:&mut EngineMut<'_>,surface:u32,name:String)->CoreEvent {
        let skipped_explicit=engine.core.find_tab_for_surface(surface).and_then(|tab|engine.core.find_pane_for_tab(tab).and_then(|pane|engine.core.find_pane_by_id(pane)).and_then(|pane|pane.tabs.iter().find(|candidate|candidate.id==tab))).is_some_and(|tab|tab.explicit_name.is_some());
        if !name.trim().is_empty() && engine.core.has_surface(surface) {engine.live.surface_titles.entry(surface).or_default().osc_title=Some(name);}
        CoreEvent::TabNameUpdated {skipped_explicit}
    }
}
