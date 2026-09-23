# 토스트 시스템

**Toast** 는 View 내부에 잠깐 떴다가 자동으로 사라지는 휘발성 피드백 UI 다 — "복사됨", "저장됨" 같은 사용자 동작의 즉각 결과. `PopupManager` 가 아니라 별도 `ToastManager`(`src/adapters/ui/toast.rs`)로 관리된다. 용어 구분은 [concepts/ubiquitous-language](../../concepts/ubiquitous-language.md).

## Popup 과의 차이

| 항목 | Popup | Toast |
|------|-------|-------|
| 수명 | 사용자가 닫을 때까지 | 일정 시간 후 자동 소멸(기본 2s, Settings › General › Overlay 의 Toast duration 으로 1~10s 조절) |
| 포커스 | 클릭 시 보유 가능 | **절대 받지 않음** |
| 입력 | 클릭/드래그/X | **소비 안 함**(마우스 통과) |
| 타이틀바 | 있음 | 없음(본문만) |
| 위치 | 자유 이동 | 스코프별 고정 스택 |
| 트리거 | 사용자 또는(debug)에이전트 | 사용자 결과와 아래 허용·예외 상황 |

Toast 는 Popup 의 변종이 *아니다* — 8대 규칙(타이틀바·X·드래그·z-order 승격·외부클릭닫기)이 토스트와 정면 충돌하므로 별도 매니저로 둔다. 단 스코프 정의와 스코프-rect 계산은 `LayoutContext` 를 재사용해 일관성을 유지한다.

## 트리거 정책 (CRITICAL)

토스트는 기본적으로 사용자 조작의 결과를 알린다. 에이전트의 CLI·IPC 요청 성공·실패는 사용자 토스트로 표시하지 않는다. [정체성 원칙](../../identity.md)의 사용자 상태 분리를 따른다.

| 상황 | 처리 |
|---|---|
| 사용자 복사·저장 | 토스트 표시 |
| 에이전트 IPC 쓰기·intent 적용 실패 | IPC 응답 또는 경고 로그. 사용자 토스트 없음 |
| mirror 연결 끊김·재연결·손실 | 연결 상태 토스트 허용. parked engine에서는 표시하지 않음 |
| 사용자 구조 변경의 원격 적용 실패 | 실패 토스트 표시 |
| agent origin 또는 직접 IPC 구조 요청의 원격 실패 | op_id로 origin을 기억해 토스트 대신 로그 |
| 원격 markdown 잘림 | 최초 열기·실패 후 변경 재조회·사용자 새로고침에는 알려진 예외로 표시. markdown.reload IPC 재조회에는 로그만 기록 |
| OSC 52 클립보드 쓰기 | 보이지 않는 클립보드 변경을 알리기 위해 표시. PTY 바이트만으로 사용자·에이전트를 구분할 수 없는 알려진 예외 |

연결 상태 알림은 사용자가 보는 원격 사본이 최신인지 알려 준다. 세션을 에이전트가 열었더라도 이후 네트워크 단절은 IPC 호출의 직접 결과와 구분한다. 반대로 에이전트가 요청한 구조 변경의 거절은 연결 상태 예외로 허용하지 않는다.

### origin이 적용되는 경로

`report_apply_error`와 preset 적용·저장 실패는 사용자 origin에서만 토스트를 낸다. 에이전트의 forward 요청에는 `silent_failure`를 붙이고 attach client의 `AgentRequests`가 회신까지 op_id를 보관한다. 성공·실패 회신 뒤에는 항목을 지우고 재연결 때도 비운다. preset 저장 성공 알림은 이 실패 처리 규칙의 대상이 아니다.

직접 `Core::apply`를 부르는 split·tab.create/close/move·pane.close·surface.close는 `structural_exec::apply_as_agent`에서, image.open은 `image::handle_open`에서 같은 표시를 붙인다. 원격 요청을 다시 전달하는 경우도 이 기계 앞 사용자의 조작이 아니므로 조용한 실패로 처리한다.

`file_handler.dispatch`는 사용자가 조작한 플러그인 팝업을 검증할 수 있으면 사용자 origin이 된다. markdown 파일열기 팝업이 이 경로다. origin을 구분하지 못하는 기존 `Some(pane)` 직접 적용과 사용자 전용 `forward_mirror_structural`은 원격 실패 토스트를 유지한다. accepted 응답 뒤 적용한 실패 사유는 기존 응답에 소급해 넣을 수 없어 로그에 남는다.

관련 검사는 `src/intent/apply_error_tests.rs`, `src/app/attach_client/agent_origin.rs`, markdown 원문 요청 테스트에 있다. 이 테스트들이 모든 토스트 생성 경로를 검사하는 것은 아니다.

### 새 경로를 검토할 때

직접 호출은 다음 검색으로 찾을 수 있다.

```bash
rg -n 'toasts|report_apply_error|push_toast' src/adapters/ipc src/app/ipc src/app/ipc.rs src/boot/headless_dispatch.rs src/boot/headless_plugins.rs -g '!**/*_tests.rs'
```

검색 결과가 없더라도 IPC가 만든 winit 이벤트·도메인 이벤트·PTY 출력·intent를 따라 다른 파일에서 토스트가 발생할 수 있다. 직접 호출 검색을 정책 전체의 검증으로 보고하지 않는다. 새 실패 경로는 실제 origin을 확인하는 지점과 비동기 회신까지 함께 검사한다.

손실 경고와 재연결 성공은 서로 다른 메시지라 반복해서 뜰 수 있다. 느린 연결에서 방해가 된다면 같은 창에서 일정 시간 발생 횟수를 세어 별도 상태 표시로 옮길지 판단한다. markdown 잘림도 문서 내 표지로 옮기는 결정은 아직 하지 않았다.

## 스코프

Popup 과 같은 enum 을 쓰지만 **위치 앵커 용도** 다(가시성 필터 역할은 거의 없음 — 어차피 짧게 떴다 사라짐). `ToastScope`: `Window` / `Workspace(usize)` / `Pane(u32)` / `Surface(u32)`. 기본은 `Surface`(어디서 일어난 일인지 모르면 `Window`). 같은 스코프 내 여럿이면 아래에서 위로 쌓고, 스코프가 화면에서 사라지면 즉시 제거.

## 시각 / 레이아웃

모든 색·치수는 Theme 토큰([theme.md](theme.md)). 배경 `surface0` + 1px `surface1` 보더 + `corner_radius`, 본문 `font_size_body`, 스코프 우측 하단 정렬·스택. 종류 강조는 `toast_accent_width`의 좌측 컬러 바:

| 종류 | 바 색 | 용도 |
|------|-------|------|
| Info | `blue` | 일반(기본) |
| Success | `green` | 완료 |
| Warning | `yellow` | 주의 |
| Error | `red` | 실패 |

> 페이드(등장/소멸 알파만, 위치 이동 없음)는 적용된다 — theme.md 의 "터미널 콘텐츠 애니메이션 0ms" 규칙은 **터미널 콘텐츠** 한정이라 비-터미널 알림 UI 에는 적용되지 않는다.

**경계 처리**: 토스트의 모든 변은 자기 스코프 `scope_rect` 안에 머문다. ① `max_width` 는 surface 안쪽 폭(`width - 2*margin`)으로 클램프해 좁은 surface 에서 좌측 누출을 막고(정상 폭 surface 에선 0.8 폭이 그대로라 시각 무변경), ② 스택이 스코프 상단을 넘으면 더 오래된 토스트는 그리지 않으며, ③ scope 경계로 painter 를 클립해 1px 단위 누출까지 차단한다. 우측 하단 정렬 자체는 유지된다.

## 입력 — 소비하지 않음

Toast 위에서 마우스 클릭/드래그해도 토스트는 무시하고 이벤트가 아래 레이어(터미널/popup/divider)로 통과한다. `popup_hovered` 도 토스트 영역에선 false. 키보드 포커스도 받지 않아 `has_focused()` 와 무관.

## 합치기 / 제한

같은 스코프에서 같은 메시지가 짧은 시간(기본 500ms) 내 재발사되면 새로 만들지 않고 **기존 토스트 수명만 갱신**(연속 Ctrl+C 깜빡임 방지). 스코프당 최대 동시 5개, 초과 시 가장 오래된 것 즉시 제거.

본문은 **200자(유니코드 문자 기준)** 로 제한한다. 초과 시 앞 200자만 남기고 줄바꿈 + 안내 접미(`toast.char_limit_notice`)를 붙여(`<앞 200자>\n(200자 제한)`) 비정상적으로 긴 입력이 토스트를 세로로 폭주시키는 것을 막는다. 길이/자르기는 char 경계로 처리해 멀티바이트에서 안전하며, coalesce 비교 이전(`push` 진입부 `truncate_message`)에 적용된다.

캡 값의 출처는 `tasty-i18n` 의 `TOAST_MAX_CHARS` **하나**다. 캡에 맞춰 문구를 만드는 쪽(`fit_fragment` · `t_fmt_fit`)과 캡을 집행하는 쪽(`truncate_message`)이 같은 상수를 읽고, 접미가 말하는 숫자도 번역문의 `{}` 에 그 값이 들어가 만들어진다. 셋 중 하나만 바뀌어 "맞췄는데 잘린다" 거나 "200자 제한이라고 적혀 있는데 실제는 아니다" 가 되는 상태를 만들 수 없다. 번역문에 숫자가 다시 박히는 것은 `tests/toast_message_fits_cap.rs` 가 막는다.

> **길이를 모르는 조각(경로·실패 사유)을 실을 때는 `push` 전에 미리 줄인다** — `tasty_i18n::t_fmt_fit` / `fit_fragment` 가 번역된 틀은 그대로 두고 **조각의 가운데만** 생략해 캡 안에 맞춘다. 호스트의 기본 잘림은 **꼬리를 버리므로**, 경로면 어느 파일인지가, 실패 사유면 OS 에러가, 문장이면 "어떻게 하라" 는 지시가 사라진다. 소비자: 언어팩 폴백 경고(`LoadReport::user_warning`), 설정의 bashrc 저장 실패(`toast.bashrc_save_failed`).

## 구조 — 그리기와 상태가 다른 크레이트에 있다

**그리기**는 `crates/tasty-ui-widgets/src/toast.rs` 가 소유한다. 본체와 갤러리 specimen 이
같은 레이아웃·색·그리기 함수를 사용한다. 입력 폭·테마·배율이 같아야 같은 화면이 된다.

- `ToastEntryView` / `ToastScopeView` / `ToastViewProps` — 그릴 준비가 끝난 입력. 시간도
  상태도 안 들어 있고 `alpha` 는 이미 계산돼 있다.
- `draw_toast_scopes(painter, props)` — 스택 배치 + 카드 chrome. `Context` 가 아니라
  `Painter` 를 받는 이유는 **떠오르는 자리가 부르는 쪽마다 다르기 때문**이다: 본체는
  `Order::Tooltip` 레이어 painter 를, 갤러리는 무대 frame 의 painter 를 넘긴다.
- `toast_layout_card(ctx, theme, message, max_width)` — 본문 galley 와 카드 크기(폭 × 높이).
  폭 상한을 **어디서 얻는가**(본체는 스코프 폭, 단일 카드는 `toast_max_width`)만 부르는 쪽이
  정하고, 줄바꿈 폭·패딩·accent 바·높이는 여기서 한 번만 정한다.
- `toast_card_colors(theme, kind, alpha)` — alpha 를 반영한 fill · border · accent · 글자 색.
  alpha 를 곱하는 순서가 여기 고정돼 있다 — 테마 색(straight)에 곱한 뒤 `Color32` 로 바꾼다.
  `Color32` 로 바꾼 뒤 곱하면 감마 공간 곱이 돼 alpha < 1 에서 색이 달라진다.
- `draw_toast_single_card(ui, theme, kind, message, alpha)` — 스택 없이 카드 한 장만
  그려야 하는 자리용(갤러리의 Toast · Toast stack specimen). 치수와 색은 위 두 함수에서 온다.
- `draw_toast_card` / `ToastCardColors` — 카드 chrome 만 그리는 하위 함수. 치수·색을
  부르는 쪽이 직접 채운다.
- `toast_accent_color(kind, theme)` — kind → 좌측 바 색.
- `toast_fade_alpha(age, lifetime, reduced_motion)` — 페이드 곡선. `Duration` 둘만 받아
  어떤 상태 타입도 안 본다.

**상태**는 `src/adapters/ui/toast.rs` 에 남는다 — host 를 봐야 하는 것들이다.

- `ToastKind` — Info / Success / Warning / Error. 정본은
  `crates/tasty-type-appearance/src/toast_kind.rs` 이고 여기는 재수출이다.
- `ToastScope` — 위 enum. 정본은 `crates/tasty-model/src/toast_kind.rs`.
- `ToastState` — id, message, kind, scope, spawned_at, lifetime.
- `ToastManager` — `push(message, kind, scope)` / `push_info(...)` / `draw(ctx, &LayoutContext, reduced_motion)`(만료 제거 + 렌더). `AppState::toasts` 로 통합, draw 는 popup draw 직후(= 위 레이어)에서.
- `compute_alpha` — `ToastState` 에서 곡선이 읽는 두 값을 꺼내는 어댑터. `ToastState` 가
  `ToastScope`(→ `tasty-model` → termwiz)를 품어 위젯 크레이트로 넘어가지 못한다.
- `truncate_message` — 캡 집행. `push` 진입부라 그리기 경로가 아니다.

> **본문 글자는 페이드하지 않는다.** `Fonts::layout` 에 색을 명시하면 그 색이 galley 에
> 박히고 `Painter::galley` 의 fallback 은 `Color32::PLACEHOLDER` 구간에만 쓰이므로,
> 지금 페이드하는 것은 카드 배경·보더·accent 바뿐이다. 갤러리 specimen 이 한때 글자까지
> 흐리게 그려 본체와 갈려 있었고 지금은 본체에 맞췄다.

모든 토스트 문자열은 `t("toast.*")` 키 — `lang/{en,ko,ja}.toml` 세 파일 동시 추가([i18n](../../dev-guide/i18n.md)).

## 관련

- [popup.md](popup.md) — 내부 팝업 시스템
- [banner.md](banner.md) — parent 상단 info+action 오버레이 (내용 적으면 Toast 권장)
- [identity](../../identity.md) — 사용자/에이전트 행동 분리
- [ADR-0635](../../adr/0635-shared-design-and-theme.md) — 카드 치수와 색 계산을 공용 구현으로 모은 이유
