# 유닛 테스트 격리 — 사용자 환경을 읽지 않는다

유닛 테스트의 결과는 **실행하는 사람의 로컬 상태에 좌우되면 안 된다.** 사용자 홈의
`config.toml` 값 하나로 무관한 테스트가 깨지면, "내 변경이 깬 것인가" 판정이 매번 수동
대조가 되고 CI 러너와 개발자 머신의 결과가 갈린다. 결정 근거는
[ADR-0045](../adr/0045-test-isolation-and-harness.md).

e2e 테스트의 격리 단위(프로세스 vs workspace)는 다른 축이다 — [e2e-tests](e2e-tests.md) ·
[ADR-0045](../adr/0045-test-isolation-and-harness.md).

## 1. 설정: 테스트 생성자는 `Settings::default()` 를 쓴다

`CoreState` 생성자는 설정을 **어디서 얻는지** 로 갈린다.

| 생성자 | 설정 출처 | 용도 |
|---|---|---|
| `CoreState::new(cols, rows, waker)` | `Settings::default()` | 테스트 / non-host 진입점 |
| `CoreState::new_with_ids(...)` | `Settings::load()` (`$TASTY_HOME/config.toml`) | host 부팅 경로 |

둘 다 내부 `new_with_ids_and_settings(..., settings)` 로 합류한다 — 설정 주입 지점이
여기 하나뿐이라, 새 진입점을 만들 때도 "파일을 읽을 것인가" 를 명시적으로 고르게 된다.

**테스트에서 특정 설정이 필요하면 엔진을 만든 뒤 그 필드만 바꾼다** — 사용자 홈에 그 값이
있기를 기대하지 않는다:

```rust
let (mut state, mut engine) = test_state();
engine.settings.general.workspace_categories_enabled = true;
```

파일 로드 동작 자체(파싱·마이그레이션·폴백)의 검증은 `Settings` 쪽 테스트가 담당하며,
그쪽은 임시 디렉토리를 명시적으로 가리킨다.

## 2. 환경변수: 반드시 RAII 가드로 복원한다

`std::env::set_var` / `remove_var` 는 프로세스 전역이다. 테스트가 직접 호출하고 마지막 줄에서
정리하는 방식은 두 곳에서 샌다.

- **패닉 경로** — 단언이 깨지면 정리 줄에 도달하지 못한다.
- **`remove_var` 로 "정리"** — 실행 환경에 원래 값이 있었으면 그 값을 잃는다. `TASTY_HOME` ·
  `TASTY_SURFACE_ID` · `TASTY_AGENT_ID` 는 tasty 터미널 안에서 실제로 설정돼 있다.

어느 쪽이든 같은 프로세스의 **뒤따르는 테스트**가 오염된 환경을 물려받아, 변경과 무관한
실패가 생긴다. 그래서 env 조작은 Drop 에서 원값을 되돌리는 가드로만 한다.

호스트 crate(`src/`)는 `crate::test_support` 의 두 가드를 쓴다.

| 가드 | 역할 |
|---|---|
| `TastyHomeGuard` | 공유 락(`TASTY_HOME_ENV_LOCK`) 획득 + `TASTY_HOME` 을 임시 디렉토리로 교체 + Drop 에서 원값 복원. `path()` 로 그 임시 루트를 얻는다 |
| `EnvVarGuard` | 임의의 키 하나를 교체하고 Drop 에서 원값 복원. 직렬화 락은 호출부 책임 |

```rust
let home = crate::test_support::TastyHomeGuard::new();
let path = next_screenshot_path().expect("dir creation must succeed");
assert!(path.starts_with(home.path().join("screenshots")));
```

**홈을 임시 디렉토리로 바꾸는 것이 목적이면 `TastyHomeGuard` 가 아니라 `IsolatedHome` 이다.**
같은 모듈의 세 번째 가드이고 env 를 **안 만진다** — 그래서 이 절(§2)의 대상이 아니고 락도
없다. 둘 다 있는 이유는 검증 대상이 다르기 때문이다: `TASTY_HOME` env **해석 자체**가 검증
대상이면 `TastyHomeGuard`, 그저 사용자 홈을 안 읽으면 되는 것이면 `IsolatedHome` 이다.
후자가 §7 형태 D 의 표준 처방이다.

다른 crate 는 `test_support` 에 접근할 수 없으므로 같은 형태의 가드를 crate 안에 둔다.

| crate | 가드 | 다루는 키 | 락 |
|---|---|---|---|
| `tasty-host-plugin` | `test_support::HomeEnvGuard` (`bundle_sig` · `manager::pump` 공용) | `HOME` + `TASTY_HOME` | `HOME_ENV_LOCK` |
| `tasty-telemetry` | `agent_id::tests::AgentIdEnvGuard` | `TASTY_AGENT_ID` | `ENV_LOCK` |
| `tasty-cli` | `request::tests::SurfaceIdEnvGuard` | `TASTY_SURFACE_ID` | 단일 `#[test]` 안에 모아 대체 |
| `tasty-cli` | `cwd_resolve::tests::CwdGuard` | 프로세스 cwd | `CWD_LOCK` |
| `tasty-settings` | `general::tests::RelativeHomeGuard` | `TASTY_HOME` | `SERIAL` |

`tasty-settings` 의 `TmpHome` 은 이 표에 없다 — 그것은 **env 를 안 만진다**(홈 경로를
`_in(Some(home.path()))` 로 주입한다). env 를 만지는 것은 상대 `TASTY_HOME` 해석 자체가
검증 대상인 `RelativeHomeGuard` 쪽뿐이다.

**가드가 락을 쥐는가는 가드마다 다르고, 그 차이가 값이다.** `AgentIdEnvGuard` ·
`RelativeHomeGuard` · `CwdGuard` 는 **생성자가 자기 락을 직접 쥔다** — 호출부가 잊어도
직렬화가 안 깨진다. `EnvVarGuard`(본체)는 안 쥔다(락은 호출부 책임)이고,
`SurfaceIdEnvGuard` 는 아예 락이 없다(그 키를 만지는 시나리오를 단일 `#[test]` 안에
모아 구조적으로 직렬이다). **락을 안 쥐는 두 갈래는 그 조건을 호출 자리 주석에 밝혀야
한다** — `no_unserialized_env_mutation` 이 함수 범위에서 직렬화 증거를 찾는데, 가드
안쪽의 `set`/`drop` 에는 `lock()` 호출이 없기 때문이다.

<a id="같은-키에-락이-셋인데-그것이-옳다--배제-단위는-레포가-아니라-테스트-바이너리다"></a>
<a id="이-추론을-지탱하는-것은-pub-여부가-아니다"></a>
<a id="재는-법--경계를-먼저-물어라"></a>

### 환경변수 락의 범위는 테스트 프로세스다

환경변수는 프로세스별 상태다. 같은 테스트 바이너리에서 같은 키를 읽거나 바꾸는 시험은
하나의 락을 공유한다. 다른 바이너리의 락까지 합칠 필요는 없다.
현재 tasty-test-support의 TASTY_HOME_ENV_LOCK, tasty-host-plugin의 HOME_ENV_LOCK,
tasty-settings 테스트의 SERIAL은 각자의 테스트 프로세스에서 사용한다.

경계의 근거는 가시성만이 아니라 cfg(test) 선언이다. 의존성으로 컴파일할 때 테스트 전용
모듈은 포함되지 않는다. 해당 선언을 제품 코드로 옮기면 같은 바이너리에 함께 들어오는지와
락의 공유 범위를 다시 확인한다. tasty-host-plugin의 test-only 선언 검사와 루트의
락 우회 검사도 서로 다른 조건을 보호한다.

TASTY_AGENT_ID를 보호하는 ENV_LOCK이나 cwd를 보호하는 CWD_LOCK은 자원이 다르다.
락 개수만 보고 합치지 말고 보호하는 상태와 프로세스 경계를 확인한다.

### 락은 **키 단위**로 하나 — 모듈마다 따로 두지 않는다

같은 crate 안에서 같은 키(또는 서로를 덮는 키 쌍)를 건드리는 테스트가 **서로 다른 락**을
잡으면, 락이 있어도 격리가 되지 않는다. 두 테스트가 같은 테스트 바이너리에서 병렬로 돌며
한쪽이 세운 `TASTY_HOME` 을 다른 쪽이 지우거나 덮어쓰고, 그 순간 앞 테스트의 파일 쓰기가
**사용자의 실제 `~/.tasty{-debug}`** 로 간다. 증상은 스케줄링에 따라 나타났다 사라진다.

`HOME` 과 `TASTY_HOME` 은 `tasty_home()` 이 함께 보는 **한 쌍**이므로 같은 락으로 묶는다.
`tasty-host-plugin` 이 `HomeEnvGuard` 하나로 통일한 이유다 — 그 crate 는 두 키를
`test_support` 한 곳에서만 만지며, 그 사실을
`test_support::tests::home_env_is_only_touched_through_this_module` 가 소스 스캔으로 고정한다
(다른 곳에 `env::set_var("HOME"…)` / `…("TASTY_HOME"…)` 가 생기면 그 테스트가 파일·행을
지목하며 실패한다).

## 3. `TASTY_HOME` 은 `HOME` 을 이긴다

`tasty_utils::path::tasty_home()` 의 우선순위는 **`TASTY_HOME` → `$HOME/.tasty{-debug}`** 다
(`crates/tasty-utils/src/path.rs`). 따라서 `HOME` 만 임시 디렉토리로 바꾸는 격리는 실행
환경에 `TASTY_HOME` 이 잡혀 있으면 통째로 무시된다 — 테스트가 임시 홈이 아니라 그 루트를
읽는다. `HOME` 파생 경로를 강제하려는 가드는 `TASTY_HOME` 도 함께 비워야 한다
(`tasty-host-plugin` 의 `HomeEnvGuard::derived_from_home()` 이 그렇게 한다. 반대로 임시 루트를
직접 지정하면 되는 경우는 `HomeEnvGuard::tasty_home()`).

## 4. 파일시스템 픽스처는 테스트가 직접 만든다

경로 해석처럼 `exists()` 로 실존을 검사하는 함수의 테스트는 **git 에 있는 경로**(`Cargo.toml`,
`src/adapters/ui` 등 `CARGO_MANIFEST_DIR` 기준)나 **테스트가 임시 디렉토리에 스스로 만든 경로**만
입력으로 쓴다. gitignored 로컬 작업 폴더나 사용자 홈처럼 *이 머신에만 있는* 경로의 실존에
기대면 clone 직후·CI 러너에서 결과가 달라진다 — 로컬 상태 의존이라는 점에서 설정·env 와 같은
축이다. 워크스페이스 dev-dependency 인 `tempfile` 로 만들고 `TempDir` 의 Drop 에 정리를 맡긴다.

```rust
let tmp = tempfile::tempdir().expect("tempdir");
std::fs::create_dir(tmp.path().join("notes")).expect("fixture dir");
let result = longest_existing_selection_path("notes/에", Some(tmp.path()), false);
assert_eq!(result, Some(tmp.path().join("notes")));
```

이 규칙은 `crates/tasty-doc-guards/tests/no_todo_file_citation.rs` 와도 맞물린다 — 로컬 작업 폴더를 언급하면 하위
경로가 무엇이든, 아예 없든 그 테스트가 잡으므로(P6) 픽스처 때문에 allowlist 에 예외를 두지
않는다. 금지 범위와 범위 밖 항목은
[ADR-0049](../adr/0049-documentation-structure-and-evidence.md) 가 정본이다.

의존이 없는 작은 fixture에서 직접 이름을 짓는 경우에는 PID로 프로세스를 가르고,
함수 호출 사이에 유지되는 `static` 단조 counter로 같은 프로세스의 재호출을 가른다.
Codex 셸 실행 시험·유효 폰트 시험·파일 선택기 테스트 helper는 이 두 축을 함께 쓴다. 셸 시험의 production
prompt cleanup은 처음 받은 temp root를 재사용해 생성과 회수의 경로 규칙을 유지한다.


## 5. 확인 방법

사용자 실제 홈으로 테스트를 실행해 격리를 확인하지 않는다. 테스트가 그 홈에 쓴 파일이
다음 실행의 기준값을 바꿀 수 있다. 빈 임시 TASTY_HOME에서 쓰기와 읽기를 각각 확인한다.

```bash
VERIFY_HOME=$(mktemp -d)
TASTY_HOME="$VERIFY_HOME" cargo test --workspace --locked
find "$VERIFY_HOME" -mindepth 1
```

격리된 엔진이 자기 임시 홈을 사용한다면 위 공통 홈에 잔여물이 없어야 한다.
잔여물이 생기면 작성 경로를 찾는다. 그 파일을 별도 임시 홈에 넣고 관련 시험을 단독 실행해
빈 홈에서의 결과와 비교한다. 전체 실행에서는 다른 시험이 같은 파일을 덮어쓸 수 있으므로
전체 통과만으로 사용자 상태를 읽지 않는다고 판단하지 않는다.

순서·병렬도·부하에 따라 실패가 달라지면 홈 잔여물, 실패 메시지, 시간 대조군을 함께 본다.
잔여물이 없다는 사실은 파일 쓰기 누출이 관측되지 않았다는 뜻이며 모든 격리 문제가 없거나
러너만 원인이라는 증명은 아니다. 병렬 부하 실험은 다른 검증과 조율하고 자기 인스턴스로만 한다.

격리 주입을 뺀 변이는 별도의 임시 홈에서 수행한다. 정상 통과 여부뿐 아니라 생성한 파일을
대조한다. 격리가 풀려도 테스트 자체는 통과할 수 있다. 원복 뒤에는 격리와 결과를 다시 확인한다.

## 6. feature 별 테스트 게이팅

본 바이너리는 `gui` feature 로 갈린다(`--no-default-features` = headless). **gui 전용 타입·모듈을
단정하는 테스트 모듈에는 `#[cfg(test)]` 가 아니라 `#[cfg(all(test, feature = "gui"))]` 를 건다.**
`#[cfg(test)]` 만 걸면 headless 테스트 **바이너리 자체가 컴파일되지 않는다** — 프로덕션 코드는
멀쩡히 `cargo check --no-default-features` 를 통과하는데도 그 feature 조합의 테스트가 한 줄도
실행되지 않는 사각이 생긴다. 개별 테스트 함수만 gui 전용이면 함수에 `#[cfg(feature = "gui")]` 를
건다.

같은 이유로 프로덕션 코드에서도 **gui 게이트된 재export 를 경유하지 않는다** — 예를 들어
`crate::terminal::*` 가 gui 전용 재export 라면 headless 에서도 도는 코드는 원본 크레이트를
직접 가리킨다(`tasty_terminal::*`).

CI 는 `.github/workflows/crossplatform-check.yml` 의 `check-headless` 잡이 이 조합
(`--no-default-features`)의 전체 스위트를 돌려 강제한다 — 통합 타깃도 돈다(채널 상세는
[`ci-gates.md`](ci-gates.md)).

## 7. 병렬 실행 경합(flake) — 공유 상태는 직렬화, 외부 자원은 소유

위 1~4 는 테스트가 **사용자 환경**을 읽어 로컬 상태에 좌우되는 축이다. 이 절은 다른 축 —
테스트끼리 **같은 프로세스에서 병렬로** 공유 상태를 밟아 스케줄링에 따라 나타났다 사라지는
실패다. 부류별 표준 처방과 근거·대안·재검토 조건은
[ADR-0045](../adr/0045-test-isolation-and-harness.md).

### 형태 A — 프로세스 내 전역 공유 상태

`static` 락/셀, 프로세스 env, 프로세스 cwd 처럼 인스턴스가 하나뿐인 상태를 테스트가
바꾼다. 처방은 **그 상태를 만지는 모든 테스트를 하나의 락으로 직렬화**하는 것이다.

- cwd: `set_current_dir` 는 프로세스에 하나뿐이라 자원을 테스트-로컬로 만들 수 없다 —
  직렬화가 유일하다(`tasty-cli` 의 `cwd_resolve` 테스트가 `CWD_LOCK` 을 함수 끝까지 잡는다).
- `static` 전역: 그 전역을 reset/read 하는 테스트는 락을 함수 끝까지, register 만 하는
  헬퍼는 그 호출을 감싼다(`surface_registry::webview_kind` 의 `WEBVIEW_KIND_TEST_LOCK`).
- env: §2 의 RAII 가드가 같은 처방의 특수형이다(획득 시 락, Drop 시 복원).

**락은 그 락을 잡는 코드끼리만 막는다(§2 "락은 키 단위" 함정의 하위형태).** 어떤 테스트가
락을 안 잡고 같은 자원을 만지거나 — 특히 그 자원을 읽는 **프로덕션 경로**를 간접 호출하면
— 락은 아무것도 막지 못한다. 그래서 처방을 적용하기 전에 "그 자원을 만지는 테스트가
이것뿐인가" 를 먼저 전수로 확인한다. 다른 크레이트의 같은 종류 접근은 별도 테스트
바이너리(별도 프로세스)라 경합이 아니다.

### 형태 B — 프로세스 밖 OS 자원 (포트·경로·소켓 TOCTOU)

`TcpListener::bind(":0")` 로 포트를 얻고 놓았다가 다시 bind 하는 사이, 고정 이름의 임시
파일을 공유하는 사이 다른 프로세스·테스트가 끼어든다. 처방은 **자원을 놓지 않거나**(리스너를
잡은 채 검증), 놓아야 하면 **lease + 재시도**, 고정 경로면 **유니크 이름**이다.

- 포트: `tasty-ssh` 의 `reserve_local_port` 는 포트를 점유하는 리스너를 함께 반환한다 —
  리스너를 잡고 있는 한 그 포트는 이 프로세스 소유라 rebind 레이스가 없다. 프로덕션에서
  ssh 가 rebind 해야 하는 경우만 명시적으로 drop 하고, 그 창은 ready-probe 재시도가 흡수한다.

### 형태 C — 부하 의존 벽시계 마감(deadline)

공유 상태 경합이 아니라, "N 초 안에 일어난다" 를 **벽시계로 단정**하는 테스트가 러너 부하에
비례해 깨진다. 단독 실행은 빠르게 통과(수십~수백 ms)인데 완주/부하에서 deadline 을 소진하고
red 다. 처방은 벽시계 폴링을 **이벤트 대기**로 바꾸는 것 — 단, 그 이벤트(waker·EOF)가 부하나
플랫폼 때문에 유실·지연될 수 있으면(Windows ConPTY 는 자식이 죽어도 read EOF 가 늦다) **제품이
이미 가진 폴 주기를 상한으로 결합**한다(이벤트 우선 + 폴백).

- 같은 프로세스 안: `process_exited_eventually_emitted` 는 `recv_timeout` 상한을 제품의
  alive-check 주기(`ALIVE_CHECK_INTERVAL`)로 잘라, waker 가 안 와도 그 주기마다 `process()` 가
  `try_wait` 폴백을 돈다. 자식 사망은 확정이므로 다음 주기에서 잡힌다.
- 별도 프로세스(자식 셸): `spawn_shell` 이 `JoinHandle` 을 반환하게 하고 테스트가 `join` 한다 —
  벽시계 없이 자식 `output()` 완료를 기다린다. 프로덕션은 그 핸들을 `let _` 로 drop 해
  fire-and-forget 을 유지한다(핸들을 버려도 스레드는 detach 되어 계속 돈다).
- **타임아웃 상향은 처방이 아니다** — 발생 빈도만 낮추고(확률 저감) 부하가 그 상한을 넘는
  날 다시 깨진다. 근거는 [유닛 테스트 격리](unit-test-isolation.md).

### 형태 D — 사용자 홈을 기준선으로 삼는 시험

시험이 `tasty_home()` 아래의 파일을 읽는 프로덕션 경로를 간접 호출한다. 그 파일은 앞선
완주가 남긴 것이라 **기계마다 다르고**, 같은 기계에서도 어제의 실행이 오늘의 기준선을 바꾼다.
§1~4 가 *읽는 값*(설정 · env)을 갈아끼웠다면 이쪽은 **읽는 자리**가 그대로 남은 축이다.

**전체 실행은 통과하지만 단독 실행은 실패할 수 있다.** 완주에서는 같은 파일을 쓰는 다른
시험이 먼저 `save()` 로 오염을 덮어쓰므로, 문제의 시험이 읽을 때쯤 내용이 이미 깨끗하다.
그 시험만 떼어 돌리면 덮어쓸 것이 없어 오염이 그대로 읽힌다. 그래서 전체 실행 통과만으로 격리를 증명할 수 없다.

처방은 직렬화가 아니라 **자원의 테스트-로컬화**다 — `tasty_utils::path::push_home_override`
(스레드 로컬, env 미조작, 테스트 전용 홈 경로 주입)로 그 시험 전용 임시 홈을 세운다.

- 본체는 `CoreState` 가 `IsolatedHome` 을 **마지막 필드**로 들고, 조립 지점
  `new_with_ids_and_settings` 가 그것을 첫 줄에서 세운다. 그래서 생성자를 무엇으로 부르든
  — 픽스처를 거치든 그 함수를 직접 부르든 — 같은 격리를 받고, 생성 이후의 `save()` 까지
  엔진 수명 내내 덮인다(마지막 필드인 것이 그 계약이다: drop 순서가 선언 순서다).
- 엔진 없이 홈을 읽는 것을 만드는 시험은 그 헬퍼가 직접 가드를 쥔다 —
  `plugin_bridge::key_dispatch` 의 `mgr()` 이 그 형태다(`PluginManager::with_registries` 가
  생성 중에 `plugins-logs` 를 만든다).

**시험마다 다른 키(surface id 등)를 고르는 완화책은 처방이 아니다.** 같은 파일을 쓰는 자리가
하나 늘 때마다 손으로 지켜야 하고, 그 규율이 지켜졌는지 재는 채널이 없다 — 어겨도 완주는
통과하므로 실행 결과만으로 드러나지 않는다.

<a id="형태-판별식--증상-하나로는-안-갈린다"></a>

### 실패 형태는 조사 단서로 사용한다

| 관측 | 우선 확인할 것 |
|---|---|
| 제한시간을 소진하고 단독 실행은 빠름 | 스케줄링 지연, 대기 조건, 이벤트 누락 |
| 병렬 실행에서 즉시 실패 | 락 없는 전역 상태, 환경변수·cwd 변경 |
| 고정 포트·경로에서 실패 | 자원 소유, 재바인딩 사이의 경쟁, OS 파일 처리 차이 |
| 전체 실행은 통과하지만 단독 실행은 실패 | 다른 시험이 만든 선행 상태, 사용자 홈 기준값 |

한 패턴이 한 원인을 확정하지는 않는다. 성공·실패 실행의 자식 생존 상태, 이벤트,
사용한 경로와 대기 시간을 비교해 원인을 좁힌다. 양쪽에 공통으로 있는 로그만으로
실패 원인을 단정하지 않는다.

### temp 경로 판정의 변수 범위

`tasty-doc-guards`의 `temp_path` 판정기는 경로가 참조한 단순 `let` 바인딩의 유일성
성분을 같은 함수의 유효한 블록 안에서만 전파한다. 선언 이전에는 보이지 않고,
성분이 없는 동명 바인딩도 앞의 값을 가린다. 안쪽 블록을 나가면 바깥 바인딩이 다시
보이며, 다른 함수와 중첩 함수의 지역 변수는 섞지 않는다. 직접 성분을 읽는 기존
줄 창도 다른 함수나 해당 자리의 블록 바깥에서 성분을 빌리지 않는다.

검증은 전체 census의 합계뿐 아니라 **어느 자리가 이동했는지**를 대조한다.
`cargo test -p tasty-doc-guards --lib temp_path`는 동명 변수·shadowing·블록·선언 순서와
주석/문자열 마스킹을 검사한다. 이전 파일 전체 전파를 되살린 변이는 이 대조에서
실패해야 한다. `--test no_unshared_fixed_temp_path`는 실제 저장소의 공유 고정 경로,
시계 단독 격리, 미검토 재호출/전달 자리의 래칫을 함께 검사한다. 가드를 수선하면서
새로 드러난 약한 fixture도 성분 자체를 고치며 pin이나 면제로 통과시키지 않는다.

이 판정은 소스 텍스트 검사다. 매크로 확장·타입 해석·destructuring 패턴·임의 재대입의
데이터 흐름까지 Rust 컴파일러처럼 해석하지 않는다. 직접 성분의 줄 창과 helper로
넘겨진 temp root의 관측 한계도 남는다.

<a id="부하-재현과-ci러너-관측"></a>
<a id="가드가-실제로-도는지는-이름-으로-스캔은-집합-동등-으로-확인한다"></a>

### 부하와 CI 결과를 비교한다

부하 실험은 다른 검증과 조율한 환경에서 같은 커밋·명령으로 반복하고 재현률을 기록한다.
성공·실패 실행의 실제 상태와 시간 대조군을 비교한다. 특정 OS에서만 재현되는 문제는
해당 OS의 로그와 코드로 확인한다. 로컬 한 번의 성공이나 직전 변경 파일 목록만으로
원인이 해결됐거나 변경과 무관하다고 결론 내리지 않는다.

```bash
gh run view <run_id> --json jobs
gh api "repos/<owner>/<repo>/actions/jobs/<job_id>/logs"
```

잡 결론뿐 아니라 각 단계의 실제 실행·실패·생략을 읽는다. 로그가 만료되면 확인한 범위가
줄었다고 보고한다. 단계 번호가 없다는 사실만으로 실제 실행 여부를 추측하지 않는다.

소스 가드는 읽은 파일 집합과 하한을 함께 확인한다. 정확한 개수도 같은 수의 다른 파일로
바뀐 경우를 놓칠 수 있으므로 추가·누락 경로를 양방향으로 비교한다.
두 feature 조합에서 검사하려면 각각 `-- --list`로 이름을 찾고 실제 실행 로그도 읽는다.
목록 조회는 시험 실행이 아니다. 패키지·타깃 경계를 남겨 같은 이름을 합치지 않는다.

## 8. 공유 픽스처 `test_state()` 는 **진짜 프로세스를 띄운다**

`src/state/tests.rs` 의 `test_state()` / `test_state_with_memory()` 는 유닛 테스트가
`AppState` + `CoreState` 한 쌍을 얻는 표준 통로다. 그 안에서 `CoreState::new` 이 도는데,
이 생성자는 **기본 워크스페이스를 만들면서 실제 PTY 를 열고 실제 셸을 fork 한다**
(`Pane::spawn_terminal` → `tasty_terminal::Terminal::new` → `portable_pty` →
`std::process::Command::spawn`).

그러니 이 픽스처를 쓰면 그 시험은 **파일 몇 개를 읽는 시험이 아니라 프로세스를 하나
띄우는 시험**이다. 따라오는 것:

- 자식 셸 프로세스 하나와 그 PTY(master `/dev/ptmx` + slave `/dev/pts/N`).
- PTY 마다 exit-watcher OS 스레드 하나(`src/core/pty_registry.rs`).
- `std::process::Command::spawn` 이 exec 결과를 부모에게 알리려고 내부에서 만드는
  AF_UNIX SEQPACKET socketpair 한 쌍. 이 spawn 경로에서 관측할 수 있는 보조 지표다.
  아래 명령으로 소켓 생성 횟수를 세되, 실제 spawn 호출과 대조해 해석한다.

<a id="몇-번-띄우는지는-이렇게-센다"></a>
<a id="안-쟀다--그-총수의-귀속-그리고-재려면-무엇이-필요한가"></a>
<a id="접두마다"></a>
<a id="안-쟀다--동시에-살아-있는-fd-의-곡선"></a>

### 프로세스 수와 fd 사용량을 측정한다

```bash
cargo test -p tasty --lib --no-run
strace -f -e trace=socketpair -o /tmp/sp.txt <테스트 바이너리>
grep -c 'socketpair(AF_UNIX' /tmp/sp.txt
```

socketpair 수는 현재 실행 경로의 보조 지표다. 다른 소켓 사용과 프로세스 생성 방식을
구분하지 않으므로 일반적인 spawn 총수와 같다고 단정하지 않는다.
직접 spawn 경로를 계측하거나 호출 스택과 대조해 지표가 맞는지 확인한다.

어느 시험이 얼마나 만드는지 찾으려면 전체 이름을 기준으로 부분집합을 나눠 비교한다.
필터가 겹치거나 빠지지 않았는지 확인한다. 공유 초기화와 실행 순서 때문에 부분 실행의 합이
전체와 다를 수도 있으므로 합이 다르다는 이유만으로 분할 오류라고 결론 내리지 않는다.

총 spawn 수와 동시에 열린 fd의 최댓값은 다르다. 병렬도를 달리해 직접 실행한 PID의
fd 수와 종류를 비교한다. 폴링은 짧게 열린 fd를 놓칠 수 있으며 openat/close만 추적해도
socket·pipe·dup으로 만든 fd를 모두 재구성할 수는 없다. 사용한 관측 범위를 함께 기록한다.

### 그 spawn 을 건너뛰는 길

생성자 안에는 있다 — `pending_layout_restore` 가 차 있으면 기본 워크스페이스를 안 만들고,
따라서 셸도 안 띄운다. 그 자리를 채우는 것은 `restore_layout` 설정이 켜져 있고 `layout_slot`
이 실제로 읽히는 경우뿐이다.

**그러나 테스트에서 닿는 길은 아니다.** `CoreState::new` 은 `layout_slot` 에 `None` 을
넘기므로 그 가지가 아예 안 돈다. 지금 유닛 테스트가 이 spawn 을 피하는 수단은 **없다** —
`test_state()` 를 안 쓰는 것 말고는.

### 왜 이것이 격리 문서에 있나

§7 형태 B(프로세스 밖 OS 자원)의 모집단이 눈에 보이는 것보다 넓기 때문이다. PTY·자식
프로세스를 다루는 시험만 그 자원을 잡는 것이 아니라, **이 픽스처를 쓰는 모든 시험**이 잡는다.
어떤 시험이 그 자원을 만지는지 이름으로 짐작하면 틀린다 — `test_state()` 를 부르는지로 본다.

## 실패가 실행 순서와 부하에 따라 달라질 때

먼저 실패한 실행과 성공한 실행의 같은 로그를 비교한다. 양쪽에 모두 있는 경고만으로
실패 원인을 정하지 않는다. 단독 재실행이 통과했다는 사실만으로 원인까지 확정하지 않는다.

| 공유 대상 | 먼저 적용할 방법 | 남는 제한 |
|---|---|---|
| 테스트가 직접 초기화하는 전역 값 | 초기화부터 단언까지 같은 락을 유지 | 같은 값을 읽는 모든 테스트도 락에 참여해야 함 |
| 제품 코드도 읽는 환경변수·홈 경로 | 값을 인자로 전달하거나 테스트별 인스턴스 사용 | 쓰는 테스트만 잠그면 제품 읽기 경로는 보호되지 않음 |
| 사용 중이어야 할 포트 | 리스너를 유지한 채 넘기는 `PortLease` | 소비자가 번호만 받아 다시 bind하면 재시도가 여전히 필요 |
| 닫힌 포트를 전제한 오류 검사 | 가능하면 실제 소켓 대신 오류 변환 함수를 검사 | OS 동작이 대상이면 재시도 사유를 명시 |
| 파일·소켓 이름 | 임시 디렉터리나 프로세스별 고유 이름 | 여러 실행이 같은 고정 경로를 사용하지 않도록 확인 |
| 프로세스 종료·이벤트 도착 | 이벤트를 기다리고 타임아웃은 무한 대기 방지에 사용 | 성능 요구가 있을 때는 별도 지연 검사 필요 |

재시도, 동시 실행 제한, 타임아웃 증가는 실패 빈도를 줄일 수 있다.
그 조치를 썼다면 조치 없이 재현한 조건과 부하를 남기고, 근본 수정 작업을 별도로 추적한다.
로컬에서만 적용한 완화가 CI에서도 적용된다고 가정하지 않는다.

가드가 GUI와 headless 양쪽에서 실행된다고 보고하려면 두 빌드 조합의 `--list`에서
테스트 이름을 확인한다. 이름이 있는 것과 같은 파일들을 검사하는 것은 별개다.
검사 파일 수 하한은 큰 누락만 잡고, 위반 건수 고정은 위반 없는 파일의 누락을 놓친다.
파일 목록 전체가 같은지는 경로 집합을 비교해야 한다. 아직 구현하지 않은 검사는
요구사항으로 표시하며 현재 보호 수단에 포함하지 않는다.

### 하네스의 공유 락

하네스 락은 선언 위치보다 획득 방법과 보호 범위를 확인한다.
`test_harness_lock_unwrap_ratchet`은 루트 `tests/`의 비면제 `.lock().unwrap()` 등을 검사한다.
검사할 획득 지점의 최소 개수는 대규모 누락을 찾기 위한 하한이며 정확한 전체 개수가 아니다.
하한보다 많은 부분 누락과 크레이트 내부 하네스는 이 검사만으로 확인할 수 없다.
락을 잡은 채 단언이 실패할 수 있는지, poison 뒤에도 진단 수집이 필요한지는 별도로 검토한다.
