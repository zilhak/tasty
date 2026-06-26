# 배너 시스템

**Banner** 는 parent(스코프) 상단에 떠서 **안내(info) + 그에 따른 즉시·임시 조치(action)** 를 제공하는 지속·인터랙티브 오버레이다 — 예: TUI 가 마우스를 캡쳐(DECSET 1000/1002/1003)해 드래그 선택이 막혔을 때 "왜 막혔는지 + 우회 방법" 을 띄우는 안내. Modal / Popup / Toast 에 이은 **4번째 오버레이 개념** 이며, `PopupManager`/`ToastManager` 가 아니라 별도 매니저로 관리된다. 용어 구분은 [concepts/ubiquitous-language](../../concepts/ubiquitous-language.md).

> **시각 토큰은 디자인 수령 후 Theme 토큰으로 확정됨.** 배너 전용 Tier-3 토큰이 본체 Theme 에 도입되어, 아래 px 수치(margin 8px, radius 8px 등)는 모두 토큰 접근자로 노출된다(섹션 "형태" 참조). 하드코딩 없음.

## 정체성 — 왜 별도 개념인가

배너는 세 축에서 기존 3종 어디에도 들어맞지 않는다.

| 축 | **Banner** | Toast | Popup |
|----|-----------|-------|-------|
| 마우스 입력 | **소비**(뒤로 전파 X) | 통과(소비 X) | 소비 |
| 키보드 포커스 | **없음** — 클릭해도 포커스 이동 X | 없음 | 가짐(클릭→포커스) |
| 내부 인터랙션(버튼) | **있음** | 없음(본문만) | 있음 |
| 타이틀바 / 드래그 / 자유이동 | 없음 | 없음 | 있음(7대 규칙) |
| 수명 | 사용자 닫기 또는 TTL | 자동소멸(고정) | 사용자 닫기 |
| 위치 | parent **상단** 고정, floating | 스코프 우측 하단 스택 | 자유 이동 |

배너는 **포커스를 받지 않으면서도 자기 영역의 마우스를 소비하고 내부 버튼을 갖는다.** 이 조합은 Popup 의 7대 규칙([popup.md](popup.md))·포커스 모델과 충돌하고(타이틀바·X·드래그·z-order 승격·자유이동 모두 없음), Toast 의 휘발성·입력통과([toast.md](toast.md))와도 충돌한다(배너는 입력을 소비하고 사용자가 닫을 수 있다). → **별도 개념·별도 매니저** 로 둔다. Toast 가 Popup 의 변종이 아니라 별도 매니저로 분리된 것과 동일한 논리다.

## 포지셔닝 — Popup / Banner / Toast

세 컴포넌트 모두 parent 기준으로 floating 되는 패널이지만 **목적** 이 다르다.

- **Popup** = 독립적인 **기능** 을 하는 컴포넌트. parent 와 연결되는 기능도 popup 으로 구현.
- **Banner** = parent 의 상태/조작에 따른 **info + 그에 따른 조치(action)** 를 손쉽게 하기 위한 컴포넌트.
- **Toast** = parent 의 상태/조작에 따른 **info 만** 표시하는 컴포넌트.

배너의 본 용도는 **사용자 안내 + 즉시/임시 action 부착** 이다. 단순 정보 표시에도 쓸 수 있으나 **내용이 적으면 Toast 를 권장** 한다 — 배너는 action 이 붙거나 내용이 있을 때 쓴다.

## 위치 규칙

배너 대상 스코프는 **View(최상위) / Workspace / Pane / Tab / Surface** 중 하나다. 공통 원칙은 하나다:

> **배너는 탭 바(탭 영역)를 가리지 않는다.** 가리면 탭 전환이 막히기 때문이다.

배치는 두 부류로 나뉜다.

### ① Workspace / Pane / Tab / Surface 배너 — "탭 바 바로 아래"

네 스코프 모두 **"탭 바 바로 아래 = 콘텐츠 영역 최상단"** 을 기준으로 상단 margin 을 두고 뜬다.

- Workspace / Pane 은 **탭 바 하단** 기준, Tab / Surface 는 **자기 영역 최상단** 기준이지만 — Tab/Surface 영역의 최상단이 곧 탭 바 아래이므로 **네 스코프 모두 사실상 같은 y 위치** 다.
- 스코프 간 차이는 **가로 폭이 어느 영역의 100% 인지**(그리고 좌우 clamp 경계)뿐이다.

### ② View / Modal 배너 — 플레이스홀더

각 View 가 지정한 **배너 플레이스홀더** 위치에 뜬다.

- **View 배너**: 워크스페이스에 **종속되지 않고** View 자체에서 띄운다. **모든 View 구현체(`MainView`/`SettingsView`/…)는 배너 표시 위치 플레이스홀더를 가져야 하며**, 각 View 가 자기에게 알맞은 곳에 지정한다. 워크스페이스 전환과 무관하게 View 위에 유지된다.
- **Modal 배너**: Modal 은 View 의 한 형태(`SettingsView`/`QuitView`/`PluginsView`)이므로 위 플레이스홀더 규칙에 포함된다. Modal 이 전역 입력을 독점하는 상태에서도 그 Modal 의 플레이스홀더 위치에 배너가 떠야 한다 — 배너는 Modal 의 입력 차단보다 **위 레이어** 에서 자기 영역의 마우스를 소비한다.

## 형태

- **floating overlay** — parent 영역을 나눠 차지하지 않고 그 **위에 떠서 덮는다**(Toast/Popup 과 동일). 배너 height 이외의 모든 공간이 그대로 하단 콘텐츠 공간이 된다.
- 너비: parent 폭 **100% − 좌우 margin**.
- margin: **상 8px / 좌 8px / 우 8px**, **하단 margin 없음**(`spacing_sm`).
- border-radius: **8px**(약간 둥근 사각형 패널) — `corner_radius_lg`(= `--tasty-radius-8`, 시스템 기본 4px 의 의도적 2배). 이 단차는 ADR 근거로 토큰화.
- 높이: **콘텐츠에 따라 가변** — 각 배너 구현체가 자체 결정. 시스템은 "프레임/셸"(`draw_shell`) 과 내부 패딩(좌우 `spacing_md` 12 / 상하 `spacing_sm` 8) 규칙만 정의.
- 배경 / 보더 / 그림자: **Theme 토큰** — `banner_bg()`(→ `surface_raised`/surface0) 배경 + 1px `banner_border()`(→ `border_strong`) 보더 + `shadow_popover()`(= `--tasty-shadow-popover`) 그림자. 본문 색은 `banner_fg()`(→ text_primary), leading 글리프 기본색은 `banner_icon_fg()`(→ text_muted, 심각도 배너는 override), 카운트다운은 `banner_countdown_fg()`(→ text_muted). 하위 스코프 디밍은 `opacity_recessed()`(0.4), 페이드 모션은 `motion_ui_ms()`(120ms).

## 닫기 버튼 / 카운트다운 (우측 상단, 같은 자리)

우상단 같은 자리에서 상태에 따라 표현이 바뀐다.

- **기본 배너(TTL 없음)**: X(닫기) 버튼이 **평소 숨김**, **배너 위 hover 시에만 표시**.
- **TTL 배너**: 평소 그 자리에 **카운트다운 숫자(초 단위)** 표시 → **hover 시 X 로 전환**.
- X 클릭 시 배너 닫힘(사용자 행동).

(숫자 타이포·크기·정렬, 숫자↔X 전환 표현은 디자인 수령 후 보강.)

## TTL (살아있는 시간)

- 배너는 선택적으로 TTL 을 가진다.
- 카운트다운은 **초 단위** 로 우상단에 표시, **0 이 되면 자동 소멸**.
- **정지 조건**: ① 배너 위에 마우스 hover 중 ② 백그라운드(자기 스코프가 현재 화면에 그려지지 않음). 정지 동안 남은 TTL 을 **보존** 하고, 재개 시 **이어서** 진행한다.

## 큐 (다중 배너)

한 스코프 상단에는 **한 번에 1개만 표시**, 나머지는 **큐** 에 대기한다.

- **한 종류(고유 id)당 하나만**:
  - 표시 중인 배너와 **동일 id** 가 다시 발생 → **카운트다운 초기화**(카운트다운 없는 배너면 무시).
  - 큐에 있는 배너와 **동일 id** → **무시**.
- 표시 중 배너가 닫히면 큐에서 **하나씩 꺼내** 표시한다.
- 큐 **최대 5개**. 꽉 찬 상태에서 새 배너 발생 시 **무조건 무시**.

## 계층 z-index / 투명도

서로 다른 스코프의 배너가 동시에 있을 때의 규칙이다.

- 계층(상위 → 하위): **View > Workspace > Pane > Tab > Surface**.
- 상위 배너의 z-index 가 하위보다 **높다**(상위가 앞에 명확히 보임).
- **상위 요소 배너가 뜨면 하위 요소 배너는 60% 투명**(잘 안 보이게).
- 높이 관계: 상위 배너가 더 크면 하위는 그 뒤에 가려져 안 보이고, **하위 배너가 더 커서 뒤로 삐져나온 부분만 60% 투명** 으로 비친다.

(60% 투명의 정확한 표현 — 단순 알파 / 페이드 / 블러 여부 — 과 z 단차는 디자인 수령 후 보강.)

## 종류(kind)

- Info / Success / Warning / Error 같은 **범용 분류는 두지 않는다**(Toast 와 다른 점).
- **각 배너의 고유 id 자체가 kind** 역할을 한다.
- 경고/에러 같은 심각도 표현은 **그 배너 디자인이 자체적으로** 처리한다.

## 발화 정책 (불가침)

**배너는 사용자 직접 조작에서만 발사된다.** IPC 로 발생하는 모든 동작은 배너를 표시하지 않는다.

- ✅ 사용자 행동(키보드/마우스로 유발된 상태) → 배너 발화
- ❌ release 의 IPC/CLI/Plugin/시스템 cascade 에서 배너 발화

tasty identity 원칙 1(에이전트 행동의 부수효과가 사용자 시각 상태에 닿지 않는다, [identity](../../identity.md))과 정합하며, [popup.md](popup.md)·[toast.md](toast.md) 의 "사용자 행동에서만 발사" 와 **동일한 규칙** 이다.

## IPC / debug

- **터미널 텍스트 읽기**(`surface.read_since_mark` 등)에는 **배너 정보를 포함하지 않는다**(텍스트 오염 방지).
- **debug 빌드 전용** 으로만 배너를 읽고 제어한다. debug 메서드는 사용자 입력 재현/내부 상태 덤프 격리 정책(`#[cfg(debug_assertions)]` + `feature="gui"`, [debug-ipc](../../dev-guide/debug-ipc.md))을 따르며, release 라우터에는 등록되지 않는다. IPC 메서드(= CLI `tasty debug banner <sub>`):
  - `debug.banner.list` (`list`) — 빌트인 def 목록 + 현재 표시/대기 상태 덤프.
  - `debug.banner.show` (`show --banner-id <id> --scope <token>`) — 배너 발화. `outcome`(`Shown`/`Queued`/`ResetCountdown`/`Ignored`) 반환.
  - `debug.banner.close` (`close --banner-id <id>`) — id 로 닫기(표시 중이면 큐 head 승격).
  - `debug.banner.set_countdown` (`set-countdown --scope <token> --seconds <n>`) — 표시 중 TTL 배너 남은 시간 강제 설정.
  - `scope` 토큰: `view` / `workspace:<i>` / `pane:<id>` / `tab:<pane>:<i>` / `surface:<id>` ([`BannerScope::from_token`]). 반환은 별도 구조 없이 "호출 함수 정보 + 인자값" 수준.

## 구조

배너 매니저는 별도 모듈(`src/adapters/ui/banner.rs`)로 둔다(Toast 가 `toast.rs` 로 분리된 것과 동일). 분류 enum(`BannerId`/`BannerScope`)은 GUI 비의존이라 `crates/tasty-model/src/banner_kind.rs` 에 잔류한다([model-view-split](../../dev-guide/model-view-split.md)).

- **`BannerDef`** — 정적·데이터 지향 정의(고유 id, TTL 유무, 콘텐츠 draw 함수). id 가 곧 kind. `defs::all_defs()`/`defs::find(&str)` 로 조회.
- **`BannerState`** — 큐/TTL 단위 인스턴스(id, scope, ttl_ms, remaining_ms). `persistent`/`with_ttl` 생성자.
- **`BannerManager`** — 스코프당 1 표시 + 최대 5 큐, TTL 카운트다운·정지/재개, 계층 z-index·디밍 스택, 마우스 소비를 중앙 관리. 큐/TTL 로직(`push`/`close_shown`/`advance`)은 egui 비의존 순수 함수라 단위 테스트로 결정론 검증. 시각 `draw()` 는 `LayoutContext` 로 스코프-rect 를 계산(popup/toast 와 일관)하고 `BannerDrawResult { hovered }` 를 돌려준다. `hovered` 는 `AppState.banner_hovered` 로 입력 레이어에 배선([input-layer](../../architecture/input-layer.md)).

모든 배너 문자열은 `t("banner.*")` 키 — `lang/{en,ko,ja}.toml` 세 파일 동시 추가([i18n](../../dev-guide/i18n.md)). 모든 색·치수는 Theme 토큰([theme.md](theme.md)).

## 관련

- [popup.md](popup.md) — 내부 팝업 시스템(독립 기능, 포커스 가짐)
- [toast.md](toast.md) — 휘발성 알림(info 만, 입력 통과)
- [concepts/ubiquitous-language](../../concepts/ubiquitous-language.md) — Modal/Popup/Toast/Banner 구분
- [identity](../../identity.md) — 사용자/에이전트 행동 분리(발화 정책 근거)
- [adr/0024-banner-fourth-overlay-concept](../../adr/0024-banner-fourth-overlay-concept.md) — 배너를 별도 4번째 개념으로 둔 결정
