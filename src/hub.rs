//! `Hub` — 외부 통신 표면. IPC 서버, 포트 파일 등 *프로세스 외부* 와 주고받는
//! 인프라를 모은다.
//!
//! Phase C 의 strangler fig 마이그레이션 중. 현재는 빈 골격이며, `Engine` 의
//! `ipc_server` / `port_file` 가 C.1.5 sub-step 에서 이쪽으로 이동한다.

#[allow(dead_code)]
pub(crate) struct Hub {
    // C.1.5 — ipc_server, port_file
}

impl Hub {
    pub(crate) fn new() -> Self {
        Self {}
    }
}
