//! 화면이 소비하는 DAG 데이터 형태 + `Task` 레코드로부터의 변환.
//!
//! 렌더 코드가 `tasty_agent::Task` 를 직접 읽지 않게 하는 경계다. 캔버스는 여기서
//! 만든 [`DagData`] 만 보므로, task 스키마가 바뀌어도 파장이 이 파일에서 멈춘다.

use tasty_agent::{DagSummary, Task, TaskCommand, TaskState};

use crate::core::agent::graph_view::{collect_graph_edges, on_failure_kind, task_command_kind};
use crate::i18n::t;

/// 노드 상태 8종. 색·글리프·라벨 3 채널을 모두 여기서 결정한다 — 색 단독으로
/// 상태를 표현하지 않는다는 접근성 규칙의 집행 지점이다.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DagStatus {
    Waiting,
    Ready,
    Running,
    Succeeded,
    Failed,
    Cancelled,
    Skipped,
    Unknown,
}

impl DagStatus {
    pub fn from_state(state: &TaskState) -> Self {
        match state {
            TaskState::Waiting => DagStatus::Waiting,
            TaskState::Ready => DagStatus::Ready,
            TaskState::Running => DagStatus::Running,
            TaskState::Succeeded => DagStatus::Succeeded,
            TaskState::Failed { .. } => DagStatus::Failed,
            TaskState::Cancelled => DagStatus::Cancelled,
            TaskState::Skipped => DagStatus::Skipped,
            TaskState::Unknown => DagStatus::Unknown,
        }
    }

    /// `rollup_state` 문자열(`DagSummary`)에서. 알 수 없으면 `Unknown`.
    pub fn from_name(name: &str) -> Self {
        match name {
            "waiting" => DagStatus::Waiting,
            "ready" => DagStatus::Ready,
            "running" => DagStatus::Running,
            "succeeded" => DagStatus::Succeeded,
            "failed" => DagStatus::Failed,
            "cancelled" => DagStatus::Cancelled,
            "skipped" => DagStatus::Skipped,
            _ => DagStatus::Unknown,
        }
    }

    /// 색이 아닌 **모양** 채널.
    ///
    /// 어휘를 기하도형(U+25xx)·수학기호에서만 고른다. 체크/엑스(U+2713/2717)나
    /// 화살촉(U+276F)은 딩뱃 블록이라 UI 프로포셔널 폰트에서 tofu 로 떨어지고
    /// (`tests/design_token_adherence.rs::no_raw_pictographic_glyph` 가 막는다),
    /// 이모지 폴백은 컬러로 대체돼 색 채널과 중복된다 — 어느 쪽이든 3 채널이
    /// 2 채널로 줄어 이 표기의 목적이 사라진다. 그래서 채움/외곽선과 원/삼각/
    /// 마름모/사선이라는 **형태 차이**만으로 8 종을 구분한다.
    pub fn glyph(self) -> &'static str {
        match self {
            DagStatus::Waiting => "\u{25E6}",   // ◦ 흰 불릿
            DagStatus::Ready => "\u{25B7}",     // ▷ 흰 삼각(다음 차례)
            DagStatus::Running => "\u{25D1}",   // ◑ 반쯤 채운 원
            DagStatus::Succeeded => "\u{25CF}", // ● 채운 원
            DagStatus::Failed => "\u{00D7}",    // × 곱셈 기호
            DagStatus::Cancelled => "\u{2298}", // ⊘ 사선 원
            DagStatus::Skipped => "\u{25C7}",   // ◇ 흰 마름모
            DagStatus::Unknown => "?",
        }
    }

    /// 철자 라벨(번역).
    pub fn label(self) -> &'static str {
        match self {
            DagStatus::Waiting => t("dag.status.waiting"),
            DagStatus::Ready => t("dag.status.ready"),
            DagStatus::Running => t("dag.status.running"),
            DagStatus::Succeeded => t("dag.status.succeeded"),
            DagStatus::Failed => t("dag.status.failed"),
            DagStatus::Cancelled => t("dag.status.cancelled"),
            DagStatus::Skipped => t("dag.status.skipped"),
            DagStatus::Unknown => t("dag.status.unknown"),
        }
    }

    /// 노드 카드 자체가 디밍되는 상태 — 실행되지 않기로 확정된 자리다.
    pub fn is_dimmed(self) -> bool {
        matches!(self, DagStatus::Cancelled | DagStatus::Skipped)
    }

    /// 여기서 **나가는** 엣지가 죽은 경로인지. 실패도 포함한다 — 실패 노드의
    /// 하류는 (fallback 이 아니면) 진행되지 않는다.
    pub fn kills_outgoing(self) -> bool {
        self.is_dimmed() || self == DagStatus::Failed
    }

    /// terminal 상태(더 이상 전이하지 않음).
    pub fn is_terminal(self) -> bool {
        matches!(
            self,
            DagStatus::Succeeded | DagStatus::Failed | DagStatus::Cancelled | DagStatus::Skipped
        )
    }
}

/// 엣지 관계 3종. dash 패턴과 색 **둘 다**로 구분한다.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DagRelation {
    DependsOn,
    Fallback,
    Reduce,
}

impl DagRelation {
    pub fn from_kind(kind: &str) -> Self {
        match kind {
            "fallback" => DagRelation::Fallback,
            "reduce" => DagRelation::Reduce,
            _ => DagRelation::DependsOn,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            DagRelation::DependsOn => t("dag.rel.depends_on"),
            DagRelation::Fallback => t("dag.rel.fallback"),
            DagRelation::Reduce => t("dag.rel.reduce"),
        }
    }

    /// 파선 패턴 `(on, off)` — `None` 이면 실선.
    ///
    /// 디자인 토큰 `dag-edge-dash-fallback "6 3"` / `dag-edge-dash-reduce "2 3"` 는
    /// SVG `stroke-dasharray` 라 색/길이 토큰 타입 체계 밖이다(생성기가 다루지
    /// 않는다). 값은 그 토큰과 같게 유지한다.
    pub fn dash(self) -> Option<(f32, f32)> {
        match self {
            DagRelation::DependsOn => None,
            DagRelation::Fallback => Some((6.0, 3.0)),
            DagRelation::Reduce => Some((2.0, 3.0)),
        }
    }
}

/// task 종류 4종 — 노드 카드 선두 아이콘.
pub fn kind_label(command_kind: &str) -> &'static str {
    match command_kind {
        "custom" => t("dag.kind.custom"),
        "reduce" => t("dag.kind.reduce"),
        "wait_barrier" => t("dag.kind.wait_barrier"),
        _ => t("dag.kind.run"),
    }
}

/// 노드 하나가 화면에 필요로 하는 전부.
#[derive(Debug, Clone)]
pub struct DagNodeData {
    pub id: String,
    pub name: String,
    pub status: DagStatus,
    pub command_kind: &'static str,
    pub on_failure_kind: &'static str,
    /// 상세 패널의 `Command` 블록에 그대로 실리는 사람이 읽는 형태.
    pub command_text: String,
    pub started_at: Option<u64>,
    pub finished_at: Option<u64>,
    pub exit_code: Option<i32>,
    /// 실패 사유 tail. 상세 패널이 복사 가능한 블록으로 보여준다.
    pub error_tail: Option<String>,
    /// 표준 출력 tail.
    pub output_tail: Option<String>,
    /// 이 노드로 **들어오는** 엣지 — `(상대 노드 인덱스, 관계)`. 상세 패널의
    /// 의존성 행이 그대로 쓴다(클릭하면 그 노드로 선택 점프).
    pub incoming: Vec<(usize, DagRelation)>,
}

/// 엣지 하나.
#[derive(Debug, Clone, Copy)]
pub struct DagEdgeData {
    pub from: usize,
    pub to: usize,
    pub relation: DagRelation,
}

/// 한 DAG 의 그래프 전부.
#[derive(Debug, Clone)]
pub struct DagGraphData {
    pub id: String,
    pub name: String,
    pub nodes: Vec<DagNodeData>,
    pub edges: Vec<DagEdgeData>,
    /// 사이클을 이루는 task id 나열(`detect_cycles` 결과). 배너 문구와 사이클
    /// 노드 강조에 모두 쓰이므로 문자열로 접지 않고 id 목록 그대로 들고 있는다.
    pub cycle: Option<Vec<String>>,
    /// terminal 상태 개수 / 전체 — 헤더의 `7/12` 표시.
    pub done: usize,
}

impl DagGraphData {
    pub fn total(&self) -> usize {
        self.nodes.len()
    }

    pub fn index_of(&self, id: &str) -> Option<usize> {
        self.nodes.iter().position(|n| n.id == id)
    }
}

/// 헤더 드롭다운 한 줄.
#[derive(Debug, Clone)]
pub struct DagListEntry {
    pub id: String,
    pub name: String,
    pub rollup: DagStatus,
    pub task_count: usize,
}

/// 러너 배지 4값.
#[derive(Debug, Clone, Copy, Default)]
pub struct RunnerBadgeData {
    pub running: bool,
    pub crashed: bool,
    /// **보고 있는 DAG 안의** ready 개수. workspace 전체가 아니다 — 화면이 12 개
    /// 짜리 DAG 를 띄운 채 옆 DAG 의 ready 를 합산하면 배지가 거짓말을 한다.
    pub ready: usize,
    pub running_count: usize,
}

impl RunnerBadgeData {
    /// 아무도 그래프를 진행시키지 않는데 할 일이 남은 상태 — 1급 경고.
    pub fn is_stalled(&self) -> bool {
        !self.running && !self.crashed && self.ready > 0
    }
}

/// 한 번의 폴링이 만들어낸 화면 데이터 전부.
#[derive(Debug, Clone)]
pub struct DagData {
    /// 관찰 중인 workspace.
    pub workspace_id: u32,
    /// 그 workspace 의 DAG 목록(드롭다운).
    pub dags: Vec<DagListEntry>,
    /// 지금 그리는 DAG. 목록이 비었거나 지정 id 가 사라졌으면 `None`.
    pub current: Option<DagGraphData>,
    pub runner: RunnerBadgeData,
    /// 대상 id 는 있는데 목록에 없을 때 `true` — 빈 상태 문구가 갈린다.
    pub target_missing: bool,
}

/// `DagSummary` 목록 + 그 DAG 의 task 부분집합 → 화면 데이터.
///
/// `tasks` 는 **그 DAG 에 속한 task 만** 이어야 한다(호출자가 `DagSummary::task_ids`
/// 로 걸러 넘긴다) — 엣지 수집이 목록 밖 참조를 엣지로 만들지 않게 하려는 것이다.
pub fn build_graph(summary: &DagSummary, tasks: &[Task]) -> DagGraphData {
    let index: std::collections::HashMap<&str, usize> = tasks
        .iter()
        .enumerate()
        .map(|(i, t)| (t.id.as_str(), i))
        .collect();

    let mut edges = Vec::new();
    let mut incoming: Vec<Vec<(usize, DagRelation)>> = vec![Vec::new(); tasks.len()];
    for edge in collect_graph_edges(tasks) {
        let (Some(&from), Some(&to)) = (index.get(edge.from.as_str()), index.get(edge.to.as_str()))
        else {
            continue;
        };
        let relation = DagRelation::from_kind(edge.kind);
        edges.push(DagEdgeData { from, to, relation });
        incoming[to].push((from, relation));
    }

    let nodes: Vec<DagNodeData> = tasks
        .iter()
        .zip(incoming)
        .map(|(t, incoming)| DagNodeData {
            id: t.id.clone(),
            name: t.name.clone(),
            status: DagStatus::from_state(&t.state),
            command_kind: task_command_kind(&t.command),
            on_failure_kind: on_failure_kind(&t.on_failure),
            command_text: command_text(&t.command),
            started_at: t.started_at,
            finished_at: t.finished_at,
            exit_code: t.result.as_ref().and_then(|r| r.exit_code),
            error_tail: error_tail(t),
            output_tail: t
                .result
                .as_ref()
                .and_then(|r| r.output.as_ref())
                .map(output_tail),
            incoming,
        })
        .collect();

    let done = nodes.iter().filter(|n| n.status.is_terminal()).count();

    DagGraphData {
        id: summary.id.clone(),
        name: summary.name.clone(),
        nodes,
        edges,
        cycle: None, // 호출자가 사이클 검출 결과로 채운다
        done,
    }
}

/// 상세 패널이 보여줄 명령 문자열. dot 렌더의 라벨과 달리 사람이 읽는 형태다.
fn command_text(command: &TaskCommand) -> String {
    match command {
        TaskCommand::Run { command, .. } => command.join(" "),
        TaskCommand::Custom { ipc_method, .. } => format!("ipc: {ipc_method}"),
        TaskCommand::Reduce { inputs, .. } => {
            format!("reduce: {}", inputs.join(", "))
        }
        TaskCommand::WaitBarrier { name } => format!("barrier: {name}"),
    }
}

/// 실패 사유 — 상태에 실린 문자열이 1순위, 없으면 `result.error`.
fn error_tail(task: &Task) -> Option<String> {
    if let TaskState::Failed { error } = &task.state
        && !error.is_empty()
    {
        return Some(error.clone());
    }
    task.result
        .as_ref()
        .and_then(|r| r.error.clone())
        .filter(|e| !e.is_empty())
}

/// `TaskResult::output` 에서 사람이 읽을 tail 을 뽑는다.
///
/// `Run` 성공 결과는 `{"pid", "stdout": {"text", …}, "stderr": {…}}` 형태다 —
/// 그 경우 stdout/stderr 텍스트만 잇는다. 그 외(Custom 의 IPC 응답 등)는 JSON 을
/// 그대로 예쁘게 찍는다.
fn output_tail(output: &serde_json::Value) -> String {
    let stream = |key: &str| {
        output
            .get(key)
            .and_then(|s| s.get("text"))
            .and_then(|t| t.as_str())
            .filter(|t| !t.is_empty())
    };
    match (stream("stdout"), stream("stderr")) {
        (None, None) => serde_json::to_string_pretty(output).unwrap_or_else(|_| output.to_string()),
        (out, err) => {
            let mut buf = String::new();
            if let Some(o) = out {
                buf.push_str(o);
            }
            if let Some(e) = err {
                if !buf.is_empty() && !buf.ends_with('\n') {
                    buf.push('\n');
                }
                buf.push_str(e);
            }
            buf
        }
    }
}

/// epoch ms → `HH:MM:SS`(로컬). 상세 패널의 `Started` 행.
pub fn format_clock(epoch_ms: u64) -> String {
    use chrono::{Local, TimeZone as _};
    match Local.timestamp_millis_opt(epoch_ms as i64) {
        chrono::LocalResult::Single(dt) => dt.format("%H:%M:%S").to_string(),
        // 표현 불가능한 타임스탬프(손상된 레코드)는 값을 지어내지 않고 비운다.
        _ => t("dag.detail.none").to_string(),
    }
}

/// 밀리초 → `1.2s` / `24s` / `3m 04s` / `1h 02m`.
pub fn format_duration_ms(ms: u64) -> String {
    let secs = ms / 1000;
    if secs < 10 {
        // 10 초 미만은 소수 한 자리 — 짧은 task 가 전부 "0s" 로 뭉개지지 않게.
        return format!("{}.{}s", secs, (ms % 1000) / 100);
    }
    if secs < 60 {
        return format!("{secs}s");
    }
    if secs < 3600 {
        return format!("{}m {:02}s", secs / 60, secs % 60);
    }
    format!("{}h {:02}m", secs / 3600, (secs % 3600) / 60)
}

/// 노드 meta 행에 보일 duration. `running` 은 경과, terminal 은 소요,
/// `waiting`/`ready` 는 표시하지 않는다.
pub fn node_duration(node: &DagNodeData, now_ms: u64) -> Option<String> {
    let started = node.started_at?;
    match node.status {
        DagStatus::Waiting | DagStatus::Ready => None,
        DagStatus::Running => Some(format_duration_ms(now_ms.saturating_sub(started))),
        _ => node
            .finished_at
            .map(|end| format_duration_ms(end.saturating_sub(started))),
    }
}
