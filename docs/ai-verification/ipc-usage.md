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
