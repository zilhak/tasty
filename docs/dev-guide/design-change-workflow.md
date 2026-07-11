# 디자인 변경 워크플로 — 요청문서 → 시안 → 정합 루프

디자인에 **없는** UI 요소를 새로 만들거나, 디자인과 **다른** 형태로 바꿔야 할 때 따르는 절차다.
"소스부터 고치지 않는다" — 먼저 디자인을 *확보*하고, 받은 디자인을 갤러리·본체로 내린다.

> 디자인에 *이미 있는데* 소스만 못 따라간 경우(구현 누락/불일치)는 이 워크플로가 필요 없다 —
> 디자인 변경이 아니므로 바로 [gallery-first](gallery-first.md) 의 구조 전사(1단계)로 간다.

이 워크플로의 도구 3분할(Figma 기획 / Claude design 디자인 / claude code 구현)과 그 근거·재검토 조건은
[ADR-0025](../adr/0025-planning-tool-split-experimental.md) 에 있다. 본 문서는 그 결정의 *현재 운영 절차* 만 기술한다.

## 요청문서 전달 경로 (직접 접근 우선 / 로컬 fallback)

요청문서를 designer(Claude design)에게 넘기는 방법은 **claude design 프로젝트 직접 접근 수단(예: DesignSync MCP)의 유무**에 따라 갈린다. 어느 경로든 요청문서의 내용·구성(§0~§8)·changelog 는 동일하다.

| | **A. 직접 접근 가능 (우선)** | **B. fallback (직접 접근 없음/실패/미인증)** |
|---|---|---|
| 요청문서 위치 | 원격 프로젝트의 **요청 인박스 폴더**(프로젝트 컨벤션 — 예: `uploads/<slug>.md`)에 직접 write | 로컬 `.claude-workspace/design-request/MMDDhhmm-design-request-<slug>.md` |
| 넣는 주체 | claude code 가 write (요청 md 한정 — 실제 디자인 파일 산출은 아님) | claude code 가 로컬 파일로 작성 |
| designer 에게 전달 | claude code 가 `tasty design chat` 으로 **자동 지시**(동시성 lock 준수, 아래) — 또는 사용자가 직접 지시 | 사용자가 요청문서를 Claude design 에 직접 제출 |
| 시안 수령 | 갱신 디자인/`.DONE` 을 직접 접근 수단으로 읽어 정합(폴더 경로 수령 불필요) | 사용자가 받아온 디자인 폴더 경로를 넘겨받아 정합 |

- **직접 접근 수단으로 write 하는 대상은 요청 인박스의 요청 md 뿐**이다. 토큰/컴포넌트/UI kit 등 실제 디자인 산출물은 여전히 designer(Claude design)만 만든다 — claude code 가 직접 수정하지 않는다는 원칙은 불변.
- **A 경로 자동 지시**: `tasty design chat`(claude-design 플러그인)이 designer 에게 프롬프트를 보낼 수 있다(off-screen Playwright). 단 claude.ai/design 은 한 프로젝트에 **동시 한 turn** 만 허용하므로, 자동 지시는 반드시 아래 **동시성 lock 프로토콜**을 따른다. (플러그인이 없거나 사용자가 원하면 지시는 사용자 몫으로 두어도 된다.)
- 이 저장소의 직접 접근 설정(접근 수단·projectId 등)은 로컬 지침(`.claude/CLAUDE.md`)에 기록한다(세션 고유값이라 커밋 문서에 박지 않는다).
- 요청 인박스 폴더명·파일명은 **대상 프로젝트의 컨벤션**을 따른다(이 프로젝트는 `uploads/<slug>.md`). B(로컬)는 tasty 컨벤션 `MMDDhhmm-design-request-<slug>.md` 를 쓴다.

### 동시성 lock (A 경로 자동 지시 시 필수)

claude.ai/design 은 한 프로젝트에 동시 한 turn 만 허용해, 이전 turn 이 진행 중일 때 새 `design chat` 을 쏘면 "Your other tab is working on a request" 로 막힌다. busy 를 값싸게 물을 API 가 없어, **공유 저장소(디자인 프로젝트 파일)에 요청마다 상태 파일을 만들어** busy/완료를 판정하는 파일 기반 advisory lock 을 쓴다.

- **규약 전문·부트스트랩**: `tasty design protocol` (규약) / `tasty design protocol --bootstrap` (대상 프로젝트에 designer 규율 심는 절차). 플러그인이 정본을 노출한다.
- **발사 전**: 원격 `design-tasks/` 를 `list_files` 로 확인 → 비-stale `.WORKING` 있으면 발사 금지·대기.
- **완료**: 대응 `.DONE`(/`.FAILED`/`.NEEDS-INPUT`) 파일 등장으로 감지하고 그 파일에서 결과를 회수(브라우저 turn-end 스크래핑에 의존 X).
- **backstop**: `tasty design chat` 은 전송 직후 busy 배너를 감지하면 `state:"busy"` 로 즉시 반환(timeout 헛대기 X). `tasty design turn-status` 는 lock TTL 만료 시 turn liveness 재확인에 쓴다.

아래 다이어그램·표·라이프사이클은 **B(fallback)** 경로를 기준으로 그린 것이다. A 경로에서는 `[2] 사용자 제출`이 "claude code 가 원격 요청 인박스에 write → `tasty design chat` 으로 자동 지시(동시성 lock 준수)"로 대체되고, 시안 수령이 직접 읽기가 된다.

## 뱅글뱅글 도는 루프

```
   ┌──────────────────────── 재요청 (부족·불일치 발견 시) ────────────────────────┐
   │                                                                              │
   ▼                                                                              │
[1] planner ──작성──▶  디자인 요청문서  ──[2] 사용자 제출──▶  [3] Claude design       │
 (요청 산출물 정의)     (.claude-workspace/         (사용자가 직접 제출)    (고충실 HTML/CSS 시안)  │
                        design-request/*.md)                                  │            │
                                                                              │ 시안 수령   │
                                                                              ▼            │
                                            [4] 정합: Figma 회귀반영 → 갤러리 specimen → 본체 구현 ─┘
                                                (구조 전사 + 토큰 정합)
```

| 단계 | 행위자 | 하는 일 | 산출물 |
|------|--------|---------|--------|
| 1 | **planner** | 무엇을·어떻게 보이게 할지 정의한 **디자인 요청문서** 작성 | `.claude-workspace/design-request/MMDDhhmm-design-request-<slug>.md` |
| 2 | **사용자** | 요청문서를 **Claude design 에 직접 제출** | (제출) |
| 3 | **designer** (Claude design) | 색·간격·인터랙션 살아있는 **고충실 시안** 생성 | HTML/CSS 시안 (휘발성) |
| 4 | **구현** (claude code) | Figma 회귀반영 → 갤러리 specimen → 본체 반영 | 코드 + Figma 갱신 |
| → 재요청 | planner | 4 에서 부족·불일치가 드러나면 **추가 요청문서**로 다시 2 로 | 새/갱신 요청문서 |

루프인 이유: 한 번에 끝나지 않는다. 시안이 열린 결정(아래 §6)을 확정하거나
와이어프레임에 없던 구조를 드러내면, 그 변화를 Figma 기획에 되먹이고(ADR-0025 회귀 반영) 부족분은 다음 요청문서로 다시 돈다.

## 디자인 요청문서란

**planner 가 작성하는 입력 산출물.** "이번에 무엇을, 어떤 화면·컴포넌트·상태·인터랙션으로 보이게 할지"를
디자이너(Claude design)가 고충실 시안으로 옮길 수 있도록 정의한 문서다. *왜/구현 배선*이 아니라 **무엇을 어떻게 보이게 할지**만 담는다.

- **위치**: 전달 경로에 따라 다르다(위 "요청문서 전달 경로" 참조) — A(직접 접근)면 원격 프로젝트의 요청 인박스 폴더(프로젝트 컨벤션, 이 프로젝트는 `uploads/`), B(fallback)면 로컬 `.claude-workspace/design-request/`(gitignored, 커밋 대상 아님).
- **파일명 규칙**: `MMDDhhmm-design-request-<slug>.md` — 맨 앞에 **월일시분(MMDDhhmm)**, 이어서 **`design-request`**, 마지막에 내용 slug 를 붙인다(예: `07101430-design-request-explorer-file-manager.md`). 날짜·시간을 선두에 둬 파일이 시간순으로 정렬되게 하고, `design-request` 접두로 문서 종류를 명시한다.
- **기준 구성**: 아래 §0~§8 구성을 그대로 따른다.

### 요청문서 구성 (§0~§8)

| 섹션 | 내용 |
|------|------|
| 헤더 메타 | 작성일 · 요청자(planner) · 수행자(designer) · **상태** · 연계 계획 · 파이프라인(ADR-0025) |
| §0 한 줄 요약 | 이 요청이 만들려는 것 한 줄 |
| §1 맥락 / 제약 | 토큰=코드 SoT(Catppuccin Mocha, raw hex 금지) · 4px 그리드/14px 폰트 상한/1px 보더 · gallery-first 부품 재사용 · i18n 가변폭 · 기존 구현과의 관계 |
| §2 인벤토리 | 디자이너가 만들 화면/컴포넌트/팝업/상태 목록 표 (신규/기존 구분) |
| §3 화면별 요구사항 | 화면 단위 레이아웃·영역 구성 |
| §4 컴포넌트별 요구사항 | 컴포넌트 단위 상태·변형 |
| §5 인터랙션 / 동작 | 시안에 주석으로 달 조작·상태전이 규칙 |
| §6 열린 결정 | 디자이너/사용자가 확정해야 할 미결 사항 |
| §7 산출물 형식 | 디자이너에게: 고충실 HTML/CSS, 상태 변형 포함, 토큰 안에서만, 회귀 반영·구현 친화 박스 경계 |
| §8 참고 | Figma 와이어프레임 nodeId, 계획 문서, 토큰/팝업 정책 링크 |

### 요청문서가 강제하는 디자인 규칙 (시안 단계부터)

요청문서 §1·§7 은 시안이 본체 토큰·정책과 어긋나지 않도록 다음을 **시안 단계에서부터** 못박는다:

- **토큰 SoT = 코드(`Theme`)** — 색·간격·치수·보더는 모두 Catppuccin Mocha 토큰 안에서만. raw hex 하드코딩 금지(시안에서도 토큰 의미 이름으로 표기). Figma Foundations 는 미러. ([theme UI 규칙](../design/systems/theme.md#ui-디자인-규칙-필수))
- **레이아웃 규칙** — 4px 그리드 · 폰트 14px 상한 · 보더 1px · 호버(흰색 +8%)/액티브(+12%) 오버레이는 직접 값 금지(자동 도출).
- **gallery-first 부품 재사용** — 보편 부품(버튼/입력/표/스크롤바/컨텍스트 메뉴/팝업)은 [공용 위젯](../design/policies/shared-widgets.md)·기존 카탈로그와 시각 일관. ([gallery-first](gallery-first.md))
- **i18n 가변폭** — 모든 문자열은 [`t()`](i18n.md) 번역 키로 노출될 예정. 시안 텍스트는 영어 기준이되 독/일/한 가변 길이를 감안해 여유 폭.

## 상태 라이프사이클

요청문서 헤더 메타의 `상태:` 필드로 추적한다.

```
requested ──(사용자 제출·시안 수령)──▶ received ──(Figma/갤러리/본체 정합)──▶ reconciled
    ▲                                                                          │
    └────────────────── re-requested (부족·불일치 발견 시 새 요청으로) ─────────────┘
```

| 상태 | 의미 |
|------|------|
| `requested` | planner 가 요청문서를 작성·확정. 아직 시안 없음 (제출 대기). |
| `received` | 사용자가 Claude design 시안을 받아옴. 정합 작업 대기/진행. |
| `reconciled` | 시안을 Figma 회귀반영 + 갤러리 specimen + 본체에 반영 완료. |
| `re-requested` | 정합 중 부족·불일치가 드러나 추가/갱신 요청문서로 루프 재진입. |

## 누가 무엇을 하나 (역할 분리)

| 역할 | 누가 | 책임 |
|------|------|------|
| **planner** | claude code(todo-maker 겸) 또는 사용자 | 요청문서 작성 — 인벤토리·제약·열린 결정 정의. Figma 기획(와이어프레임/IA/플로우) 유지. |
| **사용자** | 사람 | 요청문서를 **Claude design 에 직접 제출**하고 시안 결과를 받아온다. (이 제출은 사용자 행위 — 에이전트가 대행하지 않는다.) |
| **designer** | Claude design (claude.ai Artifacts) | 고충실 HTML/CSS 시안 생성. 색·치수는 요청문서가 준 토큰 팔레트 안에서만. |
| **구현** | claude code | 확정 시안을 [gallery-first](gallery-first.md) 순서로 — Figma 회귀반영 → 갤러리 specimen(구조 전사+토큰 정합) → 본체 반영. |

> Claude design 산출물은 **휘발성**(세션 종료 시 사라짐)이다. 확정 시안은 스크린샷을 Figma Screens 페이지에
> "확정 시안 아카이브"로 박거나 HTML 을 `.claude-workspace/`·`docs/design/` 에 보존한다(ADR-0025).

## 관련

- [ADR-0025](../adr/0025-planning-tool-split-experimental.md) — 도구 3분할(Figma/Claude design/claude code) 결정 근거·회귀 반영 루프·재검토 조건. (Experimental)
- [ADR-0020](../adr/0020-gallery-complete-component-source.md) — 갤러리 완전성 + gallery-first 결정 근거.
- [gallery-first](gallery-first.md) — 확보한 디자인을 갤러리→본체로 내리는 순서(0단계 "디자인 확보"가 이 워크플로).
- [design/policies/gallery-completeness](../design/policies/gallery-completeness.md) — cut 금지 시 디자인 보강(=이 워크플로로 재요청).
- [design/systems/design-parity-notes](../design/systems/design-parity-notes.md) · [design-gallery-mapping](../design/systems/design-gallery-mapping.md) — 시안→소스 구조 전사 원칙·매핑.
- [theme.md "UI 디자인 규칙"](../design/systems/theme.md#ui-디자인-규칙-필수) — 토큰 정합 규칙(시안부터 적용).
