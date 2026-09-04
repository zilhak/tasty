use super::{
    BinaryTree, DividerInfo, FocusDirection, Pane, PaneId, PhysicalPx, PhysicalRect,
    SplitDirection, SurfaceId,
};

/// Binary tree of Panes - physical screen splits.
/// Each leaf is a Pane with its own independent tab bar.
pub enum PaneNode {
    Leaf(Pane),
    Split {
        direction: SplitDirection,
        ratio: f32,
        first: Box<PaneNode>,
        second: Box<PaneNode>,
    },
}

impl BinaryTree for PaneNode {
    type Id = PaneId;
    /// pane 보더는 **논리**라 배율을 받아 물리로 내린다.
    fn border_width(scale_factor: f32) -> PhysicalPx {
        crate::PANE_BORDER_WIDTH.to_physical(scale_factor)
    }

    fn split_parts(&self) -> Option<(SplitDirection, f32, &Self, &Self)> {
        match self {
            PaneNode::Leaf(_) => None,
            PaneNode::Split {
                direction,
                ratio,
                first,
                second,
            } => Some((*direction, *ratio, first, second)),
        }
    }

    fn split_parts_mut(&mut self) -> Option<(SplitDirection, &mut f32, &mut Self, &mut Self)> {
        match self {
            PaneNode::Leaf(_) => None,
            PaneNode::Split {
                direction,
                ratio,
                first,
                second,
            } => Some((*direction, ratio, &mut **first, &mut **second)),
        }
    }

    fn leaf_id(&self) -> Option<PaneId> {
        match self {
            PaneNode::Leaf(p) => Some(p.id),
            _ => None,
        }
    }
}

impl PaneNode {
    /// Split the target pane in-place. The new `Pane` must be pre-created by the caller
    /// (so PTY creation happens before any structural mutation).
    ///
    /// API: returns `Some(new_pane)` if NOT found (caller can decide what to do),
    /// returns `None` if found and split was performed.
    pub fn split_pane_in_place(
        &mut self,
        target_id: PaneId,
        direction: SplitDirection,
        new_pane: Pane,
    ) -> Option<Pane> {
        match self {
            PaneNode::Leaf(pane) if pane.id == target_id => {
                let new_pane_id = new_pane.id;
                // Replace self (Leaf(orig)) with Leaf(new_pane); returns Leaf(orig).
                let original_leaf = std::mem::replace(self, PaneNode::Leaf(new_pane));
                // Replace self (Leaf(new_pane)) with the final Split; returns Leaf(new_pane).
                let new_leaf = std::mem::replace(
                    self,
                    PaneNode::Split {
                        direction,
                        ratio: 0.5,
                        first: Box::new(original_leaf),
                        // placeholder - replaced immediately below
                        second: Box::new(PaneNode::Leaf(Pane {
                            id: new_pane_id,
                            tabs: vec![],
                            active_tab: 0,
                            tab_scroll_offset: 0.0,
                        })),
                    },
                );
                // Put the real new leaf into second.
                if let PaneNode::Split { second, .. } = self {
                    **second = new_leaf;
                }
                None // success
            }
            PaneNode::Leaf(_) => Some(new_pane), // not found, return pane back
            PaneNode::Split { first, second, .. } => {
                // Try first; if not found, new_pane is returned and we try second.
                let remaining = first.split_pane_in_place(target_id, direction, new_pane);
                if let Some(pane) = remaining {
                    second.split_pane_in_place(target_id, direction, pane)
                } else {
                    None // success in first
                }
            }
        }
    }

    /// Remove a pane from the tree by promoting its sibling.
    /// Returns true if the pane was found and removed.
    /// Returns false for root leaf (can't close the only pane).
    pub fn close_pane(&mut self, target_id: PaneId) -> bool {
        match self {
            PaneNode::Leaf(_) => false, // Can't close the root pane
            PaneNode::Split { first, second, .. } => {
                // Check if first child is the target leaf
                let first_is_target =
                    matches!(first.as_ref(), PaneNode::Leaf(p) if p.id == target_id);
                let second_is_target =
                    matches!(second.as_ref(), PaneNode::Leaf(p) if p.id == target_id);

                if first_is_target {
                    // Remove first, promote second
                    let old = std::mem::replace(
                        self,
                        PaneNode::Leaf(Pane {
                            id: 0,
                            tabs: vec![],
                            active_tab: 0,
                            tab_scroll_offset: 0.0,
                        }),
                    );
                    if let PaneNode::Split { second, .. } = old {
                        *self = *second;
                    }
                    return true;
                }
                if second_is_target {
                    let old = std::mem::replace(
                        self,
                        PaneNode::Leaf(Pane {
                            id: 0,
                            tabs: vec![],
                            active_tab: 0,
                            tab_scroll_offset: 0.0,
                        }),
                    );
                    if let PaneNode::Split { first, .. } = old {
                        *self = *first;
                    }
                    return true;
                }
                // Recurse into children
                first.close_pane(target_id) || second.close_pane(target_id)
            }
        }
    }

    /// Return a reference to the first (leftmost/topmost) pane in the tree.
    pub fn first_pane(&self) -> Option<&Pane> {
        match self {
            PaneNode::Leaf(pane) => Some(pane),
            PaneNode::Split { first, .. } => first.first_pane(),
        }
    }

    /// Locate the immediate parent `Split` of the leaf `target_id`, returning
    /// `(direction, ratio, target_was_first, sibling_anchor_id)` — enough
    /// geometry to splice a closed pane back in at roughly the same spot via
    /// [`insert_pane_beside`](Self::insert_pane_beside). `sibling_anchor_id`
    /// is the first (leftmost/topmost) pane of whichever side did *not*
    /// contain `target_id`, since that side may itself be a subtree with
    /// multiple panes — any leaf within it is a valid, still-live anchor to
    /// re-split against later. Returns `None` if `target_id` is the tree
    /// root (no parent split) or isn't found.
    pub fn locate_split_context(
        &self,
        target_id: PaneId,
    ) -> Option<(SplitDirection, f32, bool, PaneId)> {
        match self {
            PaneNode::Leaf(_) => None,
            PaneNode::Split {
                direction,
                ratio,
                first,
                second,
            } => {
                let first_is_target =
                    matches!(first.as_ref(), PaneNode::Leaf(p) if p.id == target_id);
                let second_is_target =
                    matches!(second.as_ref(), PaneNode::Leaf(p) if p.id == target_id);
                if first_is_target {
                    let sibling_anchor = second.first_pane()?.id;
                    return Some((*direction, *ratio, true, sibling_anchor));
                }
                if second_is_target {
                    let sibling_anchor = first.first_pane()?.id;
                    return Some((*direction, *ratio, false, sibling_anchor));
                }
                first
                    .locate_split_context(target_id)
                    .or_else(|| second.locate_split_context(target_id))
            }
        }
    }

    /// Reinsert a previously-closed pane next to `target_id`, replicating the
    /// original split geometry captured at close time by
    /// [`locate_split_context`](Self::locate_split_context) — unlike
    /// [`split_pane_in_place`](Self::split_pane_in_place) (always ratio 0.5,
    /// new pane placed as `second`), this lets the caller pick the exact side
    /// and ratio, so a close→restore round-trip recreates (approximately) the
    /// original layout instead of always defaulting to an even 50/50 split.
    ///
    /// Returns `Some(new_pane)` if `target_id` was NOT found (caller decides
    /// fallback), `None` if found and the split was performed.
    pub fn insert_pane_beside(
        &mut self,
        target_id: PaneId,
        direction: SplitDirection,
        ratio: f32,
        new_pane: Pane,
        new_pane_is_first: bool,
    ) -> Option<Pane> {
        let is_target_leaf = matches!(self, PaneNode::Leaf(p) if p.id == target_id);
        if is_target_leaf {
            let placeholder = PaneNode::Leaf(Pane {
                id: 0,
                tabs: vec![],
                active_tab: 0,
                tab_scroll_offset: 0.0,
            });
            let original = std::mem::replace(self, placeholder);
            let (first, second) = if new_pane_is_first {
                (PaneNode::Leaf(new_pane), original)
            } else {
                (original, PaneNode::Leaf(new_pane))
            };
            *self = PaneNode::Split {
                direction,
                ratio,
                first: Box::new(first),
                second: Box::new(second),
            };
            return None;
        }
        match self {
            PaneNode::Leaf(_) => Some(new_pane),
            PaneNode::Split { first, second, .. } => {
                match first.insert_pane_beside(
                    target_id,
                    direction,
                    ratio,
                    new_pane,
                    new_pane_is_first,
                ) {
                    Some(pane) => second.insert_pane_beside(
                        target_id,
                        direction,
                        ratio,
                        pane,
                        new_pane_is_first,
                    ),
                    None => None,
                }
            }
        }
    }

    /// Find a Pane by ID (immutable).
    pub fn find_pane(&self, id: PaneId) -> Option<&Pane> {
        match self {
            PaneNode::Leaf(pane) => {
                if pane.id == id {
                    Some(pane)
                } else {
                    None
                }
            }
            PaneNode::Split { first, second, .. } => {
                first.find_pane(id).or_else(|| second.find_pane(id))
            }
        }
    }

    /// Find a Pane by ID (mutable).
    pub fn find_pane_mut(&mut self, id: PaneId) -> Option<&mut Pane> {
        match self {
            PaneNode::Leaf(pane) => {
                if pane.id == id {
                    Some(pane)
                } else {
                    None
                }
            }
            PaneNode::Split { first, second, .. } => {
                if let Some(p) = first.find_pane_mut(id) {
                    Some(p)
                } else {
                    second.find_pane_mut(id)
                }
            }
        }
    }

    /// Collect all surface IDs across all panes in this tree.
    pub fn all_surface_ids(&self) -> Vec<SurfaceId> {
        match self {
            PaneNode::Leaf(pane) => pane.all_surface_ids(),
            PaneNode::Split { first, second, .. } => {
                let mut result = first.all_surface_ids();
                result.extend(second.all_surface_ids());
                result
            }
        }
    }

    /// attach 단계 J 후속: pane 레벨 이진트리를 JSON 으로 직렬화(direction/ratio
    /// 보존). `SurfaceLayout::to_tree_json_full` 과 대칭 — Leaf 는 `Pane::to_attach_json`
    /// (id+tabs, 기존 평면 "panes" 원소와 동일 shape)에 `"type":"Leaf"` 를 얹고,
    /// Split 은 direction/ratio/first/second 를 담는다.
    pub fn to_tree_json_full(&self) -> serde_json::Value {
        match self {
            PaneNode::Leaf(pane) => {
                let mut v = pane.to_attach_json();
                v["type"] = serde_json::json!("Leaf");
                v
            }
            PaneNode::Split {
                direction,
                ratio,
                first,
                second,
            } => serde_json::json!({
                "type": "Split",
                "direction": match direction {
                    SplitDirection::Horizontal => "horizontal",
                    SplitDirection::Vertical => "vertical",
                },
                "ratio": ratio,
                "first": first.to_tree_json_full(),
                "second": second.to_tree_json_full(),
            }),
        }
    }

    // ─── BinaryTree alias (외부 caller 0 변경 보장) ────────────────────────
    //
    // 외부 호출처들은 `crate::model::BinaryTree` 를 import 하지 않으므로
    // trait method 가 dot-call scope 에 없다. 따라서 trait 와 *이름이 같은*
    // 메서드들은 inherent alias 로 보존하고, UFCS 로 trait 본체에 위임한다
    // (`self.compute_rects(...)` 로 호출하면 inherent 가 재선택되어 무한 재귀).
    // 이름이 다른 id-시리즈 (`all_pane_ids` / `next_pane_id` / `prev_pane_id`) 도
    // 동일한 위임 패턴으로 보존한다.

    pub fn all_pane_ids(&self) -> Vec<PaneId> {
        <Self as BinaryTree>::all_ids(self)
    }

    pub fn next_pane_id(&self, current: PaneId) -> PaneId {
        <Self as BinaryTree>::next_id(self, current)
    }

    pub fn prev_pane_id(&self, current: PaneId) -> PaneId {
        <Self as BinaryTree>::prev_id(self, current)
    }

    pub fn compute_rects(
        &self,
        rect: PhysicalRect,
        scale_factor: f32,
    ) -> Vec<(PaneId, PhysicalRect)> {
        <Self as BinaryTree>::compute_rects(self, rect, scale_factor)
    }

    pub fn collect_dividers(&self, rect: PhysicalRect, scale_factor: f32) -> Vec<PhysicalRect> {
        <Self as BinaryTree>::collect_dividers(self, rect, scale_factor)
    }

    pub fn find_divider_at(
        &self,
        x: f32,
        y: f32,
        rect: PhysicalRect,
        threshold: f32,
        scale_factor: f32,
    ) -> Option<DividerInfo> {
        <Self as BinaryTree>::find_divider_at(self, x, y, rect, threshold, scale_factor)
    }

    pub fn update_ratio_for_rect(
        &mut self,
        split_rect: PhysicalRect,
        new_ratio: f32,
        current_rect: PhysicalRect,
        scale_factor: f32,
    ) -> bool {
        <Self as BinaryTree>::update_ratio_for_rect(
            self,
            split_rect,
            new_ratio,
            current_rect,
            scale_factor,
        )
    }

    pub fn directional_focus(
        &self,
        current_pane_id: PaneId,
        direction: FocusDirection,
    ) -> Option<PaneId> {
        <Self as BinaryTree>::directional_focus(self, current_pane_id, direction)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn leaf_pane(id: PaneId) -> PaneNode {
        PaneNode::Leaf(Pane {
            id,
            tabs: vec![],
            active_tab: 0,
            tab_scroll_offset: 0.0,
        })
    }

    #[test]
    fn to_tree_json_full_serializes_split_direction_and_ratio() {
        let node = PaneNode::Split {
            direction: SplitDirection::Vertical,
            ratio: 0.3,
            first: Box::new(leaf_pane(7)),
            second: Box::new(leaf_pane(8)),
        };
        let json = node.to_tree_json_full();
        assert_eq!(json["type"], "Split");
        assert_eq!(json["direction"], "vertical");
        assert!((json["ratio"].as_f64().unwrap() - 0.3).abs() < 1e-6);
        assert_eq!(json["first"]["id"], 7);
        assert_eq!(json["second"]["id"], 8);
    }

    #[test]
    fn to_tree_json_full_leaf_carries_pane_payload() {
        let node = leaf_pane(9);
        let json = node.to_tree_json_full();
        assert_eq!(json["type"], "Leaf");
        assert_eq!(json["id"], 9);
        assert!(json["tabs"].is_array());
    }
}
