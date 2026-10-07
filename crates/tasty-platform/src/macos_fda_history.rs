//! Full Disk Access 보유 기록. 부팅 안내의 FDA 문단 갈래([`FdaNoticeBranch`])를 고르는 데만 쓴다.
//!
//! 보유를 마지막으로 관측했을 때의 자기 서명 해시(cdhash)를 데이터 홈에 남기고, 거부로 보일 때
//! 지금 해시와 비교한다. 안내를 띄울지는 이 기록과 관계없이 매 부팅 현재 상태로 정한다.
//! 기록을 두는 이유와 대안은
//! [ADR-0074](../../../docs/adr/0074-macos-fda-grant-record-picks-the-notice-cause.md)에 있다.
//!
//! 갈래 판정과 기록 형식은 순수 함수라 macOS가 아닌 환경에서도 시험한다. 파일 읽기·쓰기와
//! 해시 취득은 `#[cfg(all(target_os = "macos", feature = "gui"))]` 안에만 있다.

use crate::macos_permission_notice::FdaNoticeBranch;
use crate::macos_permissions::FullDiskAccess;

/// 데이터 홈 아래 기록 파일 이름. 사용자 설정(`config.toml`)과 섞지 않는다.
#[cfg(all(target_os = "macos", feature = "gui"))]
const RECORD_FILE: &str = "macos-fda-grant";

/// 기록 파일의 해시 줄 접두어.
#[cfg(any(test, all(target_os = "macos", feature = "gui")))]
const CDHASH_PREFIX: &str = "cdhash=";

/// 보유를 관측한 기록. `cdhash`는 그때의 자기 서명 해시(16진 소문자)이고 얻지 못했으면 `None`이다.
#[cfg(any(test, all(target_os = "macos", feature = "gui")))]
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FdaGrantRecord {
    pub(crate) cdhash: Option<String>,
}

/// 이번 관측에서 쓸 FDA 문단 갈래. 거부로 보일 때만 기록을 본다.
///
/// 보유 기록이 있는데 해시를 비교할 수 없으면(어느 쪽이든 얻지 못함) `Stale`로 본다.
/// `Stale`의 처방(목록에서 지우고 다시 추가)은 밖에서 꺼진 경우도 고치지만, `Revoked`의
/// 처방(다시 켜기)은 앱이 바뀐 경우를 고치지 못한다.
#[cfg(any(test, all(target_os = "macos", feature = "gui")))]
pub(crate) fn branch_for(
    record: Option<&FdaGrantRecord>,
    access: FullDiskAccess,
    current: Option<&str>,
) -> FdaNoticeBranch {
    if access != FullDiskAccess::Denied {
        return FdaNoticeBranch::Never;
    }
    match record {
        None => FdaNoticeBranch::Never,
        Some(record) => match (record.cdhash.as_deref(), current) {
            (Some(saved), Some(now)) if saved == now => FdaNoticeBranch::Revoked,
            _ => FdaNoticeBranch::Stale,
        },
    }
}

/// 이번 관측 뒤 저장할 기록. 보유를 관측했고 기록이 달라질 때만 `Some`이다.
///
/// 거부와 판단 불가는 기록을 바꾸지 않는다. 거부 관측으로 보유 기록을 지우면 거부가 이어지는
/// 다음 부팅에서 원인을 가를 수 없다.
#[cfg(any(test, all(target_os = "macos", feature = "gui")))]
pub(crate) fn record_after(
    record: Option<&FdaGrantRecord>,
    access: FullDiskAccess,
    current: Option<&str>,
) -> Option<FdaGrantRecord> {
    if access != FullDiskAccess::Granted {
        return None;
    }
    let next = FdaGrantRecord {
        cdhash: current.map(str::to_owned),
    };
    (record != Some(&next)).then_some(next)
}

/// 기록 파일 내용을 읽는다. 형식이 맞지 않으면 기록이 없는 것으로 본다.
#[cfg(any(test, all(target_os = "macos", feature = "gui")))]
pub(crate) fn parse_record(text: &str) -> Option<FdaGrantRecord> {
    let value = text
        .lines()
        .find_map(|line| line.strip_prefix(CDHASH_PREFIX))?
        .trim();
    if value.is_empty() {
        return Some(FdaGrantRecord { cdhash: None });
    }
    value
        .bytes()
        .all(|b| b.is_ascii_hexdigit())
        .then(|| FdaGrantRecord {
            cdhash: Some(value.to_ascii_lowercase()),
        })
}

/// [`parse_record`]가 읽는 형식으로 기록을 쓴다.
#[cfg(any(test, all(target_os = "macos", feature = "gui")))]
pub(crate) fn format_record(record: &FdaGrantRecord) -> String {
    format!(
        "{CDHASH_PREFIX}{}\n",
        record.cdhash.as_deref().unwrap_or_default()
    )
}

/// 이번 측정의 FDA 관측으로 안내 갈래를 정하고, 보유를 관측했으면 기록을 갱신한다.
#[cfg(all(target_os = "macos", feature = "gui"))]
pub(crate) fn observe(access: FullDiskAccess) -> FdaNoticeBranch {
    let current = self_code::cdhash();
    let Some(path) = tasty_utils::path::tasty_home().map(|home| home.join(RECORD_FILE)) else {
        return branch_for(None, access, current);
    };
    let record = load_record(&path);
    let branch = branch_for(record.as_ref(), access, current);
    if let Some(next) = record_after(record.as_ref(), access, current) {
        store_record(&path, &next);
    }
    branch
}

/// macOS GUI가 아니면 FDA를 추정하지 않으므로 보유 이력도 없다.
#[cfg(not(all(target_os = "macos", feature = "gui")))]
pub(crate) fn observe(_access: FullDiskAccess) -> FdaNoticeBranch {
    FdaNoticeBranch::Never
}

#[cfg(all(target_os = "macos", feature = "gui"))]
fn load_record(path: &std::path::Path) -> Option<FdaGrantRecord> {
    match std::fs::read_to_string(path) {
        Ok(text) => {
            let record = parse_record(&text);
            if record.is_none() {
                tracing::warn!(path = %path.display(), "FDA 보유 기록 형식이 맞지 않아 기록이 없는 것으로 본다");
            }
            record
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => None,
        Err(err) => {
            tracing::warn!(%err, path = %path.display(), "FDA 보유 기록 읽기 실패");
            None
        }
    }
}

/// 같은 디렉터리의 임시 파일에 쓴 뒤 이름을 바꾼다. 쓰다 끊겨도 이전 기록이 남는다.
#[cfg(all(target_os = "macos", feature = "gui"))]
fn store_record(path: &std::path::Path, record: &FdaGrantRecord) {
    let temporary = path.with_extension("tmp");
    let written = path
        .parent()
        .map_or(Ok(()), std::fs::create_dir_all)
        .and_then(|()| std::fs::write(&temporary, format_record(record)))
        .and_then(|()| std::fs::rename(&temporary, path));
    if let Err(err) = written {
        // 기록을 못 남겨도 이번 안내는 그대로 뜬다. 다음 거부 관측의 갈래만 `Never`로 떨어진다.
        tracing::warn!(%err, path = %path.display(), "FDA 보유 기록 저장 실패");
    }
}

/// 실행 중인 자기 코드의 서명 해시. TCC가 ad-hoc 서명 앱의 권한 행을 묶는 값(`cdhash`)이다.
/// `codesign`을 띄우지 않고 Security.framework에 직접 묻는다. 프로세스 동안 바뀌지 않아 한 번만 구한다.
#[cfg(all(target_os = "macos", feature = "gui"))]
mod self_code {
    use std::ffi::c_void;

    type CFTypeRef = *const c_void;
    type OSStatus = i32;

    const ERR_SEC_SUCCESS: OSStatus = 0;
    const SEC_CS_DEFAULT_FLAGS: u32 = 0;

    #[link(name = "Security", kind = "framework")]
    unsafe extern "C" {
        fn SecCodeCopySelf(flags: u32, code: *mut CFTypeRef) -> OSStatus;
        fn SecCodeCopyStaticCode(
            code: CFTypeRef,
            flags: u32,
            static_code: *mut CFTypeRef,
        ) -> OSStatus;
        fn SecCodeCopySigningInformation(
            code: CFTypeRef,
            flags: u32,
            information: *mut CFTypeRef,
        ) -> OSStatus;
        /// 서명 정보 딕셔너리에서 cdhash(`CFData`)를 가리키는 키.
        static kSecCodeInfoUnique: CFTypeRef;
    }

    #[link(name = "CoreFoundation", kind = "framework")]
    unsafe extern "C" {
        fn CFDictionaryGetValue(dict: CFTypeRef, key: CFTypeRef) -> CFTypeRef;
        fn CFDataGetLength(data: CFTypeRef) -> isize;
        fn CFDataGetBytePtr(data: CFTypeRef) -> *const u8;
        fn CFRelease(cf: CFTypeRef);
    }

    static CDHASH: std::sync::OnceLock<Option<String>> = std::sync::OnceLock::new();

    pub(super) fn cdhash() -> Option<&'static str> {
        CDHASH.get_or_init(read).as_deref()
    }

    fn read() -> Option<String> {
        let hash = copy_cdhash();
        if hash.is_none() {
            tracing::warn!("자기 서명 해시를 얻지 못했다 — FDA 안내 갈래는 해시 비교 없이 고른다");
        }
        hash
    }

    fn copy_cdhash() -> Option<String> {
        let mut code: CFTypeRef = std::ptr::null();
        // SAFETY: 출력 포인터는 이 스택 변수를 가리킨다. 성공하면 +1 retain 된 SecCode가 들어오고
        // 아래에서 한 번 해제한다.
        if unsafe { SecCodeCopySelf(SEC_CS_DEFAULT_FLAGS, &mut code) } != ERR_SEC_SUCCESS
            || code.is_null()
        {
            return None;
        }
        let mut static_code: CFTypeRef = std::ptr::null();
        // SAFETY: `code`는 위에서 받은 유효한 SecCode다. 출력은 +1 retain 된 SecStaticCode다.
        let status = unsafe { SecCodeCopyStaticCode(code, SEC_CS_DEFAULT_FLAGS, &mut static_code) };
        // SAFETY: `code`는 Copy 규칙으로 받은 객체라 여기서 한 번 해제한다. 이후 쓰지 않는다.
        unsafe { CFRelease(code) };
        if status != ERR_SEC_SUCCESS || static_code.is_null() {
            return None;
        }
        let mut info: CFTypeRef = std::ptr::null();
        // SAFETY: `static_code`는 유효한 SecStaticCode다. 출력은 +1 retain 된 CFDictionary다.
        let status =
            unsafe { SecCodeCopySigningInformation(static_code, SEC_CS_DEFAULT_FLAGS, &mut info) };
        // SAFETY: Copy 규칙으로 받은 객체를 한 번 해제한다. 이후 쓰지 않는다.
        unsafe { CFRelease(static_code) };
        if status != ERR_SEC_SUCCESS || info.is_null() {
            return None;
        }
        let hex = dictionary_cdhash_hex(info);
        // SAFETY: Copy 규칙으로 받은 딕셔너리를 한 번 해제한다. 값은 위에서 이미 복사했다.
        unsafe { CFRelease(info) };
        hex
    }

    /// 서명 정보 딕셔너리에서 cdhash 바이트를 16진 문자열로 복사한다. 딕셔너리는 해제하지 않는다.
    fn dictionary_cdhash_hex(info: CFTypeRef) -> Option<String> {
        // SAFETY: 프레임워크가 내보내는 상수 키를 읽기만 한다.
        let key = unsafe { kSecCodeInfoUnique };
        // SAFETY: `info`는 호출자가 쥔 유효한 CFDictionary다. Get 규칙이라 값은 딕셔너리가 소유한다.
        let data = unsafe { CFDictionaryGetValue(info, key) };
        if data.is_null() {
            return None;
        }
        // SAFETY: `data`는 `kSecCodeInfoUnique` 값인 CFData이고 딕셔너리가 살아 있는 동안 유효하다.
        let len = usize::try_from(unsafe { CFDataGetLength(data) }).ok()?;
        // SAFETY: 같은 CFData의 바이트 포인터. 길이는 바로 위에서 읽었다.
        let bytes = unsafe { CFDataGetBytePtr(data) };
        if bytes.is_null() || len == 0 {
            return None;
        }
        // SAFETY: `bytes`는 `len` 바이트를 가리키고 이 함수가 끝날 때까지 딕셔너리가 붙잡고 있다.
        let slice = unsafe { std::slice::from_raw_parts(bytes, len) };
        Some(slice.iter().map(|b| format!("{b:02x}")).collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn granted(hash: &str) -> FdaGrantRecord {
        FdaGrantRecord {
            cdhash: Some(hash.to_owned()),
        }
    }

    /// 거부로 보일 때 네 갈래: 이력 없음 · 해시 바뀜 · 해시 같음 · 비교 불가.
    #[test]
    fn a_denied_observation_picks_the_branch_from_the_grant_record() {
        let denied = FullDiskAccess::Denied;
        assert_eq!(branch_for(None, denied, Some("aa")), FdaNoticeBranch::Never);
        assert_eq!(
            branch_for(Some(&granted("aa")), denied, Some("bb")),
            FdaNoticeBranch::Stale
        );
        assert_eq!(
            branch_for(Some(&granted("aa")), denied, Some("aa")),
            FdaNoticeBranch::Revoked
        );
        assert_eq!(
            branch_for(Some(&granted("aa")), denied, None),
            FdaNoticeBranch::Stale
        );
        assert_eq!(
            branch_for(Some(&FdaGrantRecord { cdhash: None }), denied, Some("aa")),
            FdaNoticeBranch::Stale
        );
    }

    /// 보유·판단 불가로 보이면 FDA 문단은 기본 갈래다. 기록은 갈래를 바꾸지 않는다.
    #[test]
    fn only_a_denied_observation_reads_the_record() {
        for access in [FullDiskAccess::Granted, FullDiskAccess::Unknown] {
            assert_eq!(
                branch_for(Some(&granted("aa")), access, Some("bb")),
                FdaNoticeBranch::Never
            );
        }
    }

    /// 보유를 관측하면 지금 해시로 기록하고, 같은 기록이면 다시 쓰지 않는다.
    #[test]
    fn a_granted_observation_records_the_current_hash_once() {
        let granted_now = FullDiskAccess::Granted;
        assert_eq!(
            record_after(None, granted_now, Some("aa")),
            Some(granted("aa"))
        );
        assert_eq!(
            record_after(Some(&granted("aa")), granted_now, Some("bb")),
            Some(granted("bb"))
        );
        assert_eq!(
            record_after(Some(&granted("aa")), granted_now, Some("aa")),
            None
        );
        assert_eq!(
            record_after(None, granted_now, None),
            Some(FdaGrantRecord { cdhash: None })
        );
    }

    /// 거부·판단 불가는 보유 기록을 지우지 않는다. 지우면 다음 부팅에서 원인을 가를 수 없다.
    #[test]
    fn denied_and_unknown_keep_the_grant_record() {
        for access in [FullDiskAccess::Denied, FullDiskAccess::Unknown] {
            assert_eq!(record_after(Some(&granted("aa")), access, Some("bb")), None);
            assert_eq!(record_after(None, access, Some("bb")), None);
        }
    }

    #[test]
    fn the_record_file_round_trips_and_rejects_other_content() {
        for record in [granted("0a1b2c"), FdaGrantRecord { cdhash: None }] {
            assert_eq!(parse_record(&format_record(&record)), Some(record));
        }
        assert_eq!(parse_record("cdhash=0A1B\n"), Some(granted("0a1b")));
        assert_eq!(parse_record("cdhash=not-hex\n"), None);
        assert_eq!(parse_record(""), None);
    }

    /// 업데이트 시나리오: 보유 관측 → 앱이 바뀐 뒤 거부 → 거부가 이어져도 같은 갈래.
    #[test]
    fn an_update_keeps_reporting_stale_while_access_stays_denied() {
        let mut record = record_after(None, FullDiskAccess::Granted, Some("old"));
        for _ in 0..2 {
            assert_eq!(
                branch_for(record.as_ref(), FullDiskAccess::Denied, Some("new")),
                FdaNoticeBranch::Stale
            );
            if let Some(next) = record_after(record.as_ref(), FullDiskAccess::Denied, Some("new")) {
                record = Some(next);
            }
        }
        record = record_after(record.as_ref(), FullDiskAccess::Granted, Some("new")).or(record);
        assert_eq!(
            branch_for(record.as_ref(), FullDiskAccess::Denied, Some("new")),
            FdaNoticeBranch::Revoked
        );
    }
}
