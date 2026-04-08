# Tasty 사용 가이드 — Linux

Tasty를 사용하는 AI 에이전트를 위한 Linux 환경 가이드.

## 파일 경로

| 파일 | 경로 | 설명 |
|------|------|------|
| 포트 파일 | `~/.tasty/tasty.port` | IPC 포트 번호. 실행 시 생성됨 |
| 설정 파일 | `~/.tasty/config.toml` | 사용자 설정 |

## 1. 실행 여부 확인

Tasty를 조작하기 전에 실행 중인지 확인한다.

```bash
pgrep -x tasty > /dev/null && echo "running" || echo "not running"
```

포트 파일이 있지만 프로세스가 없으면 stale 파일이므로 삭제:

```bash
if [ -f ~/.tasty/tasty.port ] && ! pgrep -x tasty > /dev/null; then
    rm ~/.tasty/tasty.port
fi
```

## 2. 실행 중인 경우 — 조작

### CLI

```bash
tasty list info     # 시스템 정보
tasty list tree     # 워크스페이스/패인/탭 구조
tasty list surfaces # 서피스 목록
tasty send "ls -la"
tasty send-key enter
```

### IPC (Python)

```python
import socket, json, os

port = int(open(os.path.expanduser("~/.tasty/tasty.port")).read().strip())
s = socket.socket()
s.settimeout(5)
s.connect(("127.0.0.1", port))

def call(method, params=None):
    req = {"jsonrpc": "2.0", "id": 1, "method": method, "params": params or {}}
    s.sendall((json.dumps(req) + "\n").encode())
    return json.loads(s.recv(65536).decode())

call("system.info")
```

## 3. 실행 중이 아닌 경우 — 실행

```bash
tasty &
while [ ! -f ~/.tasty/tasty.port ]; do sleep 0.2; done
```

## 4. 종료

```bash
# IPC
python3 -c "
import socket, json, os
port = int(open(os.path.expanduser('~/.tasty/tasty.port')).read().strip())
s = socket.socket(); s.settimeout(5); s.connect(('127.0.0.1', port))
req = {'jsonrpc': '2.0', 'id': 1, 'method': 'system.shutdown', 'params': {}}
s.sendall((json.dumps(req) + '\n').encode())
"
```

## 5. 스크린샷

GUI 모드에서만 동작.

```python
call("ui.screenshot", {"path": "/tmp/tasty-capture.png"})
```

결과는 PNG 형식.
