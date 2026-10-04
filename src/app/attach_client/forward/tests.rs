use super::*;

use crate::ipc::stream::StructuralOp;
use serde_json::Value;
use std::collections::HashMap;

#[test]
fn an_agent_close_is_forwarded_with_the_agent_origin() {
    let origin_on_wire = |user_triggered| {
        let payload = structural_op_payload(
            3,
            tasty_ipc::stream::StructuralOp::CloseSurface { surface_id: 9 },
            user_triggered,
        );
        let v: Value = serde_json::from_slice(&payload).expect("json");
        assert_eq!(v["event"], "structural_op");
        v["origin"].clone()
    };
    assert_eq!(origin_on_wire(false), "agent");
    assert_eq!(origin_on_wire(true), "user");
}

#[test]
fn remote_structural_op_maps_the_move_target_to_its_remote_id() {
    // remote → local. 로컬 ID가 다른 원격 surface ID와 겹치는 배치다.
    let map: HashMap<u32, u32> = [(40, 3), (41, 4), (3, 9)].into_iter().collect();
    let local = StructuralOp::MoveSurface {
        source_surface_id: 4,
        target_surface_id: 3,
    };
    let wire = remote_structural_op(&local, 41, &map).expect("target mapped");
    assert_eq!(
        wire,
        StructuralOp::MoveSurface {
            source_surface_id: 41,
            target_surface_id: 40,
        },
        "target 도 원격 ID 로 보내야 한다 — 로컬 3 을 그대로 보내면 원격 surface 3 을 가리킨다"
    );
}

#[test]
fn remote_structural_op_drops_a_move_whose_target_is_not_mirrored() {
    let map: HashMap<u32, u32> = [(41, 4)].into_iter().collect();
    let local = StructuralOp::MoveSurface {
        source_surface_id: 4,
        target_surface_id: 7,
    };
    assert_eq!(remote_structural_op(&local, 41, &map), None);
}

#[test]
fn remote_structural_op_only_swaps_the_anchor_for_other_ops() {
    let local = StructuralOp::CloseSurface { surface_id: 4 };
    assert_eq!(
        remote_structural_op(&local, 41, &HashMap::new()),
        Some(StructuralOp::CloseSurface { surface_id: 41 })
    );
}
