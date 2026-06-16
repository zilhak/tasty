# Tasty 문서 모델 (Documentation Model)

> 이 문서는 Tasty 의 모든 설계/명세 문서가 **어떻게 나뉘고, 어디에 속하며, 서로 어떻게 연결되는가** 를 정의하는 중앙 규칙이다. 새 문서를 만들거나 기존 문서를 고치기 전에 먼저 읽는다. 이 분류체계의 *결정 근거* 는 [ADR-0006](adr/0006-docs-taxonomy-behavior-first.md).

## 1. 핵심 원칙 — 동작이 1순위, 화면은 2순위

> Tasty 정체성 전문은 [`identity.md`](identity.md). 이 절은 그중 *문서 구조에 직접 관련된 축* (headless 동작-우선)만 다룬다.

Tasty 는 **headless 로 동작**한다. 따라서 한 기능의 진실은 *내부 동작* 이고, *화면* 은 그 동작을 사람 사용자에게 투영한 것일 뿐이다.

- **기획문서 (behavior)** = 기능이 내부적으로 무엇을 하는가. headless 에서도 유효. **1순위, 부모.**
- **화면정의서 (screen)** = 그 동작이 화면에 어떻게 보이는가. **2순위, 자식.**

이 축은 Tasty 정체성인 **사용자/에이전트 행동 분리** 와 같은 축이다:

| | 기획문서 (1순위) | 화면정의서 (2순위) |
|---|---|---|
| 정체성 | 내부 동작 (headless-valid) | 동작의 시각 투영 |
| 행동 축 | 에이전트 행동 (CLI/IPC) | 사용자 행동 (키/마우스) |
| 관계 | 부모 | 자식 (하위) |

**기획 1 : 화면 0..N.** headless-only 기능은 화면 0개. 한 동작이 여러 화면에 투영되면 N개.

## 2. 문서 지도 (전체 카테고리)

docs 는 두 묶음으로 나뉜다. §1 의 **동작-우선 taxonomy 는 A(제품 명세)에만** 적용된다. B(개발·운영 가이드)는 화면/동작 축과 무관한 독립 카테고리다.

### A. 제품 명세 — "tasty 가 무엇인가"

| 종류 | 위치 | 무엇을 | 소유 / 변경 |
|------|------|--------|-------------|
| 기획문서 | `docs/features/<f>/index.md` | 내부 동작 (1순위) | docs / 자유 |
| 화면정의서 | `docs/features/<f>/screens/<s>.md` | 시각 투영 (2순위) | docs / 자유 |
| 횡단 규칙·흐름 | `docs/design/{policies,flows,systems}/` | 여러 기능 공통 규칙/흐름 | docs / 자유 |
| 용어 | `docs/concepts/` | 유비쿼터스 언어 | docs / 자유 |
| 근거 (ADR) | `docs/adr/` | 왜 그렇게 결정했나 | docs / Accepted 후 불변 (supersede) |
| 평가 / POC | `docs/evaluations/` | ADR 근거 원본 (시점성 분석) | docs / 시점 보존 |
| 시각 진실 | `design-system/…` (vendor) | 픽셀 / 토큰 / 컴포넌트 | **claude design** / design-request 경유만 |

**시각은 절대 docs 에 재서술하지 않는다.** 화면정의서는 요소 인벤토리와 "동작 상태 → 시각" 매핑만 적고, 픽셀/토큰 값은 `design-system/` 을 링크한다. 복제하면 두 진실이 생겨 drift 한다.

### B. 개발·운영 가이드 — "tasty 를 어떻게 다루나"

| 종류 | 위치 | 무엇을 | 대상 독자 |
|------|------|--------|-----------|
| **개발 가이드** | `docs/dev-guide/` | **tasty 를 개발하는 법 — 빌드·커밋·릴리스·플러그인·i18n·에러처리·GPU·디버그 IPC·자체검증 등** | 개발 AI 에이전트 |
| 아키텍처 | `docs/architecture/` | 크레이트 구조 / 데이터 흐름 / invariant | 개발 AI 에이전트 |
| 자체 검증 | `docs/ai-verification/` | UI·렌더링 검증 절차 | 개발 AI 에이전트 |
| 에이전트 가이드 | `docs/agent-guide/` | tasty 를 IPC/CLI 로 조작하는 법 (릴리스 에셋으로 배포) | 사용자의 AI 에이전트 |
| 설치 | `docs/installation.md` | OS·아키텍처별 설치 | 사용자 / 에이전트 |

> B 는 코드·프로세스에서 파생되어 claude design 도입과 무관하게 대체로 유효하다. 따라서 **백지 재작성 대상이 아니라 검토·교정 대상** 이다 (§ 재정비 절차는 [`index.md`](index.md) 참조).

## 3. 폴더 구조 (중첩)

```
docs/features/<feature>/
  index.md            # 기획문서 (내부 동작, 1순위)
  screens/
    <screen>.md       # 화면정의서 (2순위) — 0..N개
```

- headless 기능 → `screens/` 없음 (화면 없음이 구조로 드러난다).
- 다중 화면 → `screens/` 에 여러 파일.
- 템플릿: [기획](features/_feature.template.md) · [화면](features/_screen.template.md).

## 4. 연결 개념 — 합성 화면은 "언급" 으로만 잇는다

여러 기능이 한 화면에 모이는 합성 화면(사이드바, 메인 윈도우, 설정 윈도우 등)은 **자기 영역만 기술하고, 다른 동작/창으로 위임되는 요소엔 그 문서를 링크만** 한다. 임베드·복제 없음.

예 — 사이드바 화면정의서:

```
## UI 요소 인벤토리
- 최상단: 아이콘 / 로고 / 접기 버튼 영역
- 중단: 워크스페이스 영역 (남는 높이 전부)
- 최하단:
  - 도구 버튼      → features/tools-menu/ 참조
  - 플러그인 버튼   → features/plugin-system/screens/plugins-window.md 참조
  - 설정 버튼      → features/settings/screens/settings-window.md 참조
```

사이드바 문서는 "도구 메뉴에 무엇이 들었는지" 를 적지 않는다 — 버튼 설명 옆에 링크만 둔다.

## 5. design ↔ code 연계 (claude design 협업 채널)

디자인은 claude design 산출물(`design-system/`)이며 claude code 는 이를 **직접 수정하지 않는다.** 두 쪽은 두 개의 휘발성 채널로 연계한다.

```
요청서 (outbox)    claude code ──► design    반영되면 삭제
changelog (inbox)  design ──► claude code    흡수되면 폐기
```

| 채널 | 위치 | git | 수명 |
|------|------|-----|------|
| 요청서 outbox | `.claude-workspace/design-request/` | ❌ ignore | 디자인 반영 후 삭제 |
| changelog inbox | `design-system/changelog/` | ❌ ignore | docs 흡수 후 폐기 |

### changelog 흡수 = 라우팅 (복붙 아님)

changelog 엔트리 한 건은 성격이 다른 조각이 섞여 있으므로 조각별로 다른 주인에게 보낸다:

| changelog 조각 | 흡수처 |
|----------------|--------|
| What (동작/요소 변경) | `docs/features/<f>/index.md` 또는 `screens/<s>.md` |
| Tokens / .jsx / 시각 | ❌ 흡수 안 함 — `design-system/` 이 주인 (링크만) |
| Rationale (왜) | 진짜 결정이면 `docs/adr/`, 사소하면 본문 한 줄 |
| For-implementing-side (flag / OS 분기) | `docs/design/flows/` 또는 본문 |

### watermark = 커밋이 장부 (별도 상태 파일 없음)

- **디자인 회수**: `chore(design): sync design-system @ YYYY-MM-DD (<topics>)` — 이 날짜가 "어디까지 받았나" 의 watermark.
- **구현 커밋**: 본문에 `Design: <흡수된 docs 경로 또는 changelog topic>` 줄.
- **미반영 분** = docs 에 아직 흡수되지 않은 changelog 엔트리. docs 가 곧 소비 상태의 진실이다.

## 6. 작성 규칙 요약

- 새 화면/동작 → 해당 `features/<f>/` 의 기획·화면 문서에 흡수. 없으면 폴더 신설.
- 시각 수치/토큰은 적지 말고 `design-system/` 을 링크.
- 결정의 근거는 본문에 길게 쓰지 말고 ADR 로 박고 링크.
- 합성 화면은 언급/링크로만 잇는다.
- 디자인 변경이 필요하면 `.claude-workspace/design-request/` 에 요청서(changelog 포함) 작성 → 사용자가 claude design 에 제출. 상세 워크플로는 `.claude/CLAUDE.md`.

## 관련

- [ADR-0006 — 문서 분류체계: 동작 우선](adr/0006-docs-taxonomy-behavior-first.md) (근거)
- [features/index.md](features/index.md) (기획·화면 카탈로그)
- 프로젝트 디자인 워크플로: `.claude/CLAUDE.md`
