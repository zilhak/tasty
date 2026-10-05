//! 작업 출력 캡처와 실행 결과의 직렬화·저장·조회.

use super::*;

/// 출력 뒤쪽의 실패 요약을 보존한다. 앞부분의 오류는 빠질 수 있으며 ANSI escape는 유지한다.
/// 두 tail의 JSON 이스케이프를 고려해 기본 저장소 항목 상한보다 작게 잡았다. 더 낮은 설정에서는 저장이 실패할 수 있다.
pub(super) const CAPTURE_TAIL_CAP: usize = 64 * 1024;

/// 스트림 뒤쪽의 제한된 바이트와 잘린 양. 읽기 오류도 종료로 처리해 출력이 불완전할 수 있다.
#[derive(Debug, Default)]
pub(crate) struct DrainedStream {
    pub(super) data: Vec<u8>,
    pub(super) truncated: bool,
    pub(super) dropped_bytes: u64,
}

impl DrainedStream {
    pub(super) fn text(&self) -> String {
        String::from_utf8_lossy(&self.data).into_owned()
    }

    pub(super) fn to_json(&self) -> serde_json::Value {
        json!({
            "text": self.text(),
            "truncated": self.truncated,
            "dropped_bytes": self.dropped_bytes,
        })
    }
}

/// task 오류에 저장할 poll 응답 요약의 바이트 상한.
pub(super) const POLL_FAILURE_SUMMARY_CAP: usize = 2 * 1024;

/// Failed는 문자열 하나만 전달하므로 응답 JSON을 길이 제한한 진단에 포함한다.
pub(super) fn summarize_poll_response(resp: &serde_json::Value) -> String {
    let mut text = resp.to_string();
    if text.len() > POLL_FAILURE_SUMMARY_CAP {
        // UTF-8 문자 중간을 자르지 않는다.
        let mut cut = POLL_FAILURE_SUMMARY_CAP;
        while cut > 0 && !text.is_char_boundary(cut) {
            cut -= 1;
        }
        text.truncate(cut);
        text.push_str("...(truncated)");
    }
    text
}

/// EOF나 읽기 오류까지 읽고 마지막 CAPTURE_TAIL_CAP 바이트만 남긴다.
/// 파이프가 찬 자식의 종료를 기다리는 교착을 피하려고 child.wait와 별도 스레드에서 실행한다.
pub(super) fn drain_capped<R: std::io::Read>(mut reader: R) -> DrainedStream {
    let mut data = Vec::with_capacity(CAPTURE_TAIL_CAP);
    let mut dropped_bytes: u64 = 0;
    let mut chunk = [0u8; 8192];
    loop {
        match reader.read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => {
                data.extend_from_slice(&chunk[..n]);
                if data.len() > CAPTURE_TAIL_CAP {
                    let excess = data.len() - CAPTURE_TAIL_CAP;
                    data.drain(0..excess);
                    dropped_bytes += excess as u64;
                }
            }
            Err(_) => break,
        }
    }
    DrainedStream {
        truncated: dropped_bytes > 0,
        dropped_bytes,
        data,
    }
}

/// 성공은 구조화된 출력으로, 실패는 종료 코드와 출력 tail을 포함한 오류 문자열로 반환한다.
pub(crate) fn shell_outcome_from_status(
    pid: u32,
    code: Option<i32>,
    success: bool,
    stdout: DrainedStream,
    stderr: DrainedStream,
) -> PollOutcome {
    if success {
        PollOutcome::Done(TaskResult {
            exit_code: code,
            output: Some(json!({
                "pid": pid,
                "stdout": stdout.to_json(),
                "stderr": stderr.to_json(),
            })),
            error: None,
        })
    } else {
        let stdout_note = if stdout.truncated {
            format!(" (truncated, {} bytes dropped)", stdout.dropped_bytes)
        } else {
            String::new()
        };
        let stderr_note = if stderr.truncated {
            format!(" (truncated, {} bytes dropped)", stderr.dropped_bytes)
        } else {
            String::new()
        };
        PollOutcome::Failed(format!(
            "Run exited non-zero: code={:?}\n--- stdout{stdout_note} ---\n{}\n--- stderr{stderr_note} ---\n{}",
            code,
            stdout.text(),
            stderr.text(),
        ))
    }
}

/// 자식 종료와 출력 수집 뒤 결과를 저장한다. 실패하면 경고하지만 cell에는 결과를 계속 전달한다.
pub(crate) fn persist_run_result(
    memory: &Arc<Mutex<dyn MemoryStorage>>,
    workspace_id: u32,
    task_id: &str,
    outcome: &PollOutcome,
) {
    let value = MemoryValue::Json(run_outcome_to_value(outcome));
    let res = {
        let mut guard = tasty_utils::poison::recover_mutex(
            memory.lock(),
            "agent runner memory",
            &MEMORY_POISON_REPORTED,
        );
        guard.put(
            HOST_OWNER,
            &Scope::Workspace(workspace_id),
            &run_result_key(task_id),
            &value,
            &PutOpts::default(),
        )
    };
    if let Err(e) = res {
        tracing::warn!("persist run_result {task_id}: {e}");
    }
}

pub(crate) fn load_run_result(
    ctx: &RunnerContext,
    workspace_id: u32,
    task_id: &str,
) -> Option<PollOutcome> {
    ctx.with_memory(|mem| {
        let entry = mem
            .get(&Scope::Workspace(workspace_id), &run_result_key(task_id))
            .ok()??;
        match entry.value {
            MemoryValue::Json(v) => run_outcome_from_value(&v),
            _ => None,
        }
    })
}

pub(crate) fn evict_run_result(ctx: &RunnerContext, workspace_id: u32, task_id: &str) {
    let res = ctx.with_memory(|mem| {
        mem.delete(
            HOST_OWNER,
            &Scope::Workspace(workspace_id),
            &run_result_key(task_id),
            None,
        )
    });
    if let Err(e) = res {
        tracing::warn!("evict run_result {task_id}: {e}");
    }
}

/// executor 밖의 삭제·GC도 정리할 수 있도록 task와 workspace ID로 handle을 지운다.
pub(crate) fn evict_handle_key(ctx: &RunnerContext, workspace_id: u32, task_id: &str) {
    let res = ctx.with_memory(|mem| {
        mem.delete(
            HOST_OWNER,
            &Scope::Workspace(workspace_id),
            &handle_key(task_id),
            None,
        )
    });
    if let Err(e) = res {
        tracing::warn!("evict handle {task_id}: {e}");
    }
}

/// 정상 종료를 거치지 않고 task를 삭제해도 handle과 실행 결과 키를 함께 지운다.
pub(crate) fn evict_task_side_keys(ctx: &RunnerContext, workspace_id: u32, task_id: &str) {
    evict_handle_key(ctx, workspace_id, task_id);
    evict_run_result(ctx, workspace_id, task_id);
}

pub(super) fn run_outcome_to_value(outcome: &PollOutcome) -> serde_json::Value {
    match outcome {
        PollOutcome::Done(r) => json!({
            "kind": "done",
            "exit_code": r.exit_code,
            "output": r.output,
            "error": r.error,
        }),
        PollOutcome::Failed(e) => json!({
            "kind": "failed",
            "error": e,
        }),
        PollOutcome::Active => json!({ "kind": "active" }),
    }
}

pub(super) fn run_outcome_from_value(v: &serde_json::Value) -> Option<PollOutcome> {
    match v.get("kind")?.as_str()? {
        "done" => {
            let exit_code = v
                .get("exit_code")
                .and_then(|x| x.as_i64())
                .map(|x| x as i32);
            let output = v.get("output").cloned();
            let error = v
                .get("error")
                .and_then(|x| x.as_str())
                .map(|s| s.to_string());
            Some(PollOutcome::Done(TaskResult {
                exit_code,
                output,
                error,
            }))
        }
        "failed" => Some(PollOutcome::Failed(v.get("error")?.as_str()?.to_string())),
        _ => None,
    }
}
