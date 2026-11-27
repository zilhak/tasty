//! plugin 팝업 하나의 egui 레이어를 아래에서 위 순서로 모은다. 덮는 팝업은 덮음 막이 Area와 셸 두 레이어다.

use egui::{Context, LayerId, Rect};

/// 이번 패스에 plugin 레이어를 모두 함께 올려야 하는지 모은다. 누름이 든 패스, 막이가 다시 보인 패스, 막이가
/// 셸 위에 있는 패스다. `keep`은 그 올림에서 빼 제자리에 두는 막이다.
#[derive(Default)]
pub(super) struct PassLift {
    all: bool,
    keep: Vec<LayerId>,
}

impl PassLift {
    /// 이번 패스에 plugin 레이어를 모두 올리게 한다.
    pub(super) fn request(&mut self) {
        self.all = true;
    }

    /// 이번 패스에 `block`만 빼고 plugin 레이어를 모두 올리게 한다. 함께 올린 레이어끼리는 순서가 그대로라,
    /// 셸 위에 있는 막이를 셸 아래로 되돌리려면 막이를 올림에서 빼야 한다.
    fn keep_below(&mut self, block: LayerId) {
        self.all = true;
        self.keep.push(block);
    }

    /// 올림이 필요한 패스면 이번 패스의 plugin 레이어(`layers`, 아래에서 위 순서)를 `keep`만 빼고 모두
    /// `move_to_top`한다. egui는 함께 올린 레이어끼리 기존 순서를 지키고, 순서에 없던 레이어는 이 순서대로 끝에 붙인다.
    pub(super) fn apply(&self, ctx: &Context, layers: &[LayerId]) {
        if self.all {
            for layer in layers.iter().filter(|l| !self.keep.contains(l)) {
                ctx.move_to_top(*layer);
            }
        }
    }
}

/// 이번 패스에 그릴 plugin 팝업 하나의 레이어를 `layers`에 넣는다. 덮는 팝업(`cover`가 덮는 범위)은 덮음 막이
/// Area를 먼저 등록해 그 범위의 아래 위젯 입력을 막고, 셸(`shell`)을 그 뒤에 넣는다.
///
/// 막이는 Area라 직전 패스에 보이지 않았으면 egui가 혼자 맨 위로 올린다. 셸은 Area 없이 칠하는 레이어라 함께
/// 오르지 않아, 숨었다가 다시 보인 패스(워크스페이스를 오갈 때)에 막이가 셸 위로 간다. 그 패스에는 `lift`로
/// 이번 패스 plugin 레이어를 모두 함께 올린다. 셸만 올리면 다시 보인 팝업이 나중에 연 다른 plugin 팝업 위로 간다.
/// 셸이 아직 순서에 없으면(처음 보인 패스) 막이 바로 뒤에 바로 등록한다. 패스 끝 올림까지 미루면 그 사이에
/// 위 팝업의 막이 Area가 먼저 등록돼, 이 셸이 위 팝업의 막이보다 위에 든다.
///
/// 셸이 막이보다 먼저 순서에 든 경우(접힌 팝업은 셸 id로 Area를 띄우므로, 덮지 않고 접혀 있던 팝업을 펼쳐 덮게
/// 되면 막이가 셸 뒤에 등록된다)에는 함께 올려도 막이가 셸 위에 남는다. 이번 패스까지 쌓인 순서 벡터
/// (`Memory::layer_ids`)에서 막이가 셸 뒤에 있으면 그 막이만 빼고 올려 셸 아래로 되돌린다. 막이가 처음 보인 패스에는
/// egui가 막이 Area를 스스로 올리므로 되돌리지 못하고, 다음 패스에 되돌린다. 순서가 틀린 패스는 펼친 패스 1개다.
/// 막이를 누른 패스도 egui가 막이를 올리므로, 되돌림은 누름이 없는 다음 패스로 미뤄진다.
pub(super) fn push_popup_layers(
    ctx: &Context,
    layers: &mut Vec<LayerId>,
    lift: &mut PassLift,
    shell: LayerId,
    cover: Option<Rect>,
) {
    if let Some(scope) = cover {
        let block_id = shell.id.with("cover_block");
        let block = LayerId::new(shell.order, block_id);
        let (reappears, above_shell) = ctx.memory(|m| {
            let at = |layer: LayerId| m.layer_ids().position(|l| l == layer);
            let above_shell = at(block).zip(at(shell)).is_some_and(|(b, s)| b > s);
            (!m.areas().visible_last_frame(&block), above_shell)
        });
        if above_shell {
            lift.keep_below(block);
        } else if reappears {
            lift.request();
        }
        layers.push(crate::adapters::ui::popup::press::block_pointer(
            ctx, block_id, scope,
        ));
        if !ctx.memory(|m| m.layer_ids().any(|l| l == shell)) {
            ctx.move_to_top(shell);
        }
    }
    layers.push(shell);
}

#[cfg(test)]
#[path = "cover_layers_tests.rs"]
mod tests;
