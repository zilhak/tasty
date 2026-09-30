use super::{DividerInfo, FocusDirection, PhysicalPx, PhysicalRect, SplitDirection};

/// Geometry of a borrowed layout with transient ratio overrides. No leaf payload is cloned.
pub struct TreeGeometry<Id> {
    pub leaves: Vec<(Id, PhysicalRect)>,
    pub dividers: Vec<(Vec<bool>, DividerInfo, f32)>,
}

/// Common binary-tree surface for `PaneNode` and `SurfaceLayout`.
///
/// Leaf payloads differ per enum (`Pane` vs `Box<dyn Surface>`) and the
/// `Split` variant carries different side fields (`SplitNodeId` only on
/// `SurfaceLayout`). This trait abstracts *only the recursive structure*;
/// leaf-touching mutation and lookup stays in each enum's inherent impl.
pub trait BinaryTree: Sized {
    type Id: Copy + Eq;

    /// 두 자식 사이 간격의 물리 두께. pane의 논리 두께에는 배율을 적용하고
    /// surface의 물리 두께에는 적용하지 않는다.
    fn border_width(scale_factor: f32) -> PhysicalPx;

    /// `Split` 일 때 (direction, ratio, &first, &second). `Leaf` 면 None.
    fn split_parts(&self) -> Option<(SplitDirection, f32, &Self, &Self)>;

    /// Split의 가변 접근자. ratio와 두 자식만 노출하고 SplitNodeId 같은 부가 상태는 제외한다.
    fn split_parts_mut(&mut self) -> Option<(SplitDirection, &mut f32, &mut Self, &mut Self)>;

    /// `Leaf` 일 때 그 id. `Split` 이면 None.
    fn leaf_id(&self) -> Option<Self::Id>;

    // ---------- default 메서드 (구조 재귀, leaf-agnostic) ----------

    fn first_id(&self) -> Option<Self::Id> {
        match self.split_parts() {
            None => self.leaf_id(),
            Some((_, _, first, _)) => first.first_id(),
        }
    }

    fn all_ids(&self) -> Vec<Self::Id> {
        match self.split_parts() {
            None => self.leaf_id().into_iter().collect(),
            Some((_, _, first, second)) => {
                let mut v = first.all_ids();
                v.extend(second.all_ids());
                v
            }
        }
    }

    fn next_id(&self, current: Self::Id) -> Self::Id {
        let ids = self.all_ids();
        if ids.len() <= 1 {
            return current;
        }
        let pos = ids.iter().position(|i| *i == current).unwrap_or(0);
        ids[(pos + 1) % ids.len()]
    }

    fn prev_id(&self, current: Self::Id) -> Self::Id {
        let ids = self.all_ids();
        if ids.len() <= 1 {
            return current;
        }
        let pos = ids.iter().position(|i| *i == current).unwrap_or(0);
        ids[(pos + ids.len() - 1) % ids.len()]
    }

    fn preview_geometry(
        &self,
        rect: PhysicalRect,
        scale_factor: f32,
        ratio_at: &impl Fn(&[bool], f32) -> f32,
    ) -> TreeGeometry<Self::Id> {
        fn visit<T: BinaryTree>(
            tree: &T,
            rect: PhysicalRect,
            scale: f32,
            path: &mut Vec<bool>,
            ratios: &impl Fn(&[bool], f32) -> f32,
            out: &mut TreeGeometry<T::Id>,
        ) {
            if let Some((direction, ratio, first, second)) = tree.split_parts() {
                let ratio = ratios(path, ratio);
                out.dividers.push((
                    path.clone(),
                    DividerInfo {
                        direction,
                        split_rect: rect,
                    },
                    ratio,
                ));
                let (a, b) = rect.split_with_gap(direction, ratio, T::border_width(scale));
                path.push(false);
                visit(first, a, scale, path, ratios, out);
                path.pop();
                path.push(true);
                visit(second, b, scale, path, ratios, out);
                path.pop();
            } else if let Some(id) = tree.leaf_id() {
                out.leaves.push((id, rect));
            }
        }
        let mut out = TreeGeometry {
            leaves: Vec::new(),
            dividers: Vec::new(),
        };
        visit(
            self,
            rect,
            scale_factor,
            &mut Vec::new(),
            ratio_at,
            &mut out,
        );
        out
    }

    fn compute_rects(
        &self,
        rect: PhysicalRect,
        scale_factor: f32,
    ) -> Vec<(Self::Id, PhysicalRect)> {
        match self.split_parts() {
            None => self
                .leaf_id()
                .map(|id| vec![(id, rect)])
                .unwrap_or_default(),
            Some((dir, ratio, first, second)) => {
                let (r1, r2) = rect.split_with_gap(dir, ratio, Self::border_width(scale_factor));
                let mut v = first.compute_rects(r1, scale_factor);
                v.extend(second.compute_rects(r2, scale_factor));
                v
            }
        }
    }

    fn collect_dividers(&self, rect: PhysicalRect, scale_factor: f32) -> Vec<PhysicalRect> {
        match self.split_parts() {
            None => vec![],
            Some((dir, ratio, first, second)) => {
                let gap = Self::border_width(scale_factor);
                let (r1, r2) = rect.split_with_gap(dir, ratio, gap);
                let divider = match dir {
                    SplitDirection::Vertical => PhysicalRect {
                        x: r1.x + r1.width,
                        y: rect.y,
                        width: gap,
                        height: rect.height,
                    },
                    SplitDirection::Horizontal => PhysicalRect {
                        x: rect.x,
                        y: r1.y + r1.height,
                        width: rect.width,
                        height: gap,
                    },
                };
                let mut v = vec![divider];
                v.extend(first.collect_dividers(r1, scale_factor));
                v.extend(second.collect_dividers(r2, scale_factor));
                v
            }
        }
    }

    fn find_divider_at(
        &self,
        x: f32,
        y: f32,
        rect: PhysicalRect,
        threshold: f32,
        scale_factor: f32,
    ) -> Option<DividerInfo> {
        let (dir, ratio, first, second) = self.split_parts()?;
        let (r1, r2) = rect.split_with_gap(dir, ratio, Self::border_width(scale_factor));
        let divider_pos = match dir {
            SplitDirection::Vertical => (r1.x + r1.width).value(),
            SplitDirection::Horizontal => (r1.y + r1.height).value(),
        };
        let cursor_pos = match dir {
            SplitDirection::Vertical => x,
            SplitDirection::Horizontal => y,
        };
        let in_bounds = match dir {
            SplitDirection::Vertical => y >= rect.y.value() && y < (rect.y + rect.height).value(),
            SplitDirection::Horizontal => x >= rect.x.value() && x < (rect.x + rect.width).value(),
        };
        if in_bounds && (cursor_pos - divider_pos).abs() < threshold {
            return Some(DividerInfo {
                direction: dir,
                split_rect: rect,
            });
        }
        first
            .find_divider_at(x, y, r1, threshold, scale_factor)
            .or_else(|| second.find_divider_at(x, y, r2, threshold, scale_factor))
    }

    fn update_ratio_for_rect(
        &mut self,
        split_rect: PhysicalRect,
        new_ratio: f32,
        current_rect: PhysicalRect,
        scale_factor: f32,
    ) -> bool {
        let border = Self::border_width(scale_factor);
        let Some((dir, ratio, first, second)) = self.split_parts_mut() else {
            return false;
        };
        if current_rect.approx_eq(&split_rect) {
            *ratio = new_ratio.clamp(0.1, 0.9);
            return true;
        }
        let ratio_val = *ratio;
        let (r1, r2) = current_rect.split_with_gap(dir, ratio_val, border);
        first.update_ratio_for_rect(split_rect, new_ratio, r1, scale_factor)
            || second.update_ratio_for_rect(split_rect, new_ratio, r2, scale_factor)
    }

    fn directional_focus(&self, current: Self::Id, direction: FocusDirection) -> Option<Self::Id> {
        let mut path: Vec<(SplitDirection, PathSide, &Self)> = Vec::new();
        if !self.build_path_to(current, &mut path) {
            return None;
        }
        for (split_dir, side, sibling) in path.iter().rev() {
            if direction_matches_split(*split_dir, direction) {
                let want_first = direction_wants_first(direction);
                let currently_first = *side == PathSide::First;
                if currently_first != want_first {
                    return Some(sibling.edge_leaf(direction));
                }
            }
        }
        None
    }

    fn build_path_to<'a>(
        &'a self,
        target: Self::Id,
        path: &mut Vec<(SplitDirection, PathSide, &'a Self)>,
    ) -> bool {
        match self.split_parts() {
            None => self.leaf_id() == Some(target),
            Some((dir, _, first, second)) => {
                path.push((dir, PathSide::First, second));
                if first.build_path_to(target, path) {
                    return true;
                }
                path.pop();

                path.push((dir, PathSide::Second, first));
                if second.build_path_to(target, path) {
                    return true;
                }
                path.pop();
                false
            }
        }
    }

    /// 선택한 leaf에 ID가 없으면 패닉한다.
    fn edge_leaf(&self, direction: FocusDirection) -> Self::Id {
        match self.split_parts() {
            None => self
                .leaf_id()
                .expect("BUG: edge_leaf reached a leaf without an id"),
            Some((_, _, first, second)) => match direction {
                FocusDirection::Left | FocusDirection::Up => second.edge_leaf(direction),
                FocusDirection::Right | FocusDirection::Down => first.edge_leaf(direction),
            },
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PathSide {
    First,
    Second,
}

fn direction_matches_split(split: SplitDirection, dir: FocusDirection) -> bool {
    match dir {
        FocusDirection::Left | FocusDirection::Right => split == SplitDirection::Vertical,
        FocusDirection::Up | FocusDirection::Down => split == SplitDirection::Horizontal,
    }
}

fn direction_wants_first(dir: FocusDirection) -> bool {
    matches!(dir, FocusDirection::Left | FocusDirection::Up)
}
