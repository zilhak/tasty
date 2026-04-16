use std::collections::HashSet;

use super::surface_trait::Surface;
use super::SurfaceId;

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
    /// The last clicked/navigated item — used for right-side preview.
    pub selected_file: Option<String>,
    /// All currently selected items (for multi-selection & clipboard).
    pub selected_files: HashSet<String>,
    /// Anchor point for Shift+click range selection.
    pub selection_anchor: Option<String>,
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
            selected_files: HashSet::new(),
            selection_anchor: None,
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

    /// Single-click select: clear all selections, select one item, set anchor.
    pub fn select_single(&mut self, path: &str) {
        self.selected_files.clear();
        self.selected_files.insert(path.to_string());
        self.selection_anchor = Some(path.to_string());
        self.set_preview(path);
    }

    /// Ctrl/Cmd+click: toggle one item in the selection set.
    pub fn toggle_select(&mut self, path: &str) {
        if self.selected_files.contains(path) {
            self.selected_files.remove(path);
        } else {
            self.selected_files.insert(path.to_string());
        }
        self.selection_anchor = Some(path.to_string());
        self.set_preview(path);
    }

    /// Shift+click: range select from anchor to target (inclusive).
    /// `visible_paths` must be the current ordered list of visible tree nodes.
    pub fn range_select(&mut self, target: &str, visible_paths: &[String]) {
        let anchor = self.selection_anchor.clone().unwrap_or_else(|| target.to_string());
        let anchor_idx = visible_paths.iter().position(|p| p == &anchor);
        let target_idx = visible_paths.iter().position(|p| p == target);
        if let (Some(a), Some(t)) = (anchor_idx, target_idx) {
            let (start, end) = if a <= t { (a, t) } else { (t, a) };
            self.selected_files.clear();
            for p in &visible_paths[start..=end] {
                self.selected_files.insert(p.clone());
            }
        }
        self.set_preview(target);
    }

    /// Select all visible items.
    pub fn select_all(&mut self, visible_paths: &[String]) {
        self.selected_files.clear();
        for p in visible_paths {
            self.selected_files.insert(p.clone());
        }
    }

    /// Update the right-side preview for the given path.
    fn set_preview(&mut self, path: &str) {
        let ext = path.rsplit('.').next().unwrap_or("").to_lowercase();
        self.is_markdown = ext == "md" || ext == "markdown";
        self.selected_file = Some(path.to_string());
        self.scroll_offset = 0.0;

        // Only load preview for files (not directories)
        let is_dir = std::path::Path::new(path).is_dir();
        if !is_dir && is_previewable_file(path, &ext) {
            self.file_content = std::fs::read_to_string(path).ok();
        } else {
            self.file_content = None;
        }
    }

}

/// Check if a file is likely a text file suitable for preview.
fn is_previewable_file(path: &str, ext: &str) -> bool {
    const TEXT_EXTENSIONS: &[&str] = &[
        // Markup / Doc
        "md", "markdown", "txt", "text", "rst", "adoc", "org",
        // Web
        "html", "htm", "css", "js", "jsx", "ts", "tsx", "vue", "svelte", "json", "xml", "svg",
        // Config
        "toml", "yaml", "yml", "ini", "cfg", "conf", "env", "properties",
        // Programming
        "rs", "py", "go", "java", "kt", "kts", "c", "cpp", "cc", "h", "hpp", "hh",
        "cs", "swift", "rb", "pl", "pm", "lua", "r", "jl", "ex", "exs", "erl", "hrl",
        "hs", "ml", "mli", "fs", "fsi", "fsx", "clj", "cljs", "scala", "sc",
        "zig", "nim", "v", "d", "dart", "php",
        // Shell
        "sh", "bash", "zsh", "fish", "ps1", "psm1", "bat", "cmd",
        // Data
        "csv", "tsv", "sql", "graphql", "gql",
        // Build / CI
        "cmake", "gradle", "sbt", "cabal",
        // Other
        "log", "diff", "patch", "gitignore", "gitattributes", "editorconfig",
        "dockerignore", "prettierrc", "eslintrc", "babelrc",
    ];

    if TEXT_EXTENSIONS.contains(&ext) {
        return true;
    }

    // Check extensionless known filenames
    let filename = path.rsplit(['/', '\\']).next().unwrap_or("");
    const TEXT_FILENAMES: &[&str] = &[
        "Makefile", "makefile", "GNUmakefile", "Dockerfile", "Containerfile",
        "Rakefile", "Gemfile", "Procfile", "Justfile", "Vagrantfile",
        "CMakeLists.txt", "LICENSE", "LICENCE", "COPYING", "AUTHORS",
        "CHANGELOG", "README", "INSTALL", "TODO", "CONTRIBUTORS",
        ".gitignore", ".gitattributes", ".editorconfig", ".dockerignore",
        ".env", ".env.local", ".env.example",
    ];

    if TEXT_FILENAMES.contains(&filename) {
        return true;
    }

    // No extension and not a known filename — skip preview
    false
}

impl Surface for ExplorerPanel {
    fn type_name(&self) -> &'static str { "Explorer" }
    fn surface_id(&self) -> Option<SurfaceId> { Some(self.id) }
    fn display_name(&self) -> String {
        std::path::Path::new(&self.root_path)
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "Explorer".to_string())
    }
    fn as_explorer(&self) -> Option<&ExplorerPanel> { Some(self) }
    fn as_explorer_mut(&mut self) -> Option<&mut ExplorerPanel> { Some(self) }
}
