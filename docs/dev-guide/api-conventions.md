# CLI / IPC API 규약 — 명명 + 안정성

IPC 메서드·CLI 명령의 **명명 규칙**과 **호환성/버전 정책**. 단일 진실 원천은 `crates/tasty-ipc/src/method_meta.rs::METHOD_TABLE`(release 표면) — 본 문서는 그 위의 규칙·예외·진화 절차다. 전체 메서드 카탈로그는 [reference/api](../reference/api.md).

## 형식

```
IPC 메서드: <namespace>.<verb>[_<modifier>]
CLI 명령:  tasty <namespace> <verb> [--<option>]
```

예: `surface.list` ↔ `tasty surface list`, `claude.spawn` ↔ `tasty claude spawn`.

- **namespace 단수형** (`surface`, NOT `surfaces`). list 반환 키는 복수 (`surfaces: [...]`).
- **root 예외**: `split`(pane 분할) · `tree`(surface tree)만 namespace 없이 root 에 등록(자주 쓰는 짧은 명령). 새 메서드는 이 예외에 동참 금지.
- **보조 도메인은 3단** `<namespace>.<sub>.<verb>` (예: `tool.ssh.*`, `surface.meta.*` 점 표기).

namespace 별 메서드 수는 `tests/cli_naming_count_drift.rs` 가 강제한다 — 추가는 같은 minor 내 OK(테이블 동기화 필요), **제거는 SemVer 위반**(major bump 필요). 카운트 snapshot 은 테스트가 SoT 라 본 문서에 박지 않는다.

## verb 화이트리스트

새 메서드는 적합한 카테고리의 verb 를 고르고, 밖이면 PR description 에서 정당화한다(가벼운 ADR — 별도 파일 불필요).

| 카테고리 | verb |
|----------|------|
| **Read**(부작용 없음) | `list`(컬렉션→array) · `info`(단일, id 필요) · `state`(스냅샷) · `get` · `count` · `read` |
| **Write** | `create` · `update` · `set`/`unset` · `move` · `close`(소프트) · `clear` · `remove`(closed 안 남김) · `destroy`(영구, 예약) |
| **Send/외부** | `send` · `paste` · `wait` · `wake` |
| **프로세스/세션** | `spawn` · `launch` · `kill` · `respawn` · `shutdown` |
| **권한/관리**(local-only) | `install`/`enable`/`disable`/`grant`/`revoke`/`permissions` |

**modifier 패턴** `<verb>_<modifier>` 로 변종 표현(`send_key`/`send_combo`/`read_since_mark`). 한 verb 에 modifier 5개 이상 누적되면 namespace 한 단계 분리 검토.

도메인 특수 verb(예: `claude.tell`/`broadcast`, telemetry `record`/`summary`, agent `task_*`/`barrier_*`, memory `bb_*`/`plan_*`/`cache_*`/`goal_*`)는 표준 밖이지만 도메인 의미가 명확해 채택된 것들이다. 새 영역은 표준 verb 를 우선 검토하고, 채택 시 PR 에서 사유를 남긴다.

## 인자 규칙

- 대상 식별은 항상 `--<namespace> <id>` (`--surface 42`, `--tab 7`). **활성 객체 의존 금지**(포커스 독립성 — [focus 정책](../design/policies/focus.md)).
- 옵션은 kebab-case (`--strip-ansi`, `--since-mark`).

## CLI vs IPC

`crates/tasty-cli` 의 plugin CLI 빌더는 **top/sub 2단만** 지원 — plugin 이 `x.meta.set` 을 노출하려면 `tasty <plugin> meta-set` 같은 2단으로 매핑. 호스트 본체 CLI 는 3단 직접 빌드 가능.

`attach.*` IPC namespace 는 `tasty attach` 로 노출되지 않고 용도별 CLI 로 갈린다: `tasty remote attach`/`remote check`(release, 원격 SSH), `tasty debug attach`(debug 전용, 로컬 loopback). 근거·동작은 [attach-behavior](attach-behavior.md), 격리는 [debug-ipc](debug-ipc.md).

## 응답 계약 — mirror 워크스페이스로 간 구조 op

대상이 **mirror(원격 attach client) 워크스페이스**인 구조 op(`tab.create`/`split`/`tab.close`/`tab.move`/`pane.close`/`surface.close`/convert 등)는 로컬에서 실행되지 않고 원격으로 forward 된다([remote-attach](../features/remote-attach/index.md#mirror-워크스페이스-내-구조-변경)). 그 응답은 **fire-and-forget success** 다:

```json
{ "forwarded": true, "workspace_index": 2 }
```

즉 **생성된 id(surface/tab/pane)를 담지 않는다.** 원격 실행은 비동기라 응답 시점에 아직 아무것도 만들어지지 않았기 때문이다. 결과는 나중에 `StructuralDelta` 역반영으로 mirror 트리에 반영된다.

따라서 **구조 op 의 응답에서 생성된 id 를 동기로 꺼내 쓰는 호출자를 새로 만들지 않는다.** 그런 호출자는 mirror 워크스페이스에서 조용히 깨지고(응답에 필드가 없다), 게다가 forward 큐는 IPC 응답과 무관하게 드레인되므로 **로컬은 실패인데 원격에는 리소스가 남는** 고아를 만든다. 그 id 가 반드시 필요한 method 는 mirror 워크스페이스를 대상으로 **거부**해야 한다 — 실제 선례가 `terminal.spawn` 이며, 그 결정과 배경은 [ADR-0086](../adr/0086-reject-terminal-spawn-into-mirror-workspace.md).

## 권한 표 등재 (라우터 ↔ METHOD_TABLE)

**IPC 라우터에 dispatch 분기가 있는 메서드는 예외 없이 권한 표에 등재한다** — plugin 에 열 것이면 `plugin(&[..])`, local caller 전용으로 둘 것이면 `local_only()`. 표는 `METHOD_TABLE`(+ debug 빌드 전용 `DEBUG_METHODS`, prefix fallback `PREFIX_RULES`, `crates/tasty-ipc/src/method_meta.rs`).

미등재는 "닫혀 있음"으로 대충 넘어가지 않는다. `method_meta()` 가 `None` 이면 plugin/agent 호출자는 `UnknownMethod` 로 거부되긴 하지만, 그 거부가 **정책인지 등재 누락인지 표만 봐서는 구분되지 않는다** — 나중에 권한을 재검토하는 쪽이 "닫으려던 것"과 "잊은 것"을 판별할 수 없다. `local_only()` 등재는 그 판단을 코드에 남기는 선언이다(거부 자체는 `NotPluginCallable` 로 바뀔 뿐 동작은 같다).

`tests/ipc_router_table_parity.rs` 가 라우터 소스의 `"<method>" =>` 팔을 전부 훑어 강제한다. **분기를 `if method == "…"` 형태로 쓰면 이 스캔에 잡히지 않는다**(`src/app/ipc/app_methods.rs` 가 그 형태다) — 그 계열에 메서드를 추가할 때는 게이트가 아니라 사람이 등재를 확인해야 한다. 등재 누락은 조용히 오래 남는 종류의 결함이라(형제 메서드가 전부 등재된 상태에서 한둘만 빠져도 아무 신호가 없다) 리뷰가 아니라 게이트로 잡는다. debug 빌드에서만 도는데, release 에서는 `DEBUG_METHODS` 가 설계상 비어 IPC 표면에서 사라지기 때문이다([debug-ipc](debug-ipc.md)).

## plugin 점유 namespace

plugin 이 매니페스트로 contribute 하는 IPC namespace 는 호스트 예약어와 충돌 금지(`system surface tab pane workspace claude plugin hook global_hook webhook message tool notification window debug ui ime split tree memory output approval telemetry timer` 등). 상세는 [plugin-development](plugin-development.md) "예약 prefix".

### auto_wait chain

일부 plugin 명령은 1차 IPC 응답 직후 wait IPC 를 자동 chain 해 대상이 terminal state(`idle`/`needs_input`/`exited`)에 도달할 때까지 block 할 수 있다. child terminal 의 파생 상태 `stale`([ADR-0072](../adr/0072-child-state-hook-observation-fusion.md))은 **기본 terminal state 집합에 넣지 않는다** — 무출력 임계값 기반 판정은 휴리스틱이라 오탐 시 아직 일하는 자식을 종결 처리하게 된다. 다만 hook 유실로 영구 대기하는 것보다 조기 탈출이 나은 소비자는 `terminal_states` 에 직접 `"stale"` 을 추가해 선택할 수 있다. 매니페스트 `[[contributes.cli.subcommand]].auto_wait` 한 필드로 선언적으로 켠다(plugin 핸들러 미수정, CLI dynamic runner 가 chain). `map_from_response`(1차 응답→wait params, 우선) + `map_from_request`(요청→fallback) + `polling`(state_field/terminal_states/interval). `polling` 과 `auto_wait` 동시 선언은 validator 가 reject(직교 — 전자는 *이 명령 자체가 wait*, 후자는 *응답 직후 다른 method chain*). `surface`↔`surface_id` 키는 자동 alias.

**`claude spawn`/`tell`, `codex spawn`/`tell` 은 더 이상 이 메커니즘을 쓰지 않는다** — 동기 블로킹 대신 완료 시 caller surface 에 알림 훅을 주입하는 이벤트 기반 모델로 대체됐다. claude 는 `claude-idle`/`needs-input`/`process-exit` hook → `claude.notify_done`(`crates/tasty-plugin-claude/src/handlers.rs`의 `register_notify_hooks` 참조), codex 는 `codex-idle`/`process-exit` hook → `codex notify-caller`([`docs/plugins/codex/index.md`](../plugins/codex/index.md) 참고)로 각각 구현. 두 핸들러 모두 hook 이 한 번 fire 되면 알림 후 `surface.locate` 로 target 생존을 확인해, 아직 살아있으면(process-exit 가 아니었으면) 형제 hook 을 재등록한다(자기재무장) — "spawn/tell 당 알림 1회"가 아니라 "child 가 exit 할 때까지 상태 전환마다 알림"이다. auto_wait/polling 스키마 자체는 삭제되지 않았다 — 번들 plugin 중 이를 실사용하는 소비자는 없으며(전수 grep 확인), 향후 외부/서드파티 plugin 소비자를 위해 스키마만 유지한다.

---

## 안정성 정책

### 버전 단계

| 단계 | break 정책 |
|------|-----------|
| 0.x (현재) | 적극 변경. break 는 `CHANGELOG.md` 에 `(BREAK)` 표기 + **한 minor 이상 deprecation 우선**(보안 예외 즉시 제거 가능). major bump 는 사용자 결정으로만 |
| 안정선 | SemVer 엄격. `api_version = "1"` schema 는 추가만. 진입 시점은 사용자가 결정 |
| 1.x | minor 추가, major break |
| 2.0 | `api_version = "2"` 시작. plugin 이 매니페스트로 명시 선택 |

### Break 분류

| 변경 | 분류 |
|------|------|
| 새 메서드/명령 추가 · 응답에 Option/Default 필드 추가 · optional+default 파라미터 추가 | minor |
| 메서드 rename (alias 있음) · `#[serde(other)]` fallback 있는 enum variant 추가 | minor (deprecation) |
| 메서드 rename (alias 없이) · required 파라미터 추가 · optional→required 승격 | **major** |
| 응답 필드 의미/타입/nullability 변경 · 제거 · 단위·포맷 변경(ms↔s) | **major** |
| default 값 의미 변경 · 새 권한 필요(기존 plugin 중단) · 에러 코드 의미 변경 | **major** |
| fallback 없는 enum variant 추가 · 컬렉션 정렬/페이지네이션 의미 변화 | **major** |
| 비동기 이벤트(`command.invoke`/`ipc.result`/`event.dispatch`) 의미 변화 · handshake/env(`TASTY_HOST_API_VERSION`/auth token) 계약 변경 · 예약 namespace·권한 토큰 정책 변경 | **major** |

이 표는 출발선이다. 새 분류가 필요하면 PR 에 명시하고 표에 추가한다.

### Deprecation 절차

1. 옛 표면 유지 + 새 표면 추가.
2. 옛 표면 호출 시 `tracing::warn!("deprecated: <old>, use <new>")`(`crates/tasty-ipc/src/alias.rs`).
3. `CHANGELOG.md` `Deprecated` 절에 제거 기한 기록.
4. 기한 직전 일괄 제거 PR.

deprecation 기간은 "한 minor 이상"이 원칙(보안·심각 버그는 즉시 제거 가능).

### plugin-protocol schema

`api_version` 메이저를 올리는 변경: 메시지 필드 의미 변경 · 메서드 제거(alias 없이) · 응답 형식 의미 변경 · handshake/auth 계약 변경. 추가만(새 메시지, optional+default 필드)은 같은 `api_version` 내 `crates/tasty-plugin-protocol/Cargo.toml` minor bump. 이력은 `crates/tasty-plugin-protocol/CHANGELOG.md`.

### 자동화 보조

`tests/changelog_unreleased.rs`(CHANGELOG `[Unreleased]` 절 존재 검증) + `cli_naming_count_drift.rs`(메서드 카운트 drift) + `ipc_router_table_parity.rs`(라우터 팔 ↔ 권한 표 등재 대조, 위 "권한 표 등재"). PR 템플릿·`git-cliff` 초안·경로 기반 규칙은 점진 도입 대상.

## 관련

- [reference/api](../reference/api.md) — 전체 IPC/CLI 메서드 카탈로그
- [plugin-development](plugin-development.md) · [plugin-ecosystem](plugin-ecosystem.md) · [release](release.md)
