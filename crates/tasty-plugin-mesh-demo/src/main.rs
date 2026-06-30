#![forbid(unsafe_code)]

//! Tasty egui-mesh PoC plugin (A1).
//!
//! egui-mesh 채널(ADR-0028)이 동작함을 증명하는 최소 소비자다. plugin 이 자기
//! 프로세스에서 egui 를 구동·tessellate 해 mesh 를 host 에 commit 하면, host 가
//! 전용 `egui_wgpu::Renderer` 로 surface 영역에 벡터 합성한다.
//!
//! 코덱/SDK 는 재구현하지 않는다 — [`EguiMeshSurface`] 헬퍼만 호출한다.
//! `surface.set_context`(host→plugin) → `run_frame`/`tessellate`/`encode` →
//! `paint_and_send`(shared buffer commit + `PaintFrame` 알림) 전 과정을 SDK 가 은닉한다.
//!
//! 데모는 label 1개에 더해 입력 forward 를 눈으로 확인할 수 있도록 클릭 카운터
//! 버튼과 스크롤 영역을 둔다 — host 가 forward 한 실제 사용자 입력이 plugin 의
//! mesh 를 바꾸는지 검증한다.

use std::collections::HashMap;

use tasty_plugin_sdk::{
    EguiMeshSurface, Plugin, SurfaceCreateCtx, SurfaceEventCtx, SurfaceResult, SurfaceSetContextCtx,
};

const PLUGIN_ID: &str = "com.tasty.mesh-demo";
const PLUGIN_VERSION: &str = "0.1.0";

#[derive(Default)]
struct MeshDemoPlugin {
    /// surface_id → plugin 측 egui 렌더 상태(폰트 atlas·shared buffer 소유).
    surfaces: HashMap<u32, EguiMeshSurface>,
    /// surface_id → 클릭 횟수. 입력 forward 가 mesh 를 바꾸는 것을 보이는 데모 상태.
    clicks: HashMap<u32, u32>,
}

impl Plugin for MeshDemoPlugin {
    fn id(&self) -> &str {
        PLUGIN_ID
    }

    fn version(&self) -> &str {
        PLUGIN_VERSION
    }

    fn create_surface(&mut self, _ctx: SurfaceCreateCtx) -> SurfaceResult {
        // egui-mesh surface 는 tree(UiNode) 가 아니라 mesh 채널로 그린다 — 빈 결과.
        SurfaceResult::default()
    }

    fn handle_event(&mut self, _ctx: SurfaceEventCtx) -> SurfaceResult {
        SurfaceResult::default()
    }

    fn destroy_surface(&mut self, surface_id: u32) {
        // surface 가 닫히면 egui Context·shared buffer 매핑·데모 상태를 함께 해제.
        self.surfaces.remove(&surface_id);
        self.clicks.remove(&surface_id);
    }

    fn paint_surface(&mut self, ctx: SurfaceSetContextCtx) {
        self.paint(ctx);
    }
}

impl MeshDemoPlugin {
    /// `set_context` 한 frame 을 그려 host 에 mesh 를 회신한다.
    #[cfg(unix)]
    fn paint(&mut self, ctx: SurfaceSetContextCtx) {
        let sid = ctx.params.surface_id;
        // surfaces / clicks 는 서로소 필드라 동시 mutable 차용이 안전하다.
        let surface = self
            .surfaces
            .entry(sid)
            .or_insert_with(|| EguiMeshSurface::new(sid));
        let clicks = self.clicks.entry(sid).or_insert(0);
        // tessellate+encode+commit 지연 측정 (ADR-0028 reconsideration trigger 점검용).
        let t0 = std::time::Instant::now();
        let result = surface.paint(&ctx.host, &ctx.params, |egui_ctx| {
            draw_demo(egui_ctx, clicks);
        });
        match result {
            Ok(Some(_gen)) => {
                let us = t0.elapsed().as_micros();
                tracing::info!("mesh-demo surface {sid} paint sent in {us}us");
            }
            Ok(None) => {} // 정적 화면 — 송신 생략.
            Err(e) => tracing::warn!("mesh-demo surface {sid} paint failed: {e}"),
        }
    }

    /// egui-mesh shared-buffer 송신은 현재 unix 전용(host buffer.rs 가 windows 미구현).
    /// 다른 OS 에선 채널이 비활성이라 no-op — 크로스플랫폼 컴파일만 보장한다.
    #[cfg(not(unix))]
    fn paint(&mut self, _ctx: SurfaceSetContextCtx) {}
}

/// 데모 UI: label + 클릭 카운터 버튼 + 스크롤 영역. 색/폰트는 egui 기본값을 쓴다
/// (PoC — Theme 토큰 연동은 B1 markdown 전환에서 다룬다).
#[cfg(unix)]
fn draw_demo(ctx: &egui::Context, clicks: &mut u32) {
    egui::CentralPanel::default().show(ctx, |ui| {
        // 카운터를 heading 에 둬 입력 forward 효과(클릭)가 한눈에 보이게 한다.
        ui.heading(format!("Hello egui-mesh — clicks: {clicks}"));
        ui.label("Rendered in the plugin process, composited by the host.");
        // 풀폭 큰 버튼 — 헤드리스 주입이 빗나가지 않게 넉넉한 hit 영역.
        let btn = ui.add_sized([ui.available_width(), 56.0], egui::Button::new("CLICK ME"));
        if btn.clicked() {
            *clicks += 1;
        }
        ui.separator();
        // 스크롤 효과를 보이는 영역 — 줄 번호가 스크롤 시 위로 사라진다.
        egui::ScrollArea::vertical().show(ui, |ui| {
            for i in 1..=80 {
                ui.label(format!("scrollable line {i}"));
            }
        });
    });
}

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();
    tasty_plugin_sdk::run(MeshDemoPlugin::default())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plugin_identity() {
        let p = MeshDemoPlugin::default();
        assert_eq!(p.id(), "com.tasty.mesh-demo");
        assert_eq!(p.version(), "0.1.0");
    }

    #[test]
    fn destroy_clears_state() {
        let mut p = MeshDemoPlugin::default();
        p.clicks.insert(7, 3);
        p.destroy_surface(7);
        assert!(!p.clicks.contains_key(&7));
    }
}
