//! 결과를 싣기 전 task 레코드의 크기 상한.
//!
//! task 레코드는 memory 값 하나라 직렬화가 memory 값 상한(기본 1 MiB)을 넘으면 저장되지 않는다.
//! 결과가 너무 크면 러너가 출력을 뺀 짧은 실패로 바꿔 기록하는데, 그 짧은 실패마저 들어가지
//! 않으면 같은 보고를 계속 다시 내며 task 가 Running 에 머문다. 그래서 결과 없이도 레코드가
//! 커지는 몫(정의, 실행 직전에 붙는 입력 snapshot, 치환한 command)을 이 상한으로 막고, 남은
//! 몫을 짧은 실패 결과에 남긴다.

use super::Task;

/// 결과를 싣기 전 task 레코드의 직렬화 크기 상한(바이트). memory 값 상한 1 MiB 에서 짧은 실패
/// 결과(사유 4 KiB 의 사본 셋, 접수 응답 머리, 회차 기록)의 몫 256 KiB 를 남긴 크기다.
pub const MAX_RECORD_BEFORE_RESULT_BYTES: usize = 768 * 1024;

/// 레코드의 직렬화 크기가 [`MAX_RECORD_BEFORE_RESULT_BYTES`] 를 넘으면 사유를 돌려준다. `what` 은 레코드를 키운 것을 말한다.
pub fn check(task: &Task, what: &str) -> Result<(), String> {
    let limit = MAX_RECORD_BEFORE_RESULT_BYTES;
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
