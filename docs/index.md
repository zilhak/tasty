# Tasty 문서

크로스 플랫폼 GPU 가속 네이티브 터미널 에뮬레이터의 설계·명세·개발 문서. 이 인덱스가 진입점이다.

> 모든 문서는 [문서 모델](documentation-model.md)(behavior-first)에 맞춰 재작성·검증을 마쳤다. 옛 `docs-old/` 는 전부 이관/흡수/폐기되어 제거됐다.

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
| 개념 (concepts) | [concepts/index.md](concepts/index.md) | ✅ |
| 기능 (기획·화면) | [features/index.md](features/index.md) | ✅ |
| 번들 플러그인 | [plugins/index.md](plugins/index.md) | ✅ (8종) |
| 설계 (design) | [policies/](design/policies/focus.md) · [systems/](design/systems/theme.md) ([popup](design/systems/popup.md)·[toast](design/systems/toast.md)·[banner](design/systems/banner.md)·[token-crosswalk](design/systems/token-crosswalk.md)) · [flows/](design/flows/index.md) | ✅ (정책 · 시스템 · 흐름) |
| 레퍼런스 (조회) | [reference/index.md](reference/index.md) | ✅ |
| 개발 가이드 | [dev-guide/index.md](dev-guide/index.md) | ✅ |
| 아키텍처 | [architecture/index.md](architecture/index.md) | ✅ |
| AI 자체 검증 | [ai-verification/index.md](ai-verification/index.md) | ✅ |
| 근거 (ADR) | [adr/index.md](adr/index.md) | ✅ (Accepted 후 불변) |
| 설치 | [installation.md](installation.md) | ✅ |
