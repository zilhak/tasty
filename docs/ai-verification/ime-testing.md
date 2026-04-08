# IME 시뮬레이션을 이용한 디버깅 가이드

## 개요

Tasty는 `surface.ime_*` IPC/CLI를 통해 IME 입력을 프로그래밍 방식으로 시뮬레이션할 수 있다. 이를 통해 AI 에이전트가 한글/CJK 입력 파이프라인의 버그를 직접 재현하고 검증할 수 있다.

## 기본 사용법

### 한글 입력 시뮬레이션

```bash
# 1. IME 활성화
tasty ime-enable

# 2. 조합 과정 시뮬레이션 (한 → 한글)
tasty ime-preedit "ㅎ"       # 초성
tasty ime-preedit "하"       # 초성 + 중성
tasty ime-preedit "한"       # 초성 + 중성 + 종성
tasty ime-commit "한"        # 확정 → PTY 전송

tasty ime-preedit "ㄱ"       # 다음 글자 초성
tasty ime-preedit "그"       # 초성 + 중성
tasty ime-preedit "글"       # 초성 + 중성 + 종성
tasty ime-commit "글"        # 확정

# 3. IME 비활성화
tasty ime-disable
```

### IPC(JSON-RPC)로 사용

```python
import socket, json, os, time

port = int(open(os.path.expanduser("~/.tasty/tasty.port")).read().strip())
s = socket.socket()
s.settimeout(5)
s.connect(('127.0.0.1', port))

def call(method, params=None):
    req = {"jsonrpc": "2.0", "id": 1, "method": method, "params": params or {}}
    s.sendall((json.dumps(req) + '\n').encode())
    return json.loads(s.recv(65536).decode())

# IME 시뮬레이션
call("surface.ime_enable")
call("surface.ime_preedit", {"text": "ㅎ"})
time.sleep(0.1)  # preedit 렌더링 확인용 대기
call("surface.ime_preedit", {"text": "한"})
time.sleep(0.1)
call("surface.ime_commit", {"text": "한"})

# 상태 확인
result = call("surface.ime_status")
print(result)  # {"active": true, "preedit_text": null, "has_preedit": false}

call("surface.ime_disable")
```

## 디버깅 시나리오별 가이드

### 1. Preedit 렌더링 위치 검증

**목적**: 조합 중 텍스트가 올바른 셀 위치에 렌더링되는지 확인

```bash
# 터미널에 텍스트를 먼저 입력
tasty send "echo hello\r"
sleep 0.5

# IME preedit 시작
tasty ime-enable
tasty ime-preedit "한"

# 스크린샷으로 위치 확인
# (IPC: ui.screenshot)
```

**확인 포인트:**
- preedit 오버레이가 현재 커서 위치에 정확히 표시되는가
- 파란색 배경이 셀 그리드와 정렬되는가

### 2. 연속 커밋 후 Preedit 위치 검증

**목적**: 여러 글자를 연속 입력할 때 preedit 위치가 올바르게 이동하는지 확인

```bash
tasty ime-enable

# 첫 번째 글자
tasty ime-preedit "한"
tasty ime-commit "한"

# 두 번째 글자 — preedit이 오른쪽으로 이동해야 함
tasty ime-preedit "글"

# 스크린샷으로 위치 확인
```

**주의**: 쉘 에코가 처리되기 전에 다음 preedit이 시작되면 커서 위치가 아직 갱신되지 않을 수 있다. `sleep 0.1` 정도 대기하면 더 정확하다.

### 3. IME 활성 시 ASCII 입력 검증

**목적**: IME 활성 상태에서 숫자/구두점이 올바르게 입력되는지 확인

```bash
tasty ime-enable

# ASCII 텍스트는 surface.send로 전송 (실제 동작: KeyboardInput에서 ASCII는 통과)
tasty send "123"

# 한글은 IME 경로로
tasty ime-preedit "한"
tasty ime-commit "한"

tasty ime-disable

# 결과 확인
tasty read-since-mark --strip-ansi
```

### 4. 분할 패널에서 Preedit 위치 검증

**목적**: 서피스가 분할 레이아웃의 오른쪽/아래에 있을 때도 preedit이 올바른 위치에 표시되는지 확인

```bash
# 분할 생성
tasty new split --level surface --direction horizontal

# 오른쪽 서피스에서 IME 시뮬레이션
tasty ime-enable
tasty ime-preedit "한"

# 스크린샷으로 preedit이 오른쪽 서피스의 커서 위치에 표시되는지 확인
```

### 5. Preedit 중 마우스 클릭 시 커밋 검증

이 시나리오는 IME 시뮬레이션만으로는 테스트할 수 없다 (마우스 클릭은 별도 경로). 하지만 preedit 상태를 설정한 후 결과를 확인할 수 있다:

```bash
tasty ime-enable
tasty ime-preedit "한"

# ime_status로 preedit 상태 확인
tasty ime-status
# → {"active": true, "preedit_text": "한", "has_preedit": true}
```

## 검증 체크리스트

IME 관련 코드를 수정한 후 다음을 확인한다:

- [ ] `tasty ime-enable` → `tasty ime-status`에서 `active: true` 확인
- [ ] `tasty ime-preedit "한"` → `tasty ime-status`에서 `preedit_text: "한"` 확인
- [ ] `tasty ime-preedit ""` → preedit 클리어 확인
- [ ] `tasty ime-commit "한"` → PTY에 "한" 전송 확인 (`read-since-mark`)
- [ ] `tasty ime-disable` → `active: false`, preedit 클리어 확인
- [ ] 연속 preedit → commit 반복 시 텍스트가 올바르게 누적되는지 확인
- [ ] 스크린샷에서 preedit 오버레이가 올바른 위치에 파란색 배경으로 표시되는지 확인

## 제한사항

- IME 시뮬레이션은 윈도우 레벨 상태(`TastyWindow.ime_active`, `ime_preedit`)를 직접 조작한다. OS의 실제 IME 엔진은 관여하지 않는다.
- `surface_id` 지정은 현재 미지원 — 항상 포커스된 서피스에 대해 동작한다.
- 마우스 이벤트(클릭 시 preedit 커밋)는 시뮬레이션할 수 없다.
- OS IME 후보창 위치(`set_ime_cursor_area`)는 호출되지만, 실제 OS IME가 열리지는 않는다.
