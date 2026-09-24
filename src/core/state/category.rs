//! 카테고리 변경은 workspace 순서를 바꾸지 않는다. 변경 뒤 저장 예약은 호출자가 맡는다.

use super::CoreState;

impl CoreState {
    /// 생성·복원 뒤 normal을 첫 위치에 두고 없는 카테고리 참조를 normal로 돌린다.
    /// 기존 최대 ID보다 발급 기준을 높이지만 ID overflow를 별도로 검사하지는 않는다.
    #[cfg(any(feature = "gui", test))]
    pub fn ensure_normal_category(&mut self) {
        use crate::model::{NORMAL_CATEGORY_ID, WorkspaceCategory};
        match self.categories.iter().position(|c| c.is_normal()) {
            Some(0) => {}
            Some(idx) => {
                let normal = self.categories.remove(idx);
                self.categories.insert(0, normal);
            }
            None => self.categories.insert(0, WorkspaceCategory::normal()),
        }
        let max_id = self
            .categories
            .iter()
            .map(|c| c.id)
            .filter(|&id| id != NORMAL_CATEGORY_ID)
            .max();
        if let Some(max_id) = max_id {
            self.next_ids.bump_category_floor(max_id + 1);
        }
        let valid: std::collections::HashSet<u32> = self.categories.iter().map(|c| c.id).collect();
        for ws in &mut self.workspaces {
            if !valid.contains(&ws.category) {
                ws.set_category(NORMAL_CATEGORY_ID);
            }
        }
    }

    pub fn categories(&self) -> &[crate::model::WorkspaceCategory] {
        &self.categories
    }

    /// 기능을 끌 때 normal로 소속을 합친다. workspace의 전역 순서는 유지한다.
    #[cfg(feature = "gui")]
    pub fn collapse_categories_to_normal(&mut self) {
        use crate::model::{NORMAL_CATEGORY_ID, WorkspaceCategory};
        for ws in &mut self.workspaces {
            ws.set_category(NORMAL_CATEGORY_ID);
        }
        self.categories = vec![WorkspaceCategory::normal()];
    }

    /// 이름을 검증해 카테고리를 추가한다. 호출자가 mark_layout_dirty를 요청해야 저장된다.
    pub fn create_category(
        &mut self,
        raw_name: &str,
    ) -> Result<crate::model::WorkspaceCategoryId, crate::model::CategoryNameError> {
        let existing: Vec<&str> = self.categories.iter().map(|c| c.name.as_str()).collect();
        let name = crate::model::validate_new_category_name(raw_name, existing)?;
        let id = self.next_ids.next_category();
        self.categories
            .push(crate::model::WorkspaceCategory::new(id, name));
        Ok(id)
    }

    pub fn rename_category(
        &mut self,
        id: crate::model::WorkspaceCategoryId,
        raw_name: &str,
    ) -> Result<(), CategoryOpError> {
        use crate::model::NORMAL_CATEGORY_ID;
        if id == NORMAL_CATEGORY_ID {
            return Err(CategoryOpError::IsNormal);
        }
        if self.category_index(id).is_none() {
            return Err(CategoryOpError::NotFound);
        }
        let existing: Vec<&str> = self
            .categories
            .iter()
            .filter(|c| c.id != id)
            .map(|c| c.name.as_str())
            .collect();
        let name = crate::model::validate_rename_category_name(raw_name, existing)
            .map_err(CategoryOpError::Name)?;
        if let Some(cat) = self.categories.iter_mut().find(|c| c.id == id) {
            cat.name = name;
        }
        Ok(())
    }

    pub fn delete_category(
        &mut self,
        id: crate::model::WorkspaceCategoryId,
    ) -> Result<(), CategoryOpError> {
        use crate::model::NORMAL_CATEGORY_ID;
        if id == NORMAL_CATEGORY_ID {
            return Err(CategoryOpError::IsNormal);
        }
        let idx = self.category_index(id).ok_or(CategoryOpError::NotFound)?;
        for ws in &mut self.workspaces {
            if ws.category == id {
                ws.set_category(NORMAL_CATEGORY_ID);
            }
        }
        self.categories.remove(idx);
        Ok(())
    }

    /// normal의 첫 위치를 보존한다. 범위 밖은 오류이며 같은 위치로 이동하면 그대로 성공한다.
    pub fn reorder_category(
        &mut self,
        from_index: usize,
        to_index: usize,
    ) -> Result<(), CategoryOpError> {
        let len = self.categories.len();
        if from_index == 0 || to_index == 0 {
            return Err(CategoryOpError::NormalFixed);
        }
        if from_index >= len || to_index >= len {
            return Err(CategoryOpError::InvalidIndex);
        }
        if from_index == to_index {
            return Ok(());
        }
        let cat = self.categories.remove(from_index);
        self.categories.insert(to_index, cat);
        Ok(())
    }

    pub fn set_workspace_category(
        &mut self,
        ws_id: crate::model::WorkspaceId,
        cat_id: crate::model::WorkspaceCategoryId,
    ) -> Result<(), CategoryOpError> {
        if self.category_index(cat_id).is_none() {
            return Err(CategoryOpError::NotFound);
        }
        let ws = self
            .workspaces
            .iter_mut()
            .find(|w| w.id == ws_id)
            .ok_or(CategoryOpError::WorkspaceNotFound)?;
        ws.set_category(cat_id);
        Ok(())
    }

    /// normal도 접을 수 있다. 변경 뒤 mark_layout_dirty는 호출자 몫이다.
    #[cfg(any(feature = "gui", test))]
    pub fn set_category_collapsed(
        &mut self,
        id: crate::model::WorkspaceCategoryId,
        collapsed: bool,
    ) {
        if let Some(cat) = self.categories.iter_mut().find(|c| c.id == id) {
            cat.collapsed = collapsed;
        }
    }

    #[cfg(any(feature = "gui", test))]
    pub fn toggle_category_collapsed(&mut self, id: crate::model::WorkspaceCategoryId) {
        if let Some(cat) = self.categories.iter_mut().find(|c| c.id == id) {
            cat.collapsed = !cat.collapsed;
        }
    }

    /// 하나라도 펼쳐져 있으면 모두 접고, 모두 접혀 있으면 모두 편다. 저장 예약은 호출자가 맡는다.
    #[cfg(any(feature = "gui", test))]
    pub fn toggle_all_categories_collapsed(&mut self) {
        let target = self.categories.iter().any(|c| !c.collapsed);
        for cat in &mut self.categories {
            cat.collapsed = target;
        }
    }

    /// 존재하는 숫자 ID를 먼저 찾고, 아니면 ASCII 대소문자를 무시해 이름을 찾는다.
    pub fn resolve_category(&self, token: &str) -> Option<crate::model::WorkspaceCategoryId> {
        let t = token.trim();
        if let Ok(id) = t.parse::<u32>()
            && self.categories.iter().any(|c| c.id == id)
        {
            return Some(id);
        }
        self.categories
            .iter()
            .find(|c| c.name.eq_ignore_ascii_case(t))
            .map(|c| c.id)
    }

    pub fn category_name(&self, id: crate::model::WorkspaceCategoryId) -> Option<&str> {
        self.categories
            .iter()
            .find(|c| c.id == id)
            .map(|c| c.name.as_str())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CategoryOpError {
    NotFound,
    IsNormal,
    NormalFixed,
    InvalidIndex,
    WorkspaceNotFound,
    Name(crate::model::CategoryNameError),
}

impl std::fmt::Display for CategoryOpError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CategoryOpError::NotFound => f.write_str("category not found"),
            CategoryOpError::IsNormal => {
                f.write_str("the 'normal' category cannot be renamed or deleted")
            }
            CategoryOpError::NormalFixed => {
                f.write_str("the 'normal' category is fixed at position 0")
            }
            CategoryOpError::InvalidIndex => f.write_str("category index out of range"),
            CategoryOpError::WorkspaceNotFound => f.write_str("workspace not found"),
            CategoryOpError::Name(e) => write!(f, "{e}"),
        }
    }
}

#[cfg(test)]
mod category_tests {
    use super::{CategoryOpError, CoreState};
    use crate::model::{CategoryNameError, NORMAL_CATEGORY_ID};

    fn engine() -> CoreState {
        let waker: tasty_terminal::Waker = std::sync::Arc::new(|| {});
        CoreState::new(80, 24, waker).expect("engine")
    }

    #[test]
    fn starts_with_normal_only() {
        let e = engine();
        assert_eq!(e.categories().len(), 1);
        assert_eq!(e.categories()[0].id, NORMAL_CATEGORY_ID);
        assert!(e.categories()[0].is_normal());
    }

    #[test]
    fn create_rejects_reserved_and_duplicate() {
        let mut e = engine();
        let id = e.create_category("Work").unwrap();
        assert_ne!(id, NORMAL_CATEGORY_ID);
        assert_eq!(e.categories().len(), 2);
        assert_eq!(
            e.create_category("normal"),
            Err(CategoryNameError::Reserved)
        );
        assert_eq!(e.create_category("WORK"), Err(CategoryNameError::Duplicate));
    }

    #[test]
    fn rename_rejects_normal() {
        let mut e = engine();
        assert_eq!(
            e.rename_category(NORMAL_CATEGORY_ID, "x"),
            Err(CategoryOpError::IsNormal)
        );
    }

    #[test]
    fn delete_moves_workspaces_to_normal() {
        let mut e = engine();
        let cat = e.create_category("Work").unwrap();
        let ws_id = e.workspaces[0].id;
        e.set_workspace_category(ws_id, cat).unwrap();
        assert_eq!(e.workspaces[0].category, cat);
        e.delete_category(cat).unwrap();
        assert_eq!(e.workspaces[0].category, NORMAL_CATEGORY_ID);
        assert_eq!(e.categories().len(), 1);
        assert_eq!(
            e.delete_category(NORMAL_CATEGORY_ID),
            Err(CategoryOpError::IsNormal)
        );
    }

    #[test]
    fn reorder_protects_normal_position() {
        let mut e = engine();
        e.create_category("a").unwrap();
        e.create_category("b").unwrap();
        assert_eq!(e.reorder_category(0, 1), Err(CategoryOpError::NormalFixed));
        assert_eq!(e.reorder_category(1, 0), Err(CategoryOpError::NormalFixed));
        assert!(e.reorder_category(1, 2).is_ok());
        assert_eq!(e.categories()[0].id, NORMAL_CATEGORY_ID);
    }

    #[test]
    fn resolve_category_by_name_or_id() {
        let mut e = engine();
        let id = e.create_category("Study").unwrap();
        assert_eq!(e.resolve_category("study"), Some(id));
        assert_eq!(e.resolve_category(&id.to_string()), Some(id));
        assert_eq!(e.resolve_category("nope"), None);
    }

    #[test]
    fn set_collapsed_updates_memory() {
        let mut e = engine();
        let id = e.create_category("Services").unwrap();
        assert!(
            !e.categories()
                .iter()
                .find(|c| c.id == id)
                .unwrap()
                .collapsed
        );
        e.set_category_collapsed(id, true);
        assert!(
            e.categories()
                .iter()
                .find(|c| c.id == id)
                .unwrap()
                .collapsed
        );
        e.set_category_collapsed(id, false);
        assert!(
            !e.categories()
                .iter()
                .find(|c| c.id == id)
                .unwrap()
                .collapsed
        );
        e.set_category_collapsed(9999, true);
    }

    #[test]
    fn toggle_collapsed_flips() {
        let mut e = engine();
        let id = e.create_category("Toggle").unwrap();
        e.toggle_category_collapsed(id);
        assert!(
            e.categories()
                .iter()
                .find(|c| c.id == id)
                .unwrap()
                .collapsed
        );
        e.toggle_category_collapsed(id);
        assert!(
            !e.categories()
                .iter()
                .find(|c| c.id == id)
                .unwrap()
                .collapsed
        );
    }

    #[test]
    fn toggle_all_collapses_when_any_expanded_then_expands_when_all_collapsed() {
        let mut e = engine();
        let a = e.create_category("A").unwrap();
        let b = e.create_category("B").unwrap();
        e.set_category_collapsed(a, true);
        e.toggle_all_categories_collapsed();
        assert!(
            e.categories().iter().all(|c| c.collapsed),
            "하나라도 펼쳐져 있으면 전부 접힌다(normal 포함)"
        );
        e.toggle_all_categories_collapsed();
        assert!(
            e.categories().iter().all(|c| !c.collapsed),
            "전부 접혀 있으면 전부 펴진다"
        );
        assert!(!e.categories().iter().find(|c| c.id == b).unwrap().collapsed);
    }

    #[test]
    fn set_collapsed_allows_normal() {
        let mut e = engine();
        e.set_category_collapsed(NORMAL_CATEGORY_ID, true);
        assert!(
            e.categories()
                .iter()
                .find(|c| c.id == NORMAL_CATEGORY_ID)
                .unwrap()
                .collapsed
        );
    }

    #[test]
    fn collapsed_survives_layout_round_trip() {
        use crate::core::layout_persistence::SavedLayout;
        let mut e = engine();
        let id = e.create_category("Services").unwrap();
        e.set_category_collapsed(id, true);
        let saved = SavedLayout::capture(&mut e, 0);

        let mut restored = engine();
        assert!(saved.restore(&mut restored));
        let cat = restored
            .categories()
            .iter()
            .find(|c| c.name == "Services")
            .expect("Services category should be restored");
        assert!(
            cat.collapsed,
            "collapsed 상태가 왕복 후에도 유지되어야 한다"
        );
    }

    #[test]
    fn mirror_workspace_not_persisted() {
        use crate::core::layout_persistence::SavedLayout;
        let mut e = engine();
        let base_count = e.workspaces.len();
        assert!(base_count >= 1, "엔진은 기본 workspace 를 하나 이상 가진다");
        let idx = match crate::core::apply_create_workspace_inner(
            &mut e,
            crate::core::WorkspaceCreationParams::terminal(),
        )
        .unwrap()
        {
            crate::core::intent::CoreEvent::WorkspaceCreated { index, .. } => index,
            _ => panic!("expected WorkspaceCreated"),
        };
        e.workspaces[idx].mirror = true;
        assert_eq!(e.workspaces.len(), base_count + 1);

        let saved = SavedLayout::capture(&mut e, idx);
        assert_eq!(
            saved.workspaces.len(),
            base_count,
            "mirror workspace 는 저장에서 제외돼야 한다"
        );
        assert!(
            saved.active_workspace < saved.workspaces.len(),
            "remap 된 active 인덱스가 저장 목록 범위 안이어야 한다"
        );

        let mut restored = engine();
        assert!(saved.restore(&mut restored));
        assert!(
            restored.workspaces.iter().all(|w| !w.mirror),
            "복원본에 mirror workspace 가 없어야 한다"
        );
    }

    #[test]
    fn create_workspace_inner_assigns_category() {
        let mut e = engine();
        let cat = e.create_category("Services").unwrap();
        let idx = match crate::core::apply_create_workspace_inner(
            &mut e,
            crate::core::WorkspaceCreationParams {
                category: Some(cat),
                ..crate::core::WorkspaceCreationParams::terminal()
            },
        )
        .unwrap()
        {
            crate::core::intent::CoreEvent::WorkspaceCreated { index, .. } => index,
            _ => panic!("expected WorkspaceCreated"),
        };
        assert_eq!(e.workspaces[idx].category, cat);

        let idx2 = match crate::core::apply_create_workspace_inner(
            &mut e,
            crate::core::WorkspaceCreationParams {
                category: Some(9999),
                ..crate::core::WorkspaceCreationParams::terminal()
            },
        )
        .unwrap()
        {
            crate::core::intent::CoreEvent::WorkspaceCreated { index, .. } => index,
            _ => unreachable!(),
        };
        assert_eq!(e.workspaces[idx2].category, NORMAL_CATEGORY_ID);
    }
}
