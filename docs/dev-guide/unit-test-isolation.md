# 유닛 테스트 격리 — 사용자 환경을 읽지 않는다

유닛 테스트의 결과는 **실행하는 사람의 로컬 상태에 좌우되면 안 된다.** 사용자 홈의
`config.toml` 값 하나로 무관한 테스트가 깨지면, "내 변경이 깬 것인가" 판정이 매번 수동
대조가 되고 CI 러너와 개발자 머신의 결과가 갈린다. 결정 근거는
[ADR-0096](../adr/0096-unit-tests-isolated-from-user-environment.md).

e2e 테스트의 격리 단위(프로세스 vs workspace)는 다른 축이다 — [e2e-tests](e2e-tests.md) ·
[ADR-0090](../adr/0090-test-isolation-by-workspace-not-process.md).

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

다른 crate 는 `test_support` 에 접근할 수 없으므로 같은 형태의 가드를 crate 안에 둔다.

| crate | 가드 | 다루는 키 | 락 |
|---|---|---|---|
| `tasty-host-plugin` | `test_support::HomeEnvGuard` (`bundle_sig` · `manager::pump` 공용) | `HOME` + `TASTY_HOME` | `HOME_ENV_LOCK` |
| `tasty-telemetry` | `agent_id::tests::AgentIdEnvGuard` | `TASTY_AGENT_ID` | `ENV_LOCK` |
| `tasty-cli` | `request::tests::SurfaceIdEnvGuard` | `TASTY_SURFACE_ID` | 단일 `#[test]` 안에 모아 대체 |
| `tasty-settings` | `general::tests::HomeGuard` | `TASTY_HOME` | `SERIAL` |

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
[ADR-0105](../adr/0105-no-nongit-path-refs-in-tracked-sources.md) 가 정본이다.

## 5. 확인 방법

격리가 실제로 됐는지는 **같은 명령을 서로 다른 환경에서 돌려 결과가 같은지**로 본다.

```bash
cargo test --workspace --locked                      # 사용자 실제 홈
TASTY_HOME=$(mktemp -d) cargo test --workspace --locked   # 빈 홈
cargo test --workspace --locked -- --test-threads=1  # 실행 순서 고정
```

세 결과가 갈리면 어딘가에서 사용자 환경이 새고 있다는 뜻이다.

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

CI 는 `cargo test --workspace --lib --bins --no-default-features --locked` 로 이 조합을 강제한다
(`.github/workflows/crossplatform-check.yml` 의 `check-headless` 잡). e2e/통합 테스트는 GUI
기동이 필요해 이 잡에서 제외한다.

## 7. 병렬 실행 경합(flake) — 공유 상태는 직렬화, 외부 자원은 소유

위 1~4 는 테스트가 **사용자 환경**을 읽어 로컬 상태에 좌우되는 축이다. 이 절은 다른 축 —
테스트끼리 **같은 프로세스에서 병렬로** 공유 상태를 밟아 스케줄링에 따라 나타났다 사라지는
실패다. 부류별 표준 처방과 근거·대안·재검토 조건은
[ADR-0129](../adr/0129-flaky-test-classes-and-standard-fixes.md).

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

### 가드가 실제로 도는지는 "이름" 으로, 스캔은 "집합 동등" 으로 확인한다

재진입을 막는 소스 스캔 가드는 자기 모수를 함께 낸다. 스캔한 파일 수를 **하한**으로만
두면 하한 위로 빠지는 부분 누락을 못 잡는다 — **스캔 모집단을 집합으로 못박아** 추가·삭제를
양방향으로 잡는다(하한 < 정확 건수 < 집합 동등).

가드가 두 CI 채널(check-windows=기본, check-headless=`--no-default-features`)에서 **실제로
실행되는지**는 잡 초록이 아니라 두 조합의 실행 목록에 그 가드 이름이 뜨는지로 본다:

```bash
cargo test --workspace --lib --bins --locked -- --list | grep <가드 이름>
cargo test --workspace --lib --bins --no-default-features --locked -- --list | grep <가드 이름>
```

둘 다에서 이름이 나와야 두 채널 모두 그 가드를 본다(§6 cfg 게이팅 함정의 실측판). 한쪽에만
뜨면 반대 채널은 그 가드를 영영 못 본다. `--list` 의 stdout 에는 타깃 경계가 없어 workspace
목록은 동명 타깃이 dedup 된다 — 타깃별로 가르려면 `2>&1` 로 stderr 의 `Running` 줄을 함께
받는다. 그리고 검사가 "위반 0" 을 냈을 때는 그것이 "위반 없음" 인지 "그 검사가 대상을 스캔
범위에 안 넣어 아무것도 안 본 것" 인지 — 검사가 실제로 본 모수를 함께 확인해 가른다.
