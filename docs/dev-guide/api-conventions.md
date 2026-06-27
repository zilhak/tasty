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

도메인 특수 verb(예: `claude.tell`/`broadcast`, telemetry `record`/`summary`, agent `task_*`/`barrier_*`, memory `bb_*`/`plan_*`/`cache_*`)는 표준 밖이지만 도메인 의미가 명확해 채택된 것들이다. 새 영역은 표준 verb 를 우선 검토하고, 채택 시 PR 에서 사유를 남긴다.

## 인자 규칙

- 대상 식별은 항상 `--<namespace> <id>` (`--surface 42`, `--tab 7`). **활성 객체 의존 금지**(포커스 독립성 — [focus 정책](../design/policies/focus.md)).
- 옵션은 kebab-case (`--strip-ansi`, `--since-mark`).

## CLI vs IPC

`crates/tasty-cli` 의 plugin CLI 빌더는 **top/sub 2단만** 지원 — plugin 이 `x.meta.set` 을 노출하려면 `tasty <plugin> meta-set` 같은 2단으로 매핑. 호스트 본체 CLI 는 3단 직접 빌드 가능.

`attach.*` IPC namespace 는 `tasty attach` 로 노출되지 않고 용도별 CLI 로 갈린다: `tasty remote attach`/`remote check`(release, 원격 SSH), `tasty debug attach`(debug 전용, 로컬 loopback). 근거·동작은 [attach-behavior](attach-behavior.md), 격리는 [debug-ipc](debug-ipc.md).

## plugin 점유 namespace

plugin 이 매니페스트로 contribute 하는 IPC namespace 는 호스트 예약어와 충돌 금지(`system surface tab pane workspace claude plugin hook global_hook message tool notification window debug ui ime split tree memory output approval telemetry` 등). 상세는 [plugin-development](plugin-development.md) "예약 prefix".

### auto_wait chain

`claude spawn`/`tell`, `codex spawn`/`tell` 은 1차 IPC 응답 직후 wait IPC 를 자동 chain 해 child 가 terminal state(`idle`/`needs_input`/`exited`)에 도달할 때까지 block 한다. 매니페스트 `[[contributes.cli.subcommand]].auto_wait` 한 필드로 선언적으로 켠다(plugin 핸들러 미수정, CLI dynamic runner 가 chain). `map_from_response`(1차 응답→wait params, 우선) + `map_from_request`(요청→fallback) + `polling`(state_field/terminal_states/interval). `polling` 과 `auto_wait` 동시 선언은 validator 가 reject(직교 — 전자는 *이 명령 자체가 wait*, 후자는 *응답 직후 다른 method chain*). `surface`↔`surface_id` 키는 자동 alias.

---

## 안정성 정책

### 버전 단계

| 단계 | break 정책 |
|------|-----------|
| 0.x | 적극 변경. break 는 `CHANGELOG.md` 에 `(BREAK)` 표기 + **한 minor 이상 deprecation 우선**(보안 예외 즉시 제거 가능) |
| 안정선 (현재) | SemVer 엄격. `api_version = "1"` schema 는 추가만 |
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
| 비동기 이벤트(`surface.event`/`command.invoke`/`ipc.result`) 의미 변화 · handshake/env(`TASTY_HOST_API_VERSION`/auth token) 계약 변경 · 예약 namespace·권한 토큰 정책 변경 | **major** |

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

`tests/changelog_unreleased.rs`(CHANGELOG `[Unreleased]` 절 존재 검증) + `cli_naming_count_drift.rs`(메서드 카운트 drift). PR 템플릿·`git-cliff` 초안·경로 기반 규칙은 점진 도입 대상.

## 관련

- [reference/api](../reference/api.md) — 전체 IPC/CLI 메서드 카탈로그
- [plugin-development](plugin-development.md) · [plugin-ecosystem](plugin-ecosystem.md) · [release](release.md)
