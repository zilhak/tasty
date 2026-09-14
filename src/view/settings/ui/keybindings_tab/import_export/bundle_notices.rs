//! 번들 경고 — 가져온 파일의 경고를 화면이 한 블록에 쌓을 줄로 고른다.
//!
//! 경계: 경고 목록을 **읽기만** 하는 분류다. 문구·그리기는 `view_model` 과 `notices` 가 한다.
//!
//! 화면이 경고를 쌓는 규칙은 디자인이 정했다 — 경고는 톤이 같은 **한 블록**에 한 줄씩,
//! 순서는 원문 순서가 아니라 **고정**(스키마 → 모르는 액션 → 빈 그룹), 세 줄까지 보이고
//! 나머지는 접는다. 설치되지 않은 plugin 을 버린 것은 경고가 아니라 정보라 이 블록에 안
//! 들어간다(`dropped` 정보 줄이 따로 든다).

use tasty_host_plugin::keybinding_bundle::BundleWarning;

/// 접기 전에 보이는 줄 수.
pub(super) const NOTICE_FOLD_AT: usize = 3;

/// 경고 블록의 한 줄.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum BundleNotice {
    /// 이 빌드가 아는 것보다 새 스키마로 쓰였다.
    NewerSchema { found: u64, known: u32 },
    /// 이 빌드가 모르는 액션을 건너뛰었다 — 몇 개든 한 줄이다.
    UnknownActions(Vec<String>),
}

impl BundleNotice {
    /// 고정 순서의 자리 — 작을수록 위.
    fn rank(&self) -> u8 {
        match self {
            BundleNotice::NewerSchema { .. } => 0,
            BundleNotice::UnknownActions(_) => 1,
        }
    }
}

/// 경고 블록에 오를 줄 — 고정 순서.
///
/// 디자인 문구가 있는 경고만 줄이 된다. 나머지(모르는 최상위 키 · 필드 모양 불일치 · 절
/// 통째 읽기 실패 · plugin override 읽기 실패 · 없는 script 바인딩)는 화면 문구가 정해지지
/// 않아 여기 안 오르고, 호출부가 로그로만 남긴다 — [`is_shown`] 이 그 경계다. "빈 그룹"
/// 줄은 디자인 문구가 있지만 그것을 내는 경고가 코덱에 없다.
pub(super) fn bundle_notices(warnings: &[BundleWarning]) -> Vec<BundleNotice> {
    let mut out = Vec::new();
    let mut unknown = Vec::new();
    for w in warnings {
        match w {
            BundleWarning::NewerVersion { found, known } => out.push(BundleNotice::NewerSchema {
                found: *found,
                known: *known,
            }),
            BundleWarning::UnknownKeybindingField { field } => unknown.push(field.clone()),
            _ => {}
        }
    }
    if !unknown.is_empty() {
        out.push(BundleNotice::UnknownActions(unknown));
    }
    out.sort_by_key(BundleNotice::rank);
    out
}

/// 그 경고가 화면(정보 줄이나 경고 블록)에 오르는가 — 안 오르면 로그가 유일한 흔적이다.
pub(super) fn is_shown(w: &BundleWarning) -> bool {
    matches!(
        w,
        BundleWarning::NewerVersion { .. }
            | BundleWarning::UnknownKeybindingField { .. }
            | BundleWarning::DroppedUninstalledPlugin { .. }
    )
}

/// 접힘 상태에서 (보이는 줄 수, 접힌 줄 수).
pub(super) fn fold(len: usize, expanded: bool) -> (usize, usize) {
    if expanded || len <= NOTICE_FOLD_AT {
        (len, 0)
    } else {
        (NOTICE_FOLD_AT, len - NOTICE_FOLD_AT)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unknown(field: &str) -> BundleWarning {
        BundleWarning::UnknownKeybindingField {
            field: field.to_string(),
        }
    }

    /// 원문에서 모르는 액션이 스키마 경고보다 먼저 와도 블록은 스키마부터다 — 정렬 키는
    /// 원문 순서가 아니다.
    #[test]
    fn notices_follow_the_fixed_order_not_the_source_order() {
        let warnings = [
            unknown("pane.zoom_cycle"),
            BundleWarning::NewerVersion { found: 3, known: 2 },
            unknown("tab.pin"),
        ];
        assert_eq!(
            bundle_notices(&warnings),
            vec![
                BundleNotice::NewerSchema { found: 3, known: 2 },
                BundleNotice::UnknownActions(vec!["pane.zoom_cycle".into(), "tab.pin".into()]),
            ]
        );
    }

    /// 모르는 액션은 몇 개든 한 줄이다 — 경고마다 줄을 세우면 블록이 목록이 된다.
    #[test]
    fn unknown_actions_collapse_into_one_line() {
        let warnings = [unknown("a"), unknown("b"), unknown("c")];
        assert_eq!(bundle_notices(&warnings).len(), 1);
    }

    /// 설치되지 않은 plugin 은 정보 줄의 몫이라 경고 블록에 안 들어간다.
    #[test]
    fn a_dropped_plugin_is_information_not_a_notice() {
        let warnings = [BundleWarning::DroppedUninstalledPlugin {
            plugin_id: "k8s-lens".into(),
            commands: 2,
        }];
        assert!(bundle_notices(&warnings).is_empty());
        assert!(is_shown(&warnings[0]));
    }

    #[test]
    fn three_lines_show_and_the_rest_fold() {
        assert_eq!(fold(2, false), (2, 0));
        assert_eq!(fold(3, false), (3, 0));
        assert_eq!(fold(5, false), (3, 2));
        assert_eq!(fold(5, true), (5, 0));
    }
}
