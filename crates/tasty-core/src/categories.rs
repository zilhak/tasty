impl crate::CoreState {
    pub fn categories(&self)->&[tasty_model::WorkspaceCategory] {&self.categories}
    pub fn resolve_category(&self, token: &str) -> Option<tasty_model::WorkspaceCategoryId> {
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

    pub fn category_name(&self, id: tasty_model::WorkspaceCategoryId) -> Option<&str> {
        self.categories
            .iter()
            .find(|c| c.id == id)
            .map(|c| c.name.as_str())
    }
}
