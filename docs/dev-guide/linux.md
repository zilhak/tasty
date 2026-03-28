# 개발 가이드 — Linux

이 프로젝트를 개발하는 AI 에이전트를 위한 Linux 환경 가이드.

## 바이너리 경로

```
/home/zilhak/workspace/tasty/target/debug/tasty      # 디버그 빌드
/home/zilhak/workspace/tasty/target/release/tasty     # 릴리스 빌드
```

`tasty`는 PATH에 등록되어 있지 않다. 직접 경로를 사용하거나 `cargo run`으로 실행한다.

## Tasty 실행/조작

### 실행 여부 확인

```bash
pgrep -x tasty > /dev/null && echo "running" || echo "not running"
```

포트 파일이 남아있지만 프로세스가 없는 경우(stale):

```bash
if [ -f ~/.tasty/tasty.port ] && ! pgrep -x tasty > /dev/null; then
    rm ~/.tasty/tasty.port
    echo "stale port file removed"
fi
```

### 실행

```bash
# GUI
/home/zilhak/workspace/tasty/target/debug/tasty &
while [ ! -f ~/.tasty/tasty.port ]; do sleep 0.2; done

# 헤드리스 (GUI 없이 IPC만)
/home/zilhak/workspace/tasty/target/debug/tasty --headless &
while [ ! -f ~/.tasty/tasty.port ]; do sleep 0.2; done

# 다중 인스턴스
/home/zilhak/workspace/tasty/target/debug/tasty --port-file /tmp/tasty-a.port &
```

### 종료

```bash
# IPC
python3 -c "
import socket, json, os
port = int(open(os.path.expanduser('~/.tasty/tasty.port')).read().strip())
s = socket.socket(); s.settimeout(5); s.connect(('127.0.0.1', port))
req = {'jsonrpc': '2.0', 'id': 1, 'method': 'system.shutdown', 'params': {}}
s.sendall((json.dumps(req) + '\n').encode())
"

# kill (포트 파일 수동 삭제 필요)
kill $(pgrep -x tasty)
rm -f ~/.tasty/tasty.port
```

## 빌드 후 재시작

Linux에서는 실행 중인 바이너리를 `cargo build`로 교체해도 실행 중인 프로세스에 영향이 없다 (inode 기반 참조).

1. `cargo build` — 새 바이너리 생성
2. 실행 중인 인스턴스는 계속 동작 (이전 바이너리)
3. 다음 실행 시 새 바이너리 사용

## 스크린샷

GUI 모드에서만 동작. 헤드리스에서는 사용 불가.

```python
call("ui.screenshot", {"path": "/tmp/tasty-capture.png"})
```

결과는 PNG 형식으로 저장된다.

### hover/애니메이션 등 상태 의존적 UI 확인

스크린샷은 정적 캡처이므로 마우스 호버 등 상태 의존적 스타일은 직접 확인할 수 없다. 이런 경우:

1. 코드에서 조건(`if response.hovered()` 등)을 임시로 제거하여 해당 스타일이 항상 적용되도록 변경
2. 빌드 → 재시작 → 스크린샷 촬영
3. 확인 후 코드를 원래대로 복원
4. 최종 빌드
