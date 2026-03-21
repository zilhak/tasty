# 06. CLI 도구

## cmux 구현 방식

- Swift 단일 파일 (501KB)
- Unix 소켓을 통해 앱과 통신
- 명령: notify, list-notifications, new-workspace, send, send-key, tree, themes, ssh 등
- 핸들 기반 참조 (surface:1, pane:2, workspace:3)
- tmux 호환 명령 세트

## 크로스 플랫폼 구현 방안

### 구현 방식

동일한 Rust 바이너리가 GUI와 CLI 두 모드로 동작:

```
tasty                → GUI 터미널 에뮬레이터 실행
tasty notify ...     → CLI 명령 (IPC로 실행 중인 GUI 앱에 전달)
tasty new-workspace  → 새 워크스페이스 생성 명령
```

`clap` 크레이트로 서브커맨드 파싱. 서브커맨드 없이 실행하면 GUI 모드로 진입한다.

### IPC 통신

| OS | IPC 방법 | 경로 |
|----|---------|------|
| **Linux** | Unix domain socket | `$XDG_RUNTIME_DIR/tasty.sock` |
| **macOS** | Unix domain socket | `$TMPDIR/tasty.sock` |
| **Windows** | Named pipe | `\\.\pipe\tasty-{session}` |

CLI는 IPC를 통해 JSON-RPC 메시지를 실행 중인 GUI 인스턴스에 전달하고, 응답을 stdout에 출력한다.

### 주요 명령 (계획)

| 명령 | 설명 |
|------|------|
| `tasty` | GUI 터미널 에뮬레이터 실행 |
| `tasty notify <msg>` | 알림 전송 |
| `tasty new-workspace [--command <cmd>]` | 워크스페이스 생성 |
| `tasty send <text>` | 텍스트 입력 전송 |
| `tasty send-key <key>` | 키 입력 전송 |
| `tasty list` | 워크스페이스/패인 목록 |
| `tasty tree` | 전체 계층 트리 뷰 |
| `tasty split [--horizontal\|--vertical]` | 패인 분할 |
| `tasty focus <pane-id>` | 패인 포커스 |
| `tasty claude [--workspace <name>]` | Claude Code 실행 |

### 핵심 크레이트

- `clap` — CLI 파싱
- `serde_json` — JSON 직렬화
- `interprocess` 또는 `tokio` — IPC

## 최적화 전략

- **IPC 연결 풀링**: 여러 CLI 명령을 빠르게 연속 실행할 때 소켓 연결을 재사용한다. 연결 설정 오버헤드를 줄인다.
- **응답 타임아웃**: IPC 응답이 없을 때 적절한 타임아웃(예: 5초)을 적용하여 CLI가 무한 대기하지 않도록 한다.
- **바이너리 크기**: 단일 바이너리 내 CLI/GUI 모드를 심볼 스트리핑, LTO(Link-Time Optimization) 등으로 최적화한다. `cargo` 릴리스 프로파일에서 `strip = true`, `lto = true` 설정.
- **비동기 명령**: fire-and-forget 명령(예: `tasty notify`)은 응답을 기다리지 않고 즉시 종료한다. GUI에 메시지를 보내고 바로 exit한다.

## 구현 가능 여부

| OS | 가능 여부 | 비고 |
|----|-----------|------|
| Windows | ✅ 가능 | Named pipe 사용 |
| macOS | ✅ 가능 | Unix socket 사용 |
| Linux | ✅ 가능 | Unix socket 사용 |
