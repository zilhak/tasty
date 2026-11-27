//! 덮는 plugin 팝업의 덮음 막이와 셸 순서를 패스에 걸쳐 본다.

use super::{PassLift, push_popup_layers};

fn shell(n: u64) -> egui::LayerId {
    egui::LayerId::new(
        egui::Order::Foreground,
        egui::Id::new("plugin_mesh_popup").with(n),
    )
}

fn scope() -> egui::Rect {
    egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(400.0, 300.0))
}

/// 덮는 팝업 `shells`(아래에서 위 순서)를 그리는 패스 하나. `draw_plugin_popups`처럼 끝에서 올림을 적용하고,
/// `press`이면 누름이 든 패스처럼 모든 레이어를 함께 올린다. 그린 레이어를 아래에서 위 순서로 돌려준다.
fn draw_pass(ctx: &egui::Context, shells: &[egui::LayerId], press: bool) -> Vec<egui::LayerId> {
    let mut layers = Vec::new();
    // 그리기 출력은 이 시험이 보지 않는다. 레이어 순서는 패스 뒤 Memory에서 읽는다.
    let _ = ctx.run(Default::default(), |ctx| {
        layers.clear();
        let mut lift = PassLift::default();
        for shell in shells {
            push_popup_layers(ctx, &mut layers, &mut lift, *shell, Some(scope()));
            ctx.layer_painter(*shell).rect_filled(
                egui::Rect::from_min_size(egui::pos2(100.0, 100.0), egui::vec2(200.0, 80.0)),
                0.0,
                egui::Color32::DARK_GRAY,
            );
        }
        if press {
            lift.request();
        }
        lift.apply(ctx, &layers);
    });
    layers
}

/// 팝업을 그리지 않는 패스 하나(다른 워크스페이스를 보는 동안).
fn hidden_pass(ctx: &egui::Context) {
    // 그리기 출력은 이 시험이 보지 않는다.
    let _ = ctx.run(Default::default(), |_| {});
}

/// 직전 패스까지 정해진 순서에서 `layer`의 자리. 순서에 없으면 `None`이다.
fn position(ctx: &egui::Context, layer: egui::LayerId) -> Option<usize> {
    ctx.memory(|m| m.layer_ids().position(|l| l == layer))
}

/// `layers`가 모두 순서에 있고 이 차례대로 아래에서 위로 쌓였는가.
fn stacked_in_order(ctx: &egui::Context, layers: &[egui::LayerId]) -> bool {
    let at: Option<Vec<usize>> = layers.iter().map(|l| position(ctx, *l)).collect();
    at.is_some_and(|at| at.windows(2).all(|w| w[0] < w[1]))
}

/// 덮는 팝업은 막이를 먼저, 셸을 뒤에 넣고, 처음 그린 패스에 둘 다 순서에 든다(셸이 첫 누름을 기다리지 않는다).
/// 덮지 않는 팝업은 셸 하나다.
#[test]
fn a_covering_popup_lists_its_cover_block_below_its_shell() {
    let ctx = egui::Context::default();
    let layers = draw_pass(&ctx, &[shell(7)], false);
    assert_eq!(layers.len(), 2);
    assert_eq!(layers[1], shell(7));
    assert!(
        position(&ctx, shell(7)).is_some(),
        "the shell joined the order on its first pass"
    );
    assert!(stacked_in_order(&ctx, &layers));

    let mut plain = Vec::new();
    // 그리기 출력은 이 시험이 보지 않는다.
    let _ = ctx.run(Default::default(), |ctx| {
        push_popup_layers(ctx, &mut plain, &mut PassLift::default(), shell(8), None);
    });
    assert_eq!(plain, vec![shell(8)]);
}

/// Given 누름으로 순서에 든 덮는 팝업, When 워크스페이스를 오가 몇 패스 그리지 않았다가 다시 그리면, Then 막이가
/// 셸 아래에 남는다. 막이 Area만 다시 맨 위로 오르면 막이가 셸 위로 가고, host 팝업이 위에 뜬 동안 위 묶음의
/// 부모가 막이로 바뀐다.
#[test]
fn the_cover_block_stays_below_its_shell_when_the_popup_shows_again() {
    let ctx = egui::Context::default();
    let layers = draw_pass(&ctx, &[shell(7)], true);
    assert!(stacked_in_order(&ctx, &layers), "first show");
    for round in 0..3 {
        hidden_pass(&ctx);
        hidden_pass(&ctx);
        draw_pass(&ctx, &[shell(7)], false);
        assert!(
            stacked_in_order(&ctx, &layers),
            "round {round}: cover block rose above the shell after the popup showed again"
        );
        draw_pass(&ctx, &[shell(7)], true);
        assert!(
            stacked_in_order(&ctx, &layers),
            "round {round}: press kept the order"
        );
    }
}

/// Given 덮는 plugin 팝업 A(아래)·B(위), When A의 범위만 몇 패스 안 보였다가 A가 다시 보이면, Then A의 막이·셸이
/// 둘 다 B 아래에 남는다. 다시 보인 A의 셸만 올리면 A 셸이 나중에 연 B 위에 그려진다.
#[test]
fn a_popup_that_shows_again_stays_below_a_later_popup() {
    let ctx = egui::Context::default();
    let (a, b) = (shell(1), shell(2));
    draw_pass(&ctx, &[a, b], true);
    draw_pass(&ctx, &[b], false);
    draw_pass(&ctx, &[b], false);
    let layers = draw_pass(&ctx, &[a, b], false);
    let at: Vec<Option<usize>> = layers.iter().map(|l| position(&ctx, *l)).collect();
    assert!(
        stacked_in_order(&ctx, &layers),
        "A cover, A shell, B cover, B shell should stack in that order, got positions {at:?}"
    );
}
