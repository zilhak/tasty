# ADR-0056: 도메인·이벤트 저장·작업 실행을 별도 crate로 나눈다

- **Status**: Accepted — tasty-core는 Command/Event·decide/evolve와 순수 projection/query를 소유하고 tasty-domain을 흡수했다. tasty-task-runtime은 TaskService/Scope·runner·완료 대기를 소유한다. root는 App·EngineRuntime·View·저장 adapter를 연결한다. 배치와 플랫폼별 실행 검증은 구분한다.
- **Date**: 2026-09-30
- **Tags**: architecture, crates, build, domain, headless
- **Group**: foundation

## Context

[ADR-0002](0002-domain-execution-and-ports.md)는 별도 core crate를 만들지 않고 `src/core`와 `src/ports`의 모듈 경계와 소스 검사로
의존 방향을 지키기로 했다. 당시 근거는 GUI 조건·형제 모듈·공개 API를 넓게 정리해야 해서 비용에 비해 이득이 작다는 것이었고,
재검토 조건은 외부 소비자나 편집 빌드 시간이었다.

지금은 전제가 다르다. [ADR-0054](0054-app-core-view-layers-and-state-ownership.md)와
[ADR-0055](0055-structural-domain-event-sourcing.md)로 순수 도메인, 이벤트 저장, 실행 자원을 어차피 나누어야 한다.
경계를 컴파일러가 확인하지 못하면 이행 도중 결합이 다시 생긴다. 현재 root `tasty` lib에는 Core·App·View·훅 실행·에이전트 호스트 실행이
한 컴파일 단위로 들어 있어, GUI 파일 하나를 고쳐도 도메인 검사를 다시 컴파일한다.

`tasty-model`의 `DeferredSpawn`은 `tasty_terminal::Waker`를 담고 Surface 트리는 동작을 가진 trait 객체를 담는다.
지금의 `tasty-model`을 그대로 순수 도메인의 의존으로 삼으면 PTY 실행 계층이 따라온다.

(결정 당시 상태. `DeferredSpawn`이 waker를 담던 결합은 이후 해소됐다. 현재 상태는 재검토 조건 절에 적는다.)

## Decision

crate는 상태 객체별이 아니라 함께 바뀌는 코드·의존성·테스트 범위로 나눈다. State는 해당 객체의 crate에 함께 둔다.
AppState·CoreState·ViewState 전용 crate나 모든 trait을 모으는 ports crate는 만들지 않는다.
[ADR-0001](0001-crate-dependency-boundaries.md)의 의존 방향·실행 환경 우선 원칙은 그대로 적용한다.

| 책임 | 배치 |
|---|---|
| Core, CoreState, 도메인 Command/Event, 순수 decide/evolve | 새 `tasty-core`. GUI·PTY·SQL 의존 없음 |
| EventStore, transaction, journal·snapshot·checkpoint·effect 기록 | 새 `tasty-event-store`. 처음부터 독립 crate로 작성 |
| TaskService와 그 상태, 러너 호스트 실행·완료 대기·복구 | 새 `tasty-task-runtime`. 기존 `tasty-agent`와 별도 crate |
| 작업·그래프·저장·RunnerLoop 알고리즘 | 기존 `tasty-agent` 재사용 |
| HookRuntime | root 내부 `hook_runtime` 모듈. 독립 API가 안정되면 추출 검토 |
| 훅 조건·매칭 | 기존 `tasty-hooks` 유지 |
| Remote, RemoteState, SSH/attach 실행 | 기존 `tasty-remote` 확장. `tasty-ssh` 역할 유지 |
| Terminal/TerminalState, Pty/PtyState | 기존 `tasty-terminal` 안의 별도 모듈. 별도 PTY crate는 보류 |
| 순수 구조 타입 | 실행 결합을 걷어낸 `tasty-model`. 새 domain-types crate는 만들지 않음 |
| EngineSession, CommandExecutor, EffectRunner, projection 공개·recovery 실행 | root 내부 runtime 모듈 |
| View·ViewState, renderer, UI intent | root GUI 모듈 |
| App·AppState, boot, CLI 진입, adapter 조립 | root `tasty` |
| plugin 프로세스·채널 | 기존 `tasty-host-plugin`·protocol·SDK |

- `tasty-core`가 도메인 Command/Event 타입을 소유한다. `tasty-event-store`가 이를 의존하는 것은 허용하고 반대 방향은 금지한다.
  저장 형식 버전·codec·migration은 EventStore가 명시하며, 도메인 타입 필드를 바꿨다고 저널 직렬화가 자동으로 바뀌지 않게 한다.
- `tasty-core` 추출은 `tasty-model`에서 실행 인스턴스를 걷어낸 뒤에 한다. `src/core` 폴더를 통째로 옮겨 이름만 바꾸지 않는다.
  실행 코드는 root runtime으로 분류한다.
- `tasty-task-runtime`은 소비하는 port를 자기 쪽에 정의하고 root가 로컬 adapter로 구현한다. plugin 호출은 `HostIpcInjector` 기반 adapter로 주입하며,
  runtime이 IPC 핸들러나 PluginManager를 직접 호출하지 않는다. TaskService와 HookRuntime은 서로 의존하지 않고 App이 연결한다.
  거대한 HostServices trait 하나나 공용 ports crate로 모든 호출을 받지 않는다.
- GUI 포함 여부는 최상위 `gui` feature로 표현한다. 서로 배타적인 `headless`·`gui` feature 두 개를 만들지 않고, 공용 crate 기본 feature에 GUI를 넣지 않는다.
- `tasty-runtime`·`tasty-view` 추출은 보류한다. 1단계 추출 뒤 `cargo build --timings`로 측정해 판단한다.
- 새 crate는 별도 프로세스·daemon·thread를 뜻하지 않는다. 배포 단위는 계속 하나의 `tasty`일 수 있다.
- crate 분리는 이벤트 소싱 범위를 넓히지 않는다. 여러 저장 crate를 한 실행 파일에 링크했다고 cross-DB 원자성이 생기지 않는다.

## Consequences

순수 도메인 검사를 GUI·SQLite·PTY 없이 package 단위로 실행할 수 있고, 도메인이 상위 계층을 참조하면 컴파일 오류가 난다.
작업 실행 테스트를 `-p`로 골라 돌릴 수 있고 GUI 변경이 도메인 재컴파일을 유발하지 않는 방향이 된다.

`pub(crate)`를 모두 `pub`으로 바꾸면 crate는 생기지만 경계가 나빠진다. 추출마다 공개 API를 좁게 설계해야 한다.
Core나 공용 schema 변경은 의존하는 crate를 다시 컴파일하며, crate 수 증가와 LTO·metadata 비용 때문에 clean·release 빌드가
반드시 빨라지지는 않는다. 속도 개선은 추출 뒤 측정한 값으로만 주장한다.

crate 목록 문서·README·가드의 crate 수를 추출마다 함께 갱신해야 한다.

## Alternatives Considered

- core crate 없이 모듈 경계와 소스 검사만 유지하는 안([ADR-0002](0002-domain-execution-and-ports.md)): 검사는 직접 경로만 보고 형제 모듈을 거친 전이 의존을 증명하지 못한다. 대규모 재배치 중에는 이 약점이 커진다.
- 상태 객체마다 crate를 만드는 안: 함께 바뀌는 코드가 흩어지고 공개 API만 넓어진다.
- 공용 ports crate를 먼저 만드는 안: 소비자가 정해지기 전의 trait이 모이고 모든 crate가 그 crate를 의존하게 된다.
- 기존 `tasty-agent`에 호스트 실행을 넣는 안: 작은 협업 API 소비자에 plugin·IPC 실행 의존이 얹힌다.
- `tasty-hooks`를 HookRuntime으로 확장하는 안: 작은 매칭 라이브러리에 registry·plugin manifest·IPC worker 의존이 들어간다.
- `tasty-model`을 두고 `tasty-domain-types`를 새로 만드는 안: 같은 역할의 crate가 둘이 된다.

## Reconsideration Triggers

### 코드와 설정에서 확인

- `tasty-model`에서 실행 인스턴스(`DeferredSpawn.waker` 등)를 걷어내면 `tasty-core` 추출 시점을 판단한다.
  현재 상태: `DeferredSpawn`은 waker를 담지 않고 `tasty-model`의 leaf는 `SurfaceDescriptor`다. kind 인스턴스와 Terminal/Pty는 EngineRuntime의 컬렉션으로 이동했다.
  `tasty-core` 추출 때 임시 `tasty-domain`의 원본 모델/명령/사건/codec을 합쳤다. CoreState의 로컬 가변 트리는 crate 내부 projection만 사용하며 root는 확정 batch의 공개·효과 실행을 조율한다.
  도메인 payload codec과 버전 변환은 tasty-core, 저장 봉투 형식과 migration은 EventStore 소유다.
- TaskService API와 host port가 정리되면 `tasty-task-runtime` 추출 시점을 판단한다.
- HookRuntime의 공개 API가 안정되고 root 밖 소비자가 생기면 별도 crate 추출을 검토한다.
- 순수 터미널 재생·원격 mirror·renderer 테스트에서 PTY 의존을 빼야 하면 별도 PTY crate 추출을 검토한다.
- CLI에 GUI·host 전용 큰 의존이 `tasty-remote`를 통해 들어오면 그 부분만 optional feature나 새 crate로 나눈다.

### 실행 결과로 확인

- 1단계 추출 뒤 같은 toolchain·profile·cache 조건에서 GUI leaf 수정·task runtime 수정·core schema 수정·clean 빌드를 `cargo build --timings`로 비교해 `tasty-runtime`·`tasty-view` 추출 여부를 정한다.

## References

- 대체 대상: [ADR-0002](0002-domain-execution-and-ports.md) — crate 배치 부분. 계층·상태 소유는 [ADR-0054](0054-app-core-view-layers-and-state-ownership.md)
- [ADR-0063](0063-event-store-storage-fencing-and-effect-states.md) — `tasty-event-store`의 payload 저장·writer 잠금·effect 전이·schema 버전 결정
- [ADR-0064](0064-journal-domain-model-crate.md) — `tasty-core` 추출 전에 저널 도메인 모델·evolve·Decider를 새로 작성하는 순수 도메인 crate `tasty-domain`. 이 ADR의 배치를 바꾸지 않는다
- [ADR-0001](0001-crate-dependency-boundaries.md) · [ADR-0055](0055-structural-domain-event-sourcing.md) · [ADR-0062](0062-task-service-and-hook-runtime.md)
- 현재 빌드 구조: [빌드 가이드](../dev-guide/build.md), [헤드리스 컴파일 경계](../dev-guide/headless-build-boundaries.md), [아키텍처](../architecture/index.md)
- 현재 구현: `crates/tasty-model`, `crates/tasty-agent`, `crates/tasty-hooks`, `crates/tasty-remote`, `crates/tasty-terminal/src/lib.rs`.
