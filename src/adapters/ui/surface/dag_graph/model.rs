//! Task 레코드를 DAG 화면에서 사용할 데이터로 바꾼다.

use tasty_agent::{DagSummary, Task, TaskCommand, TaskState};

use crate::i18n::{t, t_fmt, t_fmt2};
use tasty_task_runtime::graph_view::{
    collect_graph_edges, drawn_edges, on_failure_kind, task_command_kind,
};

/// 색·기호·번역 라벨로 함께 표시하는 노드 상태.
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
    /// DAG 요약 전용(`rollup_state: partially_failed`). 표시 디자인이 정해지기 전까지 글리프·색은
    /// `Unknown` 과 같고 라벨만 다르다.
    PartiallyFailed,
}

impl DagStatus {
    /// 개별 task 상태 전체와 표시 순서. DAG 목록 필터는 ROLLUP_ALL을 사용한다.
    #[allow(dead_code)]
    pub const ALL: [DagStatus; 8] = [
        DagStatus::Waiting,
        DagStatus::Ready,
        DagStatus::Running,
        DagStatus::Succeeded,
        DagStatus::Failed,
        DagStatus::Cancelled,
        DagStatus::Skipped,
        DagStatus::Unknown,
    ];

    /// DAG 집계에서 반환하는 여섯 상태. Cancelled는 Skipped로, Unknown은 Waiting으로
    /// 집계되므로 목록 필터에서는 두 개별 상태를 제외한다.
    pub const ROLLUP_ALL: [DagStatus; 6] = [
        DagStatus::Waiting,
        DagStatus::Ready,
        DagStatus::Running,
        DagStatus::Succeeded,
        DagStatus::Failed,
        DagStatus::Skipped,
    ];

    pub fn from_state(state: &TaskState) -> Self {
        match state {
            TaskState::Waiting => DagStatus::Waiting,
            TaskState::Ready => DagStatus::Ready,
            TaskState::Running => DagStatus::Running,
            TaskState::Succeeded => DagStatus::Succeeded,
            TaskState::Failed { .. } => DagStatus::Failed,
            TaskState::Cancelled => DagStatus::Cancelled,
            TaskState::Skipped => DagStatus::Skipped,
            TaskState::Unknown { .. } => DagStatus::Unknown,
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
            "partially_failed" => DagStatus::PartiallyFailed,
            _ => DagStatus::Unknown,
        }
    }

    /// 글꼴 누락과 컬러 이모지 대체를 피하려 기하·수학 기호로 상태를 구분한다.
    pub fn glyph(self) -> &'static str {
        match self {
            DagStatus::Waiting => "\u{25E6}",   // ◦ 흰 불릿
            DagStatus::Ready => "\u{25B7}",     // ▷ 흰 삼각(다음 차례)
            DagStatus::Running => "\u{25D1}",   // ◑ 반쯤 채운 원
            DagStatus::Succeeded => "\u{25CF}", // ● 채운 원
            DagStatus::Failed => "\u{00D7}",    // × 곱셈 기호
            DagStatus::Cancelled => "\u{2212}", // − 빼기 기호
            // 두 건너뜀 이유(미선택·선행 결과 없음)가 같은 글리프를 쓴다.
            DagStatus::Skipped => "\u{2298}", // ⊘ 사선 원
            DagStatus::Unknown | DagStatus::PartiallyFailed => "?",
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
            DagStatus::PartiallyFailed => t("dag.status.partially_failed"),
        }
    }

    /// 취소·스킵 상태의 카드는 흐리게 표시한다.
    pub fn is_dimmed(self) -> bool {
        matches!(self, DagStatus::Cancelled | DagStatus::Skipped)
    }

    /// 하류 경로를 흐리게 표시할 시작 상태. 실행 정책을 평가하는 판정은 아니다.
    pub fn kills_outgoing(self) -> bool {
        self.is_dimmed() || self == DagStatus::Failed
    }

    /// 화면에서 완료된 것으로 세는 상태.
    pub fn is_terminal(self) -> bool {
        matches!(
            self,
            DagStatus::Succeeded
                | DagStatus::Failed
                | DagStatus::PartiallyFailed
                | DagStatus::Cancelled
                | DagStatus::Skipped
        )
    }
}

/// 엣지 관계 5종. dash 패턴과 색 **둘 다**로 구분한다.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DagRelation {
    DependsOn,
    Fallback,
    Reduce,
    /// v2 입력 binding — 값이 원본에서 받는 task 로 넘어간다.
    Binding,
    /// v2 전이 — 원본의 성공 출력으로 고른 경로.
    Transition,
}

impl DagRelation {
    pub fn from_kind(kind: &str) -> Self {
        match kind {
            "fallback" => DagRelation::Fallback,
            "reduce" => DagRelation::Reduce,
            "binding" => DagRelation::Binding,
            "transition" => DagRelation::Transition,
            _ => DagRelation::DependsOn,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            DagRelation::DependsOn => t("dag.rel.depends_on"),
            DagRelation::Fallback => t("dag.rel.fallback"),
            DagRelation::Reduce => t("dag.rel.reduce"),
            DagRelation::Binding => t("dag.rel.binding"),
            DagRelation::Transition => t("dag.rel.transition"),
        }
    }

    /// 선·틈 길이를 번갈아 적은 파선 패턴. `None` 이면 실선.
    ///
    /// 디자인 토큰 `dag-edge-dash-*`(fallback "6 3" · reduce "2 3" · binding "8 2 2 2" ·
    /// transition "10 4")는 SVG `stroke-dasharray` 라 색/길이 토큰 타입 체계 밖이다(생성기가
    /// 다루지 않는다). 시험 `edge_dashes_match_the_vendor_tokens` 가 토큰 JSON 과 대조한다.
    pub fn dash(self) -> Option<&'static [f32]> {
        match self {
            DagRelation::DependsOn => None,
            DagRelation::Fallback => Some(&[6.0, 3.0]),
            DagRelation::Reduce => Some(&[2.0, 3.0]),
            DagRelation::Binding => Some(&[8.0, 2.0, 2.0, 2.0]),
            DagRelation::Transition => Some(&[10.0, 4.0]),
        }
    }
}

/// 전이 엣지의 선택 상태. 색·파선은 같고 굵기와 불투명도만 바뀐다.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EdgeSelection {
    /// 원본이 아직 끝나지 않았다.
    Pending,
    Selected,
    NotSelected,
    /// 원본이 성공 결과를 내지 못했다. 죽은 경로와 같이 흐리게 그린다.
    Unavailable,
}

impl EdgeSelection {
    pub fn from_name(name: &str) -> Self {
        match name {
            "selected" => EdgeSelection::Selected,
            "not_selected" => EdgeSelection::NotSelected,
            "unavailable" => EdgeSelection::Unavailable,
            _ => EdgeSelection::Pending,
        }
    }

    /// 흐리게 그리는 상태.
    pub fn is_dimmed(self) -> bool {
        matches!(
            self,
            EdgeSelection::NotSelected | EdgeSelection::Unavailable
        )
    }
}

/// 건너뛴 task 의 이유. 카드 모양은 skipped 그대로이고 라벨·툴팁만 다르다.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NodeSkip {
    BranchNotSelected,
    UpstreamUnavailable { source: String, state: String },
}

impl NodeSkip {
    fn from_reason(reason: &tasty_agent::SkipReason) -> Self {
        match reason {
            tasty_agent::SkipReason::BranchNotSelected => NodeSkip::BranchNotSelected,
            tasty_agent::SkipReason::UpstreamUnavailable {
                source,
                source_state,
            } => NodeSkip::UpstreamUnavailable {
                source: source.clone(),
                state: source_state.clone(),
            },
        }
    }
}

/// 완료/전체 뒤에 붙이는 건너뜀 수 ` · {n} skipped ({k} not selected)`.
/// 건너뛴 task 가 없으면 빈 문자열이고, 괄호는 미선택이 있을 때만 붙는다.
pub fn skip_count_suffix(skipped: usize, not_selected: usize) -> String {
    if skipped == 0 {
        return String::new();
    }
    let mut text = format!(
        " \u{b7} {}",
        t_fmt("dag.count.skipped", &skipped.to_string())
    );
    if not_selected > 0 {
        text.push(' ');
        text.push_str(&t_fmt("dag.count.not_selected", &not_selected.to_string()));
    }
    text
}

/// task 종류의 철자 라벨.
pub fn kind_label(command_kind: &str) -> &'static str {
    match command_kind {
        "custom" => t("dag.kind.custom"),
        "agent" => t("dag.kind.agent"),
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
    /// 건너뛴 이유. 이유가 기록된 v2 task 만 있다.
    pub skip: Option<NodeSkip>,
}

impl DagNodeData {
    /// 카드·상세의 상태 라벨. 경로가 선택되지 않은 skipped 는 그 사실을 적는다.
    pub fn status_label(&self) -> &'static str {
        match (self.status, &self.skip) {
            (DagStatus::Skipped, Some(NodeSkip::BranchNotSelected)) => t("dag.status.not_selected"),
            (status, _) => status.label(),
        }
    }

    /// 건너뛴 이유 툴팁. 이유가 없으면 `None`.
    pub fn skip_tooltip(&self) -> Option<String> {
        if self.status != DagStatus::Skipped {
            return None;
        }
        match self.skip.as_ref()? {
            NodeSkip::BranchNotSelected => Some(t("dag.skip.branch_not_selected").to_string()),
            NodeSkip::UpstreamUnavailable { source, state } => Some(t_fmt2(
                "dag.skip.upstream_unavailable",
                source,
                &DagStatus::from_name(state).label().to_lowercase(),
            )),
        }
    }
}

/// 엣지 하나.
#[derive(Debug, Clone, Copy)]
pub struct DagEdgeData {
    pub from: usize,
    pub to: usize,
    pub relation: DagRelation,
    /// 전이 엣지의 선택 상태. 다른 관계는 `None`.
    pub selection: Option<EdgeSelection>,
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

    /// 건너뛴 task 수와 그중 경로가 선택되지 않은 수.
    pub fn skip_counts(&self) -> (usize, usize) {
        let skipped = self.nodes.iter().filter(|n| n.status == DagStatus::Skipped);
        let not_selected = skipped
            .clone()
            .filter(|n| matches!(n.skip, Some(NodeSkip::BranchNotSelected)))
            .count();
        (skipped.count(), not_selected)
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
    /// 현재 DAG 안의 Ready 개수. 워크스페이스 전체 개수는 아니다.
    pub ready: usize,
    pub running_count: usize,
}

impl RunnerBadgeData {
    /// 할 일이 남았지만 러너가 진행하지 않는 상태.
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

/// 해당 DAG에 속하는 task만 받아 화면 데이터로 바꾼다.
pub fn build_graph(summary: &DagSummary, tasks: &[Task]) -> DagGraphData {
    let index: std::collections::HashMap<&str, usize> = tasks
        .iter()
        .enumerate()
        .map(|(i, t)| (t.id.as_str(), i))
        .collect();

    let mut edges = Vec::new();
    let mut incoming: Vec<Vec<(usize, DagRelation)>> = vec![Vec::new(); tasks.len()];
    for edge in drawn_edges(collect_graph_edges(tasks)) {
        let (Some(&from), Some(&to)) = (index.get(edge.from.as_str()), index.get(edge.to.as_str()))
        else {
            continue;
        };
        let relation = DagRelation::from_kind(edge.kind);
        edges.push(DagEdgeData {
            from,
            to,
            relation,
            selection: edge.selection.map(EdgeSelection::from_name),
        });
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
            command_text: command_text(t),
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
            skip: t.skip.as_ref().map(NodeSkip::from_reason),
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
fn command_text(task: &Task) -> String {
    match &task.command {
        TaskCommand::Run { command, .. } => command.join(" "),
        TaskCommand::Custom { ipc_method, .. } => format!("ipc: {ipc_method}"),
        TaskCommand::Reduce { inputs, .. } => {
            format!("reduce: {}", inputs.join(", "))
        }
        // 입력에서 이름을 받는 barrier 는 해석 전이면 이름이 없다.
        TaskCommand::WaitBarrier { .. } => {
            format!("barrier: {}", task.barrier_name().unwrap_or("-"))
        }
        TaskCommand::Agent { provider, .. } => format!("agent: {provider}"),
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

/// 시간 값의 단위와 자릿수 표기는 번역 키에서 정한다.
pub fn format_duration_ms(ms: u64) -> String {
    let secs = ms / 1000;
    if secs < 10 {
        return t_fmt2(
            "dag.duration.sub_ten",
            &secs.to_string(),
            &((ms % 1000) / 100).to_string(),
        );
    }
    if secs < 60 {
        return t_fmt("dag.duration.seconds", &secs.to_string());
    }
    if secs < 3600 {
        return t_fmt2(
            "dag.duration.minutes",
            &(secs / 60).to_string(),
            &format!("{:02}", secs % 60),
        );
    }
    t_fmt2(
        "dag.duration.hours",
        &(secs / 3600).to_string(),
        &format!("{:02}", (secs % 3600) / 60),
    )
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tasty_agent::{OnFailure, RouteDecision, SkipReason};

    fn task(id: &str, state: TaskState) -> Task {
        Task {
            id: id.to_string(),
            workspace_id: 1,
            name: id.to_string(),
            command: TaskCommand::Run {
                command: vec!["true".to_string()],
                workspace_id: 1,
                cwd: None,
            },
            depends_on: Vec::new(),
            state,
            created_at: 0,
            started_at: None,
            finished_at: None,
            result: None,
            on_failure: OnFailure::Abort,
            metadata: serde_json::Value::Null,
            reserved_for_fallback: false,
            contract: None,
            typed_result: None,
            graph_id: None,
            input_snapshot: None,
            accepted: None,
            attempt: None,
            route: None,
            skip: None,
        }
    }

    fn summary() -> DagSummary {
        DagSummary {
            id: "d:t".to_string(),
            workspace_id: 1,
            name: "t".to_string(),
            source: "explicit",
            task_count: 0,
            state_counts: Default::default(),
            rollup_state: "waiting",
            created_at: 0,
            updated_at: 0,
            root_task_ids: Vec::new(),
            has_cycle: false,
            task_ids: Vec::new(),
        }
    }

    #[test]
    fn every_edge_kind_maps_to_its_own_relation() {
        for (kind, rel) in [
            ("depends_on", DagRelation::DependsOn),
            ("fallback", DagRelation::Fallback),
            ("reduce", DagRelation::Reduce),
            ("binding", DagRelation::Binding),
            ("transition", DagRelation::Transition),
        ] {
            assert_eq!(DagRelation::from_kind(kind), rel, "{kind}");
        }
    }

    /// 파선 배열은 vendor 토큰 `dag-edge-dash-*` 의 strokeStyle 값과 같아야 한다.
    /// 갤러리 `Rel::dash` 는 tasty-gallery 의 시험이 같은 토큰과 대조한다.
    #[test]
    fn edge_dashes_match_the_vendor_tokens() {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/crates/tasty-design-tokens/dtcg/tasty.tokens.json"
        );
        let root: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(path).expect("read vendor tokens"))
                .expect("parse vendor tokens");
        let mut tokens = std::collections::BTreeMap::new();
        let mut stack = vec![&root];
        while let Some(value) = stack.pop() {
            let Some(map) = value.as_object() else {
                continue;
            };
            for (key, child) in map {
                match key.strip_prefix("dag-edge-dash-") {
                    Some(slug) => {
                        assert_eq!(child["$type"], "strokeStyle", "{key}");
                        let dashes: Vec<f32> = child["$value"]
                            .as_str()
                            .expect("strokeStyle string")
                            .split_whitespace()
                            .map(|n| n.parse().expect("dash number"))
                            .collect();
                        assert!(tokens.insert(slug.to_string(), dashes).is_none(), "{key}");
                    }
                    None => stack.push(child),
                }
            }
        }
        let mut seen = Vec::new();
        for rel in [
            DagRelation::DependsOn,
            DagRelation::Fallback,
            DagRelation::Reduce,
            DagRelation::Binding,
            DagRelation::Transition,
        ] {
            let slug = match rel {
                DagRelation::DependsOn => None,
                DagRelation::Fallback => Some("fallback"),
                DagRelation::Reduce => Some("reduce"),
                DagRelation::Binding => Some("binding"),
                DagRelation::Transition => Some("transition"),
            };
            match slug {
                Some(slug) => {
                    let want = tokens
                        .get(slug)
                        .unwrap_or_else(|| panic!("no token for {slug}"));
                    assert_eq!(rel.dash(), Some(want.as_slice()), "{rel:?}");
                    seen.push(slug.to_string());
                }
                None => assert_eq!(rel.dash(), None, "{rel:?} is drawn solid"),
            }
        }
        seen.sort();
        assert_eq!(tokens.into_keys().collect::<Vec<_>>(), seen);
    }

    /// 경로를 고른 뒤 전이 엣지는 선택 상태를, 고르지 않은 노드는 skip 이유를 싣는다.
    /// 같은 쌍의 depends_on 은 binding 한 줄로 합쳐진다.
    #[test]
    fn build_graph_carries_selection_skip_and_binding() {
        crate::i18n::init("en");
        let mut review = task("review", TaskState::Succeeded);
        review.contract = Some(
            serde_json::from_value(json!({"contract_version": 2,
                "transitions": {"cases": [
                    {"when": {"compare": {"path": "", "op": "eq", "value": 1}}, "to": ["ship"]},
                    {"when": {"compare": {"path": "", "op": "eq", "value": 2}}, "to": ["fix"]}],
                    "no_match": "finish"}}))
            .expect("contract"),
        );
        review.route = Some(RouteDecision {
            attempt_id: Some("review#1".into()),
            matched: vec![0],
            otherwise: false,
            selected: vec!["ship".into()],
        });
        let mut ship = task("ship", TaskState::Running);
        ship.depends_on = vec!["review".into()];
        ship.contract = Some(
            serde_json::from_value(json!({"contract_version": 2,
                "input_schema": {"type": "object", "fields": {"v": {"type": "json"}}},
                "bindings": {"v": {"from_task": "review"}}}))
            .expect("contract"),
        );
        let mut fix = task("fix", TaskState::Skipped);
        fix.skip = Some(SkipReason::BranchNotSelected);
        let mut notify = task("notify", TaskState::Skipped);
        notify.skip = Some(SkipReason::UpstreamUnavailable {
            source: "ship".into(),
            source_state: "failed".into(),
        });

        let g = build_graph(&summary(), &[review, ship, fix, notify]);
        let edges: Vec<(usize, usize, DagRelation, Option<EdgeSelection>)> = g
            .edges
            .iter()
            .map(|e| (e.from, e.to, e.relation, e.selection))
            .collect();
        assert_eq!(
            edges,
            vec![
                (0, 1, DagRelation::Transition, Some(EdgeSelection::Selected)),
                (
                    0,
                    2,
                    DagRelation::Transition,
                    Some(EdgeSelection::NotSelected)
                ),
                (0, 1, DagRelation::Binding, None),
            ]
        );
        assert_eq!(g.nodes[2].status_label(), "NOT SELECTED");
        assert_eq!(
            g.nodes[2].skip_tooltip().as_deref(),
            Some("Not taken — another branch was selected.")
        );
        assert_eq!(g.nodes[3].status_label(), "SKIPPED");
        assert_eq!(
            g.nodes[3].skip_tooltip().as_deref(),
            Some("Skipped — ship failed.")
        );
        assert_eq!(g.nodes[1].skip_tooltip(), None);
    }

    /// agent 작업은 provider 와 관계없이 전용 글리프로 그리고, 모르는 종류만 run 글리프로 둔다.
    #[test]
    fn agent_tasks_take_the_agent_glyph() {
        use super::super::node::kind_icon;
        assert_eq!(
            kind_icon("agent").uri,
            crate::adapters::ui::icons::AGENT.uri
        );
        assert_eq!(
            kind_icon("unknown").uri,
            crate::adapters::ui::icons::TERM.uri
        );
    }

    /// 상태 글리프는 서로 겹치지 않고, 건너뜀은 시안의 ⊘ 를 쓴다.
    #[test]
    fn status_glyphs_are_distinct_and_skipped_is_the_slashed_circle() {
        let all = DagStatus::ALL;
        let glyphs: std::collections::BTreeSet<_> = all.iter().map(|s| s.glyph()).collect();
        assert_eq!(glyphs.len(), all.len(), "{glyphs:?}");
        assert_eq!(DagStatus::Skipped.glyph(), "\u{2298}");
        assert_eq!(DagStatus::Cancelled.glyph(), "\u{2212}");
    }

    /// 건너뜀 수는 하나 이상일 때만, 미선택 괄호는 미선택이 있을 때만 붙는다.
    #[test]
    fn skip_count_suffix_shows_only_what_is_there() {
        crate::i18n::init("en");
        assert_eq!(skip_count_suffix(0, 0), "");
        let skipped = skip_count_suffix(2, 0);
        assert!(skipped.starts_with(" \u{b7} "), "{skipped:?}");
        assert!(
            skipped.contains('2') && !skipped.contains('('),
            "{skipped:?}"
        );
        assert_eq!(
            skip_count_suffix(3, 1),
            " \u{b7} 3 skipped (1 not selected)"
        );
    }

    /// 탭 머리글의 건너뜀 수는 skipped 노드를, 미선택 수는 그중 경로 미선택을 센다.
    #[test]
    fn graph_skip_counts_split_out_the_not_selected_branch() {
        let mut fix = task("fix", TaskState::Skipped);
        fix.skip = Some(SkipReason::BranchNotSelected);
        let mut notify = task("notify", TaskState::Skipped);
        notify.skip = Some(SkipReason::UpstreamUnavailable {
            source: "a".into(),
            source_state: "failed".into(),
        });
        let graph = build_graph(&summary(), &[task("a", TaskState::Succeeded), fix, notify]);
        assert_eq!(graph.skip_counts(), (2, 1));
    }
}
