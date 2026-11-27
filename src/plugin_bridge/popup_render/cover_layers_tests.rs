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

/// 패스 하나에 팝업이 그려지는 꼴.
#[derive(Clone, Copy)]
enum Shown {
    /// 덮는 범위를 덮는 펼친 팝업. 셸은 Area 없이 칠한다.
    Covering,
    /// 덮지 않는 접힌 팝업. `display::draw_collapsed`처럼 셸 id로 보기 버튼 Area를 띄운다.
    Collapsed,
}

/// 팝업 `popups`(아래에서 위 순서)를 그리는 패스 하나. `draw_plugin_popups`처럼 끝에서 올림을 적용하고,
/// `press`이면 누름이 든 패스처럼 모든 레이어를 함께 올린다. 그린 레이어를 아래에서 위 순서로 돌려준다.
fn draw_popups(
    ctx: &egui::Context,
    popups: &[(egui::LayerId, Shown)],
    press: bool,
) -> Vec<egui::LayerId> {
    let mut layers = Vec::new();
    // 그리기 출력은 이 시험이 보지 않는다. 레이어 순서는 패스 뒤 Memory에서 읽는다.
    let _ = ctx.run(Default::default(), |ctx| {
        layers.clear();
        let mut lift = PassLift::default();
        for (shell, shown) in popups {
            match shown {
                Shown::Covering => {
                    push_popup_layers(ctx, &mut layers, &mut lift, *shell, Some(scope()));
                    ctx.layer_painter(*shell).rect_filled(
                        egui::Rect::from_min_size(
                            egui::pos2(100.0, 100.0),
                            egui::vec2(200.0, 80.0),
                        ),
                        0.0,
                        egui::Color32::DARK_GRAY,
                    );
                }
                Shown::Collapsed => {
                    push_popup_layers(ctx, &mut layers, &mut lift, *shell, None);
                    egui::Area::new(shell.id)
                        .order(egui::Order::Foreground)
                        .fixed_pos(egui::pos2(300.0, 250.0))
                        .interactable(true)
                        .show(ctx, |ui| {
                            ui.allocate_exact_size(egui::vec2(40.0, 20.0), egui::Sense::click());
                        });
                }
            }
        }
        if press {
            lift.request();
        }
        lift.apply(ctx, &layers);
    });
    layers
}

/// 덮는 팝업 `shells`(아래에서 위 순서)만 그리는 패스 하나.
fn draw_pass(ctx: &egui::Context, shells: &[egui::LayerId], press: bool) -> Vec<egui::LayerId> {
    let popups: Vec<_> = shells.iter().map(|s| (*s, Shown::Covering)).collect();
    draw_popups(ctx, &popups, press)
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

/// Given 덮지 않고 접혀 셸 id의 보기 버튼 Area로 먼저 순서에 든 팝업 A와 그 위의 덮는 팝업 B, When A를 펼쳐
/// 덮게 되면(막이가 셸 뒤에 처음 등록된다), Then 펼친 패스 하나만 순서가 틀리고 다음 패스부터 A 막이·A 셸·B 막이·
/// B 셸 순서가 된다. 펼친 패스에는 egui가 처음 보인 막이 Area를 스스로 올려 되돌릴 수 없다.
#[test]
fn a_collapsed_popup_that_expands_to_cover_puts_its_cover_block_below_its_shell() {
    let ctx = egui::Context::default();
    let (a, b) = (shell(1), shell(2));
    for _ in 0..2 {
        draw_popups(&ctx, &[(a, Shown::Collapsed), (b, Shown::Covering)], false);
    }
    let mut wrong = Vec::new();
    for pass in 0..6 {
        let layers = draw_pass(&ctx, &[a, b], false);
        if !stacked_in_order(&ctx, &layers) {
            wrong.push(pass);
        }
    }
    // 틀린 패스는 펼친 패스(0) 하나다.
    assert_eq!(
        wrong,
        vec![0],
        "only the expand pass may stack the cover block above its shell"
    );
}
