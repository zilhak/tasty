//! `continue-checklist` 세션 프로필이 싣는 `Stop` 훅 — `tasty claude checklist-hook`.
//!
//! `Stop` 훅이 stdout 에 `{"decision":"block","reason":"<체크리스트>"}` 를 내면
//! Claude Code 는 대화 종료를 막고 `reason` 을 지시로 주입해 이어서 실행한다.
//! **전역 설치 대상이 아니다** — `install.rs::MANAGED_HOOKS` 에 넣지 않고,
//! `continue-checklist` 프로필(`profile.rs` 의 host 기본 제공 프로필)로 부착된
//! 세션에서만 등록된다. 전역 `tasty claude hook stop` 의 기존 stdout 형태(무출력)는
//! 이 모듈과 무관하게 그대로 유지된다.
//!
//! ## 종료 조건 (4 분기 + 1 폴백)
//!
//! 1. 저장된 `prompt_id` 가 이번 요청과 다르면(또는 상태가 없으면) → 새 사용자 턴 →
//!    라운드 카운터를 0 으로 본다
//! 2. `last_assistant_message` 에 [`SENTINEL`] 이 있으면 → 통과(주 종료 경로 —
//!    모델이 스스로 "다 끝났다" 선언)
//! 3. 라운드 수가 상한 이상이면 → 통과(백스톱 — Claude Code 자체에는 무한 block
//!    루프를 끊는 장치가 없음을 실측 확인했으므로 훅이 직접 끊어야 한다)
//! 4. 그 외 → block, 라운드 +1
//!
//! + `prompt_id` 가 아예 없는 요청은 턴 경계를 판단할 수 없으므로 상태를 건드리지
//!   않고 이번 발화만 통과시킨다(안전 폴백, [`decide`] 밖에서 처리).
//!
//! 라운드 상태는 `TASTY_PLUGIN_DATA_DIR/checklist/rounds/<session_id>.json` 에
//! **session_id 로 키잉**해 저장한다 — 자식 세션을 동시에 여러 개 띄우므로 전역
//! 카운터 하나를 공유하면 서로의 라운드를 깎는다. `SessionEnd` 에서 정리한다
//! (`hook.rs` 의 session-end 분기가 [`remove_state_for_session`] 을 호출).
//!
//! ## 게이트
//!
//! 마커 파일(`TASTY_PLUGIN_DATA_DIR/checklist/enabled.marker`)이 있어야 발동한다.
//! 훅 등록 자체(프로필 부착)는 세션 기동 시점 스냅샷이라 세션을 끊지 않고는 뗄 수
//! 없지만, 마커 파일은 존재 여부만 보므로 `rm`/`touch` 한 번으로 재기동 없이
//! 즉시 켜고 끌 수 있다.

use std::path::{Path, PathBuf};

use serde_json::{Value, json};
use tasty_plugin_sdk::{HostHandle, IpcMethodError, i18n::Translator};
use tracing::warn;

/// 종료 선언 센티넬. `last_assistant_message` 의 substring 매칭으로 찾는다(도구
/// 호출 없이도 모델이 텍스트만으로 종료를 선언할 수 있게). 흔한 표현과 겹치면
/// 모델이 일을 안 하고 반사적으로 뱉는 실패 모드가 생기므로, 일부러 사람 산문에
/// 나타나지 않을 형태(대문자 + 대괄호 이중 래핑 + 하이픈)를 쓴다.
pub(crate) const SENTINEL: &str = "[[TASTY-CHECKLIST-DONE]]";

/// 상한 설정 항목의 storage key. `tasty-plugin.toml` 의
/// `[[contributes.settings_pages.items]]` 선언과 짝을 맞춘다.
const ROUND_LIMIT_STORAGE_KEY: &str = "continue_checklist_round_limit";

/// 설정 조회 실패 시(호출 오류·미설정) 쓰는 기본 상한. 센티넬이 반사적으로
/// 나오지 않는 한 3 라운드면 "이어서 몇 단계 더 진행"에 충분하고, 그래도 안
/// 끝나면 더 도는 것보다 백스톱으로 끊는 편이 안전하다는 판단.
const DEFAULT_ROUND_LIMIT: u32 = 3;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RoundState {
    prompt_id: String,
    rounds: u32,
}

impl RoundState {
    /// 이 crate 는 `serde` derive 를 직접 의존하지 않는다(`profile.rs` 와 동일
    /// 방침 — `serde_json` 만으로 충분). 필드가 2개뿐이라 수동 (역)직렬화 비용이
    /// 낮다.
    fn to_json(&self) -> Value {
        json!({ "prompt_id": self.prompt_id, "rounds": self.rounds })
    }

    fn from_json(v: &Value) -> Option<Self> {
        let prompt_id = v.get("prompt_id")?.as_str()?.to_string();
        let rounds = v.get("rounds")?.as_u64()?;
        Some(Self {
            prompt_id,
            rounds: rounds as u32,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Decision {
    Pass,
    Block { rounds: u32 },
}

/// 순수 결정 함수 — 단위 테스트 대상. `prompt_id` 는 호출자가 이미 존재를
/// 확인한 뒤 넘긴다(없는 경우는 이 함수 밖에서 별도 폴백으로 처리 — 위 모듈
/// doc 참고).
pub(crate) fn decide(
    stored: Option<&RoundState>,
    prompt_id: &str,
    last_assistant_message: &str,
    round_limit: u32,
) -> Decision {
    // 1) 턴 경계: 저장된 prompt_id 와 다르면(또는 상태 자체가 없으면) 새 턴 → 0.
    let current_rounds = match stored {
        Some(s) if s.prompt_id == prompt_id => s.rounds,
        _ => 0,
    };
    // 2) 센티넬 — 주 종료 경로.
    if last_assistant_message.contains(SENTINEL) {
        return Decision::Pass;
    }
    // 3) 상한 — 백스톱.
    if current_rounds >= round_limit {
        return Decision::Pass;
    }
    // 4) 그 외 — block.
    Decision::Block {
        rounds: current_rounds + 1,
    }
}

fn checklist_dir(data_dir: &Path) -> PathBuf {
    data_dir.join("checklist")
}

fn rounds_dir(data_dir: &Path) -> PathBuf {
    checklist_dir(data_dir).join("rounds")
}

fn state_file(data_dir: &Path, session_id: &str) -> PathBuf {
    rounds_dir(data_dir).join(format!("{session_id}.json"))
}

fn marker_file(data_dir: &Path) -> PathBuf {
    checklist_dir(data_dir).join("enabled.marker")
}

/// 마커 파일 존재 여부 — 게이트. `data_dir` 이 없으면(비정상 기동) 안전하게 미발동.
fn marker_present(data_dir: Option<&Path>) -> bool {
    data_dir.map(|d| marker_file(d).is_file()).unwrap_or(false)
}

fn no_data_dir_err(tr: &Translator) -> IpcMethodError {
    IpcMethodError::new(tr.t("claude.checklist.no_data_dir"))
}

fn io_err(tr: &Translator, e: std::io::Error) -> IpcMethodError {
    IpcMethodError::new(tr.t_fmt("claude.checklist.io_error", &e.to_string()))
}

/// `claude.checklist_enable` IPC 진입점 — 마커 파일을 만들어 게이트를 켠다(raw
/// `touch` 대신 제어된 진입점, CLI 배선은 `tasty-plugin.toml` 의
/// `checklist-enable`, `no_args` 그룹 재사용).
pub(crate) fn handle_enable(
    data_dir: Option<&Path>,
    tr: &Translator,
) -> Result<Value, IpcMethodError> {
    let dir = data_dir.ok_or_else(|| no_data_dir_err(tr))?;
    std::fs::create_dir_all(checklist_dir(dir)).map_err(|e| io_err(tr, e))?;
    std::fs::write(marker_file(dir), "").map_err(|e| io_err(tr, e))?;
    Ok(json!({ "enabled": true }))
}

/// `claude.checklist_disable` IPC 진입점 — 마커 파일을 지워 게이트를 끈다. 이미
/// 꺼져 있어도(마커 없음) 성공으로 취급한다(멱등).
pub(crate) fn handle_disable(
    data_dir: Option<&Path>,
    tr: &Translator,
) -> Result<Value, IpcMethodError> {
    let dir = data_dir.ok_or_else(|| no_data_dir_err(tr))?;
    match std::fs::remove_file(marker_file(dir)) {
        Ok(()) => Ok(json!({ "enabled": false })),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(json!({ "enabled": false })),
        Err(e) => Err(io_err(tr, e)),
    }
}

/// `claude.checklist_status` IPC 진입점 — 마커 파일 존재 여부 조회.
pub(crate) fn handle_status(data_dir: Option<&Path>) -> Result<Value, IpcMethodError> {
    Ok(json!({ "enabled": marker_present(data_dir) }))
}

fn read_state(data_dir: &Path, session_id: &str) -> Option<RoundState> {
    let text = std::fs::read_to_string(state_file(data_dir, session_id)).ok()?;
    let value: Value = serde_json::from_str(&text).ok()?;
    RoundState::from_json(&value)
}

fn write_state(data_dir: &Path, session_id: &str, state: &RoundState) {
    let dir = rounds_dir(data_dir);
    if let Err(e) = std::fs::create_dir_all(&dir) {
        warn!(
            "checklist: failed to create state dir {}: {e}",
            dir.display()
        );
        return;
    }
    let text = state.to_json().to_string();
    let path = state_file(data_dir, session_id);
    if let Err(e) = std::fs::write(&path, text) {
        warn!(
            "checklist: failed to write round state {}: {e}",
            path.display()
        );
    }
}

fn remove_state(data_dir: &Path, session_id: &str) {
    let path = state_file(data_dir, session_id);
    if let Err(e) = std::fs::remove_file(&path)
        && e.kind() != std::io::ErrorKind::NotFound
    {
        warn!(
            "checklist: failed to remove round state {}: {e}",
            path.display()
        );
    }
}

/// `SessionEnd` 훅(`hook.rs`)이 호출하는 정리 경로 — 세션이 끝나면 그 session_id
/// 의 라운드 상태 파일을 지운다(누수 방지). 상태가 아예 없었어도(체크리스트가
/// 걸리지 않은 세션) no-op.
pub(crate) fn remove_state_for_session(data_dir: Option<&Path>, session_id: &str) {
    if let Some(dir) = data_dir {
        remove_state(dir, session_id);
    }
}

fn fetch_round_limit(host: &HostHandle) -> u32 {
    host.call(
        "settings.get_plugin_setting",
        json!({ "storage_key": ROUND_LIMIT_STORAGE_KEY }),
    )
    .ok()
    .and_then(|v| v.get("value").and_then(|v| v.as_f64()))
    .map(|f| f.max(1.0).round() as u32)
    .unwrap_or(DEFAULT_ROUND_LIMIT)
}

/// `claude.checklist_hook` IPC 진입점. stdin JSON(Claude Code 의 Stop 페이로드)에서
/// `stdin_field` 매핑으로 채워진 params 를 받는다 — CLI 배선은 `tasty-plugin.toml`
/// 의 `checklist_hook_args`.
///
/// 실패 모드는 전부 **조용히 통과**(에러를 반환하지 않고 무출력 `{}`) — 이
/// 훅이 잘못 block 을 걸면 세션이 종료 불가 상태로 빠지므로, 불확실할 때는
/// 항상 통과 쪽으로 폴백한다:
/// - 마커 파일 없음 → 통과(게이트 꺼짐)
/// - `session_id` 없음 → 통과(상태를 키잉할 수 없음)
/// - `prompt_id` 없음 → 통과, 상태도 건드리지 않음(턴 경계를 판단할 수 없어
///   기존 라운드 카운터를 섣불리 리셋/증가시키지 않는다 — 별도 판단, 문서에
///   명시된 4 분기 밖의 폴백)
pub(crate) fn handle_checklist_hook(
    host: &HostHandle,
    data_dir: Option<&Path>,
    checklist_body: &str,
    params: &Value,
) -> Result<Value, IpcMethodError> {
    if !marker_present(data_dir) {
        return Ok(json!({}));
    }
    // marker_present(Some(_)) 를 통과했으므로 data_dir 은 반드시 Some 이지만,
    // 방어적으로 다시 한번 확인한다(향후 marker_present 구현이 바뀌어도 안전).
    let Some(data_dir) = data_dir else {
        return Ok(json!({}));
    };

    let Some(session_id) = params
        .get("session_id")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
    else {
        return Ok(json!({}));
    };
    let Some(prompt_id) = params
        .get("prompt_id")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
    else {
        return Ok(json!({}));
    };
    let last_message = params
        .get("last_assistant_message")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    let stored = read_state(data_dir, session_id);

    // `stop_hook_active` — 값싼 sanity check 로만 쓴다(주 판정은 prompt_id 비교가
    // 포섭하므로 관여하지 않는다). CLI 인자는 `"true"`/`"false"` 문자열로 온다
    // (stdin 이 boolean 이라도 `stdin_json`+string-typed arg 경로를 거치며 JSON
    // Display 문자열이 됨 — 매니페스트가 이 필드를 string 타입으로 선언한 이유이기도
    // 하다: bool 타입 인자는 clap `SetTrue` 라 stdin 값과 결합할 수 없다).
    if let Some(active) = params.get("stop_hook_active").and_then(|v| v.as_str()) {
        let expected = stored
            .as_ref()
            .is_some_and(|s| s.prompt_id == prompt_id && s.rounds > 0);
        let active_bool = active == "true";
        if active_bool != expected {
            warn!(
                "checklist: stop_hook_active={active} disagrees with stored round state for session {session_id} (sanity check only, not acted on)"
            );
        }
    }

    let round_limit = fetch_round_limit(host);
    match decide(stored.as_ref(), prompt_id, last_message, round_limit) {
        Decision::Pass => {
            remove_state(data_dir, session_id);
            Ok(json!({}))
        }
        Decision::Block { rounds } => {
            write_state(
                data_dir,
                session_id,
                &RoundState {
                    prompt_id: prompt_id.to_string(),
                    rounds,
                },
            );
            Ok(json!({ "decision": "block", "reason": checklist_body }))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_translator() -> Translator {
        let lang_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("lang");
        Translator::load(&lang_dir, "en")
    }

    // ── lang 파일 SENTINEL 크로스체크 ──
    //
    // `lang/{en,ko,ja}.toml` 의 `claude.checklist.body` 에는 이 파일의 SENTINEL
    // 상수와 동일한 리터럴이 손으로 박혀 있다 — 둘 중 하나만 고치면 모델이 실제로
    // 낼 문자열과 여기서 매칭을 시도하는 문자열이 조용히 어긋난다. 세 lang 파일을
    // 실제 `Translator` 로 로드해 그 결과가 SENTINEL 을 포함하는지 직접 검증한다.

    #[test]
    fn checklist_body_contains_sentinel_in_every_locale() {
        let lang_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("lang");
        for locale in ["en", "ko", "ja"] {
            let translator = tasty_plugin_sdk::i18n::Translator::load(&lang_dir, locale);
            let body = translator.t("claude.checklist.body");
            assert!(
                body.contains(SENTINEL),
                "lang/{locale}.toml 의 claude.checklist.body 에 SENTINEL({SENTINEL}) 리터럴이 없음"
            );
        }
    }

    fn state(prompt_id: &str, rounds: u32) -> RoundState {
        RoundState {
            prompt_id: prompt_id.to_string(),
            rounds,
        }
    }

    // ── decide() 4 분기 ──

    #[test]
    fn branch1_prompt_id_change_resets_counter_then_blocks() {
        // 이전 prompt_id 로 라운드 2까지 쌓여 있었지만, 새 prompt_id 가 들어오면
        // 0에서 다시 시작해 block(라운드 1)한다.
        let stored = state("old-prompt", 2);
        let d = decide(Some(&stored), "new-prompt", "아직 할 일 남음", 3);
        assert_eq!(d, Decision::Block { rounds: 1 });
    }

    #[test]
    fn branch1_no_stored_state_treated_as_round_zero() {
        let d = decide(None, "p1", "진행 중", 3);
        assert_eq!(d, Decision::Block { rounds: 1 });
    }

    #[test]
    fn branch2_sentinel_passes_regardless_of_round() {
        let stored = state("p1", 0);
        let msg = format!("작업을 다 마쳤습니다. {SENTINEL}");
        let d = decide(Some(&stored), "p1", &msg, 3);
        assert_eq!(d, Decision::Pass);
    }

    #[test]
    fn branch2_sentinel_beats_backstop_even_at_limit() {
        // 상한에 이미 도달했어도 센티넬이 있으면 그냥 Pass(사유는 같지만, 센티넬이
        // 우선순위상 먼저 검사됨을 고정한다).
        let stored = state("p1", 3);
        let msg = format!("끝 {SENTINEL}");
        let d = decide(Some(&stored), "p1", &msg, 3);
        assert_eq!(d, Decision::Pass);
    }

    #[test]
    fn branch3_round_limit_backstop_passes_without_sentinel() {
        let stored = state("p1", 3);
        let d = decide(Some(&stored), "p1", "아직도 하는 중", 3);
        assert_eq!(d, Decision::Pass);
    }

    #[test]
    fn branch3_one_below_limit_still_blocks() {
        let stored = state("p1", 2);
        let d = decide(Some(&stored), "p1", "아직도 하는 중", 3);
        assert_eq!(d, Decision::Block { rounds: 3 });
    }

    #[test]
    fn branch4_default_case_blocks_and_increments() {
        let stored = state("p1", 1);
        let d = decide(Some(&stored), "p1", "계속 작업 중", 5);
        assert_eq!(d, Decision::Block { rounds: 2 });
    }

    #[test]
    fn sentinel_substring_match_works_mid_message() {
        let msg = format!("전부 끝났습니다.\n\n{SENTINEL}\n\n추가로 궁금한 점 있으면 말씀하세요.");
        let d = decide(None, "p1", &msg, 3);
        assert_eq!(d, Decision::Pass);
    }

    // ── handle_checklist_hook 통합(파일 I/O 포함) ──

    fn setup_marker(dir: &Path) {
        std::fs::create_dir_all(checklist_dir(dir)).unwrap();
        std::fs::write(marker_file(dir), "").unwrap();
    }

    // handle_checklist_hook은 host.call("settings.get_plugin_setting", ...)을 쓰므로
    // 실제 IPC 연결 없는 단위 테스트에서는 fetch_round_limit 을 직접 검증하지 않고,
    // decide()/marker/state 계층을 개별적으로 검증한다(host 필요 경로는 통합 테스트
    // 대신 인터랙티브 검증으로 커버 — self-verification.md 방침).

    #[test]
    fn marker_absent_means_not_present() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(!marker_present(Some(tmp.path())));
    }

    #[test]
    fn marker_present_after_touch() {
        let tmp = tempfile::tempdir().unwrap();
        setup_marker(tmp.path());
        assert!(marker_present(Some(tmp.path())));
    }

    #[test]
    fn marker_present_false_without_data_dir() {
        assert!(!marker_present(None));
    }

    // ── handle_enable/handle_disable/handle_status ──

    #[test]
    fn enable_creates_marker_file() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(!marker_present(Some(tmp.path())));
        let result = handle_enable(Some(tmp.path()), &test_translator()).unwrap();
        assert_eq!(result, json!({ "enabled": true }));
        assert!(marker_present(Some(tmp.path())));
    }

    #[test]
    fn disable_removes_marker_file() {
        let tmp = tempfile::tempdir().unwrap();
        setup_marker(tmp.path());
        assert!(marker_present(Some(tmp.path())));
        let result = handle_disable(Some(tmp.path()), &test_translator()).unwrap();
        assert_eq!(result, json!({ "enabled": false }));
        assert!(!marker_present(Some(tmp.path())));
    }

    #[test]
    fn disable_is_idempotent_without_marker() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(!marker_present(Some(tmp.path())));
        let result = handle_disable(Some(tmp.path()), &test_translator()).unwrap();
        assert_eq!(result, json!({ "enabled": false }));
    }

    #[test]
    fn status_round_trips_enable_disable() {
        let tmp = tempfile::tempdir().unwrap();
        assert_eq!(
            handle_status(Some(tmp.path())).unwrap(),
            json!({ "enabled": false })
        );
        handle_enable(Some(tmp.path()), &test_translator()).unwrap();
        assert_eq!(
            handle_status(Some(tmp.path())).unwrap(),
            json!({ "enabled": true })
        );
        handle_disable(Some(tmp.path()), &test_translator()).unwrap();
        assert_eq!(
            handle_status(Some(tmp.path())).unwrap(),
            json!({ "enabled": false })
        );
    }

    #[test]
    fn enable_disable_status_error_without_data_dir() {
        assert!(handle_enable(None, &test_translator()).is_err());
        assert!(handle_disable(None, &test_translator()).is_err());
        // status 는 marker_present 와 동일하게 안전 폴백(false)이지 에러가 아니다.
        assert_eq!(handle_status(None).unwrap(), json!({ "enabled": false }));
    }

    #[test]
    fn state_round_trip_and_removal() {
        let tmp = tempfile::tempdir().unwrap();
        let s = state("p1", 2);
        write_state(tmp.path(), "sess-a", &s);
        assert_eq!(read_state(tmp.path(), "sess-a"), Some(s));
        remove_state(tmp.path(), "sess-a");
        assert_eq!(read_state(tmp.path(), "sess-a"), None);
    }

    #[test]
    fn remove_state_for_session_is_noop_without_data_dir() {
        // panic 없이 조용히 넘어가야 한다.
        remove_state_for_session(None, "sess-a");
    }

    #[test]
    fn remove_state_for_session_noop_when_no_state_file() {
        let tmp = tempfile::tempdir().unwrap();
        // 존재하지 않는 세션을 지우려 해도 에러/패닉 없음.
        remove_state_for_session(Some(tmp.path()), "never-existed");
    }

    #[test]
    fn concurrent_sessions_have_independent_state_files() {
        let tmp = tempfile::tempdir().unwrap();
        write_state(tmp.path(), "sess-a", &state("p1", 1));
        write_state(tmp.path(), "sess-b", &state("p1", 5));
        assert_eq!(read_state(tmp.path(), "sess-a").unwrap().rounds, 1);
        assert_eq!(read_state(tmp.path(), "sess-b").unwrap().rounds, 5);
        remove_state(tmp.path(), "sess-a");
        assert_eq!(read_state(tmp.path(), "sess-a"), None);
        assert_eq!(read_state(tmp.path(), "sess-b").unwrap().rounds, 5);
    }
}
