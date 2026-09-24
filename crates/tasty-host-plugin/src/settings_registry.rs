//! 플러그인이 선언한 설정 페이지를 보관한다. hello 뒤 등록하고 비활성화·재시작 때 지운다.
//! 알려지지 않은 카테고리도 경고 후 보관하되, 해당 카테고리의 표시 여부는 호스트 UI가 정한다.

use tasty_plugin_manifest::{SettingsCategory, SettingsPageContribute};

/// 한 plugin 이 등록한 한 settings page.
#[derive(Debug, Clone)]
pub struct SettingsPageEntry {
    pub plugin_id: String,
    pub page: SettingsPageContribute,
}

/// Plugin 이 contribute 한 settings page 들의 레지스트리.
#[derive(Debug, Default)]
pub struct SettingsPageRegistry {
    pages: Vec<SettingsPageEntry>,
}

impl SettingsPageRegistry {
    pub fn new() -> Self {
        Self { pages: Vec::new() }
    }

    /// Plugin 의 settings page 들을 등록한다. `Other(_)` 카테고리는 경고만 출력하고
    /// 보관한다.
    pub fn register(&mut self, plugin_id: String, pages: Vec<SettingsPageContribute>) {
        for page in pages {
            if let SettingsCategory::Other(unknown) = &page.category {
                tracing::warn!(
                    "plugin '{}' settings_page '{}' uses unknown category '{}' — retained for forward compatibility",
                    plugin_id,
                    page.id,
                    unknown
                );
            }
            self.pages.push(SettingsPageEntry {
                plugin_id: plugin_id.clone(),
                page,
            });
        }
    }

    /// 특정 plugin 의 모든 page 를 제거한다. plugin disable / 재시작 시 호출.
    pub fn unregister_plugin(&mut self, plugin_id: &str) {
        self.pages.retain(|e| e.plugin_id != plugin_id);
    }

    /// 등록된 순서대로 모든 entry 를 순회.
    pub fn iter(&self) -> impl Iterator<Item = &SettingsPageEntry> {
        self.pages.iter()
    }

    /// 카테고리에 속한 페이지를 가져온다.
    pub fn by_category<'a>(
        &'a self,
        category: &'a SettingsCategory,
    ) -> impl Iterator<Item = &'a SettingsPageEntry> {
        self.pages
            .iter()
            .filter(move |e| &e.page.category == category)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tasty_plugin_manifest::SettingsItemDecl;

    fn make_page(id: &str, category: SettingsCategory) -> SettingsPageContribute {
        SettingsPageContribute {
            id: id.into(),
            title_key: format!("{id}.title"),
            category,
            items: vec![SettingsItemDecl::FontOverride {
                id: "font".into(),
                label_key: format!("{id}.font"),
                storage_key: format!("plugin.{id}.font"),
            }],
        }
    }

    #[test]
    fn register_and_iter() {
        let mut reg = SettingsPageRegistry::new();
        reg.register(
            "com.example.alpha".into(),
            vec![
                make_page("appearance_sub", SettingsCategory::Appearance),
                make_page("general_sub", SettingsCategory::General),
            ],
        );
        let entries: Vec<_> = reg.iter().collect();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].page.id, "appearance_sub");
        assert_eq!(entries[1].page.id, "general_sub");
        assert!(entries.iter().all(|e| e.plugin_id == "com.example.alpha"));
    }

    #[test]
    fn register_multiple_plugins_separated() {
        let mut reg = SettingsPageRegistry::new();
        reg.register(
            "com.example.alpha".into(),
            vec![make_page("a", SettingsCategory::Appearance)],
        );
        reg.register(
            "com.example.beta".into(),
            vec![make_page("b", SettingsCategory::General)],
        );
        let by_plugin: Vec<_> = reg.iter().map(|e| e.plugin_id.as_str()).collect();
        assert_eq!(by_plugin, vec!["com.example.alpha", "com.example.beta"]);
    }

    #[test]
    fn unregister_plugin_removes_only_that_plugin_pages() {
        let mut reg = SettingsPageRegistry::new();
        reg.register(
            "com.example.alpha".into(),
            vec![
                make_page("a1", SettingsCategory::Appearance),
                make_page("a2", SettingsCategory::General),
            ],
        );
        reg.register(
            "com.example.beta".into(),
            vec![make_page("b1", SettingsCategory::Appearance)],
        );
        reg.unregister_plugin("com.example.alpha");
        let remaining: Vec<_> = reg.iter().collect();
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].plugin_id, "com.example.beta");
        assert_eq!(remaining[0].page.id, "b1");
    }

    #[test]
    fn by_category_filters_correctly() {
        let mut reg = SettingsPageRegistry::new();
        reg.register(
            "com.example.alpha".into(),
            vec![
                make_page("a_app", SettingsCategory::Appearance),
                make_page("a_gen", SettingsCategory::General),
                make_page("a_plg", SettingsCategory::Plugin),
                make_page("a_other", SettingsCategory::Other("unknown".into())),
            ],
        );
        let appearance: Vec<_> = reg
            .by_category(&SettingsCategory::Appearance)
            .map(|e| e.page.id.as_str())
            .collect();
        assert_eq!(appearance, vec!["a_app"]);

        let general: Vec<_> = reg
            .by_category(&SettingsCategory::General)
            .map(|e| e.page.id.as_str())
            .collect();
        assert_eq!(general, vec!["a_gen"]);

        let plugin: Vec<_> = reg
            .by_category(&SettingsCategory::Plugin)
            .map(|e| e.page.id.as_str())
            .collect();
        assert_eq!(plugin, vec!["a_plg"]);

        let other_cat = SettingsCategory::Other("unknown".into());
        let other: Vec<_> = reg
            .by_category(&other_cat)
            .map(|e| e.page.id.as_str())
            .collect();
        assert_eq!(other, vec!["a_other"]);

        let keybindings: Vec<_> = reg.by_category(&SettingsCategory::Keybindings).collect();
        assert!(keybindings.is_empty());
    }

    #[test]
    fn register_other_category_retains_page() {
        let mut reg = SettingsPageRegistry::new();
        reg.register(
            "com.example.future".into(),
            vec![make_page(
                "future_sub",
                SettingsCategory::Other("future-category".into()),
            )],
        );
        let entries: Vec<_> = reg.iter().collect();
        assert_eq!(entries.len(), 1);
        match &entries[0].page.category {
            SettingsCategory::Other(s) => assert_eq!(s, "future-category"),
            other => panic!("expected Other, got {:?}", other),
        }
    }
}
