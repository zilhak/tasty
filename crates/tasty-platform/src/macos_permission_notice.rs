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

/// 안내 본문 문단 하나. 값은 번역 키다.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoticeParagraph {
    /// 일반 문단.
    Text(&'static str),
    /// 번호 목록 항목. 번호는 이어진 항목끼리 1부터 매긴다.
    Step(&'static str),
    /// 본문 맨 끝의 보조 문단(작은 글씨·옅은 색).
    Aside(&'static str),
}

impl NoticeParagraph {
    pub fn key(self) -> &'static str {
        match self {
            Self::Text(k) | Self::Step(k) | Self::Aside(k) => k,
        }
    }
}

/// 안내 본문을 이루는 문단을 순서대로 돌려준다. 문단은 빈 줄 하나로 잇는다.
///
/// 서명 문단(`self_built`)은 모든 안내의 맨 끝에 보조 문단으로 둔다. 배포본도 직접 빌드도
/// ad-hoc 서명이라 앱이 둘을 가를 수 없고, 둘 다 업데이트·재빌드마다 권한을 잃는다.
pub fn permission_notice_paragraphs(fda: FdaNoticeBranch) -> Vec<NoticeParagraph> {
    use NoticeParagraph::{Aside, Step, Text};
    let mut out = vec![Text("macos_permissions.notice.intro")];
    match fda {
        FdaNoticeBranch::Never => out.push(Text("macos_permissions.notice.fda_never")),
        FdaNoticeBranch::Stale => out.extend([
            Text("macos_permissions.notice.fda_stale"),
            Step("macos_permissions.notice.fda_stale_step_remove"),
            Step("macos_permissions.notice.fda_stale_step_add"),
            Text("macos_permissions.notice.fda_stale_retry"),
            Text("macos_permissions.notice.fda_covers"),
        ]),
        FdaNoticeBranch::Revoked => out.extend([
            Text("macos_permissions.notice.fda_revoked"),
            Text("macos_permissions.notice.fda_covers"),
        ]),
    }
    out.extend([
        Text("macos_permissions.notice.screen_recording"),
        Text("macos_permissions.notice.closing"),
        Aside("macos_permissions.notice.self_built"),
    ]);
    out
}

/// [`permission_notice_paragraphs`]의 문단을 `t`로 번역해 이은 안내 본문. 번호 항목은
/// `1. `, 보조 문단은 `> `를 앞에 붙인다 — 안내 모달의 강조 표기다(`tasty_ui_widgets::info_modal`).
pub fn permission_notice_body(
    fda: FdaNoticeBranch,
    t: impl Fn(&'static str) -> &'static str,
) -> String {
    let mut step = 0;
    permission_notice_paragraphs(fda)
        .into_iter()
        .map(|p| match p {
            NoticeParagraph::Text(k) => {
                step = 0;
                t(k).to_string()
            }
            NoticeParagraph::Step(k) => {
                step += 1;
                format!("{step}. {}", t(k))
            }
            NoticeParagraph::Aside(k) => {
                step = 0;
                format!("> {}", t(k))
            }
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

/// 설정 탭 Full Disk Access 행의 보조 줄 번역 키. 허용 안 됨인데 보유 이력이 있으면 갈래별
/// 처방을 적는다(앱이 바뀌었으면 제거 후 재추가, 밖에서 꺼졌으면 다시 켜기). 상태는 그대로
/// "허용 안 됨"이고 별도 상태나 칩을 만들지 않는다.
pub fn fda_settings_detail_key(missing: bool, fda: FdaNoticeBranch) -> Option<&'static str> {
    match (missing, fda) {
        (true, FdaNoticeBranch::Stale) => {
            Some("settings.macos_permissions.full_disk_access_stale_detail")
        }
        (true, FdaNoticeBranch::Revoked) => {
            Some("settings.macos_permissions.full_disk_access_revoked_detail")
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn keys(fda: FdaNoticeBranch) -> Vec<String> {
        permission_notice_paragraphs(fda)
            .into_iter()
            .map(|p| {
                let k = p.key().trim_start_matches("macos_permissions.notice.");
                match p {
                    NoticeParagraph::Text(_) => k.to_string(),
                    NoticeParagraph::Step(_) => format!("step:{k}"),
                    NoticeParagraph::Aside(_) => format!("aside:{k}"),
                }
            })
            .collect()
    }

    /// 갈래는 FDA 문단만 바꾸고 나머지 문단의 순서는 그대로다. 갈래 (b)·(c)는 기본 문단
    /// 대신 원인 문단과 "무엇을 덮는지" 문단을 넣고, (b)는 처방을 두 단계 목록으로 적는다.
    /// 서명 문단은 어느 갈래에서나 맨 끝의 보조 문단이다.
    #[test]
    fn notice_branches_change_only_the_fda_paragraph() {
        use FdaNoticeBranch::*;
        assert_eq!(
            keys(Never),
            [
                "intro",
                "fda_never",
                "screen_recording",
                "closing",
                "aside:self_built"
            ]
        );
        assert_eq!(
            keys(Stale),
            [
                "intro",
                "fda_stale",
                "step:fda_stale_step_remove",
                "step:fda_stale_step_add",
                "fda_stale_retry",
                "fda_covers",
                "screen_recording",
                "closing",
                "aside:self_built"
            ]
        );
        assert_eq!(
            keys(Revoked),
            [
                "intro",
                "fda_revoked",
                "fda_covers",
                "screen_recording",
                "closing",
                "aside:self_built"
            ]
        );
    }

    #[test]
    fn notice_body_numbers_steps_and_marks_the_aside() {
        let body = permission_notice_body(FdaNoticeBranch::Stale, |key| key);
        let paras: Vec<&str> = body.split("\n\n").collect();
        assert_eq!(
            paras[2],
            "1. macos_permissions.notice.fda_stale_step_remove"
        );
        assert_eq!(paras[3], "2. macos_permissions.notice.fda_stale_step_add");
        assert_eq!(paras[4], "macos_permissions.notice.fda_stale_retry");
        assert_eq!(paras.last(), Some(&"> macos_permissions.notice.self_built"));
        assert_eq!(paras.len(), 9);
    }

    #[test]
    fn only_a_lost_grant_adds_a_remedy_line_to_a_missing_row() {
        use FdaNoticeBranch::*;
        assert_eq!(
            fda_settings_detail_key(true, Stale),
            Some("settings.macos_permissions.full_disk_access_stale_detail")
        );
        assert_eq!(
            fda_settings_detail_key(true, Revoked),
            Some("settings.macos_permissions.full_disk_access_revoked_detail")
        );
        assert_eq!(fda_settings_detail_key(true, Never), None);
        assert_eq!(fda_settings_detail_key(false, Stale), None);
        assert_eq!(fda_settings_detail_key(false, Revoked), None);
    }
}
