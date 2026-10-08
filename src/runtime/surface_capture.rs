//! Freeze runtime content without activating lazy leaves or performing journal I/O.
use crate::core::layout_persistence::import::surface_data::SurfaceData;
use crate::runtime::engine_session::EngineSession;

#[derive(Debug)]
pub(crate) struct CapturedSurface {
    pub surface: u32,
    pub kind: String,
    pub activation: Option<u64>,
    pub bytes: Vec<u8>,
}
impl CapturedSurface {
    pub(crate) fn weight(&self) -> usize {
        self.kind
            .len()
            .saturating_add(self.bytes.len())
            .saturating_add(96)
    }
}

/// The caller serializes captures for an engine. The worker allocates persistent content sequence
/// numbers in accepted submission order, independently of activation and physical Pty generations.
/// A placeholder contributes no new capture: its canonical immutable data/seed stays pinned.
pub(crate) fn capture(session: &EngineSession) -> Result<Vec<CapturedSurface>, String> {
    capture_selected(session, None)
}
pub(crate) fn capture_selected(
    session: &EngineSession,
    selected: Option<&std::collections::HashSet<u32>>,
) -> Result<Vec<CapturedSurface>, String> {
    let engine = session.as_ref();
    let include_content = engine.runtime.settings.general.restore_surface_content;
    let mut result = Vec::new();
    for workspace in engine.core.local_workspaces() {
        for id in workspace.all_surface_ids() {
            if selected.is_some_and(|selected| !selected.contains(&id)) {
                continue;
            }
            let descriptor = engine
                .core
                .find_surface_by_id(id)
                .ok_or("capture descriptor missing")?;
            let owner = engine
                .runtime
                .surfaces
                .get(&id)
                .ok_or("capture instance missing")?;
            if owner
                .as_any()
                .is::<crate::runtime::surface_restorer::JournalPlaceholder>()
            {
                continue;
            }
            let data = if descriptor.kind == "terminal" {
                if let Some(terminal) = engine.runtime.terminals.get(id) {
                    let restore_command = {
                        let mut memory = crate::poison::recover_mutex(
                            engine.runtime.memory.lock(),
                            crate::core::MEMORY_WHAT,
                            &crate::core::MEMORY_POISONED,
                        );
                        crate::surface_meta::SurfaceMetaStore::get(
                            &mut *memory,
                            id,
                            "restore.command",
                        )
                    };
                    let lines = include_content.then(|| terminal.capture_history_and_screen());
                    SurfaceData::Terminal {
                        cwd: engine
                            .runtime
                            .terminals
                            .cwd(id)
                            .map(|cwd| cwd.to_string_lossy().into_owned()),
                        restore_command,
                        scrollback_ref: None,
                        scrollback: lines
                            .map(|lines| tasty_terminal::disk_scrollback::serialize_lines(&lines)),
                    }
                } else {
                    // Legacy lazy terminals are consumed by the import adapter; capture must not
                    // manufacture a replacement seed from current focus or default shell settings.
                    continue;
                }
            } else if let Some(empty) = owner.as_any().downcast_ref::<crate::model::EmptySurface>()
            {
                match &empty.deferred {
                    Some(saved) => SurfaceData::Generic {
                        data: saved.snapshot.clone(),
                    },
                    None => continue,
                }
            } else {
                let definition = engine
                    .runtime
                    .surface_registry
                    .get(&descriptor.kind)
                    .ok_or_else(|| {
                        format!("cannot capture unregistered kind {}", descriptor.kind)
                    })?;
                SurfaceData::Generic {
                    data: (definition.snapshot)(owner.as_ref())
                        .unwrap_or_else(|| serde_json::json!({})),
                }
            };
            result.push(CapturedSurface {
                surface: id,
                kind: descriptor.kind.clone(),
                activation: descriptor.activation_generation,
                bytes: data.encode().map_err(|error| error.to_string())?,
            });
        }
    }
    Ok(result)
}
