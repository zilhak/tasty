//! Tasty File Explorer — 외부 plugin.
//!
//! 호스트의 `~/.tasty/plugins/com.tasty.explorer/`에 설치되어 spawn 된다.
//! 디렉터리 트리 + 우측 preview pane + 좌측 하단 즐겨찾기를 제공한다.
//! 본체 호스트 코드에는 의존하지 않으며, `tasty-plugin-sdk`만 사용한다.

use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tasty_plugin_sdk::{
    BusHandle, ButtonStyle, CommandInvokeCtx, HostHandle, Plugin, PluginEnv, SurfaceCreateCtx,
    SurfaceEventCtx, SurfaceRestoreCtx, SurfaceResult, Translator, UiEvent, UiNode,
    ui::{addressbar, hbox, label, label_color, scroll_v, splitter_id, tree_view, vbox},
};
use tasty_plugin_sdk::{SelectionMode, SplitDir, TreeNode};

fn make_button(id: &str, label_text: impl Into<String>, enabled: bool) -> UiNode {
    UiNode::Button {
        id: id.into(),
        label: label_text.into(),
        enabled,
        style: ButtonStyle::Secondary,
        tooltip_i18n_key: None,
    }
}

const PLUGIN_ID: &str = "com.tasty.explorer";
const PLUGIN_VERSION: &str = "0.1.0";
const TREE_NODE_ID: &str = "explorer.tree";
const ADDRESSBAR_ID: &str = "explorer.address";
const MAIN_SPLIT_ID: &str = "main_split";
const LEFT_SPLIT_ID: &str = "left_split";
const ADD_BOOKMARK_BTN: &str = "btn.add_bookmark";
const BOOKMARK_NAV_PREFIX: &str = "bm.nav.";
const BOOKMARK_RM_PREFIX: &str = "bm.rm.";
const PREVIEW_LIMIT_BYTES: usize = 16 * 1024;
const DEFAULT_TREE_RATIO: f32 = 0.4;
const DEFAULT_LEFT_INNER_RATIO: f32 = 0.7;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Bookmark {
    name: String,
    path: String,
}

#[derive(Default, Debug, Clone, Serialize, Deserialize)]
struct BookmarkStore {
    entries: Vec<Bookmark>,
}

impl BookmarkStore {
    fn path() -> Option<PathBuf> {
        std::env::var_os("TASTY_PLUGIN_DATA_DIR").map(|d| PathBuf::from(d).join("bookmarks.json"))
    }

    fn load() -> Self {
        let Some(p) = Self::path() else {
            return Self::default();
        };
        let Ok(s) = std::fs::read_to_string(&p) else {
            return Self::default();
        };
        serde_json::from_str(&s).unwrap_or_default()
    }

    fn save(&self) {
        let Some(p) = Self::path() else { return };
        if let Some(parent) = p.parent()
            && let Err(e) = std::fs::create_dir_all(parent)
        {
            tracing::warn!("bookmarks save mkdir failed: {e}");
            return;
        }
        match serde_json::to_string_pretty(self) {
            Ok(s) => {
                if let Err(e) = std::fs::write(&p, s) {
                    tracing::warn!("bookmarks save write failed: {e}");
                }
            }
            Err(e) => tracing::warn!("bookmarks save serialize failed: {e}"),
        }
    }

    fn add(&mut self, name: String, path: String) {
        self.entries.retain(|b| b.path != path);
        self.entries.insert(0, Bookmark { name, path });
        self.save();
    }

    fn remove_index(&mut self, idx: usize) {
        if idx < self.entries.len() {
            self.entries.remove(idx);
            self.save();
        }
    }

    fn has(&self, path: &str) -> bool {
        self.entries.iter().any(|b| b.path == path)
    }
}

struct ExplorerSurface {
    root: PathBuf,
    expanded: HashSet<PathBuf>,
    selected: Option<PathBuf>,
    preview: Option<String>,
    /// 좌(트리+즐겨찾기) vs 우(preview)의 가로 비율.
    tree_ratio: f32,
    /// 좌측 내부 세로 비율: 위(트리) vs 아래(즐겨찾기).
    left_inner_ratio: f32,
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
            tree_ratio: DEFAULT_TREE_RATIO,
            left_inner_ratio: DEFAULT_LEFT_INNER_RATIO,
        }
    }

    /// root 변경 단일 진입점. expanded/selected/preview 후처리를 일괄 수행.
    /// cwd 통보 (`surface.set_cwd`) 는 호출처가 별도로 발사 — host 핸들이
    /// `ExplorerSurface` 에 없기 때문.
    fn set_root(&mut self, new_root: PathBuf) {
        self.root = new_root.clone();
        self.expanded.clear();
        self.expanded.insert(new_root);
        self.selected = None;
        self.preview = None;
    }

    fn build_tree(&self, bookmarks: &BookmarkStore, tr: &Translator) -> UiNode {
        let root_node = build_node(&self.root, &self.expanded, &self.selected, 0);
        let tree_pane = scroll_v(tree_view(
            TREE_NODE_ID,
            vec![root_node],
            SelectionMode::Single,
        ));

        let bookmark_pane = scroll_v(build_bookmarks_section(bookmarks, tr));

        let left_column = splitter_id(
            LEFT_SPLIT_ID,
            SplitDir::Vertical,
            self.left_inner_ratio,
            tree_pane,
            bookmark_pane,
        );

        let preview = match &self.preview {
            Some(text) => label(text.clone()),
            None => label_color(tr.t("explorer.ui.preview_placeholder"), "subtext0"),
        };
        let preview_pane = scroll_v(preview);

        let split = splitter_id(
            MAIN_SPLIT_ID,
            SplitDir::Horizontal,
            self.tree_ratio,
            left_column,
            preview_pane,
        );

        let address_text = self
            .selected
            .as_ref()
            .unwrap_or(&self.root)
            .to_string_lossy()
            .to_string();
        let can_bookmark = self.selected.is_some() && !bookmarks.has(&address_text);
        vbox([
            hbox([
                addressbar(ADDRESSBAR_ID, address_text),
                make_button(ADD_BOOKMARK_BTN, "\u{2605} +", can_bookmark),
            ]),
            split,
        ])
    }

    fn refresh_preview(&mut self) {
        self.preview = self.selected.as_ref().and_then(|p| read_preview(p));
    }

    fn snapshot_value(&self) -> Value {
        serde_json::json!({
            "root": self.root.to_string_lossy(),
            "tree_ratio": self.tree_ratio,
            "left_inner_ratio": self.left_inner_ratio,
        })
    }
}

fn build_bookmarks_section(bookmarks: &BookmarkStore, tr: &Translator) -> UiNode {
    if bookmarks.entries.is_empty() {
        return vbox([
            label_color(tr.t("explorer.ui.bookmarks_heading"), "subtext1"),
            label_color(tr.t("explorer.ui.bookmarks_empty"), "subtext0"),
        ]);
    }
    let mut children: Vec<UiNode> = Vec::with_capacity(1 + bookmarks.entries.len());
    children.push(label_color(
        tr.t("explorer.ui.bookmarks_heading"),
        "subtext1",
    ));
    for (i, bm) in bookmarks.entries.iter().enumerate() {
        let nav_id = format!("{BOOKMARK_NAV_PREFIX}{i}");
        let rm_id = format!("{BOOKMARK_RM_PREFIX}{i}");
        let label_text = format!("\u{2605} {}", bm.name);
        children.push(hbox([
            make_button(&nav_id, label_text, true),
            make_button(&rm_id, "\u{2716}", true),
        ]));
    }
    vbox(children)
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
        icon: Some(if is_dir {
            "\u{1F4C1}".into()
        } else {
            "\u{1F4C4}".into()
        }),
        expanded: is_expanded,
        selected: is_selected,
        children,
        has_children: is_dir,
    }
}

/// dotfile blacklist. 현재 비어있다 — 추후 plugin 설정 메뉴에서 사용자가 추가한다.
const DOTFILE_BLACKLIST: &[&str] = &[];

fn list_children(dir: &Path) -> Vec<PathBuf> {
    let mut dirs: BTreeMap<String, PathBuf> = BTreeMap::new();
    let mut files: BTreeMap<String, PathBuf> = BTreeMap::new();
    let read = match std::fs::read_dir(dir) {
        Ok(r) => r,
        Err(_) => return Vec::new(),
    };
    for entry in read.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if DOTFILE_BLACKLIST.contains(&name.as_str()) {
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

/// host 에 surface 의 새 cwd 를 통보. fire-and-forget — 실패 시 warn 로깅만.
/// host 가 옛 버전이라 메서드 미지원이면 무해 (RemoteSurface.cwd 가 None 유지).
fn emit_cwd(host: Option<&HostHandle>, surface_id: u32, cwd: &Path) {
    let Some(host) = host else { return };
    if let Err(e) = host.call(
        "surface.set_cwd",
        serde_json::json!({
            "surface_id": surface_id,
            "cwd": cwd.to_string_lossy().to_string(),
        }),
    ) {
        tracing::warn!(surface_id = surface_id, "surface.set_cwd failed: {e}");
    }
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
    bookmarks: BookmarkStore,
    host: Option<HostHandle>,
    tr: Translator,
}

impl ExplorerPlugin {
    fn new(tr: Translator) -> Self {
        Self {
            surfaces: BTreeMap::new(),
            bookmarks: BookmarkStore::load(),
            host: None,
            tr,
        }
    }

    /// 우선순위: ① params.path (호출자가 명시한 경로) → ② ctx.cwd (호스트가
    /// source surface 로부터 carry 한 cwd) → ③ home dir (HOME / USERPROFILE)
    /// → ④ ".". 호스트 시작 cwd (env::current_dir) 는 fallback 으로 쓰지 않는다
    /// — 사용자 의도와 무관한 dir 이 새 surface 의 root 행세를 하지 않도록.
    fn root_from_ctx(ctx: &SurfaceCreateCtx) -> PathBuf {
        ctx.params
            .get("path")
            .and_then(|v| v.as_str())
            .map(PathBuf::from)
            .or_else(|| ctx.cwd.clone())
            .or_else(|| {
                std::env::var_os("HOME")
                    .or_else(|| std::env::var_os("USERPROFILE"))
                    .map(PathBuf::from)
            })
            .unwrap_or_else(|| PathBuf::from("."))
    }

    fn surface_result(&self, surface: &ExplorerSurface) -> SurfaceResult {
        let display_name = surface
            .root
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| self.tr.t("explorer.ui.default_display_name").to_string());
        SurfaceResult {
            tree: Some(surface.build_tree(&self.bookmarks, &self.tr)),
            display_name: Some(display_name),
            snapshot: Some(surface.snapshot_value()),
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
        let root = Self::root_from_ctx(&ctx);
        let surface = ExplorerSurface::new(root);
        let result = self.surface_result(&surface);
        self.surfaces.insert(ctx.surface_id, surface);
        result
    }

    fn restore_surface(&mut self, ctx: SurfaceRestoreCtx) -> SurfaceResult {
        // restore 는 data.root 가 layout.json 영속 값. fallback 은 home dir →
        // "." — 호스트 시작 cwd 는 의도적으로 사용하지 않는다 (create 와 동일 정책).
        let root = ctx
            .data
            .get("root")
            .and_then(|v| v.as_str())
            .map(PathBuf::from)
            .or_else(|| {
                std::env::var_os("HOME")
                    .or_else(|| std::env::var_os("USERPROFILE"))
                    .map(PathBuf::from)
            })
            .unwrap_or_else(|| PathBuf::from("."));
        let mut surface = ExplorerSurface::new(root);
        if let Some(tr) = ctx.data.get("tree_ratio").and_then(|v| v.as_f64()) {
            surface.tree_ratio = tr as f32;
        }
        if let Some(li) = ctx.data.get("left_inner_ratio").and_then(|v| v.as_f64()) {
            surface.left_inner_ratio = li as f32;
        }
        let result = self.surface_result(&surface);
        self.surfaces.insert(ctx.surface_id, surface);
        result
    }

    fn handle_event(&mut self, ctx: SurfaceEventCtx) -> SurfaceResult {
        let Some(surface) = self.surfaces.get_mut(&ctx.surface_id) else {
            return SurfaceResult::default();
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
            UiEvent::TreeActivate { node_id, path } if node_id == TREE_NODE_ID => {
                let p = PathBuf::from(&path);
                if p.is_dir() {
                    surface.set_root(p.clone());
                    emit_cwd(self.host.as_ref(), ctx.surface_id, &p);
                } else if let Some(host) = self.host.as_ref() {
                    if let Err(e) = host.call(
                        "file_handler.dispatch",
                        serde_json::json!({
                            "path": path,
                            "origin_surface_id": ctx.surface_id,
                        }),
                    ) {
                        tracing::warn!(path = %path, "file_handler.dispatch failed: {e}");
                    }
                } else {
                    tracing::warn!("explorer plugin host handle not set; cannot dispatch '{path}'");
                }
            }
            UiEvent::AddressbarSubmit { text, .. } => {
                let p = PathBuf::from(&text);
                if p.is_dir() {
                    surface.set_root(p.clone());
                    emit_cwd(self.host.as_ref(), ctx.surface_id, &p);
                }
            }
            UiEvent::SplitterDrag { node_id, ratio } => match node_id.as_str() {
                MAIN_SPLIT_ID => surface.tree_ratio = ratio,
                LEFT_SPLIT_ID => surface.left_inner_ratio = ratio,
                other => {
                    tracing::warn!("unknown splitter id '{other}' in drag event");
                }
            },
            UiEvent::Click { node_id } => {
                if node_id == ADD_BOOKMARK_BTN {
                    if let Some(sel) = surface.selected.clone() {
                        let path_s = sel.to_string_lossy().to_string();
                        let name = sel
                            .file_name()
                            .map(|s| s.to_string_lossy().to_string())
                            .unwrap_or_else(|| path_s.clone());
                        self.bookmarks.add(name, path_s);
                    }
                } else if let Some(rest) = node_id.strip_prefix(BOOKMARK_NAV_PREFIX) {
                    if let Ok(idx) = rest.parse::<usize>()
                        && let Some(bm) = self.bookmarks.entries.get(idx).cloned()
                    {
                        let p = PathBuf::from(&bm.path);
                        if p.is_dir() {
                            surface.set_root(p.clone());
                            emit_cwd(self.host.as_ref(), ctx.surface_id, &p);
                        } else if p.is_file() {
                            surface.selected = Some(p);
                            surface.refresh_preview();
                        }
                    }
                } else if let Some(rest) = node_id.strip_prefix(BOOKMARK_RM_PREFIX)
                    && let Ok(idx) = rest.parse::<usize>()
                {
                    self.bookmarks.remove_index(idx);
                }
            }
            _ => {}
        }
        SurfaceResult {
            tree: Some(surface.build_tree(&self.bookmarks, &self.tr)),
            display_name: None,
            snapshot: Some(surface.snapshot_value()),
        }
    }

    fn snapshot_surface(&mut self, ctx: tasty_plugin_sdk::SurfaceSnapshotCtx) -> Value {
        match self.surfaces.get(&ctx.surface_id) {
            Some(s) => s.snapshot_value(),
            None => Value::Null,
        }
    }

    fn destroy_surface(&mut self, surface_id: u32) {
        self.surfaces.remove(&surface_id);
    }

    fn on_start(&mut self, host: HostHandle, _bus: BusHandle) {
        self.host = Some(host);
    }

    fn handle_command(&mut self, ctx: CommandInvokeCtx) -> SurfaceResult {
        let Some(surface) = self.surfaces.get_mut(&ctx.surface_id) else {
            return SurfaceResult::default();
        };
        let result = match ctx.command_id.as_str() {
            "explorer.refresh" => {
                surface.refresh_preview();
                true
            }
            "explorer.go_up" => {
                if let Some(parent) = surface.root.parent().map(PathBuf::from) {
                    surface.set_root(parent.clone());
                    emit_cwd(self.host.as_ref(), ctx.surface_id, &parent);
                }
                true
            }
            other => {
                tracing::warn!("explorer plugin received unknown command '{other}'");
                false
            }
        };
        if result {
            SurfaceResult {
                tree: Some(surface.build_tree(&self.bookmarks, &self.tr)),
                display_name: None,
                snapshot: Some(surface.snapshot_value()),
            }
        } else {
            SurfaceResult::default()
        }
    }
}

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();
    let env = PluginEnv::load()?;
    let tr = Translator::from_plugin_env(&env);
    tasty_plugin_sdk::run(ExplorerPlugin::new(tr))
}
