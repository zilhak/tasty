//! 연결마다 원격 홈을 한 번 묻는다. 탐색기 주소창의 `~` 가 이 값을 쓴다.
//! 새 질의를 만들지 않고 빈 경로의 목록 요청을 쓴다. 서버는 빈 경로를 자기 홈으로 읽고 그 경로를 돌려준다.

use crate::app::App;

impl App {
    /// mirror workspace를 가진 window에 조회를 기록하고 요청을 보낸다. 보내지 못하면 홈을 모르는 채로 둔다.
    pub(super) fn probe_remote_home(&mut self, local_ws_id: u32) {
        let request_id = crate::core::next_list_dir_request_id();
        let mut recorded = false;
        for (_, main, engine) in self.engines_mut().window_pairs() {
            if engine
                .workspaces()
                .into_iter()
                .any(|ws| ws.id == local_ws_id)
            {
                main.state
                    .explorer_views
                    .begin_home_probe(local_ws_id, request_id);
                recorded = true;
                break;
            }
        }
        if !recorded {
            tracing::warn!("원격 홈 조회: mirror workspace {local_ws_id} 를 가진 window 가 없다");
            return;
        }
        if let Err(e) = self.send_list_dir_request(local_ws_id, request_id, "", None) {
            tracing::warn!("원격 홈 조회를 보내지 못했다 (mirror ws {local_ws_id}): {e}");
        }
    }
}
