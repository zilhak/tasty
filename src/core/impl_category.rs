//! workspace 카테고리의 생성·이름 변경·삭제·순서·접힘을 적용한다.
//! 검증과 변경은 CoreState의 카테고리 메서드가 맡고, 성공하면 레이아웃 저장을 예약한다.

use super::*;

impl Core {
    /// 새 카테고리는 목록 끝에 추가한다. IPC는 끝 항목으로 발급된 ID를 읽는다.
    pub(super) fn apply_create_category(
        engine: &mut crate::core::CoreState,
        name: &str,
    ) -> anyhow::Result<Vec<CoreEvent>> {
        engine
            .create_category(name)
            .map_err(|e| anyhow::anyhow!(e))?;
        engine.mark_layout_dirty();
        Ok(Vec::new())
    }

    pub(super) fn apply_rename_category(
        engine: &mut crate::core::CoreState,
        id: crate::model::WorkspaceCategoryId,
        name: &str,
    ) -> anyhow::Result<Vec<CoreEvent>> {
        engine
            .rename_category(id, name)
            .map_err(|e| anyhow::anyhow!(e))?;
        engine.mark_layout_dirty();
        Ok(Vec::new())
    }

    /// 안의 workspace는 normal로 옮긴다. 사용자 선택은 바꾸지 않는다.
    pub(super) fn apply_delete_category(
        engine: &mut crate::core::CoreState,
        id: crate::model::WorkspaceCategoryId,
    ) -> anyhow::Result<Vec<CoreEvent>> {
        engine.delete_category(id).map_err(|e| anyhow::anyhow!(e))?;
        engine.mark_layout_dirty();
        Ok(Vec::new())
    }

    pub(super) fn apply_reorder_category(
        engine: &mut crate::core::CoreState,
        from_index: usize,
        to_index: usize,
    ) -> anyhow::Result<Vec<CoreEvent>> {
        engine
            .reorder_category(from_index, to_index)
            .map_err(|e| anyhow::anyhow!(e))?;
        engine.mark_layout_dirty();
        Ok(Vec::new())
    }

    /// 없는 카테고리면 바꾸지 않는다. 접힘은 레이아웃에 저장하므로 저장을 예약한다.
    pub(super) fn apply_set_category_collapsed(
        engine: &mut crate::core::CoreState,
        id: crate::model::WorkspaceCategoryId,
        collapsed: bool,
    ) -> Vec<CoreEvent> {
        engine.set_category_collapsed(id, collapsed);
        engine.mark_layout_dirty();
        Vec::new()
    }

    /// 적용 시점의 상태를 뒤집는다. 없는 카테고리면 바꾸지 않는다.
    pub(super) fn apply_toggle_category_collapsed(
        engine: &mut crate::core::CoreState,
        id: crate::model::WorkspaceCategoryId,
    ) -> Vec<CoreEvent> {
        engine.toggle_category_collapsed(id);
        engine.mark_layout_dirty();
        Vec::new()
    }

    /// 하나라도 펼쳐져 있으면 모두 접고, 모두 접혀 있으면 모두 편다.
    pub(super) fn apply_toggle_all_categories_collapsed(
        engine: &mut crate::core::CoreState,
    ) -> Vec<CoreEvent> {
        engine.toggle_all_categories_collapsed();
        engine.mark_layout_dirty();
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::NORMAL_CATEGORY_ID;

    fn fixture() -> (Core, CoreState) {
        let waker: tasty_terminal::Waker = std::sync::Arc::new(|| {});
        let mut engine = CoreState::new(80, 24, waker).expect("engine");
        engine.layout_dirty.clear();
        (crate::ipc::handler::cli_entry_tests::test_core(), engine)
    }

    fn apply(core: &mut Core, engine: &mut CoreState, intent: DomainIntent) -> Vec<CoreEvent> {
        core.apply(engine, intent).expect("apply")
    }

    fn create(core: &mut Core, engine: &mut CoreState, name: &str) -> u32 {
        let events = apply(
            core,
            engine,
            DomainIntent::CreateCategory {
                name: name.to_string(),
            },
        );
        assert!(events.is_empty());
        engine.categories().last().expect("created").id
    }

    #[test]
    fn create_appends_and_marks_the_layout_dirty() {
        let (mut core, mut engine) = fixture();
        let id = create(&mut core, &mut engine, "Work");
        assert_eq!(engine.categories().len(), 2);
        assert_eq!(engine.category_name(id), Some("Work"));
        assert!(engine.layout_dirty.is_dirty());
    }

    #[test]
    fn a_rejected_create_returns_the_category_error_text() {
        let (mut core, mut engine) = fixture();
        create(&mut core, &mut engine, "Work");
        engine.layout_dirty.clear();
        let err = core
            .apply(
                &mut engine,
                DomainIntent::CreateCategory {
                    name: "work".to_string(),
                },
            )
            .expect_err("duplicate name");
        let direct = engine.create_category("work").expect_err("duplicate");
        assert_eq!(err.to_string(), direct.to_string());
        assert_eq!(engine.categories().len(), 2);
        assert!(!engine.layout_dirty.is_dirty());
    }

    #[test]
    fn rename_delete_and_reorder_go_through_the_category_rules() {
        let (mut core, mut engine) = fixture();
        let a = create(&mut core, &mut engine, "A");
        let b = create(&mut core, &mut engine, "B");
        apply(
            &mut core,
            &mut engine,
            DomainIntent::RenameCategory {
                id: a,
                name: "A2".to_string(),
            },
        );
        assert_eq!(engine.category_name(a), Some("A2"));
        apply(
            &mut core,
            &mut engine,
            DomainIntent::ReorderCategory {
                from_index: 2,
                to_index: 1,
            },
        );
        let order: Vec<u32> = engine.categories().iter().map(|c| c.id).collect();
        assert_eq!(order, [NORMAL_CATEGORY_ID, b, a]);
        engine.workspaces[0].set_category(b);
        apply(
            &mut core,
            &mut engine,
            DomainIntent::DeleteCategory { id: b },
        );
        assert_eq!(engine.category_index(b), None);
        assert_eq!(engine.workspaces[0].category, NORMAL_CATEGORY_ID);

        let err = core
            .apply(
                &mut engine,
                DomainIntent::DeleteCategory {
                    id: NORMAL_CATEGORY_ID,
                },
            )
            .expect_err("normal is fixed");
        let direct = engine
            .delete_category(NORMAL_CATEGORY_ID)
            .expect_err("normal is fixed");
        assert_eq!(err.to_string(), direct.to_string());
    }

    #[test]
    fn collapse_intents_set_toggle_and_toggle_all() {
        let (mut core, mut engine) = fixture();
        let a = create(&mut core, &mut engine, "A");
        apply(
            &mut core,
            &mut engine,
            DomainIntent::SetCategoryCollapsed {
                id: a,
                collapsed: true,
            },
        );
        assert!(engine.categories()[1].collapsed);
        apply(
            &mut core,
            &mut engine,
            DomainIntent::ToggleCategoryCollapsed { id: a },
        );
        assert!(!engine.categories()[1].collapsed);
        apply(
            &mut core,
            &mut engine,
            DomainIntent::ToggleAllCategoriesCollapsed,
        );
        assert!(engine.categories().iter().all(|c| c.collapsed));
        assert!(engine.layout_dirty.is_dirty());
    }

    #[test]
    fn category_intents_still_apply_locally_on_a_mirror_workspace() {
        let (mut core, mut engine) = fixture();
        engine.workspaces[0].mirror = true;
        let a = create(&mut core, &mut engine, "A");
        apply(
            &mut core,
            &mut engine,
            DomainIntent::ToggleCategoryCollapsed { id: a },
        );
        assert!(engine.categories()[1].collapsed);
        assert!(engine.pending_structural_forward.is_empty());
    }
}
