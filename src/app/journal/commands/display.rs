//! Volatile mirror annotations follow the durable result, but never enter the local domain tree.
use super::*;
use std::sync::Weak;

pub(super) struct DisplayContinuation {
    pub engine: EngineId,
    pub mirrors: Vec<(u32, Weak<()>)>,
    pub action: DisplayAction,
}

pub(super) enum DisplayAction {
    Workspace {
        id: u32,
        name: Option<String>,
        subtitle: Option<String>,
        description: Option<String>,
        category: Option<u32>,
        mapping: Option<Option<crate::model::WorkspaceAttachMapping>>,
    },
    Order(Vec<u32>),
}

impl DisplayContinuation {
    pub fn apply(
        self,
        sessions: &mut [&mut EngineSession],
    ) -> Option<(EngineId, Option<crate::core::host_event::PendingHostEvent>)> {
        let session = sessions
            .iter_mut()
            .find(|session| session.id == self.engine)?;
        let core = &mut session.core_state;
        // Delta and reconnect both replace this token even when all numeric IDs survive.
        if !self
            .mirrors
            .iter()
            .all(|(id, token)| core.matches_mirror_projection(*id, token))
        {
            return None;
        }
        let mut host_event = None;
        match self.action {
            DisplayAction::Workspace {
                id,
                name,
                subtitle,
                description,
                category,
                mapping,
            } => {
                let category = category.filter(|id| core.category_index(*id).is_some());
                if !core.has_workspace(id) {
                    return None;
                }
                if name.is_some() || subtitle.is_some() || description.is_some() {
                    host_event = Some(
                        crate::core::host_event::PendingHostEvent::WorkspaceRenamed {
                            workspace_id: id,
                            name: name.clone(),
                            subtitle: subtitle.clone(),
                            description: description.clone(),
                            user_direct: false,
                        },
                    );
                }
                // Local metadata already came from the committed projection. Only the remote
                // display annotation has a volatile write at this boundary.
                if let Some(workspace) = core
                    .mirror_workspaces
                    .iter_mut()
                    .find(|workspace| workspace.id == id)
                {
                    if let Some(name) = name {
                        workspace.name = name;
                    }
                    if let Some(subtitle) = subtitle {
                        workspace.subtitle = subtitle;
                    }
                    if let Some(description) = description {
                        workspace.description = description;
                    }
                    if let Some(category) = category {
                        workspace.category = category;
                    }
                    if let Some(mapping) = mapping {
                        workspace.attach_mapping = mapping;
                    }
                }
            }
            DisplayAction::Order(order) => {
                if !core.apply_workspace_display_order(order) {
                    return None;
                }
            }
        }
        Some((self.engine, host_event))
    }

    pub fn weight(&self) -> usize {
        self.mirrors.len() * std::mem::size_of::<(u32, Weak<()>)>()
            + match &self.action {
                DisplayAction::Order(order) => order.len() * std::mem::size_of::<u32>(),
                DisplayAction::Workspace {
                    name,
                    subtitle,
                    description,
                    mapping,
                    ..
                } => {
                    name.as_ref().map_or(0, String::len)
                        + subtitle.as_ref().map_or(0, String::len)
                        + description.as_ref().map_or(0, String::len)
                        + serde_json::to_vec(mapping)
                            .expect("mapping serializes")
                            .len()
                }
            }
    }
}
