//! Layout persistence wire format — `~/.tasty/layout.json` 에 직렬화되는 타입들.
//!
//! 모든 신규 surface 종류는 `SavedSurface::Generic { kind, data }` 변종으로 그대로
//! 거쳐가므로 본 schema 는 surface 추가에 변경되지 않는다.

use serde::de::{self, Deserializer};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::model::{SplitDirection, WorkspaceAttachMapping};

// ── Serializable structs ──

#[derive(Serialize, Deserialize)]
pub struct SavedLayout {
    pub version: u32,
    pub workspaces: Vec<SavedWorkspace>,
    pub active_workspace: usize,
}

#[derive(Serialize, Deserialize)]
pub struct SavedWorkspace {
    pub name: String,
    pub subtitle: String,
    pub description: String,
    pub pane_layout: SavedPaneNode,
    /// Index of the focused pane among all leaf panes (left-to-right DFS order).
    pub focused_pane_index: usize,
    /// attach/detach 단계 7 — 원격 컴퓨터(SSH) attach 매핑. `#[serde(default)]` 로
    /// 구버전 layout.json(필드 없음) 과 호환. None 이면 일반(로컬) 워크스페이스.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attach_mapping: Option<WorkspaceAttachMapping>,
}

#[derive(Serialize, Deserialize)]
pub enum SavedPaneNode {
    Leaf(SavedPane),
    Split {
        direction: SavedSplitDirection,
        ratio: f32,
        first: Box<SavedPaneNode>,
        second: Box<SavedPaneNode>,
    },
}

#[derive(Serialize, Deserialize)]
pub struct SavedPane {
    pub tabs: Vec<SavedTab>,
    pub active_tab: usize,
}

#[derive(Serialize, Deserialize)]
pub struct SavedTab {
    pub name: String,
    pub explicit_name: Option<String>,
    pub surface: SavedSurfaceLayout,
}

#[derive(Serialize, Deserialize)]
pub enum SavedSurfaceLayout {
    Leaf(SavedSurface),
    Split {
        direction: SavedSplitDirection,
        ratio: f32,
        first: Box<SavedSurfaceLayout>,
        second: Box<SavedSurfaceLayout>,
    },
}

#[derive(Serialize, Deserialize, Clone, Copy)]
pub enum SavedSplitDirection {
    Horizontal,
    Vertical,
}

/// Persistent surface representation.
///
/// `Terminal` stays its own variant because PTY spawn is host-managed and needs
/// engine state (cols/rows/shell/waker) at restore time; routing it through the
/// registry would muddle that path. Every other surface kind goes through `Generic`
/// where the per-kind shape is opaque JSON, defined by the `SurfaceKindDef::snapshot`
/// / `restore` pair in the registry.
#[derive(Serialize)]
pub enum SavedSurface {
    Terminal {
        cwd: Option<String>,
        /// Command to re-launch the TUI app that was running (e.g. "claude -r <session-id>").
        /// Populated from surface-meta `restore.command` at capture time; plugins own the format.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        restore_command: Option<String>,
        /// `~/.tasty/scrollback/<id>.bin` 파일 식별자. `restore_surface_content` 옵션
        /// on 일 때만 발급된다. `None` 이면 scrollback 복원을 시도하지 않는다.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        scrollback_ref: Option<String>,
    },
    Generic {
        kind: String,
        data: Value,
    },
}

impl<'de> Deserialize<'de> for SavedSurface {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let v = Value::deserialize(d)?;
        let obj = v
            .as_object()
            .ok_or_else(|| de::Error::custom("SavedSurface must be an object"))?;
        if obj.len() != 1 {
            return Err(de::Error::custom(
                "SavedSurface object must have exactly one variant key",
            ));
        }
        let (key, inner) = obj.iter().next().unwrap();
        match key.as_str() {
            "Terminal" => {
                let cwd = inner
                    .get("cwd")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());
                let restore_command = inner
                    .get("restore_command")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());
                let scrollback_ref = inner
                    .get("scrollback_ref")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());
                Ok(SavedSurface::Terminal {
                    cwd,
                    restore_command,
                    scrollback_ref,
                })
            }
            "Generic" => {
                let kind = inner
                    .get("kind")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| de::Error::custom("Generic missing 'kind'"))?
                    .to_string();
                let data = inner.get("data").cloned().unwrap_or_else(|| json!({}));
                Ok(SavedSurface::Generic { kind, data })
            }
            other => Err(de::Error::unknown_variant(other, &["Terminal", "Generic"])),
        }
    }
}

// ── Direction conversion ──

impl From<SplitDirection> for SavedSplitDirection {
    fn from(d: SplitDirection) -> Self {
        match d {
            SplitDirection::Horizontal => SavedSplitDirection::Horizontal,
            SplitDirection::Vertical => SavedSplitDirection::Vertical,
        }
    }
}

impl From<SavedSplitDirection> for SplitDirection {
    fn from(d: SavedSplitDirection) -> Self {
        match d {
            SavedSplitDirection::Horizontal => SplitDirection::Horizontal,
            SavedSplitDirection::Vertical => SplitDirection::Vertical,
        }
    }
}
