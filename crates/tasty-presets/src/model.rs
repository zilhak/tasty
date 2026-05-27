//! Layout preset 데이터 모델.
//!
//! 세 종류의 preset (`WorkspacePreset`, `TabPreset`, `PanePreset`) 이 있으며 모두
//! `LayoutPreset` trait 를 구현한다. 디스크 저장은 toml, IPC 응답은 JSON 둘 다 가능.

use serde::{Deserialize, Serialize};

/// preset 종류. 디스크 디렉토리 구분에도 사용.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PresetKind {
    Workspace,
    Tab,
    Pane,
}

impl PresetKind {
    pub fn as_str(self) -> &'static str {
        match self {
            PresetKind::Workspace => "workspace",
            PresetKind::Tab => "tab",
            PresetKind::Pane => "pane",
        }
    }
}

/// 세 preset 종류의 공통 trait.
pub trait LayoutPreset: Sized + Clone + Serialize + for<'de> Deserialize<'de> {
    const KIND: PresetKind;
    fn name(&self) -> &str;
    fn set_name(&mut self, name: String);
}

// ── Split direction (serde 호환 직렬화 enum) ──────────────────────────────

/// 분할 방향의 디스크 직렬화 표현. 라이브 트리의 `SplitDirection` 과 같은 의미.
/// 변환 (From/Into) 은 *본 바이너리의 capture / apply 모듈* 에서 담당한다.
/// presets crate 자체는 어떤 외부 enum 도 모른다.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PresetSplitDirection {
    Horizontal,
    Vertical,
}

// ── Surface ──────────────────────────────────────────────────────────────

/// preset leaf — 단일 surface 의 초기화 데이터.
///
/// `kind` 는 SurfaceKindRegistry 의 키 (`terminal`, `markdown`, `html`, `image` 등).
/// `cwd` 와 `startup_command` 는 terminal 전용 (다른 kind 는 무시).
/// `params` 는 SurfaceKindDef.snapshot 이 만든 임의 JSON — kind 별 의미.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PresetSurface {
    pub kind: String,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub startup_command: Option<String>,

    #[serde(default, skip_serializing_if = "is_null_or_empty_object")]
    pub params: serde_json::Value,
}

fn is_null_or_empty_object(v: &serde_json::Value) -> bool {
    v.is_null() || matches!(v, serde_json::Value::Object(m) if m.is_empty())
}

// ── 하위 레이아웃 (탭 안의 split) ──────────────────────────────────────────

/// 탭 내부 레이아웃 (하위 split).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum PresetSurfaceLayout {
    Leaf {
        surface: PresetSurface,
    },
    Split {
        direction: PresetSplitDirection,
        ratio: f32,
        first: Box<PresetSurfaceLayout>,
        second: Box<PresetSurfaceLayout>,
    },
}

// ── 탭 ──────────────────────────────────────────────────────────────────

/// preset 안의 단일 탭.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PresetTab {
    /// 사용자가 지정한 탭 이름. None 이면 자동 (surface display name 사용).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub explicit_name: Option<String>,

    pub layout: PresetSurfaceLayout,
}

// ── 페인 노드 (상위 레이아웃) ───────────────────────────────────────────────

/// workspace 의 상위 레이아웃 (페인 단위 split).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum PresetPaneNode {
    Leaf {
        pane: PresetPane,
    },
    Split {
        direction: PresetSplitDirection,
        ratio: f32,
        first: Box<PresetPaneNode>,
        second: Box<PresetPaneNode>,
    },
}

/// preset 안의 단일 페인 (탭들의 컨테이너).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PresetPane {
    /// 페인 안의 탭 목록. 최소 1개.
    pub tabs: Vec<PresetTab>,
    /// 활성 탭 인덱스. tabs 범위 안.
    #[serde(default)]
    pub active_tab: usize,
}

// ── 3종 preset ──────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WorkspacePreset {
    pub name: String,

    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub subtitle: String,

    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,

    pub layout: PresetPaneNode,
}

impl LayoutPreset for WorkspacePreset {
    const KIND: PresetKind = PresetKind::Workspace;
    fn name(&self) -> &str {
        &self.name
    }
    fn set_name(&mut self, name: String) {
        self.name = name;
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TabPreset {
    pub name: String,
    pub tab: PresetTab,
}

impl LayoutPreset for TabPreset {
    const KIND: PresetKind = PresetKind::Tab;
    fn name(&self) -> &str {
        &self.name
    }
    fn set_name(&mut self, name: String) {
        self.name = name;
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PanePreset {
    pub name: String,
    pub pane: PresetPane,
}

impl LayoutPreset for PanePreset {
    const KIND: PresetKind = PresetKind::Pane;
    fn name(&self) -> &str {
        &self.name
    }
    fn set_name(&mut self, name: String) {
        self.name = name;
    }
}

// ── tests ───────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_leaf() -> PresetSurfaceLayout {
        PresetSurfaceLayout::Leaf {
            surface: PresetSurface {
                kind: "terminal".into(),
                cwd: Some("/tmp".into()),
                startup_command: Some("ls".into()),
                params: serde_json::Value::Null,
            },
        }
    }

    #[test]
    fn workspace_preset_round_trip_toml() {
        let ws = WorkspacePreset {
            name: "dev".into(),
            subtitle: "main".into(),
            description: String::new(),
            layout: PresetPaneNode::Leaf {
                pane: PresetPane {
                    tabs: vec![PresetTab {
                        explicit_name: Some("build".into()),
                        layout: sample_leaf(),
                    }],
                    active_tab: 0,
                },
            },
        };
        let s = toml::to_string_pretty(&ws).unwrap();
        let back: WorkspacePreset = toml::from_str(&s).unwrap();
        assert_eq!(ws, back);
    }

    #[test]
    fn pane_preset_serializes_active_tab() {
        let p = PanePreset {
            name: "claude".into(),
            pane: PresetPane {
                tabs: vec![
                    PresetTab {
                        explicit_name: None,
                        layout: sample_leaf(),
                    },
                    PresetTab {
                        explicit_name: Some("b".into()),
                        layout: sample_leaf(),
                    },
                ],
                active_tab: 1,
            },
        };
        let s = toml::to_string_pretty(&p).unwrap();
        assert!(s.contains("active_tab = 1"));
        let back: PanePreset = toml::from_str(&s).unwrap();
        assert_eq!(p.pane.active_tab, back.pane.active_tab);
    }

    #[test]
    fn tab_preset_with_split_surface_layout() {
        let t = TabPreset {
            name: "split".into(),
            tab: PresetTab {
                explicit_name: None,
                layout: PresetSurfaceLayout::Split {
                    direction: PresetSplitDirection::Vertical,
                    ratio: 0.5,
                    first: Box::new(sample_leaf()),
                    second: Box::new(sample_leaf()),
                },
            },
        };
        let s = toml::to_string_pretty(&t).unwrap();
        let back: TabPreset = toml::from_str(&s).unwrap();
        assert_eq!(t, back);
    }

    #[test]
    fn preset_surface_omits_optional_fields_when_none() {
        let s = PresetSurface {
            kind: "html".into(),
            cwd: None,
            startup_command: None,
            params: serde_json::Value::Null,
        };
        let toml_str = toml::to_string(&s).unwrap();
        assert!(!toml_str.contains("cwd"));
        assert!(!toml_str.contains("startup_command"));
        assert!(!toml_str.contains("params"));
        assert!(toml_str.contains("kind"));
    }

    #[test]
    fn preset_kind_string_round_trip() {
        for k in [PresetKind::Workspace, PresetKind::Tab, PresetKind::Pane] {
            let s = serde_json::to_string(&k).unwrap();
            let back: PresetKind = serde_json::from_str(&s).unwrap();
            assert_eq!(k, back);
            assert_eq!(s.trim_matches('"'), k.as_str());
        }
    }
}
