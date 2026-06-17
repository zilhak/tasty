# Tasty 문서

크로스 플랫폼 GPU 가속 네이티브 터미널 에뮬레이터의 설계·명세·개발 문서. 이 인덱스가 진입점이다.

> **재정비 중**: docs 를 새 [문서 모델](documentation-model.md)에 맞춰 다시 쓰는 중이다. 기존 문서는 [`docs-old/`](../docs-old/) 에 참고용으로 보존되어 있다(현재 상태와 다를 수 있음). **새로 작성·검증된 문서만 아래에 등재한다.**

## 작업 전 필독

| 문서 | 설명 |
|------|------|
| [identity.md](identity.md) | **Tasty 정체성과 불가침 원칙** — 동시성(로컬 사용자/AI Agent/원격 사용자), 사용자/에이전트 분리, headless, 점유. 모든 설계의 축. **가장 먼저 읽는다** |
| [concepts/actors.md](concepts/actors.md) | **주체(Actors)** — 세 주체 + 점유 모델 canonical 정의 |
| [concepts/hierarchy.md](concepts/hierarchy.md) | **구조 계층** — Window/View › Workspace › Pane › Tab › Surface + 두 레벨 레이아웃 |
| [concepts/plugins.md](concepts/plugins.md) | **플러그인** — 배포/통합 축, surface_kind 렌더 분기, 권한 |
| [concepts/ubiquitous-language.md](concepts/ubiquitous-language.md) | **통합 용어집** — 전 용어 한 줄 정의 + 정본 링크, tmux/iTerm2 대응, 코드 심볼 크로스워크 |
| [concepts/typed-length.md](concepts/typed-length.md) | **타입 있는 길이** — `PhysicalPx`/`LogicalPx` newtype (DPI 혼동 컴파일 차단) |
| [documentation-model.md](documentation-model.md) | **문서 모델** — 카테고리 지도 + 기획(동작, 1순위)/화면(투영, 2순위) 분리 규칙. 새 문서 작성 전 |

## 문서

| 카테고리 | 진입 | 상태 |
|----------|------|------|
| 근거 (ADR) | [adr/index.md](adr/index.md) | ✅ 보존 (불변) |
| 기능 (기획·화면) | [features/index.md](features/index.md) | 🚧 폴더 모델 재작성 중 (템플릿 준비됨, 카탈로그 비어있음) |
| 번들 플러그인 | [plugins/index.md](plugins/index.md) | 🚧 재작성 중 (8종 등재) |
| 설계 (design) | [policies/](design/policies/focus.md) · [systems/](design/systems/theme.md) | 🚧 재작성 중 (focus·key-mapping·system-tray 정책, theme 시스템) |
| 개발 가이드 | [independent-verification](dev-guide/independent-verification.md) · [debug-ipc](dev-guide/debug-ipc.md) · [attach-behavior](dev-guide/attach-behavior.md) · [plugin-development](dev-guide/plugin-development.md) · [plugin-permissions](dev-guide/plugin-permissions.md) · [plugin-sensitive-data](dev-guide/plugin-sensitive-data.md) | 🚧 재작성 중 (독립검증·debug IPC·attach·플러그인 제작/권한 작성됨) |

## 재작성 대기 (docs-old)

아래는 [`docs-old/`](../docs-old/) 에서 재작성 대기 중 — 새 모델로 옮겨오며 검토·교정한다:

`design/`(횡단 규칙·흐름) · `architecture/` · `agent-guide/` · `ai-verification/` · `evaluations/` · `dev-guide/`(나머지) · `installation.md`
