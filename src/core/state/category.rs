//! 워크스페이스 카테고리 — 사이드바 rail 이 워크스페이스를 묶는 단위.
//!
//! 목록 하나에 불변식 셋이 걸려 있어서 따로 산다: `categories[0]` 은 언제나 예약된
//! normal 이고, id 는 재사용되지 않으며, 어떤 워크스페이스도 없는 카테고리를 가리키지
//! 않는다. 그 셋을 함께 지키는 정규화가 생성·복원 직후에 한 번 돌고, 나머지 연산은
//! 전부 그 정규화가 이미 성립시킨 상태를 전제한다.
//!
//! [`CoreState`] 의 inherent impl 을 이어 쓴다 — 이 연산들은 `workspaces` 와 id 발급기를
//! 같이 만지므로 자유 함수로 내리면 둘을 인자로 다시 엮어야 한다.

use super::CoreState;

impl CoreState {
    /// normal 불변식 보장 — `categories[0]` 이 항상 예약된 normal 이 되도록 정규화.
    ///
    /// 생성/복원 직후 호출한다. (1) normal 이 없으면 맨 앞에 삽입, (2) 있으나 0번이
    /// 아니면 0번으로 이동, (3) 또한 발급기 floor 를 기존 최대 카테고리 id + 1 이상으로
    /// 올려 재사용을 차단한다. 어떤 워크스페이스가 존재하지 않는 카테고리를 가리키면
    /// normal 로 귀속한다.
    pub fn ensure_normal_category(&mut self) {
        use crate::model::{NORMAL_CATEGORY_ID, WorkspaceCategory};
        // (1)(2) normal 을 0번에 고정.
        match self.categories.iter().position(|c| c.is_normal()) {
            Some(0) => {}
            Some(idx) => {
                let normal = self.categories.remove(idx);
                self.categories.insert(0, normal);
            }
            None => self.categories.insert(0, WorkspaceCategory::normal()),
        }
        // (3) 발급기 floor 를 최대 사용자 카테고리 id + 1 위로.
        let max_id = self
            .categories
            .iter()
            .map(|c| c.id)
            .filter(|&id| id != NORMAL_CATEGORY_ID)
            .max();
        if let Some(max_id) = max_id {
            self.next_ids.bump_category_floor(max_id + 1);
        }
        // 존재하지 않는 카테고리를 가리키는 워크스페이스는 normal 로 귀속.
        let valid: std::collections::HashSet<u32> = self.categories.iter().map(|c| c.id).collect();
        for ws in &mut self.workspaces {
            if !valid.contains(&ws.category) {
                ws.set_category(NORMAL_CATEGORY_ID);
            }
        }
    }

    /// 카테고리 목록(읽기 전용).
    pub fn categories(&self) -> &[crate::model::WorkspaceCategory] {
        &self.categories
    }

    /// 카테고리 토글 **off** 마이그레이션 (§4-2 on→off). normal 외 모든 카테고리를
    /// 제거하고 그 안의 워크스페이스를 모두 normal 로 귀속한다. **워크스페이스의 물리
    /// 순서(전역 인덱스)는 그대로 두므로** active(전역 인덱스) 도 불변이다 — 사이드바가
    /// off 면 평면이라 순서만 보존되면 충분하다.
    pub fn collapse_categories_to_normal(&mut self) {
        use crate::model::{NORMAL_CATEGORY_ID, WorkspaceCategory};
        for ws in &mut self.workspaces {
            ws.set_category(NORMAL_CATEGORY_ID);
        }
        self.categories = vec![WorkspaceCategory::normal()];
    }

    /// 새 카테고리 생성. 이름 검증(§3, 대소문자 무시 중복·예약어 거부) 후 새 id 를
    /// 발급해 Vec 끝(normal 뒤)에 push 한다. 성공 시 새 카테고리 id 반환.
    ///
    /// 카테고리 CRUD 는 **사용자 active/포커스에 닿지 않는** 순수 도메인 데이터 변경이라
    /// (원칙 1·3) cascade/active 보정이 필요 없다 — `set_attach_mapping` 직접 set +
    /// mark_layout_dirty 선례를 따른다. 호출자가 mark_layout_dirty 를 책임진다.
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

    /// 카테고리 이름 변경. normal 은 거부(`IsNormal`), 이름 검증은 대상 자신을 제외한
    /// 나머지와 비교한다.
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

    /// 카테고리 삭제. normal 은 거부. 삭제 대상 안의 워크스페이스는 **순서를 보존하며**
    /// normal 로 귀속한다(전역 인덱스 불변 → 사용자 active 영향 없음, 원칙 1·3).
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

    /// 카테고리 순서 이동(reorder). **from==0 또는 to==0 거부**(normal 0번 고정).
    /// 범위 밖이거나 from==to 면 no-op(false).
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

    /// 워크스페이스의 카테고리 소속 변경. 대상 카테고리가 존재해야 한다. **사용자
    /// active(전역 인덱스) 불변** — 소속만 바꾼다(원칙 1·3).
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

    /// 카테고리 접힘(collapsed) 상태 설정. id 로 카테고리를 찾아 `collapsed` 를
    /// 지정 값으로 둔다. normal 포함 모든 카테고리 접기 허용(디자인상 normal 도 접힘 가능).
    /// 접힘은 사용자 UI 상태지만 layout.json 영속 대상이므로 호출자가 다른 mutator
    /// 관례대로 `mark_layout_dirty` 를 책임진다. 대상이 없으면 no-op.
    pub fn set_category_collapsed(
        &mut self,
        id: crate::model::WorkspaceCategoryId,
        collapsed: bool,
    ) {
        if let Some(cat) = self.categories.iter_mut().find(|c| c.id == id) {
            cat.collapsed = collapsed;
        }
    }

    /// 카테고리 접힘 상태를 뒤집는다(호출부 단순화용 편의 메서드). 대상이 없으면 no-op.
    pub fn toggle_category_collapsed(&mut self, id: crate::model::WorkspaceCategoryId) {
        if let Some(cat) = self.categories.iter_mut().find(|c| c.id == id) {
            cat.collapsed = !cat.collapsed;
        }
    }

    /// 모든 카테고리(normal 포함)의 접힘 상태를 일괄 토글한다. **하나라도 펼쳐져 있으면
    /// 전부 접고, 전부 접혀 있으면 전부 편다** — "전체 접기/펴기" 단축키용. `set_category_collapsed`
    /// 와 동일하게 접힘은 layout.json 영속 대상이므로 호출자가 `mark_layout_dirty` 를 책임진다.
    /// 카테고리가 normal 하나뿐이어도 그 하나를 토글한다.
    pub fn toggle_all_categories_collapsed(&mut self) {
        let target = self.categories.iter().any(|c| !c.collapsed);
        for cat in &mut self.categories {
            cat.collapsed = target;
        }
    }

    /// 이름(대소문자 무시) 또는 정확 id 로 카테고리를 해석한다. CLI/IPC 가
    /// `--category <name|id>` 를 받을 때 사용. 숫자 문자열은 id 로 우선 해석한다.
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

    /// 카테고리 id → 이름. 없으면 None.
    pub fn category_name(&self, id: crate::model::WorkspaceCategoryId) -> Option<&str> {
        self.categories
            .iter()
            .find(|c| c.id == id)
            .map(|c| c.name.as_str())
    }
}

/// 카테고리 변경 연산 에러. IPC 핸들러가 사용자 메시지로 매핑한다.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CategoryOpError {
    /// 대상 카테고리를 찾을 수 없음.
    NotFound,
    /// normal 은 rename/delete 불가.
    IsNormal,
    /// normal(0번) 위치 고정 위반(reorder from/to == 0).
    NormalFixed,
    /// reorder 인덱스 범위 밖.
    InvalidIndex,
    /// 워크스페이스를 찾을 수 없음(set_workspace_category).
    WorkspaceNotFound,
    /// 이름 검증 실패.
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
        // 기본 워크스페이스를 새 카테고리로 옮긴다.
        let ws_id = e.workspaces[0].id;
        e.set_workspace_category(ws_id, cat).unwrap();
        assert_eq!(e.workspaces[0].category, cat);
        // 삭제 → 워크스페이스는 normal 로 귀속, 카테고리 제거.
        e.delete_category(cat).unwrap();
        assert_eq!(e.workspaces[0].category, NORMAL_CATEGORY_ID);
        assert_eq!(e.categories().len(), 1);
        // normal 삭제는 거부.
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
        // from/to == 0 거부.
        assert_eq!(e.reorder_category(0, 1), Err(CategoryOpError::NormalFixed));
        assert_eq!(e.reorder_category(1, 0), Err(CategoryOpError::NormalFixed));
        // 1↔2 스왑은 허용.
        assert!(e.reorder_category(1, 2).is_ok());
        // normal 은 여전히 0번.
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
        // 기본은 펼침.
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
        // 없는 id 는 no-op(패닉 없이 무시).
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
        // 하나만 접힌 초기 상태 → "하나라도 펼쳐짐" 이므로 전부 접혀야 한다.
        e.set_category_collapsed(a, true);
        e.toggle_all_categories_collapsed();
        assert!(
            e.categories().iter().all(|c| c.collapsed),
            "하나라도 펼쳐져 있으면 전부 접힌다(normal 포함)"
        );
        // 전부 접힌 상태 → 전부 펴져야 한다.
        e.toggle_all_categories_collapsed();
        assert!(
            e.categories().iter().all(|c| !c.collapsed),
            "전부 접혀 있으면 전부 펴진다"
        );
        // b 도 개별 확인(루프 전체 반영 여부).
        assert!(!e.categories().iter().find(|c| c.id == b).unwrap().collapsed);
    }

    #[test]
    fn set_collapsed_allows_normal() {
        // 디자인상 normal 도 접힘 가능.
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
        // 접힘 상태를 설정한 뒤 layout capture→restore 왕복 후에도 유지되는지 회귀.
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
        // 회귀: 원격 attach 가 만드는 mirror workspace 는 layout.json 에 저장되면
        // 안 된다(재시작 시 원격 없는 죽은 일반 ws 로 복원되는 버그). capture 가 제외하고
        // active 인덱스도 필터 후 위치로 remap 하는지 확인.
        let mut e = engine();
        let base_count = e.workspaces.len();
        assert!(base_count >= 1, "엔진은 기본 workspace 를 하나 이상 가진다");
        // 원격 attach 가 만드는 mirror workspace 를 흉내 — 새 ws 를 만들고 mirror 플래그를 세운다.
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

        // active 를 mirror 로 둔 채 capture — mirror 는 저장 제외 + active 는 클램프.
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

        // 왕복 복원 후에도 mirror 는 되살아나지 않는다.
        let mut restored = engine();
        assert!(saved.restore(&mut restored));
        assert!(
            restored.workspaces.iter().all(|w| !w.mirror),
            "복원본에 mirror workspace 가 없어야 한다"
        );
    }

    #[test]
    fn create_workspace_inner_assigns_category() {
        // 생성 시점 카테고리 소속(옵션 A). 유효 카테고리는 그 소속, dangling 은 normal.
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

        // 존재하지 않는 카테고리 → normal 유지.
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
