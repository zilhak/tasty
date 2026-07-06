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
//!
//! # 텍스처 상태 수명 + delta 체인 (유실 수리)
//!
//! wire 는 delta 기반이고 SharedBuffer 는 latest-wins 라, host 가 중간 frame 을 못 보면
//! 그 frame 에 실렸던 textures_delta(font atlas full image, image plugin 비트맵 등)는
//! 영구 유실된다. 두 겹으로 막는다:
//!
//! 1. **surface 수명 귀속** — surface 의 전용 Renderer/디코드 캐시는 "화면에 보이는
//!    동안"이 아니라 "layout 에 존재하는 동안" 유지한다(`egui_mesh_surfaces_existing`).
//!    비활성 탭/비활성 workspace 의 surface 도 도착 frame 을 매 tick 디코드해 텍스처
//!    delta 를 항상 적용한다(합성만 skip) — 탭 복귀 시 재전송 왕복 없이 즉시 정상 합성.
//! 2. **frame_seq 체인 검증 + full 재전송(텍스처 delta 한정)** — frame 메타의
//!    `frame_seq`(plugin 송신 단조 시퀀스)가 `last_seq + 1` 이 아니면 중간 frame 을 놓친
//!    것: 그 frame 에 실렸던 `textures_delta` 가 유실됐으므로 **delta 를 적용하지 않는다**
//!    ([`chain_accepts`] = false). 대신 forward 측이 다음 tick 에 `need_full_textures`
//!    플래그로 set_context 를 재송신하면 plugin SDK 가 전체 텍스처 상태를 full image 로
//!    동봉한 frame(`full_textures = true`)을 보내고, host 는 full frame 을 체인과 무관하게
//!    수락하며 텍스처 상태를 리셋한다. (Context 생성 직후 첫 frame 도 자연-full 로 마킹돼
//!    bootstrap race 를 닫는다.)
//!
//! 3. **mesh(기하) 채택은 delta 체인과 분리** — reflow frame 의 mesh 는 자기완결적 기하라
//!    중간 frame 유실과 무관한데, 위 체인 가드가 mesh 까지 묶어 거부하면 리사이즈 중 seq
//!    가 튈 때 최신 폭의 mesh 가 화면에 반영되지 못하고 옛 mesh 에 고정된다(우측 잘림).
//!    따라서 seq 불연속이어도 이 frame 의 mesh 가 참조하는 텍스처가 **모두 이미 상주**하면
//!    ([`all_textures_live`]) mesh 만 채택한다(delta 는 여전히 미적용). 채택 시 `last_seq`
//!    를 갱신하므로 다음 frame 이 자연 연속이 되어 delta 게이트도 스스로 수렴한다. 참조가
//!    하나라도 미상주면(image plugin 신규 비트맵 등) mesh 도 보류하고 full 을 재요청한다.

use std::collections::HashSet;
use std::sync::atomic::Ordering;

use egui::epaint::{ClippedPrimitive, Primitive, TextureId};

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
/// **layout 에서 사라지면**(탭/pane 닫힘 — 탭 전환·workspace 전환은 해당 없음) 정리돼
/// 전용 Renderer 의 GPU 자원이 해제된다. 비가시 상태의 GPU 텍스처 상주는 의도된
/// 비용이다 — 재활성화 시 재전송 왕복 없이 즉시 합성하기 위함(모듈 doc "텍스처 상태
/// 수명 + delta 체인").
pub(super) struct EguiMeshRenderTarget {
    /// 이 surface 전용 egui_wgpu Renderer (TextureId 네임스페이스 격리).
    renderer: egui_wgpu::Renderer,
    /// 디코드된 surface-local primitives (평행이동 전 원본).
    primitives: Vec<ClippedPrimitive>,
    /// 디코드된 pixels_per_point.
    ppp: f32,
    /// 마지막으로 디코드한 footer generation.
    generation: u64,
    /// 마지막으로 **수락한** frame 의 frame_seq (plugin 송신 단조 시퀀스). 다음 frame 은
    /// `full_textures` 이거나 `frame_seq == last_seq + 1` 일 때만 수락한다(체인 검증).
    last_seq: u64,
    /// 이 Renderer 에 업로드돼 있는 TextureId 집합. full frame 수락 시 full 에 미포함된
    /// stale 텍스처를 free 하기 위해 추적한다.
    live_textures: HashSet<TextureId>,
    /// 체인 단절로 full 재전송을 요청했고 아직 full frame 을 못 받은 상태.
    /// **재요청 자체는 수락될 때까지 매 tick 재무장**한다 — 요청한 full frame 이
    /// latest-wins 버퍼에서 유실돼도 복구가 수렴하도록(single-shot deadlock 제거). 이
    /// 플래그는 로그 스팸만 억제한다(true 인 동안 재요청 로그를 내지 않음). frame 수락 시
    /// 해제되어 다음 단절 때 다시 1회 로그한다.
    awaiting_full: bool,
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
            last_seq: 0,
            live_textures: HashSet::new(),
            awaiting_full: false,
            has_content: false,
            translated: Vec::new(),
            translated_key: None,
        }
    }

    /// frame 메타만으로 이 frame 을 수락할 수 있는지 판정한다 (디코드 전 수행).
    fn chain_accepts(&self, frame_seq: u64, full_textures: bool) -> bool {
        chain_accepts(self.has_content, self.last_seq, frame_seq, full_textures)
    }
}

/// textures_delta **적용** 게이트 (frame 메타만으로 판정, 디코드 전 수행).
///
/// 이 게이트는 이제 **mesh 수락이 아니라 텍스처 delta 적용 여부**만 결정한다.
/// mesh(기하) 채택은 [`decode_mesh_into_target`] 가 디코드 후 참조-텍스처 상주 여부로
/// 별도 판정한다(참조가 전부 live 면 seq 불연속이어도 채택). 이렇게 분리해야
/// 리사이즈 중 seq 가 튀어도 최신 reflow mesh 가 화면에 반영된다.
///
/// - full frame 은 무조건 delta 적용(텍스처 상태 리셋 동반).
/// - delta frame 은 기존 콘텐츠가 있고 시퀀스가 정확히 이어질 때만 적용 —
///   latest-wins buffer 에서 중간 frame 을 못 봤다면(seq 건너뜀) 그 frame 의
///   textures_delta 가 유실된 것이므로, 적용하면 미등록/스테일 텍스처를 참조한다.
///   구버전 plugin 의 `frame_seq == 0` 도 자연히 체인 단절로 떨어진다.
fn chain_accepts(has_content: bool, last_seq: u64, frame_seq: u64, full_textures: bool) -> bool {
    full_textures || (has_content && frame_seq == last_seq.wrapping_add(1))
}

/// 디코드된 primitives 가 참조하는 모든 `TextureId` 가 이 target 에 이미 상주(live)하는가.
///
/// 체인 단절(delta 미적용) frame 이라도 이 조건을 만족하면 mesh 만 채택해도 안전하다 —
/// 렌더에 필요한 텍스처가 전부 업로드돼 있으므로 미등록 참조로 깨지지 않는다. markdown
/// 은 폰트 atlas(`Managed(0)`)만 쓰고 bootstrap full frame 에서 이미 상주하므로 항상 통과.
/// `Callback` primitive 는 decode_paint 가 제거하므로 정상 경로엔 없다(방어적으로 무시).
fn all_textures_live(prims: &[ClippedPrimitive], live: &HashSet<TextureId>) -> bool {
    prims.iter().all(|p| match &p.primitive {
        Primitive::Mesh(m) => live.contains(&m.texture_id),
        Primitive::Callback(_) => true,
    })
}

/// [`decode_mesh_into_target`] 의 결과.
enum DecodeOutcome {
    /// mesh 를 채택해 합성 캐시를 갱신했다(delta 적용 여부와 무관).
    Accepted,
    /// mesh 가 미상주 텍스처를 참조해 보류했다 — full 재전송이 필요.
    NeedsFull,
    /// 이번 frame 은 갱신 없이 넘어갔다(footer tear / ppp 불일치 등 일시적). 재요청
    /// 불필요 — 기존 캐시로 재합성하고 다음 tick 에 다시 시도한다.
    Deferred,
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

/// SharedBuffer 의 mesh 바이트를 디코드해 한 target 에 올린다(generation 변경 시 호출).
/// footer tear / 너무 작은 buffer / ppp 불일치 가드를 통과해야 갱신한다 — 실패하면 기존
/// 캐시를 유지([`DecodeOutcome::Deferred`], 이번 frame 재합성, 다음 tick 재시도).
/// surface/popup/banner 합성기가 공유한다.
///
/// # mesh 채택과 텍스처 delta 적용의 분리
///
/// `chain_ok`([`chain_accepts`])는 **텍스처 delta 적용 여부**만 결정한다:
/// - `chain_ok`(full 또는 seq 연속): `textures_delta.set/free` 를 renderer 에 반영한다.
///   `full_textures` frame 은 plugin 의 전체 텍스처 상태를 담으므로 full 에 미포함된
///   기존 텍스처를 free 해 target 상태를 plugin 상태와 일치시킨다.
/// - `!chain_ok`(seq 점프 — 중간 frame 의 delta 유실): delta 를 적용하면 미등록/스테일
///   텍스처를 참조하므로 **적용하지 않는다.** 대신 이 frame 의 mesh 가 참조하는 텍스처가
///   모두 이미 상주하면([`all_textures_live`]) mesh(기하)만 채택한다. 하나라도 미상주면
///   보류하고 [`DecodeOutcome::NeedsFull`] 로 full 재전송을 요청하게 한다.
///
/// mesh 채택 시 `last_seq` 를 채택한 frame_seq 로 갱신하므로 다음 frame 이 seq+1 로 자연
/// 연속이 되어 delta 게이트가 다시 통과한다(체인 self-heal). markdown 은 참조가 항상
/// 폰트 atlas(상주)라 즉시 회복한다.
#[allow(clippy::too_many_arguments)] // reason: frame 디코드 컨텍스트 전체
fn decode_mesh_into_target(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    target: &mut EguiMeshRenderTarget,
    raw: &[u8],
    generation: u64,
    frame_seq: u64,
    full_textures: bool,
    chain_ok: bool,
    host_ppp: f32,
    log_id: &str,
) -> DecodeOutcome {
    // footer 길이 가드: plugin 이 footer(8B) 미만 버퍼를 등록하면 footer::load 의
    // 안전 전제(raw.len >= SIZE)가 깨진다. 검증 실패 시 해당 frame 은 skip.
    if raw.len() < tasty_shm::footer::SIZE {
        tracing::warn!(
            target = log_id,
            actual = raw.len(),
            "egui-mesh prepare: buffer too small"
        );
        return DecodeOutcome::Deferred;
    }
    // SAFETY: SharedMemory 영역. tasty-shm 동기화 규약 — plugin commit(Release) →
    // host Acquire-load → user data read. footer 시작 8B 는 mmap 페이지 align 이라 8B aligned.
    // footer generation 이 frame 메타와 다르면 half-painted 이므로 다음 frame 까지 미룬다.
    let gen_now = unsafe { tasty_shm::footer::load(raw, Ordering::Acquire) };
    if gen_now != generation {
        return DecodeOutcome::Deferred;
    }
    let user = tasty_shm::footer::user_slice(raw);

    let decoded = match tasty_plugin_protocol::mesh_wire::decode_paint(user) {
        Ok(d) => d,
        Err(e) => {
            tracing::warn!(target = log_id, "egui-mesh prepare: decode failed: {e}");
            return DecodeOutcome::Deferred;
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
        return DecodeOutcome::Deferred;
    }

    if chain_ok {
        // 텍스처 delta 적용 (체인 연속 또는 full).
        // full frame: plugin 의 전체 텍스처 상태로 리셋한다 — full 에 미포함된(=plugin 이
        // 그 사이 free 한, 또는 이 target 이 모르는 사이 대체된) 기존 텍스처를 free.
        if full_textures {
            let full_ids: HashSet<TextureId> = decoded
                .textures_delta
                .set
                .iter()
                .map(|(id, _)| *id)
                .collect();
            for stale in target.live_textures.difference(&full_ids) {
                target.renderer.free_texture(stale);
            }
            target.live_textures = full_ids;
        }

        // 전용 Renderer 라 TextureId 가 host 와 충돌하지 않는다. set → (이후 render) → free
        // 순서로 frame 경계에서 원자 처리. free 대상은 primitives 가 참조하지 않으므로
        // render 전에 풀어도 안전(set/free 는 서로소).
        for (id, delta) in &decoded.textures_delta.set {
            target.renderer.update_texture(device, queue, *id, delta);
            target.live_textures.insert(*id);
        }
        for id in &decoded.textures_delta.free {
            target.renderer.free_texture(id);
            target.live_textures.remove(id);
        }
    } else {
        // 체인 단절(seq 점프): 유실된 delta 로 인한 스테일/미등록 텍스처 오염을 막기 위해
        // delta 를 적용하지 않는다. 첫 콘텐츠는 반드시 full 로 받아야 안전하므로(부트스트랩
        // 텍스처 상주 보장) 아직 콘텐츠가 없으면 채택하지 않는다. 참조 텍스처가 하나라도
        // 미상주면 mesh 도 보류하고 full 재전송을 요청한다.
        if !target.has_content || !all_textures_live(&decoded.primitives, &target.live_textures) {
            return DecodeOutcome::NeedsFull;
        }
        // 참조가 전부 상주 — delta/live_textures 는 건드리지 않고 mesh 만 채택한다.
    }

    target.primitives = decoded.primitives;
    target.ppp = decoded.pixels_per_point;
    target.generation = generation;
    target.last_seq = frame_seq;
    target.awaiting_full = false;
    target.has_content = true;
    target.translated_key = None; // 평행이동 캐시 무효화.
    DecodeOutcome::Accepted
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
    ///
    /// - `targets`: **보이는**(활성 workspace 의 활성 탭) surface — 합성 대상.
    /// - `existing`: layout 에 **존재하는** 전체 egui-mesh surface (전 workspace, 비활성
    ///   탭 포함) — 텍스처 상태 수명 + 디코드 대상. 비가시 surface 의 도착 frame 도
    ///   디코드해 textures_delta 체인을 유지한다(합성만 skip, 모듈 doc 참조).
    pub(super) fn render_egui_mesh_surfaces(
        &mut self,
        view: &wgpu::TextureView,
        targets: &[(u32, String, PhysicalRect)],
        existing: &[(u32, String)],
        plugin_manager: &PluginManager,
    ) {
        // layout 에서 사라진(닫힌) surface 의 전용 Renderer 정리 (GPU 자원 해제).
        // 탭 전환/workspace 전환으로 안 보이게 된 것은 existing 에 남아 있어 보존된다.
        let live: HashSet<u32> = existing.iter().map(|t| t.0).collect();
        self.egui_mesh_targets.retain(|sid, _| live.contains(sid));

        // 디코드 pass — 비가시 surface 포함. frame 이 도착해 있으면 항상 최신 상태로
        // 디코드해 텍스처 delta 를 적용한다(체인 유지). 체인이 끊긴 frame 은 수락하지
        // 않고 full 재전송을 요청한다.
        let host_ppp = self.scale_factor;
        for (sid, plugin_id) in existing {
            // plugin 이 아직 frame 을 안 보냈거나 disconnect 로 정리됐으면 할 일이 없다.
            // (disconnect 시 placeholder/blank — 캐시가 남아 있어도 합성하지 않는다, §9-7.)
            let Some(frame) = plugin_manager.egui_mesh_frame(*sid) else {
                continue;
            };
            let frame_buffer_id = frame.buffer_id;
            let frame_generation = frame.generation;
            let frame_seq = frame.frame_seq;
            let frame_full = frame.full_textures;

            // 전용 Renderer ensure.
            if !self.egui_mesh_targets.contains_key(sid) {
                let renderer =
                    egui_wgpu::Renderer::new(&self.device, self.config.format, None, 1, false);
                self.egui_mesh_targets
                    .insert(*sid, EguiMeshRenderTarget::new(renderer));
            }

            // generation 이 바뀌었거나 아직 콘텐츠가 없을 때만 디코드.
            let (needs_decode, chain_ok, awaiting) = {
                let t = &self.egui_mesh_targets[sid];
                (
                    !t.has_content || t.generation != frame_generation,
                    t.chain_accepts(frame_seq, frame_full),
                    t.awaiting_full,
                )
            };
            if !needs_decode {
                continue;
            }
            if let Some(mem) = plugin_manager.plugin_buffer(plugin_id, frame_buffer_id) {
                // SAFETY: tasty-shm 동기화 규약 (decode_mesh_into_target 내 Acquire-load).
                let raw = unsafe { mem.as_slice() };
                // mesh 채택은 chain 연속과 분리 — seq 점프여도 참조 텍스처가 상주하면 최신
                // reflow mesh 를 채택한다. delta 는 chain_ok 일 때만 적용(decode 내부).
                let outcome = decode_mesh_into_target(
                    &self.device,
                    &self.queue,
                    self.egui_mesh_targets.get_mut(sid).expect("ensured above"),
                    raw,
                    frame_generation,
                    frame_seq,
                    frame_full,
                    chain_ok,
                    host_ppp,
                    plugin_id,
                );
                if matches!(outcome, DecodeOutcome::NeedsFull) {
                    // 미상주 텍스처 참조 — mesh 도 보류. full 재전송을 요청한다. 요청한
                    // full frame 이 latest-wins 버퍼에서 유실될 수 있으므로 수락될 때까지
                    // 매 tick 재무장한다(single-shot deadlock 제거). 로그는 최초 1 회만.
                    if !awaiting {
                        tracing::debug!(
                            surface = sid,
                            frame_seq,
                            "egui-mesh prepare: texture delta chain broken; requesting full resend"
                        );
                        self.egui_mesh_targets
                            .get_mut(sid)
                            .expect("ensured above")
                            .awaiting_full = true;
                    }
                    self.egui_mesh_full_requests.insert(*sid);
                }
            } else {
                tracing::warn!(
                    plugin = %plugin_id,
                    buffer = frame_buffer_id.0,
                    surface = sid,
                    "egui-mesh prepare: SharedMemory not registered"
                );
            }
        }

        // 합성 pass — 보이는 surface 만 (framebuffer 가 매 frame clear 되므로 매 frame).
        let size_in_pixels = [self.size.width, self.size.height];
        for (sid, _plugin_id, rect) in targets {
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
            let frame_seq = frame.frame_seq;
            let frame_full = frame.full_textures;
            let host_ppp = self.scale_factor;

            if !self.egui_mesh_popup_targets.contains_key(iid) {
                let renderer =
                    egui_wgpu::Renderer::new(&self.device, self.config.format, None, 1, false);
                self.egui_mesh_popup_targets
                    .insert(*iid, EguiMeshRenderTarget::new(renderer));
            }

            let (needs_decode, chain_ok, awaiting) = {
                let t = &self.egui_mesh_popup_targets[iid];
                (
                    !t.has_content || t.generation != frame_generation,
                    t.chain_accepts(frame_seq, frame_full),
                    t.awaiting_full,
                )
            };
            if needs_decode {
                if let Some(mem) = plugin_manager.plugin_buffer(&frame_plugin_id, frame_buffer_id) {
                    // SAFETY: tasty-shm 동기화 규약 (decode_mesh_into_target 내 Acquire-load).
                    let raw = unsafe { mem.as_slice() };
                    // mesh 채택과 delta 적용 분리 (surface 와 동형).
                    let outcome = decode_mesh_into_target(
                        &self.device,
                        &self.queue,
                        self.egui_mesh_popup_targets
                            .get_mut(iid)
                            .expect("ensured above"),
                        raw,
                        frame_generation,
                        frame_seq,
                        frame_full,
                        chain_ok,
                        host_ppp,
                        &frame_plugin_id,
                    );
                    if matches!(outcome, DecodeOutcome::NeedsFull) {
                        // 체인 단절 + 미상주 텍스처 참조 — full 재전송을 매 tick 재무장
                        // 요청한다(surface 동형). 로그는 최초 1 회만.
                        if !awaiting {
                            tracing::debug!(
                                popup = iid,
                                frame_seq,
                                "egui-mesh popup prepare: texture delta chain broken; requesting full resend"
                            );
                            self.egui_mesh_popup_targets
                                .get_mut(iid)
                                .expect("ensured above")
                                .awaiting_full = true;
                        }
                        self.egui_mesh_popup_full_requests.insert(*iid);
                    }
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

    /// egui-mesh banner(A3) 들을 전용 Renderer 로 합성한다.
    ///
    /// [`render_egui_mesh_popups`] 의 banner 대응 — [`GpuState::render`] 가 host egui pass
    /// *후* 부른다. banner 셸(컨테이너/border/close X/카운트다운)을 host egui(banner
    /// manager)가 그린 뒤 그 위 content_rect 에 plugin mesh 를 얹는다. mesh 는 content_rect
    /// 로 clip 되므로 셸/affordance 를 덮지 않는다. target 맵을 instance_id 로 키잉하고 frame
    /// meta 를 `banner_mesh_frame` 에서 읽는다.
    ///
    /// [`render_egui_mesh_popups`]: Self::render_egui_mesh_popups
    pub(super) fn render_egui_mesh_banners(
        &mut self,
        view: &wgpu::TextureView,
        regions: &[(u64, PhysicalRect)],
        plugin_manager: &PluginManager,
    ) {
        // 닫힌 banner 의 전용 Renderer 정리 (GPU 자원 해제).
        let live: HashSet<u64> = regions.iter().map(|r| r.0).collect();
        self.egui_mesh_banner_targets
            .retain(|iid, _| live.contains(iid));

        let size_in_pixels = [self.size.width, self.size.height];
        for (iid, rect) in regions {
            let Some(frame) = plugin_manager.banner_mesh_frame(*iid) else {
                continue;
            };
            let frame_plugin_id = frame.plugin_id.clone();
            let frame_buffer_id = frame.buffer_id;
            let frame_generation = frame.generation;
            let frame_seq = frame.frame_seq;
            let frame_full = frame.full_textures;
            let host_ppp = self.scale_factor;

            if !self.egui_mesh_banner_targets.contains_key(iid) {
                let renderer =
                    egui_wgpu::Renderer::new(&self.device, self.config.format, None, 1, false);
                self.egui_mesh_banner_targets
                    .insert(*iid, EguiMeshRenderTarget::new(renderer));
            }

            let (needs_decode, chain_ok, awaiting) = {
                let t = &self.egui_mesh_banner_targets[iid];
                (
                    !t.has_content || t.generation != frame_generation,
                    t.chain_accepts(frame_seq, frame_full),
                    t.awaiting_full,
                )
            };
            if needs_decode {
                if let Some(mem) = plugin_manager.plugin_buffer(&frame_plugin_id, frame_buffer_id) {
                    // SAFETY: tasty-shm 동기화 규약 (decode_mesh_into_target 내 Acquire-load).
                    let raw = unsafe { mem.as_slice() };
                    // mesh 채택과 delta 적용 분리 (surface 와 동형).
                    let outcome = decode_mesh_into_target(
                        &self.device,
                        &self.queue,
                        self.egui_mesh_banner_targets
                            .get_mut(iid)
                            .expect("ensured above"),
                        raw,
                        frame_generation,
                        frame_seq,
                        frame_full,
                        chain_ok,
                        host_ppp,
                        &frame_plugin_id,
                    );
                    if matches!(outcome, DecodeOutcome::NeedsFull) {
                        // 체인 단절 + 미상주 텍스처 참조 — full 재전송을 매 tick 재무장
                        // 요청한다(surface 동형). 로그는 최초 1 회만.
                        if !awaiting {
                            tracing::debug!(
                                banner = iid,
                                frame_seq,
                                "egui-mesh banner prepare: texture delta chain broken; requesting full resend"
                            );
                            self.egui_mesh_banner_targets
                                .get_mut(iid)
                                .expect("ensured above")
                                .awaiting_full = true;
                        }
                        self.egui_mesh_banner_full_requests.insert(*iid);
                    }
                } else {
                    tracing::warn!(
                        plugin = %frame_plugin_id,
                        buffer = frame_buffer_id.0,
                        banner = iid,
                        "egui-mesh banner prepare: SharedMemory not registered"
                    );
                }
            }

            if let Some(t) = self.egui_mesh_banner_targets.get_mut(iid) {
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

    /// delta 적용 게이트: full 은 무조건, delta 는 콘텐츠 보유 + 정확한 seq 연속일 때만.
    /// (이 게이트는 이제 mesh 수락이 아니라 텍스처 delta 적용 여부만 결정한다 — mesh 채택은
    /// `all_textures_live` 로 별도 판정.)
    #[test]
    fn chain_accepts_full_or_contiguous_seq_only() {
        // 신규 target (콘텐츠 없음): full 만 수락.
        assert!(!chain_accepts(false, 0, 1, false));
        assert!(chain_accepts(false, 0, 1, true));
        assert!(chain_accepts(false, 0, 7, true));

        // 콘텐츠 보유: seq 가 정확히 이어지면 수락.
        assert!(chain_accepts(true, 5, 6, false));
        // seq 건너뜀(중간 frame 관측 누락) → 거부.
        assert!(!chain_accepts(true, 5, 7, false));
        // 과거/동일 seq (재관측) → 거부.
        assert!(!chain_accepts(true, 5, 5, false));
        // full 은 체인과 무관하게 수락 (회복 경로).
        assert!(chain_accepts(true, 5, 9, true));

        // 구버전 plugin (frame_seq 항상 0, full 마킹 없음) → 항상 체인 단절.
        assert!(!chain_accepts(true, 0, 0, false));
    }

    fn mesh_prim_tex(tex: TextureId) -> ClippedPrimitive {
        ClippedPrimitive {
            clip_rect: Rect::from_min_max(Pos2::ZERO, Pos2::new(10.0, 10.0)),
            primitive: Primitive::Mesh(Mesh {
                indices: vec![0, 1, 2],
                vertices: vec![vtx(0.0, 0.0), vtx(1.0, 0.0), vtx(0.0, 1.0)],
                texture_id: tex,
            }),
        }
    }

    /// mesh 채택 판정: 참조 텍스처가 전부 상주(live)할 때만 seq 점프 frame 을 채택할 수
    /// 있다. 이 규칙이 delta 체인과 분리된 mesh 수락 게이트를 고정한다(리사이즈 reflow).
    #[test]
    fn all_textures_live_requires_every_ref_present() {
        let font = TextureId::Managed(0);
        let image = TextureId::Managed(7);
        let mut live = HashSet::new();
        live.insert(font);

        // 폰트 atlas 만 참조 — markdown 정상 케이스 → 상주하므로 채택 가능.
        let prims = vec![mesh_prim_tex(font), mesh_prim_tex(font)];
        assert!(all_textures_live(&prims, &live));

        // 미상주 텍스처(신규 비트맵) 참조 → 채택 불가(full 대기).
        let prims = vec![mesh_prim_tex(font), mesh_prim_tex(image)];
        assert!(!all_textures_live(&prims, &live));

        // 참조 없는(빈) frame → 공허 참으로 통과(합성 시 빈 mesh 는 조기 return).
        assert!(all_textures_live(&[], &live));

        // image 도 상주하면 둘 다 참조해도 채택 가능.
        live.insert(image);
        let prims = vec![mesh_prim_tex(font), mesh_prim_tex(image)];
        assert!(all_textures_live(&prims, &live));
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
