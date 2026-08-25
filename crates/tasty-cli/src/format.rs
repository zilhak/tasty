use super::{AgentCommands, Commands, ListCommands};

pub fn format_output(command: &Commands, result: &serde_json::Value) {
    match command {
        Commands::List { command } => format_list_output(command, result),
        Commands::Agent { command } => format_agent_output(command, result),
        _ => {
            // Pretty print JSON
            println!("{}", serde_json::to_string_pretty(result).unwrap());
        }
    }
}

/// `tasty agent task-{list,get,run}` 만 사람이 터미널에서 바로 읽는 텍스트로
/// 렌더한다 — 결정 5(CLI 관측 표면): "runner 가 꺼져 있다"/"이 task 는 외부
/// 신호를 기다린다" 를 raw JSON 을 눈으로 파싱하지 않고 바로 알아볼 수 있어야
/// 한다. 그 외 커맨드(barrier/semaphore/lease/rate_limit/task-graph 등)는 구조적
/// 데이터라 pretty JSON 그대로가 적절 — GUI 는 만들지 않는다(결정 5).
fn format_agent_output(command: &AgentCommands, result: &serde_json::Value) {
    match command {
        AgentCommands::TaskList { .. } => format_task_list(result),
        AgentCommands::TaskGet { .. } => format_task_get(result),
        AgentCommands::TaskRun { .. } => format_task_run(result),
        _ => println!("{}", serde_json::to_string_pretty(result).unwrap()),
    }
}

/// runner 요약 한 줄 — `{running, crashed, ready_count, running_count}` 형태의
/// `runner` 서브객체를 공유(`task_list`/`task_graph`/`task_run` 응답 공통 shape).
fn format_runner_summary(runner: &serde_json::Value) -> String {
    let running = runner
        .get("running")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let crashed = runner
        .get("crashed")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let ready = runner
        .get("ready_count")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    let inflight = runner
        .get("running_count")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    let status = if crashed {
        "crashed"
    } else if running {
        "running"
    } else {
        "stopped"
    };
    let mut line = format!("runner: {status} (ready={ready} running={inflight})");
    if !running && (ready > 0 || inflight > 0) {
        line.push_str(" — pending work but no runner; `agent task-run --action start` to resume");
    }
    line
}

/// `state` 는 `TaskState` 의 internally-tagged 직렬화(`{"kind": "...", ...}`)라
/// 최상위 문자열이 아니다 — `kind` 서브필드를 꺼내야 한다.
fn task_state_kind(task: &serde_json::Value) -> &str {
    task.get("state")
        .and_then(|s| s.get("kind"))
        .and_then(|v| v.as_str())
        .unwrap_or("?")
}

fn format_task_list(result: &serde_json::Value) {
    let tasks = result.get("tasks").and_then(|v| v.as_array());
    let Some(tasks) = tasks else {
        println!("{}", serde_json::to_string_pretty(result).unwrap());
        return;
    };
    if tasks.is_empty() {
        println!("No tasks");
    } else {
        for t in tasks {
            let id = t.get("id").and_then(|v| v.as_str()).unwrap_or("?");
            let name = t.get("name").and_then(|v| v.as_str()).unwrap_or("?");
            let state = task_state_kind(t);
            println!("{state:<10} {id}  {name}");
        }
    }
    if let Some(runner) = result.get("runner") {
        println!("{}", format_runner_summary(runner));
    }
}

/// `command`(internally-tagged, `{"kind": "...", ...}`) 한 줄 요약. audit 시나리오
/// (예: "이 task 가 정말 이 dispatch 를 실행하나")를 raw JSON 파싱 없이 확인하기 위함.
fn format_command_summary(command: &serde_json::Value) -> String {
    let kind = command.get("kind").and_then(|v| v.as_str()).unwrap_or("?");
    match kind {
        "run" => {
            let cmd = command
                .get("command")
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|x| x.as_str())
                        .collect::<Vec<_>>()
                        .join(" ")
                })
                .unwrap_or_default();
            format!("run: {cmd}")
        }
        "custom" => {
            let method = command
                .get("ipc_method")
                .and_then(|v| v.as_str())
                .unwrap_or("?");
            format!("custom: {method}")
        }
        "reduce" => {
            let strategy = command
                .get("strategy")
                .and_then(|s| s.get("kind"))
                .and_then(|v| v.as_str())
                .unwrap_or("?");
            let inputs = command
                .get("inputs")
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|x| x.as_str())
                        .collect::<Vec<_>>()
                        .join(",")
                })
                .unwrap_or_default();
            format!("reduce: strategy={strategy} inputs=[{inputs}]")
        }
        "wait_barrier" => {
            let name = command.get("name").and_then(|v| v.as_str()).unwrap_or("?");
            format!("wait_barrier: {name}")
        }
        other => other.to_string(),
    }
}

/// `on_failure`(internally-tagged) 한 줄 요약 — audit 시나리오(예: "이 task 가 정말
/// 저 fallback task 에 게이트돼 있나")를 `tasty memory get` 우회 없이 확인하기 위함.
fn format_on_failure_summary(on_failure: &serde_json::Value) -> String {
    let kind = on_failure
        .get("kind")
        .and_then(|v| v.as_str())
        .unwrap_or("abort");
    if kind != "fallback" {
        return kind.to_string();
    }
    if let Some(task) = on_failure.get("task").and_then(|v| v.as_str()) {
        format!("fallback:{task}")
    } else if on_failure.get("inline").is_some() {
        "fallback:inline".to_string()
    } else {
        "fallback".to_string()
    }
}

fn format_task_get(result: &serde_json::Value) {
    let id = result.get("id").and_then(|v| v.as_str()).unwrap_or("?");
    let name = result.get("name").and_then(|v| v.as_str()).unwrap_or("?");
    let state = task_state_kind(result);
    println!("id: {id}");
    println!("name: {name}");
    if let Some(error) = result
        .get("state")
        .and_then(|s| s.get("error"))
        .and_then(|v| v.as_str())
    {
        println!("state: {state} ({error})");
    } else {
        println!("state: {state}");
    }
    // 결정 5: AwaitExternal 로 외부 신호를 기다리는 task 는 "그냥 running" 과
    // 텍스트로도 구분되게 wait_key/deadline 을 함께 보여준다.
    if let Some(wait) = result.get("awaiting_external") {
        let wait_key = wait.get("wait_key").and_then(|v| v.as_str()).unwrap_or("?");
        let deadline_ms = wait
            .get("deadline_ms")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);
        println!("awaiting external signal — wait_key={wait_key}, deadline_ms={deadline_ms}");
    }
    if let Some(command) = result.get("command") {
        println!("command: {}", format_command_summary(command));
    }
    if let Some(deps) = result.get("depends_on").and_then(|v| v.as_array())
        && !deps.is_empty()
    {
        let list = deps
            .iter()
            .filter_map(|v| v.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        println!("depends_on: {list}");
    }
    if let Some(on_failure) = result.get("on_failure") {
        println!("on_failure: {}", format_on_failure_summary(on_failure));
    }
    if let Some(metadata) = result.get("metadata")
        && !metadata.is_null()
    {
        println!(
            "metadata: {}",
            serde_json::to_string_pretty(metadata).unwrap()
        );
    }
    if let Some(result_val) = result.get("result")
        && !result_val.is_null()
    {
        println!(
            "result: {}",
            serde_json::to_string_pretty(result_val).unwrap()
        );
    }
}

fn format_task_run(result: &serde_json::Value) {
    println!("{}", format_runner_summary(result));
}

fn format_list_output(command: &ListCommands, result: &serde_json::Value) {
    match command {
        ListCommands::Tree => format_tree(result),
        ListCommands::Workspaces => format_workspace_list(result),
        ListCommands::Panes => format_pane_list(result),
        ListCommands::Notifications => format_notification_list(result),
        _ => {
            println!("{}", serde_json::to_string_pretty(result).unwrap());
        }
    }
}

/// Render the full `list tree` output: workspace → pane → tab → surface.
///
/// Decomposed into per-level renderers ([`format_pane`], [`format_tab`],
/// [`format_tab_ids`]) so each level stays within the cognitive-complexity
/// gate. The emitted text is byte-for-byte identical to the previous
/// monolithic form.
fn format_tree(result: &serde_json::Value) {
    if let Some(workspaces) = result.as_array() {
        for ws in workspaces {
            let ws_id = ws.get("id").and_then(|v| v.as_u64()).unwrap_or(0);
            let name = ws.get("name").and_then(|v| v.as_str()).unwrap_or("?");
            let active = ws.get("active").and_then(|v| v.as_bool()).unwrap_or(false);
            let marker = if active { " *" } else { "" };
            println!("Workspace: {} (id:{}){}", name, ws_id, marker);

            if let Some(panes) = ws.get("panes").and_then(|v| v.as_array()) {
                for pane in panes {
                    format_pane(pane);
                }
            }
        }
    }
}

/// Render a single pane line and its tabs.
fn format_pane(pane: &serde_json::Value) {
    let pid = pane.get("id").and_then(|v| v.as_u64()).unwrap_or(0);
    let focused = pane
        .get("focused")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let pfx = if focused { ">" } else { " " };
    println!("  {} Pane {} (id:{})", pfx, pid, pid);

    if let Some(tabs) = pane.get("tabs").and_then(|v| v.as_array()) {
        for tab in tabs {
            format_tab(tab);
        }
    }
}

/// Render a single tab line and, for split tabs, its nested layout tree.
fn format_tab(tab: &serde_json::Value) {
    let tid = tab.get("id").and_then(|v| v.as_u64());
    let tname = tab.get("name").and_then(|v| v.as_str()).unwrap_or("?");
    let tactive = tab.get("active").and_then(|v| v.as_bool()).unwrap_or(false);
    let tpfx = if tactive { "*" } else { " " };

    // Extract surface info from the tab's surface field
    let surface = tab.get("surface");
    let stype = surface.and_then(|s| s.get("type")).and_then(|v| v.as_str());
    let sid = surface.and_then(|s| s.get("id")).and_then(|v| v.as_u64());
    let surfaces_arr = surface
        .and_then(|s| s.get("surfaces"))
        .and_then(|v| v.as_array());

    // Split tab with full nested layout → render the
    // SurfaceGroup split tree under the tab line.
    if stype == Some("SplitLayout")
        && let Some(layout) = surface.and_then(|s| s.get("layout"))
        && !layout.is_null()
    {
        let focused = surface
            .and_then(|s| s.get("focused_surface"))
            .and_then(|v| v.as_u64());
        match tid {
            Some(t) => {
                println!("      {} {} (tab:{})", tpfx, tname, t)
            }
            None => println!("      {} {}", tpfx, tname),
        }
        let mut lines = Vec::new();
        render_layout(layout, "        ", true, focused, &mut lines);
        for l in lines {
            println!("{}", l);
        }
        return;
    }

    let ids = format_tab_ids(tid, sid, surfaces_arr, stype);

    if ids.is_empty() {
        println!("      {} {}", tpfx, tname);
    } else {
        println!("      {} {} [{}]", tpfx, tname, ids);
    }
}

/// Build the bracketed `[tab:.., surface:.., <type>]` id list for a tab.
fn format_tab_ids(
    tid: Option<u64>,
    sid: Option<u64>,
    surfaces_arr: Option<&Vec<serde_json::Value>>,
    stype: Option<&str>,
) -> String {
    let mut ids = String::new();
    if let Some(t) = tid {
        ids.push_str(&format!("tab:{}", t));
    }
    if let Some(s) = sid {
        if !ids.is_empty() {
            ids.push_str(", ");
        }
        ids.push_str(&format!("surface:{}", s));
    } else if let Some(arr) = surfaces_arr {
        // SplitLayout: list all surface IDs
        for s in arr {
            if let Some(sv) = s.as_u64() {
                if !ids.is_empty() {
                    ids.push_str(", ");
                }
                ids.push_str(&format!("surface:{}", sv));
            }
        }
    }
    if let Some(t) = stype
        && t != "Terminal"
    {
        if !ids.is_empty() {
            ids.push_str(", ");
        }
        ids.push_str(t);
    }
    ids
}

/// Render a `to_tree_json_full` split tree as an indented ASCII tree.
///
/// `prefix` is the running indent for this node's line; `is_last` controls the
/// branch glyph (`└─` vs `├─`). `focused` marks the focused surface leaf.
///
/// Split node label carries direction · ratio · child positions:
/// `vertical (L|R)` = first child left, second right; `horizontal (T|B)` =
/// first top, second bottom (Vertical splits width, Horizontal splits height).
fn render_layout(
    node: &serde_json::Value,
    prefix: &str,
    is_last: bool,
    focused: Option<u64>,
    out: &mut Vec<String>,
) {
    let branch = if is_last { "└─ " } else { "├─ " };
    let child_prefix = format!("{}{}", prefix, if is_last { "   " } else { "│  " });

    match node.get("type").and_then(|v| v.as_str()) {
        Some("Split") => {
            let dir = node
                .get("direction")
                .and_then(|v| v.as_str())
                .unwrap_or("?");
            let ratio = node.get("ratio").and_then(|v| v.as_f64()).unwrap_or(0.5);
            let pct = (ratio * 100.0).round() as i64;
            let sides = if dir == "vertical" { "L|R" } else { "T|B" };
            out.push(format!(
                "{}{}{} ({}) {}:{}",
                prefix,
                branch,
                dir,
                sides,
                pct,
                100 - pct
            ));
            if let Some(first) = node.get("first") {
                render_layout(first, &child_prefix, false, focused, out);
            }
            if let Some(second) = node.get("second") {
                render_layout(second, &child_prefix, true, focused, out);
            }
        }
        _ => {
            // Leaf surface.
            let kind = node.get("kind").and_then(|v| v.as_str()).unwrap_or("?");
            let id = node.get("id").and_then(|v| v.as_u64());
            let focus_mark = if id.is_some() && id == focused {
                " *focus"
            } else {
                ""
            };
            match id {
                Some(i) => out.push(format!(
                    "{}{}surface:{} ({}){}",
                    prefix, branch, i, kind, focus_mark
                )),
                None => out.push(format!("{}{}({}){}", prefix, branch, kind, focus_mark)),
            }
        }
    }
}

/// `list workspaces` 한 행. id 를 함께 찍는 것이 핵심이다 — `terminal.spawn` 의
/// workspace-not-found 오류가 이 명령을 가리키므로, 여기서 `--workspace` 에 넣을
/// 값을 바로 얻을 수 있어야 한다. 표기는 `format_tree` 의 `(id:N)` 과 맞춘다.
///
/// `[mirror]` 는 이 워크스페이스가 원격을 attach 한 client mirror 라는 뜻이다
/// (구조 변경이 원격으로 forward 되므로 로컬과 동작이 다르다 —
/// `docs/features/remote-attach/index.md`). 번역하지 않고 고정 토큰으로 두는 것은
/// 이 파일의 `list` 계열 구조 출력이 전부 하드코딩 영어라는 컨벤션을 따른 것이다
/// (`(N panes)` · `Pane {id}` · `[ws:N name]`). 한 행에서 이것만 번역되면 오히려
/// 어긋나고, 에이전트가 파싱하는 식별 토큰이 로케일에 따라 흔들린다.
fn format_workspace_row(ws: &serde_json::Value) -> String {
    let id = ws.get("id").and_then(|v| v.as_u64()).unwrap_or(0);
    let name = ws.get("name").and_then(|v| v.as_str()).unwrap_or("?");
    let active = ws.get("active").and_then(|v| v.as_bool()).unwrap_or(false);
    let pane_count = ws.get("pane_count").and_then(|v| v.as_u64()).unwrap_or(0);
    let mirror = ws.get("mirror").and_then(|v| v.as_bool()).unwrap_or(false);
    let active_marker = if active { " *" } else { "" };
    let mirror_marker = if mirror { " [mirror]" } else { "" };
    format!("{name} (id:{id}){active_marker}{mirror_marker} ({pane_count} panes)")
}

fn format_workspace_list(result: &serde_json::Value) {
    if let Some(workspaces) = result.as_array() {
        for ws in workspaces {
            println!("{}", format_workspace_row(ws));
        }
    }
}

fn format_pane_list(result: &serde_json::Value) {
    if let Some(panes) = result.as_array() {
        for pane in panes {
            let pid = pane.get("id").and_then(|v| v.as_u64()).unwrap_or(0);
            let focused = pane
                .get("focused")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let tab_count = pane.get("tab_count").and_then(|v| v.as_u64()).unwrap_or(0);
            let ws_id = pane
                .get("workspace_id")
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            let ws_name = pane
                .get("workspace_name")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let marker = if focused { " *" } else { "" };
            println!(
                "Pane {}{} ({} tabs) [ws:{} {}]",
                pid, marker, tab_count, ws_id, ws_name
            );
        }
    }
}

fn format_notification_list(result: &serde_json::Value) {
    if let Some(notifs) = result.as_array() {
        if notifs.is_empty() {
            println!("No notifications");
            return;
        }
        for n in notifs {
            let title = n.get("title").and_then(|v| v.as_str()).unwrap_or("");
            let body = n.get("body").and_then(|v| v.as_str()).unwrap_or("");
            let read = n.get("read").and_then(|v| v.as_bool()).unwrap_or(false);
            let marker = if read { " " } else { "*" };
            if body.is_empty() {
                println!("{} {}", marker, title);
            } else {
                println!("{} {}: {}", marker, title, body);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{format_runner_summary, format_workspace_row, render_layout};
    use serde_json::json;

    #[test]
    fn runner_summary_stopped_with_no_pending_work_has_no_hint() {
        let s = format_runner_summary(&json!({
            "running": false, "crashed": false, "ready_count": 0, "running_count": 0,
        }));
        assert_eq!(s, "runner: stopped (ready=0 running=0)");
    }

    /// 결정 1 — "정지 상태 발견 가능성": runner 가 꺼져 있는데 대기 중인 task 가
    /// 있으면 재개 방법까지 안내한다.
    #[test]
    fn runner_summary_stopped_with_pending_work_hints_resume() {
        let s = format_runner_summary(&json!({
            "running": false, "crashed": false, "ready_count": 2, "running_count": 1,
        }));
        assert_eq!(
            s,
            "runner: stopped (ready=2 running=1) — pending work but no runner; \
             `agent task-run --action start` to resume"
        );
    }

    #[test]
    fn runner_summary_crashed_takes_precedence_over_running() {
        let s = format_runner_summary(&json!({
            "running": true, "crashed": true, "ready_count": 0, "running_count": 0,
        }));
        assert_eq!(s, "runner: crashed (ready=0 running=0)");
    }

    #[test]
    fn runner_summary_running_with_pending_work_has_no_hint() {
        let s = format_runner_summary(&json!({
            "running": true, "crashed": false, "ready_count": 3, "running_count": 1,
        }));
        assert_eq!(s, "runner: running (ready=3 running=1)");
    }

    /// `terminal.spawn` 의 workspace-not-found 오류가 이 명령을 가리킨다 — 행에서
    /// `--workspace` 에 넣을 값을 바로 얻을 수 있어야 한다.
    #[test]
    fn workspace_row_includes_id() {
        let row = format_workspace_row(&json!({
            "id": 3, "name": "project-a", "active": false, "pane_count": 2, "mirror": false,
        }));
        assert_eq!(row, "project-a (id:3) (2 panes)");
    }

    /// mirror 워크스페이스는 구조 변경이 원격으로 forward 되어 로컬과 동작이 다르다 —
    /// 에이전트가 조작 전에 판별할 수 있어야 한다.
    #[test]
    fn workspace_row_marks_mirror() {
        let row = format_workspace_row(&json!({
            "id": 7, "name": "remote-ws", "active": false, "pane_count": 1, "mirror": true,
        }));
        assert_eq!(row, "remote-ws (id:7) [mirror] (1 panes)");
    }

    /// active 표시는 id 뒤, mirror 앞. 둘은 서로 다른 축이라 함께 붙을 수 있다.
    #[test]
    fn workspace_row_shows_active_and_mirror_together() {
        let row = format_workspace_row(&json!({
            "id": 2, "name": "ws", "active": true, "pane_count": 4, "mirror": true,
        }));
        assert_eq!(row, "ws (id:2) * [mirror] (4 panes)");
    }

    /// 구버전 호스트 응답처럼 `mirror` 가 없으면 없는 것으로 본다(마커 없음) —
    /// 필드 추가가 구버전 CLI/호스트 조합을 깨지 않는다.
    #[test]
    fn workspace_row_without_mirror_field_defaults_to_local() {
        let row = format_workspace_row(&json!({
            "id": 1, "name": "Workspace 1", "active": true, "pane_count": 2,
        }));
        assert_eq!(row, "Workspace 1 (id:1) * (2 panes)");
    }

    #[test]
    fn render_layout_nested_split_tree() {
        // vertical(L|R) 60:40 → [left leaf 396, right = horizontal(T|B) 50:50 of 417/418]
        let layout = json!({
            "type": "Split",
            "direction": "vertical",
            "ratio": 0.6,
            "first": { "type": "Leaf", "id": 396, "kind": "terminal" },
            "second": {
                "type": "Split",
                "direction": "horizontal",
                "ratio": 0.5,
                "first": { "type": "Leaf", "id": 417, "kind": "terminal" },
                "second": { "type": "Leaf", "id": 418, "kind": "markdown" },
            },
        });
        let mut out = Vec::new();
        render_layout(&layout, "        ", true, Some(417), &mut out);
        let expected = vec![
            "        └─ vertical (L|R) 60:40",
            "           ├─ surface:396 (terminal)",
            "           └─ horizontal (T|B) 50:50",
            "              ├─ surface:417 (terminal) *focus",
            "              └─ surface:418 (markdown)",
        ];
        assert_eq!(out, expected);
    }
}
