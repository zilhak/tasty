//! macOS 부팅 권한 안내의 본문 문단 조립. 문단은 번역 키로 두고 갈래에 따라 고른다.
//!
//! 권한 조회와 달리 OS 접근이 없어 `gui` feature 없이 컴파일된다. 본체와 갤러리가 같은
//! 함수로 본문을 만든다.

/// 부팅 권한 안내에서 Full Disk Access 문단의 갈래. 제목·버튼·표시 여부는 갈래와 관계없이
/// 같고 FDA 문단만 바뀐다. 안내 표시 여부는 `macos_permissions::should_show_permission_notice`가 정한다.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FdaNoticeBranch {
    /// 보유했던 기록이 없다. 직접 추가하라는 기본 문단이다.
    Never,
    /// 보유했으나 그 뒤 앱이 바뀌었다(업데이트·재빌드). 목록의 항목을 지우고 다시 추가하라고 한다.
    Stale,
    /// 보유했고 앱이 그대로인데 Tasty 밖에서 꺼졌다. 다시 켜고, 목록에서 사라졌으면(`tccutil reset`) 다시 추가하라고 한다.
    Revoked,
}

/// 안내 본문을 이루는 문단의 번역 키를 순서대로 돌려준다. 문단은 빈 줄 하나로 잇는다.
///
/// `self_built`이면 직접 빌드한 사용자에게 인증서 서명을 권하는 문단을 넣는다. 배포본
/// 사용자에게는 서명할 것이 없어 넣지 않는다.
pub fn permission_notice_paragraph_keys(
    fda: FdaNoticeBranch,
    self_built: bool,
) -> Vec<&'static str> {
    let mut keys = vec!["macos_permissions.notice.intro"];
    match fda {
        FdaNoticeBranch::Never => keys.push("macos_permissions.notice.fda_never"),
        FdaNoticeBranch::Stale => keys.extend([
            "macos_permissions.notice.fda_stale",
            "macos_permissions.notice.fda_covers",
        ]),
        FdaNoticeBranch::Revoked => keys.extend([
            "macos_permissions.notice.fda_revoked",
            "macos_permissions.notice.fda_covers",
        ]),
    }
    keys.push("macos_permissions.notice.screen_recording");
    if self_built {
        keys.push("macos_permissions.notice.self_built");
    }
    keys.push("macos_permissions.notice.closing");
    keys
}

/// [`permission_notice_paragraph_keys`]의 문단을 `t`로 번역해 이은 안내 본문.
pub fn permission_notice_body(
    fda: FdaNoticeBranch,
    self_built: bool,
    t: impl Fn(&'static str) -> &'static str,
) -> String {
    permission_notice_paragraph_keys(fda, self_built)
        .into_iter()
        .map(t)
        .collect::<Vec<_>>()
        .join("\n\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 갈래는 FDA 문단만 바꾸고 나머지 문단의 순서는 그대로다. 갈래 (b)·(c)는 기본 문단
    /// 대신 원인 문단과 "무엇을 덮는지" 문단을 넣는다.
    #[test]
    fn notice_branches_change_only_the_fda_paragraph() {
        use FdaNoticeBranch::*;
        let k = |fda, self_built| permission_notice_paragraph_keys(fda, self_built);
        let p = |s: &str| format!("macos_permissions.notice.{s}");
        assert_eq!(
            k(Never, true),
            [
                "intro",
                "fda_never",
                "screen_recording",
                "self_built",
                "closing"
            ]
            .map(p)
        );
        assert_eq!(
            k(Stale, true),
            [
                "intro",
                "fda_stale",
                "fda_covers",
                "screen_recording",
                "self_built",
                "closing"
            ]
            .map(p)
        );
        assert_eq!(
            k(Revoked, false),
            [
                "intro",
                "fda_revoked",
                "fda_covers",
                "screen_recording",
                "closing"
            ]
            .map(p)
        );
        assert!(!k(Never, false).contains(&"macos_permissions.notice.self_built"));
    }

    #[test]
    fn notice_body_joins_paragraphs_with_one_blank_line() {
        let body = permission_notice_body(FdaNoticeBranch::Never, false, |key| key);
        assert_eq!(
            body,
            "macos_permissions.notice.intro\n\nmacos_permissions.notice.fda_never\n\n\
             macos_permissions.notice.screen_recording\n\nmacos_permissions.notice.closing"
        );
    }
}
