//! 자식 CLI 에 넘길 prompt 를 임시파일로 쓰고, 오래된 것을 청소한다.
//!
//! prompt 는 `$(cat '<path>')` 로 기동 명령에 치환되어 들어간다. 이름이 겹치면 자식 셸이
//! **남의 prompt 를 읽거나 빈 문자열을 읽는다**([`write`] 가 지우고 다시 만들기 때문에,
//! 그 사이에 치환이 끼면 빈 파일을 읽는다). 그래서 이름은 두 축으로 갈라져 있다.
//!
//! * **plugin 축** — prefix 가 plugin 마다 다르다. 같은 `surface_id` 로 claude 와 codex 를
//!   동시에 spawn 할 수 있다. 그래서 prefix 는 상수가 아니라 인자다.
//! * **인스턴스 축** — 이름에 이 프로세스의 pid 가 들어간다. `surface_id` 공간은
//!   **인스턴스마다 독립이고 매 실행 1 부터 재발급된다**(`IdGenerator::next_surface`).
//!   즉 한 머신에 tasty 를 두 벌 띄우면 **같은 번호의 surface 가 동시에 산다** — 이 축을
//!   안 가르면 두 인스턴스의 같은 plugin 이 같은 경로를 쓴다. plugin 은 인스턴스마다
//!   별도 프로세스라 pid 가 그 축을 정확히 가른다.

use std::path::{Path, PathBuf};

/// prompt 임시파일 이름 suffix. prefix 와 달리 plugin 마다 다를 이유가 없다 —
/// 청소 스윕이 `prefix`+`suffix` 로 자기 파일만 매칭하는 데만 쓰인다.
pub const SUFFIX: &str = ".txt";

/// prompt 임시파일이 생성된 뒤 이만큼 지나면 다음 spawn 시점에 청소 대상이 된다.
/// 자식 셸이 `$(cat '<path>')` 치환을 끝내는 데는 보통 수 ms~수 초면 충분하므로,
/// 10분이면 그 시간을 넉넉히 넘겨 "아직 안 읽었는데 지워지는" 레이스를 사실상
/// 배제한다.
pub const TTL: std::time::Duration = std::time::Duration::from_secs(600);

/// `dir` 안에서 이 surface 가 쓸 prompt 파일 경로 — `{prefix}{pid}-{surface_id}{SUFFIX}`.
///
/// pid 를 **인자가 아니라 여기서 읽는다.** 소비자가 넘기게 하면 "무엇을 넘길지" 가 호출부
/// 마다 갈릴 수 있는데, 이 값이 갈라야 하는 것은 *이 프로세스* 하나로 정해져 있다.
/// 같은 프로세스 안에서는 결정적이라 테스트가 프로덕션 헬퍼로 경로를 되짚을 수 있다.
///
/// pid 를 prefix **뒤**에 넣는 것이 [`sweep_stale`] 의 전이 처리를 겸한다 — 그 스윕은
/// prefix/suffix 로 매칭하므로 pid 가 없던 옛 이름(`{prefix}{surface_id}{SUFFIX}`)도
/// 그대로 걸린다. 이름 규칙을 바꿔도 옛 파일이 영영 남지 않는다.
pub fn path_for(dir: &Path, prefix: &str, surface_id: u32) -> PathBuf {
    dir.join(format!(
        "{prefix}{}-{surface_id}{SUFFIX}",
        std::process::id()
    ))
}

/// `path` 를 owner-only(0600) 권한으로 새로 만들어 `content` 를 쓴다. 같은 경로에
/// 이전 실행이 남긴 파일이 있으면(과거 버전이 좁히지 않은 권한으로 만들었을 수
/// 있는 파일 포함) 먼저 지우고 다시 만든다 — `OpenOptions::mode` 는 파일을 실제로
/// *생성*하는 순간에만 적용되고, 이미 존재하는 파일을 여는 경우엔 기존 권한이
/// 그대로 유지되기 때문이다(재사용 시 권한이 좁혀지지 않는 구멍을 막는다).
pub fn write(path: &Path, content: &str) -> std::io::Result<()> {
    // 존재하면 지우고 다시 만든다 — 위 doc 의 이유. 없으면 실패하지만 그것이
    // 정상 경로라 결과를 보지 않는다.
    let _ = std::fs::remove_file(path);
    #[cfg(unix)]
    {
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(path)?;
        file.write_all(content.as_bytes())
    }
    #[cfg(not(unix))]
    {
        // Windows 에는 mode 개념이 없다 — 임시 디렉터리의 ACL 을 따른다.
        std::fs::write(path, content)
    }
}

/// `dir` 안에서 `prefix`/[`SUFFIX`] 패턴에 매칭하고 [`TTL`] 보다 오래된(mtime 기준)
/// 파일을 best-effort 로 지운다 — 실패해도 기동을 막지 않는다.
///
/// **판정은 나이 하나다 — 이름 안의 pid 를 살아 있는지 물어보지 않는다.** 그 판정이
/// 가능하긴 하다([`path_for`] 가 pid 를 이름에 남기므로) 하지만 값이 없다: 죽은
/// 프로세스가 남긴 파일도 [`TTL`] 안에 어차피 지워지고, 이득은 그 10 분을 앞당기는 것
/// 뿐이다. 반대편 비용은 크다 — 프로세스 생존 확인은 플랫폼마다 다른 경로를 타고
/// (`/proc` 은 리눅스에만 있다) 이 크레이트에 그런 의존을 새로 들이게 된다. 이 모듈이
/// 프로세스를 만지지 않는 상태로 남는 편이 낫다.
///
/// pid 가 없던 옛 이름도 이 패턴에 걸린다 — [`path_for`] 의 pid 위치 참조.
pub fn sweep_stale(dir: &Path, prefix: &str) {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(e) => {
            tracing::debug!(
                "prompt tempfile sweep: read_dir({}) failed: {e}",
                dir.display()
            );
            return;
        }
    };
    let now = std::time::SystemTime::now();
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if !name.starts_with(prefix) || !name.ends_with(SUFFIX) {
            continue;
        }
        let is_stale = entry
            .metadata()
            .and_then(|m| m.modified())
            .and_then(|modified| {
                now.duration_since(modified)
                    .map_err(|e| std::io::Error::other(e.to_string()))
            })
            .is_ok_and(|age| age >= TTL);
        if is_stale && let Err(e) = std::fs::remove_file(entry.path()) {
            tracing::debug!(
                "prompt tempfile sweep: remove({:?}) failed: {e}",
                entry.path()
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 테스트용 prefix — 실제 plugin 의 값과 무관하게, 매칭이 prefix 인자로만
    /// 결정된다는 것을 함께 보인다.
    const PREFIX: &str = "tasty-test-prompt-";

    fn age_back(path: &Path) {
        let old = std::time::SystemTime::now() - (TTL + std::time::Duration::from_secs(60));
        // Windows `SetFileTime` 은 핸들에 `FILE_WRITE_ATTRIBUTES` 를 요구한다 —
        // `File::open` 의 읽기 전용 핸들로는 `PermissionDenied(os error 5)` 가 난다.
        // POSIX `futimens` 는 읽기 전용 fd 로도 되므로 Linux·macOS 에선 안 드러난다.
        std::fs::OpenOptions::new()
            .write(true)
            .open(path)
            .unwrap()
            .set_modified(old)
            .unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn write_is_owner_only() {
        use std::os::unix::fs::PermissionsExt;
        // 유니크 tempdir 로 격리한다 — 고정 이름은 같은 머신의 다른 완주와 충돌해 확률적 red 가
        // 난다(ADR-0129 형태 B 고정 경로). tempdir 의 Drop 이 정리하므로 수동 remove 는 불필요.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("prompt.txt");
        write(&path, "secret").unwrap();
        let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600, "got mode {mode:o}");
    }

    #[test]
    fn write_narrows_permissions_on_reuse() {
        // 이전 실행이 넓은 권한으로 만든 파일이 같은 경로에 이미 있어도, 다시 쓸 때
        // 0600 으로 좁혀져야 한다(재사용 시 구멍 방지 회귀 가드).
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("prompt.txt");
        std::fs::write(&path, "old").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        }
        write(&path, "new").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "new");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o600, "got mode {mode:o}");
        }
    }

    /// **경로에 이 프로세스의 pid 가 들어간다.** 이것이 인스턴스 축을 가르는 유일한
    /// 수단이다 — `surface_id` 는 인스턴스마다 1 부터 재발급되므로 두 인스턴스에 같은
    /// 번호가 동시에 산다. 이 단언이 죽으면 두 인스턴스가 같은 prompt 파일을 쓴다.
    #[test]
    fn the_path_carries_this_process_id() {
        let name = path_for(Path::new("/tmp"), PREFIX, 7)
            .file_name()
            .expect("file name")
            .to_string_lossy()
            .into_owned();
        assert_eq!(
            name,
            format!("{PREFIX}{}-7{SUFFIX}", std::process::id()),
            "이름 규칙이 바뀌었다"
        );
    }

    /// **pid 가 없던 옛 이름도 스윕에 걸린다.** 이름 규칙을 바꾸면 이전 버전이 남긴
    /// 파일이 새 패턴에 안 걸려 영영 남을 수 있다 — pid 를 prefix 뒤에 넣은 것이 그것을
    /// 막는다. prefix 앞에 넣거나 suffix 를 바꾸면 이 단언이 죽는다.
    #[test]
    fn sweep_still_catches_the_name_shape_that_had_no_pid() {
        let tmp = tempfile::tempdir().unwrap();
        let legacy = tmp.path().join(format!("{PREFIX}7{SUFFIX}"));
        std::fs::write(&legacy, "x").unwrap();
        age_back(&legacy);

        sweep_stale(tmp.path(), PREFIX);

        assert!(!legacy.exists(), "pid 없던 옛 이름이 안 지워졌다");
    }

    #[test]
    fn sweep_removes_files_past_ttl() {
        let tmp = tempfile::tempdir().unwrap();
        let stale = path_for(tmp.path(), PREFIX, 7);
        std::fs::write(&stale, "x").unwrap();
        age_back(&stale);

        sweep_stale(tmp.path(), PREFIX);

        assert!(!stale.exists(), "stale prompt file should have been swept");
    }

    #[test]
    fn sweep_keeps_files_within_ttl() {
        let tmp = tempfile::tempdir().unwrap();
        let fresh = path_for(tmp.path(), PREFIX, 7);
        std::fs::write(&fresh, "x").unwrap();

        sweep_stale(tmp.path(), PREFIX);

        assert!(fresh.exists(), "fresh prompt file should not be swept");
    }

    #[test]
    fn sweep_ignores_non_matching_names() {
        let tmp = tempfile::tempdir().unwrap();
        let unrelated = tmp.path().join("some-other-file.txt");
        std::fs::write(&unrelated, "x").unwrap();
        age_back(&unrelated);

        sweep_stale(tmp.path(), PREFIX);

        assert!(unrelated.exists(), "non-matching file must not be swept");
    }

    /// **다른 plugin 의 prompt 파일은 건드리지 않는다.** 같은 `surface_id` 로 claude 와
    /// codex 를 동시에 spawn 할 수 있어, prefix 로 자기 것만 고르는 것이 이 sweep 의
    /// 안전 조건이다 — prefix 를 인자로 뽑은 이유가 여기 있다.
    #[test]
    fn sweep_does_not_touch_another_prefix() {
        let tmp = tempfile::tempdir().unwrap();
        let mine = path_for(tmp.path(), PREFIX, 7);
        let theirs = path_for(tmp.path(), "tasty-other-prompt-", 7);
        std::fs::write(&mine, "x").unwrap();
        std::fs::write(&theirs, "x").unwrap();
        age_back(&mine);
        age_back(&theirs);

        sweep_stale(tmp.path(), PREFIX);

        assert!(!mine.exists(), "자기 prefix 의 낡은 파일은 지운다");
        assert!(theirs.exists(), "다른 prefix 의 파일은 낡았어도 남긴다");
    }
}
