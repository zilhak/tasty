# Tasty 문서

크로스 플랫폼 GPU 가속 네이티브 터미널 에뮬레이터의 설계·명세·개발 문서. 이 인덱스가 진입점이다.

문서의 위치와 작성 방법은 [문서 작성 규칙](documentation-model.md)을 따른다.

## 작업 전 필독

| 문서 | 설명 |
|------|------|
| [identity.md](identity.md) | **Tasty 정체성과 불가침 원칙** — 동시성(로컬 사용자/AI Agent/원격 사용자), 사용자/에이전트 분리, headless, 점유. 설계·구현 전에 읽는다 |
| [concepts/actors.md](concepts/actors.md) | **주체(Actors)** — 로컬 사용자·에이전트·원격 사용자와 점유의 정의 |
| [concepts/hierarchy.md](concepts/hierarchy.md) | **구조 계층** — Window/View › Workspace › Pane › Tab › Surface + 두 레벨 레이아웃 |
| [concepts/plugins.md](concepts/plugins.md) | **플러그인** — 배포/통합 축, surface_kind 렌더 분기, 권한 |
| [concepts/ubiquitous-language.md](concepts/ubiquitous-language.md) | **통합 용어집** — 주요 용어와 상세 설명 링크, tmux/iTerm2 및 코드 심볼과의 대응 |
| [concepts/typed-length.md](concepts/typed-length.md) | **타입 있는 길이** — `PhysicalPx`/`LogicalPx` newtype (DPI 혼동 컴파일 차단) |
| [documentation-model.md](documentation-model.md) | **문서 모델** — 문서 분류와 기능·화면 설명의 분리 기준. 새 문서를 쓰기 전에 읽는다 |

## 문서

| 카테고리 | 진입 | 설명 |
|----------|------|------|
| 개념 (concepts) | [concepts/index.md](concepts/index.md) | 공통 용어와 객체 관계 |
| 기능 (기획·화면) | [features/index.md](features/index.md) | 제품 기능과 화면 사용법 |
| 번들 플러그인 | [plugins/index.md](plugins/index.md) | 번들 9종 중 8종의 기능 문서 |
| 설계 (design) | [design/index.md](design/index.md) | 정책·시스템·흐름 |
| 레퍼런스 (조회) | [reference/index.md](reference/index.md) | API·이벤트·출력 형식 |
| 개발 가이드 | [dev-guide/index.md](dev-guide/index.md) | 구현·빌드·검증·배포 절차 |
| 아키텍처 | [architecture/index.md](architecture/index.md) | 모듈 구조와 데이터 흐름 |
| AI 자체 검증 | [ai-verification/index.md](ai-verification/index.md) | IPC·화면·플랫폼 검증 방법 |
| 근거 (ADR) | [adr/index.md](adr/index.md) | 중요한 선택·이유·대안·재검토 조건 |
| 설치 | [installation.md](installation.md) | 플랫폼별 설치 방법 |
