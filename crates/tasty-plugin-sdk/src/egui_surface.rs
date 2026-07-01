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

use std::hash::{Hash, Hasher};

use egui::{
    Context, Event, Key, Modifiers, MouseWheelUnit, PointerButton, Pos2, RawInput, Rect, vec2,
};
use tasty_plugin_protocol::mesh_wire::encode_paint;
use tasty_plugin_protocol::{
    ModifiersWire, PointerButtonWire, PopupSetContextParams, RawInputEventWire, RawInputWire,
    SurfaceSetContextParams,
};

#[cfg(unix)]
use tasty_plugin_protocol::{PluginEvent, SharedBufferId};

#[cfg(unix)]
use crate::error::PluginError;
#[cfg(unix)]
use crate::host::HostHandle;
#[cfg(unix)]
use crate::shared_buffer::SharedBuffer;

/// egui-mesh 렌더 코어 — surface/popup 공통. 자기 egui [`Context`](폰트 atlas 포함),
/// 직전 출력 해시(invalidate 판정), shared buffer(unix) 를 들고 있다. surface 와 popup 은
/// 회신 알림(`PaintFrame` vs `PopupPaintFrame`)만 다르고 run_frame/commit 로직은 동일해
/// 이 코어를 공유한다([`EguiMeshSurface`] / [`EguiMeshPopup`] 가 얇게 감싼다).
struct EguiMeshCore {
    ctx: Context,
    /// 직전 frame 에 인코드한 mesh 바이트의 해시. 같으면 정적 화면으로 보고 송신 생략.
    last_hash: Option<u64>,
    /// mesh POD 블록을 쓰는 shared buffer. 필요 크기보다 작아지면 재생성한다.
    #[cfg(unix)]
    buffer: Option<SharedBuffer>,
}

impl EguiMeshCore {
    fn new() -> Self {
        Self {
            ctx: Context::default(),
            last_hash: None,
            #[cfg(unix)]
            buffer: None,
        }
    }

    /// 한 frame 을 그려 POD mesh 바이트를 만든다. 출력이 직전과 byte 단위로 동일하면
    /// (정적 화면) `None` — 호출자는 송신을 생략한다.
    fn run_frame(
        &mut self,
        width_px: u32,
        height_px: u32,
        ppp: f32,
        raw_input: &RawInputWire,
        run_ui: impl FnMut(&Context),
    ) -> Option<Vec<u8>> {
        let raw = build_raw_input(width_px, height_px, ppp, raw_input);
        let full = self.ctx.run(raw, run_ui);
        // tessellate 는 plugin 이 수행한다(폰트 atlas 를 plugin 이 소유, research-a1 §2-1).
        let primitives = self.ctx.tessellate(full.shapes, full.pixels_per_point);
        let bytes = encode_paint(&primitives, &full.textures_delta, full.pixels_per_point);

        let hash = hash_bytes(&bytes);
        if self.last_hash == Some(hash) {
            return None;
        }
        self.last_hash = Some(hash);
        Some(bytes)
    }

    /// 인코드된 바이트를 shared buffer 에 commit 하고 (buffer_id, generation) 을 돌려준다.
    /// 회신 알림(PaintFrame/PopupPaintFrame)은 호출자가 보낸다.
    #[cfg(unix)]
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
    #[cfg(unix)]
    fn ensure_buffer(&mut self, host: &HostHandle, needed: usize) -> Result<(), PluginError> {
        let big_enough = self.buffer.as_ref().is_some_and(|b| b.len() >= needed);
        if !big_enough {
            let cap = needed.max(4096).next_power_of_two();
            self.buffer = Some(host.create_shared_buffer(cap)?);
        }
        Ok(())
    }
}

/// 한 egui-mesh surface 의 plugin 측 렌더 상태. surface 하나당 인스턴스 하나를 둔다
/// (여러 surface 면 `surface_id` 별로 분리). drop 시 shared buffer 매핑이 해제된다.
pub struct EguiMeshSurface {
    surface_id: u32,
    core: EguiMeshCore,
}

impl EguiMeshSurface {
    /// `surface_id` 에 대응하는 새 egui-mesh surface 를 만든다. egui `default_fonts` 가
    /// 설치된 독립 [`Context`] 를 소유한다.
    pub fn new(surface_id: u32) -> Self {
        Self {
            surface_id,
            core: EguiMeshCore::new(),
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
    /// 송신을 생략한다. IPC/buffer 의존이 없어 단위 테스트로 set_context→tessellate→encode
    /// 라운드를 그대로 검증할 수 있다.
    pub fn run_frame(
        &mut self,
        params: &SurfaceSetContextParams,
        run_ui: impl FnMut(&Context),
    ) -> Option<Vec<u8>> {
        self.core.run_frame(
            params.width_px,
            params.height_px,
            params.pixels_per_point,
            &params.raw_input,
            run_ui,
        )
    }

    /// `set_context` 한 frame 을 그려 shared buffer 에 commit 하고 host 에
    /// [`PluginEvent::PaintFrame`] 알림을 보낸다. 출력이 직전과 같으면 `Ok(None)`,
    /// 변경됐으면 commit 후의 footer generation 을 `Ok(Some(gen))` 으로 반환한다.
    #[cfg(unix)]
    pub fn paint(
        &mut self,
        host: &HostHandle,
        params: &SurfaceSetContextParams,
        run_ui: impl FnMut(&Context),
    ) -> Result<Option<u64>, PluginError> {
        let Some(bytes) = self.run_frame(params, run_ui) else {
            return Ok(None);
        };
        let (buffer_id, generation) = self.core.commit(host, &bytes)?;
        host.notify(&PluginEvent::PaintFrame {
            surface_id: self.surface_id,
            buffer_id,
            generation,
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
}

impl EguiMeshPopup {
    /// `instance_id` 에 대응하는 새 egui-mesh popup 을 만든다. egui `default_fonts` 가
    /// 설치된 독립 [`Context`] 를 소유한다.
    pub fn new(instance_id: u64) -> Self {
        Self {
            instance_id,
            core: EguiMeshCore::new(),
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
    /// 정적 화면이면 `None`(송신 생략).
    pub fn run_frame(
        &mut self,
        params: &PopupSetContextParams,
        run_ui: impl FnMut(&Context),
    ) -> Option<Vec<u8>> {
        self.core.run_frame(
            params.width_px,
            params.height_px,
            params.pixels_per_point,
            &params.raw_input,
            run_ui,
        )
    }

    /// `popup.set_context` 한 frame 을 그려 shared buffer 에 commit 하고 host 에
    /// [`PluginEvent::PopupPaintFrame`] 알림을 보낸다. 정적 화면이면 `Ok(None)`.
    #[cfg(unix)]
    pub fn paint(
        &mut self,
        host: &HostHandle,
        params: &PopupSetContextParams,
        run_ui: impl FnMut(&Context),
    ) -> Result<Option<u64>, PluginError> {
        let Some(bytes) = self.run_frame(params, run_ui) else {
            return Ok(None);
        };
        let (buffer_id, generation) = self.core.commit(host, &bytes)?;
        host.notify(&PluginEvent::PopupPaintFrame {
            instance_id: self.instance_id,
            buffer_id,
            generation,
        })?;
        Ok(Some(generation))
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
        };
        let raw = build_raw_input(&params);

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
}
