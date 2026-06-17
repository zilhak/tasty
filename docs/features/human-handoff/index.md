# 휴먼 핸드오프 (Approval)

- **Status**: Implemented
- **주체**: AI Agent (요청) · 로컬 사용자 (응답)
- **ADR**: 없음
- **코드**: `approval.*` 핸들러(`src/adapters/ipc/handler/approval/`), 영속 `tasty-memory`
- **화면**: approval popup (pending 큐 head)
- **메서드 목록**: [reference/api](../../reference/api.md#휴먼-핸드오프-approval)

## 목적

에이전트가 위험한 동작 전에 사용자 결정을 **동기적으로** 받는 결정 게이트. 단방향 [알림](../notifications/index.md)과 달리 **요청-응답 워크플로우**다.

## 내부 동작

### 흐름

`approval.request` → 큐 push + popup 노출 + `notification.create` 자동 발화 → 에이전트가 `approval.await(id)` 로 blocking 대기 → 사용자 응답 → await 반환. 응답 경로 3가지가 모두 같은 `approval.respond` 로 수렴: popup 버튼 클릭 · popup 단축키 `1..=9`(선택지 순서) · CLI `tasty approval respond`.

### severity 별 표시

| severity | 채널 |
|----------|------|
| `danger` | popup + notification, **사용자 직접 응답만**(self-response 거부) |
| `warn` | popup + notification |
| `info` | notification only |

popup 은 `pending_approval_ids` 큐의 head 를 그리고, 응답 시 pop 하고 다음으로 자동 이동. **Esc 는 의도적으로 차단**(우회 응답 방지).

### 응답 규칙

self-response(같은 plugin 이 자기 요청에 응답)는 `-32011`, 이미 종료된 요청은 `-32010`. `approval.await` 는 local-only(plugin 호출 deadlock 방지) — `outcome ∈ {responded, timed_out, cancelled}`.

### 영속

매 상태 전이마다 `tasty.approval.<id>` 키로 직렬화(workspace_id 있으면 `workspace:<id>`, 없으면 `global`). 응답/타임아웃/취소 후에도 보존 → `approval.history` 로 재시작 후 조회. 세션 요약은 별도 키(`approval.summary.set/get`, markdown).

## 인터페이스

- **AI Agent**: `approval.request`(`Approval` 권한) → `approval.await`. capability elevation·cap require_approval 도 같은 메커니즘 위에 얹힌다([capability-elevation](../capability-elevation/index.md), [telemetry](../telemetry/index.md)).
- **사용자**: popup 응답 / `tasty approval {request,respond,await,get,list,history,summary}`.

## 관련

- [notifications](../notifications/index.md) · [capability-elevation](../capability-elevation/index.md) · [telemetry](../telemetry/index.md)
- [reference/api](../../reference/api.md#휴먼-핸드오프-approval)
