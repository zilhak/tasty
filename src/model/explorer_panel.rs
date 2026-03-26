/// A node in the file tree.
pub struct FileNode {
    pub name: String,
    pub path: String,
    pub is_directory: bool,
    pub children: Option<Vec<FileNode>>,
    pub is_expanded: bool,
}

/// A panel that shows a file explorer with a tree view and file preview.
pub struct ExplorerPanel {
    pub id: u32,
    pub root_path: String,
    pub root_node: FileNode,
    pub selected_file: Option<String>,
    pub file_content: Option<String>,
    pub is_markdown: bool,
    pub scroll_offset: f32,
    pub tree_scroll_offset: f32,
}

impl ExplorerPanel {
    pub fn new(id: u32, root_path: String) -> Self {
        let mut root_node = FileNode {
            name: root_path
                .split(['/', '\\'])
                .last()
                .unwrap_or("root")
                .to_string(),
            path: root_path.clone(),
            is_directory: true,
            children: None,
            is_expanded: true,
        };
        Self::load_directory(&mut root_node);
        Self {
            id,
            root_path,
            root_node,
            selected_file: None,
            file_content: None,
            is_markdown: false,
            scroll_offset: 0.0,
            tree_scroll_offset: 0.0,
        }
    }

    pub fn load_directory(node: &mut FileNode) {
        if !node.is_directory {
            return;
        }
        let mut entries = Vec::new();
        if let Ok(read_dir) = std::fs::read_dir(&node.path) {
            for entry in read_dir.flatten() {
                let name = entry.file_name().to_string_lossy().to_string();
                // Skip hidden files except a few common ones
                if name.starts_with('.')
                    && !name.starts_with(".env")
                    && !name.starts_with(".gitignore")
                    && !name.starts_with(".claude")
                {
                    continue;
                }
                let path = entry.path().to_string_lossy().to_string();
                let is_dir = entry.file_type().map(|t| t.is_dir()).unwrap_or(false);
                entries.push(FileNode {
                    name,
                    path,
                    is_directory: is_dir,
                    children: None,
                    is_expanded: false,
                });
            }
        }
        // Sort: directories first, then case-insensitive name
        entries.sort_by(|a, b| {
            b.is_directory
                .cmp(&a.is_directory)
                .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
        });
        node.children = Some(entries);
    }

    pub fn select_file(&mut self, path: &str) {
        let ext = path.rsplit('.').next().unwrap_or("").to_lowercase();
        self.is_markdown = ext == "md" || ext == "markdown";
        self.file_content = std::fs::read_to_string(path).ok();
        self.selected_file = Some(path.to_string());
        self.scroll_offset = 0.0;
    }
}
