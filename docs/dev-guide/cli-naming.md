# CLI / IPC 명명 규칙

이 문서는 Tasty의 IPC 메서드와 CLI 명령에 대한 명명 규칙을 정의한다. 단일 진실 원천은 `src/ipc/method_meta.rs::METHOD_TABLE`이며, 본 문서는 그 위에서의 규칙·예외·deprecation을 설명한다.

## 기본 형식

```
IPC 메서드: <namespace>.<verb>[_<modifier>]
CLI 명령:  tasty <namespace> <verb> [--<option>]
```

예: `surface.list` ↔ `tasty surface list`, `claude.spawn` ↔ `tasty claude spawn`.

## namespace 화이트리스트

현재 등록된 namespace (단일 진실: `METHOD_TABLE`):

| namespace | 도메인 | 메서드 수 |
|-----------|--------|----------|
| `surface` | 레이아웃 surface — 구조 조작 / 터미널 I/O / OSC 133 명령 인덱싱 / `meta.*` / IME prefix | 24 + `surface.ime_*` / `surface.meta.*` prefix |
| `memory` | 영속 키-값 store (`memory.*` + `memory.secret.*` sub-namespace + Phase 7 `bb_*` / `plan_*` / `cache_*` 공유 컨텍스트 래퍼) | 43 |
| `claude` | Claude Code agent 통합 (spawn/wait/broadcast 등) | 11 |
| `plugin` | plugin 관리 (install/enable/grant 등) — local-only | 8 |
| `debug` | 디버그 (사용자 입력 재현) — local-only | 7 |
| `tool.clipboard` | 클립보드 (`tool` namespace 2단 안의 서브) | 5 |
| `output` | 출력 옵저버 (`observe_start/stop/list/info`) | 4 |
| `telemetry` | 텔레메트리 — 측정/집계/cap/이상 탐지/세션 요약 (`cap.*`, `anomaly.*` 서브 포함) | 13 |
| `agent` | 다중 에이전트 협업 — `task_*` (Phase 5.1) + `barrier_*` / `semaphore_*` (Phase 5.2, list/delete 포함) + `lease_*` (Phase 5.3) + `task_reduce` (Phase 5.4) + `rate_limit_*` (Phase 5.5) | 26 |
| `workspace`, `tab`, `message`, `window` | 레이아웃·메시지 큐·window 관리 | 각 4 |
| `notification`, `hook`, `global_hook` | 알림·hook | 각 2-3 |
| `system`, `ui`, `pane` | 호스트 정보·UI 상태·pane 조작 | 각 1-2 |
| `<plugin_prefix>` | plugin이 매니페스트로 점유한 namespace | 가변 |

**root namespace 예외**: 다음 두 메서드는 `<namespace>.` 없이 root에 등록되어 있다. 사용자가 자주 호출하는 짧은 명령이라 namespace 부여를 미룬 결정.

- `split` — pane 분할
- `tree` — surface tree 조회

새 메서드는 이 형식 예외에 동참하지 말 것 (`split.<x>`, `tree.<x>` 같은 자식도 만들지 말 것).

## verb 화이트리스트

명명 규칙의 정합성을 강화하기 위해 verb를 카테고리별로 분류한다. 새 메서드를 추가할 때 적합한 카테고리를 선택하고, 화이트리스트 밖이라면 PR description에서 정당화한다 (가벼운 ADR — 별도 파일 불필요).

### Read (idempotent, 부작용 없음)
- `list`: 컬렉션 조회, 항상 array 반환 (현재 등록: `surface.list`, `workspace.list`, `tab.list`, `pane.list`, `plugin.list`, `window.list`, `tool.clipboard.list`, `notification.list`, `hook.list`, `global_hook.list` — 10개)
- `info`: 단일 객체 상세 조회 (id 필요) — `system.info`
- `state`: 비-구조화 상태 스냅샷 (idle/active 등)
- `get`: 단일 값 조회 — `surface.meta.get`, `tool.clipboard.get`
- `count`: 컬렉션 수 — `message.count`
- `read`: 외부 시스템에서 데이터 읽기 — `surface.read_since_mark`, `message.read`

### Write (생성/변경)
- `create`: 새 객체 생성, id 자동 부여 또는 명시 인자
- `update`: 기존 객체 부분 변경 (id 필요)
- `set`: 단일 필드 값 지정 (idempotent)
- `unset`: set의 역, 필드 제거
- `move`: 객체 위치/소속 변경
- `close`: 객체 닫기 (소프트 — 사용자 복원 가능)
- `clear`: 컬렉션 비우기
- `remove`: 단일 항목 제거 (closed_items에 안 남음)
- `destroy`: 영구 삭제. 현재 사용처 없음, 예약

### Send/외부 영향
- `send`: 외부 시스템(터미널/소켓)으로 데이터 송신
- `paste`: 클립보드에서 외부로 (tool.clipboard.paste)
- `wait`: 조건 충족까지 차단/대기 (`claude.wait`)
- `wake`: 다른 컴포넌트에 신호 (`surface.wake`)

### 프로세스/세션
- `spawn`: 새 프로세스/세션 시작 (`claude.spawn`)
- `launch`: 매니페스트 기반 실행 (`claude.launch`)
- `kill`: 강제 종료 (`claude.kill`)
- `respawn`: 종료 후 재시작 (`claude.respawn`)
- `shutdown`: 호스트 종료

### 권한/관리
- `install` / `enable` / `disable` / `permissions` / `grant` / `revoke` — plugin 관리 전용 (local-only)

### 도메인 예외 verb (1회 등장, 도메인 특수)

다음 verb는 표준이 아닌 도메인 특수 표현이다. 새 영역에서 채택할 때는 표준 verb로 우선 검토:

| 도메인 예외 | 의미 | 채택 시 검토할 표준 |
|-------------|------|---------------------|
| `tell` (claude.tell) | 한 줄 메시지 전달 (side-effect 있는 호출) | `send` |
| `broadcast` (claude.broadcast) | 다수에 동시 전송 | `send` (대상 = 다수) |
| `wake` (surface.wake) | 잠든 surface 깨우기 | `send` 또는 신규 verb |
| `parent` / `children` (claude.*) | 트리 탐색 | `info` (relation 필드) |
| `screen_text`, `screen_attrs`, `cell_info`, `cursor_position`, `is_typing`, `glyph_color` | 터미널 내부 상태 조회 | `read` 또는 `state` |
| `record` (telemetry.record / record_batch) | 메트릭 1건 / N건 영속 기록 | `create` — 단, `record` 가 "측정 사건의 기록"이라는 도메인 의미가 명확해 채택. write-only side-effect 가 핵심이라 `create` 의 "객체 생성 후 id 반환" 어휘와 어긋남 |
| `summary` / `timeseries` / `top` / `session_summary` (telemetry.*) | 집계 조회 (sum/시계열/top-N/세션 단위 묶음) | `list` (배열 반환) / `info` (단일) — 집계 결과는 컬렉션도 단일 객체도 아닌 "통계 표"라서 별도 명사 채택. `session_summary` 는 `.<sub>.<verb>` 3단 대신 단수 명사로 둔 이유: 세션 요약 자체가 단일 결과 객체이며 `summary` 와 의미가 겹치지 않게 prefix 차이로 구분 |
| `anomaly.list` (telemetry.anomaly.*) | 영속 anomaly 레코드 조회 | 표준 `list` — 서브 namespace 만 별도 |
| `cap.{set,list,remove,status,reset}` (telemetry.cap.*) | cost cap CRUD + 상태 + reset | 표준 verb 조합. `reset` 은 `triggered` 마크만 비우므로 `clear` 가 후보였으나 "발화 상태를 0 으로 되돌린다"는 도메인 의미상 `reset` 이 더 명확 |
| `task_{create,list,get,await,cancel,retry,graph}` (agent.task_*) | task DAG primitive — Phase 5.1 | `<verb>_<modifier>` 패턴으로 통합한 의도적 결정. 별도 `task` namespace를 만들지 않은 이유: Phase 05 plan(`05-collaboration.md`) §"단일 namespace 결정"에서 `task/barrier/semaphore/lease/rate-limit/capability` 6개 namespace 분리안을 거부하고 단일 `agent` namespace + modifier로 통합. 신규 verb `await` (blocking 또는 poll-based 응답 대기)는 향후 `agent.barrier_await` 등에도 재사용. `graph` (DAG 시각화)는 단일 도메인 명사 — `list`/`info`가 부적합한 "구조 표"라서 별도 등록 |
| `barrier_{create,signal,await,state}` / `semaphore_{create,acquire,release}` (agent.*) | 동기화 primitive — Phase 5.2 | 동일하게 `<verb>_<modifier>` 패턴. `barrier_await` 와 `barrier_state` 는 poll-based 단계에선 같은 응답이지만 의미를 분리: `state` 는 "현재 상태 조회", `await` 는 "원하는 상태 도달 대기" — scheduler 도입 시 `await` 만 long-poll/wakeup 으로 분기. `semaphore` 는 `acquire`/`release` 로 점유 시멘틱을 명시 (`get`/`set` 으로 표현하면 의미 불명) |
| `lease_{acquire,release,list}` (agent.*) | 협조적 자원 점유 — Phase 5.3 | 동일하게 `<verb>_<modifier>` 패턴. `acquire`/`release` 는 semaphore 와 같은 어휘로 점유 시멘틱 통일. `list` 는 표준 verb (workspace 의 lease 카탈로그 반환). 별도 `state`/`get` 동사를 도입하지 않은 이유: lease 는 본질적으로 "점유 holder 가 누구인가" 외 상태가 없어 `list` 의 단일 row 가 곧 단건 상태 |
| `task_reduce` (agent.*) | task 결과 합성 — Phase 5.4 | `<verb>_<modifier>` 패턴. `task_*` 군과 한 namespace 에 두면서 reducer 동작을 modifier 로 구분. 별도 `reducer_*` namespace 를 피한 이유: reducer 는 task 결과를 입력으로 받는 task-domain 동사이지 독립적 카테고리가 아니다 |
| `rate_limit_{set,list,remove,status}` (agent.*) | rate-limit — Phase 5.5 | 동일하게 `<verb>_<modifier>` 패턴. `set` 은 (agent, metric) 쌍 upsert + 버킷 reset (이미 있으면 같은 id 유지). `remove` 는 id 기반 삭제 — (agent, metric) 으로 찾지 않는 이유: list/status 가 항상 id 를 반환하므로 호출자가 id 로 정확히 가리키게 한다. `status` 는 `list` 와 분리 — 필터(agent/metric) 가 있어 의미적으로 "특정 버킷 상태 조회"에 가깝다 |
| `grant_agent_permission` / `revoke_agent_permission` / `list_agent_permissions` (plugin.*) | agent 임시 권한 CRUD — Phase 6.3 | `<verb>_<modifier>` 패턴으로 plugin namespace 내 통합. 별도 `agent_permission` namespace 를 만들지 않은 이유: 기존 `plugin.grant` / `plugin.revoke` (plugin 권한) 와 같은 도메인 — "권한 관리"이고, 대상만 plugin/agent 로 갈리는 짝꿍이다. `agent_permission` 으로 분리하면 grant 동작의 두 표면이 다른 namespace 로 흩어진다. plan(`06-permissions.md` §"신규 namespace 만들지 않음") 의 결정 |
| `request_permission` (plugin.*) | agent 가 capability_elevation popup 을 명시 발행 — Phase 6.4 | `<verb>_<modifier>` 패턴. `request` 는 신규 verb 지만 `approval.request` 와 의미 일치 (사용자 결정을 요청). approval 와 다른 점은 result 가 직접 grant 효과로 이어진다는 점 |
| `bb_{create,put,get,get_all,get_meta,delete_field,delete,list,exists}` + `bb_snapshot{,_get,_list,_delete,_restore}` (memory.*) | Blackboard 공유 컨텍스트 — Phase 7.1/7.4 | `<verb>_<modifier>` 패턴으로 `memory` namespace 안에 통합. 별도 `bb` namespace 를 만들지 않은 이유: 일반 `memory.*` 위의 키 컨벤션 래퍼이며 권한도 동일한 `memory.read`/`memory.write` — 권한·저장소·이벤트가 같은 도메인이라 namespace 를 쪼개면 grant/revoke 표면이 흩어진다. 신규 modifier `put` / `get` / `get_all` / `get_meta` / `delete_field` / `exists` 는 표준 verb 의 변종 (`get` + 컬렉션/메타/존재 검사). `snapshot` 은 신규 도메인 verb (캡처-앤-리플레이) 로 동사 자체가 표준 화이트리스트 밖이지만 도메인 의미가 명확해 채택. snapshot 의 CRUD 는 `snapshot{,_get,_list,_delete}` 4 개 modifier 로 표준 verb 셋과 정렬, `_restore` 만 신규 도메인 modifier — restore 가 "snapshot 으로 현재 상태를 덮어쓴다" 는 단일 시멘틱이라 `set` / `apply` 로 표현하면 부정확 |
| `plan_{create,get,list,delete,add_step,remove_step,update_step}` (memory.*) | Plan 공유 컨텍스트 — Phase 7.2 | `<verb>_<modifier>` 패턴. 별도 `plan` namespace 를 만들지 않은 이유는 bb 와 동일 — `memory` 위 키 컨벤션 래퍼. `agent.task_*` (실행기) 와 의미가 겹쳐 보이지만 plan 은 "상태만 기록" 이라 권한/표면이 다르다 — `agent` namespace 로 옮기면 plugin 매니페스트가 `agent` 권한까지 요구해 불필요한 escalation. `add_step` / `remove_step` / `update_step` 은 표준 verb (`add` / `remove` / `update`) + 도메인 modifier — 별도 `step` sub-namespace 를 만들지 않은 이유는 step 단건 단독 IPC 가 없고 (plan 전체 row 한 개에 직렬화), step 조작 verb 3 개뿐이라 분리 가치가 낮다 |
| `cache_{put,get,invalidate,clear,list}` (memory.*) | Cache 공유 컨텍스트 — Phase 7.3 | `<verb>_<modifier>` 패턴. 신규 verb `invalidate` 는 표준 `remove` 가 후보였으나 "캐시 무효화" 가 idempotent (없어도 성공) 라는 캐시 도메인의 표준 어휘를 그대로 채택 — `remove` 는 일반적으로 "있어야 성공" 의미가 강해 부정확. `clear` 는 표준 verb (workspace 의 모든 cache entry 삭제) |
| `audit_{query,summary,follow,clear}` (plugin.*) | IPC audit log — Phase 6.5 | `<verb>_<modifier>` 패턴. 별도 `audit` namespace (`audit.query` 등) 를 만들지 않은 이유: 운영 도구의 일부 (plugin/agent 권한 추적) 이며 `plugin.grant_agent_permission` / `request_permission` 과 같은 운영 영역 — `plugin.*` 운영 동사 군으로 통합한다 (plan §"audit namespace 신설 안 함"). `query` 는 기존 verb 화이트리스트 외 — 표준 `list` 와 차이: list 는 컬렉션 전체 반환 시멘틱이 강하고, `query` 는 다축 필터 + 시간 범위 + limit 라는 "검색" 시멘틱이 핵심 (memory.query 와 같은 어휘). `summary` 는 telemetry.summary 와 같은 명사. `follow` 는 신규 verb — tail -f 시멘틱 (커서 기반 polling). `clear` 는 표준 verb (대상은 모든 audit record 또는 before_ms 이전) |
| `feed_bytes`, `inject_mouse`, `inject_key`, `raw_key`, `send_key`, `send_combo` | 사용자 입력 재현 | (debug 전용, 명명 자유로움) |
| `fire_hook` | hook 트리거 강제 | (sample size 1, 일관 규칙 정착 보류) |

## modifier 패턴

`<verb>_<modifier>` 형태로 verb의 변종을 표현. 예:

- `send` / `send_key` / `send_combo` / `send_to` / `send_wait_idle` — send 계열 5개
- `set_idle_state`, `set_needs_input` — set 계열 modifier
- `read_since_mark` — read 계열 modifier

**휴리스틱**: modifier가 한 verb에 5개 이상 누적되면 namespace 한 단계 분리를 검토한다. 예: `surface.send_*`(5개)는 차후 `surface.input.{send,key,combo,wait_idle}` 같은 재구조화 후보.

단, 휴리스틱은 시점에 따라 적용한다. 현재 `surface.send_*` 5개는 1.0 freeze 전까지 유지, 1.0 직전 재검토.

## 명사 규칙

- namespace는 **단수형** (`surface`, `tab`, NOT `surfaces`)
- list 반환값의 키는 단수형 + s (`surfaces: [...]`)
- 보조 도메인은 `<namespace>.<sub>.<verb>` 3단으로. 예: 클립보드는 `tool.clipboard.*` (현재 패턴)
- `surface.meta.*` (점 표기). 0.7.0 에서 옛 underscore alias (`surface.meta_*`) 가 제거됨 — 새 호출자는 점 표기만 사용.

## 인자 규칙

- 대상 식별: 항상 `--<namespace> <id>` (예: `--surface 42`, `--tab 7`)
- **활성 객체 의존 금지** (포커스 독립성 원칙, `CLAUDE.md` 참조)
- 옵션은 kebab-case (`--strip-ansi`, `--since-mark`)

## CLI vs IPC의 차이

`src/cli/dynamic.rs`의 plugin CLI 빌더는 **top/sub 2단 구조만** 지원한다. 따라서 plugin이 `surface.meta.set`을 노출하려면 `tasty <plugin> meta.set` 또는 `tasty <plugin> meta-set` 같은 2단으로 매핑한다. 호스트 본체 CLI(`src/cli/request.rs`)는 3단 구조 직접 빌드 가능.

## Plugin이 점유하는 namespace의 규칙

plugin이 매니페스트에서 contribute하는 IPC namespace는 다음 호스트 예약어와 충돌 금지:

```
system, surface, tab, pane, workspace, claude, plugin, hook, global_hook,
message, tool, notification, window, debug, ui, ime, split, tree,
memory, output, approval, telemetry
```

(상세는 `docs/dev-guide/plugin-development.md` "매니페스트 검증 / 예약 prefix")

## 변경 절차

- **새 verb/namespace 추가**: PR description에 "이 verb/namespace가 위 화이트리스트 어디에 속하는지, 또는 왜 예외인지" 한 문단. 별도 ADR 파일 불필요.
- **이름 변경 (rename)**: alias map(`src/ipc/...`)에서 old → new 매핑 + `CHANGELOG.md` Deprecated 절. 옛 이름은 1.0 tag 직전 제거.
- **메서드 제거**: minor 버전에서는 deprecated 표시만, 실제 제거는 major.

자세한 break 분류·deprecation 절차는 (예정) `docs/dev-guide/ipc-stability.md` 참조.
