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
    /// Address bar editing state. When Some, the text field is being edited.
    pub address_bar_text: String,
    /// Whether the address bar is actively being edited (has focus).
    pub address_bar_editing: bool,
    /// Whether the file preview panel is visible.
    pub show_preview: bool,
    /// Tree panel width ratio (0.0..1.0) when preview is shown. Default 0.35.
    pub tree_ratio: f32,
    /// 직전 프레임에 이 surface가 focused였는지 추적. 포커스 획득 시 트리 갱신용.
    pub was_focused: bool,
    /// 마지막 자동 갱신 시각. focused 상태에서 2초마다 증분 갱신.
    pub last_refresh: std::time::Instant,
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
            root_path: root_path.clone(),
            root_node,
            selected_file: None,
            selected_files: HashSet::new(),
            selection_anchor: None,
            file_content: None,
            is_markdown: false,
            scroll_offset: 0.0,
            tree_scroll_offset: 0.0,
            address_bar_text: root_path,
            address_bar_editing: false,
            show_preview: true,
            tree_ratio: 0.35,
            was_focused: false,
            last_refresh: std::time::Instant::now(),
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

    /// 포커스 획득 시 호출: 현재 펼쳐진 디렉토리만 디스크에서 다시 읽어 diff 갱신.
    /// expanded 상태와 selection을 보존한다.
    pub fn refresh_expanded_dirs(&mut self) {
        Self::refresh_node(&mut self.root_node);
    }

    fn refresh_node(node: &mut FileNode) {
        if !node.is_directory || !node.is_expanded {
            return;
        }

        // 디스크에서 새로 읽기
        let mut new_entries = Vec::new();
        if let Ok(read_dir) = std::fs::read_dir(&node.path) {
            for entry in read_dir.flatten() {
                let name = entry.file_name().to_string_lossy().to_string();
                if name.starts_with('.')
                    && !name.starts_with(".env")
                    && !name.starts_with(".gitignore")
                    && !name.starts_with(".claude")
                {
                    continue;
                }
                let path = entry.path().to_string_lossy().to_string();
                let is_dir = entry.file_type().map(|t| t.is_dir()).unwrap_or(false);
                new_entries.push(FileNode {
                    name,
                    path,
                    is_directory: is_dir,
                    children: None,
                    is_expanded: false,
                });
            }
        }
        new_entries.sort_by(|a, b| {
            b.is_directory
                .cmp(&a.is_directory)
                .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
        });

        // 기존 children과 비교하여 변경 시에만 갱신
        let old_children = match node.children.take() {
            Some(c) => c,
            None => {
                node.children = Some(new_entries);
                return;
            }
        };

        let old_paths: HashSet<String> = old_children.iter().map(|n| n.path.clone()).collect();
        let new_paths: HashSet<String> = new_entries.iter().map(|n| n.path.clone()).collect();

        if old_paths == new_paths {
            // 파일 목록 동일 — 기존 트리 복원 후 하위만 재귀 탐색
            node.children = Some(old_children);
        } else {
            // 변경됨 — expanded 상태를 보존하며 merge
            let expanded_map: HashSet<String> = old_children.iter()
                .filter(|n| n.is_expanded)
                .map(|n| n.path.clone())
                .collect();

            for child in &mut new_entries {
                if child.is_directory && expanded_map.contains(&child.path) {
                    child.is_expanded = true;
                    Self::load_directory(child);
                }
            }
            node.children = Some(new_entries);
        }

        // 하위 expanded 디렉토리도 재귀 확인
        if let Some(ref mut children) = node.children {
            for child in children {
                if child.is_expanded {
                    Self::refresh_node(child);
                }
            }
        }
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

    /// Navigate to a new root path. Reloads the directory tree.
    pub fn navigate_to(&mut self, path: String) {
        if !std::path::Path::new(&path).is_dir() {
            return;
        }
        self.root_path = path.clone();
        self.root_node = FileNode {
            name: path
                .split(['/', '\\'])
                .last()
                .unwrap_or("root")
                .to_string(),
            path: path.clone(),
            is_directory: true,
            children: None,
            is_expanded: true,
        };
        Self::load_directory(&mut self.root_node);
        self.address_bar_text = path;
        self.selected_file = None;
        self.selected_files.clear();
        self.selection_anchor = None;
        self.file_content = None;
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
