# 22. Surface Hook

## 개요

Surface별 이벤트 훅을 등록하여 에이전트 자동화를 가능하게 하는 시스템. 특정 이벤트 발생 시 사전 등록된 명령을 자동 실행한다.

## 이벤트 훅 API

### 지원 이벤트

| 이벤트 | 설명 |
|--------|------|
| `process-exit` | Surface의 자식 프로세스가 종료될 때 |

### 향후 확장 가능 이벤트

| 이벤트 | 설명 |
|--------|------|
| `output-match` | PTY 출력이 지정된 패턴(정규식)과 매칭될 때 |
| `idle-timeout` | 지정 시간 동안 PTY 출력이 없을 때 |
| `notification` | OSC 알림 시퀀스 수신 시 |

## CLI 인터페이스

```
tasty set hook --surface <surface_id> --event <event> --command <command>
tasty list hooks [--surface surface_id]
tasty unset-hook --hook <hook_id>
```

### 사용 예시

```bash
# Surface 3의 프로세스가 종료되면 알림 전송
tasty set hook --surface 3 --event process-exit --command "tasty notify 'Build finished'"

# Surface 5의 출력에서 "error" 패턴 감지 시 명령 실행 (향후)
tasty set hook --surface 5 --event "output-match:error" --command "tasty notify 'Error detected'"

# 등록된 훅 목록 확인
tasty list hooks
tasty list hooks --surface 3

# 훅 제거
tasty unset-hook --hook 42
```

## 소켓 API

| 메서드 | 파라미터 | 설명 |
|--------|----------|------|
| `surface.set_hook` | `surface_id`, `event`, `command` | 훅 등록 |
| `surface.list_hooks` | `surface_id?` | 훅 목록 조회 |
| `surface.unset_hook` | `hook_id` | 훅 제거 |

### 요청 예시

```json
{
  "method": "surface.set_hook",
  "params": {
    "surface_id": 3,
    "event": "process-exit",
    "command": "tasty notify 'Done'"
  }
}
```

### 응답 예시

```json
{
  "hook_id": 42,
  "surface_id": 3,
  "event": "process-exit",
  "command": "tasty notify 'Done'"
}
```

## 내부 구현

- `HookRegistry`: Surface ID + 이벤트 타입으로 인덱싱된 훅 저장소
- 이벤트 발생 시 매칭되는 훅을 찾아 `command`를 셸에서 실행
- 훅 실행은 비동기로 처리하여 메인 루프를 차단하지 않는다
- `output-match` 이벤트는 PTY 리더 스레드에서 패턴 매칭을 수행한다

## 활용 시나리오

에이전트가 다른 터미널의 프로세스 종료를 감지하여 후속 작업을 자동으로 실행한다:

1. 에이전트 A가 Surface 3에서 빌드 시작
2. `tasty set hook --surface 3 --event process-exit --command "tasty send-keys 5 'deploy.sh\n'"` 등록
3. 빌드 완료(프로세스 종료) 시 자동으로 Surface 5에 배포 명령 전송

## 구현 가능 여부

| OS | 가능 여부 | 비고 |
|----|-----------|------|
| Windows | 가능 | |
| macOS | 가능 | |
| Linux | 가능 | |
