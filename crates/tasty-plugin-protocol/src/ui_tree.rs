//! Plugin이 호스트에 보내는 UI tree 표현 + 호스트가 plugin에 보내는 사용자 이벤트.
//!
//! plugin은 매 surface.create / surface.event 응답에 `tree: UiNode | null`을 포함.
//! null이면 호스트는 이전 트리를 그대로 사용 (변경 없음).
//!
//! 위젯 v1: vbox/hbox/scroll/splitter, label/icon, button/tree/addressbar/text_preview,
//! spacer, canvas. 더 풍부한 위젯은 호스트 버전 업과 함께 추가.

use serde::{Deserialize, Serialize};

use crate::protocol::{PixelFilter, PixelFormat, SharedBufferId};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum UiNode {
    Vbox {
        #[serde(default)]
        spacing: u32,
        children: Vec<UiNode>,
    },
    Hbox {
        #[serde(default)]
        spacing: u32,
        children: Vec<UiNode>,
    },
    Scroll {
        #[serde(default = "default_true")]
        vertical: bool,
        #[serde(default)]
        horizontal: bool,
        child: Box<UiNode>,
    },
    Splitter {
        direction: SplitDir,
        ratio: f32,
        first: Box<UiNode>,
        second: Box<UiNode>,
        /// 설정 시 사용자가 divider를 드래그해 ratio를 조절할 수 있다.
        /// 호스트는 drag 중 `UiEvent::SplitterDrag { node_id, ratio }`를 plugin에
        /// 송신한다. `None`이면 고정 비율(드래그 불가).
        #[serde(default)]
        id: Option<String>,
    },

    /// 단일 자식을 부모 가용영역의 **양축(가로·세로) 중앙**에 배치하는 컨테이너.
    /// 빈/실패 상태 메시지 한 줄을 popup/pane 정중앙에 두는 용도. 호스트는
    /// `Layout::centered_and_justified` 로 렌더한다(디자인 `Align2::CENTER_CENTER` 등가).
    ///
    /// 다중 자식의 양축 중앙 의미는 모호하므로 계약상 자식은 1개로 고정한다.
    /// (추후 다양한 정렬이 필요하면 `Align { align, child }` 로 일반화 가능.)
    Center {
        child: Box<UiNode>,
    },

    Label {
        text: String,
        #[serde(default)]
        style: LabelStyle,
        /// `text|subtext0|subtext1|overlay0|blue|green|red|yellow` 또는 `#aabbcc`.
        #[serde(default)]
        color: Option<String>,
    },
    Icon {
        name: String,
    },

    Button {
        id: String,
        label: String,
        #[serde(default = "default_true")]
        enabled: bool,
        #[serde(default)]
        style: ButtonStyle,
        /// 컨테이너 폭을 채우는 full-width 버튼(디자인 `block`). 호스트가
        /// `tasty_ui_widgets::Button::block` 으로 전달한다.
        #[serde(default)]
        block: bool,
        #[serde(default)]
        tooltip_i18n_key: Option<String>,
    },
    Tree {
        id: String,
        nodes: Vec<TreeNode>,
        #[serde(default)]
        selection_mode: SelectionMode,
    },
    Addressbar {
        id: String,
        text: String,
        #[serde(default)]
        placeholder_i18n_key: Option<String>,
    },
    TextPreview {
        content: String,
        #[serde(default)]
        language: String,
    },
    Spacer {
        size: u32,
    },

    /// 외곽선 mono chip — surface kind / worktree type / status prefix 등 의미 라벨.
    /// 호스트가 `tasty_ui_widgets::tag` 로 렌더한다(비인터랙티브). `dot` 이면 선행
    /// 8px 상태 점. (디자인 `core/Tag`, transcription-spec §2-D)
    Tag {
        text: String,
        #[serde(default)]
        tone: TagTone,
        #[serde(default)]
        dot: bool,
    },

    /// 채움 pill badge — count / status. 호스트가 `tasty_ui_widgets::badge`(또는
    /// `badge_dot`)로 렌더한다. `dot` 이면 라벨 없는 8px 점. (디자인 `core/Badge`)
    Badge {
        text: String,
        #[serde(default)]
        tone: BadgeTone,
        #[serde(default)]
        dot: bool,
    },

    /// 클릭 가능한 컨테이너 행. `selected = true`이면 호스트가 hover 오버레이 배경을
    /// 깐다. 사용자가 클릭하면 [`UiEvent::Click`]이 `node_id = id`로 발화된다.
    /// 자식은 임의 UiNode (보통 hbox로 multi-span 라벨을 구성).
    SelectableRow {
        id: String,
        #[serde(default)]
        selected: bool,
        children: Vec<UiNode>,
    },

    /// SharedBuffer에 plugin이 직접 그린 raster를 노출하는 텍스처 영역.
    ///
    /// 호스트는 `(plugin_id, buffer_id)`를 키로 GPU 텍스처를 캐시하고, `Dirty` 메시지를
    /// 받은 영역만 staging 경로로 부분 업로드한다. plugin은 [`crate::SharedBufferId`]를
    /// 받아 영역에 픽셀을 쓰고 commit하면 호스트가 일관된 frame을 합성한다.
    ///
    /// `id`가 있으면 마우스 입력이 [`UiEvent::CanvasPointer`]로 라우팅된다.
    Canvas {
        /// 호스트가 부여한 SharedBuffer id. plugin이 `host.create_shared_buffer`로 받음.
        buffer_id: SharedBufferId,
        /// 픽셀 width. `width * height * format.bytes_per_pixel() + 8(footer) ≤ buffer.len()`
        /// 이어야 한다 (호스트가 검증).
        width: u32,
        /// 픽셀 height.
        height: u32,
        /// 픽셀 포맷.
        format: PixelFormat,
        /// 텍스처 샘플링 보간. 기본 [`PixelFilter::Linear`].
        #[serde(default)]
        filter: PixelFilter,
        /// 이 트리 스냅샷이 짝지어진 commit generation 번호 (디버깅·일관성 보조 용도).
        /// 호스트는 atomic load 값을 우선 사용한다.
        #[serde(default)]
        commit_seq: u64,
        /// 마우스 입력을 받기 위한 node id. None이면 hit-test 비활성.
        #[serde(default)]
        id: Option<String>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TreeNode {
    pub id: String,
    pub label: String,
    #[serde(default)]
    pub icon: Option<String>,
    #[serde(default)]
    pub expanded: bool,
    #[serde(default)]
    pub selected: bool,
    #[serde(default)]
    pub children: Vec<TreeNode>,
    /// Host hint: 이 노드가 자식을 가질 수 있는 컨테이너인가.
    /// `true` 면 host 는 `children` 이 비어 있어도 `CollapsingHeader` 로 렌더하여
    /// 사용자가 expand 토글로 다시 펼 수 있게 한다 (lazy children 모델 지원).
    /// 일반 leaf 는 `false` (기본).
    #[serde(default)]
    pub has_children: bool,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum SplitDir {
    Horizontal,
    Vertical,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum LabelStyle {
    #[default]
    Body,
    Caption,
    Heading,
    Dim,
    /// Monospace 폰트 본문 — diff/log/code 표시용. 색은 별도 `color` 필드로.
    Mono,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ButtonStyle {
    #[default]
    Secondary,
    Primary,
}

/// `UiNode::Tag` 톤 (디자인 `core/Tag` variant). 색은 호스트의 의미 토큰 accessor 로
/// 결정된다 (`tasty_ui_widgets::TagVariant` 와 1:1).
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TagTone {
    /// 외곽선(기본) — surface_raised + border_default + text_secondary.
    #[default]
    Default,
    Accent,
    Agent,
    Success,
    Warning,
    Danger,
}

/// `UiNode::Badge` 톤 (디자인 `core/Badge` variant). `tasty_ui_widgets::BadgeVariant`
/// 와 1:1.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BadgeTone {
    /// 채움 danger(기본 — unread count).
    #[default]
    Danger,
    Primary,
    Agent,
    Success,
    Neutral,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum SelectionMode {
    #[default]
    Single,
    Multi,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum UiEvent {
    Click {
        node_id: String,
    },
    Key {
        key: String,
        #[serde(default)]
        mods: Vec<String>,
    },
    TreeSelect {
        node_id: String,
        selected: Vec<String>,
    },
    TreeExpand {
        node_id: String,
        path: String,
        expanded: bool,
    },
    /// 트리 항목의 더블클릭 또는 Enter. `TreeSelect` 가 동일 클릭 시퀀스에서
    /// 선행 발화한 직후 추가로 도착한다. `path` 는 `TreeSelect.selected` 와 동일
    /// 형식 (부모 경로를 슬래시로 합친 식별자) — explorer 의 경우 실제
    /// 파일 / 디렉토리 절대 경로.
    TreeActivate {
        node_id: String,
        path: String,
    },
    AddressbarChange {
        node_id: String,
        text: String,
    },
    AddressbarSubmit {
        node_id: String,
        text: String,
    },
    ContextMenu {
        node_id: String,
        path: String,
        x: f32,
        y: f32,
    },
    Scroll {
        node_id: String,
        delta_y: f32,
    },
    /// 사용자가 draggable splitter divider를 드래그해 비율이 변경됨.
    SplitterDrag {
        node_id: String,
        ratio: f32,
    },
    FocusChanged {
        focused: bool,
    },
    Resize {
        width: u32,
        height: u32,
    },
    /// `UiNode::Canvas` 영역 위 마우스 포인터 이벤트.
    ///
    /// 좌표는 canvas-local 픽셀 좌표 (0..width, 0..height). 호스트는 frame 당 최대 1개로
    /// throttling — 빠른 마우스 이동에서는 마지막 sample만 plugin에 전달된다.
    CanvasPointer {
        node_id: String,
        x: f32,
        y: f32,
        phase: CanvasPointerPhase,
        /// 눌려있는 버튼 (Down/Drag에 의미). Move/Leave에선 보통 None.
        #[serde(default)]
        button: Option<CanvasPointerButton>,
    },
}

/// `UiEvent::CanvasPointer` 포인터 단계.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum CanvasPointerPhase {
    /// 버튼 없이 포인터 이동.
    Move,
    /// 버튼이 canvas 영역 안에서 눌림.
    Down,
    /// 버튼이 해제됨 (canvas 밖에서 해제되어도 forward).
    Up,
    /// Down 이후 버튼 유지 채 이동.
    Drag,
    /// 포인터가 canvas 영역을 벗어남.
    Leave,
}

/// `UiEvent::CanvasPointer` 버튼 종류.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum CanvasPointerButton {
    /// 주 버튼 (보통 왼쪽).
    Primary,
    /// 보조 버튼 (보통 오른쪽).
    Secondary,
    /// 가운데 버튼.
    Middle,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn label_basic_round_trip() {
        let n = UiNode::Label {
            text: "hello".into(),
            style: LabelStyle::Heading,
            color: Some("blue".into()),
        };
        let s = serde_json::to_string(&n).unwrap();
        assert!(s.contains("\"type\":\"label\""));
        assert!(s.contains("\"style\":\"heading\""));
        let parsed: UiNode = serde_json::from_str(&s).unwrap();
        assert_eq!(parsed, n);
    }

    #[test]
    fn vbox_with_children() {
        let n = UiNode::Vbox {
            spacing: 4,
            children: vec![
                UiNode::Label {
                    text: "A".into(),
                    style: LabelStyle::Body,
                    color: None,
                },
                UiNode::Spacer { size: 8 },
                UiNode::Button {
                    id: "btn".into(),
                    label: "Ok".into(),
                    enabled: true,
                    style: ButtonStyle::Primary,
                    block: true,
                    tooltip_i18n_key: None,
                },
            ],
        };
        let s = serde_json::to_string(&n).unwrap();
        let parsed: UiNode = serde_json::from_str(&s).unwrap();
        assert_eq!(parsed, n);
    }

    #[test]
    fn splitter_round_trip() {
        let n = UiNode::Splitter {
            direction: SplitDir::Horizontal,
            ratio: 0.35,
            first: Box::new(UiNode::Label {
                text: "L".into(),
                style: LabelStyle::Body,
                color: None,
            }),
            second: Box::new(UiNode::Label {
                text: "R".into(),
                style: LabelStyle::Body,
                color: None,
            }),
            id: None,
        };
        let s = serde_json::to_string(&n).unwrap();
        assert!(s.contains("\"direction\":\"horizontal\""));
        let parsed: UiNode = serde_json::from_str(&s).unwrap();
        assert_eq!(parsed, n);
    }

    #[test]
    fn center_round_trip() {
        let n = UiNode::Center {
            child: Box::new(UiNode::Label {
                text: "empty".into(),
                style: LabelStyle::Body,
                color: Some("subtext0".into()),
            }),
        };
        let s = serde_json::to_string(&n).unwrap();
        assert!(s.contains("\"type\":\"center\""));
        assert_eq!(serde_json::from_str::<UiNode>(&s).unwrap(), n);

        // 최소 JSON 디코드 — child 만 있으면 된다.
        let json = r#"{"type":"center","child":{"type":"label","text":"x"}}"#;
        match serde_json::from_str::<UiNode>(json).unwrap() {
            UiNode::Center { child } => match *child {
                UiNode::Label { text, .. } => assert_eq!(text, "x"),
                other => panic!("expected Label child, got {other:?}"),
            },
            other => panic!("expected Center, got {other:?}"),
        }
    }

    #[test]
    fn tree_round_trip() {
        let n = UiNode::Tree {
            id: "files".into(),
            nodes: vec![TreeNode {
                id: "root".into(),
                label: "/".into(),
                icon: Some("📁".into()),
                expanded: true,
                selected: false,
                children: vec![TreeNode {
                    id: "a".into(),
                    label: "a.rs".into(),
                    icon: Some("📄".into()),
                    expanded: false,
                    selected: true,
                    children: vec![],
                    has_children: false,
                }],
                has_children: true,
            }],
            selection_mode: SelectionMode::Single,
        };
        let s = serde_json::to_string(&n).unwrap();
        let parsed: UiNode = serde_json::from_str(&s).unwrap();
        assert_eq!(parsed, n);
    }

    #[test]
    fn label_mono_round_trip() {
        let n = UiNode::Label {
            text: "+ added".into(),
            style: LabelStyle::Mono,
            color: Some("green".into()),
        };
        let s = serde_json::to_string(&n).unwrap();
        assert!(s.contains("\"style\":\"mono\""));
        let parsed: UiNode = serde_json::from_str(&s).unwrap();
        assert_eq!(parsed, n);
    }

    #[test]
    fn selectable_row_round_trip() {
        let n = UiNode::SelectableRow {
            id: "row.0".into(),
            selected: true,
            children: vec![
                UiNode::Label {
                    text: " M ".into(),
                    style: LabelStyle::Mono,
                    color: Some("yellow".into()),
                },
                UiNode::Label {
                    text: "src/foo.rs".into(),
                    style: LabelStyle::Mono,
                    color: None,
                },
            ],
        };
        let s = serde_json::to_string(&n).unwrap();
        assert!(s.contains("\"type\":\"selectable_row\""));
        assert!(s.contains("\"selected\":true"));
        let parsed: UiNode = serde_json::from_str(&s).unwrap();
        assert_eq!(parsed, n);
    }

    #[test]
    fn button_block_defaults_false() {
        // block 을 생략한 최소 JSON 은 block=false 로 디코딩되어야 한다.
        let json = r#"{"type":"button","id":"b","label":"Ok"}"#;
        let parsed: UiNode = serde_json::from_str(json).unwrap();
        match parsed {
            UiNode::Button { block, enabled, .. } => {
                assert!(!block);
                assert!(enabled); // default_true
            }
            other => panic!("expected Button, got {other:?}"),
        }
        // block=true 는 round-trip.
        let n = UiNode::Button {
            id: "b".into(),
            label: "Ok".into(),
            enabled: true,
            style: ButtonStyle::Secondary,
            block: true,
            tooltip_i18n_key: None,
        };
        let s = serde_json::to_string(&n).unwrap();
        assert!(s.contains("\"block\":true"));
        assert_eq!(serde_json::from_str::<UiNode>(&s).unwrap(), n);
    }

    #[test]
    fn tag_round_trip() {
        let n = UiNode::Tag {
            text: "main".into(),
            tone: TagTone::Accent,
            dot: false,
        };
        let s = serde_json::to_string(&n).unwrap();
        assert!(s.contains("\"type\":\"tag\""));
        assert!(s.contains("\"tone\":\"accent\""));
        assert_eq!(serde_json::from_str::<UiNode>(&s).unwrap(), n);

        // tone/dot 생략 → default(Default tone, dot false).
        let json = r#"{"type":"tag","text":"x"}"#;
        match serde_json::from_str::<UiNode>(json).unwrap() {
            UiNode::Tag { tone, dot, .. } => {
                assert_eq!(tone, TagTone::Default);
                assert!(!dot);
            }
            other => panic!("expected Tag, got {other:?}"),
        }

        // dot=true 도 round-trip.
        let dotted = UiNode::Tag {
            text: " M ".into(),
            tone: TagTone::Warning,
            dot: true,
        };
        let s = serde_json::to_string(&dotted).unwrap();
        assert!(s.contains("\"dot\":true"));
        assert_eq!(serde_json::from_str::<UiNode>(&s).unwrap(), dotted);
    }

    #[test]
    fn badge_round_trip() {
        let n = UiNode::Badge {
            text: "3".into(),
            tone: BadgeTone::Primary,
            dot: false,
        };
        let s = serde_json::to_string(&n).unwrap();
        assert!(s.contains("\"type\":\"badge\""));
        assert!(s.contains("\"tone\":\"primary\""));
        assert_eq!(serde_json::from_str::<UiNode>(&s).unwrap(), n);

        // tone 생략 → default Danger.
        let json = r#"{"type":"badge","text":"9"}"#;
        match serde_json::from_str::<UiNode>(json).unwrap() {
            UiNode::Badge { tone, dot, .. } => {
                assert_eq!(tone, BadgeTone::Danger);
                assert!(!dot);
            }
            other => panic!("expected Badge, got {other:?}"),
        }

        let dotted = UiNode::Badge {
            text: String::new(),
            tone: BadgeTone::Success,
            dot: true,
        };
        let s = serde_json::to_string(&dotted).unwrap();
        assert!(s.contains("\"dot\":true"));
        assert_eq!(serde_json::from_str::<UiNode>(&s).unwrap(), dotted);
    }

    #[test]
    fn ui_event_click() {
        let ev = UiEvent::Click {
            node_id: "btn_open".into(),
        };
        let s = serde_json::to_string(&ev).unwrap();
        assert!(s.contains("\"kind\":\"click\""));
        let parsed: UiEvent = serde_json::from_str(&s).unwrap();
        assert_eq!(parsed, ev);
    }

    #[test]
    fn ui_event_tree_select() {
        let ev = UiEvent::TreeSelect {
            node_id: "files".into(),
            selected: vec!["root/a".into()],
        };
        let s = serde_json::to_string(&ev).unwrap();
        assert!(s.contains("\"kind\":\"tree_select\""));
        let parsed: UiEvent = serde_json::from_str(&s).unwrap();
        assert_eq!(parsed, ev);
    }

    #[test]
    fn canvas_round_trip_with_defaults() {
        // filter/commit_seq/id를 생략하면 wire에 빠지고, 디코딩 시 기본값(Linear / 0 / None)
        // 으로 채워진다.
        let n = UiNode::Canvas {
            buffer_id: SharedBufferId(7),
            width: 320,
            height: 200,
            format: PixelFormat::Rgba8,
            filter: PixelFilter::Linear,
            commit_seq: 0,
            id: None,
        };
        let s = serde_json::to_string(&n).unwrap();
        assert!(s.contains("\"type\":\"canvas\""));
        assert!(s.contains("\"format\":\"rgba8\""));
        let parsed: UiNode = serde_json::from_str(&s).unwrap();
        assert_eq!(parsed, n);

        // 명시적으로 filter/id를 지정해도 round-trip.
        let n2 = UiNode::Canvas {
            buffer_id: SharedBufferId(9),
            width: 16,
            height: 16,
            format: PixelFormat::Bgra8,
            filter: PixelFilter::Nearest,
            commit_seq: 42,
            id: Some("draw".into()),
        };
        let s2 = serde_json::to_string(&n2).unwrap();
        assert!(s2.contains("\"filter\":\"nearest\""));
        assert!(s2.contains("\"commit_seq\":42"));
        let parsed2: UiNode = serde_json::from_str(&s2).unwrap();
        assert_eq!(parsed2, n2);
    }

    #[test]
    fn canvas_minimal_json_decodes() {
        // plugin 쪽이 optional 필드를 생략한 최소 페이로드를 보내도 디코딩이 성공해야 한다.
        let json = r#"{
            "type": "canvas",
            "buffer_id": 11,
            "width": 64,
            "height": 64,
            "format": "rgba8"
        }"#;
        let parsed: UiNode = serde_json::from_str(json).unwrap();
        match parsed {
            UiNode::Canvas {
                buffer_id,
                width,
                height,
                format,
                filter,
                commit_seq,
                id,
            } => {
                assert_eq!(buffer_id.0, 11);
                assert_eq!(width, 64);
                assert_eq!(height, 64);
                assert_eq!(format, PixelFormat::Rgba8);
                assert_eq!(filter, PixelFilter::Linear);
                assert_eq!(commit_seq, 0);
                assert_eq!(id, None);
            }
            other => panic!("expected Canvas, got {:?}", other),
        }
    }

    #[test]
    fn ui_event_canvas_pointer_round_trip() {
        let ev = UiEvent::CanvasPointer {
            node_id: "draw".into(),
            x: 12.5,
            y: 7.0,
            phase: CanvasPointerPhase::Drag,
            button: Some(CanvasPointerButton::Primary),
        };
        let s = serde_json::to_string(&ev).unwrap();
        assert!(s.contains("\"kind\":\"canvas_pointer\""));
        assert!(s.contains("\"phase\":\"drag\""));
        assert!(s.contains("\"button\":\"primary\""));
        let parsed: UiEvent = serde_json::from_str(&s).unwrap();
        assert_eq!(parsed, ev);

        // button 생략(Move/Leave 등).
        let ev2 = UiEvent::CanvasPointer {
            node_id: "draw".into(),
            x: 0.0,
            y: 0.0,
            phase: CanvasPointerPhase::Move,
            button: None,
        };
        let s2 = serde_json::to_string(&ev2).unwrap();
        let parsed2: UiEvent = serde_json::from_str(&s2).unwrap();
        assert_eq!(parsed2, ev2);
    }
}
