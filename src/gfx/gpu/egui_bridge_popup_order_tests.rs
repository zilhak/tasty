//! host·plugin 팝업 묶음의 egui 레이어 순서를 여러 패스에 걸쳐 본다. 덮는 plugin 팝업은 덮음 막이 Area와
//! 셸 레이어(Area가 아닌 `layer_painter`) 두 레이어다. egui `end_pass`가 sublayer 부모를 HashMap 순서로
//! 처리하므로 결과가 패스마다 달라질 수 있어, 한 패스가 아니라 여러 패스를 연달아 본다.

use super::enforce_host_plugin_popup_z_order;

/// 한 번 맞는 확률이 반이라도 이만큼 연달아 맞을 수는 없다.
const PASSES: usize = 40;

fn foreground(name: &str) -> egui::LayerId {
    egui::LayerId::new(egui::Order::Foreground, egui::Id::new(name))
}

fn rect(x: f32, y: f32, w: f32, h: f32) -> egui::Rect {
    egui::Rect::from_min_size(egui::pos2(x, y), egui::vec2(w, h))
}

/// 패스 하나를 돌린다.
fn run_pass(ctx: &egui::Context, draw: impl FnMut(&egui::Context)) {
    // 그리기 출력은 이 시험이 보지 않는다. 레이어 순서는 패스 뒤 Memory에서 읽는다.
    let _ = ctx.run(Default::default(), draw);
}

/// plugin 덮음 막이. `press::block_pointer`처럼 덮는 범위의 포인터를 받는 Area다.
fn show_cover(ctx: &egui::Context, layer: egui::LayerId, scope: egui::Rect) {
    crate::adapters::ui::popup::press::block_pointer(ctx, layer.id, scope);
}

/// plugin 셸. Area 없이 레이어에 바로 칠한다.
fn paint_shell(ctx: &egui::Context, layer: egui::LayerId, frame: egui::Rect) {
    ctx.layer_painter(layer)
        .rect_filled(frame, 0.0, egui::Color32::DARK_GRAY);
}

/// host 팝업 하나. 포인터를 받는 Area다.
fn show_host(ctx: &egui::Context, layer: egui::LayerId, frame: egui::Rect) {
    egui::Area::new(layer.id)
        .order(egui::Order::Foreground)
        .fixed_pos(frame.min)
        .interactable(true)
        .sense(egui::Sense::hover())
        .show(ctx, |ui| {
            ui.allocate_exact_size(frame.size(), egui::Sense::click());
        });
}

/// 직전 패스까지 정해진 순서에서 `layer`의 자리. 순서에 없으면 egui가 순서 있는 레이어 뒤에 그리므로 맨 위로 본다.
fn position(ctx: &egui::Context, layer: egui::LayerId) -> usize {
    ctx.memory(|m| m.layer_ids().position(|l| l == layer))
        .unwrap_or(usize::MAX)
}

/// 덮음 막이와 셸이 이미 정해진 순서(`cover_first`)에 있는 채로 host 팝업 두 레이어가 그 위에 뜬다.
/// `cover_first == false`는 워크스페이스를 오가 막이 Area만 다시 맨 위로 오른 뒤의 순서다.
struct Scene {
    cover: egui::LayerId,
    shell: egui::LayerId,
    host: [egui::LayerId; 2],
    scope: egui::Rect,
    frame: egui::Rect,
    host_frame: egui::Rect,
}

impl Scene {
    fn new() -> Self {
        Self {
            cover: foreground("plugin_cover"),
            shell: foreground("plugin_shell"),
            host: [foreground("host_cover"), foreground("host_picker")],
            scope: rect(0.0, 0.0, 400.0, 300.0),
            frame: rect(100.0, 100.0, 200.0, 80.0),
            host_frame: rect(50.0, 50.0, 300.0, 200.0),
        }
    }

    /// plugin 팝업만 뜬 패스. `cover_first`이면 막이가 셸 아래, 아니면 셸 위에 오도록 처음 순서를 잡는다.
    fn start(&self, ctx: &egui::Context, start: Start) {
        run_pass(ctx, |ctx| match start {
            Start::CoverFirst => {
                show_cover(ctx, self.cover, self.scope);
                paint_shell(ctx, self.shell, self.frame);
                ctx.move_to_top(self.shell);
            }
            Start::ShellFirst => {
                ctx.move_to_top(self.shell);
                paint_shell(ctx, self.shell, self.frame);
                show_cover(ctx, self.cover, self.scope);
            }
            Start::ShellUnordered => {
                show_cover(ctx, self.cover, self.scope);
                paint_shell(ctx, self.shell, self.frame);
            }
        });
    }

    /// host 팝업이 plugin 팝업 위인 패스 하나.
    fn host_on_top_pass(&self, ctx: &egui::Context) {
        run_pass(ctx, |ctx| {
            show_cover(ctx, self.cover, self.scope);
            paint_shell(ctx, self.shell, self.frame);
            show_host(ctx, self.host[0], self.scope);
            show_host(ctx, self.host[1], self.host_frame);
            enforce_host_plugin_popup_z_order(ctx, &self.host, &[self.cover, self.shell], true);
        });
    }
}

#[derive(Debug, Clone, Copy)]
enum Start {
    /// 처음 열림: 막이 Area가 먼저 순서에 들고 셸이 누름으로 그 뒤에 든다.
    CoverFirst,
    /// 워크스페이스를 오간 뒤: 다시 보인 막이 Area만 맨 위로 올라 셸 위에 있다.
    ShellFirst,
    /// 셸이 아직 순서에 없다(누름 전).
    ShellUnordered,
}

/// Given 덮는 plugin 팝업 위에 host 팝업, When 패스를 여러 번 돌리면, Then 매 패스 host 두 레이어가
/// plugin 두 레이어보다 위다. 셸이 host 위로 오른 패스가 마우스를 움직일 때마다 보이던 번쩍임이다.
#[test]
fn host_popup_layers_stay_above_both_plugin_cover_layers_every_pass() {
    for start in [Start::CoverFirst, Start::ShellFirst, Start::ShellUnordered] {
        let scene = Scene::new();
        let ctx = egui::Context::default();
        scene.start(&ctx, start);
        let mut wrong = Vec::new();
        for pass in 0..PASSES {
            scene.host_on_top_pass(&ctx);
            let plugin_top = position(&ctx, scene.cover).max(position(&ctx, scene.shell));
            let host_bottom = scene.host.iter().map(|l| position(&ctx, *l)).min();
            if host_bottom.is_none_or(|h| h <= plugin_top) {
                wrong.push(pass);
            }
        }
        assert!(
            wrong.is_empty(),
            "{start:?}: host popup was not above every plugin layer in passes {wrong:?}"
        );
    }
}

/// Given 워크스페이스를 오가 덮음 막이가 셸 위로 오른 순서, When host 팝업 위를 가리키면, Then 매 패스 포인터가
/// 걸린 레이어는 host 팝업이다. 막이가 host 위로 오른 패스에는 막이가 포인터를 가져가 호버·클릭이 사라졌다.
#[test]
fn pointer_over_a_host_popup_reaches_it_after_the_cover_rose_above_the_shell() {
    let scene = Scene::new();
    let ctx = egui::Context::default();
    scene.start(&ctx, Start::ShellFirst);
    let point = scene.host_frame.center();
    let mut lost = Vec::new();
    for pass in 0..PASSES {
        scene.host_on_top_pass(&ctx);
        if ctx.layer_id_at(point) != Some(scene.host[1]) {
            lost.push(pass);
        }
    }
    assert!(
        lost.is_empty(),
        "pointer over the host popup went to another layer in passes {lost:?}"
    );
}

/// Given plugin 팝업이 host 팝업 위, When 패스를 여러 번 돌리면, Then 매 패스 plugin 레이어가 host 두 레이어보다
/// 위다. 아래 묶음이 여러 레이어인 경우는 host 쪽에도 있다(덮는 host 팝업의 막이와 본체).
#[test]
fn plugin_popup_layers_stay_above_both_host_layers_every_pass() {
    let scene = Scene::new();
    let ctx = egui::Context::default();
    run_pass(&ctx, |ctx| {
        show_host(ctx, scene.host[1], scene.host_frame);
        show_host(ctx, scene.host[0], scene.scope);
    });
    let mut wrong = Vec::new();
    for pass in 0..PASSES {
        run_pass(&ctx, |ctx| {
            show_host(ctx, scene.host[1], scene.host_frame);
            show_host(ctx, scene.host[0], scene.scope);
            show_cover(ctx, scene.cover, scene.scope);
            paint_shell(ctx, scene.shell, scene.frame);
            enforce_host_plugin_popup_z_order(ctx, &scene.host, &[scene.cover, scene.shell], false);
        });
        let host_top = scene
            .host
            .iter()
            .map(|l| position(&ctx, *l))
            .max()
            .unwrap_or(0);
        let plugin_bottom = position(&ctx, scene.cover).min(position(&ctx, scene.shell));
        if plugin_bottom <= host_top {
            wrong.push(pass);
        }
    }
    assert!(
        wrong.is_empty(),
        "plugin popup was not above every host layer in passes {wrong:?}"
    );
}
