//! Stop-훅 게이트의 판정 경로 — `tasty claude checklist-hook --gate <name>`.
//!
//! 판정에 쓰는 3요소(본문 · 센티넬 · 라운드 상한)는 [`crate::gate`] 레지스트리가
//! 이름으로 들고 있고, 이 모듈은 그 이름을 받아 **게이트별로** 판정한다. 게이트를
//! 지정하지 않은 호출은 host 기본 게이트([`crate::gate::DEFAULT_GATE_NAME`])로
//! 해석되므로, `--gate` 없이 설치돼 있던 기존 훅 명령도 그대로 동작한다.
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
//! 2. `last_assistant_message` 에 **그 게이트의 센티넬**이 있으면 → 통과(주 종료
//!    경로 — 모델이 스스로 "다 끝났다" 선언)
//! 3. 라운드 수가 상한 이상이면 → 통과(백스톱 — Claude Code 자체에는 무한 block
//!    루프를 끊는 장치가 없음을 실측 확인했으므로 훅이 직접 끊어야 한다)
//! 4. 그 외 → block, 라운드 +1
//!
//! + `prompt_id` 가 아예 없는 요청은 턴 경계를 판단할 수 없으므로 상태를 건드리지
//!   않고 이번 발화만 통과시킨다(안전 폴백, [`decide`] 밖에서 처리).
//!
//! ## 라운드 상태 키잉 — (게이트 × 세션)
//!
//! `TASTY_PLUGIN_DATA_DIR/checklist/gates/<gate>/rounds/<session_id>.json`.
//!
//! session_id 축은 자식 세션을 동시에 여러 개 띄우기 때문이다 — 전역 카운터 하나를
//! 공유하면 서로의 라운드를 깎는다. 게이트 축은 **같은 실패 모드의 반복**이다:
//! `--profile a,b` 로 게이트를 둘 부착하면 `profile_merge` 의 `hooks` concat 규칙상
//! 두 Stop 훅이 각각 등록·발화하므로, 게이트를 구분하지 않으면 두 게이트가 한
//! 카운터를 읽고 써서 서로의 라운드를 깎는다.
//!
//! `SessionEnd` 에서 정리한다(`hook.rs` 의 session-end 분기가
//! [`remove_state_for_session`] 을 호출) — 그 세션의 **모든 게이트** 상태를 지운다.
//! 호출부는 어느 게이트가 붙어 있었는지 알 수 없기 때문이다(전역 `session-end` 훅은
//! `MANAGED_HOOKS` 로 항상 설치되며 게이트와 무관하게 발화한다).
//!
//! 게이트 축이 생기기 전의 `checklist/rounds/<session_id>.json` 은 **legacy 경로**다.
//! 라운드 상태는 세션 수명과 함께 사라지는 휘발성 데이터라 마이그레이션하지 않지만,
//! 구버전이 남긴 파일이 orphan 으로 남지 않도록 session-end 정리는 그 경로도 함께
//! 지운다.
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

/// host 기본 게이트의 종료 선언 센티넬 — 등록 게이트가 `--sentinel` 을 주지 않았을
/// 때의 기본값이기도 하다(`gate::register` 가 등록 시점에 실체화한다).
/// `last_assistant_message` 의 substring 매칭으로 찾는다(도구
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
    sentinel: &str,
    round_limit: u32,
) -> Decision {
    // 1) 턴 경계: 저장된 prompt_id 와 다르면(또는 상태 자체가 없으면) 새 턴 → 0.
    let current_rounds = match stored {
        Some(s) if s.prompt_id == prompt_id => s.rounds,
        _ => 0,
    };
    // 2) 센티넬 — 주 종료 경로. 게이트별 값이라 상수가 아니라 인자다.
    if last_assistant_message.contains(sentinel) {
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

/// 게이트별 상태 루트. 이 아래 한 단계가 게이트 이름 디렉토리다 —
/// [`remove_state_for_session`] 이 게이트를 모른 채 순회할 수 있는 이유.
fn gates_dir(data_dir: &Path) -> PathBuf {
    checklist_dir(data_dir).join("gates")
}

/// 호출자는 `gate` 가 [`crate::gate::show`] 를 통과한 이름임을 보장해야 한다 —
/// 그 관문이 경로 조각으로 안전한 short-name 규칙을 강제한다.
fn rounds_dir(data_dir: &Path, gate: &str) -> PathBuf {
    gates_dir(data_dir).join(gate).join("rounds")
}

fn state_file(data_dir: &Path, gate: &str, session_id: &str) -> PathBuf {
    rounds_dir(data_dir, gate).join(format!("{session_id}.json"))
}

/// 게이트 축이 생기기 전 경로. 읽거나 쓰지 않고, session-end 정리에서만 지운다.
fn legacy_state_file(data_dir: &Path, session_id: &str) -> PathBuf {
    checklist_dir(data_dir)
        .join("rounds")
        .join(format!("{session_id}.json"))
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

fn read_state(data_dir: &Path, gate: &str, session_id: &str) -> Option<RoundState> {
    let text = std::fs::read_to_string(state_file(data_dir, gate, session_id)).ok()?;
    let value: Value = serde_json::from_str(&text).ok()?;
    RoundState::from_json(&value)
}

fn write_state(data_dir: &Path, gate: &str, session_id: &str, state: &RoundState) {
    let dir = rounds_dir(data_dir, gate);
    if let Err(e) = std::fs::create_dir_all(&dir) {
        warn!(
            "checklist: failed to create state dir {}: {e}",
            dir.display()
        );
        return;
    }
    let text = state.to_json().to_string();
    let path = state_file(data_dir, gate, session_id);
    if let Err(e) = std::fs::write(&path, text) {
        warn!(
            "checklist: failed to write round state {}: {e}",
            path.display()
        );
    }
}

fn remove_state(data_dir: &Path, gate: &str, session_id: &str) {
    remove_state_file(&state_file(data_dir, gate, session_id));
}

fn remove_state_file(path: &Path) {
    if let Err(e) = std::fs::remove_file(path)
        && e.kind() != std::io::ErrorKind::NotFound
    {
        warn!(
            "checklist: failed to remove round state {}: {e}",
            path.display()
        );
    }
}

/// `SessionEnd` 훅(`hook.rs`)이 호출하는 정리 경로 — 세션이 끝나면 그 session_id
/// 의 라운드 상태 파일을 **모든 게이트에서** 지운다(누수 방지). 상태가 아예
/// 없었어도(게이트가 걸리지 않은 세션) no-op.
///
/// 호출부는 게이트를 알 수 없으므로 시그니처에 게이트가 없다 — 대신 게이트
/// 디렉토리를 순회한다. 디렉토리 이름은 파일시스템에서 읽은 값이라 경로 조각으로
/// 안전하다(등록 관문을 통과한 이름만 만들어진다).
pub(crate) fn remove_state_for_session(data_dir: Option<&Path>, session_id: &str) {
    let Some(dir) = data_dir else {
        return;
    };
    // 게이트 축 도입 전 빌드가 남긴 orphan — 마이그레이션은 하지 않지만 정리는 한다.
    remove_state_file(&legacy_state_file(dir, session_id));
    let Ok(entries) = std::fs::read_dir(gates_dir(dir)) else {
        return; // 게이트가 하나도 붙지 않은 세션 — 디렉토리 자체가 없다.
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        remove_state_file(&path.join("rounds").join(format!("{session_id}.json")));
    }
}

/// Settings 값 조회. 호출에 `host` 가 필요해 단위 테스트가 어렵다 — 판정에 쓰는
/// 폴백 순서 자체는 [`resolve_round_limit`] 로 떼어 두었다.
fn fetch_settings_round_limit(host: &HostHandle) -> Option<u32> {
    host.call(
        "settings.get_plugin_setting",
        json!({ "storage_key": ROUND_LIMIT_STORAGE_KEY }),
    )
    .ok()
    .and_then(|v| v.get("value").and_then(|v| v.as_f64()))
    .map(|f| f.max(1.0).round() as u32)
}

/// 라운드 상한 폴백 체인 — 게이트 정의 > Settings > [`DEFAULT_ROUND_LIMIT`].
///
/// 게이트가 이기는 이유: 명시 지정이 전역 기본값을 이기는 것이 일반적인 설정
/// 우선순위이고, `--rounds 5` 로 등록해 둔 게이트가 Settings 값에 조용히 덮이면
/// 등록 인자가 무의미해진다. 게다가 Settings 항목의 storage key 는 이름부터
/// `continue_checklist_round_limit` 으로 host 기본 게이트 전용이라, 그 값을 사용자
/// 정의 게이트에 강제할 근거가 없다. host 기본 게이트는 `round_limit` 미지정이라
/// 폴백이 Settings 로 내려가 기존 동작이 그대로 보존된다.
fn resolve_round_limit(gate_limit: Option<u32>, settings_limit: Option<u32>) -> u32 {
    gate_limit.or(settings_limit).unwrap_or(DEFAULT_ROUND_LIMIT)
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
/// - `--gate` 가 가리키는 게이트가 없거나 읽히지 않음 → 통과. 등록이 지워졌는데
///   훅 명령이 남아 있는 세션은 정상적인 상태이고, 여기서 에러를 내면 그 세션이
///   종료 불가가 된다
pub(crate) fn handle_checklist_hook(
    host: &HostHandle,
    data_dir: Option<&Path>,
    checklist_body: &str,
    tr: &Translator,
    params: &Value,
) -> Result<Value, IpcMethodError> {
    hook_response(data_dir, checklist_body, tr, params, || {
        fetch_settings_round_limit(host)
    })
}

/// [`handle_checklist_hook`] 의 host 비의존 본체. Settings 조회만 클로저로 빼서
/// 단위 테스트가 판정 전체를 돌려볼 수 있게 한다 — 조회는 게이트가 자체 상한을
/// 주지 않았을 때만 호출되므로, 미등록 게이트로 발화한 훅은 IPC 를 한 번도 하지
/// 않고 통과한다.
fn hook_response(
    data_dir: Option<&Path>,
    host_gate_body: &str,
    tr: &Translator,
    params: &Value,
    settings_round_limit: impl Fn() -> Option<u32>,
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

    // `--gate` 는 optional 이고 기본값이 매니페스트에 박혀 있지만, IPC 직접 호출은
    // 그 기본값을 거치지 않으므로 여기서도 같은 기본값으로 떨어뜨린다.
    let gate_name = params
        .get("gate")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .unwrap_or(crate::gate::DEFAULT_GATE_NAME);
    let Ok((owner, gate_def, gate_body)) = crate::gate::show(Some(data_dir), gate_name, tr) else {
        return Ok(json!({}));
    };
    // host 기본 게이트 본문은 lang 문자열이라 기동 시 한 번 해석해 둔 캐시를 그대로
    // 쓴다. 등록 게이트 본문은 사용자 파일이라 `gate::show` 가 매 발화마다 읽는다 —
    // 재등록으로 갱신한 본문이 세션 재기동 없이 반영되어야 한다.
    let body = if owner == "host" {
        host_gate_body
    } else {
        gate_body.as_str()
    };

    let stored = read_state(data_dir, gate_name, session_id);

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

    let round_limit = resolve_round_limit(gate_def.round_limit, {
        // 게이트가 자체 상한을 주면 Settings 를 조회하지 않는다 — 어차피 지는 값이다.
        if gate_def.round_limit.is_some() {
            None
        } else {
            settings_round_limit()
        }
    });
    match decide(
        stored.as_ref(),
        prompt_id,
        last_message,
        &gate_def.sentinel,
        round_limit,
    ) {
        Decision::Pass => {
            remove_state(data_dir, gate_name, session_id);
            Ok(json!({}))
        }
        Decision::Block { rounds } => {
            write_state(
                data_dir,
                gate_name,
                session_id,
                &RoundState {
                    prompt_id: prompt_id.to_string(),
                    rounds,
                },
            );
            Ok(json!({ "decision": "block", "reason": body }))
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
        let d = decide(Some(&stored), "new-prompt", "아직 할 일 남음", SENTINEL, 3);
        assert_eq!(d, Decision::Block { rounds: 1 });
    }

    #[test]
    fn branch1_no_stored_state_treated_as_round_zero() {
        let d = decide(None, "p1", "진행 중", SENTINEL, 3);
        assert_eq!(d, Decision::Block { rounds: 1 });
    }

    #[test]
    fn branch2_sentinel_passes_regardless_of_round() {
        let stored = state("p1", 0);
        let msg = format!("작업을 다 마쳤습니다. {SENTINEL}");
        let d = decide(Some(&stored), "p1", &msg, SENTINEL, 3);
        assert_eq!(d, Decision::Pass);
    }

    #[test]
    fn branch2_sentinel_beats_backstop_even_at_limit() {
        // 상한에 이미 도달했어도 센티넬이 있으면 그냥 Pass(사유는 같지만, 센티넬이
        // 우선순위상 먼저 검사됨을 고정한다).
        let stored = state("p1", 3);
        let msg = format!("끝 {SENTINEL}");
        let d = decide(Some(&stored), "p1", &msg, SENTINEL, 3);
        assert_eq!(d, Decision::Pass);
    }

    #[test]
    fn branch3_round_limit_backstop_passes_without_sentinel() {
        let stored = state("p1", 3);
        let d = decide(Some(&stored), "p1", "아직도 하는 중", SENTINEL, 3);
        assert_eq!(d, Decision::Pass);
    }

    #[test]
    fn branch3_one_below_limit_still_blocks() {
        let stored = state("p1", 2);
        let d = decide(Some(&stored), "p1", "아직도 하는 중", SENTINEL, 3);
        assert_eq!(d, Decision::Block { rounds: 3 });
    }

    #[test]
    fn branch4_default_case_blocks_and_increments() {
        let stored = state("p1", 1);
        let d = decide(Some(&stored), "p1", "계속 작업 중", SENTINEL, 5);
        assert_eq!(d, Decision::Block { rounds: 2 });
    }

    /// 센티넬이 게이트별 값이 됐으므로, 인자로 준 센티넬만 매칭에 쓰인다 —
    /// 기본 센티넬이 메시지에 있어도 그건 이 게이트의 종료 선언이 아니다.
    #[test]
    fn decide_uses_given_sentinel_not_the_default() {
        let msg = format!("끝났습니다 {SENTINEL}");
        assert_eq!(
            decide(None, "p1", &msg, "[[A-DONE]]", 3),
            Decision::Block { rounds: 1 }
        );
        assert_eq!(
            decide(None, "p1", "끝났습니다 [[A-DONE]]", "[[A-DONE]]", 3),
            Decision::Pass
        );
    }

    #[test]
    fn sentinel_substring_match_works_mid_message() {
        let msg = format!("전부 끝났습니다.\n\n{SENTINEL}\n\n추가로 궁금한 점 있으면 말씀하세요.");
        let d = decide(None, "p1", &msg, SENTINEL, 3);
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
        write_state(tmp.path(), "g", "sess-a", &s);
        assert_eq!(read_state(tmp.path(), "g", "sess-a"), Some(s));
        remove_state(tmp.path(), "g", "sess-a");
        assert_eq!(read_state(tmp.path(), "g", "sess-a"), None);
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
        write_state(tmp.path(), "g", "sess-a", &state("p1", 1));
        write_state(tmp.path(), "g", "sess-b", &state("p1", 5));
        assert_eq!(read_state(tmp.path(), "g", "sess-a").unwrap().rounds, 1);
        assert_eq!(read_state(tmp.path(), "g", "sess-b").unwrap().rounds, 5);
        remove_state(tmp.path(), "g", "sess-a");
        assert_eq!(read_state(tmp.path(), "g", "sess-a"), None);
        assert_eq!(read_state(tmp.path(), "g", "sess-b").unwrap().rounds, 5);
    }

    /// 위 테스트의 **게이트 축** 버전 — 같은 session_id 라도 게이트가 다르면 카운터가
    /// 섞이지 않는다. `--profile a,b` 로 게이트 둘을 동시에 부착하는 시나리오가
    /// 이것에 달려 있다.
    #[test]
    fn gate_state_files_are_independent_per_gate() {
        let tmp = tempfile::tempdir().unwrap();
        write_state(tmp.path(), "gate-a", "sess-1", &state("p1", 1));
        write_state(tmp.path(), "gate-b", "sess-1", &state("p1", 4));
        assert_eq!(
            read_state(tmp.path(), "gate-a", "sess-1").unwrap().rounds,
            1
        );
        assert_eq!(
            read_state(tmp.path(), "gate-b", "sess-1").unwrap().rounds,
            4
        );

        remove_state(tmp.path(), "gate-a", "sess-1");
        assert_eq!(read_state(tmp.path(), "gate-a", "sess-1"), None);
        assert_eq!(
            read_state(tmp.path(), "gate-b", "sess-1").unwrap().rounds,
            4
        );
    }

    #[test]
    fn remove_state_for_session_clears_all_gates() {
        let tmp = tempfile::tempdir().unwrap();
        for gate in ["gate-a", "gate-b", "gate-c"] {
            write_state(tmp.path(), gate, "sess-1", &state("p1", 2));
            // 지우면 안 되는 다른 세션도 함께 심어 둔다.
            write_state(tmp.path(), gate, "sess-2", &state("p1", 2));
        }

        remove_state_for_session(Some(tmp.path()), "sess-1");

        for gate in ["gate-a", "gate-b", "gate-c"] {
            assert_eq!(read_state(tmp.path(), gate, "sess-1"), None, "{gate}");
            assert!(read_state(tmp.path(), gate, "sess-2").is_some(), "{gate}");
        }
    }

    #[test]
    fn remove_state_for_session_noop_when_no_gates_dir() {
        let tmp = tempfile::tempdir().unwrap();
        // gates/ 디렉토리가 아예 없는 상태 — 에러/패닉 없이 조용히 넘어가야 한다.
        assert!(!gates_dir(tmp.path()).exists());
        remove_state_for_session(Some(tmp.path()), "sess-1");
    }

    #[test]
    fn remove_state_for_session_also_clears_the_legacy_path() {
        let tmp = tempfile::tempdir().unwrap();
        let legacy = legacy_state_file(tmp.path(), "sess-1");
        std::fs::create_dir_all(legacy.parent().unwrap()).unwrap();
        std::fs::write(&legacy, "{}").unwrap();

        remove_state_for_session(Some(tmp.path()), "sess-1");
        assert!(!legacy.exists(), "게이트 축 이전 orphan 이 남았다");
    }

    // ── 라운드 상한 폴백 체인 ──

    #[test]
    fn round_limit_prefers_gate_value_over_settings() {
        assert_eq!(resolve_round_limit(Some(5), Some(2)), 5);
    }

    #[test]
    fn round_limit_falls_back_to_settings_then_default() {
        assert_eq!(resolve_round_limit(None, Some(7)), 7);
        assert_eq!(resolve_round_limit(None, None), DEFAULT_ROUND_LIMIT);
    }

    // ── hook_response (host 비의존 본체) ──

    /// 센티넬을 포함한 본문 파일을 만들고 그 이름으로 게이트를 등록한다 —
    /// `gate::register` 가 본문의 센티넬 포함을 검증하므로 둘을 함께 준비해야 한다.
    fn register_gate(dir: &Path, name: &str, sentinel: &str, rounds: Option<u32>) {
        let body = dir.join(format!("{name}-body.md"));
        std::fs::write(&body, format!("게이트 {name} 본문\n{sentinel}\n")).unwrap();
        crate::gate::register(Some(dir), name, &body, Some(sentinel), rounds).unwrap();
    }

    fn hook_params(gate: &str, session: &str, prompt: &str, last: &str) -> Value {
        json!({
            "gate": gate,
            "session_id": session,
            "prompt_id": prompt,
            "last_assistant_message": last,
        })
    }

    fn fire(dir: &Path, params: &Value, settings_limit: Option<u32>) -> Value {
        hook_response(Some(dir), "HOST-BODY", &test_translator(), params, || {
            settings_limit
        })
        .unwrap()
    }

    #[test]
    fn unknown_gate_passes_silently() {
        let tmp = tempfile::tempdir().unwrap();
        setup_marker(tmp.path());
        // 등록된 적 없는 이름이 훅 명령에 박혀 있는 상태(등록이 지워진 세션).
        let params = hook_params("no-such-gate", "sess-1", "p1", "아직 작업 중");
        assert_eq!(fire(tmp.path(), &params, Some(3)), json!({}));
        // 상태 파일도 만들지 않는다.
        assert!(!gates_dir(tmp.path()).join("no-such-gate").exists());
    }

    #[test]
    fn hook_uses_the_gates_own_sentinel_and_body() {
        let tmp = tempfile::tempdir().unwrap();
        setup_marker(tmp.path());
        register_gate(tmp.path(), "gate-a", "[[A-DONE]]", Some(2));

        // 다른 게이트의 센티넬로는 통과하지 못한다.
        let blocked = fire(
            tmp.path(),
            &hook_params("gate-a", "sess-1", "p1", "끝 [[B-DONE]]"),
            Some(9),
        );
        assert_eq!(blocked["decision"], "block");
        assert!(
            blocked["reason"]
                .as_str()
                .unwrap()
                .contains("게이트 gate-a"),
            "등록 게이트 본문이 주입되지 않았다: {blocked}"
        );

        // 자기 센티넬이면 통과.
        let passed = fire(
            tmp.path(),
            &hook_params("gate-a", "sess-1", "p1", "끝 [[A-DONE]]"),
            Some(9),
        );
        assert_eq!(passed, json!({}));
    }

    #[test]
    fn hook_gate_round_limit_wins_over_settings() {
        let tmp = tempfile::tempdir().unwrap();
        setup_marker(tmp.path());
        register_gate(tmp.path(), "gate-a", "[[A-DONE]]", Some(1));

        // Settings 가 9 여도 게이트 상한 1 이 이긴다: 1 라운드 block 후 통과.
        let first = fire(
            tmp.path(),
            &hook_params("gate-a", "sess-1", "p1", "진행 중"),
            Some(9),
        );
        assert_eq!(first["decision"], "block");
        assert_eq!(
            read_state(tmp.path(), "gate-a", "sess-1").unwrap().rounds,
            1
        );

        let second = fire(
            tmp.path(),
            &hook_params("gate-a", "sess-1", "p1", "진행 중"),
            Some(9),
        );
        assert_eq!(second, json!({}));
    }

    #[test]
    fn two_gates_in_one_session_keep_independent_counters() {
        let tmp = tempfile::tempdir().unwrap();
        setup_marker(tmp.path());
        register_gate(tmp.path(), "gate-a", "[[A-DONE]]", Some(2));
        register_gate(tmp.path(), "gate-b", "[[B-DONE]]", Some(5));

        for _ in 0..2 {
            fire(
                tmp.path(),
                &hook_params("gate-a", "sess-1", "p1", "진행 중"),
                None,
            );
            fire(
                tmp.path(),
                &hook_params("gate-b", "sess-1", "p1", "진행 중"),
                None,
            );
        }
        // gate-a 는 상한 2 에 도달, gate-b 는 아직 2/5.
        assert_eq!(
            read_state(tmp.path(), "gate-a", "sess-1").unwrap().rounds,
            2
        );
        assert_eq!(
            read_state(tmp.path(), "gate-b", "sess-1").unwrap().rounds,
            2
        );

        let a = fire(
            tmp.path(),
            &hook_params("gate-a", "sess-1", "p1", "진행 중"),
            None,
        );
        let b = fire(
            tmp.path(),
            &hook_params("gate-b", "sess-1", "p1", "진행 중"),
            None,
        );
        assert_eq!(a, json!({}), "gate-a 는 백스톱으로 통과해야 한다");
        assert_eq!(b["decision"], "block", "gate-b 는 아직 상한 전이다");
    }

    #[test]
    fn host_default_gate_is_used_when_no_gate_param_and_keeps_cached_body() {
        let tmp = tempfile::tempdir().unwrap();
        setup_marker(tmp.path());
        // `--gate` 가 없는 (게이트 축 이전) 훅 명령 모양.
        let params = json!({
            "session_id": "sess-1",
            "prompt_id": "p1",
            "last_assistant_message": "아직 작업 중",
        });
        let blocked = fire(tmp.path(), &params, Some(3));
        assert_eq!(blocked["decision"], "block");
        assert_eq!(
            blocked["reason"], "HOST-BODY",
            "host 기본 게이트는 캐시된 본문을 써야 한다"
        );
        // 상태는 host 기본 게이트 이름으로 키잉된다.
        assert!(read_state(tmp.path(), crate::gate::DEFAULT_GATE_NAME, "sess-1").is_some());

        // 센티넬은 host 기본 센티넬.
        let passed = fire(
            tmp.path(),
            &json!({
                "session_id": "sess-1",
                "prompt_id": "p1",
                "last_assistant_message": format!("끝 {SENTINEL}"),
            }),
            Some(3),
        );
        assert_eq!(passed, json!({}));
    }

    #[test]
    fn marker_off_passes_before_touching_the_gate_registry() {
        let tmp = tempfile::tempdir().unwrap();
        // 마커 없음 — 게이트를 등록해 두었어도 발동하지 않는다.
        register_gate(tmp.path(), "gate-a", "[[A-DONE]]", Some(2));
        let params = hook_params("gate-a", "sess-1", "p1", "진행 중");
        assert_eq!(fire(tmp.path(), &params, Some(3)), json!({}));
        assert_eq!(read_state(tmp.path(), "gate-a", "sess-1"), None);
    }

    /// 매니페스트의 `--gate` 기본값과 [`crate::gate::DEFAULT_GATE_NAME`] 이 어긋나면
    /// `--gate` 없이 설치된 훅이 조용히 미등록 게이트로 해석돼 **아무 판정도 하지
    /// 않는 상태**가 된다(실패 모드가 "조용히 통과"라 눈에 띄지 않는다).
    #[test]
    fn manifest_gate_flag_default_matches_constant() {
        let manifest =
            std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tasty-plugin.toml");
        let text = std::fs::read_to_string(&manifest).unwrap();
        let re = regex::Regex::new(r#"flag = "--gate".*?default = "([^"]+)""#).unwrap();
        let found = re
            .captures(&text)
            .expect("tasty-plugin.toml 에 --gate 인자 선언이 없다");
        assert_eq!(&found[1], crate::gate::DEFAULT_GATE_NAME);
    }
}
