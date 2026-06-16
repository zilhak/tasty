# Tasty 문서

크로스 플랫폼 GPU 가속 네이티브 터미널 에뮬레이터의 설계·명세·개발 문서. 이 인덱스가 진입점이다.

> **재정비 중**: 제품 명세(기획·화면)를 새 [문서 모델](documentation-model.md)에 맞춰 다시 쓰는 중이다. 옛 명세는 [`docs-old/`](../docs-old/) 에 참고용으로 남아 있으나 **현재 상태와 안 맞는 부분이 많다** (특히 화면기획 — claude design 도입 이후 변경). 개발·운영 가이드(B)는 코드/프로세스 파생이라 유효하므로 복원되어 있다 (검토·교정 대상).

## 작업 전 필독

| 문서 | 설명 |
|------|------|
| [identity.md](identity.md) | **Tasty 정체성과 불가침 원칙** — 동시성 중심 다중 에이전트 터미널, 사용자/에이전트 분리, headless, 개인화. 모든 설계의 축. **가장 먼저 읽는다** |
| [documentation-model.md](documentation-model.md) | **문서 모델** — 전체 카테고리 지도 + 기획(동작, 1순위)/화면(투영, 2순위) 분리 규칙. 새 문서 작성 전 필독 |
| [concepts/index.md](concepts/index.md) | 개념·용어 (유비쿼터스 언어, 레이아웃, typed-length) — 코드 작업 전 필독 |

---

## A. 제품 명세 — "tasty 가 무엇인가"

| 카테고리 | 진입 | 상태 |
|----------|------|------|
| 근거 (ADR) | [adr/index.md](adr/index.md) | ✅ 보존 (불변) |
| 평가 / POC | [evaluations/index.md](evaluations/index.md) | ✅ 복원 |
| 기능 (기획·화면) | [features/index.md](features/index.md) | 🚧 폴더 모델로 재작성 중 (양식: 기획/화면 템플릿 2종 준비됨, 카탈로그 비어있음) |
| 횡단 규칙·흐름 | `design/{policies,flows,systems}/` | 🚧 재정비 대기 (docs-old) |
| 시각 진실 | `design-system/` (vendor) | ⏳ vendor 예정 (claude design 산출물) |

## B. 개발·운영 가이드 — "tasty 를 어떻게 다루나"

| 카테고리 | 진입 | 설명 |
|----------|------|------|
| [개발 가이드](dev-guide/index.md) | `dev-guide/` | tasty 개발 — 빌드·커밋·릴리스·플러그인·i18n·에러처리·GPU·디버그 (개발 AI 에이전트용) |
| [아키텍처](architecture/index.md) | `architecture/` | 크레이트 구조 / 데이터 흐름 / invariant |
| [자체 검증](ai-verification/visual-verification.md) | `ai-verification/` | UI·렌더링 검증 절차 |
| [에이전트 가이드](agent-guide/index.md) | `agent-guide/` | tasty 를 IPC/CLI 로 조작 (릴리스 에셋) |
| [설치](installation.md) | `installation.md` | OS·아키텍처별 설치 |

---

> **링크 주의**: 복원된 B 문서들 일부가 아직 docs-old 에 있는 features/design 을 가리켜 깨진 링크가 있을 수 있다. 해당 명세 재작성 시 함께 교정한다.
