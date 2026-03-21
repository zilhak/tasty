# 07. 소켓 API

## cmux 구현 방식

- Unix domain socket
- v1 텍스트 프로토콜 (공백 구분)
- v2 JSON-RPC 프로토콜 (핸들 기반)
- 접근 모드: off / cmuxOnly / automation / password / allowAll
- 카테고리: window, workspace, surface, pane, notification, browser, system

## 크로스 플랫폼 구현 방안

### 프로토콜 설계

JSON-RPC 2.0 단일 프로토콜로 시작 (v1 레거시 불필요):

```json
{
  "jsonrpc": "2.0",
  "method": "workspace.create",
  "params": { "name": "agent-1", "command": "claude" },
  "id": 1
}
```

### IPC 전송 계층

| OS | 전송 방식 | 경로 |
|----|----------|------|
| **Linux** | Unix domain socket | `$XDG_RUNTIME_DIR/tasty.sock` 또는 `/tmp/tasty-{uid}.sock` |
| **macOS** | Unix domain socket | `$TMPDIR/tasty.sock` |
| **Windows** | Named pipe | `\\.\pipe\tasty-{session}` |

추상화:

```rust
trait IpcTransport {
    async fn listen(&self) -> impl Stream<Item = Connection>;
    async fn connect(&self) -> Connection;
}
```

### 인증

| 모드 | 설명 |
|------|------|
| off | 비활성화 |
| local | 같은 사용자만 (파일 퍼미션 기반, Windows는 ACL) |
| password | 토큰 기반 인증 |
| open | 모든 접근 허용 |

### 주요 메서드

네이티브 GUI 앱이므로 윈도우, 레이아웃, 외형 등 풍부한 제어가 가능하다.

| 카테고리 | 메서드 |
|----------|--------|
| window | list, create, close, focus, resize, move, fullscreen |
| workspace | list, create, select, close, rename, reorder |
| pane | list, focus, split, close, send, send_key, zoom |
| notification | list, create, clear, mark_read |
| layout | get, set, save, restore |
| appearance | theme, font_size, opacity |
| system | info, identify, version, screenshot |

### 이벤트 스트리밍

클라이언트가 이벤트를 구독할 수 있는 notification 채널:

```json
{
  "jsonrpc": "2.0",
  "method": "event.subscribe",
  "params": { "events": ["workspace.changed", "pane.output", "notification.new"] },
  "id": 2
}
```

GUI 앱이므로 윈도우 이벤트, 포커스 변경 등 더 많은 이벤트를 제공할 수 있다.

## 최적화 전략

- **메시지 배칭**: 다수의 API 호출을 하나의 배치 요청으로 처리한다. JSON-RPC의 배치 프로토콜을 활용하여 왕복 횟수를 줄인다.
- **이벤트 스트리밍 필터링**: 클라이언트가 관심 있는 이벤트만 구독하여 불필요한 직렬화/전송을 방지한다. 구독 시 이벤트 타입 필터를 지정한다.
- **연결 수 제한**: 동시 소켓 연결 수를 제한(예: 최대 32개)하여 리소스 누수를 방지한다. 연결 수 초과 시 가장 오래된 유휴 연결을 끊는다.
- **JSON 직렬화 최적화**: `simd-json` 또는 `serde`의 zero-copy 디시리얼라이제이션을 활용하여 직렬화/역직렬화 성능을 높인다.

## 구현 가능 여부

| OS | 가능 여부 | 비고 |
|----|-----------|------|
| Windows | ✅ 가능 | Named pipe |
| macOS | ✅ 가능 | Unix socket |
| Linux | ✅ 가능 | Unix socket |
