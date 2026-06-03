//! Plugin Canvas 텍스처 prepare 단계.
//!
//! 매 frame 호스트는 egui 그리기에 앞서 활성 workspace의 plugin Canvas 노드를 찾아내
//! GPU 텍스처를 보장(ensure)하고 dirty rect 만큼 staging upload한다. 그래야 이후 egui
//! 그리기에서 [`crate::plugin_bridge::ui_tree_render::render_remote_surface`]가 캐시에서 바로
//! [`egui::TextureId`]를 얻어 합성할 수 있다.
//!
//! # 흐름
//!
//! 1. 활성 workspace의 모든 pane → active tab → 모든 surface를 순회.
//! 2. [`crate::plugin_bridge::remote_surface::RemoteSurface`]로 다운캐스트되는 surface에 대해
//!    트리에서 [`tasty_plugin_protocol::UiNode::Canvas`] 노드를 재귀 수집.
//! 3. plugin 별로 dirty rect를 한 번씩 drain.
//! 4. 각 canvas에 대해 호스트 측 [`tasty_shm::SharedMemory`]를 lookup, 크기 검증, atomic
//!    generation Acquire-load, dirty rect만 staging Vec 경유 wgpu upload.
//!
//! # 크기 검증
//!
//! `width × height × bpp + footer ≤ shm.len()`이 보장되지 않으면 warn 로그 후 skip.
//! Plugin이 보낸 width/height를 신뢰하기 전 호스트가 한 번 확인한다.

use std::collections::HashMap;
use std::sync::atomic::Ordering;

use tasty_plugin_protocol::{PixelFilter, PixelFormat, PixelRect, SharedBufferId, UiNode};

use super::GpuState;
use super::canvas_texture::CanvasKey;
use crate::plugin::PluginManager;
use crate::plugin_bridge::remote_surface::RemoteSurface;
use crate::state::AppState;

struct PendingCanvas {
    plugin_id: String,
    buffer_id: SharedBufferId,
    width: u32,
    height: u32,
    format: PixelFormat,
    filter: PixelFilter,
}

impl GpuState {
    /// 활성 workspace의 plugin Canvas 노드 GPU 텍스처를 ensure + upload.
    ///
    /// [`GpuState::render`]가 [`GpuState::run_egui_frame`] 호출 직전에 부른다.
    pub(super) fn prepare_plugin_canvases(
        &mut self,
        state: &AppState,
        engine: &crate::core::CoreState,
        plugin_manager: &PluginManager,
    ) {
        let pending = collect_canvas_requests(state, engine, plugin_manager);
        if pending.is_empty() {
            return;
        }

        // plugin 별 dirty rect를 한 번 drain. 동일 plugin이 여러 canvas를 가지면 한
        // 묶음으로 받아 각 buffer_id별 lookup.
        let mut dirty_per_plugin: HashMap<String, HashMap<SharedBufferId, Option<PixelRect>>> =
            HashMap::new();
        for c in &pending {
            if !dirty_per_plugin.contains_key(&c.plugin_id) {
                dirty_per_plugin.insert(
                    c.plugin_id.clone(),
                    plugin_manager.take_plugin_dirty_rects(&c.plugin_id),
                );
            }
        }

        for c in pending {
            let Some(mem) = plugin_manager.plugin_buffer(&c.plugin_id, c.buffer_id) else {
                tracing::warn!(
                    plugin = %c.plugin_id,
                    buffer = c.buffer_id.0,
                    "canvas prepare: SharedMemory not registered"
                );
                continue;
            };

            // 크기 검증: width * height * bpp + footer ≤ mem.len().
            let bpp = c.format.bytes_per_pixel() as usize;
            let pixel_bytes = (c.width as usize)
                .checked_mul(c.height as usize)
                .and_then(|v| v.checked_mul(bpp));
            let required = match pixel_bytes.and_then(|p| p.checked_add(tasty_shm::footer::SIZE)) {
                Some(v) => v,
                None => {
                    tracing::warn!(
                        plugin = %c.plugin_id,
                        buffer = c.buffer_id.0,
                        width = c.width,
                        height = c.height,
                        "canvas prepare: size overflow"
                    );
                    continue;
                }
            };
            if required > mem.len() {
                tracing::warn!(
                    plugin = %c.plugin_id,
                    buffer = c.buffer_id.0,
                    required,
                    actual = mem.len(),
                    "canvas prepare: buffer too small for declared size"
                );
                continue;
            }

            // SAFETY: SharedMemory 영역. tasty-shm 문서의 동기화 규약에 따라 plugin이
            // commit (Release) → host가 Acquire-load → user data 읽음 → 재 load로 검증.
            // 본 prepare는 frame 합성용 read-only; 동시 mutate로 인한 "unspecified value"는
            // generation 비교로 다음 frame까지 미루므로 tear가 최종 화면에 노출되지 않는다.
            let raw = unsafe { mem.as_slice() };
            // SAFETY: 위와 동일 조건. footer 시작 8B는 mmap 페이지 align이라 8B aligned.
            let generation = unsafe { tasty_shm::footer::load(raw, Ordering::Acquire) };
            let user = tasty_shm::footer::user_slice(raw);

            let dirty = dirty_per_plugin
                .get(&c.plugin_id)
                .and_then(|m| m.get(&c.buffer_id))
                .copied()
                .flatten();

            let key = CanvasKey {
                plugin_id: c.plugin_id.clone(),
                buffer_id: c.buffer_id,
            };
            self.canvas_textures.ensure(
                &key,
                &self.device,
                &mut self.egui_renderer,
                c.width,
                c.height,
                c.format,
                c.filter,
            );
            self.canvas_textures
                .upload_if_dirty(&key, &self.queue, user, generation, dirty);
        }
    }
}

/// 활성 workspace에서 visible plugin Canvas 노드 일람 + 모든 plugin popup instance.
///
/// 본 함수는 GPU 자원에 손대지 않는다 — 순수 데이터 수집. 다음 단계에서 ensure + upload.
fn collect_canvas_requests(
    state: &AppState,
    engine: &crate::core::CoreState,
    plugin_manager: &PluginManager,
) -> Vec<PendingCanvas> {
    let mut out: Vec<PendingCanvas> = Vec::new();
    let ws = state.active_workspace(engine);
    let pane_ids = ws.pane_layout().all_pane_ids();
    for pane_id in pane_ids {
        let Some(pane) = ws.pane_layout().find_pane(pane_id) else {
            continue;
        };
        let Some(tab) = pane.tabs.get(pane.active_tab) else {
            continue;
        };
        let layout = tab.layout();
        for sid in layout.all_surface_ids() {
            let Some(surface) = layout.find_surface(sid) else {
                continue;
            };
            let Some(remote) = surface.as_any().downcast_ref::<RemoteSurface>() else {
                continue;
            };
            let Ok(tree_opt) = remote.tree.lock() else {
                continue;
            };
            if let Some(node) = tree_opt.as_ref() {
                collect_canvases_in_node(node, &remote.plugin_id, &mut out);
            }
        }
    }
    // Plugin popup 인스턴스도 자기 plugin_id 컨텍스트에서 Canvas 노드를 가질 수 있다.
    // popup_instances는 워크스페이스 무관 전역이므로 항상 전부 순회.
    for (_id, inst) in plugin_manager.popup_instances() {
        if let Some(node) = inst.tree.as_ref() {
            collect_canvases_in_node(node, &inst.plugin_id, &mut out);
        }
    }
    out
}

fn collect_canvases_in_node(node: &UiNode, plugin_id: &str, out: &mut Vec<PendingCanvas>) {
    match node {
        UiNode::Canvas {
            buffer_id,
            width,
            height,
            format,
            filter,
            commit_seq: _,
            id: _,
        } => {
            out.push(PendingCanvas {
                plugin_id: plugin_id.to_string(),
                buffer_id: *buffer_id,
                width: *width,
                height: *height,
                format: *format,
                filter: *filter,
            });
        }
        UiNode::Vbox { children, .. } | UiNode::Hbox { children, .. } => {
            for c in children {
                collect_canvases_in_node(c, plugin_id, out);
            }
        }
        UiNode::Scroll { child, .. } => collect_canvases_in_node(child, plugin_id, out),
        UiNode::Splitter { first, second, .. } => {
            collect_canvases_in_node(first, plugin_id, out);
            collect_canvases_in_node(second, plugin_id, out);
        }
        UiNode::Label { .. }
        | UiNode::Icon { .. }
        | UiNode::Button { .. }
        | UiNode::Tree { .. }
        | UiNode::Addressbar { .. }
        | UiNode::TextPreview { .. }
        | UiNode::Spacer { .. }
        | UiNode::SelectableRow { .. } => {}
    }
}
