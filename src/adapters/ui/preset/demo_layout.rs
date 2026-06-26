//! Preset 데모 레이아웃 미리보기 위젯 (본체, read-only — TODO 07 Phase 1).
//!
//! 저장된 `Preset*` 트리를 받아 **구조만** 축소 렌더한다 — pane split(상위) /
//! tab strip / surface split(하위) / surface leaf(kind 표시명)을 서로 다른 시각
//! weight 로 그려 계층이 라벨 없이 읽히게 한다. 라이브 surface 렌더(터미널 GPU
//! 패스 / WebView)는 **재사용하지 않는다** — 전용 placeholder 위젯이다.
//!
//! 갤러리 specimen `crates/tasty-gallery/src/catalog/components/preset_editor.rs`
//! 의 시각 구조를 본체 데이터·아이콘으로 1:1 대응시킨다. 차이점:
//!  - 입력이 정적 샘플이 아니라 실제 `WorkspacePreset`/`TabPreset`/`PanePreset`.
//!  - leaf 라벨을 주입된 resolver(런타임 kind→표시명)로 해석.
//!  - mini-tab 이 **live** — 클릭 시 미리보기의 active 탭만 바꾼다(저장본 불변).
//!
//! 3종 구조 레벨의 시각 weight (디자인 changelog 2026-06-25):
//!  - Pane split (상위) → 테두리 카드 + **5px bg-app gap** (무거운 divider).
//!  - Surface split (하위) → **1px border-default hairline** (가벼운 divider).
//!  - Surface leaf → kind 아이콘(accent) + 표시명(가운데, mono). 내용 렌더 안 함.
//!  - Mini tab strip → 20px, bg-sidebar. 활성 = bg-panel + 2px accent 하단 bar + kind 아이콘.

use tasty_presets::{
    PanePreset, PresetPane, PresetPaneNode, PresetSplitDirection, PresetSurface,
    PresetSurfaceLayout, PresetTab, TabPreset, WorkspacePreset,
};
use tasty_type_appearance::theme::Theme;
use tasty_ui_widgets::{Input, select};

use crate::adapters::ui::icons::{self, Icon};
use crate::i18n::t;

// 디자인 고정 px (Theme 에 대응 토큰 없는 preview 전용 치수 — specimen 과 동일).
/// 상위(pane) divider = bordered 카드 사이 bg-app 공백.
const PANE_GAP: f32 = 5.0;
/// mini tab strip height.
const STRIP_H: f32 = 20.0;
/// 활성 탭 본문 padding.
const BODY_PAD: f32 = 3.0;
/// surface leaf 아이콘↔라벨 gap.
const LEAF_GAP: f32 = 6.0;
/// mini tab 좌우 padding.
const TAB_PAD_X: f32 = 9.0;
/// mini tab 아이콘↔라벨 gap.
const TAB_GAP: f32 = 5.0;
/// 편집 모드 선택 핸들(split/remove) 한 변 크기.
const HANDLE_SZ: f32 = 18.0;
/// 핸들 사이 gap.
const HANDLE_GAP: f32 = 2.0;
/// 핸들 클러스터 모서리 inset.
const HANDLE_INSET: f32 = 4.0;
/// inline leaf form 최대 폭.
const FORM_MAX_W: f32 = 240.0;
/// inline leaf form 좌우 padding.
const FORM_PAD: f32 = 6.0;
/// inline leaf form 필드 세로 gap.
const FORM_GAP: f32 = 4.0;

/// 편집 시 kind 드롭다운 후보 — terminal(builtin) + 생성 가능한 등록 kind.
/// `empty`/`attached` 는 제외(capture/apply 정규화 정책과 정합). registry 미주입
/// 컨텍스트라 정적 목록을 쓰되, 현재 leaf 의 kind 가 목록에 없으면 런타임에 덧붙여
/// plugin/unknown kind 가 편집 중 유실되지 않게 한다(`kind_candidates`).
const EDIT_KINDS: &[&str] = &["terminal", "markdown", "image", "explorer", "html"];

/// 현재 kind 를 포함한 편집 후보 목록.
fn kind_candidates(current: &str) -> Vec<String> {
    let mut v: Vec<String> = EDIT_KINDS.iter().map(|s| s.to_string()).collect();
    if !v.iter().any(|k| k == current) {
        v.push(current.to_string());
    }
    v
}

// ── kind 시각 매핑 (아이콘 + accent) ────────────────────────────────────
//
// 표시명(label)은 registry/i18n 으로 해석하지만, *아이콘과 accent 색*은 본질적으로
// 시각 매핑이라 kind 문자열로 직접 결정한다(`tab_bar::kind_icon` 과 동일 idiom).
// 미지정 kind 는 중립(FILE + text-secondary)으로 떨어진다 — plugin/remote kind 안전.

fn kind_icon(kind: &str) -> Icon {
    match kind {
        "markdown" => icons::MD,
        "explorer" => icons::FOLDER,
        "image" => icons::IMAGE,
        "terminal" | "attached" => icons::TERM,
        _ => icons::FILE,
    }
}

fn kind_accent(theme: &Theme, kind: &str) -> egui::Color32 {
    match kind {
        "terminal" | "attached" => theme.accent_success().to_egui(),
        "markdown" => theme.accent_primary().to_egui(),
        "image" => theme.accent_info().to_egui(),
        "explorer" => theme.accent_agent().to_egui(),
        // 미지정 kind: accent 없이 중립(라벨과 같은 secondary).
        _ => theme.text_secondary().to_egui(),
    }
}

/// 레지스트리 없는 컨텍스트(현재 `PresetView` 윈도우는 `CoreState` 미접근)용
/// fallback kind→표시명 해석기.
///
/// `surface.kind.<kind>` i18n 키를 시도하고(= registry `display_name_i18n_key`
/// 규약과 동일 키. builtin/plugin 모두 이 네임스페이스를 쓴다), 미번역이면 kind
/// 첫 글자를 대문자로(`convert.rs::resolve_label` 의 capitalize fallback 패턴).
///
/// TODO 08(화면 통합)에서 `PresetView` 에 registry 가 주입되면, registry
/// `kinds_snapshot()`/`get()` 기반 resolver 로 교체할 자리.
pub fn fallback_kind_label(kind: &str) -> String {
    let key = format!("surface.kind.{kind}");
    let tr = t(&key);
    if tr != key.as_str() {
        return tr.to_string();
    }
    let mut c = kind.chars();
    match c.next() {
        None => String::new(),
        Some(f) => f.to_ascii_uppercase().to_string() + c.as_str(),
    }
}

// ── 정규화된 preview 모델 ───────────────────────────────────────────────
//
// 3종 preset(`Workspace`/`Tab`/`Pane`)을 공통 preview 모델로 정규화한다 — 위젯은
// 단일 타입만 받아 분기를 최소화한다(Codex 크로스체크 제안). 라벨은 build 시
// resolver 로 미리 해석해 둔다(렌더러는 registry/i18n 비의존).

/// surface leaf — kind 식별자 + 미리 해석된 표시명 + 편집용 round-trip 필드.
///
/// read-only 미리보기는 `kind`/`label` 만 쓰지만, 편집 모드가 트리를 모델로
/// 되돌려(`rebuild_*`) 디스크에 쓰려면 `cwd`/`startup`/`params` 를 손실 없이 보관해야
/// 한다. `id` 는 편집 선택/핸들 대상 지정용 안정 식별자(build 시 부여).
#[derive(Clone, Debug, PartialEq)]
struct Leaf {
    id: usize,
    kind: String,
    label: String,
    cwd: Option<String>,
    startup: Option<String>,
    params: serde_json::Value,
}

impl Leaf {
    /// mem::replace 자리채움용 빈 leaf (즉시 덮어써져 drop 됨).
    fn placeholder() -> Self {
        Leaf {
            id: 0,
            kind: String::new(),
            label: String::new(),
            cwd: None,
            startup: None,
            params: serde_json::Value::Null,
        }
    }
}

/// 하위 레이아웃(탭 안의 surface split).
#[derive(Clone, Debug, PartialEq)]
enum SurfNode {
    Leaf(Leaf),
    Split {
        row: bool,
        ratio: f32,
        first: Box<SurfNode>,
        second: Box<SurfNode>,
    },
}

impl SurfNode {
    /// 탭 대표 kind = 첫 leaf (디자인 `activeKind` — mini-tab 아이콘 구동).
    fn rep_kind(&self) -> &str {
        let mut n = self;
        loop {
            match n {
                SurfNode::Leaf(l) => return &l.kind,
                SurfNode::Split { first, .. } => n = first,
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
struct PreviewTab {
    /// 표시명 — `explicit_name` 우선, 없으면 대표 leaf 의 kind 표시명(자동).
    name: String,
    /// round-trip 용 사용자 지정 이름(자동 이름이면 None).
    explicit_name: Option<String>,
    layout: SurfNode,
}

#[derive(Clone, Debug, PartialEq)]
struct PreviewPane {
    /// 안정 식별자(build 시 부여) — 탭 클릭 상호작용 id + active override 키.
    id: usize,
    tabs: Vec<PreviewTab>,
    active: usize,
}

/// 상위 레이아웃(pane split).
#[derive(Clone, Debug, PartialEq)]
enum PaneNode {
    Leaf(PreviewPane),
    Split {
        row: bool,
        ratio: f32,
        first: Box<PaneNode>,
        second: Box<PaneNode>,
    },
}

/// scope variant — Workspace/Pane 은 pane 트리, Tab 은 단일 surface-split 프레임.
#[derive(Clone, Debug, PartialEq)]
enum Root {
    Panes(PaneNode),
    /// Tab scope: strip 없이 단일 탭 본문처럼 프레임.
    TabFrame(SurfNode),
}

/// 정규화된 preview 트리. 라이브 상호작용(active 탭)은 트리 안에 보관된다 —
/// 호출자가 프레임 간 인스턴스를 유지(`Clone`)하면 클릭 전환이 지속된다.
#[derive(Clone, Debug, PartialEq)]
pub struct DemoLayout {
    root: Root,
    /// 편집 중 새 노드에 부여할 다음 id (build 시 high-water mark + 1).
    next_id: usize,
}

/// build 동안 모든 노드(pane/leaf)에 0..N id 를 부여하는 카운터.
struct IdGen(usize);
impl IdGen {
    fn next(&mut self) -> usize {
        let id = self.0;
        self.0 += 1;
        id
    }
}

fn norm_surf(
    node: &PresetSurfaceLayout,
    resolve: &dyn Fn(&str) -> String,
    ids: &mut IdGen,
) -> SurfNode {
    match node {
        PresetSurfaceLayout::Leaf { surface } => SurfNode::Leaf(Leaf {
            id: ids.next(),
            kind: surface.kind.clone(),
            label: resolve(&surface.kind),
            cwd: surface.cwd.clone(),
            startup: surface.startup_command.clone(),
            params: surface.params.clone(),
        }),
        PresetSurfaceLayout::Split {
            direction,
            ratio,
            first,
            second,
        } => SurfNode::Split {
            row: is_row(*direction),
            ratio: *ratio,
            first: Box::new(norm_surf(first, resolve, ids)),
            second: Box::new(norm_surf(second, resolve, ids)),
        },
    }
}

fn norm_tab(tab: &PresetTab, resolve: &dyn Fn(&str) -> String, ids: &mut IdGen) -> PreviewTab {
    let layout = norm_surf(&tab.layout, resolve, ids);
    // explicit_name 우선, 없으면 대표 surface 의 표시명(디자인의 자동 탭 이름 규칙).
    let name = tab
        .explicit_name
        .clone()
        .unwrap_or_else(|| resolve(layout.rep_kind()));
    PreviewTab {
        name,
        explicit_name: tab.explicit_name.clone(),
        layout,
    }
}

fn norm_pane(pane: &PresetPane, resolve: &dyn Fn(&str) -> String, ids: &mut IdGen) -> PreviewPane {
    let id = ids.next();
    let tabs: Vec<PreviewTab> = pane
        .tabs
        .iter()
        .map(|t| norm_tab(t, resolve, ids))
        .collect();
    let active = pane.active_tab.min(tabs.len().saturating_sub(1));
    PreviewPane { id, tabs, active }
}

fn norm_pane_node(
    node: &PresetPaneNode,
    resolve: &dyn Fn(&str) -> String,
    ids: &mut IdGen,
) -> PaneNode {
    match node {
        PresetPaneNode::Leaf { pane } => PaneNode::Leaf(norm_pane(pane, resolve, ids)),
        PresetPaneNode::Split {
            direction,
            ratio,
            first,
            second,
        } => PaneNode::Split {
            row: is_row(*direction),
            ratio: *ratio,
            first: Box::new(norm_pane_node(first, resolve, ids)),
            second: Box::new(norm_pane_node(second, resolve, ids)),
        },
    }
}

/// 라이브 모델 의미(`tasty-type-geometry::SplitDirection`)와 동일하게:
/// `Vertical` = 폭 분할(좌우, row), `Horizontal` = 높이 분할(상하, column).
/// capture/apply 와 일치시켜 미리보기가 실제 적용 결과와 같은 방향으로 읽히게 한다.
fn is_row(d: PresetSplitDirection) -> bool {
    matches!(d, PresetSplitDirection::Vertical)
}

impl DemoLayout {
    pub fn from_workspace(p: &WorkspacePreset, resolve: impl Fn(&str) -> String) -> Self {
        let mut ids = IdGen(0);
        let root = Root::Panes(norm_pane_node(&p.layout, &resolve, &mut ids));
        Self {
            root,
            next_id: ids.0,
        }
    }

    pub fn from_tab(p: &TabPreset, resolve: impl Fn(&str) -> String) -> Self {
        let mut ids = IdGen(0);
        let root = Root::TabFrame(norm_surf(&p.tab.layout, &resolve, &mut ids));
        Self {
            root,
            next_id: ids.0,
        }
    }

    pub fn from_pane(p: &PanePreset, resolve: impl Fn(&str) -> String) -> Self {
        let mut ids = IdGen(0);
        let root = Root::Panes(PaneNode::Leaf(norm_pane(&p.pane, &resolve, &mut ids)));
        Self {
            root,
            next_id: ids.0,
        }
    }

    /// read-only 미리보기를 그리고 탭 클릭 상호작용을 처리한다.
    /// 탭 클릭으로 active 가 바뀌면 `true` 를 반환한다(호출자 repaint 신호).
    pub fn show(&mut self, ui: &mut egui::Ui, theme: &Theme, rect: egui::Rect) -> bool {
        let mut cx = DrawCtx {
            edit: false,
            sel: None,
            act: None,
        };
        self.draw(ui, theme, rect, &mut cx);
        match cx.act {
            Some(Act::SetActive { pane, idx }) => self.set_active(pane, idx),
            _ => false,
        }
    }

    /// 편집 모드로 그린다. 선택(`selected`)은 leaf id. 트리/필드/active 가 바뀌어
    /// 디스크 동기화가 필요하면 [`ShowOutcome::Mutated`], 선택만 바뀌면
    /// [`ShowOutcome::Repaint`] 를 반환한다.
    pub fn show_edit(
        &mut self,
        ui: &mut egui::Ui,
        theme: &Theme,
        rect: egui::Rect,
        selected: &mut Option<usize>,
    ) -> ShowOutcome {
        // 배경 클릭 = 선택 해제(specimen 의 onClick→setSel(null)). leaf/위젯 interact
        // 가 뒤에 추가되어 위에 얹히므로, 그것들을 누르면 bg.clicked() 는 false 가 된다.
        let bg = ui.interact(rect, ui.id().with("preset_demo_bg"), egui::Sense::click());

        let mut cx = DrawCtx {
            edit: true,
            sel: *selected,
            act: None,
        };
        self.draw(ui, theme, rect, &mut cx);

        let act = cx.act.or_else(|| bg.clicked().then_some(Act::Deselect));
        match act {
            None => ShowOutcome::None,
            Some(Act::Select(id)) => {
                *selected = Some(id);
                ShowOutcome::Repaint
            }
            Some(Act::Deselect) => {
                if selected.is_some() {
                    *selected = None;
                    ShowOutcome::Repaint
                } else {
                    ShowOutcome::None
                }
            }
            Some(Act::SetActive { pane, idx }) => {
                if self.set_active(pane, idx) {
                    ShowOutcome::Mutated
                } else {
                    ShowOutcome::None
                }
            }
            Some(Act::SetKind { id, kind }) => {
                self.set_kind(id, &kind);
                ShowOutcome::Mutated
            }
            Some(Act::SetField { id, cwd, value }) => {
                self.set_field(id, cwd, value);
                ShowOutcome::Mutated
            }
            Some(Act::Split { id, row }) => {
                self.split_leaf(id, row);
                ShowOutcome::Mutated
            }
            Some(Act::Remove { id }) => {
                if self.remove_leaf(id) {
                    if *selected == Some(id) {
                        *selected = None;
                    }
                    ShowOutcome::Mutated
                } else {
                    ShowOutcome::None
                }
            }
            Some(Act::AddTab { pane }) => {
                self.add_tab(pane);
                ShowOutcome::Mutated
            }
        }
    }

    fn draw(&self, ui: &mut egui::Ui, theme: &Theme, rect: egui::Rect, cx: &mut DrawCtx) {
        match &self.root {
            Root::Panes(node) => draw_pane_tree(ui, theme, rect, node, cx),
            Root::TabFrame(node) => draw_tab_frame(ui, theme, rect, node, cx),
        }
    }

    /// pane_id 의 active 탭을 idx 로 바꾼다. 실제로 변하면 true.
    fn set_active(&mut self, pane_id: usize, idx: usize) -> bool {
        for_each_pane_mut(&mut self.root, &mut |pane| {
            if pane.id == pane_id && idx < pane.tabs.len() && pane.active != idx {
                pane.active = idx;
                true
            } else {
                false
            }
        })
    }

    // ── 편집 mutation ────────────────────────────────────────────────────

    fn alloc_id(&mut self) -> usize {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    fn set_kind(&mut self, id: usize, kind: &str) {
        let label = fallback_kind_label(kind);
        for_each_surf_root_mut(&mut self.root, &mut |node| {
            if let Some(l) = find_leaf_mut(node, id) {
                l.kind = kind.to_string();
                l.label = label.clone();
            }
        });
        self.refresh_auto_names();
    }

    /// `cwd == true` 면 cwd, 아니면 startup 필드를 설정. 빈 문자열은 None.
    fn set_field(&mut self, id: usize, cwd: bool, value: String) {
        let v = if value.is_empty() { None } else { Some(value) };
        for_each_surf_root_mut(&mut self.root, &mut |node| {
            if let Some(l) = find_leaf_mut(node, id) {
                if cwd {
                    l.cwd = v.clone();
                } else {
                    l.startup = v.clone();
                }
            }
        });
    }

    fn split_leaf(&mut self, id: usize, row: bool) {
        let new_leaf = Leaf {
            id: self.alloc_id(),
            kind: "terminal".to_string(),
            label: fallback_kind_label("terminal"),
            cwd: None,
            startup: None,
            params: serde_json::Value::Null,
        };
        let mut slot = Some(new_leaf);
        for_each_surf_root_mut(&mut self.root, &mut |node| {
            split_node(node, id, row, &mut slot);
        });
        self.refresh_auto_names();
    }

    /// leaf 를 제거하고 부모 split 을 형제로 collapse. 단일 surface(루트 leaf)는
    /// 제거 불가 — 그 경우 false.
    fn remove_leaf(&mut self, id: usize) -> bool {
        let mut removed = false;
        for_each_surf_root_mut(&mut self.root, &mut |node| {
            if remove_node(node, id) {
                removed = true;
            }
        });
        if removed {
            self.refresh_auto_names();
        }
        removed
    }

    fn add_tab(&mut self, pane_id: usize) {
        let leaf_id = self.alloc_id();
        for_each_pane_mut(&mut self.root, &mut |pane| {
            if pane.id == pane_id {
                pane.tabs.push(PreviewTab {
                    name: fallback_kind_label("terminal"),
                    explicit_name: None,
                    layout: SurfNode::Leaf(Leaf {
                        id: leaf_id,
                        kind: "terminal".to_string(),
                        label: fallback_kind_label("terminal"),
                        cwd: None,
                        startup: None,
                        params: serde_json::Value::Null,
                    }),
                });
                pane.active = pane.tabs.len() - 1;
                true
            } else {
                false
            }
        });
    }

    /// 자동 이름(explicit_name 없는) 탭의 표시명을 대표 kind 로 갱신 — kind/구조
    /// 변경 후 mini-tab 라벨 live-update.
    fn refresh_auto_names(&mut self) {
        fn fix_pane(node: &mut PaneNode) {
            match node {
                PaneNode::Leaf(pane) => {
                    for t in &mut pane.tabs {
                        if t.explicit_name.is_none() {
                            t.name = fallback_kind_label(t.layout.rep_kind());
                        }
                    }
                }
                PaneNode::Split { first, second, .. } => {
                    fix_pane(first);
                    fix_pane(second);
                }
            }
        }
        if let Root::Panes(node) = &mut self.root {
            fix_pane(node);
        }
    }

    // ── 모델 reconstruction (편집 결과 → 디스크 저장용) ──────────────────

    /// Workspace/Pane scope 의 상위 pane 트리를 모델로 되돌린다. Tab scope 면 None.
    pub fn rebuild_pane_node(&self) -> Option<PresetPaneNode> {
        match &self.root {
            Root::Panes(n) => Some(pane_node_to_model(n)),
            Root::TabFrame(_) => None,
        }
    }

    /// Pane scope 의 단일 pane 을 모델로 되돌린다. 그 외 None.
    pub fn rebuild_single_pane(&self) -> Option<PresetPane> {
        match &self.root {
            Root::Panes(PaneNode::Leaf(p)) => Some(pane_to_model(p)),
            _ => None,
        }
    }

    /// Tab scope 의 surface 레이아웃을 모델로 되돌린다. 그 외 None.
    pub fn rebuild_surface_layout(&self) -> Option<PresetSurfaceLayout> {
        match &self.root {
            Root::TabFrame(s) => Some(surf_to_model(s)),
            _ => None,
        }
    }
}

/// [`DemoLayout::show_edit`] 결과 — 호출자 repaint/persist 분기용.
pub enum ShowOutcome {
    /// 변화 없음.
    None,
    /// 선택만 바뀜 — repaint 만, 저장 안 함.
    Repaint,
    /// 트리/필드/active 변경 — 디스크 동기화 필요.
    Mutated,
}

/// 한 프레임에 수집되는 편집 의도(specimen `applyAction` 의 액션).
enum Act {
    Select(usize),
    Deselect,
    SetActive { pane: usize, idx: usize },
    SetKind { id: usize, kind: String },
    SetField { id: usize, cwd: bool, value: String },
    Split { id: usize, row: bool },
    Remove { id: usize },
    AddTab { pane: usize },
}

/// draw 재귀에 전달되는 편집 컨텍스트.
struct DrawCtx {
    edit: bool,
    sel: Option<usize>,
    act: Option<Act>,
}

// ── 트리 워크 헬퍼 (편집) ───────────────────────────────────────────────

/// 모든 pane 을 순회하며 `f` 를 호출, 하나라도 true 면 true(첫 적용에서 멈추지 않음).
fn for_each_pane_mut(root: &mut Root, f: &mut impl FnMut(&mut PreviewPane) -> bool) -> bool {
    fn walk(node: &mut PaneNode, f: &mut impl FnMut(&mut PreviewPane) -> bool) -> bool {
        match node {
            PaneNode::Leaf(pane) => f(pane),
            PaneNode::Split { first, second, .. } => {
                let a = walk(first, f);
                let b = walk(second, f);
                a || b
            }
        }
    }
    match root {
        Root::Panes(node) => walk(node, f),
        Root::TabFrame(_) => false,
    }
}

/// 각 탭의 surface 레이아웃 루트(또는 TabFrame 루트)마다 `f` 를 호출.
fn for_each_surf_root_mut(root: &mut Root, f: &mut impl FnMut(&mut SurfNode)) {
    fn walk(node: &mut PaneNode, f: &mut impl FnMut(&mut SurfNode)) {
        match node {
            PaneNode::Leaf(pane) => {
                for t in &mut pane.tabs {
                    f(&mut t.layout);
                }
            }
            PaneNode::Split { first, second, .. } => {
                walk(first, f);
                walk(second, f);
            }
        }
    }
    match root {
        Root::Panes(node) => walk(node, f),
        Root::TabFrame(s) => f(s),
    }
}

fn find_leaf_mut(node: &mut SurfNode, id: usize) -> Option<&mut Leaf> {
    match node {
        SurfNode::Leaf(l) => (l.id == id).then_some(l),
        SurfNode::Split { first, second, .. } => {
            if let Some(l) = find_leaf_mut(first, id) {
                Some(l)
            } else {
                find_leaf_mut(second, id)
            }
        }
    }
}

/// id 의 leaf 를 split(leaf, new_leaf)로 교체. `slot` 에서 new_leaf 를 take.
fn split_node(node: &mut SurfNode, id: usize, row: bool, slot: &mut Option<Leaf>) -> bool {
    match node {
        SurfNode::Leaf(l) => {
            if l.id == id
                && let Some(nl) = slot.take()
            {
                let old = std::mem::replace(l, Leaf::placeholder());
                *node = SurfNode::Split {
                    row,
                    ratio: 0.5,
                    first: Box::new(SurfNode::Leaf(old)),
                    second: Box::new(SurfNode::Leaf(nl)),
                };
                return true;
            }
            false
        }
        SurfNode::Split { first, second, .. } => {
            split_node(first, id, row, slot) || split_node(second, id, row, slot)
        }
    }
}

/// id 의 leaf 를 제거하고 부모 split 을 형제로 collapse. 루트 leaf 면 false.
fn remove_node(node: &mut SurfNode, id: usize) -> bool {
    let SurfNode::Split { first, second, .. } = node else {
        return false;
    };
    let first_is = matches!(first.as_ref(), SurfNode::Leaf(l) if l.id == id);
    if first_is {
        let sibling = std::mem::replace(second.as_mut(), SurfNode::Leaf(Leaf::placeholder()));
        *node = sibling;
        return true;
    }
    let second_is = matches!(second.as_ref(), SurfNode::Leaf(l) if l.id == id);
    if second_is {
        let sibling = std::mem::replace(first.as_mut(), SurfNode::Leaf(Leaf::placeholder()));
        *node = sibling;
        return true;
    }
    remove_node(first, id) || remove_node(second, id)
}

// ── 모델 reconstruction ─────────────────────────────────────────────────

/// `row` → split 방향. is_row 의 역: row(좌우)=Vertical, column(상하)=Horizontal.
fn dir_from_row(row: bool) -> PresetSplitDirection {
    if row {
        PresetSplitDirection::Vertical
    } else {
        PresetSplitDirection::Horizontal
    }
}

fn surf_to_model(node: &SurfNode) -> PresetSurfaceLayout {
    match node {
        SurfNode::Leaf(l) => PresetSurfaceLayout::Leaf {
            surface: PresetSurface {
                kind: l.kind.clone(),
                cwd: l.cwd.clone(),
                startup_command: l.startup.clone(),
                params: l.params.clone(),
            },
        },
        SurfNode::Split {
            row,
            ratio,
            first,
            second,
        } => PresetSurfaceLayout::Split {
            direction: dir_from_row(*row),
            ratio: *ratio,
            first: Box::new(surf_to_model(first)),
            second: Box::new(surf_to_model(second)),
        },
    }
}

fn tab_to_model(t: &PreviewTab) -> PresetTab {
    PresetTab {
        explicit_name: t.explicit_name.clone(),
        layout: surf_to_model(&t.layout),
    }
}

fn pane_to_model(p: &PreviewPane) -> PresetPane {
    PresetPane {
        tabs: p.tabs.iter().map(tab_to_model).collect(),
        active_tab: p.active,
    }
}

fn pane_node_to_model(n: &PaneNode) -> PresetPaneNode {
    match n {
        PaneNode::Leaf(p) => PresetPaneNode::Leaf {
            pane: pane_to_model(p),
        },
        PaneNode::Split {
            row,
            ratio,
            first,
            second,
        } => PresetPaneNode::Split {
            direction: dir_from_row(*row),
            ratio: *ratio,
            first: Box::new(pane_node_to_model(first)),
            second: Box::new(pane_node_to_model(second)),
        },
    }
}

// ── rect 분할 헬퍼 (specimen 과 동일) ───────────────────────────────────

/// `rect` 를 비율로 나눈다. divider 만큼을 가운데 띠로 빼고 first/second 분배.
/// 반환 = (first, divider, second).
fn split_rects(
    rect: egui::Rect,
    row: bool,
    ratio: f32,
    divider: f32,
) -> (egui::Rect, egui::Rect, egui::Rect) {
    if row {
        let avail = (rect.width() - divider).max(0.0);
        let fw = avail * ratio;
        let first = egui::Rect::from_min_size(rect.min, egui::vec2(fw, rect.height()));
        let mid = egui::Rect::from_min_size(
            egui::pos2(rect.min.x + fw, rect.min.y),
            egui::vec2(divider, rect.height()),
        );
        let second = egui::Rect::from_min_size(
            egui::pos2(rect.min.x + fw + divider, rect.min.y),
            egui::vec2(avail - fw, rect.height()),
        );
        (first, mid, second)
    } else {
        let avail = (rect.height() - divider).max(0.0);
        let fh = avail * ratio;
        let first = egui::Rect::from_min_size(rect.min, egui::vec2(rect.width(), fh));
        let mid = egui::Rect::from_min_size(
            egui::pos2(rect.min.x, rect.min.y + fh),
            egui::vec2(rect.width(), divider),
        );
        let second = egui::Rect::from_min_size(
            egui::pos2(rect.min.x, rect.min.y + fh + divider),
            egui::vec2(rect.width(), avail - fh),
        );
        (first, mid, second)
    }
}

// ── 재귀 렌더 ───────────────────────────────────────────────────────────

/// 하위 레이아웃(surface split). Leaf = kind 박스, Split = 1px hairline.
fn draw_surf(
    ui: &mut egui::Ui,
    theme: &Theme,
    rect: egui::Rect,
    node: &SurfNode,
    cx: &mut DrawCtx,
) {
    match node {
        SurfNode::Leaf(l) => draw_surface_box(ui, theme, rect, l, cx),
        SurfNode::Split {
            row,
            ratio,
            first,
            second,
        } => {
            let (r1, line, r2) = split_rects(rect, *row, *ratio, theme.border_width.value());
            draw_surf(ui, theme, r1, first, cx);
            ui.painter_at(rect)
                .rect_filled(line, 0.0, theme.border_default().to_egui());
            draw_surf(ui, theme, r2, second, cx);
        }
    }
}

/// surface leaf — bg-app fill, 가운데 kind 아이콘(accent) + 표시명(mono, secondary).
/// 편집 모드: 1px separator inset outline(편집 가능 영역 표시), 선택 시 2px accent
/// inset outline + 핸들 클러스터(split-right/down/remove) + inline leaf form.
fn draw_surface_box(
    ui: &mut egui::Ui,
    theme: &Theme,
    rect: egui::Rect,
    leaf: &Leaf,
    cx: &mut DrawCtx,
) {
    let p = ui.painter_at(rect);
    p.rect_filled(rect, 0.0, theme.bg_app().to_egui());

    let selected = cx.edit && cx.sel == Some(leaf.id);

    // 편집 모드: 클릭 = 선택. (배경 deselect 보다 위에 얹혀 우선.)
    if cx.edit {
        let resp = ui.interact(
            rect,
            ui.id().with(("preset_leaf", leaf.id)),
            egui::Sense::click(),
        );
        if resp.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        if resp.clicked() && !selected {
            cx.act = Some(Act::Select(leaf.id));
        }
    }

    if selected {
        draw_leaf_form(ui, theme, rect, leaf, cx);
    } else {
        let icon = theme.icon_glyph_size_md.value();
        let label_h = theme.font_size_caption.value();
        let total = icon + LEAF_GAP + label_h;
        let icon_cy = rect.center().y - total * 0.5 + icon * 0.5;
        paint_icon(
            ui,
            kind_icon(&leaf.kind),
            egui::pos2(rect.center().x, icon_cy),
            icon,
            kind_accent(theme, &leaf.kind),
        );
        // painter_at 가 rect 로 clip 하므로 좁은 leaf 에서도 라벨이 넘치지 않는다.
        ui.painter_at(rect).text(
            egui::pos2(
                rect.center().x,
                icon_cy + icon * 0.5 + LEAF_GAP + label_h * 0.5,
            ),
            egui::Align2::CENTER_CENTER,
            &leaf.label,
            egui::FontId::monospace(label_h),
            theme.text_secondary().to_egui(),
        );
    }

    // outline: 선택=2px accent, 편집 일반=1px separator.
    if selected {
        let w = theme.tab_indicator_width.value();
        ui.painter_at(rect).rect_stroke(
            rect.shrink(w * 0.5),
            0.0,
            egui::Stroke::new(w, theme.accent_primary().to_egui()),
            egui::StrokeKind::Inside,
        );
    } else if cx.edit {
        let w = theme.border_width.value();
        ui.painter_at(rect).rect_stroke(
            rect,
            0.0,
            egui::Stroke::new(w, theme.separator.to_egui()),
            egui::StrokeKind::Inside,
        );
    }

    // 선택 leaf: 우상단 핸들 클러스터(split-right / split-down / remove).
    if selected {
        draw_handle_cluster(ui, theme, rect, leaf.id, cx);
    }
}

/// 우상단 핸들 클러스터 — split-right(row) / split-down(col) / remove(danger).
fn draw_handle_cluster(
    ui: &mut egui::Ui,
    theme: &Theme,
    rect: egui::Rect,
    leaf_id: usize,
    cx: &mut DrawCtx,
) {
    // 우상단 정렬, 시각 순서(좌→우): split-right · split-down · remove.
    let step = HANDLE_SZ + HANDLE_GAP;
    let x0 = rect.max.x - HANDLE_INSET - HANDLE_SZ * 3.0 - HANDLE_GAP * 2.0;
    let y = rect.min.y + HANDLE_INSET;
    let cell = |i: f32| {
        egui::Rect::from_min_size(
            egui::pos2(x0 + step * i, y),
            egui::vec2(HANDLE_SZ, HANDLE_SZ),
        )
    };
    let split_right = cell(0.0);
    let split_down = cell(1.0);
    let remove = cell(2.0);

    if mini_handle(ui, theme, split_right, icons::SPLIT, false, ("sr", leaf_id)) {
        cx.act = Some(Act::Split {
            id: leaf_id,
            row: true,
        });
    }
    if mini_handle(
        ui,
        theme,
        split_down,
        icons::SPLIT_DOWN,
        false,
        ("sd", leaf_id),
    ) {
        cx.act = Some(Act::Split {
            id: leaf_id,
            row: false,
        });
    }
    if mini_handle(ui, theme, remove, icons::TRASH, true, ("rm", leaf_id)) {
        cx.act = Some(Act::Remove { id: leaf_id });
    }
}

/// 작은 핸들 버튼. surface-raised bg + border-strong, danger 면 아이콘 danger.
fn mini_handle(
    ui: &mut egui::Ui,
    theme: &Theme,
    rect: egui::Rect,
    icon: Icon,
    danger: bool,
    salt: (&'static str, usize),
) -> bool {
    let resp = ui.interact(rect, ui.id().with(salt), egui::Sense::click());
    let radius = theme.corner_radius_sm.value();
    let bg = if resp.hovered() {
        theme.surface_active().to_egui()
    } else {
        theme.surface_raised().to_egui()
    };
    let p = ui.painter_at(rect);
    p.rect(
        rect,
        radius,
        bg,
        egui::Stroke::new(theme.border_width.value(), theme.border_strong().to_egui()),
        egui::StrokeKind::Inside,
    );
    let color = if danger {
        theme.accent_danger().to_egui()
    } else {
        theme.text_secondary().to_egui()
    };
    let glyph = HANDLE_SZ * 0.62;
    paint_icon(ui, icon, rect.center(), glyph, color);
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    resp.clicked()
}

/// inline leaf parameter editor — kind Select · cwd Input · startup Input(terminal 한정).
fn draw_leaf_form(
    ui: &mut egui::Ui,
    theme: &Theme,
    rect: egui::Rect,
    leaf: &Leaf,
    cx: &mut DrawCtx,
) {
    let inner_w = (rect.width() - FORM_PAD * 2.0).min(FORM_MAX_W).max(0.0);
    if inner_w < 1.0 {
        return;
    }
    // 핸들 클러스터(상단) 아래에서 시작.
    let top = rect.min.y + HANDLE_INSET * 2.0 + HANDLE_SZ;
    let form_rect = egui::Rect::from_min_max(
        egui::pos2(rect.center().x - inner_w * 0.5, top),
        egui::pos2(rect.center().x + inner_w * 0.5, rect.max.y - FORM_PAD),
    );
    if form_rect.height() < 1.0 {
        return;
    }

    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(form_rect)
            .layout(egui::Layout::top_down(egui::Align::Min)),
    );
    child.set_clip_rect(form_rect);
    child.spacing_mut().item_spacing.y = FORM_GAP;

    // kind Select.
    field_label(&mut child, theme, &t("preset.edit.kind"));
    let candidates = kind_candidates(&leaf.kind);
    let labels: Vec<String> = candidates.iter().map(|k| fallback_kind_label(k)).collect();
    let label_refs: Vec<&str> = labels.iter().map(|s| s.as_str()).collect();
    let mut sel_idx = candidates.iter().position(|k| *k == leaf.kind).unwrap_or(0);
    if select(
        &mut child,
        theme,
        &format!("preset_kind_{}", leaf.id),
        &mut sel_idx,
        &label_refs,
        inner_w,
        true,
    ) {
        cx.act = Some(Act::SetKind {
            id: leaf.id,
            kind: candidates[sel_idx].clone(),
        });
    }

    // cwd Input (mono).
    field_label(&mut child, theme, &t("preset.edit.cwd"));
    let mut cwd_buf = leaf.cwd.clone().unwrap_or_default();
    let cwd_resp = Input::new()
        .mono(true)
        .width(inner_w)
        .show(&mut child, theme, &mut cwd_buf);
    if cwd_resp.changed() {
        cx.act = Some(Act::SetField {
            id: leaf.id,
            cwd: true,
            value: cwd_buf,
        });
    }

    // startup Input (mono) — terminal 한정.
    if leaf.kind == "terminal" {
        field_label(&mut child, theme, &t("preset.edit.startup"));
        let mut su_buf = leaf.startup.clone().unwrap_or_default();
        let su_resp = Input::new()
            .mono(true)
            .width(inner_w)
            .placeholder(&t("preset.edit.startup_hint"))
            .show(&mut child, theme, &mut su_buf);
        if su_resp.changed() {
            cx.act = Some(Act::SetField {
                id: leaf.id,
                cwd: false,
                value: su_buf,
            });
        }
    }
}

/// form 필드 라벨 — mono micro, uppercase, muted.
fn field_label(ui: &mut egui::Ui, theme: &Theme, text: &str) {
    ui.label(
        egui::RichText::new(text.to_uppercase())
            .monospace()
            .size(theme.font_size_micro.value())
            .color(theme.text_muted().to_egui()),
    );
}

/// 상위 레이아웃(pane split). Leaf = pane 카드, Split = 5px bg-app gap.
fn draw_pane_tree(
    ui: &mut egui::Ui,
    theme: &Theme,
    rect: egui::Rect,
    node: &PaneNode,
    cx: &mut DrawCtx,
) {
    match node {
        PaneNode::Leaf(pane) => draw_pane_card(ui, theme, rect, pane, cx),
        PaneNode::Split {
            row,
            ratio,
            first,
            second,
        } => {
            // divider(PANE_GAP)는 칠하지 않는다 — bg-app 공백이 무거운 상위 divider.
            let (r1, _gap, r2) = split_rects(rect, *row, *ratio, PANE_GAP);
            draw_pane_tree(ui, theme, r1, first, cx);
            draw_pane_tree(ui, theme, r2, second, cx);
        }
    }
}

/// pane 카드 = 테두리 카드 + mini tab strip(클릭 가능) + 활성 탭의 surface 레이아웃.
fn draw_pane_card(
    ui: &mut egui::Ui,
    theme: &Theme,
    rect: egui::Rect,
    pane: &PreviewPane,
    cx: &mut DrawCtx,
) {
    let radius = theme.corner_radius.value();
    let bw = theme.border_width.value();
    let sep = theme.separator.to_egui();
    let p = ui.painter_at(rect);
    p.rect_filled(rect, radius, theme.bg_app().to_egui());

    // mini tab strip 배경.
    let strip = egui::Rect::from_min_size(rect.min, egui::vec2(rect.width(), STRIP_H));
    p.rect_filled(strip, 0.0, theme.bg_sidebar().to_egui());

    let tab_font = egui::FontId::proportional(theme.font_size_caption.value());
    let icon_sz = theme.icon_glyph_size_sm.value();
    let mut x = strip.min.x;
    for (i, t) in pane.tabs.iter().enumerate() {
        let on = i == pane.active;
        let lw = text_width(ui, &t.name, tab_font.clone());
        let tw = TAB_PAD_X + icon_sz + TAB_GAP + lw + TAB_PAD_X;
        let tab_rect =
            egui::Rect::from_min_size(egui::pos2(x, strip.min.y), egui::vec2(tw, STRIP_H));

        // 클릭 상호작용 — active 가 아닌 탭만 pointer + 클릭.
        let resp = ui.interact(
            tab_rect,
            ui.id().with(("preset_demo_tab", pane.id, i)),
            egui::Sense::click(),
        );
        if !on && resp.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        if resp.clicked() {
            cx.act = Some(Act::SetActive {
                pane: pane.id,
                idx: i,
            });
        }

        let rep = t.layout.rep_kind();
        let p = ui.painter_at(strip);
        if on {
            p.rect_filled(tab_rect, 0.0, theme.bg_panel().to_egui());
            // 2px accent 하단 bar.
            let bar = egui::Rect::from_min_size(
                egui::pos2(
                    tab_rect.min.x,
                    tab_rect.max.y - theme.tab_indicator_width.value(),
                ),
                egui::vec2(tw, theme.tab_indicator_width.value()),
            );
            p.rect_filled(bar, 0.0, theme.accent_primary().to_egui());
        }
        if i > 0 {
            // 탭 사이 separator(borderRight).
            p.vline(x, strip.y_range(), egui::Stroke::new(bw, sep));
        }
        let icon_c = egui::pos2(
            tab_rect.min.x + TAB_PAD_X + icon_sz * 0.5,
            tab_rect.center().y,
        );
        let icon_color = if on {
            kind_accent(theme, rep)
        } else {
            theme.text_muted().to_egui()
        };
        paint_icon(ui, kind_icon(rep), icon_c, icon_sz, icon_color);
        ui.painter_at(strip).text(
            egui::pos2(
                tab_rect.min.x + TAB_PAD_X + icon_sz + TAB_GAP,
                tab_rect.center().y,
            ),
            egui::Align2::LEFT_CENTER,
            &t.name,
            tab_font.clone(),
            if on {
                theme.text_primary().to_egui()
            } else {
                theme.text_muted().to_egui()
            },
        );
        x += tw;
    }

    // 편집 모드: strip 끝에 add-tab "+" 버튼.
    if cx.edit {
        let add =
            egui::Rect::from_min_size(egui::pos2(x, strip.min.y), egui::vec2(STRIP_H, STRIP_H));
        let resp = ui.interact(
            add,
            ui.id().with(("preset_demo_addtab", pane.id)),
            egui::Sense::click(),
        );
        let col = if resp.hovered() {
            theme.text_secondary().to_egui()
        } else {
            theme.text_muted().to_egui()
        };
        if resp.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        paint_icon(ui, icons::PLUS, add.center(), icon_sz, col);
        if resp.clicked() {
            cx.act = Some(Act::AddTab { pane: pane.id });
        }
    }

    // strip border-bottom.
    ui.painter_at(rect)
        .hline(strip.x_range(), strip.max.y, egui::Stroke::new(bw, sep));

    // 활성 탭 본문 — padding 3, bg-app.
    let body = egui::Rect::from_min_max(egui::pos2(rect.min.x, strip.max.y), rect.max);
    let inner = body.shrink(BODY_PAD);
    if let Some(t) = pane.tabs.get(pane.active).or_else(|| pane.tabs.first()) {
        draw_surf(ui, theme, inner, &t.layout, cx);
    }

    // 카드 외곽 border.
    ui.painter_at(rect).rect_stroke(
        rect,
        radius,
        egui::Stroke::new(bw, theme.border_default().to_egui()),
        egui::StrokeKind::Inside,
    );
}

/// Tab scope — strip 없이 단일 탭 본문처럼 프레임(테두리 + radius + padding 3).
fn draw_tab_frame(
    ui: &mut egui::Ui,
    theme: &Theme,
    rect: egui::Rect,
    node: &SurfNode,
    cx: &mut DrawCtx,
) {
    let radius = theme.corner_radius.value();
    let bw = theme.border_width.value();
    let p = ui.painter_at(rect);
    p.rect_filled(rect, radius, theme.bg_app().to_egui());
    draw_surf(ui, theme, rect.shrink(BODY_PAD), node, cx);
    ui.painter_at(rect).rect_stroke(
        rect,
        radius,
        egui::Stroke::new(bw, theme.border_default().to_egui()),
        egui::StrokeKind::Inside,
    );
}

fn paint_icon(ui: &mut egui::Ui, icon: Icon, center: egui::Pos2, size: f32, color: egui::Color32) {
    let r = egui::Rect::from_center_size(center, egui::vec2(size, size));
    icon.image(size, color).paint_at(ui, r);
}

fn text_width(ui: &egui::Ui, text: &str, font: egui::FontId) -> f32 {
    ui.fonts(|f| {
        f.layout_no_wrap(text.to_owned(), font, egui::Color32::PLACEHOLDER)
            .size()
            .x
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tasty_presets::{PresetPane, PresetSurface, PresetSurfaceLayout, PresetTab};

    fn surf(kind: &str) -> PresetSurfaceLayout {
        PresetSurfaceLayout::Leaf {
            surface: PresetSurface {
                kind: kind.into(),
                cwd: None,
                startup_command: None,
                params: serde_json::Value::Null,
            },
        }
    }

    fn ssplit(
        d: PresetSplitDirection,
        r: f32,
        a: PresetSurfaceLayout,
        b: PresetSurfaceLayout,
    ) -> PresetSurfaceLayout {
        PresetSurfaceLayout::Split {
            direction: d,
            ratio: r,
            first: Box::new(a),
            second: Box::new(b),
        }
    }

    /// 테스트용 resolver — registry 없이 kind 를 그대로 대문자 라벨로.
    fn up(kind: &str) -> String {
        let mut c = kind.chars();
        match c.next() {
            None => String::new(),
            Some(f) => f.to_ascii_uppercase().to_string() + c.as_str(),
        }
    }

    #[test]
    fn vertical_split_is_row_horizontal_is_column() {
        // 라이브 모델 의미와 일치: Vertical=좌우(row), Horizontal=상하(column).
        assert!(is_row(PresetSplitDirection::Vertical));
        assert!(!is_row(PresetSplitDirection::Horizontal));
    }

    #[test]
    fn normalizes_tab_preset_and_resolves_labels() {
        let p = TabPreset {
            name: "t".into(),
            tab: PresetTab {
                explicit_name: None,
                layout: ssplit(
                    PresetSplitDirection::Vertical,
                    0.5,
                    surf("terminal"),
                    surf("markdown"),
                ),
            },
        };
        let dl = DemoLayout::from_tab(&p, up);
        match &dl.root {
            Root::TabFrame(SurfNode::Split {
                row,
                ratio,
                first,
                second,
            }) => {
                assert!(*row);
                assert_eq!(*ratio, 0.5);
                assert!(matches!(first.as_ref(), SurfNode::Leaf(l) if l.label == "Terminal"));
                assert!(matches!(second.as_ref(), SurfNode::Leaf(l) if l.label == "Markdown"));
            }
            _ => panic!("expected TabFrame split"),
        }
    }

    #[test]
    fn tab_name_falls_back_to_rep_kind_label() {
        // explicit_name 없으면 대표(첫) leaf 의 표시명을 탭 이름으로.
        let pane = PresetPane {
            tabs: vec![PresetTab {
                explicit_name: None,
                layout: ssplit(
                    PresetSplitDirection::Horizontal,
                    0.5,
                    surf("markdown"),
                    surf("terminal"),
                ),
            }],
            active_tab: 0,
        };
        let p = PanePreset {
            name: "p".into(),
            pane,
        };
        let dl = DemoLayout::from_pane(&p, up);
        match &dl.root {
            Root::Panes(PaneNode::Leaf(pp)) => {
                assert_eq!(pp.tabs[0].name, "Markdown");
            }
            _ => panic!("expected single pane"),
        }
    }

    #[test]
    fn active_tab_is_clamped_to_range() {
        let pane = PresetPane {
            tabs: vec![
                PresetTab {
                    explicit_name: Some("a".into()),
                    layout: surf("terminal"),
                },
                PresetTab {
                    explicit_name: Some("b".into()),
                    layout: surf("terminal"),
                },
            ],
            active_tab: 9, // 범위 밖
        };
        let p = PanePreset {
            name: "p".into(),
            pane,
        };
        let dl = DemoLayout::from_pane(&p, up);
        match &dl.root {
            Root::Panes(PaneNode::Leaf(pp)) => assert_eq!(pp.active, 1),
            _ => panic!(),
        }
    }

    #[test]
    fn set_active_switches_only_on_real_change() {
        let pane = PresetPane {
            tabs: vec![
                PresetTab {
                    explicit_name: Some("a".into()),
                    layout: surf("terminal"),
                },
                PresetTab {
                    explicit_name: Some("b".into()),
                    layout: surf("markdown"),
                },
            ],
            active_tab: 0,
        };
        let p = PanePreset {
            name: "p".into(),
            pane,
        };
        let mut dl = DemoLayout::from_pane(&p, up);
        // pane id 0 (첫 pane). 0→1 변경 = true, 다시 1→1 = false.
        assert!(dl.set_active(0, 1));
        assert!(!dl.set_active(0, 1));
        // 존재하지 않는 pane id → false.
        assert!(!dl.set_active(99, 0));
        match &dl.root {
            Root::Panes(PaneNode::Leaf(pp)) => assert_eq!(pp.active, 1),
            _ => panic!(),
        }
    }

    /// 단일 leaf pane preset 빌더 (kind 지정).
    fn single_pane(kind: &str) -> PanePreset {
        PanePreset {
            name: "p".into(),
            pane: PresetPane {
                tabs: vec![PresetTab {
                    explicit_name: None,
                    layout: surf(kind),
                }],
                active_tab: 0,
            },
        }
    }

    /// surf 트리에서 leaf id 를 방문 순서로 수집.
    fn surf_leaf_ids(node: &SurfNode, out: &mut Vec<usize>) {
        match node {
            SurfNode::Leaf(l) => out.push(l.id),
            SurfNode::Split { first, second, .. } => {
                surf_leaf_ids(first, out);
                surf_leaf_ids(second, out);
            }
        }
    }

    fn first_surf(dl: &DemoLayout) -> &SurfNode {
        match &dl.root {
            Root::Panes(PaneNode::Leaf(p)) => &p.tabs[0].layout,
            _ => panic!("expected single pane"),
        }
    }

    #[test]
    fn set_kind_updates_leaf_and_label() {
        let mut dl = DemoLayout::from_pane(&single_pane("terminal"), up);
        let mut ids = Vec::new();
        surf_leaf_ids(first_surf(&dl), &mut ids);
        let leaf_id = ids[0];

        dl.set_kind(leaf_id, "markdown");
        match first_surf(&dl) {
            SurfNode::Leaf(l) => {
                assert_eq!(l.kind, "markdown");
                assert_eq!(l.label, "Markdown");
            }
            _ => panic!(),
        }
        // 모델 round-trip 에도 반영.
        let pane = dl.rebuild_single_pane().unwrap();
        match &pane.tabs[0].layout {
            PresetSurfaceLayout::Leaf { surface } => assert_eq!(surface.kind, "markdown"),
            _ => panic!(),
        }
    }

    #[test]
    fn split_leaf_creates_sibling() {
        let mut dl = DemoLayout::from_pane(&single_pane("terminal"), up);
        let mut ids = Vec::new();
        surf_leaf_ids(first_surf(&dl), &mut ids);
        let leaf_id = ids[0];

        dl.split_leaf(leaf_id, true);
        match first_surf(&dl) {
            SurfNode::Split {
                row, first, second, ..
            } => {
                assert!(*row);
                assert!(matches!(first.as_ref(), SurfNode::Leaf(_)));
                assert!(matches!(second.as_ref(), SurfNode::Leaf(_)));
            }
            _ => panic!("expected split after split_leaf"),
        }
        // 새 leaf 는 고유 id.
        let mut after = Vec::new();
        surf_leaf_ids(first_surf(&dl), &mut after);
        assert_eq!(after.len(), 2);
        assert_ne!(after[0], after[1]);
    }

    #[test]
    fn remove_leaf_collapses_parent_and_guards_sole() {
        let mut dl = DemoLayout::from_pane(&single_pane("terminal"), up);
        let mut ids = Vec::new();
        surf_leaf_ids(first_surf(&dl), &mut ids);
        let sole = ids[0];
        // 단일 surface 는 제거 불가.
        assert!(!dl.remove_leaf(sole));

        dl.split_leaf(sole, false);
        let mut after = Vec::new();
        surf_leaf_ids(first_surf(&dl), &mut after);
        assert_eq!(after.len(), 2);
        // 하나 제거 → 형제로 collapse.
        assert!(dl.remove_leaf(after[1]));
        let mut final_ids = Vec::new();
        surf_leaf_ids(first_surf(&dl), &mut final_ids);
        assert_eq!(final_ids, vec![after[0]]);
    }

    #[test]
    fn add_tab_appends_terminal_and_activates() {
        let mut dl = DemoLayout::from_pane(&single_pane("markdown"), up);
        let pane_id = match &dl.root {
            Root::Panes(PaneNode::Leaf(p)) => p.id,
            _ => panic!(),
        };
        dl.add_tab(pane_id);
        match &dl.root {
            Root::Panes(PaneNode::Leaf(p)) => {
                assert_eq!(p.tabs.len(), 2);
                assert_eq!(p.active, 1);
                assert_eq!(p.tabs[1].layout.rep_kind(), "terminal");
            }
            _ => panic!(),
        }
    }

    #[test]
    fn panes_get_unique_ids() {
        let p = WorkspacePreset {
            name: "w".into(),
            subtitle: String::new(),
            description: String::new(),
            layout: PresetPaneNode::Split {
                direction: PresetSplitDirection::Vertical,
                ratio: 0.5,
                first: Box::new(PresetPaneNode::Leaf {
                    pane: PresetPane {
                        tabs: vec![PresetTab {
                            explicit_name: Some("a".into()),
                            layout: surf("terminal"),
                        }],
                        active_tab: 0,
                    },
                }),
                second: Box::new(PresetPaneNode::Leaf {
                    pane: PresetPane {
                        tabs: vec![PresetTab {
                            explicit_name: Some("b".into()),
                            layout: surf("markdown"),
                        }],
                        active_tab: 0,
                    },
                }),
            },
        };
        let dl = DemoLayout::from_workspace(&p, up);
        let mut ids = Vec::new();
        fn collect(node: &PaneNode, ids: &mut Vec<usize>) {
            match node {
                PaneNode::Leaf(p) => ids.push(p.id),
                PaneNode::Split { first, second, .. } => {
                    collect(first, ids);
                    collect(second, ids);
                }
            }
        }
        if let Root::Panes(node) = &dl.root {
            collect(node, &mut ids);
        }
        // norm_pane 이 leaf 전에 pane id 를 소비하므로: pane0(id0)→leaf(id1),
        // pane1(id2)→leaf(id3). pane id 만 모으면 [0, 2].
        assert_eq!(ids, vec![0, 2]);
    }
}
