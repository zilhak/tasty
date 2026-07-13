# child 완료 알림 — PTY tell 단일 의존의 함정과 completion-log 대안

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

`terminal.tell` **은 그대로 유지**(즉시 눈에 보이는 fallback)하되, 완료 이벤트를 **파일에
한 줄씩 append 하는 대안 경로를 추가**한다. conductor 는 이 파일을 Claude Code 내장
**Monitor tool** 로 tail 하면 busy/idle 여부와 무관하게 다음 turn 에 완료를 전달받는다 —
Monitor 가 뿜는 background-task notification 은 idle 세션도 깨우기 때문이다(상류 동작,
아래 근거).

- **경로 규약**: `<tasty_home>/notify/<caller_surface>.log`
  - `tasty_home()` = `~/.tasty`(release) / `~/.tasty-debug`(debug) / `TASTY_HOME` env override.
  - **한 머신에 `~/.tasty` 와 `~/.tasty-debug` 가 공존**할 수 있으므로, 어느 루트인지는
    호스트가 부팅 시 확정한 값을 **env 로 내려받아야만** 판별 가능하다(경로를 눈대중으로
    구성하면 안 됨).
  - plugin(writer)은 호스트가 주입한 `TASTY_HOME` env 로 호스트와 **동일 루트**를 본다
    (`crates/tasty-host-plugin/src/process.rs` 가 plugin spawn 시 `TASTY_HOME` 전파).
  - conductor(reader)가 떠 있는 터미널 PTY 도 동일하게 `TASTY_HOME` 을 env 로 받는다
    (`crates/tasty-terminal/src/lib.rs` `Terminal::new` 가 `TASTY_SURFACE_ID` 와 함께
    `TASTY_HOME` 도 주입). 따라서 conductor 는 **`$TASTY_HOME` env 를 그대로 읽어**
    `$TASTY_HOME/notify/$TASTY_SURFACE_ID.log` 로 경로를 구성한다 — writer/reader 가
    같은 루트를 보장받는다.
- **라인 포맷**: caller 에게 tell 로 주입하던 메시지와 **동일 문자열** 한 줄
  (예: `spawn 완료: surface 42` / `tell 완료: surface 42`). divergence 방지.
- **크기 관리**: append 전 파일이 256 KiB 이상이면 truncate 후 새로 쓴다(무한 성장 방어).
  `tail -F` 는 파일 축소를 감지해 재오픈하므로 arm 된 Monitor 는 truncate 후 라인을 계속
  받는다.
- 구현: `crates/tasty-utils/src/notify.rs`(공유 append 헬퍼) + 두 plugin 의
  `handle_notify_done`(claude) / `handle_notify_caller`(codex).

### conductor 운영 규약 — Monitor arm

conductor 는 child 를 dispatch 한 뒤 **한 번** 자기 surface 의 완료 로그를 arm 한다:

```
Monitor({ command: "tail -n0 -F \"$TASTY_HOME/notify/$TASTY_SURFACE_ID.log\"", persistent: true })
```

`$TASTY_HOME` 과 `$TASTY_SURFACE_ID` 는 conductor 터미널 env 에 이미 주입돼 있으므로
경로를 하드코딩하거나 debug/release 루트를 추측할 필요가 없다.

이후 child 완료마다 append 된 라인이 Monitor 이벤트로 전달된다. `-n0` 은 기존 라인을 건너뛰고
arm 시점 이후만 받게 한다. `persistent: true` 로 세션 내내 열려 있어 재-arm 이 필요 없다.

### Monitor 사용 시 주의 (상류 제약)

- **WebSocket 소스 금지**: Monitor 의 `ws` 소스는 loopback/사설 IP 를 거부하므로
  (`ws://127.0.0.1` 불가) tasty 완료 로그엔 쓸 수 없다. 반드시 `command`(파일 tail) 소스.
- Monitor tool 은 Amazon Bedrock / Google Cloud Agent Platform / Microsoft Foundry 에서
  미제공이고, `DISABLE_TELEMETRY` 또는 `CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC` 가
  설정되면 비활성이다. 이 경우 completion-log 는 여전히 append 되지만 자동 전달은 안 되므로
  `terminal.tell` fallback 이나 수동 확인에 의존한다.
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

## 일반 교훈

- 수신 세션 상태(busy/idle)에 의존하는 PTY 입력 주입은 완료 알림의 **단일 경로로 부적합**하다.
  파일 append + 에이전트가 능동적으로 arm 하는 감시(Monitor tail)가 상태 독립적이다.
- writer(plugin)와 reader(conductor)가 같은 파일을 가리키려면 경로 SoT 를 `tasty_home()`
  하나로 통일하고, 호스트가 **양쪽 프로세스 모두**에 `TASTY_HOME` 을 env 로 내려줘야 한다 —
  plugin spawn(`tasty-host-plugin`)뿐 아니라 conductor 가 사는 터미널 PTY spawn
  (`tasty-terminal` `Terminal::new`)에도. 한쪽만 전파하면 다른 쪽이 debug/release 루트를
  판별하지 못해 경로가 어긋난다.

## 날짜

2026-07-13 최초 작성.
