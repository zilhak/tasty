//! Stop-훅 게이트 레지스트리 — 이름으로 등록해 둔 (본문, 센티넬, 라운드 상한)
//! 3요소를 게이트 하나로 묶는다.
//!
//! `profile.rs`(Claude 세션 프로필 레지스트리)의 형태를 **미러링**한다 — short-name
//! 규칙 · `<owner>/<short>` id · `TASTY_PLUGIN_DATA_DIR` 하위 저장 · `data_dir` 이
//! `None` 이면 명시적 에러 · `XxxError` + `translate(&Translator)`. 타입은 공유하지
//! 않는다(이 레포의 확립된 방식, `profile.rs` 모듈 doc 참조).
//!
//! 이 plugin 은 단일 스레드 IPC 디스패치(`ClaudePlugin::handle_ipc_method`)만
//! 레지스트리를 건드리므로 `RwLock`/`OnceLock` 프로세스 전역 싱글턴이 불필요하다
//! (`profile.rs` · `state.rs` 와 동일 근거).
//!
//! ## 게이트 이름 = 프로필 부착 이름
//!
//! 게이트를 등록하면 동명의 attachable 프로필로 부착 가능해진다 — 별도 attach
//! 문법을 만들지 않는다. 그래서 두 레지스트리의 이름 공간은 **한 평면**이고,
//! 같은 이름이 양쪽에 생기면 조용히 한쪽이 가려진다. 그 shadowing 을 만들지 않기
//! 위해 등록 시점에 **양방향으로** 거부한다(`gate-register` 는 동명 registered
//! 프로필을, `profile-register` 는 동명 게이트를). 이 plugin 은 조용한 shadowing
//! 으로 이미 사고를 겪었고(`profile.rs` 모듈 doc), 같은 이유로 `--profile-file`
//! 반복 지정도 last-wins 대신 명시적 에러로 거부한다.
//!
//! ## 저장 위치
//!
//! `TASTY_PLUGIN_DATA_DIR` 하위. 사용자 원본과 tasty 생성물을 나누는
//! `profiles/registered/` vs `profiles/generated/` 방침을 그대로 따른다:
//! - `gates/registered/<gate-name>.json` — 게이트 정의(`sentinel` / `round_limit`,
//!   둘 다 optional)
//! - `gates/bodies/<gate-name>.md` — 등록 시 호출자가 준 본문 파일의 **복사본**
//!   (원본이 나중에 옮겨지거나 지워져도 게이트는 살아 있다 — `profile::register`
//!   와 같은 방침)
//!
//! 본문을 정의 JSON 에 인라인하지 않는 이유는 그 복사본 방침에 더해, 본문이 여러
//! 줄 마크다운이라 JSON 문자열 인라인보다 파일이 다루기 쉽기 때문이다.
//!
//! 라운드 상태·enable 마커의 게이트별 경로는 이 모듈이 다루지 않는다(게이트
//! **정의** 저장만 담당).
//!
//! ## host 기본 게이트는 코드 상수로 남는다
//!
//! `continue-checklist` 는 파일로 실체화되지 않고 [`host_default_gate`] 조회
//! 함수로만 존재한다(본문은 lang 의 `claude.checklist.body`, 센티넬은
//! [`crate::checklist::SENTINEL`], 라운드 상한은 미지정 → Settings 폴백).
//! `MANAGED_HOOKS` · `HOST_DEFAULT_PROFILE_NAMES` 가 이미 같은 형태이고, host
//! 기본값을 데이터 디렉토리에 실체화하면 사용자가 지웠을 때 되살릴 경로가 없다.

use std::path::{Path, PathBuf};

use serde_json::{Value, json};
use tasty_plugin_sdk::{IpcMethodError, i18n::Translator};

use crate::checklist::SENTINEL;

/// 게이트 short-name 규칙 — `profile.rs::is_valid_short_name` 과 **동일 규칙**
/// (소문자/숫자/하이픈, 최대 32자). 두 레지스트리의 이름이 한 평면을 공유하므로
/// 규칙이 갈리면 한쪽에서만 쓸 수 있는 이름이 생긴다. 파일명으로도 그대로 쓰이므로
/// 경로 traversal 문자(`/`, `..`)를 원천 배제한다.
pub(crate) fn is_valid_short_name(s: &str) -> bool {
    if s.is_empty() || s.len() > 32 {
        return false;
    }
    s.chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GateError {
    /// 호스트가 `TASTY_PLUGIN_DATA_DIR` 를 주입하지 않은 비정상 기동.
    NoDataDir,
    InvalidShortName(String),
    UnknownGate(String),
    BodyNotReadable {
        path: String,
        message: String,
    },
    /// 본문에 실효 센티넬이 없다 — 모델이 종료를 선언할 방법을 안내받지 못해
    /// 라운드 상한 백스톱까지 무조건 도달한다(게이트가 "N턴 강제 연장" 으로 변질).
    BodyMissingSentinel {
        sentinel: String,
    },
    /// 빈 센티넬은 모든 메시지에 매칭되어(`str::contains("")` 는 항상 참) 게이트가
    /// 첫 라운드에 통과한다 — 게이트를 등록해 놓고 꺼두는 것과 같아진다.
    EmptySentinel,
    /// 라운드 상한 0 — 게이트가 한 번도 block 하지 못한다.
    RoundsBelowOne,
    /// 동명 registered 프로필이 이미 있다(이름 공간이 한 평면이므로 거부).
    ProfileNameConflict(String),
    Io {
        path: String,
        message: String,
    },
}

impl GateError {
    pub(crate) fn translate(&self, tr: &Translator) -> String {
        match self {
            Self::NoDataDir => tr.t("claude.gate.no_data_dir").to_string(),
            Self::InvalidShortName(s) => tr.t_fmt("claude.gate.invalid_short_name", s),
            Self::UnknownGate(id) => tr.t_fmt("claude.gate.unknown_gate", id),
            Self::BodyNotReadable { path, message } => tr
                .t("claude.gate.body_not_readable")
                .replacen("{}", path, 1)
                .replacen("{}", message, 1),
            Self::BodyMissingSentinel { sentinel } => {
                tr.t_fmt("claude.gate.body_missing_sentinel", sentinel)
            }
            Self::EmptySentinel => tr.t("claude.gate.empty_sentinel").to_string(),
            Self::RoundsBelowOne => tr.t("claude.gate.rounds_below_one").to_string(),
            // 메시지가 이름을 두 번 쓴다(충돌한 이름 + 해제 명령 예시) — `t_fmt` 는
            // 첫 `{}` 하나만 채우므로 두 자리 이상은 `replacen` 을 겹쳐 쓴다.
            Self::ProfileNameConflict(name) => tr
                .t("claude.gate.profile_name_conflict")
                .replacen("{}", name, 1)
                .replacen("{}", name, 1),
            Self::Io { path, message } => tr
                .t("claude.gate.io_error")
                .replacen("{}", path, 1)
                .replacen("{}", message, 1),
        }
    }
}

/// 게이트 정의 — 본문을 뺀 나머지 2요소. 본문은 별도 파일이라 이 구조체에 담기지
/// 않는다(`show` 가 함께 돌려준다).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GateDef {
    /// 종료 선언 센티넬. 미지정으로 등록하면 기본 센티넬이 여기 실체화된다 —
    /// 정의 파일만 보고도 실효값을 알 수 있어야 하기 때문.
    pub sentinel: String,
    /// 라운드 상한. `None` 이면 Settings → `DEFAULT_ROUND_LIMIT` 순 폴백(해석은
    /// 훅 발화 시점의 몫이라 이 모듈은 미지정을 그대로 보존한다).
    pub round_limit: Option<u32>,
}

impl GateDef {
    fn to_json(&self) -> Value {
        match self.round_limit {
            Some(n) => json!({ "sentinel": self.sentinel, "round_limit": n }),
            None => json!({ "sentinel": self.sentinel }),
        }
    }

    /// 미지의 필드는 무시하고 아는 것만 읽는다 — 정의 파일은 tasty 가 쓰고 tasty 가
    /// 읽으므로, 구버전이 신버전 파일을 만나도 아는 범위에서 동작하는 편이 낫다.
    fn from_json(v: &Value) -> Self {
        Self {
            sentinel: v
                .get("sentinel")
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())
                .unwrap_or(SENTINEL)
                .to_string(),
            round_limit: v
                .get("round_limit")
                .and_then(|v| v.as_u64())
                .map(|n| n as u32)
                .filter(|n| *n >= 1),
        }
    }
}

/// `claude gate-list` 가 반환하는 항목 요약. `round_limit` 이 `None` 인 항목은
/// `round_limit_source` 로 어디서 폴백되는지 알린다 — 실효값을 이 계층에서
/// 확정하지 않는 이유는 Settings 조회가 호스트 왕복(`HostHandle`)을 요구해서다.
#[derive(Debug, Clone)]
pub struct GateSummary {
    /// `<owner>/<short>` 형식.
    pub id: String,
    pub owner: &'static str,
    pub sentinel: String,
    pub round_limit: Option<u32>,
    /// `"gate"`(정의가 직접 지정) 또는 `"settings"`(미지정 → Settings 폴백).
    pub round_limit_source: &'static str,
    /// 이 게이트의 마커가 켜져 있는가(`checklist-enable --gate <name>`). 게이트별
    /// on/off 를 한 번에 보려면 `checklist-status` 를 게이트 수만큼 호출해야 하므로
    /// 목록 쪽에 싣는다 — `checklist-status` 응답 형태는 기존 호출자를 위해
    /// `{ "enabled": bool }` 그대로 둔다.
    pub enabled: bool,
}

/// host 가 코드로 내장한 기본 게이트. 사용자가 같은 이름으로 등록하면 그쪽이
/// 이긴다(`show`/`list` 모두 registered 파일을 먼저 찾고, 없을 때만 여기로 온다).
///
/// `continue-checklist` — 본문은 lang 의 `claude.checklist.body`, 센티넬은
/// [`SENTINEL`], 라운드 상한은 미지정(Settings 폴백). 본문을 여기 상수로 박지 않는
/// 이유는 로케일별로 달라야 하기 때문이고, 그 본문이 센티넬을 포함한다는 불변식은
/// `checklist.rs` 의 `checklist_body_contains_sentinel_in_every_locale` 이 컴파일
/// 타임에 강제한다(사용자 등록 본문에는 같은 불변식을 [`register`] 가 런타임에 건다).
pub fn host_default_gate(short_name: &str, tr: &Translator) -> Option<(GateDef, String)> {
    match short_name {
        DEFAULT_GATE_NAME => Some((
            GateDef {
                sentinel: SENTINEL.to_string(),
                round_limit: None,
            },
            tr.t("claude.checklist.body").to_string(),
        )),
        _ => None,
    }
}

/// 게이트를 지정하지 않은 훅 호출이 해석되는 이름 — host 기본 게이트. 매니페스트
/// `checklist_hook_args` 의 `--gate` 기본값이 이 값과 같아야 하며,
/// `manifest_gate_flag_default_matches_constant` 테스트가 그 일치를 강제한다.
pub(crate) const DEFAULT_GATE_NAME: &str = "continue-checklist";

/// [`host_default_gate`] 가 아는 이름 전체 — `list` 이 host 항목을 나열할 때 순회한다.
const HOST_DEFAULT_GATE_NAMES: &[&str] = &[DEFAULT_GATE_NAME];

fn registered_dir(data_dir: &Path) -> PathBuf {
    data_dir.join("gates").join("registered")
}

fn bodies_dir(data_dir: &Path) -> PathBuf {
    data_dir.join("gates").join("bodies")
}

fn registered_file(data_dir: &Path, short_name: &str) -> PathBuf {
    registered_dir(data_dir).join(format!("{short_name}.json"))
}

fn body_file(data_dir: &Path, short_name: &str) -> PathBuf {
    bodies_dir(data_dir).join(format!("{short_name}.md"))
}

fn require_data_dir(data_dir: Option<&Path>) -> Result<&Path, GateError> {
    data_dir.ok_or(GateError::NoDataDir)
}

/// 이 이름으로 등록된 사용자 게이트가 있는가 — `profile::register` 가 반대 방향
/// 충돌을 검사할 때 쓴다(이름 공간이 한 평면이라는 계약의 대칭 절반).
pub(crate) fn is_registered(data_dir: Option<&Path>, short_name: &str) -> bool {
    data_dir.is_some_and(|d| registered_file(d, short_name).is_file())
}

/// 이 이름의 게이트가 실재하는가(등록 게이트 또는 host 기본 게이트) — 마커를
/// 켜고 끄는 진입점이 오타를 거를 때 쓴다. [`show`] 와 달리 본문 파일을 읽지
/// 않는다: 마커 토글에 본문은 필요 없고, 본문이 읽히지 않는다는 이유로 이미
/// 켜 둔 게이트를 끄지 못하게 되면 곤란하다.
pub(crate) fn ensure_known(
    data_dir: Option<&Path>,
    short_name: &str,
    tr: &Translator,
) -> Result<(), GateError> {
    if !is_valid_short_name(short_name) {
        return Err(GateError::InvalidShortName(short_name.to_string()));
    }
    if is_registered(data_dir, short_name) || host_default_gate(short_name, tr).is_some() {
        return Ok(());
    }
    Err(GateError::UnknownGate(format!("user/{short_name}")))
}

// ── 등록/조회/해제 (user 출처) ────────────────────────────────────────────

/// `short_name` 으로 게이트를 등록한다. `body_path` 의 내용을 읽어 실효 센티넬
/// 포함 여부를 검증한 뒤 정의 JSON 과 본문 복사본을 함께 쓴다 — 원본이 나중에
/// 옮겨지거나 지워져도 게이트는 영향받지 않는다. 이미 등록된 이름이면 정의와
/// 본문을 **둘 다** 덮어쓴다(재등록으로 갱신하는 것이 정상 사용).
///
/// 검증은 파일을 하나라도 쓰기 **전에** 전부 끝낸다 — 정의만 쓰고 본문에서 실패하면
/// 본문 없는 게이트가 남는다.
pub fn register(
    data_dir: Option<&Path>,
    short_name: &str,
    body_path: &Path,
    sentinel: Option<&str>,
    round_limit: Option<u32>,
) -> Result<(), GateError> {
    if !is_valid_short_name(short_name) {
        return Err(GateError::InvalidShortName(short_name.to_string()));
    }
    let data_dir = require_data_dir(data_dir)?;

    // 이름 공간이 프로필과 한 평면이다 — 조용히 가리지 않고 여기서 거부한다.
    if crate::profile::is_registered(Some(data_dir), short_name) {
        return Err(GateError::ProfileNameConflict(short_name.to_string()));
    }

    let sentinel = match sentinel {
        Some("") => return Err(GateError::EmptySentinel),
        Some(s) => s.to_string(),
        None => SENTINEL.to_string(),
    };
    if round_limit == Some(0) {
        return Err(GateError::RoundsBelowOne);
    }

    let body = std::fs::read_to_string(body_path).map_err(|e| GateError::BodyNotReadable {
        path: body_path.display().to_string(),
        message: e.to_string(),
    })?;
    if !body.contains(&sentinel) {
        return Err(GateError::BodyMissingSentinel { sentinel });
    }

    let def = GateDef {
        sentinel,
        round_limit,
    };

    let def_dir = registered_dir(data_dir);
    std::fs::create_dir_all(&def_dir).map_err(|e| GateError::Io {
        path: def_dir.display().to_string(),
        message: e.to_string(),
    })?;
    let body_dir = bodies_dir(data_dir);
    std::fs::create_dir_all(&body_dir).map_err(|e| GateError::Io {
        path: body_dir.display().to_string(),
        message: e.to_string(),
    })?;

    // 본문을 먼저 쓴다 — 정의 파일의 존재가 "이 게이트는 쓸 수 있다" 의 신호이므로,
    // 그 신호가 본문보다 먼저 나타나면 안 된다.
    let body_dest = body_file(data_dir, short_name);
    std::fs::write(&body_dest, &body).map_err(|e| GateError::Io {
        path: body_dest.display().to_string(),
        message: e.to_string(),
    })?;
    let def_dest = registered_file(data_dir, short_name);
    let text = serde_json::to_string_pretty(&def.to_json()).unwrap_or_else(|_| "{}".to_string());
    std::fs::write(&def_dest, text).map_err(|e| GateError::Io {
        path: def_dest.display().to_string(),
        message: e.to_string(),
    })?;
    Ok(())
}

/// `short_name` 등록을 해제한다. 정의와 본문을 **둘 다** 지운다 — 본문만 남으면
/// 다음 등록이 조용히 옛 본문을 덮어쓰는 것처럼 보이는 orphan 이 된다. 없으면
/// `UnknownGate`.
pub fn unregister(data_dir: Option<&Path>, short_name: &str) -> Result<(), GateError> {
    // 이름이 그대로 파일명이 되므로 삭제도 등록과 같은 관문을 통과해야 한다 —
    // 검증 없이 경로를 조립하면 `../` 로 data_dir 밖 파일을 지울 수 있다.
    if !is_valid_short_name(short_name) {
        return Err(GateError::InvalidShortName(short_name.to_string()));
    }
    let data_dir = require_data_dir(data_dir)?;
    let def = registered_file(data_dir, short_name);
    if !def.is_file() {
        return Err(GateError::UnknownGate(format!("user/{short_name}")));
    }
    std::fs::remove_file(&def).map_err(|e| GateError::Io {
        path: def.display().to_string(),
        message: e.to_string(),
    })?;
    let body = body_file(data_dir, short_name);
    // 본문이 이미 없는 상태(수동 삭제 등)는 정상 완료로 본다 — 목표는 "둘 다 없음".
    if body.is_file() {
        std::fs::remove_file(&body).map_err(|e| GateError::Io {
            path: body.display().to_string(),
            message: e.to_string(),
        })?;
    }
    Ok(())
}

/// 게이트 정의 + 본문을 반환한다. 반환하는 owner prefix(`"user"`/`"host"`)는 실제로
/// 어느 출처에서 읽었는지를 그대로 반영한다 — 사용자 등록이 없어 host 기본값으로
/// fallback 됐는데 `"user/..."` 라고 답하면 호출자가 실체와 다른 id 로 착각한다
/// (`profile::show_registered` 와 같은 이유).
///
/// `data_dir` 이 `None` 이어도 host 기본 게이트는 조회된다 — 조회는 저장소를
/// 요구하지 않는다(등록/해제만 명시적 에러).
pub fn show(
    data_dir: Option<&Path>,
    short_name: &str,
    tr: &Translator,
) -> Result<(&'static str, GateDef, String), GateError> {
    // 조회도 같은 관문 — 검증을 건너뛰면 `../` 로 data_dir 밖 파일 내용을 그대로
    // 돌려주는 읽기 통로가 된다.
    if !is_valid_short_name(short_name) {
        return Err(GateError::InvalidShortName(short_name.to_string()));
    }
    if let Some(data_dir) = data_dir {
        let def_path = registered_file(data_dir, short_name);
        if def_path.is_file() {
            let text = std::fs::read_to_string(&def_path).map_err(|e| GateError::Io {
                path: def_path.display().to_string(),
                message: e.to_string(),
            })?;
            let value: Value = serde_json::from_str(&text).map_err(|e| GateError::Io {
                path: def_path.display().to_string(),
                message: e.to_string(),
            })?;
            let body_path = body_file(data_dir, short_name);
            let body = std::fs::read_to_string(&body_path).map_err(|e| GateError::Io {
                path: body_path.display().to_string(),
                message: e.to_string(),
            })?;
            return Ok(("user", GateDef::from_json(&value), body));
        }
    }
    match host_default_gate(short_name, tr) {
        Some((def, body)) => Ok(("host", def, body)),
        None => Err(GateError::UnknownGate(format!("user/{short_name}"))),
    }
}

/// 등록된 사용자 게이트 + host 기본 게이트를 함께 나열한다. host 를 먼저 보여준다 —
/// "항상 있는 것" 이 먼저 눈에 띄어야 한다(`profile::list` 와 같은 정렬 방침).
///
/// 사용자가 host 기본 게이트와 같은 이름으로 등록했으면 그 이름은 **user 항목으로만**
/// 나온다 — 같은 이름이 두 줄로 보이면 어느 쪽이 실효인지 목록만 봐서는 알 수 없다.
pub fn list(data_dir: Option<&Path>, tr: &Translator) -> Vec<GateSummary> {
    let mut user_names: Vec<String> = Vec::new();
    if let Some(data_dir) = data_dir {
        user_names = std::fs::read_dir(registered_dir(data_dir))
            .into_iter()
            .flatten()
            .filter_map(|e| e.ok())
            .filter_map(|e| {
                let name = e.file_name().to_string_lossy().into_owned();
                name.strip_suffix(".json").map(str::to_string)
            })
            .collect();
        user_names.sort();
    }

    let mut out: Vec<GateSummary> = Vec::new();
    for name in HOST_DEFAULT_GATE_NAMES {
        if user_names.iter().any(|u| u == name) {
            continue;
        }
        if let Some((def, _body)) = host_default_gate(name, tr) {
            let enabled = crate::checklist::marker_present(data_dir, name);
            out.push(summary(format!("host/{name}"), "host", def, enabled));
        }
    }
    for name in &user_names {
        // 정의를 못 읽는 항목(손상/권한)은 목록에서 빼지 않고 기본값으로 보여준다 —
        // 목록에서 사라지면 사용자가 지울 대상조차 찾지 못한다.
        let def = data_dir
            .map(|d| registered_file(d, name))
            .and_then(|p| std::fs::read_to_string(p).ok())
            .and_then(|t| serde_json::from_str::<Value>(&t).ok())
            .map(|v| GateDef::from_json(&v))
            .unwrap_or_else(|| GateDef {
                sentinel: SENTINEL.to_string(),
                round_limit: None,
            });
        let enabled = crate::checklist::marker_present(data_dir, name);
        out.push(summary(format!("user/{name}"), "user", def, enabled));
    }
    out
}

fn summary(id: String, owner: &'static str, def: GateDef, enabled: bool) -> GateSummary {
    GateSummary {
        id,
        owner,
        round_limit_source: if def.round_limit.is_some() {
            "gate"
        } else {
            "settings"
        },
        sentinel: def.sentinel,
        round_limit: def.round_limit,
        enabled,
    }
}

// ── IPC handler (main.rs 배선 대상) ────────────────────────────────────────

fn require_name<'a>(params: &'a Value, tr: &Translator) -> Result<&'a str, IpcMethodError> {
    params
        .get("name")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| IpcMethodError::invalid_params(tr.t("claude.params.missing_name")))
}

pub(crate) fn to_ipc_err(e: GateError, tr: &Translator) -> IpcMethodError {
    IpcMethodError::new(tr.t_fmt("claude.gate.error_prefix", &e.translate(tr)))
}

fn summary_to_json(s: &GateSummary) -> Value {
    json!({
        "id": s.id,
        "owner": s.owner,
        "sentinel": s.sentinel,
        "round_limit": s.round_limit,
        "round_limit_source": s.round_limit_source,
        "enabled": s.enabled,
    })
}

/// `claude.gate_register` — `--name <short> --body-file <path> [--sentinel <s>]
/// [--rounds <n>]`. `body_file` 은 CLI `path_kind = "file"` 정규화를 이미 거친
/// 절대경로.
pub(crate) fn handle_register(
    data_dir: Option<&Path>,
    params: &Value,
    tr: &Translator,
) -> Result<Value, IpcMethodError> {
    let name = require_name(params, tr)?;
    let body_file = params
        .get("body_file")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| IpcMethodError::invalid_params(tr.t("claude.params.missing_body_file")))?;
    let sentinel = params
        .get("sentinel")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty());
    // `--sentinel ""` 는 CLI 에서 빈 문자열로 도착하는데, 위 filter 가 그걸 `None`
    // (미지정)으로 접어버리면 "빈 센티넬은 거부" 계약이 조용히 무력화된다. 키가
    // 존재하는데 값이 빈 경우를 따로 잡아 `EmptySentinel` 로 보낸다.
    if sentinel.is_none()
        && params
            .get("sentinel")
            .and_then(|v| v.as_str())
            .is_some_and(str::is_empty)
    {
        return Err(to_ipc_err(GateError::EmptySentinel, tr));
    }
    let rounds = params
        .get("rounds")
        .and_then(|v| v.as_u64())
        .map(|n| n as u32);
    register(data_dir, name, Path::new(body_file), sentinel, rounds)
        .map_err(|e| to_ipc_err(e, tr))?;
    Ok(json!({ "id": format!("user/{name}") }))
}

/// `claude.gate_unregister` — `--name <short>`.
pub(crate) fn handle_unregister(
    data_dir: Option<&Path>,
    params: &Value,
    tr: &Translator,
) -> Result<Value, IpcMethodError> {
    let name = require_name(params, tr)?;
    unregister(data_dir, name).map_err(|e| to_ipc_err(e, tr))?;
    Ok(json!({ "id": format!("user/{name}") }))
}

/// `claude.gate_list` — 등록 게이트 + host 기본 게이트.
pub(crate) fn handle_list(
    data_dir: Option<&Path>,
    _params: &Value,
    tr: &Translator,
) -> Result<Value, IpcMethodError> {
    let entries: Vec<Value> = list(data_dir, tr).iter().map(summary_to_json).collect();
    Ok(json!({ "gates": entries }))
}

/// `claude.gate_show` — 정의 + 본문 텍스트.
pub(crate) fn handle_show(
    data_dir: Option<&Path>,
    params: &Value,
    tr: &Translator,
) -> Result<Value, IpcMethodError> {
    let name = require_name(params, tr)?;
    let (owner, def, body) = show(data_dir, name, tr).map_err(|e| to_ipc_err(e, tr))?;
    Ok(json!({
        "id": format!("{owner}/{name}"),
        "owner": owner,
        "sentinel": def.sentinel,
        "round_limit": def.round_limit,
        "round_limit_source": if def.round_limit.is_some() { "gate" } else { "settings" },
        "body": body,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_translator() -> Translator {
        let lang_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("lang");
        Translator::load(&lang_dir, "en")
    }

    /// 센티넬을 포함한 본문 파일을 만들어 경로를 돌려준다 — 등록이 통과해야 하는
    /// 케이스의 공통 준비.
    fn body_with(tmp: &Path, name: &str, text: &str) -> PathBuf {
        let p = tmp.join(name);
        std::fs::write(&p, text).unwrap();
        p
    }

    fn write_profile(data_dir: &Path, short_name: &str, content: &str) {
        let dir = data_dir.join("profiles").join("registered");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(format!("{short_name}.json")), content).unwrap();
    }

    #[test]
    fn register_copies_body_and_show_returns_it() {
        let tmp = tempfile::tempdir().unwrap();
        let src = body_with(tmp.path(), "b.md", &format!("do the thing\n{SENTINEL}\n"));
        register(Some(tmp.path()), "mygate", &src, None, Some(5)).unwrap();

        assert!(registered_file(tmp.path(), "mygate").is_file());
        assert!(body_file(tmp.path(), "mygate").is_file());

        let (owner, def, body) = show(Some(tmp.path()), "mygate", &test_translator()).unwrap();
        assert_eq!(owner, "user");
        assert_eq!(def.sentinel, SENTINEL);
        assert_eq!(def.round_limit, Some(5));
        assert!(body.contains("do the thing"));

        // 원본이 사라져도 게이트는 살아 있다(복사본 방침).
        std::fs::remove_file(&src).unwrap();
        let (_, _, body) = show(Some(tmp.path()), "mygate", &test_translator()).unwrap();
        assert!(body.contains("do the thing"));
    }

    #[test]
    fn register_rejects_body_without_sentinel() {
        let tmp = tempfile::tempdir().unwrap();
        let src = body_with(tmp.path(), "b.md", "no sentinel here\n");
        let err = register(Some(tmp.path()), "bad", &src, None, None).unwrap_err();
        assert!(matches!(err, GateError::BodyMissingSentinel { .. }));
        // 거부됐으면 아무 파일도 남지 않아야 한다.
        assert!(!registered_file(tmp.path(), "bad").exists());
        assert!(!body_file(tmp.path(), "bad").exists());
    }

    #[test]
    fn register_with_custom_sentinel_validates_against_that_sentinel() {
        let tmp = tempfile::tempdir().unwrap();
        // 기본 센티넬은 있지만 커스텀 센티넬은 없는 본문 → 커스텀 기준으로 거부.
        let only_default = body_with(tmp.path(), "a.md", &format!("{SENTINEL}\n"));
        let err = register(
            Some(tmp.path()),
            "custom",
            &only_default,
            Some("<<MINE>>"),
            None,
        )
        .unwrap_err();
        assert!(matches!(err, GateError::BodyMissingSentinel { .. }));

        // 커스텀 센티넬을 담은 본문은 기본 센티넬이 없어도 통과.
        let only_custom = body_with(tmp.path(), "b.md", "finish with <<MINE>>\n");
        register(
            Some(tmp.path()),
            "custom",
            &only_custom,
            Some("<<MINE>>"),
            None,
        )
        .unwrap();
        let (_, def, _) = show(Some(tmp.path()), "custom", &test_translator()).unwrap();
        assert_eq!(def.sentinel, "<<MINE>>");
    }

    #[test]
    fn register_rejects_empty_sentinel() {
        let tmp = tempfile::tempdir().unwrap();
        let src = body_with(tmp.path(), "b.md", "anything\n");
        let err = register(Some(tmp.path()), "empty", &src, Some(""), None).unwrap_err();
        assert_eq!(err, GateError::EmptySentinel);
    }

    #[test]
    fn register_rejects_invalid_short_name() {
        let tmp = tempfile::tempdir().unwrap();
        let src = body_with(tmp.path(), "b.md", &format!("{SENTINEL}\n"));
        let err = register(Some(tmp.path()), "Bad Name!", &src, None, None).unwrap_err();
        assert!(matches!(err, GateError::InvalidShortName(_)));
    }

    /// data_dir 밖으로 새는 이름을 만들어 준다 — 게이트 경로가 `data_dir/gates/
    /// {registered,bodies}/<name>.<ext>` 라, `../` 3개면 tempdir 루트로 빠져나간다.
    /// 검증이 없으면 그 파일이 실제 대상이 된다는 것이 이 이름이 증명하는 것.
    fn escaping_name() -> &'static str {
        "../../../outside"
    }

    #[test]
    fn unregister_rejects_path_traversal_name_and_leaves_outside_file_intact() {
        let tmp = tempfile::tempdir().unwrap();
        let data_dir = tmp.path().join("data");
        std::fs::create_dir_all(registered_dir(&data_dir)).unwrap();
        std::fs::create_dir_all(bodies_dir(&data_dir)).unwrap();

        // 검증이 빠지면 unregister 가 지웠을 바로 그 두 경로.
        let victim_def = registered_file(&data_dir, escaping_name());
        let victim_body = body_file(&data_dir, escaping_name());
        std::fs::write(&victim_def, "{}").unwrap();
        std::fs::write(&victim_body, "outside body").unwrap();

        let err = unregister(Some(&data_dir), escaping_name()).unwrap_err();
        assert!(matches!(err, GateError::InvalidShortName(_)));
        assert!(victim_def.is_file(), "data_dir 밖 정의 파일이 삭제됐다");
        assert!(victim_body.is_file(), "data_dir 밖 본문 파일이 삭제됐다");
    }

    #[test]
    fn show_rejects_path_traversal_name_and_does_not_leak_outside_file() {
        let tmp = tempfile::tempdir().unwrap();
        let data_dir = tmp.path().join("data");
        std::fs::create_dir_all(registered_dir(&data_dir)).unwrap();
        std::fs::create_dir_all(bodies_dir(&data_dir)).unwrap();

        // 검증이 빠지면 show 가 읽어 반환했을 바로 그 두 경로.
        std::fs::write(
            registered_file(&data_dir, escaping_name()),
            "{\"sentinel\":\"X\"}",
        )
        .unwrap();
        std::fs::write(body_file(&data_dir, escaping_name()), "SECRET-BODY\n").unwrap();

        let err = show(Some(&data_dir), escaping_name(), &test_translator()).unwrap_err();
        assert!(matches!(err, GateError::InvalidShortName(_)));

        // data_dir 이 없을 때도 host fallback 으로 새지 않는다.
        let err = show(None, escaping_name(), &test_translator()).unwrap_err();
        assert!(matches!(err, GateError::InvalidShortName(_)));
    }

    #[test]
    fn register_rejects_when_profile_with_same_name_exists() {
        let tmp = tempfile::tempdir().unwrap();
        write_profile(tmp.path(), "clash", "{}");
        let src = body_with(tmp.path(), "b.md", &format!("{SENTINEL}\n"));
        let err = register(Some(tmp.path()), "clash", &src, None, None).unwrap_err();
        assert!(matches!(err, GateError::ProfileNameConflict(_)));
    }

    /// 대칭 방향 — 게이트가 먼저 있으면 동명 프로필 등록이 거부된다.
    #[test]
    fn profile_register_rejects_when_gate_with_same_name_exists() {
        let tmp = tempfile::tempdir().unwrap();
        let body = body_with(tmp.path(), "b.md", &format!("{SENTINEL}\n"));
        register(Some(tmp.path()), "clash", &body, None, None).unwrap();

        let profile_src = tmp.path().join("p.json");
        std::fs::write(&profile_src, "{}").unwrap();
        let err = crate::profile::register(Some(tmp.path()), "clash", &profile_src).unwrap_err();
        assert!(matches!(
            err,
            crate::profile::ProfileError::GateNameConflict(_)
        ));
    }

    #[test]
    fn register_rejects_rounds_below_one() {
        let tmp = tempfile::tempdir().unwrap();
        let src = body_with(tmp.path(), "b.md", &format!("{SENTINEL}\n"));
        let err = register(Some(tmp.path()), "zero", &src, None, Some(0)).unwrap_err();
        assert_eq!(err, GateError::RoundsBelowOne);
    }

    #[test]
    fn register_without_data_dir_is_rejected() {
        let tmp = tempfile::tempdir().unwrap();
        let src = body_with(tmp.path(), "b.md", &format!("{SENTINEL}\n"));
        let err = register(None, "x", &src, None, None).unwrap_err();
        assert_eq!(err, GateError::NoDataDir);
    }

    #[test]
    fn reregister_overwrites_definition_and_body() {
        let tmp = tempfile::tempdir().unwrap();
        let first = body_with(tmp.path(), "a.md", &format!("first\n{SENTINEL}\n"));
        register(Some(tmp.path()), "g", &first, None, Some(2)).unwrap();

        let second = body_with(tmp.path(), "b.md", &format!("second\n{SENTINEL}\n"));
        register(Some(tmp.path()), "g", &second, None, None).unwrap();

        let (_, def, body) = show(Some(tmp.path()), "g", &test_translator()).unwrap();
        assert!(body.contains("second"));
        assert!(!body.contains("first"));
        assert_eq!(def.round_limit, None, "재등록이 정의도 함께 갈아치운다");
    }

    #[test]
    fn unregister_removes_definition_and_body() {
        let tmp = tempfile::tempdir().unwrap();
        let src = body_with(tmp.path(), "b.md", &format!("{SENTINEL}\n"));
        register(Some(tmp.path()), "gone", &src, None, None).unwrap();

        unregister(Some(tmp.path()), "gone").unwrap();
        assert!(!registered_file(tmp.path(), "gone").exists());
        assert!(
            !body_file(tmp.path(), "gone").exists(),
            "본문만 남으면 orphan 이다"
        );

        let err = unregister(Some(tmp.path()), "gone").unwrap_err();
        assert!(matches!(err, GateError::UnknownGate(_)));
    }

    #[test]
    fn list_includes_host_default_and_user_gates() {
        let tmp = tempfile::tempdir().unwrap();
        let src = body_with(tmp.path(), "b.md", &format!("{SENTINEL}\n"));
        register(Some(tmp.path()), "mygate", &src, None, Some(5)).unwrap();

        let entries = list(Some(tmp.path()), &test_translator());
        let host = entries
            .iter()
            .find(|e| e.id == "host/continue-checklist")
            .expect("host default gate is listed");
        assert_eq!(host.sentinel, SENTINEL);
        assert_eq!(host.round_limit, None);
        assert_eq!(host.round_limit_source, "settings");

        let user = entries
            .iter()
            .find(|e| e.id == "user/mygate")
            .expect("user gate is listed");
        assert_eq!(user.round_limit, Some(5));
        assert_eq!(user.round_limit_source, "gate");
    }

    /// `gate-list` 가 게이트별 on/off 를 보여준다 — 게이트마다
    /// `checklist-status` 를 부르지 않고 한 번에 보려면 이 필드가 필요하다.
    #[test]
    fn list_reports_each_gates_enabled_state() {
        let tmp = tempfile::tempdir().unwrap();
        let tr = test_translator();
        let src = body_with(tmp.path(), "b.md", &format!("{SENTINEL}\n"));
        register(Some(tmp.path()), "mygate", &src, None, None).unwrap();

        let off = list(Some(tmp.path()), &tr);
        assert!(
            off.iter().all(|e| !e.enabled),
            "켠 적 없는데 on 으로 보인다"
        );

        crate::checklist::handle_enable(Some(tmp.path()), &json!({ "gate": "mygate" }), &tr)
            .unwrap();
        let on = list(Some(tmp.path()), &tr);
        assert!(on.iter().find(|e| e.id == "user/mygate").unwrap().enabled);
        assert!(
            !on.iter()
                .find(|e| e.id == "host/continue-checklist")
                .unwrap()
                .enabled,
            "다른 게이트를 켰는데 host 기본 게이트가 on 으로 보인다"
        );
    }

    /// 마커 조회가 `data_dir` 없이도 답해야 목록이 host 기본 게이트를 계속 보여준다.
    #[test]
    fn list_without_data_dir_reports_gates_as_off() {
        assert!(list(None, &test_translator()).iter().all(|e| !e.enabled));
    }

    #[test]
    fn list_without_data_dir_shows_only_host_default() {
        let entries = list(None, &test_translator());
        assert!(!entries.is_empty());
        assert!(entries.iter().all(|e| e.owner == "host"));
        assert!(entries.iter().any(|e| e.id == "host/continue-checklist"));
    }

    #[test]
    fn show_falls_back_to_host_default_gate() {
        let tmp = tempfile::tempdir().unwrap();
        let tr = test_translator();
        let (owner, def, body) = show(Some(tmp.path()), "continue-checklist", &tr).unwrap();
        assert_eq!(owner, "host");
        assert_eq!(def.sentinel, SENTINEL);
        assert_eq!(def.round_limit, None);
        assert!(
            body.contains(SENTINEL),
            "host 기본 본문은 센티넬을 포함한다"
        );

        // 모르는 이름은 host 폴백도 없다.
        let err = show(Some(tmp.path()), "nope", &tr).unwrap_err();
        assert!(matches!(err, GateError::UnknownGate(_)));
    }

    #[test]
    fn user_gate_shadows_host_default_of_same_name() {
        let tmp = tempfile::tempdir().unwrap();
        let src = body_with(tmp.path(), "b.md", "my own checklist <<MINE>>\n");
        register(
            Some(tmp.path()),
            "continue-checklist",
            &src,
            Some("<<MINE>>"),
            Some(7),
        )
        .unwrap();

        let (owner, def, body) =
            show(Some(tmp.path()), "continue-checklist", &test_translator()).unwrap();
        assert_eq!(owner, "user");
        assert_eq!(def.sentinel, "<<MINE>>");
        assert_eq!(def.round_limit, Some(7));
        assert!(body.contains("my own checklist"));

        // 목록에는 같은 이름이 두 줄로 나오지 않는다 — 실효 항목 하나만.
        let entries = list(Some(tmp.path()), &test_translator());
        let matching: Vec<&GateSummary> = entries
            .iter()
            .filter(|e| e.id.ends_with("/continue-checklist"))
            .collect();
        assert_eq!(matching.len(), 1);
        assert_eq!(matching[0].id, "user/continue-checklist");
    }

    /// 등록 시점에 미지정한 센티넬은 정의 파일에 기본값으로 실체화된다 — 정의만
    /// 보고도 실효값을 알 수 있어야 한다.
    #[test]
    fn omitted_sentinel_is_materialized_as_the_default() {
        let tmp = tempfile::tempdir().unwrap();
        let src = body_with(tmp.path(), "b.md", &format!("{SENTINEL}\n"));
        register(Some(tmp.path()), "g", &src, None, None).unwrap();
        let text = std::fs::read_to_string(registered_file(tmp.path(), "g")).unwrap();
        let v: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(v["sentinel"], SENTINEL);
        assert!(v.get("round_limit").is_none());
    }
}

#[cfg(test)]
mod message_tests {
    use super::*;

    /// 충돌 메시지는 이름을 두 번 쓴다(충돌한 이름 + 해제 명령 예시). `t_fmt` 로
    /// 넘기면 두 번째 `{}` 가 그대로 새어나가 사용자에게 리터럴 중괄호가 보인다.
    #[test]
    fn name_conflict_messages_fill_every_placeholder() {
        let lang_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("lang");
        for locale in ["en", "ko", "ja"] {
            let tr = Translator::load(&lang_dir, locale);

            let gate_side = GateError::ProfileNameConflict("mygate".into()).translate(&tr);
            assert!(gate_side.contains("mygate"), "{locale}: {gate_side}");
            assert!(
                !gate_side.contains("{}"),
                "{locale}: 채워지지 않은 placeholder 가 남았다: {gate_side}"
            );

            let profile_side =
                crate::profile::ProfileError::GateNameConflict("mygate".into()).translate(&tr);
            assert!(profile_side.contains("mygate"), "{locale}: {profile_side}");
            assert!(
                !profile_side.contains("{}"),
                "{locale}: 채워지지 않은 placeholder 가 남았다: {profile_side}"
            );
        }
    }
}
