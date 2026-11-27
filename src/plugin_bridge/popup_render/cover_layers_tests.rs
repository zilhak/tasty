//! 덮는 plugin 팝업의 덮음 막이와 셸 순서를 패스에 걸쳐 본다.

use super::push_popup_layers;

fn shell() -> egui::LayerId {
    egui::LayerId::new(
        egui::Order::Foreground,
        egui::Id::new("plugin_mesh_popup").with(7_u64),
    )
}

fn scope() -> egui::Rect {
    egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(400.0, 300.0))
}

/// 팝업을 그리는 패스 하나. `press`이면 누름이 든 패스처럼 그 팝업의 레이어를 모두 함께 올린다.
fn draw_pass(ctx: &egui::Context, press: bool) -> Vec<egui::LayerId> {
    let mut layers = Vec::new();
    // 그리기 출력은 이 시험이 보지 않는다. 레이어 순서는 패스 뒤 Memory에서 읽는다.
    let _ = ctx.run(Default::default(), |ctx| {
        layers.clear();
        push_popup_layers(ctx, &mut layers, shell(), Some(scope()));
        ctx.layer_painter(shell()).rect_filled(
            egui::Rect::from_min_size(egui::pos2(100.0, 100.0), egui::vec2(200.0, 80.0)),
            0.0,
            egui::Color32::DARK_GRAY,
        );
        if press {
            for layer in &layers {
                ctx.move_to_top(*layer);
            }
        }
    });
    layers
}

/// 팝업을 그리지 않는 패스 하나(다른 워크스페이스를 보는 동안).
fn hidden_pass(ctx: &egui::Context) {
    // 그리기 출력은 이 시험이 보지 않는다.
    let _ = ctx.run(Default::default(), |_| {});
}

/// 직전 패스까지 정해진 순서에서 `layer`의 자리. 순서에 없으면 egui가 순서 있는 레이어 뒤에 그리므로 맨 위로 본다.
fn position(ctx: &egui::Context, layer: egui::LayerId) -> usize {
    ctx.memory(|m| m.layer_ids().position(|l| l == layer))
        .unwrap_or(usize::MAX)
}

/// 덮는 팝업은 막이를 먼저, 셸을 뒤에 넣는다. 덮지 않는 팝업은 셸 하나다.
#[test]
fn a_covering_popup_lists_its_cover_block_below_its_shell() {
    let ctx = egui::Context::default();
    let layers = draw_pass(&ctx, false);
    assert_eq!(layers.len(), 2);
    assert_eq!(layers[1], shell());
    assert!(position(&ctx, layers[0]) < position(&ctx, shell()));

    let mut plain = Vec::new();
    // 그리기 출력은 이 시험이 보지 않는다.
    let _ = ctx.run(Default::default(), |ctx| {
        push_popup_layers(ctx, &mut plain, shell(), None);
    });
    assert_eq!(plain, vec![shell()]);
}

/// Given 누름으로 순서에 든 덮는 팝업, When 워크스페이스를 오가 몇 패스 그리지 않았다가 다시 그리면, Then 막이가
/// 셸 아래에 남는다. 막이 Area만 다시 맨 위로 오르면 막이가 셸 위로 가고, host 팝업이 위에 뜬 동안 위 묶음의
/// 부모가 막이로 바뀐다.
#[test]
fn the_cover_block_stays_below_its_shell_when_the_popup_shows_again() {
    let ctx = egui::Context::default();
    let layers = draw_pass(&ctx, true);
    let cover = layers[0];
    assert!(
        position(&ctx, cover) < position(&ctx, shell()),
        "first show"
    );
    for round in 0..3 {
        hidden_pass(&ctx);
        hidden_pass(&ctx);
        draw_pass(&ctx, false);
        assert!(
            position(&ctx, cover) < position(&ctx, shell()),
            "round {round}: cover block rose above the shell after the popup showed again"
        );
        draw_pass(&ctx, true);
        assert!(
            position(&ctx, cover) < position(&ctx, shell()),
            "round {round}: press kept the order"
        );
    }
}
