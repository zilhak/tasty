# 아키텍처 개요

Tasty는 17개 소스 파일, 약 7,575줄의 Rust 코드로 구성된 크로스 플랫폼 GPU 가속 터미널 에뮬레이터다.

## 기술 스택

| 역할 | 라이브러리 | 버전 |
|------|-----------|------|
| 윈도우/입력 | winit | 0.30 |
| GPU 렌더링 | wgpu | 24 |
| UI 위젯 | egui + egui-wgpu + egui-winit | 0.31 |
| VTE 파싱 | termwiz | 0.22 |
| PTY | portable-pty (ConPTY/Unix) | 0.8 |
| 폰트 래스터라이징 | cosmic-text + swash | - |
| IPC 프로토콜 | serde_json (JSON-RPC 2.0) | - |
| CLI | clap | - |
| 설정 파일 | toml + directories | - |
| OS 알림 | notify-rust | - |

## 소스 파일 구성

| 파일 | 줄 수 | 역할 |
|------|-------|------|
| `model.rs` | 1,370 | 데이터 모델 (Workspace/Pane/Tab/Panel/Surface 계층) |
| `main.rs` | 762 | 진입점, winit 이벤트 루프, App 구조체 |
| `state.rs` | 733 | 애플리케이션 상태 관리 (AppState) |
| `renderer.rs` | 699 | wgpu 기반 셀 렌더러 (CellRenderer) |
| `terminal.rs` | 646 | PTY + VTE 파싱 + 터미널 에뮬레이션 |
| `ipc/handler.rs` | 564 | JSON-RPC 요청 핸들러 (20개 메서드) |
| `ui.rs` | 401 | egui UI (사이드바, 탭 바, 알림 패널) |
| `gpu.rs` | 370 | GPU 상태 관리 (GpuState, wgpu 초기화) |
| `font.rs` | 358 | 폰트 설정, 글리프 아틀라스 |
| `cli.rs` | 331 | CLI 클라이언트 (clap 서브커맨드) |
| `hooks.rs` | 290 | 이벤트 훅 시스템 (HookManager) |
| `settings.rs` | 274 | TOML 설정 파일 로드/저장 |
| `notification.rs` | 239 | 알림 저장소 + OS 알림 |
| `settings_ui.rs` | 208 | egui 설정 윈도우 UI |
| `ipc/server.rs` | 196 | TCP 기반 IPC 서버 |
| `ipc/protocol.rs` | 131 | JSON-RPC 2.0 프로토콜 타입 |
| `ipc/mod.rs` | 3 | IPC 모듈 re-export |
| **합계** | **7,575** | |

## 모듈 의존성 다이어그램 (DAG)

```
                          ┌──────────┐
                          │  main.rs │  (진입점)
                          └────┬─────┘
               ┌───────────────┼───────────────────┐
               │               │                   │
               ▼               ▼                   ▼
          ┌────────┐     ┌──────────┐        ┌──────────┐
          │ gpu.rs │     │ state.rs │        │  cli.rs  │
          └───┬────┘     └────┬─────┘        └────┬─────┘
              │               │                   │
     ┌────────┼───────┐   ┌──┼───────────────┐   │
     │        │       │   │  │               │   │
     ▼        ▼       ▼   ▼  ▼               ▼   ▼
┌─────────┐ ┌────┐ ┌──────────┐  ┌──────────────────┐
│renderer │ │ui.rs│ │ model.rs │  │   ipc/           │
│  .rs    │ └──┬──┘ └────┬─────┘  │  ├ server.rs     │
└────┬────┘    │         │        │  ├ handler.rs    │
     │         │         ▼        │  └ protocol.rs   │
     ▼         │   ┌───────────┐  └────────┬─────────┘
┌─────────┐    │   │terminal.rs│           │
│ font.rs │    │   └───────────┘           │
└─────────┘    │                           │
               ▼                           │
      ┌──────────────┐                     │
      │settings_ui.rs│                     │
      └──────┬───────┘                     │
             ▼                             │
      ┌────────────┐     ┌───────────────┐ │
      │settings.rs │     │notification.rs│ │
      └────────────┘     └───────────────┘ │
                                           │
                         ┌──────────┐      │
                         │ hooks.rs │◄─────┘
                         └──────────┘
```

## 계층 구조

### 기반 계층 (Foundation)

외부 의존성 없이 순수 데이터 구조와 로직을 제공한다.

| 모듈 | 의존 대상 |
|------|-----------|
| `model.rs` | `terminal.rs` |
| `ipc/protocol.rs` | serde_json |
| `settings.rs` | toml, directories |
| `notification.rs` | `model.rs` (타입만), notify-rust |
| `hooks.rs` | regex |

### 중간 계층 (Services)

기반 계층 위에 구축된 서비스 모듈.

| 모듈 | 의존 대상 |
|------|-----------|
| `terminal.rs` | portable-pty, termwiz |
| `font.rs` | cosmic-text, wgpu |
| `ipc/server.rs` | `ipc/protocol.rs` |
| `ipc/handler.rs` | `state.rs`, `model.rs`, `hooks.rs`, `ipc/protocol.rs` |

### 상위 계층 (Composition)

여러 중간 계층을 조합하여 상위 기능을 구현한다.

| 모듈 | 의존 대상 |
|------|-----------|
| `state.rs` | `model.rs`, `terminal.rs`, `settings.rs`, `notification.rs`, `hooks.rs`, `settings_ui.rs` |
| `renderer.rs` | `font.rs`, `model.rs`, wgpu, termwiz |
| `ui.rs` | `state.rs`, `model.rs`, egui |
| `settings_ui.rs` | `settings.rs`, egui |

### 최상위 계층 (Integration)

모든 하위 계층을 통합하는 진입점.

| 모듈 | 의존 대상 |
|------|-----------|
| `gpu.rs` | `renderer.rs`, `state.rs`, `ui.rs`, `settings_ui.rs`, `model.rs`, wgpu, egui |
| `main.rs` | `gpu.rs`, `state.rs`, `cli.rs`, `ipc/server.rs`, `ipc/handler.rs`, `model.rs`, `terminal.rs`, `hooks.rs`, `notification.rs`, `settings.rs`, `settings_ui.rs`, winit |
| `cli.rs` | `ipc/protocol.rs`, `ipc/server.rs`, clap |

## 데이터 흐름 요약

1. **키보드 입력 → 터미널 → 화면 출력**: winit KeyEvent → App::window_event → Terminal::send_key → PTY → 리더 스레드 → EventLoopProxy → process() → Surface → CellRenderer → wgpu 렌더
2. **PTY 출력 → 파싱 → 렌더링**: PTY 리더 스레드 → mpsc 채널 → Terminal::process → termwiz Parser → action_to_changes → Surface → CellRenderer::prepare_viewport → 2-pass 렌더
3. **IPC 요청 → 처리 → 응답**: TCP 클라이언트 → IpcServer → mpsc 채널 → main 스레드 process_ipc → handler::handle → AppState 조작 → JsonRpcResponse → TCP 전송
4. **알림 발생 → 저장 → UI 표시**: Terminal 이벤트 → collect_events → NotificationStore::add → 사이드바 배지 + 알림 패널 렌더
5. **설정 로드 → 적용**: TOML 파일 → Settings::load → AppState 초기화 → 런타임 반영

## 하위 문서

| 문서 | 설명 |
|------|------|
| [모듈별 상세](modules.md) | 17개 소스 파일 상세 분석 |
| [데이터 흐름](data-flows.md) | 5가지 주요 데이터 흐름 단계별 설명 |
| [라이브러리 분리 분석](library-separation/index.md) | 8개 분리 후보를 7개 관점에서 다관점 분석 |
| [리팩토링 분석](refactoring.md) | 코드 개선점, 리팩토링 로드맵 |
