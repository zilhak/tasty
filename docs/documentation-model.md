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
| 기획문서 | `docs/features/<f>/index.md` | 내부 동작 (1순위) — **host 제공** | docs / 자유 |
| 화면정의서 | `docs/features/<f>/screens/<s>.md` | 시각 투영 (2순위) | docs / 자유 |
| 번들 플러그인 | `docs/plugins/<id>/index.md` (+ `screens/`) | **플러그인이 제공**하는 동작·화면 (features 와 동일 구조, 제공자만 다름) | docs / 자유 |
| 횡단 규칙·흐름 | `docs/design/{policies,flows,systems}/` | 여러 기능 공통 규칙/흐름 | docs / 자유 |
| 용어 | `docs/concepts/` | 유비쿼터스 언어 | docs / 자유 |
| 근거 (ADR) | `docs/adr/` | 왜 그렇게 결정했나 (보류 결정·대안·재검토 trigger 포함) | docs / Accepted 후 불변 (supersede) |
| 시각 진실 | `design-system/…` (vendor) | 픽셀 / 토큰 / 컴포넌트 | **claude design** / design-request 경유만 |

**시각은 절대 docs 에 재서술하지 않는다.** 화면정의서는 요소 인벤토리와 "동작 상태 → 시각" 매핑만 적고, 픽셀/토큰 값은 `design-system/` 을 링크한다. 복제하면 두 진실이 생겨 drift 한다.

### B. 개발·운영 가이드 — "tasty 를 어떻게 다루나"

| 종류 | 위치 | 무엇을 | 대상 독자 |
|------|------|--------|-----------|
| **개발 가이드** | `docs/dev-guide/` | **tasty 를 개발하는 법 — 빌드·커밋·릴리스·플러그인·i18n·에러처리·GPU·디버그 IPC·자체검증 등** | 개발 AI 에이전트 |
| 아키텍처 | `docs/architecture/` | 크레이트 구조 / 데이터 흐름 / invariant | 개발 AI 에이전트 |
| 자체 검증 | `docs/ai-verification/` | UI·렌더링 검증 절차 | 개발 AI 에이전트 |
| 에이전트 가이드 | 미신설 — 조회용 표면은 [`docs/reference/`](reference/index.md) | tasty 를 IPC/CLI 로 조작하는 법 (릴리스 에셋으로 배포) | 사용자의 AI 에이전트 |
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

여러 기능이 한 화면에 모이는 합성 화면(사이드바, MainView, 설정 화면 등)은 **자기 영역만 기술하고, 다른 동작/창으로 위임되는 요소엔 그 문서를 링크만** 한다. 임베드·복제 없음.

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

## 5. design ↔ code 연계 (claude design 협업)

디자인은 claude design 산출물(`design-system/`)이며 claude code 는 **직접 수정하지 않는다.** 시각 진실은 `design-system/` 이 소유하고 docs 는 **링크만** 한다(재서술 금지).

디자인을 바꿔야 하면 소스를 먼저 고치지 말고 **claude design 에 변경을 요청**하고, **변경된 디자인을 받아 재적용**한다. 받은 변경 내용은 아래 §6 배치 규칙대로 docs 에 흡수한다(동작→features, 시각→design-system 링크, 근거→ADR). 요청 제출·회수의 구체 워크플로는 [`dev-guide/design-change-workflow.md`](dev-guide/design-change-workflow.md) 에 정의돼 있다(요청문서 → 시안 → 정합 루프).

## 6. 작성 규칙 요약

- 새 화면/동작 → 해당 `features/<f>/` 의 기획·화면 문서에 흡수. 없으면 폴더 신설.
- **문서 종류에 맞는 내용만 넣는다 (배치 규칙).** 내부 구현(파일·함수 콜사이트, feature gate, 동작 배선)은 **적어도 된다 — 단 위치가 dev-guide 또는 기획문서(내부 동작)** 다. `agent-guide`(usage)는 *agent 가 tasty 로 무엇을 할 수 있나* 만 다루므로 거기엔 구현 정보를 넣지 않는다 (구현이 *틀린* 게 아니라 *그 섹션에 불필요* 한 것). 단 빌드/로드맵 상태(`Phase …`, `구현 예정`, `이관 상태`)는 transient 이므로 어디에도 두지 않는다 (현재 상태만).
- **마크다운 체크박스(task list)를 쓰지 않는다.** 목록 항목을 `[ ]`·`[x]` 로 시작하는 체크리스트 형식은 본질이 진행 추적(했다/안 했다)이라 위 transient 금지와 같은 이유로 docs 어디에도 두지 않는다 — 체크 상태에 정해진 의미가 없어 정보도 담지 못한다. Acceptance Criteria 는 평문 `Given … When … Then …` 불릿으로, 검증·절차 항목은 평문 불릿이나 번호 목록으로 적는다. 미구현 범위는 `Status` 줄과 본문 문장으로 적는다. `crates/tasty-doc-guards/tests/no_checkbox_in_docs.rs` 가 `docs/**/*.md` 전체에 강제한다 — `doc-guards.yml` 이 main push · PR 마다 자동으로 돌린다. 그 잡에는 **경로 필터가 없어 문서만 바뀐 push 에서도 돈다**([ADR-0138](adr/0138-doc-guards-live-in-a-dependency-free-crate.md) · [ci-gates](dev-guide/ci-gates.md)).
- **인용한 좌표는 따라갈 수 있어야 한다.** `src/…` · `tests/…` · `crates/…` 처럼 레포 경로 형태로 적은 파일은 실재해야 한다. 없으면 (a) 옮겨졌으면 현재 경로로 고치고, (b) 남의 저장소 경로면 크레이트 이름을 앞에 붙여(`egui/src/style.rs`) 우리 경로 형태에서 빼고, (c) 생성물이거나 실재한 적이 없으면 경로 인용 대신 서술로 적는다. **틀린 좌표는 주어 없는 문장보다 나쁘다** — 이름과 경로가 붙어 있으면 확인된 것처럼 보여 아무도 다시 세지 않는다. 크레이트 안의 문서(`crates/<이름>/README.md` 등)가 적는 `src/…` 는 그 크레이트 기준으로도 해석하므로 그대로 써도 된다. `crates/tasty-doc-guards/tests/cited_coordinates_exist.rs` 가 추적되는 모든 `*.md` 에 강제한다 — `doc-guards.yml` 이 main push · PR 마다 자동으로 돌린다.
- **좌표 없이 이름만 적은 인용은 자동 채널이 없다.** 백틱으로만 적은 식별자(테스트 이름 · 함수 이름)가 실재하는지는 아무 가드도 보지 않는다. 이름 바로 뒤 괄호가 파일을 지목하는 형태(`` `이름`(`경로`) ``)로 적으면 그 한 형태만 위 가드가 판정한다. 전량을 재려면 손으로 돌린다:

  ```bash
  comm -23 <(git grep -ho '`[a-z][a-z0-9_]*`' -- '*.md' | tr -d '`' | grep _ | sort -u) \
           <(git grep -hoE '[A-Za-z_][A-Za-z0-9_]*' -- '*.rs' '*.toml' '*.sh' '*.yml' | sort -u)
  ```

  이 명령의 함정 셋. **① 소스 모수에서 `*.toml` 을 빼면 안 된다** — plugin 매니페스트가 선언하는 이름(hook 인자 · action id 등)이 통째로 죽은 참조로 나온다. **② 결과는 답이 아니라 후보다** — 외부 크레이트 이름, clippy lint, egui API, 제거를 기록한 ADR 의 옛 이름이 정당하게 섞여 있다. 실제로 고쳐야 하는 것은 "이 테스트가 강제한다" 처럼 **우리 코드를 지목한 자리**뿐이다. **③ 통합 테스트 타깃은 파일 이름이라 소스 토큰 집합에 없다** — `tests/<이름>.rs` 를 확장자 없이 인용하면 실재하는데도 없는 것으로 나온다. 파일 이름(스템)도 모수에 넣어야 한다.
- **표는 렌더에서도 내용을 지켜야 한다.** 소스로는 멀쩡한데 렌더에서만 내용을 잃는 부류가 있고, 사람이 소스를 읽으면 다 보이므로 리뷰로도 안 잡힌다. 둘을 지킨다. **① 셀 안의 `|` 는 백슬래시로 이스케이프한다** — 코드 스팬 안이라도 셀을 쪼갠다(GFM 은 헤더가 열 수를 정하고 넘친 셀을 **버린다**). **② 표와 표 사이에 빈 줄을 둔다** — 붙이면 뒤 표의 헤더와 구분행이 앞 표의 본문 행으로 삼켜져 그 표가 통째로 렌더되지 않는다. `crates/tasty-doc-guards/tests/markdown_tables_render_whole.rs` 가 추적되는 모든 `*.md` 에 강제한다 — `doc-guards.yml` 이 main push · PR 마다 자동으로 돌린다. 그 가드가 **못 잡는 것**(코드펜스 짝, 헤더와 구분행의 열 수 불일치, 4 칸 이상 들여쓴 표, 링크·이미지, 셀 안 HTML)은 그 파일의 모듈 주석에 사전 등록돼 있다.
- **인덱스 행이 무엇을 거울처럼 싣는지가 폴더마다 다르다 — 섞지 않는다.** 실측(2026-09-06,
  `docs/**/index.md` 13 벌 · 표 행이 문서를 링크하는 짝 329 건):
  [`adr/index.md`](adr/index.md) 는 링크 문구로 **대상 문서의 제목**을 싣고(179 중 175 가
  일치), 나머지 12 벌은 **파일명 slug** 를 싣는다(150 중 2). 구조적 이유가 있다 — ADR
  인덱스만 별도 식별자 열(`#`)을 가져서 링크 문구가 제목을 실을 여유가 있고, 나머지는
  링크 자체가 식별자 노릇을 한다(그 문서들은 경로로 인용된다). **147 건은 어긋난 것이
  아니라 다른 관례다.** 그래서 "인덱스 행과 본문이 같아야 한다" 를 전 폴더에 일반 규칙으로
  걸 수 없다 — 어느 쪽이 짝인지는 모양이 아니라 관례가 정하고, 모양은 둘이 같다.
- **값을 두 곳에 두면 그 짝을 보는 가드도 같이 만든다.** 한쪽만 움직이는 것이 기본값이기
  때문이다 — 재sync 나 부분 개정에서는 본문만 올라가고 인덱스 행이 첫 커밋 값으로 남는다.
  ADR 인덱스 행의 `Status`·`Date` 는
  `crates/tasty-doc-guards/tests/adr_index_parity.rs` 가 본다(`Title`·`Tags` 는 정규화
  규칙이 아직 없어 범위 밖이며, 그 사유는 그 파일의 모듈 주석에 있다).
  **짝을 보는 가드가 아직 없는 자리가 그 밖에 남아 있다.**

  가드를 만들 때는 초록만 보지 말고 **한쪽 값을 흔들어 빨개지는지** 확인한다 —
  초록은 "짝을 보고 있다" 와 "그 열을 아예 안 읽는다" 둘 다와 양립한다.
- 시각 수치/토큰은 적지 말고 `design-system/` 을 링크.
- 결정의 근거는 본문에 길게 쓰지 말고 ADR 로 박고 링크.
- 합성 화면은 언급/링크로만 잇는다.
- 디자인 변경이 필요하면 소스를 먼저 고치지 말고 claude design 에 변경을 요청하고, 변경된 디자인을 받아 재적용한다 (구체 워크플로는 [`dev-guide/design-change-workflow.md`](dev-guide/design-change-workflow.md)).

## 관련

- [ADR-0006 — 문서 분류체계: 동작 우선](adr/0006-docs-taxonomy-behavior-first.md) (근거)
- [features/index.md](features/index.md) (기획·화면 카탈로그)
- [dev-guide/design-change-workflow.md](dev-guide/design-change-workflow.md) — 디자인 변경 워크플로(요청문서 → 시안 → 정합 루프)
