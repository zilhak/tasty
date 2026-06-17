# 환경 노트 (에이전트 부트스트랩)

AI 에이전트가 tasty 를 조작하기 전 알아야 할 OS별 경로·실행 확인·접속 패턴. 메서드 표는 [api.md](api.md).

## 경로 (모든 OS)

| 파일 | 경로 | 설명 |
|------|------|------|
| 포트 파일 | `~/.tasty/tasty.port` | IPC 동적 포트. 실행 시 생성 |
| 설정 | `~/.tasty/config.toml` | 사용자 설정 |

(Windows 는 `~` = `%USERPROFILE%`. debug 빌드 포트 파일은 `~/.tasty/tasty-debug.port`.)

## 실행 여부 확인

```bash
pgrep -x tasty >/dev/null && echo running || echo "not running"
# 포트 파일은 있으나 프로세스가 없으면 stale — 삭제
[ -f ~/.tasty/tasty.port ] && ! pgrep -x tasty >/dev/null && rm ~/.tasty/tasty.port
```

## 실행 / 대기 / 종료

```bash
tasty &
until tasty list info 2>/dev/null; do sleep 0.2; done    # 포트 뜰 때까지 (sleep 루프 대신 조건검사)
tasty list tree            # 구조 (ID 포함)
tasty send text "ls -la\r" --surface <id>
# 종료: system.shutdown IPC (또는 tasty 명령)
```

## IPC 직접 (Python)

```python
import socket, json, os
port = int(open(os.path.expanduser("~/.tasty/tasty.port")).read().strip())
s = socket.socket(); s.settimeout(5); s.connect(("127.0.0.1", port))
def call(m, p=None):
    s.sendall((json.dumps({"jsonrpc":"2.0","id":1,"method":m,"params":p or {}}) + "\n").encode())
    return json.loads(s.recv(1<<16).decode())
call("system.info")
```

## 스크린샷

`ui.screenshot {path}` 는 **GUI 모드 + debug 빌드** 에서만 동작한다(release 미노출, [debug-ipc](../dev-guide/debug-ipc.md)). 결과 PNG.

## 관련

- [api.md](api.md) — 전체 IPC/CLI · [dev-guide/self-verification](../dev-guide/self-verification.md) — 검증 시나리오
