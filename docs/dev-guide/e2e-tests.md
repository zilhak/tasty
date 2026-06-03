# e2e tests — 격리 정책 + timeout 정책

`tests/e2e_tests.rs` 는 실 tasty 바이너리를 spawn 하여 IPC 로 조작하는
end-to-end 테스트다. `tests/common/mod.rs::TastyInstance::spawn` 가 공통 spawn
fixture 다. 본 문서는 spawn 의 환경 격리 + timeout 정책 + flaky 대응 절차를
기술한다.

## 1. 환경 격리

각 테스트마다 `$TMPDIR/tasty-test-home-{pid}-{nanos}/` 를 새 HOME 으로 만들고
다음 env 를 spawn 직전에 정리한다:

| env | 처리 | 이유 |
|-----|-----|-----|
| `HOME` | 격리 HOME 으로 override | tasty 의 `~/.tasty/`, db, plugin extract 위치 격리 |
| `ZDOTDIR` | 격리 HOME 으로 override | zsh rc 위치 격리 |
| `SHELL` | `env_remove` | host login shell 누수 차단 (`detect_bash` 의 `$SHELL` 경로 차단) |
| `OH_MY_ZSH`, `ZSH` | `env_remove` | oh-my-zsh customization 누수 차단 |
| `TASTY_SURFACE_ID` | `env_remove` | 부모가 tasty 안에서 실행 중일 때 `boot/cli_routing.rs:55` 의 augmented help 분기 차단 |
| `RUST_LOG` | `tasty=info` 로 override | child stderr 폭주로 인한 OS pipe backpressure 회피 |

또한 격리 HOME 에 다음 파일을 사전 작성한다:

| 파일 | 내용 | 이유 |
|-----|-----|-----|
| `.zshrc`, `.bashrc` | 빈 파일 | shell rc customization 차단 |
| `.tasty/config.toml` | `[general] shell="/bin/sh"` (POSIX) / Git Bash 경로 (Windows) + `restore_layout=false` 등 | `is_shell_valid()` 즉시 true → `detect_bash()` (host `/etc/passwd` 의존) 미호출 → `shell_setup_mode` 진입 차단 |

`shell_setup_mode` 에 진입하면 port file 이 영구히 작성되지 않아 spawn 이
timeout panic 한다. config.toml 사전 작성이 이 경로의 결정적 차단이다.

## 2. Timeout 정책

`spawn` 은 두 단계 timeout 을 갖는다:

| 단계 | 상수 | 값 | 조건 |
|-----|-----|---|-----|
| S1 | `SPAWN_PORT_TIMEOUT` | 30 s | `--port-file` 에 u16 port 가 쓰여짐 (`init_app_state` 후) |
| S2 | `SPAWN_SHELL_TIMEOUT` | 15 s | first surface 의 `screen_text` 가 non-empty (first PTY prompt) |

값 산정 근거:

- dev cold path worst-case: GPU init (cold Metal compile 200–2500 ms) +
  plugin discover/extract (50–500 ms) + theme `first_run_init` (30–500 ms) +
  db init (10–80 ms) + create_app_state (~150 ms) = **300–4000 ms** in
  release, **dev profile ~3.5x slower** per CLAUDE.md.
- self-hosted runner 변동 폭 (다른 cargo test job 동시 실행, macOS Metal driver
  busy 등) 흡수 마진 = 약 25 s.

단순 timeout 증가가 아닌, 결정적 fix (config.toml 사전 작성) 후의 *마진* 이다.

## 3. stderr tail 진단

spawn 이 timeout 으로 panic 할 경우 child stderr 의 마지막 30 라인을 panic
메시지에 첨부한다. 구현:

- `Stdio::piped()` + background drain thread + `Arc<Mutex<VecDeque<String>>>`
  (capacity 256). OS pipe buffer (Linux 64 KB / macOS 16 KB) 가 가득 차서
  child 가 write block 되는 것을 방지.
- `RUST_LOG=tasty=info` 로 verbosity cap. drain thread 가 1차 방어, verbosity
  cap 이 2차.

## 4. Flaky 발생 시 절차

1. panic 메시지의 `--- stderr (last 30 lines) ---` 섹션 확인
2. `tracing::info!` 로그에서 마지막 단계 식별:
   - `IPC server listening on 127.0.0.1:{port}` 가 보임 → port file 작성은 됐으나
     S2 (PTY prompt) timeout. shell path / rc 점검
   - 위 로그가 없음 → S1 timeout. config.toml 의 shell 경로 유효성, plugin 초기화,
     theme init 단계 점검
3. 동일 환경 재현: `cargo test --test e2e_tests -- --nocapture`
4. 결정적 차단이 깨졌으면 `TastyInstance::spawn` 의 env / config 추가 보강

## 5. 본 phase 범위 밖 (follow-up)

- `tests/gui_common/mod.rs` 대칭 적용 (gui_tests 는 전수 `#[ignore]` 라 일반
  `cargo test --workspace` 에서 실행되지 않음, 현재 flaky 발현 경로 아님)
- src 측 port file 작성 시점 이동 (plugin/theme init *전* 으로) — IPC handler 의
  pending queue 도입 필요
- 부팅 단계별 `tracing::info!(target="tasty_boot", phase=..., elapsed_ms=...)`
- Windows Git Bash 미설치 환경의 shell 결정 보강
