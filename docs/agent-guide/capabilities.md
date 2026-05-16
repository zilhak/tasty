# 권한 / Capabilities / Audit

Plugin 과 agent (claude.spawn 같은 호스트 launched 자식) 가 IPC 호출 시 호스트가 권한을 강제하고, 부족할 때 사용자에게 popup 으로 즉시 elevation 을 받고, 모든 호출의 allow/deny 결정을 audit log 에 영속하는 일련의 흐름. plugin 시스템 (manifest 권한) 위에 얹은 런타임 권한 모델이다.

## 큰 그림

```
[agent IPC 호출]
   │
   ▼
[dispatcher: ensure_allowed]  ── 통과 → telemetry/cap_block 검사 → 핸들러 실행
   │ 거부
   ▼
[Agent + MissingPermission 인가?]
   │ 예 → approval.request 자동 발행 (kind=capability_elevation)
   │      에러 응답에 { kind, approval_id, permission, method } 첨부
   │      → agent 가 approval.await 하면 응답에 맞춰 임시 grant 자동 적용
   │ 아니오 → -32001 permission_denied 만 반환
   │
   └────────────────────────────────────────► [모든 결정 audit log 영속]
```

세 축:
1. **권한 평가** — `method_meta` 가 메서드별 필요 권한을 선언. caller (Plugin/Agent) 의 `permissions` 셋에 모두 포함돼야 통과.
2. **Capability elevation** — Agent 가 권한 부족으로 거부될 때 자동 popup. 사용자가 approve / approve_permanently / deny 선택, approve 면 임시 grant TTL 부여.
3. **Audit log** — 모든 IPC 호출 (allow + deny) 을 영속 (`tasty.audit.{ts:013}.{seq:04}`, Global scope, 기본 30일 보존). 운영자가 `plugin.audit_*` 로 조회.

## Agent session 권한

agent (claude.spawn 등으로 호스트가 띄운 자식 프로세스) 는 launch 시 `session.issue` 로 token 을 발급받고, 자기 권한 셋이 그 토큰에 묶인다.

- **Base permissions**: `session.issue { permissions: [...] }` 로 issue 시 부여. 부모 caller 의 권한 셋의 부분집합만 가능 (escalation 금지). Local caller (CLI) 는 무제한.
- **Temp grants**: `plugin.grant_agent_permission` 으로 런타임 추가, TTL 지정 가능. base 와 분리 슬롯 — base 와 무관하게 추가/회수.

자식 프로세스는 모든 IPC 호출에 `TASTY_SESSION_TOKEN` 환경변수의 토큰을 envelope 의 `session_token` 필드로 첨부한다. 호스트가 token 으로 `CallerContext::Agent` 를 만들고 권한을 평가한다. invalid/expired/revoked 토큰은 `-32001 permission_denied` 로 거부 — Local 로 fallback 하지 않는다 (위조 방어).

| 작업 | IPC | CLI |
|---|---|---|
| 토큰 발급 | `session.issue { agent_id, permissions?, ttl_ms? }` (AgentManage 권한) | — |
| 토큰 회수 | `session.revoke { token }` | — |
| 세션 목록 | `session.list` (local-only) | — |
| 임시 grant 추가 | `plugin.grant_agent_permission { agent_id, permission, ttl_secs? }` (local-only) | `tasty plugin grant-agent-permission --agent X --permission fs.write --ttl 3600` |
| 임시 grant 회수 | `plugin.revoke_agent_permission { agent_id, permission }` (local-only) | `tasty plugin revoke-agent-permission --agent X --permission fs.write` |
| 권한 셋 조회 | `plugin.list_agent_permissions { agent_id? }` (plugin-callable readonly) | `tasty plugin list-agent-permissions [--agent X]` |

## Capability elevation flow

agent 가 권한 부족으로 거부되면 호스트는 같은 (agent_id, permission) 의 Pending elevation 이 없을 때 자동으로 `approval.request` 를 발행한다.

```
[agent]  fs.write 호출
   │
   ▼
[host dispatcher]  ensure_allowed 실패 (MissingPermission(fs.write))
   │
   ▼
[host]  approval.request(kind=capability_elevation,
                          choices=[approve, approve_permanently, deny],
                          metadata={ permission: fs.write,
                                     agent_id: ...,
                                     method: ...,
                                     grant_ttl_secs: 3600 })
   │   (같은 (agent_id, permission) 의 Pending 이 이미 있으면 재사용 — popup 폭주 방지)
   │
   ▼
[host]  -32001 응답, error.data = { kind, approval_id, permission, method }
   │
   ▼
[agent]  data.approval_id 로 approval.await(id) 폴링
   │
   ▼
[user]   popup 에서 approve / approve_permanently / deny 클릭
   │
   ▼
[host]   approval.respond → 응답 처리 후, choice 에 맞춰:
          - approve              → 3600s TTL 임시 grant
          - approve_permanently  → 무기한 grant
          - deny                 → grant 없음
   │
   ▼
[agent]  await 응답 받음 → 같은 IPC 재호출 → 이번엔 통과
```

agent 가 elevation 을 **미리** 발행할 수도 있다 — 거부 대기하지 말고 명시적으로 popup 띄우기:

```bash
tasty plugin request-permission --agent claude:child-1 \
    --permission fs.write \
    --reason "Apply migration patch to repo"
```

IPC: `plugin.request_permission { agent_id?, permission, reason? } → { approval_id, agent_id, permission }`. Agent caller 는 `agent_id` 생략 시 자기 자신 id 로 fallback. plugin-callable (`Approval` 권한 필요).

응답 매핑 (approval.respond):
- `approve` → metadata.grant_ttl_secs (기본 3600s) TTL 로 grant 적용
- `approve_permanently` → 무기한 grant 적용 (revoke 전까지)
- `deny` → grant 없음. 다음 호출도 거부됨

## Audit log

모든 IPC 호출 — allow 와 deny 모두 — 이 dispatcher 의 단일 진입점에서 자동 영속된다. 호스트 자체 메서드 (`window.*`, `plugin.*` 등 main.rs 라우터 분기) 도 같은 hook 으로 기록.

레코드 형식:

```json
{
  "ts_ms": 1736012345678,
  "seq": 42,
  "caller_kind": "agent",          // local | internal | plugin | agent
  "caller_id": "claude:child-1",   // agent_id / plugin_id / "_host"
  "method": "fs.write",
  "decision": "allow",             // allow | deny
  "reason": null,                  // deny 사유 (allow 면 보통 null)
  "workspace_id": 2                // 호출 시점 active workspace (없으면 omit)
}
```

영속 키: `tasty.audit.{ts:013}.{seq:04}` (Global scope). `seq` 는 telemetry 와 공유된 단조 증가 — 같은 ms 안의 호출도 순서 결정 가능. 기본 보존 30 일, `audit_query` 호출 시 lazy evict.

### IPC (모두 local_only)

| 메서드 | 설명 |
|---|---|
| `plugin.audit_query { caller_kind?, caller_id?, method_prefix?, decision?, since_ms?, until_ms?, limit? } → { records, count }` | 필터된 record 목록. method_prefix 는 `surface.` 같은 접두사 매칭. since/until 은 ts_ms 범위. |
| `plugin.audit_summary { ...query 와 같은 필터, top_n?=10 } → { total, allow, deny, by_caller, by_method }` | 집계. by_caller / by_method 는 count 내림차순 top_n. |
| `plugin.audit_follow { ...필터, after_ts_ms?, after_seq?, limit?=100 } → { records, count, next_after_ts_ms, next_after_seq }` | `(ts_ms, seq)` 커서 보다 strictly 큰 record. 커서 미지정 첫 호출은 빈 배열 + 현재 latest 커서 (`tail -f -n 0` 시멘틱). |
| `plugin.audit_clear { before_ms? } → { removed }` | `before_ms` 이전 record 삭제 (생략 시 전체). |

### CLI

```bash
# 최근 100 건 조회 (deny 만).
tasty plugin audit-query --decision deny --limit 100

# 운영 상황 요약 — caller 별, method 별 top 10.
tasty plugin audit-summary --since-ms $(date -d '1 hour ago' +%s%3N)

# tail -f 스타일 폴링 (500ms 마다).
tasty plugin audit-follow --caller-kind agent --decision deny

# 7일 이전 record 삭제.
tasty plugin audit-clear --before-ms $(date -d '7 days ago' +%s%3N)
```

`audit-follow` 는 IPC 를 거치되 CLI 측에서 polling 루프 — IPC 자체는 stateless. 한 번에 가져올 batch 크기와 폴링 간격을 조정할 수 있다 (`--batch 100 --interval-ms 500`).

## Permission token 표

```
surface.read / surface.write
terminal.read / terminal.write / terminal.spawn
clipboard.read / clipboard.write
fs.read / fs.write
notification
memory.read / memory.write / memory.secret
approval
telemetry
agent                  # = AgentManage. session.issue / agent.* 일부 메서드
ipc.invoke:<prefix>    # 접두사 매칭 (예: ipc.invoke:memory.* 로 모든 memory.* 호출 허용)
```

매니페스트에서 `permissions = ["fs.write", "memory.read", "ipc.invoke:debug.*"]` 처럼 선언. 미선언 권한은 grant 자체가 불가능 — 운영자가 권한을 주려 해도 manifest 에 없으면 거부된다.

## 시나리오 빠른 참조

- **자식 Claude 가 처음으로 파일 쓰기 시도 → popup**: 거부 응답 받는 즉시 같은 호출 재시도하지 말고 `error.data.approval_id` 로 `approval.await` 폴링. 응답 받은 뒤 재호출하면 grant 가 이미 들어가 있어 통과.
- **임시 권한을 한 시간만 주고 싶다**: `tasty plugin grant-agent-permission --agent X --permission fs.write --ttl 3600`. TTL 만료 후 자동 회수, `audit_query` 의 deny 가 다시 시작되면 만료된 것을 확인 가능.
- **이상 행동 감지**: `tasty plugin audit-summary --since-ms <1시간 전>` 후 `by_method` 상단에 익숙치 않은 메서드가 있는지 확인. 특정 caller 를 일시 멈추려면 `session.revoke` 로 토큰 자체를 무효화.
- **사고 후 사후 분석**: `audit-query` 로 deny 시점 전후 5 분의 같은 caller 호출을 모두 timeline 으로 추출. record 의 `reason` 필드가 거부 사유를 담는다.

## 관련 문서

- 권한 enum / token 매핑 / 매니페스트 형식: [`dev-guide/plugin-permissions.md`](../dev-guide/plugin-permissions.md)
- agent 식별 / session token 구현: [`dev-guide/agent-identification.md`](../dev-guide/agent-identification.md)
- approval 일반 흐름 (capability_elevation 외 다른 kind 도 동일): [`approval.md`](approval.md)
