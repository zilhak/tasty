# 의존성 분리 — 파일 분할 계획

대형 파일을 모듈 디렉토리로 분할하여 가독성, 테스트 용이성, 그리고 향후 라이브러리 추출을 준비한다.

## 현재 파일 크기

| # | 파일 | 줄 수 | 역할 |
|---|------|-------|------|
| 1 | `model.rs` | 1,775 | 데이터 모델 (Workspace/Pane/Tab/Panel/Surface 계층) |
| 2 | `terminal.rs` | 1,358 | PTY + VTE 파싱 + 터미널 에뮬레이션 |
| 3 | `main.rs` | 891 | 진입점, winit 이벤트 루프, App 구조체 |
| 4 | `renderer.rs` | 700 | wgpu 기반 셀 렌더러 (CellRenderer) |
| 5 | `state.rs` | 611 | 애플리케이션 상태 관리 (AppState) |
| 6 | `ipc/handler.rs` | 591 | JSON-RPC 요청 핸들러 (20개 메서드) |
| 7 | `font.rs` | 408 | 폰트 설정, 글리프 아틀라스 |
| 8 | `gpu.rs` | 402 | GPU 상태 관리 (GpuState, wgpu 초기화) |
| 9 | `ui.rs` | 401 | egui UI (사이드바, 탭 바, 알림 패널) |
| 10 | `cli.rs` | 340 | CLI 클라이언트 (clap 서브커맨드) |
| 11 | `settings.rs` | 326 | TOML 설정 파일 로드/저장 |
| 12 | `hooks.rs` | 290 | 이벤트 훅 시스템 (HookManager) |
| 13 | `notification.rs` | 239 | 알림 저장소 + OS 알림 |
| 14 | `settings_ui.rs` | 208 | egui 설정 윈도우 UI |
| 15 | `ipc/server.rs` | 196 | TCP 기반 IPC 서버 |
| 16 | `ipc/protocol.rs` | 131 | JSON-RPC 2.0 프로토콜 타입 |
| 17 | `ipc/mod.rs` | 3 | IPC 모듈 re-export |
| | **합계** | **8,870** | |

## 분할 대상 (400줄 이상)

| 파일 | 줄 수 | 분할 후 구조 | 상세 문서 |
|------|-------|-------------|----------|
| `model.rs` | 1,775 | `src/model/` (6파일) | [model-split.md](model-split.md) |
| `terminal.rs` | 1,358 | `src/terminal/` (5파일) | [terminal-split.md](terminal-split.md) |
| `main.rs` | 891 | `main.rs` + 2파일 | [main-split.md](main-split.md) |
| `renderer.rs` | 700 | `src/renderer/` (4파일) | [renderer-split.md](renderer-split.md) |
| `ipc/handler.rs` | 591 | `src/ipc/handler/` (3파일) | [handler-split.md](handler-split.md) |
| `font.rs` | 408 | 분할 불필요 (단일 책임) | — |

## 라이브러리 분리와의 연계

파일 분할은 [라이브러리 분리 계획](../library-separation/index.md)의 사전 작업이다.

| 분할 작업 | 연계되는 라이브러리 crate |
|----------|------------------------|
| `terminal.rs` → `src/terminal/` | `tasty-terminal` crate 추출 |
| `model.rs` → `src/model/` | `tasty-model` crate 추출 |
| `renderer.rs` → `src/renderer/` | `tasty-renderer` crate 추출 |
| `ipc/handler.rs` → `src/ipc/handler/` | `tasty-ipc` crate 추출 |

모듈 디렉토리로 분할한 뒤, 각 `mod.rs`의 pub 인터페이스를 그대로 crate의 `lib.rs`로 승격시키면 외부 API 변경 없이 라이브러리 추출이 가능하다.

## 추가 분석 문서

| 문서 | 설명 |
|------|------|
| [커플링 분석](coupling-analysis.md) | 타입별 참조 핫스팟, 순환 의존성 검사 |
| [실행 로드맵](execution-roadmap.md) | 7단계 실행 순서, 검증 방법 |

## 실행 순서 요약

| 단계 | 작업 | 예상 변경 파일 |
|------|------|--------------|
| 1 | `model.rs` → `src/model/` | 6파일 생성, 6파일 import 수정 |
| 2 | `terminal.rs` → `src/terminal/` | 5파일 생성, 2파일 import 수정 |
| 3 | `renderer.rs` → `src/renderer/` | 4파일 생성, 1파일 import 수정 |
| 4 | `ipc/handler.rs` → `src/ipc/handler/` | 3파일 생성 |
| 5 | `main.rs` 분할 | 2파일 생성 |
| 6 | 전체 검증 | `cargo check`, `cargo test`, `cargo clippy` |
| 7 | import 그래프 정리 | 불필요한 pub 제거, 재검증 |
