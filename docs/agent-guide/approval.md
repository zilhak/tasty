# 휴먼 핸드오프 — Approval

에이전트가 위험한 동작 전에 사용자 결정을 **동기적으로** 받는 결정 게이트. 단방향 `notification.create` 와 달리 **요청-응답 워크플로우**다.

## 흐름

```
[에이전트]                            [tasty]                          [사용자]
  approval.request(...)  ─────►  큐 push + popup 노출 ───────────────►  GUI
                                  notification.create 자동 발화
  approval.await(id)     ─────►  blocking
                                  approval.respond(id, choice)  ◄──── GUI 클릭/숫자키 또는 CLI
                       ◄──────  응답 반환
  계속 진행
```

응답 경로 3가지:
- GUI popup 선택지 버튼 클릭
- GUI popup 단축키 1..=9 (선택지 순서대로)
- CLI `tasty approval respond --id <id> --choice <key>` (또는 다른 plugin)

세 경로 모두 같은 IPC `approval.respond` 로 수렴 — 영속/waiter 깨우기 동일.

## IPC

| 메서드 | 권한 | 설명 |
|---|---|---|
| `approval.request { title, body?, choices?, default_choice?, timeout_ms?, severity?, workspace_id?, surface_id?, metadata? }` | `Approval` | 새 요청 생성. `severity` ∈ {`info`, `warn`, `danger`}. `workspace_id` 미지정 시 활성 워크스페이스로 fallback. 응답: `{ id, record }`. |
| `approval.respond { id, choice, comment? }` | `Approval` | 응답 제출. self-response (같은 plugin 이 자기 요청에 응답) 는 `-32011 self_response_forbidden`. 이미 종료된 요청은 `-32010 already_responded`. |
| `approval.await { id, timeout_ms? }` | local-only | blocking 대기. `timeout_ms=0|null` 이면 record 의 `timeout_ms` 사용, 그것도 없으면 무한 대기. 응답: `{ outcome: "responded", choice, by, comment? }` / `{ outcome: "timed_out", default_choice? }` / `{ outcome: "cancelled" }`. plugin 호출은 미지원 (deadlock 방지). |
| `approval.cancel { id }` | `Approval` | 종료되지 않은 요청을 취소. waiter 가 `Cancelled` 로 깨어난다. |
| `approval.get { id }` | `Approval` | 단일 record 조회. 없으면 `null`. |
| `approval.list { state?, workspace_id? }` | `Approval` | in-memory 조회 (현 세션). `state` ∈ {`pending`, `responded`, `timed_out`, `cancelled`, `terminal`}. |
| `approval.history { since?, until?, workspace_id?, requester_id?, decision?, state?, limit? }` | `Approval` | 영속 기록 (memory store) 조회. 재시작 후에도 사용 가능. `since`/`until` 은 memory `updated_at` (unix ms). `decision` 은 응답한 choice key. 결과는 transition 시간 역순. |
| `approval.summary.set { workspace_id, content }` | `Approval` + `MemoryWrite` | workspace 별 markdown 요약을 저장 (overwrite). |
| `approval.summary.get { workspace_id }` | `Approval` + `MemoryRead` | workspace 요약 조회. `{ workspace_id, content, updated_at }` 또는 `content: null`. |

## CLI

```bash
# 새 요청. choices 는 콤마 구분 "key:label[:destructive]".
tasty approval request \
    --title "Apply migration 0042?" \
    --body @migration.diff \
    --severity danger \
    --choice "approve:Approve,deny:Deny:true" \
    --default-choice deny \
    --timeout-ms 30000 \
    --workspace-id 1

# 응답.
tasty approval respond --id req_abc123 --choice approve --comment "looks safe"

# 대기 — 응답 받을 때까지 stdin 차단. timeout 시 종료 코드 124.
tasty approval await --id req_abc123 --timeout-ms 30000

# 조회.
tasty approval get --id req_abc123
tasty approval list --state pending
tasty approval history --workspace-id 1 --decision deny --limit 20

# 세션 요약.
tasty approval summary set --workspace-id 1 --content @summary.md
tasty approval summary get --workspace-id 1
```

## Severity 별 표시

| severity | 채널 |
|---|---|
| `danger` | popup + notification. 사용자 직접 응답만 (self-response 거부) |
| `warn`   | popup + notification |
| `info`   | notification only (선택) |

popup 은 `pending_approval_ids` 큐의 head 를 그린다. 응답 시 head 가 pop 되고 다음으로 자동 이동. Esc 는 의도적으로 차단 — 우회 응답 방지.

## Diff Surface 와 연계

approval 의 `metadata.diff_surface_id` 로 surface 를 가리키면 GUI 가 자동 인식해 popup 에서 "Open diff" 안내를 제공 (사용자가 직접 surface 로 이동).

diff surface 자체는 별도 IPC:

```bash
# 좌/우 분할 diff 표시. apply_action 은 사용자가 Apply 클릭 시 클립보드에 복사된다.
tasty split --level surface --target this --type diff \
    --meta '{"title":"migration 0042","before_file":"/tmp/old.sql","after_file":"/tmp/new.sql","apply_action":"sqlite3 db.sqlite < /tmp/new.sql"}'
```

Apply 는 명령을 자동 실행하지 않고 클립보드에 복사한다 — 사용자가 자기 활성 터미널에 붙여넣어 실행한다. 이는 의도된 안전 동선이다.

## 영속 모델

- 매 상태 전이마다 record 가 `tasty.approval.<id>` 키 (memory) 로 직렬화된다.
  - workspace_id 있으면 `scope=workspace:<id>`, 없으면 `scope=global`.
- `approval.history` 는 모든 scope 의 prefix `tasty.approval.` 키를 훑어 in-process 필터링.
- 세션 요약은 별도 키 `tasty.approval.summary` (`scope=workspace:<id>`) — history 와 격리.
- 응답/타임아웃/취소 후에도 record 는 보존된다 (`approval.history` 로 조회 가능).
