# 23. Read Mark (Delta Tracking) API

## 개요

터미널 출력에 마크를 설정하고, 마크 이후의 새 출력만 효율적으로 읽는 API. 에이전트가 다른 터미널에 명령을 보내고 결과만 가져올 때 유용하다.

## CLI 인터페이스

```
tasty set mark [--surface surface_id]
tasty read mark [--surface surface_id] [--strip-ansi]
```

### 사용 예시

```bash
# 현재 출력 위치에 마크 설정
tasty set mark --surface 3

# Surface 3에 명령 전송
tasty send text --surface 3 "ls -la\n"

# 마크 이후 새 출력만 읽기
tasty read mark --surface 3

# ANSI 이스케이프 시퀀스 제거 후 읽기
tasty read mark --surface 3 --strip-ansi
```

## 소켓 API

| 메서드 | 파라미터 | 설명 |
|--------|----------|------|
| `surface.set_mark` | `surface_id` | 현재 출력 위치에 마크 설정 |
| `surface.read_since_mark` | `surface_id`, `strip_ansi?` | 마크 이후 새 출력 반환 |

### 요청 예시

```json
{
  "method": "surface.set_mark",
  "params": { "surface_id": 3 }
}
```

```json
{
  "method": "surface.read_since_mark",
  "params": {
    "surface_id": 3,
    "strip_ansi": true
  }
}
```

### 응답 예시

```json
{
  "output": "total 42\ndrwxr-xr-x  5 user group 160 Mar 20 10:00 .\n..."
}
```

## 내부 구현

- 각 Surface에 `mark_offset: Option<usize>` 필드 추가
- `set_mark` 호출 시 현재 스크롤백 버퍼의 오프셋(바이트 위치)을 저장
- `read_since_mark` 호출 시 저장된 오프셋부터 현재 끝까지의 버퍼 내용 반환
- 마크가 설정되지 않은 상태에서 `read_since_mark` 호출 시 전체 출력 반환

### 버퍼 관리

- PTY 리더 스레드에서 수신한 원시 바이트를 별도 링 버퍼에 축적
- 링 버퍼 최대 크기: 설정 가능 (기본 1MB)
- 마크 오프셋이 버퍼에서 이미 밀려난 경우 에러 반환

### ANSI 이스케이프 제거

- `--strip-ansi` 옵션 사용 시 VT 제어 시퀀스를 제거한 플레인 텍스트 반환
- CSI, OSC, ESC 시퀀스 모두 제거
- 가시 텍스트와 개행만 남긴다

## 활용 시나리오

에이전트가 다른 터미널에 명령을 보내고 결과만 효율적으로 읽는다:

1. `tasty set mark --surface 3` — 현재 위치 기록
2. `tasty send text --surface 3 "git status\n"` — 명령 전송
3. 잠시 대기 (또는 idle-timeout 훅 활용)
4. `tasty read mark --surface 3 --strip-ansi` — 명령 결과만 가져오기

기존 `tasty read` (전체 출력)와 달리, 델타 트래킹으로 새 내용만 효율적으로 추출한다.

## 구현 가능 여부

| OS | 가능 여부 | 비고 |
|----|-----------|------|
| Windows | 가능 | |
| macOS | 가능 | |
| Linux | 가능 | |
