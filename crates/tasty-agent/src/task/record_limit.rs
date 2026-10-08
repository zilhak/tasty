//! 결과를 싣기 전 task 레코드의 크기 상한.
//!
//! task 레코드는 memory 값 하나라 직렬화가 memory 값 상한(기본 1 MiB)을 넘으면 저장되지 않는다.
//! 결과가 너무 크면 러너가 출력을 뺀 짧은 실패로 바꿔 기록하는데, 그 짧은 실패마저 들어가지
//! 않으면 같은 보고를 계속 다시 내며 task 가 Running 에 머문다. 그래서 결과 없이도 레코드가
//! 커지는 몫(정의, 실행 직전에 붙는 입력 snapshot, 치환한 command)을 이 상한으로 막고, 남은
//! 몫을 짧은 실패 결과에 남긴다.

use super::Task;

/// 결과를 싣기 전 task 레코드의 직렬화 크기 상한(바이트)의 최댓값. memory 값 상한이 기본값
/// 1 MiB 이면 이 값이 상한이다.
pub const MAX_RECORD_BEFORE_RESULT_BYTES: usize = 768 * 1024;

/// memory 값 상한에서 짧은 실패 결과(사유 4 KiB 의 사본 셋, 접수 응답 머리, 회차 기록)에 남기는
/// 몫(바이트).
pub const RESULT_ROOM_BYTES: usize = 256 * 1024;

/// memory 값 상한이 작아 [`RESULT_ROOM_BYTES`] 를 남기면 이보다 작아질 때 쓰는 상한(바이트).
/// 이때는 짧은 실패가 들어갈 몫을 보장하지 못한다.
pub const MIN_RECORD_BEFORE_RESULT_BYTES: usize = 64 * 1024;

/// memory 값 상한(`entry_max_bytes`)에 맞춘 결과 전 레코드 상한.
/// `min(MAX_RECORD_BEFORE_RESULT_BYTES, entry_max_bytes - RESULT_ROOM_BYTES)` 이고, 그 값이
/// [`MIN_RECORD_BEFORE_RESULT_BYTES`] 보다 작으면 그 최솟값이다.
pub fn limit_for(entry_max_bytes: u64) -> usize {
    let entry = usize::try_from(entry_max_bytes).unwrap_or(usize::MAX);
    entry.saturating_sub(RESULT_ROOM_BYTES).clamp(
        MIN_RECORD_BEFORE_RESULT_BYTES,
        MAX_RECORD_BEFORE_RESULT_BYTES,
    )
}

/// memory 값 상한이 결과 몫을 남기지 못할 만큼 작은가.
pub fn leaves_no_result_room(entry_max_bytes: u64) -> bool {
    let entry = usize::try_from(entry_max_bytes).unwrap_or(usize::MAX);
    entry.saturating_sub(RESULT_ROOM_BYTES) < MIN_RECORD_BEFORE_RESULT_BYTES
}

/// 부팅 때 한 번 부른다. 몫을 남기지 못하는 설정이면 경고를 남긴다.
pub fn warn_if_no_result_room(entry_max_bytes: u64) {
    if leaves_no_result_room(entry_max_bytes) {
        tracing::warn!(
            entry_max_bytes,
            limit = MIN_RECORD_BEFORE_RESULT_BYTES,
            "memory entry limit leaves no room for a task result; task records before their \
             result are capped at the minimum and a too large result may not be recorded"
        );
    }
}

/// 상한을 넘는 정의를 시작하지 않고 실패로 끝낼 때의 사유. memory 값 상한에 거의 닿은 레코드도
/// 실패를 기록할 수 있도록 크기·id 없이 고정하고 상태에만 싣는다(자세한 크기는 로그에 남긴다).
pub const REFUSED_TO_START: &str =
    "task record too large: the definition leaves no room for a result";

/// 레코드의 직렬화 크기가 `limit` 를 넘으면 사유를 돌려준다. `what` 은 레코드를 키운 것을 말한다.
pub fn check(task: &Task, what: &str, limit: usize) -> Result<(), String> {
    let size = serde_json::to_vec(task).map_or(usize::MAX, |b| b.len());
    if size <= limit {
        return Ok(());
    }
    Err(format!(
        "task record too large: task {}: {what} makes the task record {size} bytes, over the \
         {limit} byte limit for a task before its result; keep large data out of the task (store \
         it elsewhere and pass a reference)",
        task.id
    ))
}

#[cfg(test)]
#[path = "record_limit_tests.rs"]
mod tests;
