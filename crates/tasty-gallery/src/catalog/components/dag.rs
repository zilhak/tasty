//! Task DAG specimen — 디자인 `gallery/dag.jsx` 의 구조 전사.
//!
//! 부품 원본은 디자인 프로젝트의 `ui_kits/terminal/overlays/dag_view.jsx`
//! (상태 어휘 · 노드 카드 · 러너 배지 · 크롬 · 빈 상태) 와 `dag_surfaces.jsx`
//! (캔버스 · 노드 상세 · 풀탭 서피스) 다. 이 모듈은 그 jsx 의 레이아웃 구조를
//! egui 로 1:1 옮긴 것이고, 본체(`tasty` bin)의 `surface/dag_graph/` 를 호출하지
//! 않는다 — 갤러리는 본체 바이너리에 의존할 수 없어 **전사 미러**로 만든다
//! (`remote_tool` / `switch_overlay` 와 같은 선례, `docs/design/systems/
//! design-gallery-mapping.md`).
//!
//! # 좌표는 실제 엔진에서 온다
//!
//! 노드 배치만은 미러가 아니라 **본체와 같은 코드**다 — `tasty-dag-layout` 은
//! egui/Theme 를 모르는 순수 계산 crate 라 갤러리가 그대로 의존할 수 있다.
//! 디자인 jsx 의 `dagLayout()` 은 시안용 최단 구현(longest-path + 중앙정렬)이라
//! 좌표가 sugiyama 결과와 다르다. 갤러리는 **본체가 실제로 그리는 좌표**를 보여야
//! 하므로 엔진 쪽을 따른다.
//!
//! # 글리프 치환 (디자인 대비 의도적 차이)
//!
//! 디자인의 상태 글리프 중 `❯`(U+276F) `✓`(U+2713) `✗`(U+2717) 는 유니코드
//! Dingbats 블록이라 UI 비례 폰트에서 tofu 로 떨어진다. 본체는 같은 이유로
//! 기하 도형(`▷ ● ×` 등)으로 치환했고 `tests/design_token_adherence.rs` 의
//! `no_raw_pictographic_glyph` 게이트가 그 블록을 host UI 소스에서 금지한다.
//! 갤러리도 **본체와 같은 치환 세트**를 쓴다 — 렌더되지 않는 글자를 전시하면
//! 정합 판정 자체가 무의미해지기 때문이다.

pub mod canvas;
pub mod chrome;
pub mod detail;
pub mod edges;
pub mod node;
pub mod rows;
pub mod runner;
pub mod states;
pub mod surface;
pub mod window;

use tasty_dag_layout::{GraphLayout, LayoutConfig, Orientation, layout_dag};
use tasty_icons::Icon;
use tasty_type_appearance::color::HexColor;
use tasty_type_appearance::theme::Theme;

/// 실행 상태 8 종 (`DAG_STATUS` / `DAG_STATUS_ORDER`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Waiting,
    Ready,
    Running,
    Succeeded,
    Failed,
    Cancelled,
    Skipped,
    Unknown,
}

/// 디자인 `DAG_STATUS_ORDER` 와 같은 순서.
pub const STATUS_ORDER: [Status; 8] = [
    Status::Waiting,
    Status::Ready,
    Status::Running,
    Status::Succeeded,
    Status::Failed,
    Status::Cancelled,
    Status::Skipped,
    Status::Unknown,
];

/// DAG **rollup** 어휘 6 종 — [`STATUS_ORDER`] 에서 `Cancelled` 와 `Unknown` 만 빠진 것.
///
/// 개별 노드는 8 종 전부가 될 수 있지만, DAG 하나의 대표 상태를 뽑는 호스트의 rollup
/// 은 waiting / ready / running / succeeded / failed / skipped 여섯만 낸다 —
/// cancelled 가 섞인 DAG 는 skipped 로, unknown 이 남은 DAG 는 waiting 으로 접힌다.
/// 그래서 rollup 값을 비교하는 목록 상태 필터는 이 6 종만 나열한다(8 종을 나열하면
/// 어떤 DAG 와도 일치하지 않는 죽은 선택지가 둘 생긴다). 노드 범례처럼 어휘 전체가
/// 필요한 곳은 계속 [`STATUS_ORDER`] 를 쓴다.
pub const ROLLUP_ORDER: [Status; 6] = [
    Status::Waiting,
    Status::Ready,
    Status::Running,
    Status::Succeeded,
    Status::Failed,
    Status::Skipped,
];

impl Status {
    /// 토큰 접미사 (`--tasty-dag-status-<key>`).
    pub fn key(self) -> &'static str {
        match self {
            Status::Waiting => "waiting",
            Status::Ready => "ready",
            Status::Running => "running",
            Status::Succeeded => "succeeded",
            Status::Failed => "failed",
            Status::Cancelled => "cancelled",
            Status::Skipped => "skipped",
            Status::Unknown => "unknown",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Status::Waiting => "Waiting",
            Status::Ready => "Ready",
            Status::Running => "Running",
            Status::Succeeded => "Succeeded",
            Status::Failed => "Failed",
            Status::Cancelled => "Cancelled",
            Status::Skipped => "Skipped",
            Status::Unknown => "Unknown",
        }
    }

    /// 색이 아닌 두 번째 채널. 모듈 문서의 "글리프 치환" 참고.
    pub fn glyph(self) -> &'static str {
        match self {
            Status::Waiting => "\u{25E6}",   // ◦
            Status::Ready => "\u{25B7}",     // ▷
            Status::Running => "\u{25D1}",   // ◑
            Status::Succeeded => "\u{25CF}", // ●
            Status::Failed => "\u{00D7}",    // ×
            Status::Cancelled => "\u{2298}", // ⊘
            Status::Skipped => "\u{25C7}",   // ◇
            Status::Unknown => "?",
        }
    }

    /// 상태 accent — 좌측 바 · 미니맵 · (대개) 카드 보더.
    pub fn accent(self, theme: &Theme) -> HexColor {
        match self {
            Status::Waiting => theme.dag_status_waiting(),
            Status::Ready => theme.dag_status_ready(),
            Status::Running => theme.dag_status_running(),
            Status::Succeeded => theme.dag_status_succeeded(),
            Status::Failed => theme.dag_status_failed(),
            Status::Cancelled => theme.dag_status_cancelled(),
            Status::Skipped => theme.dag_status_skipped(),
            Status::Unknown => theme.dag_status_unknown(),
        }
    }

    /// 카드 바탕 (`sBg`).
    pub fn bg(self, theme: &Theme) -> HexColor {
        match self {
            Status::Waiting => theme.dag_status_waiting_bg(),
            Status::Ready => theme.dag_status_ready_bg(),
            Status::Running => theme.dag_status_running_bg(),
            Status::Succeeded => theme.dag_status_succeeded_bg(),
            Status::Failed => theme.dag_status_failed_bg(),
            Status::Cancelled => theme.dag_status_cancelled_bg(),
            Status::Skipped => theme.dag_status_skipped_bg(),
            Status::Unknown => theme.dag_status_unknown_bg(),
        }
    }

    /// 철자 라벨 톤 (`sLabel`) — 10px 에서도 읽히도록 상태별로 따로 잡힌 역할.
    pub fn label_fg(self, theme: &Theme) -> HexColor {
        match self {
            Status::Waiting => theme.dag_status_waiting_label(),
            Status::Ready => theme.dag_status_ready_label(),
            Status::Running => theme.dag_status_running_label(),
            Status::Succeeded => theme.dag_status_succeeded_label(),
            Status::Failed => theme.dag_status_failed_label(),
            Status::Cancelled => theme.dag_status_cancelled_label(),
            Status::Skipped => theme.dag_status_skipped_label(),
            Status::Unknown => theme.dag_status_unknown_label(),
        }
    }

    /// 카드 보더. 디자인은 waiting / cancelled / skipped 만 중립 보더를 쓰고
    /// 나머지는 상태 accent 를 그대로 두른다.
    pub fn border(self, theme: &Theme) -> HexColor {
        if matches!(self, Status::Waiting | Status::Cancelled | Status::Skipped) {
            theme.dag_node_border()
        } else {
            self.accent(theme)
        }
    }

    /// 실패/취소 상류에 막혀 실행되지 않을 경로 (`DIM_STATUS`).
    pub fn is_dim(self) -> bool {
        matches!(self, Status::Skipped | Status::Cancelled)
    }
}

/// task 종류 4 종 (`DAG_KIND`) — 이름 행 앞의 글리프로만 구분된다.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Run,
    Custom,
    Reduce,
    WaitBarrier,
}

impl Kind {
    pub fn icon(self) -> Icon {
        match self {
            Kind::Run => tasty_icons::TERMINAL,
            Kind::Custom => tasty_icons::PLUG,
            Kind::Reduce => tasty_icons::LAYERS,
            Kind::WaitBarrier => tasty_icons::LOCK,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Kind::Run => "run",
            Kind::Custom => "custom",
            Kind::Reduce => "reduce",
            Kind::WaitBarrier => "barrier",
        }
    }

    /// 명세 키 (`run` / `custom` / `reduce` / `wait_barrier`).
    pub fn key(self) -> &'static str {
        match self {
            Kind::WaitBarrier => "wait_barrier",
            other => other.label(),
        }
    }
}

/// 의존 관계 3 종 (`DAG_REL`) — 색과 파선 패턴을 **함께** 써서 구분한다.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rel {
    DependsOn,
    Fallback,
    Reduce,
}

impl Rel {
    pub fn key(self) -> &'static str {
        match self {
            Rel::DependsOn => "depends_on",
            Rel::Fallback => "fallback",
            Rel::Reduce => "reduce",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Rel::DependsOn => "depends on",
            Rel::Fallback => "fallback",
            Rel::Reduce => "reduce",
        }
    }

    pub fn color(self, theme: &Theme) -> HexColor {
        match self {
            Rel::DependsOn => theme.dag_edge_depends(),
            Rel::Fallback => theme.dag_edge_fallback(),
            Rel::Reduce => theme.dag_edge_reduce(),
        }
    }

    /// `strokeDasharray` — `(on, off)` 길이. 실선이면 `None`.
    pub fn dash(self) -> Option<(f32, f32)> {
        match self {
            Rel::DependsOn => None,
            Rel::Fallback => Some((6.0, 3.0)),
            Rel::Reduce => Some((2.0, 3.0)),
        }
    }
}

/// 카드 한 장이 표현하는 task.
#[derive(Debug, Clone)]
pub struct Node {
    pub id: String,
    pub name: String,
    pub kind: Kind,
    pub status: Status,
    pub dur: Option<String>,
    pub started: Option<String>,
    pub exit: Option<i32>,
    pub cmd: String,
    pub err: Option<String>,
    pub deps: Vec<(String, Rel)>,
}

impl Node {
    fn new(id: &str, name: &str, kind: Kind, status: Status, cmd: &str) -> Self {
        Self {
            id: id.to_owned(),
            name: name.to_owned(),
            kind,
            status,
            dur: None,
            started: None,
            exit: None,
            cmd: cmd.to_owned(),
            err: None,
            deps: Vec::new(),
        }
    }

    fn ran(mut self, dur: &str, started: &str, exit: i32) -> Self {
        self.dur = Some(dur.to_owned());
        self.started = Some(started.to_owned());
        self.exit = Some(exit);
        self
    }

    fn running_for(mut self, dur: &str, started: &str) -> Self {
        self.dur = Some(dur.to_owned());
        self.started = Some(started.to_owned());
        self
    }

    fn dep(mut self, from: &str, rel: Rel) -> Self {
        self.deps.push((from.to_owned(), rel));
        self
    }

    fn err(mut self, tail: &str) -> Self {
        self.err = Some(tail.to_owned());
        self
    }
}

/// 호스트 러너의 생사 + 대기/실행 카운트.
#[derive(Debug, Clone, Copy)]
pub struct Runner {
    pub running: bool,
    pub crashed: bool,
    pub ready: u32,
    pub active: u32,
}

/// DAG 한 개.
#[derive(Debug, Clone)]
pub struct Graph {
    pub id: String,
    pub name: String,
    pub workspace: String,
    pub updated: String,
    /// 사이클을 이루는 task id 들. 있으면 캔버스 상단에 배너가 고정된다.
    pub cycle: Option<Vec<String>>,
    pub nodes: Vec<Node>,
    pub runner: Runner,
}

impl Graph {
    pub fn index_of(&self, id: &str) -> Option<usize> {
        self.nodes.iter().position(|n| n.id == id)
    }

    pub fn node(&self, id: &str) -> Option<&Node> {
        self.nodes.iter().find(|n| n.id == id)
    }

    /// `(from_idx, to_idx, rel)` — 레이어가 증가하는 방향(의존 대상 → 의존하는 쪽).
    pub fn edges(&self) -> Vec<(usize, usize, Rel)> {
        let mut out = Vec::new();
        for (to, n) in self.nodes.iter().enumerate() {
            for (from, rel) in &n.deps {
                if let Some(from) = self.index_of(from) {
                    out.push((from, to, *rel));
                }
            }
        }
        out
    }

    /// 엣지가 죽은 경로인지 — 도착이 skipped/cancelled 이거나 출발이 실패/skipped/cancelled.
    pub fn edge_is_dim(&self, from: usize, to: usize) -> bool {
        let src = self.nodes[from].status;
        let dst = self.nodes[to].status;
        dst.is_dim() || src == Status::Failed || src.is_dim()
    }
}

/// 레이아웃 설정을 토큰에서 만든다 — 본체와 같은 치수를 같은 엔진에 먹인다.
pub fn layout_config(theme: &Theme, dir: Orientation) -> LayoutConfig {
    LayoutConfig {
        orientation: dir,
        node_size: (theme.dag_node_width(), theme.dag_node_height()),
        layer_gap: theme.dag_layer_gap(),
        sibling_gap: theme.dag_sibling_gap(),
        ..LayoutConfig::default()
    }
}

/// 그래프 좌표 — `tasty-dag-layout` 실엔진.
pub fn layout(graph: &Graph, theme: &Theme, dir: Orientation) -> GraphLayout {
    let ids: Vec<String> = graph.nodes.iter().map(|n| n.id.clone()).collect();
    let edges: Vec<(usize, usize)> = graph.edges().iter().map(|(f, t, _)| (*f, *t)).collect();
    layout_dag(&ids, &edges, &layout_config(theme, dir))
}

const ERR_TAIL: &str = "error: linking with `cc` failed: exit status: 1\n  \
= note: /usr/bin/ld: cannot find -lssl: No such file or directory\n          \
/usr/bin/ld: cannot find -lcrypto: No such file or directory\n          \
collect2: error: ld returned 1 exit status\n\n\
error: could not compile `tasty-host` (bin \"tasty-host\") due to 1 previous error\n\
warning: build failed, waiting for other jobs to finish...\n\
make: *** [Makefile:42: release] Error 101";

/// 디자인 `DAG_BUILD` — 12 노드, 8 상태 전부와 3 관계 전부가 한 번씩 나온다.
pub fn build_dag() -> Graph {
    use Kind::*;
    use Rel::*;
    use Status::*;
    Graph {
        id: "build-and-deploy".into(),
        name: "build-and-deploy".into(),
        workspace: "tasty".into(),
        updated: "12s ago".into(),
        cycle: None,
        nodes: vec![
            Node::new(
                "checkout",
                "checkout",
                Run,
                Succeeded,
                "git fetch --depth 1 && git checkout $SHA",
            )
            .ran("1.2s", "10:31:02", 0),
            Node::new(
                "install",
                "deps:install",
                Run,
                Succeeded,
                "cargo fetch --locked",
            )
            .ran("24s", "10:31:04", 0)
            .dep("checkout", DependsOn),
            Node::new(
                "lint",
                "lint:clippy",
                Run,
                Succeeded,
                "cargo clippy -- -D warnings",
            )
            .ran("8s", "10:31:28", 0)
            .dep("install", DependsOn),
            Node::new("unit", "test:unit", Run, Running, "cargo test --lib")
                .running_for("12s", "10:31:28")
                .dep("install", DependsOn),
            Node::new("e2e", "test:e2e", Run, Ready, "cargo test --test e2e")
                .dep("install", DependsOn),
            Node::new(
                "build_linux",
                "build:linux-x86_64",
                Run,
                Failed,
                "cargo build --release --target x86_64-unknown-linux-gnu",
            )
            .ran("41s", "10:31:29", 101)
            .err(ERR_TAIL)
            .dep("install", DependsOn),
            Node::new(
                "build_retry",
                "build:linux (musl fallback)",
                Run,
                Succeeded,
                "cargo build --release --target x86_64-unknown-linux-musl",
            )
            .ran("58s", "10:32:11", 0)
            .dep("build_linux", Fallback),
            Node::new(
                "build_mac",
                "build:macos-arm64",
                Run,
                Waiting,
                "cargo build --release --target aarch64-apple-darwin",
            )
            .dep("unit", DependsOn),
            Node::new(
                "sign",
                "sign:artifacts",
                Custom,
                Skipped,
                "ipc: signer.sign(artifacts)",
            )
            .dep("build_linux", DependsOn),
            Node::new(
                "package",
                "package:release",
                Kind::Reduce,
                Cancelled,
                "reduce: collect(build_retry, build_mac)",
            )
            .dep("build_retry", Rel::Reduce)
            .dep("build_mac", Rel::Reduce),
            Node::new(
                "gate",
                "publish:gate",
                WaitBarrier,
                Waiting,
                "barrier: await approval",
            )
            .dep("package", DependsOn),
            Node::new(
                "notify",
                "notify:agents",
                Custom,
                Unknown,
                "ipc: agents.broadcast(release)",
            )
            .dep("gate", DependsOn),
        ],
        runner: Runner {
            running: true,
            crashed: false,
            ready: 1,
            active: 1,
        },
    }
}

/// 디자인 `DAG_INDEX` — 5 노드 소형 그래프.
pub fn index_dag() -> Graph {
    use Kind::*;
    use Rel::*;
    use Status::*;
    Graph {
        id: "index-refresh".into(),
        name: "index-refresh".into(),
        workspace: "tasty-docs".into(),
        updated: "2m ago".into(),
        cycle: None,
        nodes: vec![
            Node::new("scan", "scan:sources", Run, Succeeded, "rg --files docs/")
                .ran("3s", "10:22:40", 0),
            Node::new("a", "chunk:a-m", Run, Succeeded, "index chunk a-m")
                .ran("11s", "10:22:43", 0)
                .dep("scan", DependsOn),
            Node::new("b", "chunk:n-z", Run, Running, "index chunk n-z")
                .running_for("9s", "10:22:43")
                .dep("scan", DependsOn),
            Node::new(
                "merge",
                "merge:index",
                Kind::Reduce,
                Waiting,
                "reduce: merge(a, b)",
            )
            .dep("a", Rel::Reduce)
            .dep("b", Rel::Reduce),
            Node::new("swap", "swap:live", Custom, Waiting, "ipc: index.swap()")
                .dep("merge", DependsOn),
        ],
        runner: Runner {
            running: true,
            crashed: false,
            ready: 0,
            active: 1,
        },
    }
}

/// 디자인 `DAG_CYCLE` — 러너는 거부하지만 서피스는 그대로 그린다.
pub fn cycle_dag() -> Graph {
    use Kind::*;
    use Rel::*;
    use Status::*;
    Graph {
        id: "release-notes".into(),
        name: "release-notes".into(),
        workspace: "tasty".into(),
        updated: "just now".into(),
        cycle: Some(vec!["draft".into(), "review".into(), "revise".into()]),
        nodes: vec![
            Node::new(
                "collect",
                "collect:commits",
                Run,
                Succeeded,
                "git log --oneline",
            )
            .ran("2s", "09:58:10", 0),
            Node::new(
                "draft",
                "draft:notes",
                Custom,
                Unknown,
                "ipc: writer.draft()",
            )
            .dep("collect", DependsOn)
            .dep("revise", DependsOn),
            Node::new(
                "review",
                "review:notes",
                Custom,
                Waiting,
                "ipc: reviewer.review()",
            )
            .dep("draft", DependsOn),
            Node::new(
                "revise",
                "revise:notes",
                Custom,
                Waiting,
                "ipc: writer.revise()",
            )
            .dep("review", DependsOn),
        ],
        runner: Runner {
            running: false,
            crashed: false,
            ready: 2,
            active: 0,
        },
    }
}

/// 디자인 `DAG_DENSE` — 1 + 6 레이어 × 9 = 55 노드. LOD 하위 티어 검증용.
pub fn dense_dag() -> Graph {
    const POOL: [Status; 10] = [
        Status::Succeeded,
        Status::Succeeded,
        Status::Succeeded,
        Status::Running,
        Status::Ready,
        Status::Waiting,
        Status::Failed,
        Status::Skipped,
        Status::Cancelled,
        Status::Unknown,
    ];
    let mut nodes = vec![
        Node::new("n0", "fan:root", Kind::Run, Status::Succeeded, "fan out")
            .ran("1s", "10:00:00", 0),
    ];
    for layer in 1..=6usize {
        for i in 0..9usize {
            let parent = if layer == 1 {
                "n0".to_owned()
            } else {
                format!("n{}_{}", layer - 1, (i + (i % 2)) % 9)
            };
            nodes.push(
                Node::new(
                    &format!("n{layer}_{i}"),
                    &format!("task:{layer}-{i}"),
                    Kind::Run,
                    POOL[(layer * 3 + i) % POOL.len()],
                    &format!("run step {layer}.{i}"),
                )
                .ran(&format!("{}s", layer + i), &format!("10:0{layer}:0{i}"), 0)
                .dep(&parent, Rel::DependsOn),
            );
        }
    }
    Graph {
        id: "wide-fanout".into(),
        name: "wide-fanout".into(),
        workspace: "tasty".into(),
        updated: "4s ago".into(),
        cycle: None,
        nodes,
        runner: Runner {
            running: true,
            crashed: false,
            ready: 6,
            active: 4,
        },
    }
}
