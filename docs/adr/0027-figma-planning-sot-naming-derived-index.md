# ADR-0027: Figma 기획 파일의 SoT·네이밍 규약과 파생 인덱스 (anti-drift 유지 구조)

- **Status**: Accepted (2026-07-14 일부 개정 — 구분자 `/` 금지 해제, §2·§3 참조)
- **Date**: 2026-06-28
- **Tags**: figma, planning, naming-convention, source-of-truth, anti-drift, sigma, spellbook, workflow, adr-0025

## Context

[ADR-0025](0025-planning-tool-split-experimental.md) 로 Figma 기획 파일(8 페이지)을 운영해 보니, 시간이 지나며 **기획 기록이 썩기 시작**했다. 구체 증상:

- **이주 고고학(migration archaeology)** — `3:3`/`3:6` 페이지 메모(spellbook scroll)가 `⚠ 이동 공지(트랙 D)` + "본문은 이동 전 기준, 현재 위치는 37:1079" 로 덮여, cold-start 에이전트가 본문과 정정 공지를 머릿속에서 diff 해야 한다.
- **세션/트랙 식별자 누수** — `T7`/`T8`/`트랙 D` 같은 작업 세션 이름과 `.claude-workspace/conductor/T8/` 같은 휘발 경로가 영속 기록에 박혀, 미래 에이전트가 해소할 수 없는 노이즈가 됐다.
- **상태의 산란** — "Explorer 26:3 = 디자인 입력 확정" 같은 *결정 상태*가 페이지 scroll 본문 깊숙이 묻혀, 전체 진행 상태를 알려면 9 개 scroll 을 다 읽어야 한다.
- **nodeId 취약** — scroll 의 `nodeId↔의도` 매핑이 재import/삭제로 깨져, "사용 직전 get_tree 로 재검증" 이라는 단서를 매번 달아야 한다.

근본 원인은 하나다: **파생 데이터(derived)를 사람/AI 가 손으로 유지**하기 때문이다. 같은 사실의 두 번째 복사본(scroll 본문의 nodeId 매핑 vs 실제 Figma, Cover page-map vs 실제 페이지)이 존재하면 반드시 갈라지고, 갈라지면 "정정 공지" 를 덧붙이게 되어 고고학이 쌓인다. CLAUDE.md 의 docs 규칙(`구현 히스토리 남기지 않는다 · 현재 상태만`)을 spellbook 이 사실상 어기고 있었다.

이를 고치려면 "한 번 정리" 가 아니라 **유지 규칙(invariant)** 이 필요하다. 그리고 그 규칙이 공염불이 되지 않으려면 sigma 도구로 실제 강제 가능해야 한다. 그래서 채택 전에 라이브 검증(아래 Decision 의 "검증된 도구 사실")을 먼저 수행했다.

## Decision

Figma 기획 파일의 유지를 **"사실 1개당 SoT 1곳, 나머지는 생성(derive), 현재 상태만, 역사는 다른 곳"** 원리로 고정한다. 토큰을 code→Figma 단방향으로 미러하는 기존 원칙([ADR-0025](0025-planning-tool-split-experimental.md))을 파일 운영 전반으로 일반화한 것이다.

### 1. 사실의 종류별 SoT 배치

| 사실 종류 | SoT (authored, 손으로 쓰는 곳) | 파생물 (생성, 손대지 않는 곳) |
|---|---|---|
| 정체성 (이 노드가 뭔가) | **Figma 노드 이름 = 안정 slug** | — |
| 위치/구조 (어느 page/frame) | (없음 — tree-walk 로 알아냄) | 생성된 `_index` 구조표 |
| 상태 (plan/design/built…) | **노드 이름의 상태 접두 토큰** | 생성된 `_index` PLAN STATUS 표 |
| 의도 (durable 스펙) | **노드당 annotation / scroll 의 의도 블록 1개** | — |
| 근거/역사 (왜 옮겼나·대안) | **ADR + Figma version history** | — |
| 기계 (재현 스크립트·폰트 함정) | **repo (`.claude-workspace/` · dev-guide)** | — |

### 2. 네이밍 규약 (린치핀)

frame/section 이름에 상태와 slug 를 박는다. **slug 가 SoT 이고, nodeId 는 일회성 핸들이다.**

```
<status-emoji> <kind>.<slug>

status-emoji (= ADR-0025 파이프라인 단계):
  🔵 plan     구조/와이어 완료, 디자인 대기
  🎨 design   고충실 시안 완료, 구현 대기
  ✅ built     gallery/code 반영됨
  🗄 parked    Archive(보류/폐기)
  🔒 mirror    code 가 SoT (토큰·기존 컴포넌트) — 워크아이템 아님

kind: screen / wire / flow / token / comp / overlay …
구분자 = '.'  (첫 점 앞 = kind, 뒤 = slug 경로)

예:  🎨 screen.settings   ✅ comp.overlay.modal   🔵 wire.explorer   🔒 token.color
```

**구분자 `/` 는 허용된다 (2026-07-14 개정).** 금지의 근거였던 find_node 충돌이 해소됐다 —
sigma `find_node` 의 **배열 path**(`["icon/arrow/left"]`, 원소 = 리터럴 이름 매칭)와
`get_tree` `namePattern` 모두 슬래시 포함 이름을 정확히 조회함이 라이브 검증됐다.
기존 `.` 표기는 그대로 유효하며(재명명 불요·계속 권장), `/` 는 **Figma Assets 패널
그룹핑이 필요한 컴포넌트**(아이콘 세트 등 — 이름 슬래시가 폴더 계층으로 렌더됨)에 쓴다.
잔존 주의: `get_tree` 의 `fullPath` 문자열 표시에서는 이름 속 `/` 가 계층처럼 보인다
(조회 정확성과는 무관한 표시 모호성).

### 3. 조회·생성 도구 규약 (검증된 도구 사실)

라이브 검증(임시 frame 생성→조작→삭제, 실제 노드 무손상)으로 확정한 sigma 동작:

- **조회 = `sigma_get_tree` + `filter.namePattern`(정규식).** slug 로 노드+현재 nodeId 를 회수한다. ✅ 동작.
- **`sigma_find_node` 는 보조 lookup.** (2026-07-14 개정) 채택 당시의 두 결함이 모두 해소됐다 — ① page-직속 frame 조회 실패는 sigma 에서 수정됨(바인딩 page 미반영 버그), ② 이름 속 `/` 충돌은 **배열 path**(원소 = 리터럴 이름, 쪼개지 않음)로 회피 가능함이 검증됨. 문자열 path 만 `/` 를 계층 구분자로 쪼개며, 실패 시 sigma 가 배열 형태를 제안하는 힌트를 반환한다. 단 exact full-name 매칭 특성상(status 이모지까지 알아야 함) **status-무관 조회는 여전히 get_tree namePattern 이 1차**다.
- **`sigma_modify_node` 의 `setPluginData`/`getPluginData`/`getPluginDataKeys` 동작.** pluginData 는 노드에 keyed 되어 **rename 을 거쳐도 생존**(이름 churn 면역). 단 재import 는 새 노드를 만들므로 pluginData 는 소실 → 생성기가 재적용해야 한다. 그래서 **slug+status 의 1차 SoT 는 (가시·diff 가능한) 노드 이름**이고, pluginData 는 비표시 메타(예: 핸드오프 문서 링크, 근거 ADR 번호)용 *보조* 저장소다.
- **상태 수집 가능.** get_tree 가 이모지 접두 포함 이름을 반환하므로, 전 페이지를 walk 해 `_index` 의 구조표·STATUS 표를 기계 생성할 수 있다.

### 4. 파생 인덱스 = 생성물 (Cover / `_index`)

- **Cover page-map 과 `_index` 의 구조·STATUS 표는 생성기(`.claude-workspace/temp/gen-index.js` 류)가 전 페이지를 walk → 이름 토큰 파싱 → 덮어쓴다.** 사람은 절대 손편집하지 않는다.
- Cover 와 `_index` 는 **진실이 아니라 진실을 비추는 view** 다 — 내비게이션 역할만 한다. (좋은 디자인 시스템 파일이 첫 페이지에 Cover/README 를 두되 그것을 SoT 로 삼지 않는 것과 같다.)

### 5. 유지 불변식 (6개)

1. **사실 1개 = SoT 1곳.** (정체성=이름, 의도=annotation/scroll 의도블록, 상태=이름 토큰, 근거=ADR, 기계=repo)
2. **파생물은 생성만, 손편집 금지.** Cover page-map · `_index` 표.
3. **현재 상태만.** "옮겼다/바뀌었다" 서술 금지 — 이동 사유는 ADR 또는 Figma version history 로.
4. **nodeId 를 영속 텍스트에 쓰지 않는다.** 항상 slug, 사용 직전 get_tree namePattern 으로 해소.
5. **세션/트랙 식별자(T7·트랙 D…)·휘발 경로는 영속 기록 금지.** 박제가 필요하면 ADR 번호로.
6. **scroll 은 "의도" 만 담는다.** 상태는 생성물로, 기계는 repo 로 분리 → scroll 이 얇아지고 안 썩는다.

## Consequences

- **얻은 것**:
  - nodeId 취약 원천 소멸 — slug 가 SoT 라 nodeId 는 마음껏 churn 해도 된다.
  - 진행 상태가 한 표(`_index` STATUS)로 롤업 — 9 개 scroll 정독 불요.
  - 이주 공지·세션 토큰이 쌓일 자리가 없어짐 — 규칙 3·5 가 입구를 막는다.
  - 규약이 도구로 강제 가능함을 채택 전 검증 — ADR 이 공염불이 아니다.
- **잃은 것 / 위험**:
  - **기존 8 scroll + Cover 의 1 회 리라이트 비용** — 이주 공지/T-토큰 제거, 의도-only 로 축소, 이름에 상태 토큰 부여.
  - **생성기 미작성 동안은 파생물이 수동** — gen-index 가 나오기 전까지 Cover/`_index` 는 임시로 손유지될 수 있다(규칙 2 위반 상태). 생성기 완성이 본 ADR 의 완결 조건.
  - **pluginData 는 재import 에 약함** — 보조 메타는 import 파이프라인이 재적용하도록 생성기 입력에 포함해야 한다.
- **운영 비용 / 유지 부담**:
  - 매 구조 변경 후 **gen-index 1 회 실행** 이 새 의식(ritual)으로 추가된다(저렴 — tree-walk).
  - 네이밍 규약 준수를 사람이 지켜야 한다 — 위반은 생성기가 "미분류 노드" 로 리포트해 드러낸다.

## Alternatives Considered

- **`find_node` 로 slug 조회**: 검증에서 page-직속 frame 을 못 찾음(깨끗한 이름도) → 신뢰 불가. get_tree namePattern 으로 대체.
- **구분자 `/`**: (채택 당시) find_node 경로 구분자 충돌 + get_tree `fullPath` 에서 가짜 계층으로 표시됨 → `.` 채택. (2026-07-14) 전자는 find_node 배열 path 로 해소되어 `/` 금지는 해제(§2 개정) — fullPath 표시 모호성만 잔존.
- **slug+status 의 1차 SoT 를 pluginData 로**: 비표시라 사람이 audit/​diff 하기 어렵고, 재import 에 소실됨. 가시성·도구친화성에서 이름이 우월 → pluginData 는 보조로 강등.
- **Cover/`_index` 를 손 유지**: 세 번째 드리프트 복사본을 만든다 — 바로 그 병의 재발. → 생성물로 고정.
- **code/JSON(생성기 입력)을 단일 SoT 로, Figma 는 순수 출력**: 가능하지만 ADR-0025 의 "Figma 에서 기획한다" 와 충돌하고, 손으로 Figma 를 만지는 현재 워크플로(직접 도구 편집)를 부정한다. 현 단계에선 **이름=SoT(가시·hand-editable) + 생성 view** 의 절충을 택한다.

## Reconsideration Triggers

다음 중 하나가 충족되면 본 ADR 을 재검토한다.

- **[ADR-0025](0025-planning-tool-split-experimental.md) 의 Figma 기획 단계가 제거되면** → 본 ADR 도 함께 폐기(Figma 파일 자체가 없어지므로 무의미).
- sigma 가 `find_node` 의 page-직속 frame 조회를 고치면 → 조회 도구 규약(3)을 재검토(find_node 복권 가능). **(발화·반영됨, 2026-07-14 — find_node 보조 복권 + 배열 path 검증 + `/` 금지 해제)**
- pluginData 가 재import 에도 보존되는 경로(예: import 가 pluginData 를 실어 보냄)가 생기면 → slug+status 의 SoT 를 pluginData 로 승격 검토.
- 네이밍 규약 위반이 반복적으로 누적되면 → 생성기에 lint/거부(reject) 단계를 추가하거나, 규약을 단순화.
- gen-index 생성기가 끝내 작성되지 않으면 → 파생물 자동화 전제가 무너지므로 규칙 2(파생물 손편집 금지)를 완화하거나 본 ADR 을 축소.

## References

- [ADR-0025: 기획 단계 도구 3분할 — 실험적](0025-planning-tool-split-experimental.md) — 본 ADR 의 상위 우산
- [ADR-0020: 갤러리는 본체 UI 컴포넌트의 완전한 단일 출처 — gallery-first](0020-gallery-complete-component-source.md)
- [ADR-0006: 문서 분류체계 — 동작 우선](0006-docs-taxonomy-behavior-first.md) — 현재 상태만 기술하는 docs 원칙
- spellbook: `category=figma-planning, sub_category=tasty` (페이지별 scroll + `_index`)
- Figma 기획 파일: https://www.figma.com/design/ct3uPefwY2uk6i1i9wYpkU/Untitled
- 검증 근거: 본 ADR 채택 전 라이브 테스트(임시 frame 41:1080 생성→rename×3→pluginData R/W→삭제, Archive 페이지 무손상 복구).
