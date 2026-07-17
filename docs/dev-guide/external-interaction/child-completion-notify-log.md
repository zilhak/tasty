# child 완료 알림 — PTY tell 단일 의존의 함정과 completion-log 단일 채널화

## 증상

child(claude/codex)가 작업을 끝내 caller(conductor)에게 "완료" 를 알릴 때, caller
세션이 **busy(다른 turn 을 생성 중)** 이면 완료 알림이 **씹힌다**. conductor 가 무거운
작업(빌드/리뷰/직접 코딩)을 도는 동안 child 완료 알림이 도착해도 놓치는 사고가 실제로
있었다.

## 원인

완료 알림의 원래 유일 경로는 `terminal.tell`(claude `notify-done` / codex
`notify-caller`)로 caller surface 의 PTY 에 텍스트+`\r` 을 강제 주입하는 것이다. 이는
**사용자 타이핑을 흉내내는 입력 주입**이라, 수신 세션이 다른 turn 을 처리 중이면 그
입력이 프롬프트 버퍼로 흡수되거나 turn 경계에서만 소비되어, 즉시 새 turn 을 일으키지
못한다. idle 세션이라도 tell 텍스트가 프롬프트에 얹힐 뿐 자동 제출/turn 시작을 보장하지
못하는 경우가 있다.

## 처방 (현재 상태)

완료 이벤트를 **파일에 한 줄씩 append** 한다. conductor 는 이 파일을 Claude Code 내장
**Monitor tool** 로 tail 하면 busy/idle 여부와 무관하게 다음 turn 에 완료를 전달받는다 —
Monitor 가 뿜는 background-task notification 은 idle 세션도 깨우기 때문이다(상류 동작,
아래 근거).

원래는 이 append 와 함께 `terminal.tell` 도 발사했으나(즉시 눈에 보이는 fallback),
completion-log(Monitor) 채널이 안정적으로 검증된 뒤 **완료-알림 경로에서 `terminal.tell`
주입은 제거**했다. caller 가 Claude Code CLI 세션이면 주입된 텍스트가 **사람이 직접
타이핑해 제출한 발화와 구분되지 않는 형태**로 대화 트랜스크립트에 섞여 들어가는 부작용이
있었기 때문이다. 이제 completion-log 가 완료 알림의 **유일한 채널**이다. (일반 메시지
전달인 `tasty claude tell` / `tasty codex tell` 의 `terminal.tell` 은 완료 알림이 아니라
메시지 전달 그 자체이므로 그대로 유지된다.)

- **경로 규약**: `<parent_home>/notify/<caller_surface>.log`
  - `parent_home` = 호스트가 부팅 시 확정한 데이터 루트를 자식에 **`TASTY_PARENT_HOME`**
    env 로 내려준 값. 없으면 `tasty_home()`(= `~/.tasty` release / `~/.tasty-debug` debug /
    `TASTY_HOME` override)으로 fallback.
  - **한 머신에 `~/.tasty` 와 `~/.tasty-debug` 가 공존**할 수 있으므로, 어느 루트인지는
    호스트가 부팅 시 확정한 값을 **env 로 내려받아야만** 판별 가능하다(경로를 눈대중으로
    구성하면 안 됨).
  - plugin(writer)은 호스트가 주입한 `TASTY_PARENT_HOME` env 로 호스트와 **동일 루트**를
    본다 (`crates/tasty-host-plugin/src/process.rs` 가 plugin spawn 시 전파).
  - conductor(reader)가 떠 있는 터미널 PTY 도 동일하게 `TASTY_PARENT_HOME` 을 env 로 받는다
    (`crates/tasty-terminal/src/lib.rs` `Terminal::new` 가 `TASTY_SURFACE_ID` 와 함께
    주입). 따라서 conductor 는 **`$TASTY_PARENT_HOME` env 를 그대로 읽어**
    `$TASTY_PARENT_HOME/notify/$TASTY_SURFACE_ID.log` 로 경로를 구성한다 — writer/reader 가
    같은 루트를 보장받는다.
  - **왜 `TASTY_HOME` 이 아니라 `TASTY_PARENT_HOME` 인가**: `TASTY_HOME` 은
    `tasty_home()`(자기 데이터 루트 결정, override 전용)의 1순위 입력이다. 정보성 부모-루트
    값을 `TASTY_HOME` 으로 주입하면, **release 앱이 spawn 한 터미널 안에서 debug 빌드를
    실행**했을 때 그 debug 프로세스가 부모의 release 루트(`~/.tasty`)를 자기 데이터 루트
    override 로 오인한다. 그러면 `cfg!(debug_assertions)` 로 `~/.tasty-debug` 에 격리돼야 할
    debug 인스턴스가 release 의 `~/.tasty/tasty.port` 를 자기 포트로 덮어써, release 앱에
    붙어있던 모든 `tasty` CLI 가 통째로 연결 불가가 되는 사고가 실제로 발생했다
    (2026-07-14). self-determination(`TASTY_HOME`)과 정보성 broadcast(`TASTY_PARENT_HOME`)를
    **환경변수 이름으로 분리**해 이 오인을 원천 차단한다.
- **라인 포맷**: 완료 메시지 **한 줄** (예: `surface 42 작업 완료 (호출 방식: spawn)` /
  `surface 42 작업 완료 (호출 방식: tell)`). 과거엔 `spawn 완료: surface 42` 형태였으나,
  `command_name`(spawn/tell)이 문장 맨 앞에서 완료의 주어처럼 읽혀 "spawn 이라는 호출이
  접수/완료됐다"로 오독되기 쉬웠다 — 실제 의미는 "그 child 가 맡은 작업이 끝났다"인데
  conductor 가 이를 spawn 접수 확인 정도로 여기고 실제 완료 알림을 계속 무시하는 사고로
  이어졌다(2026-07-17). 이제 "작업 완료"를 앞세우고 호출 방식은 괄호로 분리한다
  (`notify_done_message`/`notify_caller_message`, `crates/tasty-plugin-{claude,codex}/src/handlers.rs`).
- **크기 관리**: append 전 파일이 256 KiB 이상이면 truncate 후 새로 쓴다(무한 성장 방어).
  `tail -F` 는 파일 축소를 감지해 재오픈하므로 arm 된 Monitor 는 truncate 후 라인을 계속
  받는다.
- 구현: `crates/tasty-utils/src/notify.rs`(공유 append 헬퍼) + 두 plugin 의
  `handle_notify_done`(claude) / `handle_notify_caller`(codex).

### conductor 운영 규약 — Monitor arm

conductor 는 child 를 dispatch 한 뒤 **한 번** 자기 surface 의 완료 로그를 arm 한다:

```
Monitor({ command: "tail -n0 -F \"$TASTY_PARENT_HOME/notify/$TASTY_SURFACE_ID.log\"", persistent: true })
```

`$TASTY_PARENT_HOME` 과 `$TASTY_SURFACE_ID` 는 conductor 터미널 env 에 이미 주입돼 있으므로
경로를 하드코딩하거나 debug/release 루트를 추측할 필요가 없다.

이후 child 완료마다 append 된 라인이 Monitor 이벤트로 전달된다. `-n0` 은 기존 라인을 건너뛰고
arm 시점 이후만 받게 한다. `persistent: true` 로 세션 내내 열려 있어 재-arm 이 필요 없다.

### Monitor 사용 시 주의 (상류 제약)

- **WebSocket 소스 금지**: Monitor 의 `ws` 소스는 loopback/사설 IP 를 거부하므로
  (`ws://127.0.0.1` 불가) tasty 완료 로그엔 쓸 수 없다. 반드시 `command`(파일 tail) 소스.
- Monitor tool 은 Amazon Bedrock / Google Cloud Agent Platform / Microsoft Foundry 에서
  미제공이고, `DISABLE_TELEMETRY` 또는 `CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC` 가
  설정되면 비활성이다. 이 경우 completion-log 는 여전히 append 되지만 자동 전달은 안 되므로
  수동 확인(로그 파일 직접 조회)에 의존한다 — `terminal.tell` fallback 은 위장 발화 부작용
  때문에 제거됐으므로 더는 대안 채널이 아니다.
- 간헐적 전달 지연(수십 초)이 보고돼 있다(아래 근거). 손실이 아니라 지연이다.

## 근거

- **tasty 측(append)**: 소스 확인 — `crates/tasty-utils/src/notify.rs`,
  `crates/tasty-plugin-{claude,codex}/src/handlers.rs`. 단위 테스트로 경로/포맷/truncate 검증.
- **Claude Code 측(Monitor 가 idle 세션을 깨움 / 채널이 idle 을 못 깨움)**: 상류
  `anthropics/claude-code` 이슈로 확인. background-task notification 이 idle 세션을 (오히려
  과하게) 깨운다: `#76331`. 반대로 MCP Channels(`notifications/claude/channel`) 는 idle
  세션 미wake 가 다수 OPEN 으로 남아(`#44380`/`#76330`/`#73381`) 완료 알림 주력으로 부적합 —
  그래서 채널이 아니라 **에이전트가 직접 arm 하는 Monitor** 방식을 택했다. Monitor 라인
  전달의 간헐 지연: `#76508`.

## 패키징된 macOS `.app` 의 PATH 제약 — hook 셸 커맨드가 자기 자신을 재호출할 때

완료 알림 hook(`register_notify_hooks`, `crates/tasty-plugin-{claude,codex}/src/handlers.rs`)이
등록하는 `command` 는 `tasty claude notify-done ...` / `tasty codex notify-caller ...` 형태로,
**`tasty` 자기 자신을 PATH 로 재호출**한다. 이 셸 커맨드는 `src/hook_handler/trigger.rs::spawn_shell`
이 `sh -c`(windows `cmd /C`)로 실행하며 **부모(host 앱) 프로세스의 환경을 상속**한다.

- **함정**: 패키징된 `.app` 을 macOS LaunchServices(Dock/Finder 더블클릭/`open Tasty.app`)로 띄우면
  host 프로세스의 PATH 가 `/usr/bin:/bin:/usr/sbin:/sbin` 로 제한된다. `tasty` 바이너리가 있는
  `.../Tasty.app/Contents/MacOS` 는 이 최소 PATH 에 없으므로, 상속받은 셸이 `tasty` 를 못 찾아
  `command not found`(exit 127)로 **조용히 실패**한다 — completion-log append 가 도착하지 않는다.
- **dev 에서 안 보이는 이유**: `cargo run` / 터미널에서 직접 띄운 바이너리는 그 터미널의 풍부한 PATH를
  상속하므로 재현되지 않는다. LaunchServices 로 띄운 `.app` 에서만 드러난다.
- **처방**: `spawn_shell` 이 자식 프로세스의 PATH 를 보강한다 — `std::env::current_exe()` 의 부모
  디렉토리(=실행 중인 `tasty` 바이너리가 있는 곳)를 PATH 맨 앞에 붙여, 자기 자신 재호출은 최소 PATH
  환경에서도 항상 해결된다. `current_exe()` 실패는 상속 PATH 그대로 두는 fallback(패닉 없음).
  이 보강은 `InlineShell`/`ShellCommand`(레지스트리) 양쪽이 공유하는 `spawn_shell` 한 곳에서 처리돼
  모든 hook 셸 커맨드에 적용된다. **스코프**: self-binary 디렉토리 하나만 추가하며, 로그인쉘(`$SHELL -lc`)
  이나 사용자 커스텀 PATH(nvm/rbenv/cargo bin 등)를 복제하지는 않는다.
- **공유 헬퍼**: 실제 PATH 계산은 `tasty_utils::process::path_prepending_self_dir` 하나로 통일돼,
  hook 셸(`spawn_shell`)과 PTY 셸(`crates/tasty-terminal/src/lib.rs::Terminal::new`, conductor 자신의
  인터랙티브 터미널이 `tasty` CLI 를 찾는 것도 이 경로 덕분)이 **동일 로직**을 쓴다. 구분자(`:`/`;`)는
  `std::env::{split_paths,join_paths}` 로 크로스플랫폼 처리.
- **회귀 방어**: `crates/tasty-utils/src/process.rs` 의 `prepends_self_binary_dir_to_minimal_path`
  (순수 함수, 최소 PATH prepend 검증) + `src/hook_handler/trigger.rs` 의
  `inline_shell_resolves_self_binary_via_augmented_path`(self-dir 없는 PATH 에서 basename 재호출 성공
  end-to-end 검증).

## 일반 교훈

- 수신 세션 상태(busy/idle)에 의존하는 PTY 입력 주입은 완료 알림의 **단일 경로로 부적합**하다.
  파일 append + 에이전트가 능동적으로 arm 하는 감시(Monitor tail)가 상태 독립적이다.
- writer(plugin)와 reader(conductor)가 같은 파일을 가리키려면 경로 SoT 를 하나로 통일하고,
  호스트가 **양쪽 프로세스 모두**에 부모 루트를 env 로 내려줘야 한다 — plugin spawn
  (`tasty-host-plugin`)뿐 아니라 conductor 가 사는 터미널 PTY spawn
  (`tasty-terminal` `Terminal::new`)에도. 한쪽만 전파하면 다른 쪽이 debug/release 루트를
  판별하지 못해 경로가 어긋난다.
- **정보성 broadcast 값은 self-determination 용 env(`TASTY_HOME`)와 이름을 겹치면 안 된다.**
  부모가 자기 루트를 자식에 알려주는 값을 `TASTY_HOME` 으로 주입하면, 그 자식이 다시 tasty
  바이너리(특히 다른 프로파일: release 안에서 debug)를 실행할 때 부모 루트를 자기 데이터
  루트 override 로 오인해 프로파일 격리가 깨진다. 그래서 broadcast 는 `TASTY_PARENT_HOME`
  으로 분리했다(위 "왜 `TASTY_HOME` 이 아니라" 참조).

## 날짜

2026-07-13 최초 작성.
