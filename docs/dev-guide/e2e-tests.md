# E2E 테스트 — 격리 + timeout 정책

`tests/e2e_tests.rs` 는 실 tasty 바이너리를 spawn 하여 IPC 로 조작하는 end-to-end 테스트다. `tests/common/mod.rs::TastyInstance::spawn` 이 공통 spawn fixture 다. 자체 검증 절차는 [self-verification.md](self-verification.md), debug 전용 IPC 는 [debug-ipc.md](debug-ipc.md).

## 0. 전제: plugin 바이너리 최신화 (필수)

**e2e 는 `cargo test --test e2e_tests` 단독 실행 전에 `cargo build --workspace` 가 선행돼야 한다** (또는 처음부터 `cargo test --workspace` 사용). package 한정 test 는 본체(tasty.exe)만 빌드하고 plugin bin crate 들을 빌드하지 않는데, dev bundle(`target/debug/builtin-plugins/`)은 **매니페스트는 소스에서, 바이너리는 target exe 에서 독립적으로** `copy_if_newer` 하므로 stale plugin exe 가 최신 매니페스트를 달고 격리 TASTY_HOME 에 설치된다. 이 drift 는 plugin↔host 계약이 바뀐 직후(예: `markdown.recent` 의 host adapter 이관) namespace 호출을 "Method not found" 로 깨뜨린다. 호스트는 hello 시 바이너리 보고 버전 ≠ 매니페스트 버전이면 `version drift` warn 을 남긴다 — spawn 실패 진단 시 stderr tail 에서 이 경고를 먼저 확인.

## 1. 환경 격리

각 테스트마다 `$TMPDIR/tasty-test-home-{pid}-{nanos}/` 를 새 HOME 으로 만들고, host 환경 누수를 spawn 직전에 차단한다:

| env | 처리 | 이유 |
|-----|------|------|
| `HOME` / `ZDOTDIR` | 격리 HOME override | zsh rc 위치 격리 (macOS/Linux 의 `~/.tasty/` 격리도 겸함) |
| `TASTY_HOME` | 격리 `.tasty` 로 명시 | tasty 루트 해석은 `directories::BaseDirs`(=Windows 는 USERPROFILE) 기반이라 **HOME 만으로는 Windows 에서 격리되지 않는다** — 실사용자 `~/.tasty-debug` 세션 복원이 새어든다. `TASTY_HOME` 이 루트 override 의 SoT |
| `SHELL` | 제거 | host login shell 누수 차단(`detect_bash` 의 `$SHELL` 경로) |
| `OH_MY_ZSH` / `ZSH` | 제거 | oh-my-zsh customization 누수 차단 |
| `TASTY_SURFACE_ID` | 제거 | 부모가 tasty 안일 때 augmented-help 분기 차단 |
| `RUST_LOG` | `tasty=info` | child stderr 폭주에 의한 OS pipe backpressure 회피 |

격리 HOME 에 사전 작성하는 파일:

| 파일 | 내용 | 이유 |
|------|------|------|
| `.zshrc` / `.bashrc` | 빈 파일 | shell rc customization 차단 |
| `.tasty/config.toml` | `shell="/bin/sh"`(POSIX) / Git Bash(Windows) + `restore_layout=false` | `is_shell_valid()` 즉시 true → `detect_bash()`(host `/etc/passwd` 의존) 미호출 → **shell_setup_mode 진입 차단** |

`shell_setup_mode` 에 진입하면 port file 이 영구히 안 써져 spawn 이 timeout panic 한다. config.toml 사전 작성이 이 경로의 *결정적* 차단이다.

## 2. Timeout (2단계)

| 단계 | 상수 | 값 | 조건 |
|------|------|-----|------|
| S1 | `SPAWN_PORT_TIMEOUT` | 30 s | `--port-file` 에 port 가 쓰여짐 |
| S2 | `SPAWN_SHELL_TIMEOUT` | 15 s | first surface `screen_text` 가 non-empty (첫 PTY prompt) |

값은 단순 증가가 아니라 **결정적 fix(config.toml 사전 작성) 위의 마진**이다 — dev cold path worst-case(GPU init + plugin discover/extract + theme/db init, dev 프로필 ~3.5× 느림) + self-hosted runner 변동 폭 흡수.

## 3. stderr tail 진단

spawn timeout panic 시 child stderr 마지막 30 라인을 panic 메시지에 첨부한다. `Stdio::piped()` + background drain thread + 링버퍼(capacity 256)로 OS pipe buffer(Linux 64KB / macOS 16KB)가 차서 child 가 write block 되는 것을 방지. `RUST_LOG=tasty=info` 로 verbosity 를 cap 한다(drain 1차 + cap 2차 방어).

## 4. Flaky 대응 절차

1. panic 의 `--- stderr (last 30 lines) ---` 확인.
2. 마지막 `tracing::info!` 단계 식별: `IPC server listening on 127.0.0.1:{port}` 가 보이면 **S2**(PTY prompt) timeout → shell path/rc 점검. 안 보이면 **S1** → config.toml shell 유효성·plugin·theme init 점검.
3. 재현: `cargo test --test e2e_tests -- --nocapture`.
4. 결정적 차단이 깨졌으면 `TastyInstance::spawn` 의 env/config 보강.

> `tests/gui_common/mod.rs`(gui_tests)는 전수 `#[ignore]` 라 일반 `cargo test --workspace` 에서 실행되지 않는다.

## VTE 시뮬레이터 (`tasty-tui-simulator`)

터미널 동작 검증용 도구 — 고수준 명령을 raw VTE escape 시퀀스로 변환해 출력한다(터미널 입장에선 실제 TUI 앱과 같은 바이트 스트림). **인터랙티브 모드**(stdin REPL — 외부에서 `surface.send` 로 명령 단계 전송, 명령마다 `OK` 동기화)와 원샷 시나리오를 제공한다. 명령: cursor/print/sgr/fg·bg/altscreen/scroll-region/erase/raw/esc 등, 종료 제어 `quit`/`exit-code N`/`crash`(SIGABRT)/`panic`. debug 의 `debug.cell_info`/`debug.screen_attrs`([debug-ipc](debug-ipc.md))와 조합하면 셀 속성을 결정적으로 자동 검증할 수 있다.

로직은 `lib.rs` 에 있고 두 진입점이 공유한다(SoT 하나) — 독립 바이너리 `tasty-tui-sim`(`cargo build -p tasty-tui-simulator`, release 빌드 가능) 과 `tasty debug sim <subcommand>`(debug 빌드 한정). **debug 빌드에선 별도 빌드/PATH 설정 없이** `tasty debug sim ...` 으로 바로 호출할 수 있다(이미 `tasty` 가 PATH 에 있으므로). surface 안에서 stdout 에 직접 VTE 를 뿜는 로컬 동작이라 IPC 를 거치지 않는다. 자세한 명령 목록·부하 모드(`flood`)는 [tui-testing](tui-testing.md).

## 관련

- [self-verification.md](self-verification.md) — 커밋 전 시나리오 재현
- [build.md](build.md) — dev/release/dist 프로필 (timeout 값 산정 근거)
