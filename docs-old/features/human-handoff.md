# 휴먼 핸드오프 (Approval)

- **Status**: Implemented

위험한 동작 전에 사용자 결정을 동기적으로 받는 요청-응답 결정 게이트. `tasty-approval` 크레이트가 도메인 로직을, 호스트가 popup/persistence/CLI/IPC를 담당한다. 상세 사용 가이드: [agent-guide/approval.md](agent-guide/approval.md).

### 핵심 컴포넌트
- `tasty-approval` 크레이트: `ApprovalRequest`/`ApprovalRecord`/`ApprovalState`/`Severity` 도메인 모델 + `ApprovalStore` (in-memory queue + waiters). plugin이 자기 요청에 응답하는 self-response는 `-32011 self_response_forbidden`으로 거부
- IPC 9종 (Permission `Approval`; summary set/get은 `MemoryWrite`/`MemoryRead`도 함께 필요):
  - `approval.request` — 새 요청 생성. `severity` ∈ {info, warn, danger}, `workspace_id` 자동 fallback
  - `approval.respond` — GUI/CLI 양쪽이 같은 IPC로 수렴
  - `approval.await` — local-only (blocking + timeout). plugin 호출 미지원, host 내부 worker thread에서만
  - `approval.cancel`, `approval.get`, `approval.list`, `approval.history` — 조회/취소
  - `approval.summary.set`/`get` — workspace별 markdown 세션 요약 (수동 작성)

### Popup 통합 (Phase 3.2)
- popup `"approval"`이 `pending_approval_ids` 큐의 head를 그림. severity ∈ {warn, danger}는 popup + 알림, info는 알림만
- 응답 경로 3가지가 모두 같은 IPC `approval.respond`로 수렴: ① popup 선택지 버튼 클릭, ② popup 단축키 1..=9 (선택지 순서), ③ CLI `tasty approval respond`
- 응답 후 head pop → 큐가 비어 있지 않으면 자동으로 다음 head로 재오픈
- Esc는 의도적으로 차단 — 우회 응답 방지

### 영속 & 히스토리 (Phase 3.3)
- 매 상태 전이마다 record를 `tasty.approval.<id>` 키로 직렬화. `workspace_id` 있으면 `scope=workspace:<id>`, 없으면 `scope=global`
- `approval.history`는 모든 scope의 prefix `tasty.approval.` 키를 훑어 in-process 필터링 (since/until/workspace_id/requester_id/decision/state/limit). 재시작 후에도 조회 가능
- 응답/타임아웃/취소 후에도 record는 보존됨

### 세션 요약 (Phase 3.5)
- 별도 키 `tasty.approval.summary` (`scope=workspace:<id>`) — history와 격리
- markdown 자유 입력. CLI는 `@file` prefix로 파일 내용 첨부 지원
- 향후 Phase 4.5의 `telemetry.session_summary`(자동 생성)와는 별개 경로

### CLI
- `tasty approval {request,respond,await,cancel,get,list,history,summary {set,get}}`
