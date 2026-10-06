//! 공용 판정 `tasty_utils::process::env_keys_to_strip` 가 시험 목록과 맞는지 확인한다.

use std::ffi::OsString;

use tasty_test_support::strip_env_keys::{KEPT, STRIPPED};
use tasty_utils::process::env_keys_to_strip;

/// Claude Code 세션 키와 CMUX_* 는 고르고 사용자 설정·TASTY_*·일반 키는 남긴다.
#[test]
fn claude_session_and_cmux_keys_are_stripped_but_others_are_kept() {
    let all = STRIPPED.iter().chain(KEPT).map(OsString::from);
    let picked = env_keys_to_strip(all);
    let expected: Vec<OsString> = STRIPPED.iter().map(OsString::from).collect();
    assert_eq!(picked, expected);
}
