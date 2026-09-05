//! attach/detach 작업 J — 서버측 readonly 뷰 (display-only mirror).
//!
//! decisions.md 정정(2026-06-07): client 가 점유한 surface/workspace 를 서버측은
//! **내용 숨김(placeholder)이 아니라 readonly + polling 뷰잉**으로 본다. 서버 본인은
//! 조작만 불가(입력 차단 유지, `apply_send_to_surface`)하고, 화면/대화 기록은
//! readonly 로 계속 본다.
//!
//! 서버는 PTY/grid 권위 owner 라 데이터가 이미 있다. live grid 를 매 프레임 그대로
//! 그리면 "실시간"이 되어 사용자 확정 UX("3초 polling")와 어긋난다. → 점유 surface
//! 마다 **display-only mirror**(detached `Terminal`)를 두고, 3초 `Tick::AttachView` tick 때만
//! live grid 스냅샷을 feed 한다. render_pass 가 is_hard_occupied surface 를 이 mirror 로
//! 렌더한다(plan §2.3). live Terminal 은 PTY 소유 + 입력 라우팅 전용으로 유지.
//!
//! headless 빌드는 렌더가 없어 호출자가 없다(gui 한정 — render_pass/attach_poll).
//! 같은 정책의 `attach.rs` 와 동형으로 *headless 한정* dead_code 를 침묵한다.
// 이유: display-only mirror 를 만드는 것이 렌더 경로뿐이라 렌더가 없는 headless 엔 호출자가 없다(위).
#![cfg_attr(not(feature = "gui"), allow(dead_code))]

use tasty_terminal::Terminal;

use crate::core::CoreState;

impl CoreState {
    /// 서버측 readonly display mirror 를 live grid 스냅샷으로 갱신한다(3초 cadence).
    /// 점유된(`is_hard_occupied`) 각 surface 에 대해 mirror 가 없으면 만들고, live grid 의
    /// 현재 화면을 snapshot→feed 한다. snapshot 은 `\x1b[2J\x1b[H`(clear+home) 로
    /// 시작하므로 같은 mirror 에 반복 feed 해도 누적 없이 덮어쓴다. 더 이상 점유되지
    /// 않는 surface 의 mirror 는 제거한다.
    ///
    /// 반환: 갱신할 readonly mirror 가 하나라도 있으면 true(호출자가 dirty 표시용).
    pub(crate) fn refresh_readonly_views(&mut self) -> bool {
        // 현재 점유 중인 surface 집합(터미널 lock 보유분).
        let attached: Vec<u32> = self
            .attach
            .locks_snapshot()
            .into_iter()
            .map(|(sid, _)| sid)
            .collect();

        // 점유 해제된 surface 의 stale mirror 제거.
        self.readonly_views.retain(|sid, _| attached.contains(sid));

        let mut any = false;
        for sid in attached {
            let Some(live) = self.terminals.get(sid) else {
                // live 터미널이 아직 없으면(deferred 미spawn 등) skip.
                continue;
            };
            let cols = live.cols();
            let rows = live.rows();
            let snapshot = live.snapshot_as_vt();
            let mirror = self
                .readonly_views
                .entry(sid)
                .or_insert_with(|| Terminal::new_detached(cols, rows));
            // 크기 변동 대비(점유 중 resize) — 다르면 재생성.
            if mirror.cols() != cols || mirror.rows() != rows {
                *mirror = Terminal::new_detached(cols, rows);
            }
            mirror.feed_bytes(&snapshot);
            any = true;
        }
        any
    }

    /// render_pass 가 is_hard_occupied surface 를 렌더할 때 사용하는 display mirror.
    /// 아직 첫 `refresh_readonly_views` 전이면 None → render_pass 는 그 surface 를
    /// 건너뛴다(잠깐 빈 화면; 다음 tick 에 채워짐).
    pub(crate) fn readonly_view(&self, surface_id: u32) -> Option<&Terminal> {
        self.readonly_views.get(&surface_id)
    }
}
