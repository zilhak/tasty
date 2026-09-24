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

    /// 파일 전체 surface의 누락·중복 ID를 다시 부여한다. 처음 나타난 고유 ID는 유지한다.
    /// 변경했으면 true를 반환해 호출자가 디스크에 다시 저장할 수 있게 한다.
    fn normalize_surface_ids(&mut self) -> bool;
}

// 먼저 기존 ID의 최댓값을 구하고 전체 트리를 다시 순회해 누락·중복 ID를 부여한다.

/// 다음 ID와 이미 사용한 ID를 기억하는 정규화 상태.
#[derive(Default)]
struct IdNormalizer {
    next: u32,
    claimed: std::collections::BTreeSet<u32>,
    changed: bool,
}

impl IdNormalizer {
    /// 패스 1: 기존 id 로 high-water mark 갱신.
    fn observe(&mut self, s: &PresetSurface) {
        if let Some(id) = s.id {
            self.next = self.next.max(id.saturating_add(1));
        }
    }

    /// 패스 2: 결손/중복이면 새 번호 부여. 최초 등장한 유효 id 는 그대로 확정.
    fn assign(&mut self, s: &mut PresetSurface) {
        match s.id {
            // 최초 등장한 유효 id — 유지(claimed 에 등록).
            Some(id) if self.claimed.insert(id) => {}
            // None 또는 이미 등장한 중복 id — high-water 이후로 재부여.
            _ => {
                let new_id = self.next;
                self.next = self.next.saturating_add(1);
                self.claimed.insert(new_id);
                s.id = Some(new_id);
                self.changed = true;
            }
        }
    }
}

fn observe_layout(layout: &PresetSurfaceLayout, n: &mut IdNormalizer) {
    match layout {
        PresetSurfaceLayout::Leaf { surface } => n.observe(surface),
        PresetSurfaceLayout::Split { first, second, .. } => {
            observe_layout(first, n);
            observe_layout(second, n);
        }
    }
}

fn assign_layout(layout: &mut PresetSurfaceLayout, n: &mut IdNormalizer) {
    match layout {
        PresetSurfaceLayout::Leaf { surface } => n.assign(surface),
        PresetSurfaceLayout::Split { first, second, .. } => {
            assign_layout(first, n);
            assign_layout(second, n);
        }
    }
}

fn observe_pane_node(node: &PresetPaneNode, n: &mut IdNormalizer) {
    match node {
        PresetPaneNode::Leaf { pane } => {
            for t in &pane.tabs {
                observe_layout(&t.layout, n);
            }
        }
        PresetPaneNode::Split { first, second, .. } => {
            observe_pane_node(first, n);
            observe_pane_node(second, n);
        }
    }
}

fn assign_pane_node(node: &mut PresetPaneNode, n: &mut IdNormalizer) {
    match node {
        PresetPaneNode::Leaf { pane } => {
            for t in &mut pane.tabs {
                assign_layout(&mut t.layout, n);
            }
        }
        PresetPaneNode::Split { first, second, .. } => {
            assign_pane_node(first, n);
            assign_pane_node(second, n);
        }
    }
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
    /// preset 파일 안에서 사용하는 식별자. runtime surface ID와는 별개다.
    /// 구버전이나 새 항목의 None은 로드·저장 때 정규화하며 최초 고유 ID는 유지한다.
    /// 복제한 preset은 같은 ID를 가져도 되며 실제 적용 시 호스트가 runtime ID를 새로 발급한다.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<u32>,

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
    fn normalize_surface_ids(&mut self) -> bool {
        let mut n = IdNormalizer::default();
        observe_pane_node(&self.layout, &mut n);
        assign_pane_node(&mut self.layout, &mut n);
        n.changed
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
    fn normalize_surface_ids(&mut self) -> bool {
        let mut n = IdNormalizer::default();
        observe_layout(&self.tab.layout, &mut n);
        assign_layout(&mut self.tab.layout, &mut n);
        n.changed
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
    fn normalize_surface_ids(&mut self) -> bool {
        let mut n = IdNormalizer::default();
        for t in &self.pane.tabs {
            observe_layout(&t.layout, &mut n);
        }
        for t in &mut self.pane.tabs {
            assign_layout(&mut t.layout, &mut n);
        }
        n.changed
    }
}

// ── tests ───────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_leaf() -> PresetSurfaceLayout {
        PresetSurfaceLayout::Leaf {
            surface: PresetSurface {
                id: None,
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
            id: None,
            kind: "html".into(),
            cwd: None,
            startup_command: None,
            params: serde_json::Value::Null,
        };
        let toml_str = toml::to_string(&s).unwrap();
        assert!(!toml_str.contains("cwd"));
        assert!(!toml_str.contains("startup_command"));
        assert!(!toml_str.contains("params"));
        assert!(!toml_str.contains("id"));
        assert!(toml_str.contains("kind"));
    }

    // ── surface id 정규화 ────────────────────────────────────────────────

    fn leaf_with_id(id: Option<u32>) -> PresetSurfaceLayout {
        PresetSurfaceLayout::Leaf {
            surface: PresetSurface {
                id,
                kind: "terminal".into(),
                cwd: None,
                startup_command: None,
                params: serde_json::Value::Null,
            },
        }
    }

    fn split(a: PresetSurfaceLayout, b: PresetSurfaceLayout) -> PresetSurfaceLayout {
        PresetSurfaceLayout::Split {
            direction: PresetSplitDirection::Vertical,
            ratio: 0.5,
            first: Box::new(a),
            second: Box::new(b),
        }
    }

    /// 트리의 surface id 를 방문 순서로 수집.
    fn surface_ids(layout: &PresetSurfaceLayout, out: &mut Vec<Option<u32>>) {
        match layout {
            PresetSurfaceLayout::Leaf { surface } => out.push(surface.id),
            PresetSurfaceLayout::Split { first, second, .. } => {
                surface_ids(first, out);
                surface_ids(second, out);
            }
        }
    }

    fn tab_preset(layout: PresetSurfaceLayout) -> TabPreset {
        TabPreset {
            name: "t".into(),
            tab: PresetTab {
                explicit_name: None,
                layout,
            },
        }
    }

    #[test]
    fn normalize_assigns_missing_ids_deterministically() {
        let mut t = tab_preset(split(
            leaf_with_id(None),
            split(leaf_with_id(None), leaf_with_id(None)),
        ));
        assert!(t.normalize_surface_ids());
        let mut ids = Vec::new();
        surface_ids(&t.tab.layout, &mut ids);
        assert_eq!(ids, vec![Some(0), Some(1), Some(2)]);
        assert!(!t.normalize_surface_ids());
    }

    #[test]
    fn normalize_preserves_existing_and_fills_gaps() {
        let mut t = tab_preset(split(leaf_with_id(Some(5)), leaf_with_id(None)));
        assert!(t.normalize_surface_ids());
        let mut ids = Vec::new();
        surface_ids(&t.tab.layout, &mut ids);
        assert_eq!(ids, vec![Some(5), Some(6)]);
    }

    #[test]
    fn normalize_reassigns_duplicates() {
        let mut t = tab_preset(split(leaf_with_id(Some(3)), leaf_with_id(Some(3))));
        assert!(t.normalize_surface_ids());
        let mut ids = Vec::new();
        surface_ids(&t.tab.layout, &mut ids);
        assert_eq!(ids[0], Some(3));
        assert_eq!(ids[1], Some(4)); // high-water = max(3)+1
        assert_ne!(ids[0], ids[1]);
    }

    #[test]
    fn normalize_noop_when_all_unique() {
        let mut t = tab_preset(split(leaf_with_id(Some(0)), leaf_with_id(Some(1))));
        assert!(!t.normalize_surface_ids());
    }

    #[test]
    fn normalize_workspace_is_file_wide_unique() {
        // 두 pane 각각 결손 leaf — 파일 전체에서 유일해야 한다(탭 단위 아님).
        let ws = WorkspacePreset {
            name: "w".into(),
            subtitle: String::new(),
            description: String::new(),
            layout: PresetPaneNode::Split {
                direction: PresetSplitDirection::Vertical,
                ratio: 0.5,
                first: Box::new(PresetPaneNode::Leaf {
                    pane: PresetPane {
                        tabs: vec![PresetTab {
                            explicit_name: None,
                            layout: leaf_with_id(None),
                        }],
                        active_tab: 0,
                    },
                }),
                second: Box::new(PresetPaneNode::Leaf {
                    pane: PresetPane {
                        tabs: vec![
                            PresetTab {
                                explicit_name: None,
                                layout: leaf_with_id(None),
                            },
                            PresetTab {
                                explicit_name: None,
                                layout: split(leaf_with_id(None), leaf_with_id(None)),
                            },
                        ],
                        active_tab: 0,
                    },
                }),
            },
        };
        let mut ws = ws;
        assert!(ws.normalize_surface_ids());
        let mut all: Vec<u32> = Vec::new();
        fn walk(node: &PresetPaneNode, out: &mut Vec<u32>) {
            match node {
                PresetPaneNode::Leaf { pane } => {
                    for t in &pane.tabs {
                        let mut ids = Vec::new();
                        surface_ids(&t.layout, &mut ids);
                        out.extend(ids.into_iter().flatten());
                    }
                }
                PresetPaneNode::Split { first, second, .. } => {
                    walk(first, out);
                    walk(second, out);
                }
            }
        }
        walk(&ws.layout, &mut all);
        assert_eq!(all.len(), 4);
        let unique: std::collections::BTreeSet<u32> = all.iter().copied().collect();
        assert_eq!(unique.len(), 4, "surface ids must be file-wide unique");
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
