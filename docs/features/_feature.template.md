<!--
기획문서 (internal behavior, 1순위) 템플릿.
새 기능을 만들 때:  cp docs/features/_feature.template.md docs/features/<feature>/index.md
규칙: docs/documentation-model.md / docs/identity.md (작성 전 필독).

작성 시 지킬 것:
- 내부 *동작*(WHAT)만 적는다. 내부 *구현*(파일·함수 콜사이트, feature gate)은 코드가 진실 → 적지 않는다.
- 시각 수치/토큰은 design-system 을 링크. docs 에 재서술 금지.
- 빌드/로드맵 상태(Phase·구현 예정·이관)는 적지 않는다 — 현재 상태만.
- 화면이 없으면(headless 전용) "## 화면" 섹션을 지운다.
이 주석 블록은 실제 문서에서 삭제한다.
-->

# <기능 이름>

- **Status**: Implemented | Partial | Planned
- **주체**: 로컬 사용자 / AI Agent / 원격 접속 사용자 중 이 기능을 쓰는 주체 (복수 가능 — [주체](../../concepts/ubiquitous-language.md#주체-actors))
- **ADR**: ADR-XXXX (있으면)
- **코드**: `src/...` / `crates/...`
- **화면**: `screens/<screen>.md` (없으면 "없음 — headless 전용")

## 목적

이 기능이 왜 존재하나. 한 문단.

## 내부 동작 (headless-valid)

기능이 *무엇을* 하나 — 상태·판정 규칙·흐름·예외. GUI 없이도 성립하는 동작.

## 인터페이스

- **AI Agent (IPC/CLI)**: `tasty ...` / IPC `...` — 파라미터·응답
- **사용자 트리거**: <단축키 / 클릭 / 자동> (화면이 있는 경우)
- **원격 / 점유**: (해당 시) 점유 필요 여부 등

## 비-목표 (Out of scope)

이 기능이 *하지 않는* 것.

## Acceptance Criteria

- [ ] Given <조건> When <행동> Then <결과>  <!-- headless(IPC/CLI)로 검증 가능한 형태 -->

## 화면

- `screens/<screen>.md` — <한 줄 설명>
