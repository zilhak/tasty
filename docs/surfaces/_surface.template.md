<!--
Surface kind 하나의 명세 템플릿.
새 kind 를 만들 때:  cp docs/surfaces/_surface.template.md docs/surfaces/<kind>/index.md
규칙: docs/documentation-model.md · docs/surfaces/index.md 의 "타입 문서의 공통 항목".

작성 시 지킬 것:
- 내부 동작을 먼저 쓰고 화면은 끝에 연결한다.
- 공통 항목 중 해당하지 않는 절은 지우지 않고 이유를 한 줄로 적는다.
- 구조 트리·분할·cwd 상속처럼 모든 kind 에 같은 규칙은 다시 쓰지 않고 작업 영역·정책 문서를 연결한다.
- 시각 수치와 토큰은 디자인 원본을 연결하고 값을 반복해서 적지 않는다.
- 마크다운 체크박스를 쓰지 않는다. 검증 기준은 평문 Given/When/Then 불릿이다.
- 화면이 하나이고 템플릿 절 밖의 고유 규칙이 없으면 "## 화면" 에 화면정의서를 직접 쓴다.
  둘 이상이거나 고유 규칙이 있으면 screens/ 파일로 빼고 여기엔 목록만 둔다(documentation-model.md §3).
링크는 docs/surfaces/<kind>/index.md 위치 기준으로 쓴다. 이 주석 블록은 실제 문서에서 삭제한다.
-->

# <kind 표시 이름> (`<kind>`)

- **Status**: Implemented | Partial | Planned
- **kind**: `<kind>` — host 내장 | 번들 plugin `<plugin id>`
- **렌더 경로**: host-egui | egui-mesh | webview
- **주체**: 로컬 사용자 / AI Agent / 원격 접속 사용자 중 이 kind 를 쓰는 주체
- **ADR**: ADR-XXXX (있으면)
- **코드**: `src/...` / `crates/...`
- **화면**: 아래 절 또는 `screens/<screen>.md`

## 목적·지원 범위

이 kind 가 보여 주는 콘텐츠와 지원하지 않는 범위.

## 상태 소유자

구조 트리의 leaf, 실행 자원, View 상태가 각각 어디에 있는지.

## 생성·갱신·종료

만드는 경로와 필수 params, 콘텐츠 갱신, 닫을 때 정리하는 자원.

## 저장·복원

레이아웃에 저장하는 값과 저장하지 않는 값, 복원 순서, 복원 실패 시 상태.

## IPC·CLI

이 kind 전용 명령과 공통 명령에서 다른 점.

## headless·원격 제약

헤드리스 등록 여부와 attach mirror 에서의 동작.

## 검증 기준

- Given <조건> When <행동> Then <결과>

## 화면

### 트리거

### UI 요소 인벤토리

### 상태별 시각

### 시각 소스
