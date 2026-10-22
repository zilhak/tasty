//! 오버레이 창 위치 요청과 창 관리자 응답의 대조. OS 를 부르지 않는 순수 판단이다.
//!
//! 관리 창이라 위치·크기 요청은 창 관리자가 바꿀 수 있다. GNOME Shell(mutter 46)은 프로그램이 요청한
//! 위치를 작업 영역 안으로 옮겼다. 사용자가 일부를 화면 밖으로 끌어낸 주 창이나 전체화면 주 창과 같은
//! 사각형은 받아 주지 않았다(측정). 응답이 요청과 다르면 그림이 주 창과 어긋나므로 그 주 창 위치에서는
//! 합성을 낼 수 없다고 보고하고, 주 창이 다른 위치로 움직이면 다시 시도한다.

use std::collections::VecDeque;

/// 화면 좌표 `[x, y, 너비, 높이]`(물리 px).
pub(super) type Placement = [i32; 4];

/// 응답을 기다리는 요청의 상한. 넘치면 오래된 것부터 버린다.
const IN_FLIGHT_CAP: usize = 16;

#[derive(Debug)]
pub(super) struct Fit {
    /// 마지막으로 읽은 주 창 클라이언트 영역.
    parent: Placement,
    /// 마지막으로 오버레이 창에 요청한 위치(만들 때 위치 포함).
    placed: Placement,
    /// 아직 응답이 오지 않은 요청. 응답은 요청 순서대로 온다.
    in_flight: VecDeque<Placement>,
    /// 창 관리자가 받아 주지 않은 요청. 그때의 주 창 위치와 같다.
    refused: Option<Placement>,
}

impl Fit {
    /// 오버레이 창을 `parent` 위치로 만들었다.
    pub(super) fn new(parent: Placement) -> Self {
        Self {
            parent,
            placed: parent,
            in_flight: VecDeque::new(),
            refused: None,
        }
    }

    /// 주 창 위치를 새로 읽었다. 거절된 위치에서 벗어났으면 다시 시도할 수 있다.
    pub(super) fn parent_read(&mut self, parent: Placement) {
        self.parent = parent;
        if self.refused.is_some_and(|r| r != parent) {
            self.refused = None;
        }
    }

    /// 오버레이 창에 보낼 위치. 마지막 요청과 같으면 보내지 않는다.
    pub(super) fn request(&mut self) -> Option<Placement> {
        if self.parent == self.placed {
            return None;
        }
        self.placed = self.parent;
        if self.in_flight.len() == IN_FLIGHT_CAP {
            self.in_flight.pop_front();
        }
        self.in_flight.push_back(self.parent);
        Some(self.parent)
    }

    /// 오버레이 창이 `actual` 로 configure 됐다. 새로 거절됐으면 참이다.
    pub(super) fn configured(&mut self, actual: Placement) -> bool {
        if let Some(i) = self.in_flight.iter().position(|r| *r == actual) {
            self.in_flight.drain(..=i);
            return false;
        }
        self.in_flight.clear();
        if actual == self.placed {
            return false;
        }
        self.refused.replace(self.placed) != Some(self.placed)
    }

    /// 지금 주 창 위치가 거절된 위치다.
    pub(super) fn refused(&self) -> bool {
        self.refused == Some(self.parent)
    }

    /// 거절 기록이 있다. 주 창이 움직였는지 확인해야 한다.
    pub(super) fn awaits_parent_move(&self) -> bool {
        self.refused.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const A: Placement = [100, 100, 800, 600];
    const B: Placement = [200, 100, 800, 600];
    const C: Placement = [300, 100, 800, 600];
    const OFF: Placement = [-500, 100, 800, 600];
    const CLAMPED: Placement = [0, 100, 800, 600];

    /// 요청한 위치가 그대로 오면 거절이 아니다. 앞 요청의 늦은 응답도 거절이 아니다.
    #[test]
    fn answers_to_pending_requests_are_not_refusals() {
        let mut fit = Fit::new(A);
        assert!(!fit.configured(A));
        fit.parent_read(B);
        assert_eq!(fit.request(), Some(B));
        fit.parent_read(C);
        assert_eq!(fit.request(), Some(C));
        assert_eq!(fit.request(), None);
        assert!(!fit.configured(B));
        assert!(!fit.configured(C));
        assert!(!fit.refused());
    }

    /// 창 관리자가 위치를 바꾸면 그 주 창 위치에서는 거절이다. 같은 거절은 한 번만 알린다. 주 창이 다른 곳으로
    /// 가면 다시 시도한다.
    #[test]
    fn a_moved_answer_refuses_that_parent_placement_until_the_parent_moves() {
        let mut fit = Fit::new(A);
        fit.parent_read(OFF);
        assert_eq!(fit.request(), Some(OFF));
        assert!(fit.configured(CLAMPED));
        assert!(fit.refused());
        assert!(!fit.configured(CLAMPED));
        fit.parent_read(OFF);
        assert!(fit.refused());
        assert!(fit.awaits_parent_move());
        fit.parent_read(B);
        assert!(!fit.refused());
        assert!(!fit.awaits_parent_move());
        assert_eq!(fit.request(), Some(B));
        assert!(!fit.configured(B));
    }

    /// 만들 때 위치를 보이는 순간 창 관리자가 바꿔도 거절이다.
    #[test]
    fn the_first_map_can_be_refused() {
        let mut fit = Fit::new(OFF);
        assert!(fit.configured(CLAMPED));
        assert!(fit.refused());
    }

    /// 응답이 오지 않는 요청이 쌓여도 상한을 넘지 않는다.
    #[test]
    fn unanswered_requests_stay_bounded() {
        let mut fit = Fit::new(A);
        for x in 0..100 {
            fit.parent_read([x, 0, 10, 10]);
            fit.request();
        }
        assert_eq!(fit.in_flight.len(), IN_FLIGHT_CAP);
        assert!(!fit.configured([99, 0, 10, 10]));
        assert!(fit.in_flight.is_empty());
    }
}
