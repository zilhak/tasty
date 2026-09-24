//! 가져오기 경고를 스키마, 알 수 없는 액션, 빈 그룹 순으로 정리한다.
//! 세 줄까지 표시하고 나머지는 접는다. 생략된 plugin은 별도 정보 줄로 표시한다.

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

/// 화면 문구가 있는 경고를 정해진 순서로 반환한다. 나머지는 호출자가 로그에 남긴다.
/// 빈 그룹 문구는 있으나 현재 코덱이 해당 경고를 만들지는 않는다.
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

/// 화면에 표시할 경고인지 확인한다. 나머지는 로그에만 남는다.
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

    /// 알 수 없는 액션은 개수와 관계없이 한 줄로 묶는다.
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

    /// 한 줄만 있으면 숨겨진 줄 수는 0이다.
    #[test]
    fn a_single_notice_has_nothing_to_fold() {
        assert_eq!(fold(1, false), (1, 0));
        assert_eq!(fold(1, true), (1, 0));
    }
}
