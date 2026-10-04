//! Build and materialize local mirror projections using durable logical IDs.
use super::*;

/// 로컬 PTY 없이 mirror 터미널을 만든다. 입력은 별도 스레드로 원격에 전달한다.
pub(super) fn make_mirror_surface(
    remote_id: u32,
    local_id: u32,
    cols: usize,
    rows: usize,
    frame_tx: &SharedFrameSender,
    engine: &mut EngineMut<'_>,
) {
    let mut mirror = Terminal::new_detached(cols, rows);
    bind_mirror_input(&mut mirror, remote_id, frame_tx, false);
    // mirror의 feed_bytes는 process의 lazy 동기화를 거치지 않아 옵저버 게이트를 여기서 초기화한다.
    mirror.set_output_events_enabled(engine.observer_router.wants(local_id));
    engine.runtime.terminals.insert(local_id, mirror, None);
}

pub(super) fn pending_op_focus_for(
    op: &StructuralOp,
    close_focus_candidates: &[u32],
    remote_to_local: &HashMap<u32, u32>,
) -> Option<PendingOpFocus> {
    match op {
        StructuralOp::NewTab { .. }
        | StructuralOp::SplitSurface { .. }
        | StructuralOp::SplitPane { .. }
        | StructuralOp::RestoreClosedItem { .. } => Some(PendingOpFocus::NewResource),
        StructuralOp::CloseSurface { .. }
        | StructuralOp::CloseTab { .. }
        | StructuralOp::ClosePane { .. } => {
            let candidates: Vec<u32> = close_focus_candidates
                .iter()
                .filter_map(|local_sid| {
                    remote_to_local
                        .iter()
                        .find(|&(_, l)| l == local_sid)
                        .map(|(&r, _)| r)
                })
                .collect();
            if candidates.is_empty() {
                None
            } else {
                Some(PendingOpFocus::Close { candidates })
            }
        }
        _ => None,
    }
}

pub(super) fn install_mirror_fallbacks(workspace: &Workspace, engine: &mut EngineMut<'_>) {
    for id in workspace.all_surface_ids() {
        engine
            .runtime
            .surfaces
            .entry(id)
            .or_insert_with(|| Box::new(EmptySurface::new(id)));
    }
}

pub(super) type MirrorMarkdownLeaves = HashMap<u32, String>;

pub(super) struct SurvivorMapping {
    pub(super) remote_to_local: HashMap<u32, u32>,
    pub(super) terminals: HashSet<u32>,
    pub(super) mesh: HashMap<u32, MirrorMeshInfo>,
    pub(super) explorer: HashMap<u32, std::path::PathBuf>,
    pub(super) markdown: MirrorMarkdownLeaves,
    /// 새로 매핑한 원격 surface는 사용자 new-tab/split의 포커스 후보가 된다.
    pub(super) newly_created_remote_ids: Vec<u32>,
}

impl SurvivorMapping {
    pub(super) fn markdown_ids(&self) -> HashSet<u32> {
        self.markdown.keys().copied().collect()
    }
}

/// kind가 허용된 markdown 플러그인에 등록됐는지 확인한다.
pub(super) fn markdown_mirror_available(
    engine: &crate::runtime::engine_access::EngineRef<'_>,
) -> bool {
    engine
        .runtime
        .surface_registry
        .get_live(MARKDOWN_MIRROR_KIND)
        .is_some_and(|def| {
            matches!(
                &def.source,
                crate::runtime::surface_registry::KindSource::Plugin(p) if p == MARKDOWN_PLUGIN_ID
            )
        })
}

/// 원격 경로는 remote.file로 전달한다. file을 쓰면 플러그인이 로컬 경로로 읽는다.
pub(super) fn create_mirror_markdown_surface(
    descriptor: &Value,
    local_id: u32,
    engine: &crate::runtime::engine_access::EngineRef<'_>,
) -> Option<Box<dyn Surface>> {
    let params = mirror_markdown_params(descriptor);
    let definition = engine
        .runtime
        .surface_registry
        .get_live(MARKDOWN_MIRROR_KIND)?;
    if !matches!(&definition.source,crate::runtime::surface_registry::KindSource::Plugin(plugin) if plugin==MARKDOWN_PLUGIN_ID)
    {
        return None;
    }
    match (definition.create)(local_id, None, &params).and_then(|prepared| prepared.publish()) {
        Ok(surface) => Some(surface),
        Err(e) => {
            tracing::warn!(
                "attach mirror: markdown surface {local_id} 생성 실패 — 빈 surface: {e}"
            );
            None
        }
    }
}

/// 생성과 deferred 복원이 같은 remote params를 사용한다.
pub(super) fn mirror_markdown_params(descriptor: &Value) -> Value {
    let file = descriptor
        .get("file")
        .and_then(|v| v.as_str())
        .unwrap_or_default();
    let display_name = descriptor
        .get("display_name")
        .and_then(|v| v.as_str())
        .unwrap_or(MARKDOWN_MIRROR_KIND);
    serde_json::json!({
        "display_name": display_name,
        "remote": { "file": file },
    })
}

pub(super) fn deferred_mirror_markdown_surface(
    descriptor: &Value,
    local_id: u32,
) -> Box<dyn Surface> {
    Box::new(EmptySurface::new_deferred_plugin(
        local_id,
        DeferredPlugin {
            kind: MARKDOWN_MIRROR_KIND.to_string(),
            snapshot: mirror_markdown_params(descriptor),
        },
    ))
}

/// mirror 삭제는 일반 lifecycle 큐를 거치지 않아 사라진 문서를 플러그인에 직접 알린다.
pub(super) fn destroy_mirror_markdown_surfaces(
    plugin_manager: &mut Option<crate::plugin::PluginManager>,
    ids: impl IntoIterator<Item = u32>,
) {
    let Some(mgr) = plugin_manager.as_mut() else {
        return;
    };
    for id in ids {
        mgr.destroy_remote_surface(id, Some(MARKDOWN_MIRROR_KIND));
    }
}

/// 이 이벤트의 surface ID는 로컬 ID다.
pub(super) fn push_markdown_changed(
    plugin_manager: &mut Option<crate::plugin::PluginManager>,
    local_surface_id: u32,
) {
    let Some(mgr) = plugin_manager.as_mut() else {
        return;
    };
    mgr.emit_host_event_to_plugin(
        MARKDOWN_PLUGIN_ID,
        MARKDOWN_MIRROR_CHANGED_EVENT,
        &serde_json::json!({ "surface_id": local_surface_id }),
        tasty_plugin_protocol::EventScope::System,
    );
}

/// payload.surface_id는 플러그인이 아는 로컬 ID여야 한다.
pub(super) fn push_markdown_content_result(
    plugin_manager: &mut Option<crate::plugin::PluginManager>,
    payload: &Value,
) {
    let Some(mgr) = plugin_manager.as_mut() else {
        return;
    };
    mgr.emit_host_event_to_plugin(
        MARKDOWN_PLUGIN_ID,
        MARKDOWN_MIRROR_CONTENT_RESULT_EVENT,
        payload,
        tasty_plugin_protocol::EventScope::System,
    );
}

/// 요청을 보낼 수 없거나 연결이 끊기면 실패를 합성한다. request_id=0은 대기 요청 취소다.
pub(super) fn markdown_content_failure(
    local_surface_id: u32,
    request_id: u64,
    reason: &str,
) -> Value {
    serde_json::json!({
        "surface_id": local_surface_id,
        "request_id": request_id,
        "ok": false,
        "file": Value::Null,
        "source": Value::Null,
        "truncated": false,
        "reason": reason,
    })
}

pub(super) fn stable_mirror_id(
    ids: &mut HashMap<u32, u32>,
    remote: u32,
    next: impl FnOnce() -> anyhow::Result<u32>,
) -> anyhow::Result<u32> {
    if remote != 0
        && let Some(id) = ids.get(&remote)
    {
        return Ok(*id);
    }
    let id = next()?;
    if remote != 0 {
        ids.insert(remote, id);
    }
    Ok(id)
}

pub(super) fn mirror_id_needs(
    tree: &Value,
    surfaces: usize,
    new_workspace: bool,
) -> anyhow::Result<Vec<(tasty_core::IdKind, u32)>> {
    use tasty_core::IdKind;
    pub(super) fn count(value: &Value) -> usize {
        match value {
            Value::Array(values) => values
                .iter()
                .fold(1usize, |sum, value| sum.saturating_add(count(value))),
            Value::Object(values) => values
                .values()
                .fold(1usize, |sum, value| sum.saturating_add(count(value))),
            _ => 1,
        }
    }
    // Upper bound includes malformed-tree fallback leaves and both legacy/recursive wire shapes.
    let nodes = count(tree)
        .checked_mul(2)
        .and_then(|nodes| nodes.checked_add(8))
        .ok_or_else(|| anyhow::anyhow!("mirror structure size overflow"))?;
    let needs = vec![
        (IdKind::Workspace, u32::from(new_workspace)),
        (IdKind::Pane, u32::try_from(nodes)?),
        (IdKind::Tab, u32::try_from(nodes)?),
        (
            IdKind::Surface,
            u32::try_from(
                nodes
                    .checked_add(surfaces)
                    .ok_or_else(|| anyhow::anyhow!("mirror surface count overflow"))?,
            )?,
        ),
    ];
    Ok(needs)
}
pub(super) fn lease_mirror_ids(
    engine: &EngineMut<'_>,
    tree: &Value,
    surfaces: usize,
    new_workspace: bool,
) -> anyhow::Result<crate::runtime::id_reservations::ReservedIds> {
    let result = engine
        .runtime
        .ids
        .lease(&mirror_id_needs(tree, surfaces, new_workspace)?);
    (engine.runtime.waker)();
    result.map_err(anyhow::Error::new)
}

#[allow(clippy::too_many_arguments)] // reason: fixed mirror parser inputs
pub(super) fn build_pane_from_json(
    structure_ids: &mut MirrorStructureIds,
    navigation: &mut crate::state::navigation::NavigationState,
    p: &Value,
    ids: &crate::runtime::id_reservations::ReservedIds,
    map: &HashMap<u32, u32>,
    term: &HashSet<u32>,
    mesh: &HashMap<u32, MirrorMeshInfo>,
    explorer: &HashMap<u32, std::path::PathBuf>,
    markdown: &mut MirrorMarkdownLeaves,
) -> anyhow::Result<Pane> {
    let mut tabs = Vec::new();
    let mut active = 0;
    for (index, value) in p
        .get("tabs")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .enumerate()
    {
        let layout = match build_layout(
            navigation,
            value.get("layout").unwrap_or(&Value::Null),
            &MirrorLayoutSources {
                ids,
                map,
                term,
                mesh,
                explorer,
            },
            markdown,
        )? {
            Some(layout) => layout,
            None => SurfaceLayout::Leaf(crate::model::SurfaceDescriptor::new(
                ids.next_surface()?,
                "empty",
            )),
        };
        let remote = value.get("id").and_then(Value::as_u64).unwrap_or(0) as u32;
        let tab_id = stable_mirror_id(&mut structure_ids.remote_tabs, remote, || ids.next_tab())?;
        let focus = value
            .get("focused_surface")
            .and_then(Value::as_u64)
            .and_then(|id| map.get(&(id as u32)))
            .copied()
            .or_else(|| layout.first_surface_id())
            .unwrap_or(0);
        let tab = Tab {
            id: tab_id,
            name: value
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or(crate::i18n::t("attach.tab_title_fallback"))
                .into(),
            explicit_name: None,
            layout_opt: Some(layout),
        };
        if value
            .get("active")
            .and_then(Value::as_bool)
            .unwrap_or(false)
        {
            active = index;
        }
        navigation.initialize_surface(&tab, focus);
        tabs.push(tab);
    }
    if tabs.is_empty() {
        tabs.push(Tab {
            id: ids.next_tab()?,
            name: crate::i18n::t("attach.tab_title_fallback").into(),
            explicit_name: None,
            layout_opt: Some(SurfaceLayout::Leaf(crate::model::SurfaceDescriptor::new(
                ids.next_surface()?,
                "empty",
            ))),
        });
    }
    let remote = p.get("id").and_then(Value::as_u64).unwrap_or(0) as u32;
    let id = stable_mirror_id(&mut structure_ids.panes, remote, || ids.next_pane())?;
    let pane = Pane { id, tabs };
    navigation.initialize_tab(&pane, active);
    Ok(pane)
}

#[allow(clippy::too_many_arguments)] // reason: fixed recursive mirror parser inputs
pub(super) fn build_pane_node(
    structure_ids: &mut MirrorStructureIds,
    navigation: &mut crate::state::navigation::NavigationState,
    node: &Value,
    ids: &crate::runtime::id_reservations::ReservedIds,
    map: &HashMap<u32, u32>,
    term: &HashSet<u32>,
    mesh: &HashMap<u32, MirrorMeshInfo>,
    explorer: &HashMap<u32, std::path::PathBuf>,
    markdown: &mut MirrorMarkdownLeaves,
    pane_id_map: &mut HashMap<u32, u32>,
) -> anyhow::Result<Option<PaneNode>> {
    Ok(match node.get("type").and_then(Value::as_str) {
        Some("Leaf") => {
            let Some(remote) = node.get("id").and_then(Value::as_u64) else {
                return Ok(None);
            };
            let pane = build_pane_from_json(
                structure_ids,
                navigation,
                node,
                ids,
                map,
                term,
                mesh,
                explorer,
                markdown,
            )?;
            pane_id_map.insert(remote as u32, pane.id);
            Some(PaneNode::Leaf(pane))
        }
        Some("Split") => {
            let (Some(first), Some(second)) = (node.get("first"), node.get("second")) else {
                return Ok(None);
            };
            let Some(first) = build_pane_node(
                structure_ids,
                navigation,
                first,
                ids,
                map,
                term,
                mesh,
                explorer,
                markdown,
                pane_id_map,
            )?
            else {
                return Ok(None);
            };
            let Some(second) = build_pane_node(
                structure_ids,
                navigation,
                second,
                ids,
                map,
                term,
                mesh,
                explorer,
                markdown,
                pane_id_map,
            )?
            else {
                return Ok(None);
            };
            Some(PaneNode::Split {
                direction: wire_direction(node),
                ratio: node.get("ratio").and_then(Value::as_f64).unwrap_or(0.5) as f32,
                first: Box::new(first),
                second: Box::new(second),
            })
        }
        _ => None,
    })
}
pub(super) fn wire_direction(node: &Value) -> SplitDirection {
    if node.get("direction").and_then(Value::as_str) == Some("vertical") {
        SplitDirection::Vertical
    } else {
        SplitDirection::Horizontal
    }
}

#[allow(clippy::too_many_arguments)] // reason: mirror projection and reserved identity inputs
pub(super) fn build_mirror_workspace(
    structure_ids: &mut MirrorStructureIds,
    navigation: &mut crate::state::navigation::NavigationState,
    ws_id: u32,
    name: &str,
    tree: &Value,
    ids: &crate::runtime::id_reservations::ReservedIds,
    map: &HashMap<u32, u32>,
    term: &HashSet<u32>,
    mesh: &HashMap<u32, MirrorMeshInfo>,
    explorer: &HashMap<u32, std::path::PathBuf>,
    markdown: &mut MirrorMarkdownLeaves,
) -> anyhow::Result<Workspace> {
    let focused = tree
        .get("focused_pane")
        .and_then(Value::as_u64)
        .unwrap_or(0) as u32;
    let mut pane_map = HashMap::new();
    let mut layout = if let Some(node) = tree.get("pane_layout") {
        build_pane_node(
            structure_ids,
            navigation,
            node,
            ids,
            map,
            term,
            mesh,
            explorer,
            markdown,
            &mut pane_map,
        )?
    } else {
        None
    };
    if layout.is_none() {
        for value in tree
            .get("panes")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let pane = build_pane_from_json(
                structure_ids,
                navigation,
                value,
                ids,
                map,
                term,
                mesh,
                explorer,
                markdown,
            )?;
            pane_map.insert(
                value.get("id").and_then(Value::as_u64).unwrap_or(0) as u32,
                pane.id,
            );
            layout = Some(match layout {
                None => PaneNode::Leaf(pane),
                Some(first) => PaneNode::Split {
                    direction: SplitDirection::Horizontal,
                    ratio: 0.5,
                    first: Box::new(first),
                    second: Box::new(PaneNode::Leaf(pane)),
                },
            });
        }
    }
    let layout = match layout {
        Some(layout) => layout,
        None => PaneNode::Leaf(Pane::new_with_surface(
            ids.next_pane()?,
            ids.next_tab()?,
            crate::i18n::t("attach.tab_title_fallback").into(),
            crate::model::SurfaceDescriptor::new(ids.next_surface()?, "empty"),
        )),
    };
    let focused = pane_map
        .get(&focused)
        .copied()
        .or_else(|| layout.first_pane().map(|pane| pane.id))
        .unwrap_or(0);
    let mut workspace = Workspace::from_restored(ws_id, name.into(), String::new(), layout);
    workspace.mirror = true;
    navigation.initialize_pane(&workspace, focused);
    structure_ids.retain_workspace(&workspace);
    Ok(workspace)
}

pub(super) struct MirrorLayoutSources<'a> {
    pub(super) ids: &'a crate::runtime::id_reservations::ReservedIds,
    pub(super) map: &'a HashMap<u32, u32>,
    pub(super) term: &'a HashSet<u32>,
    pub(super) mesh: &'a HashMap<u32, MirrorMeshInfo>,
    pub(super) explorer: &'a HashMap<u32, std::path::PathBuf>,
}
pub(super) fn build_layout(
    navigation: &mut crate::state::navigation::NavigationState,
    node: &Value,
    sources: &MirrorLayoutSources<'_>,
    markdown: &mut MirrorMarkdownLeaves,
) -> anyhow::Result<Option<SurfaceLayout>> {
    Ok(match node.get("type").and_then(Value::as_str) {
        Some("Leaf") => {
            let Some(remote) = node.get("id").and_then(Value::as_u64) else {
                return Ok(None);
            };
            let local = match sources.map.get(&(remote as u32)) {
                Some(local) => *local,
                None => sources.ids.next_surface()?,
            };
            let kind = if sources.term.contains(&local) {
                "terminal"
            } else if let Some(info) = sources.mesh.get(&local) {
                info.kind.as_str()
            } else if sources.explorer.contains_key(&local) {
                "explorer"
            } else {
                markdown.get(&local).map_or("empty", String::as_str)
            };
            Some(SurfaceLayout::Leaf(crate::model::SurfaceDescriptor::new(
                local, kind,
            )))
        }
        Some("Split") => {
            let (Some(first), Some(second)) = (node.get("first"), node.get("second")) else {
                return Ok(None);
            };
            let Some(first) = build_layout(navigation, first, sources, markdown)? else {
                return Ok(None);
            };
            let Some(second) = build_layout(navigation, second, sources, markdown)? else {
                return Ok(None);
            };
            let node_id = crate::model::SplitNodeId::allocate();
            navigation.split_hints.insert(
                node_id,
                node.get("focus_second")
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
            );
            Some(SurfaceLayout::Split {
                direction: wire_direction(node),
                ratio: node.get("ratio").and_then(Value::as_f64).unwrap_or(0.5) as f32,
                first: Box::new(first),
                second: Box::new(second),
                node_id,
            })
        }
        _ => None,
    })
}

impl crate::runtime::engine_access::EngineMut<'_> {
    /// Remote placeholders materialize on the App side and never enter the local journal.
    pub(crate) fn reify_displayed_mirror_resources(&mut self, selected: &[u32]) {
        for id in selected {
            if !self.core.is_mirror_surface(*id) {
                continue;
            }
            let deferred = self
                .runtime
                .surfaces
                .get(id)
                .and_then(|surface| surface.as_any().downcast_ref::<EmptySurface>())
                .and_then(|empty| match &empty.deferred {
                    Some(crate::model::Deferred::Plugin(value)) => Some(value.clone()),
                    _ => None,
                });
            let Some(deferred) = deferred else {
                continue;
            };
            if deferred.kind != MARKDOWN_MIRROR_KIND || !markdown_mirror_available(&self.as_ref()) {
                continue;
            }
            let Some(definition) = self.runtime.surface_registry.get_live(&deferred.kind) else {
                continue;
            };
            let surface = match (definition.restore)(*id, &deferred.snapshot)
                .and_then(|prepared| prepared.publish())
            {
                Ok(surface) => surface,
                Err(error) => {
                    tracing::warn!(surface = *id, "mirror kind restoration failed: {error}");
                    continue;
                }
            };
            let kind = surface.kind().to_owned();
            drop(self.runtime.surfaces.insert(*id, surface));
            if let Some((index, pane)) = self.core.find_workspace_index_for_surface(*id)
                && let Some(workspace) = self.core.workspace_at(index).map(|workspace| workspace.id)
                && let Some(workspace) = self.core.mirror_workspace_mut(workspace)
                && let Some(pane) = workspace.pane_layout_mut().find_pane_mut(pane)
                && let Some(tab) = pane.tabs.iter_mut().find(|tab| tab.contains_surface(*id))
                && let Some(descriptor) = tab.surface_mut(*id)
            {
                descriptor.kind = kind;
            }
        }
    }
}
