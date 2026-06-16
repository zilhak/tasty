# Tasty IPC를 통한 조작

Tasty는 AI가 조작 가능한 터미널이다. 정상 모드(터미널이 뜬 상태)에서는 IPC 서버가 자동으로 뜨며, 포트 파일(`~/.tasty/tasty.port`)을 통해 접속할 수 있다. Claude Code 등 터미널 안에서 동작하는 AI도 IPC로 Tasty를 제어할 수 있다.

**IPC 조작 예시 (Python)**:
```python
import socket, json
port = int(open(os.path.expanduser("~/.tasty/tasty.port")).read().strip())
s = socket.socket()
s.connect(('127.0.0.1', port))
# 스크린샷
s.sendall((json.dumps({"jsonrpc":"2.0","id":1,"method":"ui.screenshot","params":{"path":"capture.png"}}) + '\n').encode())
# 키 입력
s.sendall((json.dumps({"jsonrpc":"2.0","id":2,"method":"surface.send_key","params":{"key":"ls\r"}}) + '\n').encode())
```

**CLI 조작 예시**:
```bash
tasty list workspaces               # 워크스페이스 목록
tasty send text "hello"             # 텍스트 전송
tasty send key "enter"              # 키 입력
tasty notify --title "Done"         # 알림
```

## `\r` (Enter) 전송 시 주의사항

`surface.send`에서 `\r`을 Enter(CR, 0x0D)로 보내려면 **전송 경로에 따라 처리가 다르다**. 이를 혼동하면 `\r`이 리터럴 백슬래시+r로 전송되어 셸 명령이 실행되지 않거나 화면에 `\r`이 그대로 표시된다.

### CLI에서: `\r`은 리터럴이다

셸(bash/zsh)의 큰따옴표 `"..."` 안에서 `\r`은 이스케이프되지 않는다. 리터럴 `\` + `r` 두 글자가 그대로 전달된다.

```bash
# ❌ 틀림 — 리터럴 \r이 전송됨
tasty send text "ls -la\r"

# ✅ 올바른 방법 1: send text + send key 분리
tasty send text "ls -la"
tasty send key enter

# ✅ 올바른 방법 2: $'...' ANSI-C 인용 사용
tasty send text $'ls -la\r'
```

### IPC (JSON-RPC)에서: `\r`은 CR이다

JSON 스펙에서 `\r`은 U+000D (Carriage Return)이다. Python `json.dumps()`가 자동으로 올바르게 인코딩한다.

```python
# ✅ JSON에서 \r은 실제 CR(0x0D)로 전송됨
call("surface.send", {"text": "ls -la\r"})
```

### 요약

| 전송 경로 | `"text\r"` 의미 | Enter 전송법 |
|-----------|----------------|-------------|
| CLI `tasty send text "..."` | 리터럴 `\` + `r` | `send text` + `send key enter` 또는 `$'...\r'` |
| Python/JSON-RPC | CR (0x0D) ✅ | `{"text": "...\r"}` |
| CLI `tasty send text $'...'` | CR (0x0D) ✅ | `$'text\r'` |

**AI 에이전트가 명령을 실행할 때**: Bash 도구에서 `tasty send text "cat\r"`을 쓰면 리터럴 `\r`이 전송된다. 반드시 `tasty send text "cat" && tasty send key enter` 또는 Python IPC를 사용할 것.

## 응답 읽기: `read_line` 만 쓸 것 (`read_to_end` 금지)

Tasty IPC 서버는 응답을 한 줄(`\n` 종료)로 보낸 뒤 **connection 을 즉시 닫지 않는다**. 즉 client 가 `read_to_end` / TCP EOF 대기로 읽으면 응답을 받았는데도 read timeout 만큼 더 기다린다 (Rust `set_read_timeout(Some(Duration::from_secs(10)))` 면 정확히 10초).

이 함정은 측정 도구 자체가 잘못된 지연 패턴을 만들어내기 때문에 매우 위험하다. "모든 IPC 호출이 정확히 10초씩 걸린다" → "백그라운드 throttling 이다" 같은 잘못된 가설로 시간이 낭비된다.

### Rust

```rust
// ❌ 금지 — server 가 close 할 때까지 read timeout 만큼 대기
let mut buf = Vec::new();
stream.read_to_end(&mut buf).ok();

// ✅ 응답 한 줄만 읽는다
let mut reader = BufReader::new(&stream);
let mut line = String::new();
reader.read_line(&mut line).ok();
```

### Python

```python
# ✅ 줄 단위로 읽는다
f = s.makefile('rb')
line = f.readline()
resp = json.loads(line)
```

`tests/common/mod.rs::call()` 가 표준 구현. 새 테스트나 디버그 repro 도구를 만들 때 이것을 그대로 따른다.
