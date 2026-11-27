//! plugin 팝업 하나의 egui 레이어를 아래에서 위 순서로 모은다. 덮는 팝업은 덮음 막이 Area와 셸 두 레이어다.

use egui::{Context, LayerId, Rect};

/// 이번 패스에 그릴 plugin 팝업 하나의 레이어를 `layers`에 넣는다. 덮는 팝업(`cover`가 덮는 범위)은 덮음 막이
/// Area를 먼저 등록해 그 범위의 아래 위젯 입력을 막고, 셸(`shell`)을 그 뒤에 넣는다.
///
/// 막이는 Area라 직전 패스에 보이지 않았으면 egui가 혼자 맨 위로 올린다. 셸은 Area 없이 칠하는 레이어라 함께
/// 오르지 않아, 숨었다가 다시 보인 패스(워크스페이스를 오갈 때)에 막이가 셸 위로 간다. 그 패스에 셸도 올리면
/// egui가 함께 올린 레이어끼리의 기존 순서를 지키므로, 처음 보인 패스에 정한 막이 아래·셸 위 순서가 남는다.
/// 누름으로 막이가 오르는 패스에는 `draw_plugin_popups` 끝에서 plugin 레이어를 모두 함께 올린다.
pub(super) fn push_popup_layers(
    ctx: &Context,
    layers: &mut Vec<LayerId>,
    shell: LayerId,
    cover: Option<Rect>,
) {
    if let Some(scope) = cover {
        let block_id = shell.id.with("cover_block");
        let block = LayerId::new(shell.order, block_id);
        let reappears = !ctx.memory(|m| m.areas().visible_last_frame(&block));
        layers.push(crate::adapters::ui::popup::press::block_pointer(
            ctx, block_id, scope,
        ));
        if reappears {
            ctx.move_to_top(shell);
        }
    }
    layers.push(shell);
}

#[cfg(test)]
#[path = "cover_layers_tests.rs"]
mod tests;
