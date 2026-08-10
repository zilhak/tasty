//! egui-mesh plugin SDK 헬퍼 (A1-S4).
//!
//! plugin 프로세스가 **자기 egui [`Context`] 와 폰트 atlas 를 소유**하고, host 가 보낸
//! `surface.set_context`([`SurfaceSetContextParams`]) 를 받아 egui 를 구동·tessellate 한 뒤
//! POD 바이트로 인코드해 shared buffer 로 host 에 회신하는 흐름을 은닉한다. plugin 작성자는
//! UI closure(`|egui_ctx| { egui::CentralPanel... }`) 만 구현하면 된다.
//!
//! ## 흐름 (research-a1 §2-1·§2-5)
//! ```text
//! host → plugin: surface.set_context { width_px, height_px, ppp, raw_input }
//!   │  [`build_raw_input`]: 물리 px → 논리 포인트, RawInputWire → egui RawInput
//!   ▼
//! ctx.run(raw_input, run_ui)  ── plugin 이 자기 Fonts/atlas 소유
//!   │  FullOutput { shapes, textures_delta, pixels_per_point }
//!   ▼
//! ctx.tessellate(shapes, ppp) → Vec<ClippedPrimitive>   (tessellate 는 plugin 이 수행)
//!   │  mesh_wire::encode_paint(§3) → POD 바이트
//!   ▼
//! SharedBuffer write + commit(Release)
//! plugin → host: PluginEvent::PaintFrame { surface_id, buffer_id, generation }
//! ```
//!
//! ## generation (invalidate 시에만 송신)
//! 정적 화면은 매 frame 무조건 보내지 않는다. [`EguiMeshSurface::run_frame`] 은 인코드된
//! 바이트의 해시를 직전 frame 과 비교해, **출력이 바뀐 frame 만** 송신한다(host 는 그래도
//! footer generation 비교로 한 번 더 거른다).
//!
//! ## 폰트
//! 기본은 egui `default_fonts`(plugin 소유 atlas). host 와의 폰트 parity 가 필요하면
//! [`EguiMeshSurface::context`] 로 [`Context`] 를 받아 `set_fonts` 로 동일 폰트를 설치한다
//! (B1 markdown 이식 단계의 관심사).

use std::collections::BTreeMap;
use std::hash::{Hash, Hasher};
use std::time::Duration;

use egui::epaint::textures::{TextureOptions, TexturesDelta};
use egui::epaint::{ImageData, ImageDelta, TextureId};
use egui::{
    Context, Event, ImeEvent, Key, Modifiers, MouseWheelUnit, OutputCommand, PointerButton, Pos2,
    RawInput, Rect, vec2,
};
use tasty_plugin_protocol::mesh_wire::encode_paint;
use tasty_plugin_protocol::{
    BannerSetContextParams, ImeWire, ModifiersWire, PointerButtonWire, PopupSetContextParams,
    RawInputEventWire, RawInputWire, SurfaceSetContextParams, ThemeWire,
};

#[cfg(any(unix, windows))]
use tasty_plugin_protocol::{PluginEvent, SharedBufferId};

#[cfg(any(unix, windows))]
use crate::error::PluginError;
#[cfg(any(unix, windows))]
use crate::host::HostHandle;
#[cfg(any(unix, windows))]
use crate::shared_buffer::SharedBuffer;

/// egui-mesh 렌더 코어 — surface/popup 공통. 자기 egui [`Context`](폰트 atlas 포함),
/// 직전 출력 해시(invalidate 판정), shared buffer(unix) 를 들고 있다. surface 와 popup 은
/// 회신 알림(`PaintFrame` vs `PopupPaintFrame`)만 다르고 run_frame/commit 로직은 동일해
/// 이 코어를 공유한다([`EguiMeshSurface`] / [`EguiMeshPopup`] 가 얇게 감싼다).
struct EguiMeshCore {
    ctx: Context,
    /// 직전 frame 에 인코드한 mesh 바이트의 해시. 같으면 정적 화면으로 보고 송신 생략.
    last_hash: Option<u64>,
    /// 직전 set_context 의 렌더 컨텍스트(geom/ppp/theme). out-of-band 상태 변경 뒤
    /// [`repaint_last`](Self::repaint_last)가 **빈 입력**으로 재-run 할 때 재사용한다.
    /// 첫 set_context 도착 전엔 `None` — 그 때 repaint 는 no-op.
    last_ctx: Option<CachedContext>,
    /// 이 코어가 host 로 보낸 텍스처들의 누적 full 상태 (id → full image + options).
    /// wire 는 delta 기반 + latest-wins buffer 라 host 가 중간 frame 을 놓칠 수 있다 —
    /// host 가 `need_full_textures` 로 복구를 요청하면 이 상태를 full image delta 로
    /// 재구성해 동봉한다(font atlas 포함, image plugin 비트맵 등 임의 Managed 텍스처 전부).
    /// `BTreeMap` — full 재구성 인코딩의 결정적 순서(해시 dedup 안정성).
    tex_state: BTreeMap<TextureId, (ImageData, TextureOptions)>,
    /// 지금까지 **송신한** frame 수(= 마지막 송신 frame 의 seq, 1부터). footer generation
    /// 과 달리 shared buffer 재생성(성장)과 무관하게 이어지는 단조 시퀀스로, host 가
    /// textures_delta 체인의 연속성(`frame_seq == last + 1`)을 검증하는 데 쓰인다.
    frame_seq: u64,
    /// mesh POD 블록을 쓰는 shared buffer. 필요 크기보다 작아지면 재생성한다.
    #[cfg(any(unix, windows))]
    buffer: Option<SharedBuffer>,
    /// 직전 `render()` 가 egui `viewport_output` 에서 읽은 self-repaint 요청(있다면).
    /// egui 의 스크롤 스무딩 등 다중 프레임 애니메이션이 `ctx.request_repaint_after`로
    /// "다음 pass 도 그려달라"고 신호하면 여기 채워진다. 이 채널은 host-side 이벤트가
    /// 있을 때만 pass 를 구동하므로(`docs/dev-guide/egui-mesh-channel.md` "set_context
    /// 송신 정책"), 이 신호를 누군가 읽어 host 에 재-invalidate 를 요청하지 않으면
    /// 유휴 상태에서 애니메이션이 방치된다([`EguiMeshSurface`]가 소비).
    pending_self_repaint: Option<Duration>,
    /// 직전 `render()` 의 `platform_output.commands` 중 `OutputCommand::CopyText` —
    /// `Event::Copy` 를 처리한 frame 에서 plugin 자신의 텍스트 선택(selectable label /
    /// `TextEdit`)이 있었을 때만 채워진다. 클립보드 기록은 plugin 이 직접 한다
    /// (ADR-0009) — 이 필드는 그 값을 host round-trip 없이 plugin 코드로 넘겨주는
    /// 통로일 뿐이다.
    last_copied_text: Option<String>,
}

/// 한 번의 렌더가 만든 송신 후보 frame — 인코드된 mesh 바이트 + full 마킹.
struct MeshFrame {
    bytes: Vec<u8>,
    /// 이 frame 의 textures_delta 가 누적 텍스처 상태 **전체**를 full image 로 담는가.
    /// (첫 frame 은 자연-full, `need_full_textures` 응답은 합성-full.) host 는 full
    /// frame 을 체인 연속성과 무관하게 수락하고 자기 텍스처 상태를 리셋한다.
    full_textures: bool,
}

/// 직전 set_context 의 재-paint 재현에 필요한 host-side 컨텍스트 스냅샷.
/// **입력 이벤트는 담지 않는다** — 재-paint 는 identity 불변식상 빈 events 로만 한다
/// (가짜 사용자 입력 무주입). `focused` 는 이벤트가 아니라 지속 상태라 캐시해 재현한다
/// (identity 불변식 무위반) — 재-paint 프레임이 focused=false 로 떨어지면 포커스 의존
/// UI(커서·드롭다운·focused 배경)가 재-paint 에서만 퇴행하는 결함이 생긴다. theme 은
/// plugin 의 draw closure 가 캐시된 값을 다시 쓸 수 있도록 [`EguiMeshCore::last_theme`]
/// 로 노출한다.
struct CachedContext {
    width_px: u32,
    height_px: u32,
    ppp: f32,
    focused: bool,
    theme: Option<ThemeWire>,
}

impl EguiMeshCore {
    fn new() -> Self {
        let ctx = Context::default();
        // egui 내장 키보드 줌(Cmd/Ctrl +/-/0)을 끈다 — 배율은 host 가 ui_zoom(설정) +
        // native ppp 로 제어한다. 켜두면 forward 된 Cmd+= 등이 plugin Context 의
        // zoom_factor 를 올려 메모리에 눌러앉고, host 와 달리 리셋 경로가 없어 서피스가
        // 예기치 않게 확대된 채 유지된다(본체 `gpu.rs` 도 동일 이유로 끈다).
        ctx.options_mut(|opts| {
            opts.zoom_with_keyboard = false;
        });
        Self {
            ctx,
            last_hash: None,
            last_ctx: None,
            tex_state: BTreeMap::new(),
            frame_seq: 0,
            #[cfg(any(unix, windows))]
            buffer: None,
            pending_self_repaint: None,
            last_copied_text: None,
        }
    }

    /// 한 frame 을 그려 POD mesh 바이트를 만든다. 출력이 직전과 byte 단위로 동일하면
    /// (정적 화면) `None` — 호출자는 송신을 생략한다. `need_full` 이면 dedup 을 우회하고
    /// 누적 텍스처 상태 전체를 full 로 동봉한다(host 텍스처 상태 복구).
    #[allow(clippy::too_many_arguments)] // reason: set_context 렌더 컨텍스트 전체
    fn run_frame(
        &mut self,
        width_px: u32,
        height_px: u32,
        ppp: f32,
        theme: Option<&ThemeWire>,
        raw_input: &RawInputWire,
        need_full: bool,
        run_ui: impl FnMut(&Context),
    ) -> Option<MeshFrame> {
        // out-of-band 재-paint 가 재현할 수 있도록 이번 컨텍스트를 캐시한다
        // (입력 이벤트 제외 — focused 는 지속 상태라 포함).
        self.last_ctx = Some(CachedContext {
            width_px,
            height_px,
            ppp,
            focused: raw_input.focused,
            theme: theme.cloned(),
        });
        let raw = build_raw_input(width_px, height_px, ppp, raw_input);
        self.render(raw, need_full, run_ui)
    }

    /// 마지막 캐시된 컨텍스트(geom/ppp/focused)로 **빈 이벤트 + 직전 focused 보존** 재-run
    /// 한다. plugin 이 out-of-band 로 상태를 바꾼 뒤 화면을 갱신할 때 쓴다. 첫 set_context
    /// 전(캐시 없음)이면 `None`(no-op). 출력이 직전과 동일하면(상태 변경이 화면에 안 걸림)
    /// `None`.
    ///
    /// identity 불변식: 재-run 의 `raw_input.events` 는 빈 배열 — 가짜 사용자 입력을
    /// 주입하지 않는다. `focused` 는 이벤트가 아니라 지속 상태라 직전 set_context 값을
    /// 그대로 재현한다(false 로 떨어뜨리면 `has_focus()` 게이트가 커서·드롭다운 등
    /// 포커스 의존 UI 를 재-paint 프레임에서만 퇴행시킨다).
    fn repaint_last(&mut self, run_ui: impl FnMut(&Context)) -> Option<MeshFrame> {
        let (width_px, height_px, ppp, focused) = {
            let c = self.last_ctx.as_ref()?;
            (c.width_px, c.height_px, c.ppp, c.focused)
        };
        let wire = RawInputWire {
            focused,
            ..Default::default()
        };
        let raw = build_raw_input(width_px, height_px, ppp, &wire);
        self.render(raw, false, run_ui)
    }

    /// 직전 set_context 의 theme 스냅샷. plugin 의 재-paint closure 가 캐시된 theme 으로
    /// 다시 그릴 수 있도록 노출한다. 첫 set_context 전이거나 theme 미동봉이면 `None`.
    fn last_theme(&self) -> Option<&ThemeWire> {
        self.last_ctx.as_ref().and_then(|c| c.theme.as_ref())
    }

    /// 직전 `render()` 가 egui `viewport_output` 에서 읽은 self-repaint 지연(있다면).
    /// egui 의 스크롤 스무딩(`unprocessed_scroll_delta` drain, egui 0.31
    /// `input_state/mod.rs`) 등 다중 프레임 애니메이션이 아직 안 끝났으면 `Duration`
    /// 이 채워진다(0 이면 즉시). 완전히 안정되면 `None`.
    fn pending_self_repaint(&self) -> Option<Duration> {
        self.pending_self_repaint
    }

    /// 직전 `render()` 가 처리한 `Event::Copy` 로 텍스트 선택이 복사됐다면 그 문자열을
    /// 1회 소비(take)해 반환한다. 선택이 없었거나 `Event::Copy` 가 없던 frame 이면 `None`.
    fn take_copied_text(&mut self) -> Option<String> {
        self.last_copied_text.take()
    }

    /// egui 를 구동·tessellate·encode 하고 직전 출력과 해시 비교로 dedup 한다.
    /// 출력이 직전과 byte 단위로 동일하면 `None`(송신 생략). `run_frame`/`repaint_last` 공용.
    ///
    /// `need_full` 이면 이 frame 의 delta 대신 누적 텍스처 상태 **전체**를 full image 로
    /// 동봉하고 dedup 을 우회한다 — host 가 텍스처 상태를 잃었다고 알린 상황이므로,
    /// 출력 바이트가 직전과 같더라도 반드시 재송신해야 회복된다.
    fn render(
        &mut self,
        raw: RawInput,
        need_full: bool,
        run_ui: impl FnMut(&Context),
    ) -> Option<MeshFrame> {
        let full = self.ctx.run(raw, run_ui);
        // 이번 pass 가 `Event::Copy` 를 처리해 텍스트 선택을 복사했다면 채워진다(egui
        // 내장 selectable-label/TextEdit 복사 로직 — `OutputCommand::CopyText` 로
        // 나온다, 옛 `PlatformOutput::copied_text` 필드는 deprecated). 매 render() 마다
        // 갱신되므로 다음 pass 에 선택이 없으면 자연히 지워진다.
        self.last_copied_text = full.platform_output.commands.iter().find_map(|c| match c {
            OutputCommand::CopyText(text) if !text.is_empty() => Some(text.clone()),
            _ => None,
        });
        // egui 가 이번 pass 에서 추가 pass 를 요청했는지 읽어둔다 — dedup(아래) 으로
        // 이번 frame 이 송신 생략되더라도 유실되지 않도록 매 render() 마다 갱신한다.
        // egui-mesh 는 단일 ROOT viewport 만 쓴다(`build_raw_input` 이 viewport_id 를
        // 항상 기본값 ROOT 로 둔다) — `Duration::MAX` 는 egui 관례상 "요청 없음".
        self.pending_self_repaint = full
            .viewport_output
            .get(&egui::ViewportId::ROOT)
            .map(|v| v.repaint_delay)
            .filter(|d| *d < Duration::MAX);
        // tessellate 는 plugin 이 수행한다(폰트 atlas 를 plugin 이 소유, research-a1 §2-1).
        let primitives = self.ctx.tessellate(full.shapes, full.pixels_per_point);
        // 이 frame 의 delta 를 누적 상태에 먼저 반영한다 — full 재구성은 항상 최신 상태 기준.
        let naturally_full = self.accumulate_textures(&full.textures_delta);
        let (bytes, full_textures) = if need_full {
            (
                encode_paint(
                    &primitives,
                    &self.full_texture_delta(),
                    full.pixels_per_point,
                ),
                true,
            )
        } else {
            (
                encode_paint(&primitives, &full.textures_delta, full.pixels_per_point),
                naturally_full,
            )
        };

        let hash = hash_bytes(&bytes);
        if !need_full && self.last_hash == Some(hash) {
            return None;
        }
        self.last_hash = Some(hash);
        Some(MeshFrame {
            bytes,
            full_textures,
        })
    }

    /// 이 frame 의 textures_delta 를 누적 상태에 적용하고, delta 가 누적 상태 **전체**를
    /// full image(`pos == None`)로 담고 있는지(자연-full frame — 예: Context 생성 후 첫
    /// frame) 반환한다.
    fn accumulate_textures(&mut self, delta: &TexturesDelta) -> bool {
        for (id, d) in &delta.set {
            match d.pos {
                None => {
                    self.tex_state.insert(*id, (d.image.clone(), d.options));
                }
                Some(pos) => match self.tex_state.get_mut(id) {
                    Some((base, options)) => {
                        *options = d.options;
                        patch_image(base, &d.image, pos);
                    }
                    // egui 는 full image 를 먼저 보낸 텍스처에만 patch 를 낸다 —
                    // base 없는 patch 는 계약 위반이므로 기록만 하고 버린다.
                    None => tracing::warn!(
                        "egui-mesh: texture patch for unknown texture {id:?}; dropping"
                    ),
                },
            }
        }
        for id in &delta.free {
            self.tex_state.remove(id);
        }
        self.tex_state.keys().all(|id| {
            delta
                .set
                .iter()
                .any(|(sid, sd)| sid == id && sd.pos.is_none())
        })
    }

    /// 누적 텍스처 상태 전체를 full image delta 로 재구성한다 (`need_full_textures` 응답).
    fn full_texture_delta(&self) -> TexturesDelta {
        TexturesDelta {
            set: self
                .tex_state
                .iter()
                .map(|(id, (image, options))| {
                    (
                        *id,
                        ImageDelta {
                            image: image.clone(),
                            options: *options,
                            pos: None,
                        },
                    )
                })
                .collect(),
            free: Vec::new(),
        }
    }

    /// 송신 확정된 frame 의 시퀀스를 발급한다(1부터 단조 증가). commit 성공 후에만
    /// 호출해 "송신된 frame 수" 와 어긋나지 않게 한다.
    #[cfg(any(unix, windows))]
    fn next_frame_seq(&mut self) -> u64 {
        self.frame_seq += 1;
        self.frame_seq
    }

    /// 인코드된 바이트를 shared buffer 에 commit 하고 (buffer_id, generation) 을 돌려준다.
    /// 회신 알림(PaintFrame/PopupPaintFrame)은 호출자가 보낸다.
    #[cfg(any(unix, windows))]
    fn commit(
        &mut self,
        host: &HostHandle,
        bytes: &[u8],
    ) -> Result<(SharedBufferId, u64), PluginError> {
        self.ensure_buffer(host, bytes.len())?;
        let buffer = self
            .buffer
            .as_ref()
            .expect("ensure_buffer guarantees a buffer");

        // SAFETY: 이 buffer 는 코어가 단독 소유한다(동시 mutate 없음). host 는 commit 의
        // generation footer(fetch_add Release)로 half-painted frame 을 거른다.
        unsafe {
            buffer.as_mut_slice()[..bytes.len()].copy_from_slice(bytes);
        }
        buffer.commit(None)?;
        Ok((buffer.id(), buffer.generation()))
    }

    /// shared buffer 가 `needed` 바이트를 담을 수 있게 보장한다. 부족하면 헤드룸을 둔
    /// 크기로 새로 만든다(폰트 atlas 가 큰 첫 frame spike 를 흡수, 매 frame 재생성 방지).
    #[cfg(any(unix, windows))]
    fn ensure_buffer(&mut self, host: &HostHandle, needed: usize) -> Result<(), PluginError> {
        let big_enough = self.buffer.as_ref().is_some_and(|b| b.len() >= needed);
        if !big_enough {
            let cap = needed.max(4096).next_power_of_two();
            let new_buf = host.create_shared_buffer(cap)?;
            // 성장 교체: 구버퍼 폐기를 host 에 통지해 host 측 매핑도 해제시킨다.
            // 통지가 없으면 구세대 버퍼의 host 매핑이 plugin 수명 내내 남는다.
            // 실패는 leak 1건 유지일 뿐이라 frame 진행을 막지 않는다 — warn 만.
            if let Some(old) = self.buffer.replace(new_buf)
                && let Err(e) = host.notify(&PluginEvent::SharedBufferReleased { id: old.id() })
            {
                tracing::warn!("shared buffer release notify failed: {e}");
            }
        }
        Ok(())
    }
}

/// [`EguiMeshCore::pending_self_repaint`] 가 있으면(egui 가 다음 pass 를 요청), 그
/// 지연 뒤 `notify` 를 1회 실행하는 타이머 스레드를 스폰한다. `armed`(이 코어 인스턴스가
/// 소유한 가드)가 이미 세팅돼 있으면 아무것도 하지 않는다 — 타이머가 fire 하면 다시
/// 풀리고, 그 다음 `render()` 가 여전히 지연이 필요하면 재-arm 한다(자연 수렴, idle
/// 상태에서 스레드가 폭주하지 않는다). Surface/Popup 공용 — 각자 자기 id 를 실은
/// `*Invalidated` 이벤트를 `notify` 클로저로 만든다.
#[cfg(any(unix, windows))]
fn spawn_self_repaint_timer(
    delay: Duration,
    armed: &std::sync::Arc<std::sync::atomic::AtomicBool>,
    host: &HostHandle,
    notify: impl FnOnce(&HostHandle) -> Result<(), PluginError> + Send + 'static,
) {
    if armed
        .compare_exchange(
            false,
            true,
            std::sync::atomic::Ordering::AcqRel,
            std::sync::atomic::Ordering::Acquire,
        )
        .is_err()
    {
        return;
    }
    let host = host.clone();
    let armed = std::sync::Arc::clone(armed);
    std::thread::spawn(move || {
        if !delay.is_zero() {
            std::thread::sleep(delay);
        }
        armed.store(false, std::sync::atomic::Ordering::Release);
        if let Err(e) = notify(&host) {
            tracing::warn!("egui-mesh self-repaint notify failed: {e}");
        }
    });
}

/// 한 egui-mesh surface 의 plugin 측 렌더 상태. surface 하나당 인스턴스 하나를 둔다
/// (여러 surface 면 `surface_id` 별로 분리). drop 시 shared buffer 매핑이 해제된다.
pub struct EguiMeshSurface {
    surface_id: u32,
    core: EguiMeshCore,
    /// `schedule_self_repaint` 가 중복 타이머 스레드를 만들지 않도록 거는 가드.
    /// 타이머가 fire 하면 다시 풀리고(`False`), 그 다음 `render()` 가 여전히 지연이
    /// 필요하면 재-arm 한다 — 스레드가 폭주하지 않고 자연 수렴한다.
    #[cfg(any(unix, windows))]
    self_repaint_armed: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

impl EguiMeshSurface {
    /// `surface_id` 에 대응하는 새 egui-mesh surface 를 만든다. egui `default_fonts` 가
    /// 설치된 독립 [`Context`] 를 소유한다.
    pub fn new(surface_id: u32) -> Self {
        Self {
            surface_id,
            core: EguiMeshCore::new(),
            #[cfg(any(unix, windows))]
            self_repaint_armed: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        }
    }

    /// 이 surface 의 host 측 식별자.
    pub fn surface_id(&self) -> u32 {
        self.surface_id
    }

    /// 폰트/스타일을 커스터마이즈할 수 있도록 내부 egui [`Context`] 를 노출한다.
    /// (예: host 와 동일 폰트 설치 → `surface.context().set_fonts(...)`.)
    pub fn context(&self) -> &Context {
        &self.core.ctx
    }

    /// `set_context` 입력으로 한 frame 을 그려 POD mesh 바이트를 만든다.
    ///
    /// 출력이 직전 frame 과 byte 단위로 동일하면(정적 화면) `None` 을 반환한다 — 호출자는
    /// 송신을 생략한다. 단, `params.need_full_textures` 면 dedup 을 우회하고 누적 텍스처
    /// 상태 전체를 full 로 동봉한다. IPC/buffer 의존이 없어 단위 테스트로
    /// set_context→tessellate→encode 라운드를 그대로 검증할 수 있다.
    pub fn run_frame(
        &mut self,
        params: &SurfaceSetContextParams,
        run_ui: impl FnMut(&Context),
    ) -> Option<Vec<u8>> {
        self.run_frame_inner(params, run_ui).map(|f| f.bytes)
    }

    fn run_frame_inner(
        &mut self,
        params: &SurfaceSetContextParams,
        run_ui: impl FnMut(&Context),
    ) -> Option<MeshFrame> {
        self.core.run_frame(
            params.width_px,
            params.height_px,
            params.pixels_per_point,
            params.theme.as_ref(),
            &params.raw_input,
            params.need_full_textures,
            run_ui,
        )
    }

    /// 직전 `set_context` 의 theme 스냅샷(재-paint closure 재구성용). 첫 컨텍스트 전 `None`.
    pub fn last_theme(&self) -> Option<&ThemeWire> {
        self.core.last_theme()
    }

    /// 직전 `run_frame`/`paint` 가 처리한 `RawInputEventWire::Copy` 로 텍스트 선택이
    /// 복사됐다면 그 문자열을 1회 소비해 반환한다(egui `Event::Copy` → 내장
    /// selectable-label/`TextEdit` 복사 로직). plugin 은 이 값을 OS 클립보드에 직접
    /// 쓴다(ADR-0009 — 비-샌드박스 프로세스라 host round-trip 이 필요 없다).
    pub fn take_copied_text(&mut self) -> Option<String> {
        self.core.take_copied_text()
    }

    /// `set_context` 한 frame 을 그려 shared buffer 에 commit 하고 host 에
    /// [`PluginEvent::PaintFrame`] 알림을 보낸다. 출력이 직전과 같으면 `Ok(None)`,
    /// 변경됐으면 commit 후의 footer generation 을 `Ok(Some(gen))` 으로 반환한다.
    #[cfg(any(unix, windows))]
    pub fn paint(
        &mut self,
        host: &HostHandle,
        params: &SurfaceSetContextParams,
        run_ui: impl FnMut(&Context),
    ) -> Result<Option<u64>, PluginError> {
        let frame = self.run_frame_inner(params, run_ui);
        self.schedule_self_repaint(host);
        let Some(frame) = frame else {
            return Ok(None);
        };
        let byte_len = frame.bytes.len() as u32;
        let (buffer_id, generation) = self.core.commit(host, &frame.bytes)?;
        host.notify(&PluginEvent::PaintFrame {
            surface_id: self.surface_id,
            buffer_id,
            generation,
            frame_seq: self.core.next_frame_seq(),
            full_textures: frame.full_textures,
            byte_len,
        })?;
        Ok(Some(generation))
    }

    /// egui 가 `viewport_output` 으로 self-repaint 를 요청했으면(스크롤 스무딩 등 남은
    /// 다중 프레임 애니메이션), 그 지연 뒤 host 에 [`PluginEvent::SurfaceInvalidated`]
    /// 를 보내 기존 idle-invalidate 재forward 경로에 편승한다
    /// (`docs/dev-guide/egui-mesh-channel.md` "plugin self-repaint" · "idle
    /// invalidate"). host 의 `forward_egui_mesh_context` 는 host-side 이벤트(입력/
    /// geom/theme 변경 등)가 있을 때만 다음 pass 를 구동하므로, 이 신호가 없으면
    /// 유휴 상태(무입력)에서 egui 내장 애니메이션이 끝까지 재생되지 못하고 방치된다
    /// (스크롤 델타가 남은 채 정지) — 이후 무관한 입력(마우스 이동 등)이 와야만
    /// 뒤늦게 몰아서 반영되는 버그의 원인.
    #[cfg(any(unix, windows))]
    fn schedule_self_repaint(&self, host: &HostHandle) {
        let Some(delay) = self.core.pending_self_repaint() else {
            return;
        };
        let surface_id = self.surface_id;
        spawn_self_repaint_timer(delay, &self.self_repaint_armed, host, move |host| {
            host.notify(&PluginEvent::SurfaceInvalidated { surface_id })
        });
    }

    /// out-of-band 상태 변경 뒤 **빈 이벤트 + 직전 focused 보존**으로 마지막 컨텍스트를
    /// 재-paint 한다(옵션 A). 캐시된 geom/ppp/focused 로 재-run → 출력이 바뀌면
    /// [`PluginEvent::PaintFrame`] 송신, 안 바뀌었거나 첫 set_context 전이면 `Ok(None)`.
    /// host 는 이 PaintFrame 에 깨어나 재합성한다(별도 재-forward 왕복 불필요).
    #[cfg(any(unix, windows))]
    pub fn repaint_last(
        &mut self,
        host: &HostHandle,
        run_ui: impl FnMut(&Context),
    ) -> Result<Option<u64>, PluginError> {
        let frame = self.core.repaint_last(run_ui);
        self.schedule_self_repaint(host);
        let Some(frame) = frame else {
            return Ok(None);
        };
        let byte_len = frame.bytes.len() as u32;
        let (buffer_id, generation) = self.core.commit(host, &frame.bytes)?;
        host.notify(&PluginEvent::PaintFrame {
            surface_id: self.surface_id,
            buffer_id,
            generation,
            frame_seq: self.core.next_frame_seq(),
            full_textures: frame.full_textures,
            byte_len,
        })?;
        Ok(Some(generation))
    }
}

/// 한 egui-mesh popup 인스턴스의 plugin 측 렌더 상태(A2). [`EguiMeshSurface`] 의 popup
/// 대응 — 회신 알림이 [`PluginEvent::PopupPaintFrame`] 이고 `instance_id` 로 키잉되는
/// 점만 다르다. popup 인스턴스 하나당 하나를 두고, `popup.closed` 수신 시 drop 한다.
pub struct EguiMeshPopup {
    instance_id: u64,
    core: EguiMeshCore,
    /// [`EguiMeshSurface::self_repaint_armed`] 와 동형 — `schedule_self_repaint` 중복
    /// 타이머 방지 가드.
    #[cfg(any(unix, windows))]
    self_repaint_armed: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

impl EguiMeshPopup {
    /// `instance_id` 에 대응하는 새 egui-mesh popup 을 만든다. egui `default_fonts` 가
    /// 설치된 독립 [`Context`] 를 소유한다.
    pub fn new(instance_id: u64) -> Self {
        Self {
            instance_id,
            core: EguiMeshCore::new(),
            #[cfg(any(unix, windows))]
            self_repaint_armed: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        }
    }

    /// 이 popup 의 host 측 인스턴스 식별자.
    pub fn instance_id(&self) -> u64 {
        self.instance_id
    }

    /// 폰트/스타일 커스터마이즈용 내부 egui [`Context`] 노출.
    pub fn context(&self) -> &Context {
        &self.core.ctx
    }

    /// `popup.set_context` 입력으로 한 frame 을 그려 POD mesh 바이트를 만든다.
    /// 정적 화면이면 `None`(송신 생략). `need_full_textures` 면 dedup 우회 + 전체 텍스처 동봉.
    pub fn run_frame(
        &mut self,
        params: &PopupSetContextParams,
        run_ui: impl FnMut(&Context),
    ) -> Option<Vec<u8>> {
        self.run_frame_inner(params, run_ui).map(|f| f.bytes)
    }

    fn run_frame_inner(
        &mut self,
        params: &PopupSetContextParams,
        run_ui: impl FnMut(&Context),
    ) -> Option<MeshFrame> {
        self.core.run_frame(
            params.width_px,
            params.height_px,
            params.pixels_per_point,
            params.theme.as_ref(),
            &params.raw_input,
            params.need_full_textures,
            run_ui,
        )
    }

    /// 직전 `popup.set_context` 의 theme 스냅샷(재-paint closure 재구성용). 첫 컨텍스트 전 `None`.
    pub fn last_theme(&self) -> Option<&ThemeWire> {
        self.core.last_theme()
    }

    /// `popup.set_context` 한 frame 을 그려 shared buffer 에 commit 하고 host 에
    /// [`PluginEvent::PopupPaintFrame`] 알림을 보낸다. 정적 화면이면 `Ok(None)`.
    #[cfg(any(unix, windows))]
    pub fn paint(
        &mut self,
        host: &HostHandle,
        params: &PopupSetContextParams,
        run_ui: impl FnMut(&Context),
    ) -> Result<Option<u64>, PluginError> {
        let frame = self.run_frame_inner(params, run_ui);
        self.schedule_self_repaint(host);
        let Some(frame) = frame else {
            return Ok(None);
        };
        let (buffer_id, generation) = self.core.commit(host, &frame.bytes)?;
        host.notify(&PluginEvent::PopupPaintFrame {
            instance_id: self.instance_id,
            buffer_id,
            generation,
            frame_seq: self.core.next_frame_seq(),
            full_textures: frame.full_textures,
        })?;
        Ok(Some(generation))
    }

    /// out-of-band 상태 변경 뒤 **빈 이벤트 + 직전 focused 보존**으로 마지막 컨텍스트를
    /// 재-paint 한다(옵션 A). 출력이 바뀌면 [`PluginEvent::PopupPaintFrame`] 송신,
    /// 안 바뀌었거나 첫 set_context 전이면 `Ok(None)`.
    #[cfg(any(unix, windows))]
    pub fn repaint_last(
        &mut self,
        host: &HostHandle,
        run_ui: impl FnMut(&Context),
    ) -> Result<Option<u64>, PluginError> {
        let frame = self.core.repaint_last(run_ui);
        self.schedule_self_repaint(host);
        let Some(frame) = frame else {
            return Ok(None);
        };
        let (buffer_id, generation) = self.core.commit(host, &frame.bytes)?;
        host.notify(&PluginEvent::PopupPaintFrame {
            instance_id: self.instance_id,
            buffer_id,
            generation,
            frame_seq: self.core.next_frame_seq(),
            full_textures: frame.full_textures,
        })?;
        Ok(Some(generation))
    }

    /// [`EguiMeshSurface::schedule_self_repaint`] 의 popup 대응 —
    /// [`PluginEvent::PopupInvalidated`] 로 host 의 popup pending-repaint 경로
    /// (ADR-0056 `plugin_mesh_popup_pending_repaint`)에 편승한다.
    #[cfg(any(unix, windows))]
    fn schedule_self_repaint(&self, host: &HostHandle) {
        let Some(delay) = self.core.pending_self_repaint() else {
            return;
        };
        let instance_id = self.instance_id;
        spawn_self_repaint_timer(delay, &self.self_repaint_armed, host, move |host| {
            host.notify(&PluginEvent::PopupInvalidated { instance_id })
        });
    }
}

/// 한 egui-mesh banner 인스턴스의 plugin 측 렌더 상태(A3). [`EguiMeshPopup`] 의 banner
/// 대응 — 회신 알림이 [`PluginEvent::BannerPaintFrame`] 이고 `instance_id` 로 키잉되는
/// 점만 다르다. banner 인스턴스 하나당 하나를 두고, `banner.closed` 수신 시 drop 한다.
pub struct EguiMeshBanner {
    instance_id: u64,
    core: EguiMeshCore,
}

impl EguiMeshBanner {
    /// `instance_id` 에 대응하는 새 egui-mesh banner 를 만든다. egui `default_fonts` 가
    /// 설치된 독립 [`Context`] 를 소유한다.
    pub fn new(instance_id: u64) -> Self {
        Self {
            instance_id,
            core: EguiMeshCore::new(),
        }
    }

    /// 이 banner 의 host 측 인스턴스 식별자.
    pub fn instance_id(&self) -> u64 {
        self.instance_id
    }

    /// 폰트/스타일 커스터마이즈용 내부 egui [`Context`] 노출.
    pub fn context(&self) -> &Context {
        &self.core.ctx
    }

    /// `banner.set_context` 입력으로 한 frame 을 그려 POD mesh 바이트를 만든다.
    /// 정적 화면이면 `None`(송신 생략). `need_full_textures` 면 dedup 우회 + 전체 텍스처 동봉.
    pub fn run_frame(
        &mut self,
        params: &BannerSetContextParams,
        run_ui: impl FnMut(&Context),
    ) -> Option<Vec<u8>> {
        self.run_frame_inner(params, run_ui).map(|f| f.bytes)
    }

    fn run_frame_inner(
        &mut self,
        params: &BannerSetContextParams,
        run_ui: impl FnMut(&Context),
    ) -> Option<MeshFrame> {
        self.core.run_frame(
            params.width_px,
            params.height_px,
            params.pixels_per_point,
            params.theme.as_ref(),
            &params.raw_input,
            params.need_full_textures,
            run_ui,
        )
    }

    /// 직전 `banner.set_context` 의 theme 스냅샷(재-paint closure 재구성용). 첫 컨텍스트 전 `None`.
    pub fn last_theme(&self) -> Option<&ThemeWire> {
        self.core.last_theme()
    }

    /// `banner.set_context` 한 frame 을 그려 shared buffer 에 commit 하고 host 에
    /// [`PluginEvent::BannerPaintFrame`] 알림을 보낸다. 정적 화면이면 `Ok(None)`.
    #[cfg(any(unix, windows))]
    pub fn paint(
        &mut self,
        host: &HostHandle,
        params: &BannerSetContextParams,
        run_ui: impl FnMut(&Context),
    ) -> Result<Option<u64>, PluginError> {
        let Some(frame) = self.run_frame_inner(params, run_ui) else {
            return Ok(None);
        };
        let (buffer_id, generation) = self.core.commit(host, &frame.bytes)?;
        host.notify(&PluginEvent::BannerPaintFrame {
            instance_id: self.instance_id,
            buffer_id,
            generation,
            frame_seq: self.core.next_frame_seq(),
            full_textures: frame.full_textures,
        })?;
        Ok(Some(generation))
    }

    /// out-of-band 상태 변경 뒤 **빈 이벤트 + 직전 focused 보존**으로 마지막 컨텍스트를
    /// 재-paint 한다(옵션 A). 출력이 바뀌면 [`PluginEvent::BannerPaintFrame`] 송신,
    /// 안 바뀌었거나 첫 set_context 전이면 `Ok(None)`.
    #[cfg(any(unix, windows))]
    pub fn repaint_last(
        &mut self,
        host: &HostHandle,
        run_ui: impl FnMut(&Context),
    ) -> Result<Option<u64>, PluginError> {
        let Some(frame) = self.core.repaint_last(run_ui) else {
            return Ok(None);
        };
        let (buffer_id, generation) = self.core.commit(host, &frame.bytes)?;
        host.notify(&PluginEvent::BannerPaintFrame {
            instance_id: self.instance_id,
            buffer_id,
            generation,
            frame_seq: self.core.next_frame_seq(),
            full_textures: frame.full_textures,
        })?;
        Ok(Some(generation))
    }
}

/// full image `base` 위에 증분 patch 를 (x, y) 오프셋으로 합성한다. kind 불일치나
/// 경계 초과는 egui delta 계약 위반 — 기록만 하고 버린다(다음 full 재전송으로 자가 회복).
#[allow(clippy::cognitive_complexity)] // complexity-exempt: ImageData 종류(Color/Font)별 bounds-check + copy 나열 — egui delta 계약상 두 kind 처리가 구조적으로 대칭이라 분해해도 절반짜리 로직 두 함수로만 흩어짐.
fn patch_image(base: &mut ImageData, patch: &ImageData, [x, y]: [usize; 2]) {
    match (base, patch) {
        (ImageData::Color(base), ImageData::Color(patch)) => {
            let [pw, ph] = patch.size;
            let [bw, bh] = base.size;
            if x + pw > bw || y + ph > bh {
                tracing::warn!("egui-mesh: color patch out of bounds; dropping");
                return;
            }
            let base = std::sync::Arc::make_mut(base);
            for row in 0..ph {
                let dst = (y + row) * bw + x;
                let src = row * pw;
                base.pixels[dst..dst + pw].copy_from_slice(&patch.pixels[src..src + pw]);
            }
        }
        (ImageData::Font(base), ImageData::Font(patch)) => {
            let [pw, ph] = patch.size;
            let [bw, bh] = base.size;
            if x + pw > bw || y + ph > bh {
                tracing::warn!("egui-mesh: font patch out of bounds; dropping");
                return;
            }
            for row in 0..ph {
                let dst = (y + row) * bw + x;
                let src = row * pw;
                base.pixels[dst..dst + pw].copy_from_slice(&patch.pixels[src..src + pw]);
            }
        }
        _ => tracing::warn!("egui-mesh: texture patch kind mismatch; dropping"),
    }
}

fn hash_bytes(bytes: &[u8]) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    bytes.hash(&mut hasher);
    hasher.finish()
}

/// [`RawInputWire`] + 크기/ppp 를 egui [`RawInput`] 으로 매핑한다. 좌표는 surface-local
/// 논리 포인트(좌상단 0,0)로 들어오므로 그대로 쓰고, screen_rect 는 물리 px / ppp 로 계산한다.
/// surface 와 popup 이 공유한다(키잉 id 만 다르고 렌더 컨텍스트 구조는 동일).
fn build_raw_input(width_px: u32, height_px: u32, ppp: f32, input: &RawInputWire) -> RawInput {
    let mut raw = RawInput::default();

    let ppp = if ppp > 0.0 { ppp } else { 1.0 };
    // 물리 픽셀 → 논리 포인트. egui 레이아웃은 포인트 단위다.
    let width_pt = width_px as f32 / ppp;
    let height_pt = height_px as f32 / ppp;
    raw.screen_rect = Some(Rect::from_min_size(Pos2::ZERO, vec2(width_pt, height_pt)));

    // ppp 는 viewport 의 native_pixels_per_point 로 전달 → ctx.pixels_per_point 가 이 값을
    // 따르고, full_output.pixels_per_point 도 동일해져 tessellate/encode 와 정합한다.
    let viewport_id = raw.viewport_id;
    raw.viewports
        .entry(viewport_id)
        .or_default()
        .native_pixels_per_point = Some(ppp);

    raw.time = input.time;
    raw.focused = input.focused;
    raw.modifiers = map_modifiers(&input.modifiers);
    raw.events = input.events.iter().filter_map(map_event).collect();
    raw
}

fn map_modifiers(m: &ModifiersWire) -> Modifiers {
    Modifiers {
        alt: m.alt,
        ctrl: m.ctrl,
        shift: m.shift,
        mac_cmd: m.mac_cmd,
        command: m.command,
    }
}

fn map_button(b: PointerButtonWire) -> PointerButton {
    match b {
        PointerButtonWire::Primary => PointerButton::Primary,
        PointerButtonWire::Secondary => PointerButton::Secondary,
        PointerButtonWire::Middle => PointerButton::Middle,
    }
}

/// 한 와이어 이벤트를 egui [`Event`] 로 매핑한다. 매핑 불가한 키 이름은 `None`(드롭) —
/// 프로토콜 계약([`RawInputEventWire::Key`])대로 plugin 이 무시한다.
fn map_event(e: &RawInputEventWire) -> Option<Event> {
    Some(match e {
        RawInputEventWire::PointerMoved { x, y } => Event::PointerMoved(Pos2::new(*x, *y)),
        RawInputEventWire::PointerButton {
            x,
            y,
            button,
            pressed,
            modifiers,
        } => Event::PointerButton {
            pos: Pos2::new(*x, *y),
            button: map_button(*button),
            pressed: *pressed,
            modifiers: map_modifiers(modifiers),
        },
        RawInputEventWire::PointerGone => Event::PointerGone,
        RawInputEventWire::Scroll { x, y } => Event::MouseWheel {
            unit: MouseWheelUnit::Point,
            delta: vec2(*x, *y),
            modifiers: Modifiers::default(),
        },
        RawInputEventWire::Key {
            key,
            pressed,
            repeat,
            modifiers,
        } => Event::Key {
            key: Key::from_name(key)?,
            physical_key: None,
            pressed: *pressed,
            repeat: *repeat,
            modifiers: map_modifiers(modifiers),
        },
        RawInputEventWire::Text { text } => Event::Text(text.clone()),
        RawInputEventWire::Ime { event } => Event::Ime(match event {
            ImeWire::Enabled => ImeEvent::Enabled,
            ImeWire::Preedit { text } => ImeEvent::Preedit(text.clone()),
            ImeWire::Commit { text } => ImeEvent::Commit(text.clone()),
            ImeWire::Disabled => ImeEvent::Disabled,
        }),
        RawInputEventWire::Copy => Event::Copy,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::epaint::Primitive;
    use tasty_plugin_protocol::RawInputWire;
    use tasty_plugin_protocol::mesh_wire::decode_paint;

    fn ctx_params(width_px: u32, height_px: u32, ppp: f32) -> SurfaceSetContextParams {
        SurfaceSetContextParams {
            surface_id: 1,
            width_px,
            height_px,
            pixels_per_point: ppp,
            raw_input: RawInputWire::default(),
            theme: None,
            need_full_textures: false,
        }
    }

    /// set_context → run → tessellate → encode 라운드. 인코드 결과가 decode_paint 로
    /// 복원되고, 라벨을 그리면 mesh + 폰트 atlas 가 실린다.
    #[test]
    fn run_frame_round_trips_through_decode() {
        let mut surface = EguiMeshSurface::new(1);
        let params = ctx_params(400, 300, 2.0);
        let bytes = surface
            .run_frame(&params, |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    ui.heading("Tasty egui-mesh");
                    ui.label("A1-S4 plugin SDK helper");
                });
            })
            .expect("first frame is always a change");

        let decoded = decode_paint(&bytes).expect("decode encoded mesh");
        // ppp 가 native_pixels_per_point 로 전달돼 출력 ppp 와 정합해야 한다.
        assert_eq!(decoded.pixels_per_point, 2.0);
        let mesh_count = decoded
            .primitives
            .iter()
            .filter(|p| matches!(p.primitive, Primitive::Mesh(_)))
            .count();
        assert!(mesh_count > 0, "label/heading should tessellate to meshes");
        assert!(
            !decoded.textures_delta.set.is_empty(),
            "first frame should carry the font atlas"
        );
    }

    /// 정적 화면: 같은 입력+UI 를 반복하면 egui 가 안정화(폰트 atlas 업로드·레이아웃
    /// 수렴)된 뒤 출력 무변화 → None(송신 생략)으로 떨어지고, 이후로도 None 을 유지한다.
    #[test]
    fn static_frame_stabilizes_to_no_change() {
        let mut surface = EguiMeshSurface::new(1);
        let params = ctx_params(320, 200, 1.0);
        let draw = |ctx: &Context| {
            egui::CentralPanel::default().show(ctx, |ui| {
                ui.label("static");
            });
        };
        // 첫 frame 은 항상 변화로 송신된다.
        assert!(
            surface.run_frame(&params, draw).is_some(),
            "first frame paints"
        );
        // 몇 frame 안에 무변화(None)로 수렴해야 한다.
        let stabilized = (0..5).any(|_| surface.run_frame(&params, draw).is_none());
        assert!(
            stabilized,
            "static UI should converge to no-change within a few frames"
        );
        // 수렴 후 동일 입력은 계속 송신 생략.
        assert!(
            surface.run_frame(&params, draw).is_none(),
            "stable static UI stays skipped"
        );
    }

    /// 출력이 바뀌면(다른 텍스트) 다시 Some 을 돌려준다.
    #[test]
    fn changed_output_paints_again() {
        let mut surface = EguiMeshSurface::new(1);
        let params = ctx_params(320, 200, 1.0);
        assert!(
            surface
                .run_frame(&params, |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        ui.label("before");
                    });
                })
                .is_some()
        );
        assert!(
            surface
                .run_frame(&params, |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        ui.label("after");
                    });
                })
                .is_some(),
            "different content must repaint"
        );
    }

    /// 첫 set_context 전(캐시 없음)엔 재-paint 가 no-op 이어야 한다(옵션 A 계약).
    #[test]
    fn repaint_last_is_noop_before_first_context() {
        let mut surface = EguiMeshSurface::new(1);
        let out = surface.core.repaint_last(|ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                ui.label("no context yet");
            });
        });
        assert!(out.is_none(), "no cached context → repaint is a no-op");
    }

    /// out-of-band 상태 변경(입력 없이) 뒤 재-paint: 캐시된 geom 으로 재-run 하되
    /// 출력이 같으면 dedup(None), 바뀌면 프레임 생성(Some).
    #[test]
    fn repaint_last_repaints_only_on_output_change() {
        let mut surface = EguiMeshSurface::new(1);
        let params = ctx_params(320, 200, 1.0);
        let draw_a = |ctx: &Context| {
            egui::CentralPanel::default().show(ctx, |ui| {
                ui.label("a");
            });
        };
        // 정적 화면으로 수렴시켜 폰트 atlas 업로드까지 안정화한다.
        surface.run_frame(&params, draw_a);
        for _ in 0..5 {
            surface.run_frame(&params, draw_a);
        }
        // 같은 내용의 재-paint 는 dedup 으로 송신 생략.
        assert!(
            surface.core.repaint_last(draw_a).is_none(),
            "unchanged output dedups to no-send"
        );
        // out-of-band 로 내용이 바뀌면 입력 없이도 재-paint 가 프레임을 만든다.
        let changed = surface.core.repaint_last(|ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                ui.label("b");
            });
        });
        assert!(
            changed.is_some(),
            "changed output must repaint without any user input"
        );
    }

    /// identity 불변식: 재-paint 가 재현하는 입력에는 사용자 이벤트가 하나도 없다.
    /// (focused 는 이벤트가 아닌 지속 상태라 캐시 재현 대상 — 아래
    /// `repaint_last_preserves_last_focused` 가 검증.)
    #[test]
    fn empty_raw_input_carries_no_events() {
        let raw = build_raw_input(320, 200, 1.0, &RawInputWire::default());
        assert!(
            raw.events.is_empty(),
            "repaint replays with empty events — no fake events injected"
        );
        assert!(!raw.focused, "default wire maps to focused=false");
    }

    /// 재-paint 는 직전 set_context 의 focused 를 보존해야 한다 — focused=false 로
    /// 재-run 하면 `has_focus()` 의 viewport 게이트가 꺼져 포커스 의존 UI(커서·드롭다운·
    /// editing 상태머신)가 재-paint 프레임에서만 퇴행한다(markdown 주소창 진동의 원인).
    #[test]
    fn repaint_last_preserves_last_focused() {
        let mut surface = EguiMeshSurface::new(1);
        let focused_params = SurfaceSetContextParams {
            raw_input: RawInputWire {
                focused: true,
                ..Default::default()
            },
            ..ctx_params(320, 200, 1.0)
        };
        surface.run_frame(&focused_params, |ctx| {
            assert!(ctx.input(|i| i.focused), "set_context frame is focused");
            egui::CentralPanel::default().show(ctx, |ui| {
                ui.label("a");
            });
        });
        // 재-paint 재-run 프레임에서도 i.focused 가 true 로 유지돼야 한다.
        let mut checked = false;
        let _ = surface.core.repaint_last(|ctx| {
            checked = true;
            assert!(
                ctx.input(|i| i.focused),
                "repaint must replay the last focused state, not default(false)"
            );
            egui::CentralPanel::default().show(ctx, |ui| {
                ui.label("b");
            });
        });
        assert!(checked, "repaint ran the ui closure");

        // 직전이 unfocused 였으면 캐시값도 false — banner(항상 focused=false forward)
        // 등의 기존 동작 불변.
        surface.run_frame(&ctx_params(320, 200, 1.0), |ctx| {
            assert!(!ctx.input(|i| i.focused));
            egui::CentralPanel::default().show(ctx, |ui| {
                ui.label("c");
            });
        });
        let _ = surface.core.repaint_last(|ctx| {
            assert!(
                !ctx.input(|i| i.focused),
                "unfocused last context replays unfocused"
            );
            egui::CentralPanel::default().show(ctx, |ui| {
                ui.label("d");
            });
        });
    }

    /// 캐시된 theme 스냅샷이 set_context 뒤 노출되고, theme 미동봉이면 None.
    #[test]
    fn last_theme_reflects_last_set_context() {
        let mut surface = EguiMeshSurface::new(1);
        assert!(surface.last_theme().is_none(), "no context yet → no theme");
        // theme 미동봉 params.
        let params = ctx_params(320, 200, 1.0);
        surface.run_frame(&params, |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                ui.label("x");
            });
        });
        assert!(
            surface.last_theme().is_none(),
            "set_context without theme → last_theme stays None"
        );
    }

    /// Context 생성 후 첫 frame 은 자연-full 로 마킹된다 — bootstrap race(gen1 이
    /// 디코드 전에 덮여도 host 가 full 재요청으로 회복)의 전제.
    #[test]
    fn first_frame_is_naturally_full() {
        let mut surface = EguiMeshSurface::new(1);
        let params = ctx_params(320, 200, 1.0);
        let frame = surface
            .run_frame_inner(&params, |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    ui.label("first");
                });
            })
            .expect("first frame paints");
        assert!(
            frame.full_textures,
            "first frame carries the whole texture state (font atlas full image)"
        );
        assert!(
            !surface.core.tex_state.is_empty(),
            "font atlas accumulated into tex_state"
        );
    }

    /// need_full_textures: 정적 화면(dedup 수렴)이어도 강제 송신되고, 누적 텍스처
    /// 전체가 full image(pos == None)로 동봉되며 full 로 마킹된다.
    #[test]
    fn need_full_bypasses_dedup_and_carries_all_textures() {
        let mut surface = EguiMeshSurface::new(1);
        let params = ctx_params(320, 200, 1.0);
        let draw = |ctx: &Context| {
            egui::CentralPanel::default().show(ctx, |ui| {
                ui.label("static");
            });
        };
        // 정적 수렴시켜 일반 재-run 은 None 이 되게 한다.
        for _ in 0..6 {
            surface.run_frame(&params, draw);
        }
        assert!(surface.run_frame(&params, draw).is_none(), "converged");

        let full_params = SurfaceSetContextParams {
            need_full_textures: true,
            ..ctx_params(320, 200, 1.0)
        };
        let frame = surface
            .run_frame_inner(&full_params, draw)
            .expect("need_full forces a send even when output is unchanged");
        assert!(frame.full_textures);

        let decoded = decode_paint(&frame.bytes).expect("decode");
        assert_eq!(
            decoded.textures_delta.set.len(),
            surface.core.tex_state.len(),
            "full frame carries every accumulated texture"
        );
        assert!(
            decoded
                .textures_delta
                .set
                .iter()
                .all(|(_, d)| d.pos.is_none()),
            "full frame textures are full images"
        );
        assert!(decoded.textures_delta.free.is_empty());
    }

    /// 누적층: full → patch 합성 → free 제거, 자연-full 판정.
    #[test]
    fn accumulate_textures_composites_patches_and_frees() {
        use egui::epaint::ColorImage;
        use egui::{Color32, epaint::textures::TexturesDelta};

        let mut core = EguiMeshCore::new();
        let id = TextureId::User(7);
        let full = ImageDelta {
            image: ImageData::Color(std::sync::Arc::new(ColorImage {
                size: [2, 2],
                pixels: vec![Color32::BLACK; 4],
            })),
            options: Default::default(),
            pos: None,
        };
        let delta = TexturesDelta {
            set: vec![(id, full)],
            free: vec![],
        };
        assert!(
            core.accumulate_textures(&delta),
            "all-full delta is naturally full"
        );

        // (1,1) 에 1x1 patch — 해당 픽셀만 바뀐다.
        let patch = ImageDelta {
            image: ImageData::Color(std::sync::Arc::new(ColorImage {
                size: [1, 1],
                pixels: vec![Color32::WHITE],
            })),
            options: Default::default(),
            pos: Some([1, 1]),
        };
        let delta = TexturesDelta {
            set: vec![(id, patch)],
            free: vec![],
        };
        assert!(
            !core.accumulate_textures(&delta),
            "patch-only delta is not full"
        );
        let (img, _) = core.tex_state.get(&id).expect("texture retained");
        let ImageData::Color(img) = img else {
            panic!("expected color image");
        };
        assert_eq!(img.pixels[3], Color32::WHITE, "patched pixel at (1,1)");
        assert_eq!(img.pixels[0], Color32::BLACK, "other pixels intact");

        // free → 누적 상태에서 제거, full 재구성에서 빠진다.
        let delta = TexturesDelta {
            set: vec![],
            free: vec![id],
        };
        core.accumulate_textures(&delta);
        assert!(core.tex_state.is_empty());
        assert!(core.full_texture_delta().set.is_empty());
    }

    /// RawInputWire → egui RawInput 매핑: 좌표 보존, 키 이름 파싱, 매핑 불가 키 드롭.
    #[test]
    fn raw_input_mapping_covers_pointer_scroll_key_text() {
        let wire = RawInputWire {
            time: Some(1.5),
            focused: true,
            modifiers: ModifiersWire {
                ctrl: true,
                ..Default::default()
            },
            events: vec![
                RawInputEventWire::PointerMoved { x: 12.0, y: 34.0 },
                RawInputEventWire::PointerButton {
                    x: 12.0,
                    y: 34.0,
                    button: PointerButtonWire::Secondary,
                    pressed: true,
                    modifiers: ModifiersWire::default(),
                },
                RawInputEventWire::Scroll { x: 0.0, y: -8.0 },
                RawInputEventWire::Key {
                    key: "Enter".into(),
                    pressed: true,
                    repeat: false,
                    modifiers: ModifiersWire::default(),
                },
                // 매핑 불가한 키 이름 → 드롭.
                RawInputEventWire::Key {
                    key: "NotARealKey".into(),
                    pressed: true,
                    repeat: false,
                    modifiers: ModifiersWire::default(),
                },
                RawInputEventWire::Text { text: "hi".into() },
            ],
        };
        let params = SurfaceSetContextParams {
            surface_id: 7,
            width_px: 200,
            height_px: 100,
            pixels_per_point: 2.0,
            raw_input: wire,
            theme: None,
            need_full_textures: false,
        };
        let raw = build_raw_input(
            params.width_px,
            params.height_px,
            params.pixels_per_point,
            &params.raw_input,
        );

        // screen_rect = 물리 px / ppp.
        let rect = raw.screen_rect.expect("screen_rect");
        assert_eq!(rect.width(), 100.0);
        assert_eq!(rect.height(), 50.0);
        assert_eq!(
            raw.viewports
                .get(&raw.viewport_id)
                .and_then(|v| v.native_pixels_per_point),
            Some(2.0)
        );
        assert_eq!(raw.time, Some(1.5));
        assert!(raw.focused);
        assert!(raw.modifiers.ctrl);

        // 매핑 불가 키 1개가 드롭돼 6개 중 5개만 남는다.
        assert_eq!(raw.events.len(), 5);
        assert!(matches!(raw.events[0], Event::PointerMoved(p) if p == Pos2::new(12.0, 34.0)));
        assert!(matches!(
            raw.events[1],
            Event::PointerButton {
                button: PointerButton::Secondary,
                pressed: true,
                ..
            }
        ));
        assert!(matches!(
            raw.events[2],
            Event::MouseWheel {
                unit: MouseWheelUnit::Point,
                ..
            }
        ));
        assert!(matches!(
            raw.events[3],
            Event::Key {
                key: Key::Enter,
                pressed: true,
                ..
            }
        ));
        assert!(matches!(&raw.events[4], Event::Text(t) if t == "hi"));
    }

    /// IME wire 4단계가 각각 대응하는 `egui::Event::Ime(ImeEvent::…)` 로 매핑된다.
    /// markdown 주소창의 라이브 preedit 표시가 이 매핑에 의존한다.
    #[test]
    fn ime_wire_maps_to_egui_ime_events() {
        let cases = [
            (ImeWire::Enabled, ImeEvent::Enabled),
            (
                ImeWire::Preedit { text: "ㅎ".into() },
                ImeEvent::Preedit("ㅎ".into()),
            ),
            (
                ImeWire::Commit { text: "한".into() },
                ImeEvent::Commit("한".into()),
            ),
            (ImeWire::Disabled, ImeEvent::Disabled),
        ];
        for (wire, expected) in cases {
            let mapped = map_event(&RawInputEventWire::Ime { event: wire });
            assert_eq!(mapped, Some(Event::Ime(expected)));
        }
    }

    /// TODO 15 회귀 방지: egui 가 `ctx.request_repaint_after` 로 "다음 pass 도
    /// 그려달라"고 요청하면, `render()` 는 그 신호를 버리지 않고
    /// `pending_self_repaint()` 로 노출해야 한다 — 과거에는 `full.viewport_output`
    /// 을 완전히 무시해 이 정보가 유실됐다(markdown 트랙패드 스크롤이 유휴 상태에서
    /// 몰아서 뒤늦게 반영되던 결함의 근본 원인).
    #[test]
    fn render_captures_egui_repaint_request_instead_of_dropping_it() {
        let mut surface = EguiMeshSurface::new(1);
        let params = ctx_params(320, 200, 1.0);
        let draw_static = |ctx: &Context| {
            egui::CentralPanel::default().show(ctx, |ui| {
                ui.label("static");
            });
        };

        // bootstrap frame(폰트 atlas 업로드 등 첫 frame 특유의 repaint 요청이 섞일 수
        // 있어 baseline 값 자체는 단언하지 않는다 — 안정화 목적으로만 1회 그린다).
        surface.run_frame(&params, draw_static);

        // egui 내부 애니메이션(스크롤 스무딩 등)에 의존하지 않고, `request_repaint_after`
        // 를 직접 호출해 "다음 pass 필요" 신호를 결정적으로 재현한다.
        surface.run_frame(&params, |ctx| {
            ctx.request_repaint_after(std::time::Duration::from_millis(50));
            draw_static(ctx);
        });
        let delay = surface
            .core
            .pending_self_repaint()
            .expect("egui's repaint request must not be dropped by render()");
        assert!(
            delay <= std::time::Duration::from_millis(50),
            "captured delay must reflect (or be tighter than) the requested 50ms, got {delay:?}"
        );

        // 다음 pass 부터는 더 이상 요청하지 않으면 정보가 stale 하게 남지 않고
        // 갱신된다 — 몇 프레임 안에 None 으로 수렴해야 한다(첫 몇 frame 은 egui 의
        // 자체 안정화 repaint 요청이 섞일 수 있어 `static_frame_stabilizes_to_no_change`
        // 와 동일하게 수렴 여부만 확인한다).
        let converged = (0..5).any(|_| {
            surface.run_frame(&params, draw_static);
            surface.core.pending_self_repaint().is_none()
        });
        assert!(
            converged,
            "pending_self_repaint must not stay stuck once egui stops requesting repaints"
        );
    }

    /// 실제 버그 재현 경로: `Point` 단위 8pt 이상 스크롤 델타(예: 물리 마우스 휠
    /// notch 가 `mouse.rs` 에서 `*50.0` 스케일된 값)는 egui 내부에서
    /// `is_smooth=false` 로 판정돼 `unprocessed_scroll_delta` 에 적립되고, 여러
    /// pass 에 걸쳐 지수완화로 drain 된다(egui 0.31 `input_state/mod.rs:340-394`).
    /// drain 이 끝나기 전까지 `wants_repaint_after()` 는 `Duration::ZERO`(즉시
    /// repaint)를 반환하므로, 이 pass 뒤 `pending_self_repaint()` 도 채워져야 한다.
    #[test]
    fn large_scroll_delta_leaves_a_pending_self_repaint_request() {
        let mut surface = EguiMeshSurface::new(1);

        // bootstrap — 폰트 atlas 업로드 등 첫 frame 특유의 잡음을 먼저 안정화한다.
        surface.run_frame(&ctx_params(320, 200, 1.0), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                ui.label("scrollable content");
            });
        });

        let mut params = ctx_params(320, 200, 1.0);
        params.raw_input.events = vec![RawInputEventWire::Scroll { x: 0.0, y: -50.0 }];
        surface.run_frame(&params, |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                ui.label("scrollable content");
            });
        });

        let delay = surface
            .core
            .pending_self_repaint()
            .expect("large scroll delta must leave egui wanting another pass");
        assert_eq!(
            delay,
            Duration::ZERO,
            "unprocessed scroll delta requests an immediate repaint"
        );
    }

    /// [`EguiMeshPopup`] 도 [`EguiMeshCore::pending_self_repaint`] 를 공유한다 — 위
    /// surface 테스트와 동형으로, popup 채널(git-viewer/clipboard-viewer)도 같은
    /// 정보 유실 없이 self-repaint 요청을 캡처해야 한다(TODO 15 popup 대응,
    /// `PopupInvalidated`).
    #[test]
    fn popup_also_captures_egui_repaint_request() {
        let mut popup = EguiMeshPopup::new(1);
        let params = PopupSetContextParams {
            instance_id: 1,
            width_px: 320,
            height_px: 200,
            pixels_per_point: 1.0,
            raw_input: RawInputWire::default(),
            theme: None,
            need_full_textures: false,
        };
        popup.run_frame(&params, |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                ui.label("popup content");
            });
        });
        popup.run_frame(&params, |ctx| {
            ctx.request_repaint_after(std::time::Duration::from_millis(30));
            egui::CentralPanel::default().show(ctx, |ui| {
                ui.label("popup content");
            });
        });
        assert!(
            popup.core.pending_self_repaint().is_some(),
            "popup render() must not drop egui's repaint request either"
        );
    }

    /// `RawInputEventWire::Copy` 가 `map_event` 를 거쳐 실제 `egui::Event::Copy` 로
    /// 도착하는지 확인한다 — markdown 복사 회귀의 근본 원인은 host 가 이 이벤트를
    /// plugin 의 egui `Context` 가 아니라 자기 자신의 top-level `Context` 에 주입하던
    /// 것이었다. wire → `Event` 매핑 자체가 다시 끊기지 않도록 고정한다.
    #[test]
    fn copy_wire_event_maps_to_egui_copy() {
        assert_eq!(map_event(&RawInputEventWire::Copy), Some(Event::Copy));
    }

    /// 엔드투엔드 회귀: 텍스트가 선택된 상태에서 `Copy` wire 이벤트를 보내면, egui 의
    /// 내장 `TextEdit` 선택-복사 로직이 `platform_output.commands` 에 `CopyText` 를
    /// 채우고 `render()` 가 그 값을 `take_copied_text()` 로 노출해야 한다. 선택이
    /// 없으면(다음 frame) 값이 남아있지 않아야 한다(1회 소비 + 자연 소거).
    #[test]
    fn copy_event_exposes_selected_text_edit_range() {
        let mut surface = EguiMeshSurface::new(1);
        let id = egui::Id::new("copy_test");
        let mut buf = "hello world".to_string();

        // frame 1: TextEdit 에 포커스를 주고, 전체 텍스트를 선택한 상태를 저장한다
        // (실제 마우스 드래그 대신 `TextEditState` 를 직접 seed — egui 공식 문서의
        // "새 selection 만들기" 레시피와 동일한 방식).
        surface.run_frame(&ctx_params(400, 100, 1.0), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                ui.memory_mut(|m| m.request_focus(id));
                let mut output = egui::TextEdit::singleline(&mut buf).id(id).show(ui);
                let range = egui::text::CCursorRange::two(
                    egui::text::CCursor::new(0),
                    egui::text::CCursor::new(buf.chars().count()),
                );
                output.state.cursor.set_char_range(Some(range));
                output.state.store(ui.ctx(), output.response.id);
            });
        });
        assert_eq!(
            surface.take_copied_text(),
            None,
            "selecting text alone must not copy anything"
        );

        // frame 2: 선택은 유지된 채(입력 없는 재-run 이 아니라 같은 UI 를 다시 그려
        // 포커스+선택을 재현) Copy wire 이벤트만 보낸다.
        let mut copy_params = ctx_params(400, 100, 1.0);
        copy_params.raw_input.events = vec![RawInputEventWire::Copy];
        surface.run_frame(&copy_params, |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                ui.memory_mut(|m| m.request_focus(id));
                ui.add(egui::TextEdit::singleline(&mut buf).id(id));
            });
        });

        assert_eq!(surface.take_copied_text().as_deref(), Some("hello world"));
        // 1회 소비 — 바로 다시 물으면 비어 있다.
        assert_eq!(surface.take_copied_text(), None);
    }
}
