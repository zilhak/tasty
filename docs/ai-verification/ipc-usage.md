# IPC 로 tasty 조작·검증

Tasty가 실행되면 로컬 IPC 서버의 포트 파일로 접속할 수 있다. 기본 release 포트 파일은 `~/.tasty/tasty.port`이며, 전체 메서드는 [API 문서](../reference/api.md)에 있다.

```python
import socket, json, os
port = int(open(os.path.expanduser("~/.tasty/tasty.port")).read().strip())
s = socket.socket(); s.connect(('127.0.0.1', port))
def call(method, params=None):
    s.sendall((json.dumps({"jsonrpc":"2.0","id":1,"method":method,"params":params or {}})+'\n').encode())
    return json.loads(s.makefile('rb').readline())
```

```bash
tasty list workspaces
tasty send text "hello" && tasty send key enter
```

## 함정 1 — `\r`(Enter) 전송

| 전송 경로 | `"text\r"` 의미 | Enter 전송법 |
|-----------|----------------|-------------|
| CLI `tasty send text "..."` | CR (0x0D) ✅ — CLI 가 `\r` `\n` `\t` `\\` `\0` 을 해석한다 | `"text\r"` |
| Python/JSON-RPC | CR (0x0D) ✅ | `{"text": "...\r"}` |
| CLI `tasty send text $'...'` | CR (0x0D) ✅ | `$'text\r'` |

**Bash 도구에서 `tasty send text "cat\r"` 는 CR 을 보낸다** — CLI 가 전송 전에 C 식 이스케이프를 풀기 때문이다(`crates/tasty-cli/src/request.rs` 의 `unescape`).

## 함정 2 — 응답은 `read_line` 으로 (`read_to_end` 금지)

IPC 응답은 개행으로 끝나는 JSON 한 줄이다. 서버가 바로 연결을 닫지는 않으므로 `read_line`으로 읽는다. `read_to_end`로 EOF를 기다리면 응답을 받은 뒤에도 설정한 read timeout까지 멈출 수 있다. 예를 들어 timeout이 10초면 그만큼 기다리게 되며, 이를 서버 처리 지연이나 throttling으로 오해하지 않는다.

```rust
// ❌ server close 까지 read timeout 만큼 대기
stream.read_to_end(&mut buf).ok();
// ✅ 한 줄만
BufReader::new(&stream).read_line(&mut line).ok();
```

```python
line = s.makefile('rb').readline()   # ✅ 줄 단위
```

`tests/common/mod.rs::call()` 가 표준 구현 — 새 테스트/디버그 repro 도구는 이것을 따른다.

## 대화형 작업 수행 — 도구 한계 우회

에이전트는 전용 Tasty surface에서 대화형 프로그램을 실행하고 화면을 읽으며 입력을 보낼 수 있다.

### 핵심 명제

Tasty surface에는 실제 PTY(Windows에서는 ConPTY)가 연결된다. `send`, `read screen`, 상태 확인을 반복하면 실행 중인 프로그램과 대화할 수 있다.

### 왜 되는가 — 두 장벽을 실 PTY 가 둘 다 뚫는다

파이프로 출력만 받는 실행 도구는 프로그램 실행 중에 추가 입력을 보내기 어렵고, OpenSSH처럼 `/dev/tty`에서 비밀번호를 읽는 프로그램에는 제어 터미널이 필요하다.

Tasty에서는 `send`와 `read screen`을 별도 요청으로 반복하고, 프로그램은 PTY로 전달된 입력을 읽는다. 한 번의 도구 호출에서 모든 대화를 끝낼 필요는 없다.

### 적용 예

ssh 비밀번호/`sudo` 암호/OTP 프롬프트, 대화형 REPL, 설치 마법사, `git rebase -i` 같은 풀스크린 에디터 등 — 파이프 도구로는 손댈 수 없던 대화형 절차를 surface 안에서 진행할 수 있다.

> SSH 연결은 [원격 attach](../features/remote-attach/index.md)와 [로컬 신뢰 정책](../adr/0011-secrets-and-local-trust.md)에 따라 키 인증을 사용한다. PTY가 비밀번호 프롬프트를 처리할 수 있다는 설명이 비밀번호 자동화를 권장하는 것은 아니다.

### 한계

- **실시간이 아니다(폴링)** — `send` 와 `read` 사이에 프로그램이 아직 프롬프트를 안 띄웠을 수 있다. `send` 전후로 `read screen` 해 상태를 확인하고, 화면 상태와 보낸 키가 어긋나는 desync 를 경계한다.
- **ghost-suggestion 혼동 주의** — Claude Code CLI 등은 빈 입력창에 아직 아무도 타이핑하지 않은 자동완성 제안(dim 텍스트)을 그려둔다. `read screen`(`surface.screen_text`/`pty.read`)은 이런 dim 셀을 **기본 제외**해 실제 입력된 텍스트만 반환하므로, child 의 입력 버퍼 상태를 오독할 위험이 낮다. 제안 텍스트 자체를 보고 싶을 때만 `--show-dim`.
- **비밀번호는 에코되지 않는다** — 비번 입력은 surface 에 표시되지 않으므로 입력이 들어갔는지 화면으로 확인할 수 없다. 다음 화면 상태(프롬프트 통과/재요청)로만 역추론한다.
- **자격증명을 만들어내지는 못한다** — 능력 ≠ 비밀 우회. 모르는 비밀번호/OTP 를 surface 가 대신 알아내 주지 않는다.

### 제약 (불가침 — 반드시 지킨다)

대화형 작업도 "에이전트 행동"이므로 [사용자 행동 ↔ 에이전트 행동 분리](../identity.md) 원칙([프로젝트 CLAUDE.md §1](../../CLAUDE.md))의 적용을 받는다.

- **사용자 활성 쉘 금지** — `foreground_process` 가 사용자 작업이거나 `busy:true` 인 surface 는 건드리지 않는다.
- **전용 테스트 surface 에만** — `tasty new tab --pane <pane_id>` 로 만든 surface 에서만 입력 주입을 한다.
- **끝나면 정리** — 작업이 끝난 테스트 surface 는 닫는다.

> `send`/`read` 는 release 정식 명령이다(debug 전용이 아니다). 입력 *시뮬레이션* 류의 `debug.*` 명령만 `#[cfg(debug_assertions)]` 게이트로 debug 빌드에 한정된다 — 혼동하지 않는다.
