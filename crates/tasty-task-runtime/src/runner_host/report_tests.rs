use super::MarkerScanner;

fn scan(chunks: &[&[u8]]) -> Vec<String> {
    let mut s = MarkerScanner::default();
    let mut out = Vec::new();
    for c in chunks {
        s.feed(c, |t| out.push(t.to_string()));
    }
    s.finish(|t| out.push(t.to_string()));
    out
}

#[test]
fn only_whole_marker_lines_are_picked_from_stderr() {
    assert_eq!(
        scan(&[
            b"warn\n::tasty-report::one\r\n  ::tasty-report::indented\n::tasty-re",
            b"port::two"
        ]),
        ["one", "two"]
    );
    assert_eq!(scan(&[b"::tasty-report::\n"]), [""]);
    assert!(scan(&[b"no markers\nat all"]).is_empty());
}

#[test]
fn an_overlong_marker_line_keeps_its_head_and_the_next_line_still_counts() {
    let mut long = b"::tasty-report::".to_vec();
    long.extend(std::iter::repeat_n(b'x', super::MARKER_LINE_CAP * 2));
    long.extend_from_slice(b"\n::tasty-report::next\n");
    let got = scan(&[&long]);
    assert_eq!(got.len(), 2);
    assert_eq!(
        got[0].len(),
        super::MARKER_LINE_CAP - "::tasty-report::".len()
    );
    assert_eq!(got[1], "next");
}

#[cfg(unix)]
mod run {
    use serde_json::{Value, json};
    use tasty_agent::TaskState;
    use tasty_agent::runner::RunnerLoop;
    use tasty_agent::task::report::{ReportAddress, ReportSource};
    use tasty_agent::task::{TaskGraphSpec, TaskStore};

    use super::super::super::tests::fresh_ctx;
    use super::super::super::*;

    fn store_op<R>(ctx: &RunnerContext, f: impl FnOnce(&mut TaskStore) -> R) -> R {
        ctx.with_memory(|mem| {
            let seq = ctx.agent_seq.clone();
            let mut store = TaskStore::new(mem, HOST_OWNER, seq.as_ref());
            f(&mut store)
        })
    }

    fn get(ctx: &RunnerContext, id: &str) -> Task {
        store_op(ctx, |s| s.get(1, &id.to_string()).unwrap().expect("task"))
    }

    fn run_to_end(ctx: &RunnerContext, graph: Value, id: &str) -> Task {
        let spec: TaskGraphSpec = serde_json::from_value(graph).unwrap();
        store_op(ctx, |s| s.submit_graph(1, spec, 0).unwrap());
        let mut runner = RunnerLoop::new(HostExecutor::new(ctx.clone()));
        for n in 0..200 {
            let snapshot = store_op(ctx, |s| s.list(1).unwrap());
            let (a, b) = (ctx.clone(), ctx.clone());
            runner.tick(
                1,
                10 + n,
                &snapshot,
                move |ws, id, st, n| store_op(&a, |s| s.set_state(ws, id, st, n).map(|_| ())),
                move |ws, id, c, n| store_op(&b, |s| s.complete(ws, id, c, n).map(|_| ())),
            );
            if get(ctx, id).state.is_terminal() {
                return get(ctx, id);
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        panic!("{id} did not finish");
    }

    fn stream(t: &Task, name: &str) -> String {
        t.typed_result
            .as_ref()
            .unwrap()
            .raw
            .execution
            .as_ref()
            .unwrap()[name]["text"]
            .as_str()
            .unwrap()
            .to_string()
    }

    /// run 은 자기 블록 주소를 환경 변수로 받고, stderr 의 표지 줄은 블록에 들어가며 stderr 에도 남는다.
    #[test]
    fn a_run_gets_its_address_and_its_stderr_markers_reach_its_block() {
        let (_td, ctx) = fresh_ctx();
        let script = "printf '%s' \"$TASTY_TASK_REPORT\"; \
                      echo 'plain' >&2; echo '::tasty-report::checked 3 files' >&2; \
                      printf '::tasty-report::no newline' >&2";
        let t = run_to_end(
            &ctx,
            json!({"contract_version": 2, "tasks": [
                {"id": "r", "command": {"kind": "run", "workspace_id": 1,
                    "command": ["sh", "-c", script]}}]}),
            "r",
        );
        assert_eq!(t.state, TaskState::Succeeded);
        let addr = ReportAddress::parse(&stream(&t, "stdout")).expect("address");
        assert_eq!(addr.task_id, "r");
        assert_eq!(addr.attempt, 1);
        assert_eq!(addr.source, ReportSource::Run);
        assert_eq!(addr.token, t.report_token.as_ref().unwrap().token);
        assert!(stream(&t, "stderr").contains("::tasty-report::checked 3 files"));
        let blocks = store_op(&ctx, |s| s.report_blocks(&t).unwrap());
        let entries: Vec<_> = blocks[0]
            .entries
            .iter()
            .map(|e| (e.source, e.text.as_str()))
            .collect();
        assert_eq!(
            entries,
            [
                (ReportSource::StderrMarker, "checked 3 files"),
                (ReportSource::StderrMarker, "no newline")
            ]
        );
    }

    /// reduce 의 custom 셸도 같은 블록 주소를 받는다.
    #[test]
    fn a_custom_reduce_shell_gets_the_address_of_its_own_block() {
        let (_td, ctx) = fresh_ctx();
        let t = run_to_end(
            &ctx,
            json!({"contract_version": 2, "tasks": [
                {"id": "a", "command": {"kind": "run", "workspace_id": 1, "command": ["true"]}},
                {"id": "m", "command": {"kind": "reduce", "inputs": ["a"], "strategy": {
                    "kind": "custom",
                    "command": "printf '\"%s\"' \"$TASTY_TASK_REPORT\""}},
                 "output_schema": {"type": "string"}}]}),
            "m",
        );
        assert_eq!(t.state, TaskState::Succeeded, "{:?}", t.typed_result);
        let out = t.typed_result.as_ref().unwrap().output.to_wire();
        let addr = ReportAddress::parse(out.as_str().unwrap()).expect("address");
        assert_eq!(addr.task_id, "m");
        assert_eq!(addr.source, ReportSource::ReduceCustom);
    }
}
