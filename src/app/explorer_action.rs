//! App applies builtin kind changes after the readonly drawing phase.
pub(super) fn apply_to_explorer_panel(
    ex: &mut crate::model::ExplorerPanel,
    act: &crate::explorer_ui::ExplorerAction,
) {
    use crate::explorer_ui::ExplorerAction as A;
    match act {
        A::Navigate(p) => {
            ex.active_tab_mut().navigate_to(p.clone());
        }
        A::GoBack => {
            ex.active_tab_mut().go_back();
        }
        A::GoForward => {
            ex.active_tab_mut().go_forward();
        }
        A::GoUp => {
            ex.active_tab_mut().go_up();
        }
        A::SetViewMode(m) => {
            ex.active_tab_mut().view_mode = *m;
        }
        A::SetSort(col) => {
            let tab = ex.active_tab_mut();
            if tab.sort_column == *col {
                tab.sort_dir = tab.sort_dir.toggled();
            } else {
                tab.sort_column = *col;
                tab.sort_dir = crate::model::SortDir::Asc;
            }
        }
        A::NewTab => ex.add_tab(),
        A::CloseTab(i) => ex.close_tab(*i),
        A::SelectTab(i) => {
            if *i < ex.tabs.len() {
                ex.active = *i;
            }
        }
        A::OpenFile(_)
        | A::Refresh
        | A::ContextMenu { .. }
        | A::AddressRejected(_)
        | A::Create { .. }
        | A::MoreMenu { .. } => {}
    }
}
