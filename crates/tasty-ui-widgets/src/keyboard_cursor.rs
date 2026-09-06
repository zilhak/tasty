//! 목록형 위젯의 **키보드 커서** 규약 — 한 벌만 둔다.
//!
//! 드롭다운·팝업 목록은 방향키 커서를 똑같이 움직인다: 아직 커서가 없으면 진행 방향의
//! 끝에서 시작하고, 목록 끝에서 순환하고, 짚을 수 없는(비활성) 행은 건너뛴다. 이 규약이
//! 위젯마다 따로 구현되어 있으면 한쪽만 바뀌었을 때 아무 소리가 안 난다 — 두 목록이
//! 방향키에 다르게 반응하는 것은 컴파일도 테스트도 안 막는다.
//!
//! 그래서 구현을 여기 한 벌 두고 위젯은 **부른다**. 비활성 마스크가 없는 위젯은
//! `disabled` 에 `None` 을 준다 — 그러면 "전부 활성" 이라 순환만 남는다.

/// 행 `i` 가 짚을 수 있는가 — 마스크가 없거나 짧으면 그 행은 활성이다(호출부가
/// 마스크를 안 주는 흔한 경우가 곧 "전부 활성").
pub(crate) fn row_enabled(disabled: Option<&[bool]>, i: usize) -> bool {
    !disabled.and_then(|d| d.get(i)).copied().unwrap_or(false)
}

/// 진행 방향의 첫 **짚을 수 있는** 행. 전부 비활성이거나 목록이 비면 `None`.
///
/// `Home`/`End` 의 종착이자, 아직 커서가 없을 때 방향키가 들어오는 자리다.
pub(crate) fn edge_enabled(n: usize, disabled: Option<&[bool]>, forward: bool) -> Option<usize> {
    if forward {
        (0..n).find(|i| row_enabled(disabled, *i))
    } else {
        (0..n).rev().find(|i| row_enabled(disabled, *i))
    }
}

/// 방향키 한 번의 커서 이동 — 비활성 행은 건너뛰고 목록 끝에서 순환한다.
/// 짚을 행이 없으면 `None`.
pub(crate) fn step_active(
    active: Option<usize>,
    n: usize,
    disabled: Option<&[bool]>,
    forward: bool,
) -> Option<usize> {
    if n == 0 {
        return None;
    }
    let Some(start) = active else {
        return edge_enabled(n, disabled, forward);
    };
    let start = start.min(n - 1);
    // k = n 이면 제자리로 돌아온다 — "활성 행이 자기 하나뿐" 인 경우까지 덮는다.
    (1..=n)
        .map(|k| {
            if forward {
                (start + k) % n
            } else {
                (start + n - k) % n
            }
        })
        .find(|i| row_enabled(disabled, *i))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn step_active_wraps_and_skips_disabled() {
        // 마스크 없음 — 끝에서 순환.
        assert_eq!(step_active(None, 3, None, true), Some(0));
        assert_eq!(step_active(None, 3, None, false), Some(2));
        assert_eq!(step_active(Some(0), 3, None, true), Some(1));
        assert_eq!(step_active(Some(2), 3, None, true), Some(0));
        assert_eq!(step_active(Some(0), 3, None, false), Some(2));
        // 가운데가 비활성이면 건너뛴다(양방향).
        let d = [false, true, false];
        assert_eq!(step_active(Some(0), 3, Some(&d), true), Some(2));
        assert_eq!(step_active(Some(2), 3, Some(&d), false), Some(0));
        // 아직 커서가 없을 때도 비활성 끝은 피한다.
        let edges = [true, false, true];
        assert_eq!(step_active(None, 3, Some(&edges), true), Some(1));
        assert_eq!(step_active(None, 3, Some(&edges), false), Some(1));
    }

    #[test]
    fn step_active_degenerate() {
        // 목록이 비면 짚을 곳이 없다.
        assert_eq!(step_active(None, 0, None, true), None);
        assert_eq!(step_active(Some(0), 0, None, false), None);
        // 전부 비활성도 마찬가지.
        let all = [true, true];
        assert_eq!(step_active(None, 2, Some(&all), true), None);
        assert_eq!(step_active(Some(0), 2, Some(&all), true), None);
        // 활성 행이 자기 하나뿐이면 제자리.
        let one = [false, true];
        assert_eq!(step_active(Some(0), 2, Some(&one), true), Some(0));
        assert_eq!(step_active(Some(0), 2, Some(&one), false), Some(0));
        // 목록이 줄어들어 범위를 벗어난 커서도 마지막 행 기준으로 이어진다.
        assert_eq!(step_active(Some(9), 2, None, true), Some(0));
    }

    #[test]
    fn edge_enabled_finds_the_outermost_toggleable_row() {
        let d = [true, false, false, true];
        assert_eq!(edge_enabled(4, Some(&d), true), Some(1));
        assert_eq!(edge_enabled(4, Some(&d), false), Some(2));
        assert_eq!(edge_enabled(4, None, true), Some(0));
        assert_eq!(edge_enabled(4, None, false), Some(3));
        assert_eq!(edge_enabled(0, None, true), None);
        let all = [true, true];
        assert_eq!(edge_enabled(2, Some(&all), false), None);
    }
}
