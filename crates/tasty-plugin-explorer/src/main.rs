//! Tasty File Explorer — 외부 plugin.
//!
//! 호스트의 `~/.tasty/plugins/com.tasty.explorer/`에 설치되어 spawn 된다.
//! 디렉터리 트리를 표시하고 사용자 선택을 처리한다. 본체 호스트 코드에는
//! 의존하지 않으며, `tasty-plugin-sdk`만 사용한다.

use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};

use serde_json::Value;
use tasty_plugin_sdk::{
    Plugin, SurfaceCreateCtx, SurfaceEventCtx, SurfaceResult, SurfaceRestoreCtx, UiEvent, UiNode,
    ui::{addressbar, hbox, label, scroll_v, splitter, tree_view, vbox},
};
use tasty_plugin_sdk::{SelectionMode, SplitDir, TreeNode};

const PLUGIN_ID: &str = "com.tasty.explorer";
const PLUGIN_VERSION: &str = "0.1.0";
const TREE_NODE_ID: &str = "explorer.tree";
const ADDRESSBAR_ID: &str = "explorer.address";
const PREVIEW_LIMIT_BYTES: usize = 16 * 1024;

struct ExplorerSurface {
    root: PathBuf,
    expanded: HashSet<PathBuf>,
    selected: Option<PathBuf>,
    preview: Option<String>,
}

impl ExplorerSurface {
    fn new(root: PathBuf) -> Self {
        let mut expanded = HashSet::new();
        expanded.insert(root.clone());
        Self {
            root,
            expanded,
            selected: None,
            preview: None,
        }
    }

    fn build_tree(&self) -> UiNode {
        let root_node = build_node(&self.root, &self.expanded, &self.selected, 0);
        let tree = scroll_v(tree_view(
            TREE_NODE_ID,
            vec![root_node],
            SelectionMode::Single,
        ));
        let address_text = self
            .selected
            .as_ref()
            .unwrap_or(&self.root)
            .to_string_lossy()
            .to_string();
        let preview = match &self.preview {
            Some(text) => label(text.clone()),
            None => label("Select a file to preview"),
        };
        vbox([
            hbox([addressbar(ADDRESSBAR_ID, address_text)]),
            splitter(SplitDir::Horizontal, 0.4, tree, scroll_v(preview)),
        ])
    }

    fn refresh_preview(&mut self) {
        self.preview = self.selected.as_ref().and_then(|p| read_preview(p));
    }
}

fn build_node(
    path: &Path,
    expanded: &HashSet<PathBuf>,
    selected: &Option<PathBuf>,
    depth: usize,
) -> TreeNode {
    let name = path
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.to_string_lossy().into_owned());
    let is_dir = path.is_dir();
    let is_expanded = expanded.contains(path);
    let is_selected = selected.as_deref() == Some(path);

    let children: Vec<TreeNode> = if is_dir && is_expanded {
        let _ = depth; // 향후 깊이 제한용
        list_children(path)
            .into_iter()
            .map(|child_path| build_node(&child_path, expanded, selected, depth + 1))
            .collect()
    } else {
        Vec::new()
    };

    TreeNode {
        id: path.to_string_lossy().to_string(),
        label: name,
        icon: Some(if is_dir { "📁".into() } else { "📄".into() }),
        expanded: is_expanded,
        selected: is_selected,
        children,
    }
}

fn list_children(dir: &Path) -> Vec<PathBuf> {
    let mut dirs: BTreeMap<String, PathBuf> = BTreeMap::new();
    let mut files: BTreeMap<String, PathBuf> = BTreeMap::new();
    let read = match std::fs::read_dir(dir) {
        Ok(r) => r,
        Err(_) => return Vec::new(),
    };
    for entry in read.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with('.')
            && !matches!(
                name.as_str(),
                ".env" | ".gitignore" | ".claude" | ".editorconfig"
            )
        {
            continue;
        }
        let path = entry.path();
        let key = name.to_lowercase();
        if entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
            dirs.insert(key, path);
        } else {
            files.insert(key, path);
        }
    }
    dirs.into_values().chain(files.into_values()).collect()
}

fn read_preview(path: &Path) -> Option<String> {
    if !path.is_file() {
        return None;
    }
    let bytes = std::fs::read(path).ok()?;
    let take = bytes.len().min(PREVIEW_LIMIT_BYTES);
    let slice = &bytes[..take];
    let text = String::from_utf8_lossy(slice).to_string();
    let mut out = text;
    if bytes.len() > PREVIEW_LIMIT_BYTES {
        out.push_str("\n\n[truncated]");
    }
    Some(out)
}

struct ExplorerPlugin {
    surfaces: BTreeMap<u32, ExplorerSurface>,
}

impl ExplorerPlugin {
    fn new() -> Self {
        Self {
            surfaces: BTreeMap::new(),
        }
    }

    fn root_from_params(params: &Value) -> PathBuf {
        params
            .get("path")
            .and_then(|v| v.as_str())
            .map(PathBuf::from)
            .or_else(|| std::env::current_dir().ok())
            .unwrap_or_else(|| PathBuf::from("."))
    }

    fn surface_result(&self, surface: &ExplorerSurface) -> SurfaceResult {
        let display_name = surface
            .root
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| "Files".to_string());
        SurfaceResult {
            tree: Some(surface.build_tree()),
            display_name: Some(display_name),
        }
    }
}

impl Plugin for ExplorerPlugin {
    fn id(&self) -> &str {
        PLUGIN_ID
    }

    fn version(&self) -> &str {
        PLUGIN_VERSION
    }

    fn create_surface(&mut self, ctx: SurfaceCreateCtx) -> SurfaceResult {
        let root = Self::root_from_params(&ctx.params);
        let surface = ExplorerSurface::new(root);
        let result = self.surface_result(&surface);
        self.surfaces.insert(ctx.surface_id, surface);
        result
    }

    fn restore_surface(&mut self, ctx: SurfaceRestoreCtx) -> SurfaceResult {
        let root = ctx
            .data
            .get("root")
            .and_then(|v| v.as_str())
            .map(PathBuf::from)
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
        let surface = ExplorerSurface::new(root);
        let result = self.surface_result(&surface);
        self.surfaces.insert(ctx.surface_id, surface);
        result
    }

    fn handle_event(&mut self, ctx: SurfaceEventCtx) -> SurfaceResult {
        let Some(surface) = self.surfaces.get_mut(&ctx.surface_id) else {
            return SurfaceResult {
                tree: None,
                display_name: None,
            };
        };
        match ctx.event {
            UiEvent::TreeExpand {
                node_id: _,
                path,
                expanded,
            } => {
                let p = PathBuf::from(&path);
                if expanded {
                    surface.expanded.insert(p);
                } else {
                    surface.expanded.remove(&p);
                }
            }
            UiEvent::TreeSelect { selected, .. } => {
                surface.selected = selected.first().map(PathBuf::from);
                surface.refresh_preview();
            }
            UiEvent::AddressbarSubmit { text, .. } => {
                let p = PathBuf::from(&text);
                if p.is_dir() {
                    surface.root = p.clone();
                    surface.expanded.clear();
                    surface.expanded.insert(p);
                    surface.selected = None;
                    surface.preview = None;
                }
            }
            _ => {}
        }
        SurfaceResult {
            tree: Some(surface.build_tree()),
            display_name: None,
        }
    }

    fn snapshot_surface(&mut self, ctx: tasty_plugin_sdk::SurfaceSnapshotCtx) -> Value {
        match self.surfaces.get(&ctx.surface_id) {
            Some(s) => serde_json::json!({ "root": s.root.to_string_lossy() }),
            None => Value::Null,
        }
    }

    fn destroy_surface(&mut self, surface_id: u32) {
        self.surfaces.remove(&surface_id);
    }
}

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info".into()),
        )
        .init();
    tasty_plugin_sdk::run(ExplorerPlugin::new())
}
