# Tasty 에이전트 가이드 — Linux

## 바이너리 경로

개발 환경에서 빌드한 바이너리:

```
/home/zilhak/workspace/tasty/target/debug/tasty      # 디버그 빌드
/home/zilhak/workspace/tasty/target/release/tasty     # 릴리스 빌드
```

`tasty`는 기본적으로 PATH에 등록되어 있지 않다. 직접 경로를 사용하거나 `cargo run`으로 실행한다.

## 설정/런타임 파일 경로

| 파일 | 경로 | 설명 |
|------|------|------|
| 포트 파일 | `~/.tasty/tasty.port` | IPC 포트 번호. 실행 시 생성됨 |
| 설정 파일 | `~/.tasty/config.toml` | 사용자 설정 |
| 번역 오버라이드 | `~/.tasty/lang/{code}.toml` | 커스텀 번역 |

## 1. Tasty 실행 여부 확인

Tasty를 조작하기 전에 **반드시** 실행 중인지 확인한다. 두 가지 방법이 있다.

### 방법 1: 프로세스 확인 (권장)

```bash
pgrep -x tasty > /dev/null && echo "running" || echo "not running"
```

### 방법 2: 포트 파일 확인

```bash
cat ~/.tasty/tasty.port 2>/dev/null
```

**주의**: 포트 파일은 Tasty가 `kill`(SIGTERM/SIGKILL)로 강제 종료되면 삭제되지 않는다. 따라서 포트 파일이 존재해도 프로세스가 실제로 살아있는지 `pgrep`으로 반드시 교차 확인해야 한다.

```bash
# 안전한 확인: 포트 파일 있지만 프로세스 없으면 stale 포트 파일 정리
if [ -f ~/.tasty/tasty.port ] && ! pgrep -x tasty > /dev/null; then
    rm ~/.tasty/tasty.port
    echo "stale port file removed"
fi
```

## 2. 실행 중인 경우 — 기존 인스턴스 조작

### CLI

```bash
TASTY=/home/zilhak/workspace/tasty/target/debug/tasty

$TASTY info          # 시스템 정보
$TASTY tree          # 워크스페이스/패인/탭 구조
$TASTY surfaces      # 서피스 목록
$TASTY send "ls -la"
$TASTY send-key enter
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

# 예시
call("system.info")
# 응답: {"jsonrpc": "2.0", "result": {"active_workspace": 0, "version": "0.1.0", "workspace_count": 1}, "id": 1}
```

## 3. 실행 중이 아닌 경우 — 직접 실행

### GUI 모드

```bash
/home/zilhak/workspace/tasty/target/debug/tasty &

# 포트 파일 생성 대기 (IPC 서버 기동 완료 시점)
while [ ! -f ~/.tasty/tasty.port ]; do sleep 0.2; done

# 이후 CLI/IPC 조작 가능
```

### 헤드리스 모드 (GUI 없이 IPC만)

디스플레이가 없거나 CI/테스트 환경에서 사용한다.

```bash
/home/zilhak/workspace/tasty/target/debug/tasty --headless &

while [ ! -f ~/.tasty/tasty.port ]; do sleep 0.2; done
```

### 다중 인스턴스

`--port-file` 옵션으로 포트 파일을 분리하면 여러 인스턴스를 동시에 실행할 수 있다.

```bash
# 인스턴스 A
/home/zilhak/workspace/tasty/target/debug/tasty --port-file /tmp/tasty-a.port &

# 인스턴스 B
/home/zilhak/workspace/tasty/target/debug/tasty --port-file /tmp/tasty-b.port &
```

다중 인스턴스에 접속할 때는 CLI가 아닌 IPC(Python)로 포트 파일을 직접 읽어 연결해야 한다. CLI는 기본 포트 파일(`~/.tasty/tasty.port`)만 참조한다.

```python
port = int(open("/tmp/tasty-a.port").read().strip())
s.connect(("127.0.0.1", port))
```

## 4. 종료

```bash
# 방법 1: IPC
python3 -c "
import socket, json, os
port = int(open(os.path.expanduser('~/.tasty/tasty.port')).read().strip())
s = socket.socket(); s.settimeout(5); s.connect(('127.0.0.1', port))
req = {'jsonrpc': '2.0', 'id': 1, 'method': 'system.shutdown', 'params': {}}
s.sendall((json.dumps(req) + '\n').encode())
"

# 방법 2: kill (포트 파일이 남으므로 수동 삭제 필요)
kill $(pgrep -x tasty)
rm -f ~/.tasty/tasty.port
```

## 5. 스크린샷

GUI 모드에서만 동작한다. 헤드리스 모드에서는 사용 불가.

### IPC

```python
call("ui.screenshot", {"path": "/tmp/tasty-capture.png"})
```

결과는 PNG 형식으로 저장된다.

## 6. 빌드 후 재시작

Linux에서는 실행 중인 바이너리를 `cargo build`로 교체해도 실행 중인 프로세스에 영향이 없다 (inode 기반 참조).

1. `cargo build` — 새 바이너리 생성
2. 실행 중인 인스턴스는 계속 동작 (이전 바이너리)
3. 다음 실행 시 새 바이너리 사용

## 7. 워크플로우 요약

```
1. pgrep -x tasty 로 프로세스 확인
   |
   +-- 실행 중 --> CLI/IPC로 조작
   |
   +-- 미실행 --> 포트 파일 잔존 확인
                  |
                  +-- 있으면 삭제 (stale)
                  |
                  +-- tasty 실행 (GUI 또는 --headless)
                      --> 포트 파일 생성 대기
                      --> CLI/IPC로 조작
```
