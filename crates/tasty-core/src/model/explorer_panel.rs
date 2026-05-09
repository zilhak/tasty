use std::collections::HashSet;

use super::SurfaceId;
use super::surface_trait::Surface;

/// A node in the file tree.
pub struct FileNode {
    pub name: String,
    pub path: String,
    pub is_directory: bool,
    pub children: Option<Vec<FileNode>>,
    pub is_expanded: bool,
}

/// 파일 탐색기 surface 모델. 디렉터리 트리 데이터(`root_node`)와 루트 경로만 보유.
/// 선택/스크롤/주소바 편집 등 휘발성 GUI 상태는 호스트의 `ExplorerView`에 둔다.
pub struct ExplorerPanel {
    pub id: u32,
    pub root_path: String,
    pub root_node: FileNode,
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
            let expanded_map: HashSet<String> = old_children
                .iter()
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

    /// 새 루트 경로로 트리를 다시 로드한다. 디렉터리가 아니면 false 반환.
    /// view 측 selection/scroll/address bar 리셋은 호출자가 별도로 처리한다
    /// (`ExplorerView::reset_after_navigate`).
    pub fn navigate_to(&mut self, path: String) -> bool {
        if !std::path::Path::new(&path).is_dir() {
            return false;
        }
        self.root_path = path.clone();
        self.root_node = FileNode {
            name: path.split(['/', '\\']).last().unwrap_or("root").to_string(),
            path,
            is_directory: true,
            children: None,
            is_expanded: true,
        };
        Self::load_directory(&mut self.root_node);
        true
    }
}

/// 파일이 미리보기 가능한 텍스트 포맷인지 판정. ExplorerView에서 호출.
pub fn is_previewable_file(path: &str, ext: &str) -> bool {
    const TEXT_EXTENSIONS: &[&str] = &[
        // Markup / Doc
        "md",
        "markdown",
        "txt",
        "text",
        "rst",
        "adoc",
        "org",
        // Web
        "html",
        "htm",
        "css",
        "js",
        "jsx",
        "ts",
        "tsx",
        "vue",
        "svelte",
        "json",
        "xml",
        "svg",
        // Config
        "toml",
        "yaml",
        "yml",
        "ini",
        "cfg",
        "conf",
        "env",
        "properties",
        // Programming
        "rs",
        "py",
        "go",
        "java",
        "kt",
        "kts",
        "c",
        "cpp",
        "cc",
        "h",
        "hpp",
        "hh",
        "cs",
        "swift",
        "rb",
        "pl",
        "pm",
        "lua",
        "r",
        "jl",
        "ex",
        "exs",
        "erl",
        "hrl",
        "hs",
        "ml",
        "mli",
        "fs",
        "fsi",
        "fsx",
        "clj",
        "cljs",
        "scala",
        "sc",
        "zig",
        "nim",
        "v",
        "d",
        "dart",
        "php",
        // Shell
        "sh",
        "bash",
        "zsh",
        "fish",
        "ps1",
        "psm1",
        "bat",
        "cmd",
        // Data
        "csv",
        "tsv",
        "sql",
        "graphql",
        "gql",
        // Build / CI
        "cmake",
        "gradle",
        "sbt",
        "cabal",
        // Other
        "log",
        "diff",
        "patch",
        "gitignore",
        "gitattributes",
        "editorconfig",
        "dockerignore",
        "prettierrc",
        "eslintrc",
        "babelrc",
    ];

    if TEXT_EXTENSIONS.contains(&ext) {
        return true;
    }

    // Check extensionless known filenames
    let filename = path.rsplit(['/', '\\']).next().unwrap_or("");
    const TEXT_FILENAMES: &[&str] = &[
        "Makefile",
        "makefile",
        "GNUmakefile",
        "Dockerfile",
        "Containerfile",
        "Rakefile",
        "Gemfile",
        "Procfile",
        "Justfile",
        "Vagrantfile",
        "CMakeLists.txt",
        "LICENSE",
        "LICENCE",
        "COPYING",
        "AUTHORS",
        "CHANGELOG",
        "README",
        "INSTALL",
        "TODO",
        "CONTRIBUTORS",
        ".gitignore",
        ".gitattributes",
        ".editorconfig",
        ".dockerignore",
        ".env",
        ".env.local",
        ".env.example",
    ];

    if TEXT_FILENAMES.contains(&filename) {
        return true;
    }

    // No extension and not a known filename — skip preview
    false
}

impl Surface for ExplorerPanel {
    crate::impl_surface_any!();

    fn kind(&self) -> &'static str {
        "explorer"
    }
    fn type_name(&self) -> &'static str {
        "Explorer"
    }
    fn surface_id(&self) -> Option<SurfaceId> {
        Some(self.id)
    }
    fn display_name(&self) -> String {
        std::path::Path::new(&self.root_path)
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "Explorer".to_string())
    }
    fn as_explorer(&self) -> Option<&ExplorerPanel> {
        Some(self)
    }
    fn as_explorer_mut(&mut self) -> Option<&mut ExplorerPanel> {
        Some(self)
    }
    /// 실제로 트리가 가리키는 `root_path`만 사용한다 (주소바 편집 버퍼는 view에 있음).
    fn source_cwd(&self) -> Option<std::path::PathBuf> {
        if self.root_path.is_empty() {
            None
        } else {
            Some(std::path::PathBuf::from(&self.root_path))
        }
    }
}
