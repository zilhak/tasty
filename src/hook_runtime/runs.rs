//! 수동 `hook_handler.dispatch` 실행의 최종 결과. 접수 응답의 `run` 값으로 `hook_handler.result`가 조회한다.
//! 작업 스레드가 결과를 쓰고 IPC 처리가 읽으므로 프로세스 전역 잠금 안에 둔다. 최근 [`KEEP`]건만 보관한다.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};

use serde_json::{Value, json};

use crate::hook_handler::exec::ShellEvent;
use crate::hook_handler::outcome::{SequenceOutcome, Verdict};

/// 보관할 실행 수. 넘으면 가장 오래된 기록부터 지우며 지운 실행은 조회하면 알 수 없는 run이 된다.
pub(crate) const KEEP: usize = 128;

#[derive(Debug, Clone)]
struct Run {
    id: u64,
    handler: String,
    action: &'static str,
    /// None이면 진행 중이다.
    state: Option<&'static str>,
    result: String,
    steps: Option<Value>,
    exit_code: Option<i32>,
}

impl Run {
    fn to_json(&self) -> Value {
        let mut v = json!({
            "run": self.id,
            "id": self.handler,
            "action": self.action,
            "done": self.state.is_some(),
            "state": self.state.unwrap_or("running"),
            "result": self.result,
        });
        if let Some(steps) = &self.steps {
            v["steps"] = steps.clone();
        }
        if self.action == ACTION_SHELL {
            v["exit_code"] = json!(self.exit_code);
        }
        v
    }
}

pub(crate) const ACTION_SEQUENCE: &str = "ipc_sequence";
pub(crate) const ACTION_SHELL: &str = "shell_command";

static NEXT: AtomicU64 = AtomicU64::new(1);
static POISONED: AtomicBool = AtomicBool::new(false);

fn runs() -> std::sync::MutexGuard<'static, VecDeque<Run>> {
    static RUNS: OnceLock<Mutex<VecDeque<Run>>> = OnceLock::new();
    crate::poison::recover_mutex(
        RUNS.get_or_init(Default::default).lock(),
        "hook dispatch runs",
        &POISONED,
    )
}

fn update(id: u64, f: impl FnOnce(&mut Run)) {
    if let Some(run) = runs().iter_mut().find(|r| r.id == id) {
        f(run);
    }
}

/// 진행 중인 실행을 만들고 run 값을 돌려준다.
pub(crate) fn begin(handler: &str, action: &'static str) -> u64 {
    let id = NEXT.fetch_add(1, Ordering::Relaxed);
    let mut runs = runs();
    while runs.len() >= KEEP {
        runs.pop_front();
    }
    runs.push_back(Run {
        id,
        handler: handler.to_string(),
        action,
        state: None,
        result: "running".into(),
        steps: None,
        exit_code: None,
    });
    id
}

/// 실행기 대기열에 넣지 못해 동기 오류로 답한 실행은 조회 대상에서 지운다.
pub(crate) fn forget(id: u64) {
    runs().retain(|r| r.id != id);
}

pub(crate) fn finish_sequence(id: u64, outcome: &SequenceOutcome) {
    let verdict = outcome.verdict();
    update(id, |run| {
        run.state = Some(verdict.as_str());
        run.result = outcome.result_line();
        run.steps = Some(outcome.steps_json());
    });
}

/// 셸 핸들러는 프로세스를 띄웠는지로 판정한다. 종료 코드는 정보로만 싣는다.
pub(crate) fn shell_event(id: u64, event: ShellEvent) {
    update(id, |run| match event {
        ShellEvent::Started => {
            run.state = Some(Verdict::Succeeded.as_str());
            run.result = "process started".into();
        }
        ShellEvent::NotStarted(error) => {
            run.state = Some(Verdict::Failed.as_str());
            run.result = format!("process could not start: {error}");
        }
        ShellEvent::Exited(code) => {
            run.exit_code = code;
            run.result = match code {
                Some(code) => format!("process started and exited with code {code}"),
                None => "process started and exited without a code".into(),
            };
        }
    });
}

pub(crate) fn lookup(id: u64) -> Option<Value> {
    runs().iter().find(|r| r.id == id).map(Run::to_json)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hook_handler::outcome::{StepResult, StepStatus};

    #[test]
    fn a_run_moves_from_running_to_its_final_state() {
        let id = begin("user/t", ACTION_SEQUENCE);
        assert_eq!(lookup(id).unwrap()["state"], "running");
        assert_eq!(lookup(id).unwrap()["done"], false);
        let outcome = SequenceOutcome {
            steps: vec![
                StepResult {
                    index: 0,
                    method: "a.ok".into(),
                    status: StepStatus::Ok,
                },
                StepResult {
                    index: 1,
                    method: "b.missing".into(),
                    status: StepStatus::Failed("rpc error -32601: Method not found".into()),
                },
            ],
        };
        finish_sequence(id, &outcome);
        let got = lookup(id).unwrap();
        assert_eq!(got["state"], "partial");
        assert_eq!(got["done"], true);
        assert_eq!(got["steps"][1]["status"], "failed");
        assert!(got["result"].as_str().unwrap().contains("b.missing"));
    }

    #[test]
    fn a_shell_run_is_judged_by_starting_not_by_exit_code() {
        let ok = begin("user/s", ACTION_SHELL);
        shell_event(ok, ShellEvent::Started);
        shell_event(ok, ShellEvent::Exited(Some(3)));
        let got = lookup(ok).unwrap();
        assert_eq!(got["state"], "succeeded");
        assert_eq!(got["exit_code"], 3);
        let bad = begin("user/s", ACTION_SHELL);
        shell_event(bad, ShellEvent::NotStarted("No such file".into()));
        assert_eq!(lookup(bad).unwrap()["state"], "failed");
        forget(bad);
        assert!(lookup(bad).is_none());
    }
}
