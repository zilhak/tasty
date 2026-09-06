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

### 형태 C — 부하 의존 벽시계 마감(deadline)

공유 상태 경합이 아니라, "N 초 안에 일어난다" 를 **벽시계로 단정**하는 테스트가 러너 부하에
비례해 깨진다. 단독 실행은 빠르게 green(수십~수백 ms)인데 완주/부하에서 deadline 을 소진하고
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
  날 다시 깨진다. 근거는 ADR-0129 "완화와 은폐의 경계".

### 형태 판별식 — 증상 하나로는 안 갈린다

| 지문 | 형태 | 근본 처방 |
|---|---|---|
| deadline 시간을 정확히 소진한 뒤 실패("30초"/"5초"), 단독은 수백 ms | C | 벽시계→이벤트+폴 주기 폴백 |
| 즉시 실패, 단독 green·병렬 red, 락 없는 `static`/env/cwd 접근 | A | 직렬화 락 또는 자원 테스트-로컬화 |
| 즉시 실패, 고정 포트/경로/파일 이름, drop-후-rebind, Windows 파일 시맨틱 가정 | B | 놓지-않기 / lease+재시도 / unique 이름(tempdir) |

- "단독 green / 완주 red" 는 A·C 공통이라 그것만으로 안 갈린다 — **시간**을 본다: deadline 을
  정확히 소진하면 C, 즉시 실패면 A/B(그 다음 자원 종류로 A·B 를 가른다).
- 원인을 **후보 열거로 추정하지 말고 계측으로** 가른다 — 실패 지점에 상태 플래그(예: 자식
  사망 여부 · 이벤트 방출 여부)를 심어 그 조합이 어느 칸인지로 답을 낸다. 성공 회차·실패
  회차 **두 극에 다 있는** 로그·패닉은 판별력이 0 이다(ADR-0129 triage 규칙).

### 부하 재현과 CI(러너) 관측

로컬 재현: CPU 를 코어 수만큼 busy loop 로 포화시킨 뒤 대상 테스트를 30 회쯤 반복해
**재현률**(N/30)로 잰다. 단독 반복 green 과 대비하면 부하 축임이 드러난다. 고치기 전/후
재현률을 두 열로 낸다(예: before 8/30 → after 0/30).

- ★ **러너가 로컬보다 형태 C 를 더 잘 드러낸다**: 어떤 형태 C 는 로컬(예: 24-core Linux)에서
  8/30 인데 Windows 러너에서는 거의 매번 났다 — cmd spawn 이 sh 보다 무거워 임계를 쉽게 넘긴다.
  그러면 **그 러너의 연속 green 이 로컬 반복보다 강한 증거**다. 로컬에서 재현이 안 되는 형태 C
  도 있다(그 러너에서만 임계를 넘음) — 그때 판정 채널은 CI 하나뿐이다.

CI 잡에서 어느 스텝·테스트가 죽는지는 잡 초록/빨강이 아니라 스텝 단위로 본다:

```bash
gh run view <run_id> --json jobs \
  -q '.jobs[]|select(.name=="<job>")|.steps[]|"\(.number) \(.conclusion)\t\(.name)"'
gh api "repos/<owner>/<repo>/actions/jobs/<job_id>/logs" \
  | grep -E '\.\.\. FAILED|test result: FAILED\.'
```

함정:

- `gh run list` 는 워크플로 결론까지만 준다 — 스텝은 위 `--json jobs` 로.
- 로그에서 `grep … | head` 는 앞부분(대개 green)만 자른다 — `test result: FAILED` 나
  `… FAILED` 를 직접 노리거나 `tail` 을 쓴다.
- 스텝 번호가 건너뛰면(5 다음 10) 그 사이는 **안 돈 것**이다 — 미실행을 성공으로 세지 않는다.
- 오래된 run 은 로그가 만료돼 상세 회수가 안 된다 — 그때는 소스 정적 대조로 좁힌다.
- **빨간 잡은 그 아래를 가린다**: 앞 스텝이 죽으면 뒤 스텝은 안 돌고, 한 스텝 안에서도 상시
  red 가 그 밑의 간헐 flake 를 가린다(`--no-fail-fast` 라도 간헐분은 회차마다 다르게 뜬다).
  그래서 위 겹을 걷어내야 아래가 드러나며, **"초록 1 회"는 "고쳤다"가 아니라 "이번엔 안 났다"
  일 수 있다** — 근본 수정인지 간헐인지는 **연속 관측**과 소스(그 사이 그 파일이 실제로
  바뀌었나: `git log <base>..<tip> -- <path>`)로 가른다.

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
  AF_UNIX SEQPACKET socketpair 한 쌍. **이것은 우리 코드의 채널이 아니다** — 그래서
  "socketpair 한 번 = 자식 프로세스 spawn 한 번" 이라는 등식이 성립하고, 아래 명령이
  spawn 횟수를 그대로 센다.

### 몇 번 띄우는지는 이렇게 센다

```bash
cargo test --bin tasty --no-run                       # 테스트 바이너리 경로를 찍는다
strace -f -e trace=socketpair -o /tmp/sp.txt <그 경로>
grep -c 'socketpair(AF_UNIX' /tmp/sp.txt
```

수를 여기 적지 않는다([ADR-0139](../adr/0139-numbers-in-docs-are-classified-by-lineage-not-by-name.md))
— 시험이 늘면 같이 는다. 이 수의 성질만 적어 둘 값이 있다: **병렬도에 안 움직인다.**
`--test-threads` 를 바꿔도 호출 총수는 같다(바뀌는 것은 동시에 살아 있는 수뿐이다).
그래서 이 한 수는 "얼마나 많이 띄우는가" 만 재고 "얼마나 겹치는가" 에 오염되지 않는다.

#### 안 쟀다 — 그 총수의 **귀속**, 그리고 재려면 무엇이 필요한가

총수는 위 명령이 답하지만 **어느 시험이 그중 몇을 띄우는지**는 안 쟀다. 호출 스택이 필요한데
`strace -k` 는 이 환경의 빌드에 없다. 다만 계기를 바꾸지 않고도 갈 길이 있다 — 위 성질(총수가
병렬도에 안 움직인다)에서 **집합을 쪼개 각각 세면 합이 총수와 같다**가 따라온다. 그러니 필요한
것은 새 계기가 아니라 **분할**이다: 테스트 이름을 모듈 접두로 나눠 같은 명령을 부분집합마다
돌리고, 부분의 합이 총수와 맞는지로 분할이 샜는지 확인한다. 스택이 꼭 필요하면 libunwind 를
붙인 `strace -k` 빌드나 gdb `catch syscall` + backtrace 가 대안이다.

분할의 모양은 이렇다 — 필터 하나가 부분집합 하나이고, 부분마다 위 명령을 그대로 돌린다.

```bash
BIN=<위 --no-run 이 찍은 경로>
"$BIN" --list | sed -n 's/: test$//p' | cut -d: -f1 | sort -u   # 모듈 접두 목록
# 접두마다:
strace -f -e trace=socketpair -o /tmp/sp-<접두>.txt "$BIN" <접두>
grep -c 'socketpair(AF_UNIX' /tmp/sp-<접두>.txt
```

**합이 총수와 다르면 분할이 샌 것이다** — 접두가 겹치거나(같은 시험을 두 번 셌다) 어느 접두에도
안 걸린 시험이 있다. 그때는 귀속을 읽지 말고 분할부터 고친다.

#### 안 쟀다 — **동시에** 살아 있는 fd 의 곡선

위 총수와 **다른 물음**이다(저쪽은 "몇 번", 이쪽은 "몇 겹"). 병렬도를 올릴 때 동시 생존 fd 가
어디서 꺾이는지는 안 쟀다. 재려면 둘이 필요하다.

- `--test-threads` 를 1·2·4·8·… 로 올리며 같은 표본을 돌려 **최댓값 곡선**을 얻는다. 한 점만
  재면 꺾임이 안 보인다.
- 꺾이는 지점에서 fd 를 **종류별**(pts · socket · pipe)로 나눠 센다. 어느 자원이 상한을
  만드는지는 총수가 아니라 그 나눔이 답한다.

★ 곡선이 평평해 보이면 그것을 결론으로 쓰기 전에 계기부터 의심한다 — 폴링은 수명이 짧은 fd 를
놓친다. `strace -e trace=openat,close` 로 열림·닫힘을 시간축에 붙여 재구성하면 폴링이 놓친
것이 있는지 갈린다.

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
