use super::{DividerInfo, FocusDirection, PhysicalPx, PhysicalRect, SplitDirection};

/// Common binary-tree surface for `PaneNode` and `SurfaceLayout`.
///
/// Leaf payloads differ per enum (`Pane` vs `Box<dyn Surface>`) and the
/// `Split` variant carries different side fields (`focus_second` only on
/// `SurfaceLayout`). This trait abstracts *only the recursive structure*;
/// leaf-touching mutation and lookup stays in each enum's inherent impl.
pub trait BinaryTree: Sized {
    type Id: Copy + Eq;

    /// 분할선(= 두 자식 사이 간격)의 **물리** 두께.
    ///
    /// 연관 상수가 아니라 메서드인 이유: 구현마다 원본 상수의 좌표계가 다르다.
    /// pane 보더는 논리라 배율을 받아야 물리가 나오고(`PANE_BORDER_WIDTH`),
    /// surface 보더는 hairline 이라 배율을 **무시하는 것이 정답**이다
    /// (`SURFACE_BORDER_WIDTH`). 두 경우를 한 상수로 표현할 수 없다.
    /// 근거: `docs/adr/0148-physical-px-constants-are-split-by-what-they-are-for.md`.
    fn border_width(scale_factor: f32) -> PhysicalPx;

    /// `Split` 일 때 (direction, ratio, &first, &second). `Leaf` 면 None.
    fn split_parts(&self) -> Option<(SplitDirection, f32, &Self, &Self)>;

    /// 변이용 split 접근자. `ratio` 만 `&mut`; `focus_second` 등 부가 필드는 노출하지 않음.
    ///
    /// 주의: `first`/`second` 가 `Box<Self>` 인 enum 에서는 `&mut **first` 형태로
    /// 명시 deref 가 필요할 수 있다 (`Box<Self> -> Self -> &mut Self`). 패턴 매칭에서
    /// `first` 가 `&mut Box<Self>` 로 바인딩되면 `DerefMut` coercion 으로 통과하지만
    /// 명시 deref 가 더 안전.
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

    /// 패닉 의존성: leaf 가 `leaf_id() == None` 인 경우 (e.g. surface_id 가 없는
    /// empty surface) `.expect(...)` 가 패닉. 기존 inherent `edge_leaf` 동작과
    /// 의미적으로 *동일* — 신규 회귀가 아니다.
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
