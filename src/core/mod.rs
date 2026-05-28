//! `Core` — 도메인 본체. GUI 가 떠 있든 떠 있지 않든 항상 존재하는 상태.
//!
//! Phase C 의 strangler fig 마이그레이션 중. 현재는 빈 골격이며, `Engine` /
//! `EngineState` 의 필드를 sub-step 마다 한 그룹씩 이동시킨다. 마이그레이션
//! 계획은 `.claude-workspace/plans/phase-c-strangler-restructure.md`.

use std::sync::atomic::AtomicU64;
use std::sync::{Arc, Mutex};

pub(crate) struct Core {
    /// Layout preset 디스크 캐시 (`~/.tasty/presets/`). App 전역 단일 인스턴스.
    /// `Arc<Mutex<>>` 로 MainWindow / PresetWindow / EngineState 모두에 공유한다 —
    /// 단일 source of truth.
    pub preset_store: Arc<Mutex<tasty_presets::PresetStore>>,

    /// Telemetry 이벤트 시퀀스 — 같은 ms 안에서 event_key 충돌 방지용 단조 증가 카운터.
    /// App 전역 단일. EngineState.telemetry_seq 는 이 Arc 의 clone (mirror).
    pub telemetry_seq: Arc<tasty_telemetry::TelemetrySeq>,

    /// Telemetry 이상 탐지 — 호스트 singleton. in-memory sliding window 만 보관.
    /// App 전역 단일. EngineState.anomaly_detector 는 이 Arc 의 clone (mirror).
    pub anomaly_detector: Arc<tasty_telemetry::AnomalyDetector>,

    /// 휴먼 핸드오프 — approval 요청/응답 큐 + 대기자 채널. App 전역 단일.
    /// EngineState.approval_store 는 이 Arc 의 clone (mirror).
    pub approval_store: Arc<tasty_approval::ApprovalStore>,

    /// Agent task ID 시퀀스 — 같은 ms 안에서 task_id 충돌 방지용 단조 증가 카운터.
    /// App 전역 단일. EngineState.agent_seq 는 이 Arc 의 clone (mirror).
    pub agent_seq: Arc<AtomicU64>,
    // sub-step 마다 더 추가됨 (plan 참조).
}

impl Core {
    pub(crate) fn new() -> Self {
        Self {
            preset_store: Arc::new(Mutex::new(tasty_presets::PresetStore::load_default())),
            telemetry_seq: Arc::new(tasty_telemetry::TelemetrySeq::new()),
            anomaly_detector: Arc::new(tasty_telemetry::AnomalyDetector::new()),
            approval_store: Arc::new(tasty_approval::ApprovalStore::new()),
            agent_seq: Arc::new(AtomicU64::new(0)),
        }
    }
}
