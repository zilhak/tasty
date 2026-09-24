# 권한 상승 & 감사 (Capability elevation & audit)

- **Status**: Implemented
- **주체**: AI Agent (요청) · 로컬 사용자 (승인) · 운영자 (audit 조회)
- **ADR**: 없음
- **코드**: dispatcher 권한 게이트(`handler.rs`), `src/adapters/ipc/audit.rs`, session/elevation 핸들러
- **화면**: capability_elevation approval popup
- **메서드 목록**: [reference/api](../../reference/api.md#plugin-관리-plugin-local-only)

## 목적

plugin/agent 가 IPC 호출 시 호스트가 권한을 강제하고, 부족하면 **popup으로 사용자에게 추가 권한을 요청**하고, **거부된 호출을 audit log 에 영속**한다. 매니페스트 권한([plugin-permissions](../../dev-guide/plugin-permissions.md)) 위에 추가한 런타임 권한 모델이다.

## 내부 동작

<a id="세-축"></a>

### 세 가지 역할

1. **권한 평가** — `method_meta` 가 메서드별 필요 권한 선언. caller 의 권한 셋에 모두 포함돼야 통과.
2. **Capability elevation** — Agent 가 권한 부족으로 거부되면 자동 popup.
3. **Audit log** — IPC 의 deny 결정을 dispatcher 단일 진입점에서 영속(allow 는 기록 안 함 — 아래).

### Agent session 권한

claude.spawn 등으로 띄운 자식은 `session.issue` 로 토큰 발급(base permissions = 부모 권한의 부분집합, escalation 금지 — 단 plugin 은 자기 namespace의 `ipc.invoke:<prefix>` 권한이 없어도 그 권한을 자식에게 부여할 수 있다). plugin namespace 메서드(표에 이름이 없는 것)는 agent 에게도 그 namespace 의 `ipc.invoke:<prefix>` 를 요구한다 — 없으면 아래 elevation 으로 연다([ADR-0012](../../adr/0012-request-admission-and-isolation.md)). 자식은 모든 호출에 `TASTY_SESSION_TOKEN` 을 envelope `session_token` 으로 첨부 → 호스트가 `CallerContext::Agent` 구성. invalid/expired/revoked 토큰은 `-32001`(Local fallback 안 함 — 위조 방어). 런타임 추가 grant 는 `plugin.grant_agent_permission`(TTL 가능, base 와 분리 슬롯).

### Elevation flow

Agent 가 권한 부족으로 거부되면 호스트가 (같은 (agent, permission) Pending 없을 때) `approval.request{kind=capability_elevation, choices=[approve, approve_permanently, deny]}` 자동 발행. 거부 응답의 `error.data` 에 `{approval_id, permission, method}` 첨부 → agent 가 `approval.await` 폴링 → 사용자 선택에 따라 임시(TTL)/무기한 grant 적용 → agent 재호출 시 통과. agent 가 `plugin.request_permission` 으로 **미리** 발행할 수도 있다(거부 대기 없이). 메커니즘은 [human-handoff](../human-handoff/index.md) 위에 얹힘.

### Audit log

레코드: `ts_ms, seq, caller_kind(local/plugin/agent), caller_id, method, decision(allow/deny), reason?, workspace_id?`. 영속 키 `tasty.audit.{ts}.{seq}`(global, query 시 lazy evict). `seq` 는 telemetry 와 공유 단조 증가.

**권한 거부(`deny`)만 기록한다.** 허용된 호출까지 저장하면 반복 조회가 대부분인 에이전트
작업에서 불필요한 데이터가 계속 쌓인다. 따라서 이 로그로 허용된 작업의 수행 이력을
추적할 수는 없다. 거부는 메서드와 관계없이 모두 기록한다.
근거와 재검토 조건은 [ADR-0009](../../adr/0009-state-storage-and-retention.md)를 따른다.

elevation 자체는 이 로그에 의존하지 않는다 — 승인 이력은 `tasty.approval.*` 로 별도 영속되고, elevation 발행 트리거도 deny 경로다.

보존: 관측 로그 3종 공통 정책(`store::log_retention`)을 따른다 — 50시간 + 5만 건 상한이며 부팅과 런타임 양쪽에서 집행된다.

## 인터페이스

- **AI Agent**: `session.issue`(AgentManage), `plugin.request_permission`(Approval), `plugin.list_agent_permissions`(readonly).
- **운영자/CLI (local-only)**: `plugin.{grant,revoke}_agent_permission` · `plugin.audit_{query,summary,follow,clear}`(`tasty plugin audit-query/summary/follow/clear` — follow 는 CLI 측 폴링).

## 관련

- [dev-guide/plugin-permissions](../../dev-guide/plugin-permissions.md) — 권한 토큰/매니페스트 · [human-handoff](../human-handoff/index.md) — approval 메커니즘
- [identity](../../identity.md) — 사용자/에이전트 분리
