//! Claude 세션 프로필 **부착 기록** — session id 로 키잉해 plugin data dir 에 남긴다.
//!
//! ## 왜 필요한가
//!
//! 부착 상태의 원본 저장처는 surface meta 2 키(`claude-session-profile{,-names}`,
//! [`crate::reboot`])인데, surface meta 는 앱 재시작/닫은 탭 복원을 **구조적으로**
//! 넘지 못한다 — 복원은 stale id 와 겹치지 않는 새 surface id 를 발급하고
//! (`bump_surface_floor`) 곧바로 live 아닌 `Scope::Surface` meta 를 purge 한다.
//! 복원된 프로세스에 프로필을 다시 실을 유일한 창구는 `restore.command` 문자열이며,
//! 그 문자열을 다시 쓰려면 "이 세션에 무엇이 붙어 있었나" 를 surface 바깥에 남겨야
//! 한다. 세션 id 는 `claude -r <id>` 를 건너 보존되므로(실측) 그 키가 될 수 있다.
//!
//! ## 저장 위치
//!
//! `TASTY_PLUGIN_DATA_DIR/profiles/attachments/<session_id>.json` — 같은 plugin 의
//! checklist 라운드 상태(`checklist/gates/<gate>/rounds/<session_id>.json`)와 동형이다.
//! plugin data dir 은 설치 디렉터리와 분리돼 있어 `upgrade-builtins`/재설치를 건너
//! 보존된다(`docs/dev-guide/plugin-development.md` "plugin data dir 수명 계약").
//!
//! ## 무엇을 저장하나
//!
//! **이름으로 부착한 것은 이름을, 경로로 부착한 것은 경로를** 저장한다 — 이름은
//! 복원 시점에 매번 다시 해석해야 최신 등록 내용(그 사이 `profile-register` 로
//! 갱신됐을 수 있다)이 반영되기 때문이다. 해석 결과 경로를 캐시하면 meta 쪽
//! 계약(`PROFILE_NAMES_META_KEY` 주석)과 어긋난다.
//!
//! ## 수명
//!
//! 부착 시 write, session-start 마다 re-stamp, 전역 session-end 에서 **종료 표시**
//! ([`mark_ended`]), 그리고 [`sweep`] 이 회수한다. session-end 가 파일을 즉시 지우지
//! **않는** 이유는 실측 때문이다: 탭을 닫으면 PTY 가 죽으면서 claude 가 `SessionEnd`
//! 를 발화하는데 호스트는 아직 살아 있어 그 훅이 정상 도달한다. 여기서 기록을 지우면
//! 곧바로 이어지는 닫은 탭 복원(Ctrl+Shift+T)이 프로필을 잃는다 — 복원된 프로세스는
//! `restore.command` 덕에 `--settings` 를 달고 뜨지만, meta 를 되살릴 근거가 사라져
//! `profile-current` 와 무인자 reboot 승계가 깨진다.
//!
//! 그래서 session-end 는 `ended_at` 만 찍고, 짧은 유예([`ENDED_GRACE`]) 뒤 sweep 이
//! 회수한다. 기록은 **session id 로 키잉**되므로 그 사이 살아남아도 다른 세션이 읽을
//! 수 없다 — 같은 id 가 다시 나타나는 유일한 경로가 `claude -r`(=복원)이고, 그때는
//! 기록이 남아 있는 편이 정확히 맞다. 강제 종료처럼 훅이 아예 못 뛴 잔재는 긴
//! TTL([`RECORD_TTL`])이 담당한다. `--clear-profile` 만은 사용자가 명시적으로 뗀
//! 것이므로 유예 없이 [`remove`] 로 즉시 지운다.

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::json;
use tracing::warn;

/// 부착 기록 1 건. 이름 부착과 경로 부착을 구분해 보존한다 — 이름은 복원 시점에
/// 재해석하고, 경로는 그대로 쓴다.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum AttachRecord {
    /// `--profile <names>` — 쉼표 구분 이름 목록(등록 프로필 또는 게이트).
    Names(String),
    /// `--profile-file <path>` — 절대 경로.
    Path(String),
}

impl AttachRecord {
    fn kind(&self) -> &'static str {
        match self {
            AttachRecord::Names(_) => "names",
            AttachRecord::Path(_) => "path",
        }
    }

    fn value(&self) -> &str {
        match self {
            AttachRecord::Names(v) | AttachRecord::Path(v) => v,
        }
    }
}

/// orphan 기록 회수 TTL. 넉넉하게 잡는 이유: session-start 마다 re-stamp 로 mtime 이
/// 갱신되므로(앱 재시작·resume 은 모두 session-start 를 발화시킨다) 살아있는 세션의
/// 기록은 실질적으로 계속 젊어진다. 그럼에도 "재기동 없이 아주 오래 도는 세션" 을
/// 오탐으로 지우면 다음 복원에서 프로필이 조용히 빠지므로, 회수는 그 실수가 사실상
/// 불가능한 길이에서만 한다 — 남은 파일은 수백 바이트짜리이고 누적은 오동작이 아니다.
const RECORD_TTL: Duration = Duration::from_secs(90 * 24 * 60 * 60);

/// session-end 로 종료 표시된 기록의 유예. 닫은 탭 복원(Ctrl+Shift+T)은 사용자가
/// 실수를 되돌리는 조작이라 몇 초~몇 분 안에 일어나고, 앱을 껐다 켜서 되살리는
/// 경우까지 감안해도 하루면 충분하다. 이 유예가 지나면 기록은 `restore.command`
/// meta 와 사실상 같은 수명을 가진 셈이 된다.
const ENDED_GRACE: Duration = Duration::from_secs(24 * 60 * 60);

fn attachments_dir(data_dir: &Path) -> PathBuf {
    data_dir.join("profiles").join("attachments")
}

/// 기록 파일 경로. `session_id` 는 [`crate::reboot::is_safe_session_id`] 를 통과한
/// 값이어야 한다 — 그 규칙(영숫자/`-`/`_`)이 경로 조각으로 안전함을 보장한다.
/// 검증은 호출자가 아니라 이 모듈의 각 진입점이 직접 한다(훅 payload 는 외부
/// 입력이라 `..` 같은 조각이 들어올 수 있다).
fn record_file(data_dir: &Path, session_id: &str) -> PathBuf {
    attachments_dir(data_dir).join(format!("{session_id}.json"))
}

/// `data_dir` 과 session id 를 함께 검증한다. 둘 중 하나라도 못 쓰면 기록 없이
/// 진행한다 — 기록은 복원 품질을 높이는 보조 저장소이고, 그 부재가 훅 처리 자체를
/// 실패시킬 이유는 아니다.
fn resolve(data_dir: Option<&Path>, session_id: &str) -> Option<PathBuf> {
    let dir = data_dir?;
    if !crate::reboot::is_safe_session_id(session_id) {
        return None;
    }
    Some(record_file(dir, session_id))
}

/// 부착 기록을 쓴다(있으면 덮어쓴다). 실패는 warn 로그 후 무시.
pub(crate) fn store(data_dir: Option<&Path>, session_id: &str, record: &AttachRecord) {
    let Some(path) = resolve(data_dir, session_id) else {
        return;
    };
    if let Some(parent) = path.parent()
        && let Err(e) = std::fs::create_dir_all(parent)
    {
        warn!(
            "claude profile attach: failed to create {}: {e}",
            parent.display()
        );
        return;
    }
    // ended_at 을 싣지 않는다 — 다시 찍힌 기록은 살아있는 세션의 것이다.
    let text = json!({ "kind": record.kind(), "value": record.value() }).to_string();
    if let Err(e) = std::fs::write(&path, text) {
        warn!(
            "claude profile attach: failed to write {}: {e}",
            path.display()
        );
    }
}

/// 부착 기록을 읽는다. 파일이 없거나 형식이 깨졌으면 `None` — 깨진 기록은 없는
/// 것과 같이 취급하고(프로필 없이 진행) warn 만 남긴다.
pub(crate) fn load(data_dir: Option<&Path>, session_id: &str) -> Option<AttachRecord> {
    let path = resolve(data_dir, session_id)?;
    let text = std::fs::read_to_string(&path).ok()?;
    let value: serde_json::Value = match serde_json::from_str(&text) {
        Ok(v) => v,
        Err(e) => {
            warn!(
                "claude profile attach: malformed record {}: {e}",
                path.display()
            );
            return None;
        }
    };
    let kind = value.get("kind").and_then(|v| v.as_str());
    let inner = value
        .get("value")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty());
    match (kind, inner) {
        (Some("names"), Some(v)) => Some(AttachRecord::Names(v.to_string())),
        (Some("path"), Some(v)) => Some(AttachRecord::Path(v.to_string())),
        _ => {
            warn!(
                "claude profile attach: unusable record {} (kind={kind:?})",
                path.display()
            );
            None
        }
    }
}

/// 기록에 종료 시각을 찍는다 — 전역 session-end 가 쓰는 경로. 파일을 지우지 않는
/// 이유는 모듈 doc "수명" 절 참고(닫은 탭 복원이 기록을 다시 필요로 한다).
/// 기록이 없으면 아무것도 하지 않는다.
pub(crate) fn mark_ended(data_dir: Option<&Path>, session_id: &str) {
    let Some(path) = resolve(data_dir, session_id) else {
        return;
    };
    let Some(record) = load(data_dir, session_id) else {
        return;
    };
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let text =
        json!({ "kind": record.kind(), "value": record.value(), "ended_at": now }).to_string();
    if let Err(e) = std::fs::write(&path, text) {
        warn!(
            "claude profile attach: failed to mark {} ended: {e}",
            path.display()
        );
    }
}

/// 부착 기록을 지운다(`--clear-profile` 처럼 명시적 해제). 없는 파일은 성공으로 본다.
pub(crate) fn remove(data_dir: Option<&Path>, session_id: &str) {
    let Some(path) = resolve(data_dir, session_id) else {
        return;
    };
    if let Err(e) = std::fs::remove_file(&path)
        && e.kind() != std::io::ErrorKind::NotFound
    {
        warn!(
            "claude profile attach: failed to remove {}: {e}",
            path.display()
        );
    }
}

/// TTL 을 넘긴 기록을 best-effort 로 회수한다 — session-start 마다 호출하는 지연
/// 삭제 방식(`handlers::sweep_stale_prompt_files` 와 같은 형태). 실패는 무시한다:
/// 디렉터리 경합이나 권한 문제가 훅 처리를 막을 이유가 아니다.
pub(crate) fn sweep(data_dir: Option<&Path>) {
    sweep_at(data_dir, std::time::SystemTime::now());
}

/// [`sweep`] 의 본체. `now` 를 주입받아 TTL 경과 분기를 플랫폼 중립으로 테스트할 수
/// 있게 한다(파일 mtime 을 과거로 미는 방식은 unix 전용 API 가 필요하다).
fn sweep_at(data_dir: Option<&Path>, now: std::time::SystemTime) {
    let Some(dir) = data_dir.map(attachments_dir) else {
        return;
    };
    let entries = match std::fs::read_dir(&dir) {
        Ok(e) => e,
        Err(e) => {
            // 아직 부착이 한 번도 없었으면 디렉터리 자체가 없다 — 정상.
            tracing::debug!(
                "claude profile attach sweep: read_dir({}) failed: {e}",
                dir.display()
            );
            return;
        }
    };
    let now_epoch = now
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    for entry in entries.flatten() {
        if !entry.file_name().to_string_lossy().ends_with(".json") {
            continue;
        }
        // 종료 표시된 기록은 짧은 유예만 준다 — 그 시점부터는 닫은 탭 복원이
        // 되살릴 여지가 사실상 없다.
        let ended = ended_at(&entry.path());
        let stale = match ended {
            Some(at) => now_epoch.saturating_sub(at) >= ENDED_GRACE.as_secs(),
            None => entry
                .metadata()
                .and_then(|m| m.modified())
                .and_then(|modified| {
                    now.duration_since(modified)
                        .map_err(|e| std::io::Error::other(e.to_string()))
                })
                .is_ok_and(|age| age >= RECORD_TTL),
        };
        if stale && let Err(e) = std::fs::remove_file(entry.path()) {
            tracing::debug!(
                "claude profile attach sweep: remove({:?}) failed: {e}",
                entry.path()
            );
        }
    }
}

/// 기록 파일의 `ended_at`(unix secs). 없거나 못 읽으면 `None`(=살아있는 기록).
fn ended_at(path: &Path) -> Option<u64> {
    let text = std::fs::read_to_string(path).ok()?;
    let value: serde_json::Value = serde_json::from_str(&text).ok()?;
    value.get("ended_at").and_then(|v| v.as_u64())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn store_and_load_round_trips_names() {
        let tmp = tempfile::tempdir().unwrap();
        store(
            Some(tmp.path()),
            "sess-1",
            &AttachRecord::Names("reviewer".into()),
        );
        assert_eq!(
            load(Some(tmp.path()), "sess-1"),
            Some(AttachRecord::Names("reviewer".into()))
        );
    }

    #[test]
    fn store_and_load_round_trips_path() {
        let tmp = tempfile::tempdir().unwrap();
        store(
            Some(tmp.path()),
            "sess-1",
            &AttachRecord::Path("/abs/p.json".into()),
        );
        assert_eq!(
            load(Some(tmp.path()), "sess-1"),
            Some(AttachRecord::Path("/abs/p.json".into()))
        );
    }

    #[test]
    fn store_overwrites_previous_record() {
        let tmp = tempfile::tempdir().unwrap();
        store(Some(tmp.path()), "s", &AttachRecord::Names("a".into()));
        store(Some(tmp.path()), "s", &AttachRecord::Names("b".into()));
        assert_eq!(
            load(Some(tmp.path()), "s"),
            Some(AttachRecord::Names("b".into()))
        );
    }

    #[test]
    fn remove_deletes_the_record_and_is_idempotent() {
        let tmp = tempfile::tempdir().unwrap();
        store(Some(tmp.path()), "s", &AttachRecord::Names("a".into()));
        remove(Some(tmp.path()), "s");
        assert_eq!(load(Some(tmp.path()), "s"), None);
        remove(Some(tmp.path()), "s");
    }

    /// 훅 payload 의 session id 는 외부 입력이라 경로 조각으로 쓰기 전에 걸러야 한다.
    #[test]
    fn unsafe_session_id_never_touches_the_filesystem() {
        let tmp = tempfile::tempdir().unwrap();
        store(
            Some(tmp.path()),
            "../escape",
            &AttachRecord::Names("a".into()),
        );
        assert!(!tmp.path().join("profiles").exists());
        assert_eq!(load(Some(tmp.path()), "../escape"), None);
    }

    #[test]
    fn no_data_dir_is_a_silent_no_op() {
        assert_eq!(load(None, "s"), None);
        store(None, "s", &AttachRecord::Names("a".into()));
        remove(None, "s");
        sweep(None);
    }

    #[test]
    fn malformed_record_reads_as_absent() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = attachments_dir(tmp.path());
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("s.json"), "{not json").unwrap();
        assert_eq!(load(Some(tmp.path()), "s"), None);
        std::fs::write(dir.join("s.json"), r#"{"kind":"bogus","value":"x"}"#).unwrap();
        assert_eq!(load(Some(tmp.path()), "s"), None);
    }

    /// session-end 표시는 기록을 지우지 않는다 — 닫은 탭 복원이 다시 읽어야 한다.
    #[test]
    fn mark_ended_keeps_the_record_readable() {
        let tmp = tempfile::tempdir().unwrap();
        store(Some(tmp.path()), "s", &AttachRecord::Names("probe".into()));
        mark_ended(Some(tmp.path()), "s");
        assert_eq!(
            load(Some(tmp.path()), "s"),
            Some(AttachRecord::Names("probe".into()))
        );
        assert!(ended_at(&record_file(tmp.path(), "s")).is_some());
    }

    /// re-stamp(=session-start)는 종료 표시를 지운다 — 세션이 되살아났다는 뜻이므로.
    #[test]
    fn re_stamping_clears_the_ended_mark() {
        let tmp = tempfile::tempdir().unwrap();
        store(Some(tmp.path()), "s", &AttachRecord::Names("probe".into()));
        mark_ended(Some(tmp.path()), "s");
        store(Some(tmp.path()), "s", &AttachRecord::Names("probe".into()));
        assert!(ended_at(&record_file(tmp.path(), "s")).is_none());
    }

    /// 기록이 없으면 종료 표시는 아무것도 만들지 않는다.
    #[test]
    fn mark_ended_on_a_missing_record_is_a_no_op() {
        let tmp = tempfile::tempdir().unwrap();
        mark_ended(Some(tmp.path()), "s");
        assert_eq!(load(Some(tmp.path()), "s"), None);
    }

    /// 종료 표시된 기록은 유예 안에서는 살아남고(복원 가능), 유예를 넘기면 회수된다.
    #[test]
    fn sweep_respects_the_ended_grace() {
        let tmp = tempfile::tempdir().unwrap();
        store(Some(tmp.path()), "s", &AttachRecord::Names("probe".into()));
        mark_ended(Some(tmp.path()), "s");

        sweep(Some(tmp.path()));
        assert!(record_file(tmp.path(), "s").exists(), "유예 안에서는 보존");

        let future = std::time::SystemTime::now() + ENDED_GRACE + Duration::from_secs(60);
        sweep_at(Some(tmp.path()), future);
        assert!(!record_file(tmp.path(), "s").exists(), "유예를 넘기면 회수");
    }

    /// 살아있는(종료 표시 없는) 기록은 짧은 유예로는 회수되지 않는다 — 긴 TTL 만 본다.
    #[test]
    fn sweep_does_not_apply_the_ended_grace_to_live_records() {
        let tmp = tempfile::tempdir().unwrap();
        store(Some(tmp.path()), "s", &AttachRecord::Names("probe".into()));
        let future = std::time::SystemTime::now() + ENDED_GRACE + Duration::from_secs(60);
        sweep_at(Some(tmp.path()), future);
        assert!(record_file(tmp.path(), "s").exists());
    }

    /// TTL 안쪽 기록은 건드리지 않는다.
    #[test]
    fn sweep_keeps_records_within_ttl() {
        let tmp = tempfile::tempdir().unwrap();
        store(Some(tmp.path()), "fresh", &AttachRecord::Names("a".into()));
        sweep(Some(tmp.path()));
        assert!(record_file(tmp.path(), "fresh").exists());
    }

    /// TTL 을 넘기면 회수된다. mtime 을 과거로 미는 대신 `now` 를 미래로 주입한다 —
    /// 같은 비교식을 플랫폼 중립으로 검증한다.
    #[test]
    fn sweep_removes_records_past_ttl() {
        let tmp = tempfile::tempdir().unwrap();
        store(Some(tmp.path()), "old", &AttachRecord::Names("a".into()));
        let future = std::time::SystemTime::now() + RECORD_TTL + Duration::from_secs(60);
        sweep_at(Some(tmp.path()), future);
        assert!(!record_file(tmp.path(), "old").exists());
    }
}
