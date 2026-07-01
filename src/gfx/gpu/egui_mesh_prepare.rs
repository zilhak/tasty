//! egui-mesh surface 의 host 측 합성 (A1-S5).
//!
//! plugin 이 자기 프로세스에서 egui 를 tessellate 한 paint 출력을 POD 바이트로
//! SharedBuffer 에 commit 하면(ADR-0028), host 가 그 바이트를 [`mesh_wire::decode_paint`]
//! 로 `(Vec<ClippedPrimitive>, TexturesDelta, ppp)` 로 복원해 **전용 `egui_wgpu::Renderer`**
//! 로 surface 영역에 합성한다.
//!
//! # 전용 Renderer (TextureId 격리, §4-3)
//!
//! plugin 의 `TextureId::Managed(0)`(폰트 atlas) 가 host `egui_renderer` 의 Managed(0)
//! (host 폰트) 와 충돌하므로, egui-mesh surface 마다 독립 `egui_wgpu::Renderer` 를 둔다 —
//! TextureId 네임스페이스가 자연 격리돼 remap 이 불필요하다. 텍스처 lifecycle
//! (set→render→free) 도 이 전용 Renderer 안에서 frame 경계에 원자 처리된다.
//!
//! # 좌표 변환 (영역 한정, §2-4)
//!
//! plugin 은 0,0 = 좌상단 기준 surface-local 좌표로 그린다. egui_wgpu 의 scissor 는
//! framebuffer 절대 좌표라 viewport 만으로는 위치를 옮길 수 없다. 따라서 모든
//! vertex.pos / clip_rect 를 surface origin 만큼 평행이동해 window-global 좌표로 만든
//! 뒤(=정확한 위치), clip_rect 를 surface 경계로 intersect 해(=영역 한정) 합성한다.
//! 변환 결과는 (generation, rect) 가 바뀔 때만 재계산하도록 캐시한다.
//!
//! # generation / ppp 가드 (§9-3·§9-4)
//!
//! - generation 이 직전 합성과 같으면 재디코드를 건너뛰고 캐시된 mesh 로 재합성한다
//!   (stale tear 방지: half-painted frame 은 footer generation 으로 다음 frame 까지 미룸).
//! - 디코드된 ppp 가 host 의 현재 ppp 와 어긋나면(리사이즈/DPI 전환 직후 한 frame 지연)
//!   이번 frame 합성을 skip 해 잘못된 스케일 합성을 막는다.

use std::collections::HashSet;
use std::sync::atomic::Ordering;

use egui::epaint::{ClippedPrimitive, Primitive};

use super::GpuState;
use crate::model::PhysicalRect;
use crate::plugin::PluginManager;
use crate::plugin_bridge::egui_mesh_surface::EguiMeshSurface;
use crate::state::AppState;

/// 디코드 ppp 와 host ppp 의 허용 오차. float 비교라 작은 epsilon.
const PPP_EPS: f32 = 1.0e-3;

/// 한 egui-mesh surface 의 전용 Renderer + 디코드 캐시.
///
/// `egui_mesh_targets: HashMap<surface_id, _>` 로 [`GpuState`] 가 보관한다. surface 가
/// layout 에서 사라지면 정리돼 전용 Renderer 의 GPU 자원이 해제된다.
pub(super) struct EguiMeshRenderTarget {
    /// 이 surface 전용 egui_wgpu Renderer (TextureId 네임스페이스 격리).
    renderer: egui_wgpu::Renderer,
    /// 디코드된 surface-local primitives (평행이동 전 원본).
    primitives: Vec<ClippedPrimitive>,
    /// 디코드된 pixels_per_point.
    ppp: f32,
    /// 마지막으로 디코드한 footer generation.
    generation: u64,
    /// 한 번이라도 유효 mesh 를 디코드했는가. false 면 합성 skip.
    has_content: bool,
    /// window-global 좌표로 평행이동/클립한 합성용 primitives 캐시.
    translated: Vec<ClippedPrimitive>,
    /// `translated` 가 유효한 (generation, rect bits) 키. 둘 중 하나라도 바뀌면 재계산.
    translated_key: Option<(u64, [u32; 4])>,
}

impl EguiMeshRenderTarget {
    fn new(renderer: egui_wgpu::Renderer) -> Self {
        Self {
            renderer,
            primitives: Vec::new(),
            ppp: 1.0,
            generation: 0,
            has_content: false,
            translated: Vec::new(),
            translated_key: None,
        }
    }
}

/// 활성 workspace 에서 egui-mesh surface 의 (surface_id, plugin_id, 물리 rect) 일람.
///
/// `surface_regions` 가 layout 에 실제 존재하는 surface 만 반환하고, 그중
/// [`EguiMeshSurface`] 로 다운캐스트되는 것만 골라낸다. 화이트리스트 gate 에서 거부된
/// kind 는 애초에 `EguiMeshSurface` 로 생성되지 않으므로(registry 미등록) 여기 잡히지
/// 않는다 — registry 미등록 kind 의 합성을 시도하지 않는다(A1-S1 인계 점검).
pub(super) fn collect_egui_mesh_targets(
    state: &AppState,
    engine: &crate::core::CoreState,
    terminal_rect: PhysicalRect,
) -> Vec<(u32, String, PhysicalRect)> {
    let mut out: Vec<(u32, String, PhysicalRect)> = Vec::new();
    for (_pane_id, _pane_rect, regions) in state.surface_regions(engine, terminal_rect) {
        for r in regions {
            if let Some(ms) = r.surface.as_any().downcast_ref::<EguiMeshSurface>() {
                out.push((r.id, ms.plugin_id.clone(), r.rect));
            }
        }
    }
    out
}

/// primitives 를 surface origin 만큼 평행이동하고 clip_rect 를 surface 경계로 클립한다.
///
/// - `offset` (points): surface origin = (rect.x / ppp, rect.y / ppp).
/// - `bounds` (points, window-global): surface 의 화면 영역. clip 을 여기로 intersect 해
///   plugin 이 영역 밖을 그리려 해도 scissor 가 surface 안에 갇힌다.
fn offset_and_clip(
    prims: &[ClippedPrimitive],
    offset: egui::Vec2,
    bounds: egui::Rect,
) -> Vec<ClippedPrimitive> {
    prims
        .iter()
        .map(|p| {
            let clip_rect = p.clip_rect.translate(offset).intersect(bounds);
            let primitive = match &p.primitive {
                Primitive::Mesh(m) => {
                    let mut m = m.clone();
                    for v in &mut m.vertices {
                        v.pos += offset;
                    }
                    Primitive::Mesh(m)
                }
                // decode_paint 는 Callback 을 제거하므로 정상 경로엔 오지 않는다.
                // 방어적으로 그대로 통과(합성 시 무시됨).
                Primitive::Callback(c) => Primitive::Callback(c.clone()),
            };
            ClippedPrimitive {
                clip_rect,
                primitive,
            }
        })
        .collect()
}

/// SharedBuffer 의 mesh 바이트를 디코드해 한 target 에 올린다(generation 변경 시에만 호출).
/// footer tear / 너무 작은 buffer / ppp 불일치 가드를 통과해야 갱신한다 — 실패하면
/// 기존 캐시를 유지(이번 frame 재합성). surface/popup 합성기가 공유한다.
fn decode_mesh_into_target(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    target: &mut EguiMeshRenderTarget,
    raw: &[u8],
    generation: u64,
    host_ppp: f32,
    log_id: &str,
) {
    // footer 길이 가드: plugin 이 footer(8B) 미만 버퍼를 등록하면 footer::load 의
    // 안전 전제(raw.len >= SIZE)가 깨진다. canvas_prepare 형제 경로처럼 skip.
    if raw.len() < tasty_shm::footer::SIZE {
        tracing::warn!(
            target = log_id,
            actual = raw.len(),
            "egui-mesh prepare: buffer too small"
        );
        return;
    }
    // SAFETY: SharedMemory 영역. tasty-shm 동기화 규약 — plugin commit(Release) →
    // host Acquire-load → user data read. footer 시작 8B 는 mmap 페이지 align 이라 8B aligned.
    // footer generation 이 frame 메타와 다르면 half-painted 이므로 다음 frame 까지 미룬다.
    let gen_now = unsafe { tasty_shm::footer::load(raw, Ordering::Acquire) };
    if gen_now != generation {
        return;
    }
    let user = tasty_shm::footer::user_slice(raw);

    let decoded = match tasty_plugin_protocol::mesh_wire::decode_paint(user) {
        Ok(d) => d,
        Err(e) => {
            tracing::warn!(target = log_id, "egui-mesh prepare: decode failed: {e}");
            return;
        }
    };

    // ppp 불일치 가드 (§9-4): 리사이즈/DPI 전환 직후 plugin 이 옛 ppp 로 tessellate 한
    // stale mesh 일 수 있다. host ppp 와 어긋나면 이번 frame 합성을 미룬다 — generation 을
    // 갱신하지 않으므로 다음 frame 에 다시 시도하고, plugin 이 새 ppp 로 보내면 통과한다.
    if (decoded.pixels_per_point - host_ppp).abs() > PPP_EPS {
        tracing::debug!(
            target = log_id,
            decoded_ppp = decoded.pixels_per_point,
            host_ppp,
            "egui-mesh prepare: ppp mismatch, deferring composite"
        );
        return;
    }

    // 전용 Renderer 라 TextureId 가 host 와 충돌하지 않는다. set → (이후 render) → free
    // 순서로 frame 경계에서 원자 처리. free 대상은 primitives 가 참조하지 않으므로
    // render 전에 풀어도 안전(set/free 는 서로소).
    for (id, delta) in &decoded.textures_delta.set {
        target.renderer.update_texture(device, queue, *id, delta);
    }
    for id in &decoded.textures_delta.free {
        target.renderer.free_texture(id);
    }
    target.primitives = decoded.primitives;
    target.ppp = decoded.pixels_per_point;
    target.generation = generation;
    target.has_content = true;
    target.translated_key = None; // 평행이동 캐시 무효화.
}

/// 캐시된 mesh 를 물리 rect 영역에 합성한다(전용 Renderer + 전용 pass).
/// surface/popup 합성기가 공유한다 — popup 은 host egui pass *후*, surface 는 *전*.
fn composite_mesh_target(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    target: &mut EguiMeshRenderTarget,
    view: &wgpu::TextureView,
    rect: PhysicalRect,
    size_in_pixels: [u32; 2],
) {
    if !target.has_content {
        return;
    }
    let ppp = target.ppp;
    if ppp <= 0.0 {
        return;
    }

    // 평행이동 캐시 갱신 (generation/rect 변경 시에만 재계산).
    let rect_bits = [
        rect.x.value().to_bits(),
        rect.y.value().to_bits(),
        rect.width.value().to_bits(),
        rect.height.value().to_bits(),
    ];
    let key = (target.generation, rect_bits);
    if target.translated_key != Some(key) {
        let offset = egui::vec2(rect.x.value() / ppp, rect.y.value() / ppp);
        let bounds = egui::Rect::from_min_size(
            egui::pos2(rect.x.value() / ppp, rect.y.value() / ppp),
            egui::vec2(rect.width.value() / ppp, rect.height.value() / ppp),
        );
        target.translated = offset_and_clip(&target.primitives, offset, bounds);
        target.translated_key = Some(key);
    }
    if target.translated.is_empty() {
        return;
    }

    let screen_descriptor = egui_wgpu::ScreenDescriptor {
        size_in_pixels,
        pixels_per_point: ppp,
    };

    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("egui_mesh_encoder"),
    });
    target.renderer.update_buffers(
        device,
        queue,
        &mut encoder,
        &target.translated,
        &screen_descriptor,
    );
    {
        let render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("egui_mesh_pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Load,
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
        });
        let mut render_pass = render_pass.forget_lifetime();
        target
            .renderer
            .render(&mut render_pass, &target.translated, &screen_descriptor);
    }
    queue.submit(std::iter::once(encoder.finish()));
}

impl GpuState {
    /// egui-mesh surface 들을 전용 Renderer 로 framebuffer 에 합성한다.
    ///
    /// [`GpuState::render`] 가 terminal 렌더 직후, host egui chrome 합성 직전에 부른다.
    /// mesh 콘텐츠는 terminal 콘텐츠와 같은 layer(host chrome 아래)에 놓인다.
    pub(super) fn render_egui_mesh_surfaces(
        &mut self,
        view: &wgpu::TextureView,
        targets: &[(u32, String, PhysicalRect)],
        plugin_manager: &PluginManager,
    ) {
        // layout 에서 사라진 surface 의 전용 Renderer 정리 (GPU 자원 해제).
        let live: HashSet<u32> = targets.iter().map(|t| t.0).collect();
        self.egui_mesh_targets.retain(|sid, _| live.contains(sid));

        let size_in_pixels = [self.size.width, self.size.height];
        for (sid, plugin_id, rect) in targets {
            // plugin 이 아직 frame 을 안 보냈거나 disconnect 로 정리됐으면 합성할 것이 없다.
            // (disconnect 시 placeholder/blank — 캐시가 남아 있어도 합성하지 않는다, §9-7.)
            let Some(frame) = plugin_manager.egui_mesh_frame(*sid) else {
                continue;
            };
            let frame_buffer_id = frame.buffer_id;
            let frame_generation = frame.generation;
            let host_ppp = self.scale_factor;

            // 전용 Renderer ensure.
            if !self.egui_mesh_targets.contains_key(sid) {
                let renderer =
                    egui_wgpu::Renderer::new(&self.device, self.config.format, None, 1, false);
                self.egui_mesh_targets
                    .insert(*sid, EguiMeshRenderTarget::new(renderer));
            }

            // generation 이 바뀌었거나 아직 콘텐츠가 없을 때만 디코드.
            let needs_decode = {
                let t = &self.egui_mesh_targets[sid];
                !t.has_content || t.generation != frame_generation
            };
            if needs_decode {
                if let Some(mem) = plugin_manager.plugin_buffer(plugin_id, frame_buffer_id) {
                    // SAFETY: tasty-shm 동기화 규약 (decode_mesh_into_target 내 Acquire-load).
                    let raw = unsafe { mem.as_slice() };
                    decode_mesh_into_target(
                        &self.device,
                        &self.queue,
                        self.egui_mesh_targets.get_mut(sid).expect("ensured above"),
                        raw,
                        frame_generation,
                        host_ppp,
                        plugin_id,
                    );
                } else {
                    tracing::warn!(
                        plugin = %plugin_id,
                        buffer = frame_buffer_id.0,
                        surface = sid,
                        "egui-mesh prepare: SharedMemory not registered"
                    );
                }
            }

            // 합성 (콘텐츠 있으면 매 frame — framebuffer 가 매 frame clear 되므로).
            if let Some(t) = self.egui_mesh_targets.get_mut(sid) {
                composite_mesh_target(&self.device, &self.queue, t, view, *rect, size_in_pixels);
            }
        }
    }

    /// egui-mesh popup(A2) 들을 전용 Renderer 로 합성한다.
    ///
    /// [`GpuState::render`] 가 host egui pass *후* 부른다 — popup 셸(scrim/bg/border)을
    /// host egui 가 그린 뒤 그 위 content_rect 에 plugin mesh 를 얹는다. mesh 는 content_rect
    /// 로 clip 되므로 border/close 버튼을 덮지 않는다. surface 와 같은 디코드/합성 헬퍼를
    /// 쓰되, target 맵을 instance_id 로 키잉하고 frame meta 를 `popup_mesh_frame` 에서 읽는다.
    pub(super) fn render_egui_mesh_popups(
        &mut self,
        view: &wgpu::TextureView,
        regions: &[(u64, PhysicalRect)],
        plugin_manager: &PluginManager,
    ) {
        // 닫힌 popup 의 전용 Renderer 정리 (GPU 자원 해제).
        let live: HashSet<u64> = regions.iter().map(|r| r.0).collect();
        self.egui_mesh_popup_targets
            .retain(|iid, _| live.contains(iid));

        let size_in_pixels = [self.size.width, self.size.height];
        for (iid, rect) in regions {
            let Some(frame) = plugin_manager.popup_mesh_frame(*iid) else {
                continue;
            };
            let frame_plugin_id = frame.plugin_id.clone();
            let frame_buffer_id = frame.buffer_id;
            let frame_generation = frame.generation;
            let host_ppp = self.scale_factor;

            if !self.egui_mesh_popup_targets.contains_key(iid) {
                let renderer =
                    egui_wgpu::Renderer::new(&self.device, self.config.format, None, 1, false);
                self.egui_mesh_popup_targets
                    .insert(*iid, EguiMeshRenderTarget::new(renderer));
            }

            let needs_decode = {
                let t = &self.egui_mesh_popup_targets[iid];
                !t.has_content || t.generation != frame_generation
            };
            if needs_decode {
                if let Some(mem) = plugin_manager.plugin_buffer(&frame_plugin_id, frame_buffer_id) {
                    // SAFETY: tasty-shm 동기화 규약 (decode_mesh_into_target 내 Acquire-load).
                    let raw = unsafe { mem.as_slice() };
                    decode_mesh_into_target(
                        &self.device,
                        &self.queue,
                        self.egui_mesh_popup_targets
                            .get_mut(iid)
                            .expect("ensured above"),
                        raw,
                        frame_generation,
                        host_ppp,
                        &frame_plugin_id,
                    );
                } else {
                    tracing::warn!(
                        plugin = %frame_plugin_id,
                        buffer = frame_buffer_id.0,
                        popup = iid,
                        "egui-mesh popup prepare: SharedMemory not registered"
                    );
                }
            }

            if let Some(t) = self.egui_mesh_popup_targets.get_mut(iid) {
                composite_mesh_target(&self.device, &self.queue, t, view, *rect, size_in_pixels);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    // 테스트 fixture 는 mesh 좌표 변환을 검증하려고 원시 정점값을 직접 만든다
    // (UI 색/좌표 "디자인" 이 아니라 합성 기하). clippy 의 정상 예외 경로.
    #![allow(clippy::disallowed_methods)]

    use super::*;
    use egui::Color32;
    use egui::emath::{Pos2, Rect};
    use egui::epaint::{Mesh, TextureId, Vertex};

    fn vtx(x: f32, y: f32) -> Vertex {
        Vertex {
            pos: Pos2::new(x, y),
            uv: Pos2::ZERO,
            color: Color32::WHITE,
        }
    }

    fn mesh_prim(clip: Rect, verts: Vec<Vertex>) -> ClippedPrimitive {
        ClippedPrimitive {
            clip_rect: clip,
            primitive: Primitive::Mesh(Mesh {
                indices: vec![0, 1, 2],
                vertices: verts,
                texture_id: TextureId::Managed(0),
            }),
        }
    }

    /// 평행이동: 모든 vertex.pos 와 clip_rect 가 surface origin 만큼 더해진다.
    #[test]
    fn offset_translates_vertices_and_clip() {
        let prims = vec![mesh_prim(
            Rect::from_min_max(Pos2::new(0.0, 0.0), Pos2::new(100.0, 80.0)),
            vec![vtx(0.0, 0.0), vtx(10.0, 0.0), vtx(0.0, 10.0)],
        )];
        // surface origin = (50, 40) points, surface = 100x80 points.
        let offset = egui::vec2(50.0, 40.0);
        let bounds = Rect::from_min_size(egui::pos2(50.0, 40.0), egui::vec2(100.0, 80.0));

        let out = offset_and_clip(&prims, offset, bounds);
        assert_eq!(out.len(), 1);
        // clip 은 (0,0)-(100,80) → translate (50,40) → (50,40)-(150,120),
        // bounds (50,40)-(150,120) 와 intersect → 동일.
        assert_eq!(out[0].clip_rect.min, Pos2::new(50.0, 40.0));
        assert_eq!(out[0].clip_rect.max, Pos2::new(150.0, 120.0));
        let Primitive::Mesh(m) = &out[0].primitive else {
            panic!("expected mesh");
        };
        assert_eq!(m.vertices[0].pos, Pos2::new(50.0, 40.0));
        assert_eq!(m.vertices[1].pos, Pos2::new(60.0, 40.0));
        assert_eq!(m.vertices[2].pos, Pos2::new(50.0, 50.0));
    }

    /// clip 이 surface 경계를 넘어가면 bounds 로 잘린다(영역 한정).
    #[test]
    fn clip_is_clamped_to_surface_bounds() {
        let prims = vec![mesh_prim(
            // surface-local 로 영역(0..200)을 넘는 clip.
            Rect::from_min_max(Pos2::new(0.0, 0.0), Pos2::new(200.0, 200.0)),
            vec![vtx(0.0, 0.0)],
        )];
        let offset = egui::vec2(50.0, 40.0);
        // surface 는 100x80 만.
        let bounds = Rect::from_min_size(egui::pos2(50.0, 40.0), egui::vec2(100.0, 80.0));

        let out = offset_and_clip(&prims, offset, bounds);
        // translate → (50,40)-(250,240), bounds (50,40)-(150,120) 로 intersect.
        assert_eq!(out[0].clip_rect.min, Pos2::new(50.0, 40.0));
        assert_eq!(out[0].clip_rect.max, Pos2::new(150.0, 120.0));
    }

    /// 디코드 출력(mesh_wire) → offset_and_clip 통합: 실제 tessellate 한 mesh 가
    /// 평행이동 후에도 mesh 로 유지되고 좌표가 origin 만큼 이동한다.
    #[test]
    fn decoded_paint_offsets_cleanly() {
        let ctx = egui::Context::default();
        let raw_input = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(200.0, 150.0))),
            ..Default::default()
        };
        let full_output = ctx.run(raw_input, |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                ui.label("egui-mesh composite");
            });
        });
        let primitives = ctx.tessellate(full_output.shapes, full_output.pixels_per_point);
        let bytes = tasty_plugin_protocol::mesh_wire::encode_paint(
            &primitives,
            &full_output.textures_delta,
            full_output.pixels_per_point,
        );
        let decoded = tasty_plugin_protocol::mesh_wire::decode_paint(&bytes).expect("decode");

        let offset = egui::vec2(300.0, 200.0);
        let bounds = Rect::from_min_size(egui::pos2(300.0, 200.0), egui::vec2(200.0, 150.0));
        let out = offset_and_clip(&decoded.primitives, offset, bounds);

        assert_eq!(out.len(), decoded.primitives.len());
        // 모든 mesh 가 유지되고, 각 vertex 가 정확히 origin offset 만큼 이동했다.
        // (AA feathering 으로 surface 밖에 약간 삐져나간 정점도 평행이동만 정확하면 OK —
        // clip_rect 가 surface 경계로 가둔다.)
        for (orig, cp) in decoded.primitives.iter().zip(&out) {
            let (Primitive::Mesh(om), Primitive::Mesh(m)) = (&orig.primitive, &cp.primitive) else {
                panic!("expected mesh after offset");
            };
            for (ov, v) in om.vertices.iter().zip(&m.vertices) {
                assert_eq!(v.pos, ov.pos + offset);
            }
            // clip 은 surface bounds 안.
            assert!(bounds.contains_rect(cp.clip_rect));
        }
    }
}
