# 토스트 시스템

**Toast** 는 View 내부에 잠깐 떴다가 자동으로 사라지는 휘발성 피드백 UI 다 — "복사됨", "저장됨" 같은 사용자 동작의 즉각 결과. `PopupManager` 가 아니라 별도 `ToastManager`(`src/adapters/ui/toast.rs`)로 관리된다. 용어 구분은 [concepts/ubiquitous-language](../../concepts/ubiquitous-language.md).

## Popup 과의 차이

| 항목 | Popup | Toast |
|------|-------|-------|
| 수명 | 사용자가 닫을 때까지 | 일정 시간 후 자동 소멸(기본 2s) |
| 포커스 | 클릭 시 보유 가능 | **절대 받지 않음** |
| 입력 | 클릭/드래그/X | **소비 안 함**(마우스 통과) |
| 타이틀바 | 있음 | 없음(본문만) |
| 위치 | 자유 이동 | 스코프별 고정 스택 |
| 트리거 | 사용자 또는(debug)에이전트 | **사용자 행동만** |

Toast 는 Popup 의 변종이 *아니다* — 7대 규칙(타이틀바·X·드래그·z-order 승격·외부클릭닫기)이 토스트와 정면 충돌하므로 별도 매니저로 둔다. 단 스코프 정의와 스코프-rect 계산은 `LayoutContext` 를 재사용해 일관성을 유지한다.

## 트리거 정책 (CRITICAL)

**Toast 는 사용자 행동(키보드 단축키 / 마우스)에서만 발사된다.** CLI/IPC 를 통한 에이전트 동작은 사용자 시각 상태에 영향을 주면 안 되므로 토스트를 띄우지 않는다([identity](../../identity.md) 원칙 1, [popup.md](popup.md) 발화 정책과 동일).

복사 예시:
- 터미널 선택 후 `Ctrl+C` → ✅ · Explorer 경로 복사 → ✅ · 클립보드 뷰어에서 항목 클릭 복사 → ✅
- IPC `clipboard.*` 쓰기 → ❌ · OSC 52(터미널 프로그램이 보낸 클립보드 시퀀스) → ❌ (사용자가 직접 누른 게 아님)

## 스코프

Popup 과 같은 enum 을 쓰지만 **위치 앵커 용도** 다(가시성 필터 역할은 거의 없음 — 어차피 짧게 떴다 사라짐). `ToastScope`: `Window` / `Workspace(usize)` / `Pane(u32)` / `Surface(u32)`. 기본은 `Surface`(어디서 일어난 일인지 모르면 `Window`). 같은 스코프 내 여럿이면 아래에서 위로 쌓고, 스코프가 화면에서 사라지면 즉시 제거.

## 시각 / 레이아웃

모든 색·치수는 Theme 토큰([theme.md](theme.md)). 배경 `surface0` + 1px `surface1` 보더 + `corner_radius`, 본문 `font_size_body`, 스코프 우측 하단 정렬·스택. 종류 강조는 좌측 4px 컬러 바:

| 종류 | 바 색 | 용도 |
|------|-------|------|
| Info | `blue` | 일반(기본) |
| Success | `green` | 완료 |
| Warning | `yellow` | 주의 |
| Error | `red` | 실패 |

> 페이드(등장/소멸 알파만, 위치 이동 없음)는 적용된다 — theme.md 의 "터미널 콘텐츠 애니메이션 0ms" 규칙은 **터미널 콘텐츠** 한정이라 비-터미널 알림 UI 에는 적용되지 않는다.

## 입력 — 소비하지 않음

Toast 위에서 마우스 클릭/드래그해도 토스트는 무시하고 이벤트가 아래 레이어(터미널/popup/divider)로 통과한다. `popup_hovered` 도 토스트 영역에선 false. 키보드 포커스도 받지 않아 `has_focused()` 와 무관.

## 합치기 / 제한

같은 스코프에서 같은 메시지가 짧은 시간(기본 500ms) 내 재발사되면 새로 만들지 않고 **기존 토스트 수명만 갱신**(연속 Ctrl+C 깜빡임 방지). 스코프당 최대 동시 5개, 초과 시 가장 오래된 것 즉시 제거.

본문은 **200자(유니코드 문자 기준)** 로 제한한다. 초과 시 앞 200자만 남기고 줄바꿈 + 안내 접미(`toast.char_limit_notice`)를 붙여(`<앞 200자>\n(200자 제한)`) 비정상적으로 긴 입력이 토스트를 세로로 폭주시키는 것을 막는다. 길이/자르기는 char 경계로 처리해 멀티바이트에서 안전하며, coalesce 비교 이전(`push` 진입부 `truncate_message`)에 적용된다.

## 구조 (`src/adapters/ui/toast.rs`)

- `ToastKind` — Info / Success / Warning / Error.
- `ToastScope` — 위 enum.
- `ToastState` — id, message, kind, scope, spawned_at, lifetime.
- `ToastManager` — `push(message, kind, scope)` / `push_info(...)` / `draw(ctx, LayoutContext)`(만료 제거 + 렌더). `AppState::toasts` 로 통합, draw 는 popup draw 직후(= 위 레이어)에서.

모든 토스트 문자열은 `t("toast.*")` 키 — `lang/{en,ko,ja}.toml` 세 파일 동시 추가([i18n](../../dev-guide/i18n.md)).

## 관련

- [popup.md](popup.md) — 내부 팝업 시스템
- [identity](../../identity.md) — 사용자/에이전트 행동 분리
