# 배너 시스템

배너는 소속 영역 상단에 안내와 즉시 할 수 있는 조치를 표시하는 오버레이다. TUI가 마우스를 캡처(DECSET 1000/1002/1003)해 드래그 선택이 막혔을 때 이유와 우회 방법을 알리는 것이 한 예다. Popup·Toast와 별도 관리자를 사용한다. 용어는 [통합 용어집](../../concepts/ubiquitous-language.md)을 따른다.

색과 치수는 아래 Theme 접근자로 읽는다. 숫자를 호출부에 직접 쓰지 않는다.

## 정체성 — 왜 별도 개념인가

배너는 안내와 바로 할 수 있는 조치를 함께 보여 준다. 마우스 입력과 버튼을 지원하지만 키보드 포커스를 가져가지 않는다.

| 동작 | Banner | Toast | Popup |
|---|---|---|---|
| 마우스 | 자기 카드 영역에서 소비 | 통과 | 소비 |
| 키보드 포커스 | 없음 | 없음 | 있음 |
| 내부 버튼 | 있음 | 없음 | 있음 |
| 위치·이동 | 부모 상단 고정 | 범위별 고정 스택 | 이동 가능 |
| 수명 | 닫기 또는 TTL | 자동 소멸 | 사용자가 닫음 |

이 차이를 유지하려고 별도 BannerManager를 둔다. popup의 타이틀바·이동·포커스 기능을 여러 옵션으로 끄거나 입력이 통과하는 toast에 버튼을 넣지 않는다.

## 포지셔닝 — Popup / Banner / Toast

Popup은 독립 기능, Banner는 안내와 조치, Toast는 짧은 정보 표시를 담당한다. 모두 부모 영역 위에 표시하지만 역할이 다르다. 배너에 단순 정보를 넣을 수도 있으나 내용이 짧고 조치가 없으면 Toast를 사용한다.

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
- border-radius: **8px**(약간 둥근 사각형 패널) — `corner_radius_lg`(= `--tasty-radius-8`, 시스템 기본 4px 의 의도적 2배). 기본 반경과 구분된 토큰을 사용한다.
- 높이: **콘텐츠에 따라 가변** — 각 배너 구현체가 자체 결정. 시스템은 "프레임/셸"(`draw_shell`) 과 내부 패딩(좌우 `spacing_md` 12 / 상하 `spacing_sm` 8) 규칙만 정의.
- 배경 / 보더 / 그림자: **Theme 토큰** — `banner_bg()`(→ `surface_raised`/surface0) 배경 + 1px `banner_border()`(→ `border_strong`) 보더 + `shadow_popover()`(= `--tasty-shadow-popover`) 그림자. 본문 색은 `banner_fg()`(→ text_primary), leading 글리프 기본색은 `banner_icon_fg()`(→ text_muted, 심각도 배너는 override), 카운트다운은 `banner_countdown_fg()`(→ text_muted). 하위 스코프 디밍은 `opacity_recessed()`(0.4), 페이드 모션은 없다(`banner_fade()` 는 생성만 되고 소비처가 없다).

## 닫기 버튼 / 카운트다운 (우측 상단, 같은 자리)

우상단 같은 자리에서 상태에 따라 표현이 바뀐다.

- **기본 배너(TTL 없음)**: X(닫기) 버튼이 **평소 숨김**, **배너 위 hover 시에만 표시**.
- **TTL 배너**: 평소 그 자리에 **카운트다운 숫자(초 단위)** 표시 → **hover 시 X 로 전환**.
- X 클릭 시 배너 닫힘(사용자 행동).
- 닫기 버튼은 갤러리의 `dismiss_x()`와 같은 Ghost/Sm `IconButton`과 `icons::CLOSE` SVG를 사용한다. `"✕"` 문자는 UI 폰트에 없으면 빈 사각형으로 표시되므로 사용하지 않는다. 색은 IconButton의 기본 색을 따른다(ghost: text-secondary, hover: text-primary). 카운트다운은 `banner_countdown_fg()`를 사용한다.

## "더보기"(⋯) 컨텍스트 메뉴 — mouse-capture 배너 전용

mouse-capture 배너(`defs::BANNER_MOUSE_CAPTURE`)에 한해, X 왼쪽에 "더보기" ⋯ 트리거가
같은 버튼 열에 나란히 놓인다. 다른 배너 kind 는 이 트리거를 갖지 않는다.

- **노출 조건**: X 와 동일 — 배너 hover 시에만. 단 ⋯ 의 컨텍스트 메뉴가 열려 있는 동안은
  hover 여부와 무관하게 **계속 표시 + active(강조) 상태 유지** — 재사용하려면 ⋯ 재클릭.
- **배치**: ⋯ 가 X 왼쪽, 사이 4px gap(`spacing_xs`). 이 배너는 항상 2 슬롯 몫(56px =
  2×24 + gap 4 + gap 4)을 본문 우측에 예약한다 — hover 진입/이탈로 본문 폭이 흔들리지
  않도록, hover 전에도 예약 폭은 고정이다(다른 배너는 기존 1 슬롯 28px 그대로).
- **트리거 아이콘**: SVG `icons::MORE` — 수평 3-dot(`M5 12h.01M12 12h.01M19 12h.01`).
- **메뉴**: host `PopupDef` 의 `headless: true` 컨텍스트 메뉴(`popup-implementation.md`).
  앵커는 트리거 버튼 아래 4px, 우측 정렬 — 뷰포트 하단 공간이 없으면 위로 flip. outside
  click/Esc 로 닫힘(scrim 없음), ↑↓/Enter/Esc 키보드 내비게이션은 기존 headless 메뉴와 동일.
  min-width 200px / max-width 288px, 내부 패딩 4px, 배경·보더·radius·그림자는 다른 메뉴
  (Tools menu 등)와 같은 토큰을 재사용한다.
- **항목 2개(순서 고정)**, 클릭 시 즉시 실행 + 메뉴 닫힘, 둘 다 neutral 톤(danger 아님 —
  파괴/유실 없고 Settings 에서 되돌릴 수 있음):
  1. **"{app}에 대해 이 알림 끄기"**(`icons::BELL`) — `mouse_capture_banner_blacklist` 에
     foreground 프로그램 이름 추가 + **배너도 즉시 함께 닫힘**.
  2. **"{app}에 대해 마우스 캡처 비활성화"**(`icons::MOUSE`) — `mouse_capture_blacklist` 에
     추가. **배너는 남는다** — 캡처가 이미 풀렸음을 사용자가 읽고 직접 닫도록.
- **라벨 렌더**: 고정 텍스트 + 프로그램 이름(mono, 강조) **두 조각**으로 분리 렌더한다 —
  하나의 문자열로 합쳐 ellipsis 하면 로케일에 따라(특히 en) 프로그램 이름부터 잘리기
  때문이다. 고정 텍스트는 줄바꿈/truncate 없음, 프로그램 이름 세그먼트만 축소+ellipsis,
  전체 이름은 항목 tooltip 으로 보완한다.
- 두 블랙리스트는 Settings › Terminal › Mouse Capture 탭과 데이터를 공유한다. 메뉴와 설정 화면은 같은 저장·매칭 규칙을 사용한다([ADR-0015](../../adr/0015-terminal-user-input-routing.md)).

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

하위 배너 디밍은 `opacity_recessed()`를 사용한다. 별도 페이드 모션은 적용하지 않는다.

## 종류(kind)

- Info / Success / Warning / Error 같은 **범용 분류는 두지 않는다**(Toast 와 다른 점).
- **각 배너의 고유 id 자체가 kind** 역할을 한다.
- 경고/에러 같은 심각도 표현은 **그 배너 디자인이 자체적으로** 처리한다.

## 발화 정책 (불가침)

배너는 사용자 직접 조작에 대한 안내로 표시한다. release의 IPC·CLI·플러그인·시스템 작업만으로는 표시하지 않는다. [사용자와 에이전트 행동 분리](../../identity.md)를 따른다. 토스트에 허용된 원격 연결 상태 알림은 배너의 예외가 아니다.

## IPC / debug

- `surface.read_since_mark` 등 터미널 읽기에는 배너 텍스트를 넣지 않는다. 배너는 egui의 `Order::Foreground`로 그려지며 termwiz 그리드와 분리돼 있다. 별도 텍스트 필터는 필요 없다.
- **debug 빌드 전용** 으로만 배너를 읽고 제어한다. debug 메서드는 사용자 입력 재현/내부 상태 덤프 격리 정책(`#[cfg(debug_assertions)]` + `feature="gui"`, [debug-ipc](../../dev-guide/debug-ipc.md))을 따르며, release 라우터에는 등록되지 않는다. IPC 메서드(= CLI `tasty debug banner <sub>`):
  - `debug.banner.list` (`list`) — 빌트인 def 목록 + 현재 표시/대기 상태 + 기하 덤프.
    표시 중인 배너마다 셸 rect(논리)와 plugin egui-mesh 콘텐츠 rect(물리)를 낸다.
    셸 rect 의 출처는 hover 판정이 쓰는 것과 같은 `card_rects` 라 **직전 프레임 실측**이고,
    그래서 뜬 직후 첫 프레임에는 없다 — 배너는 popup 과 달리 자기 좌표를 모델에 들고
    있지 않고 컨테이너가 매 프레임 배치한다.
  - `debug.banner.show` (`show --banner-id <id> --scope <token>`) — 배너 발생. `outcome`(`Shown`/`Queued`/`ResetCountdown`/`Ignored`) 반환.
  - `debug.banner.close` (`close --banner-id <id>`) — id 로 닫기(표시 중이면 큐 head 승격).
  - `debug.banner.set_countdown` (`set-countdown --scope <token> --seconds <n>`) — 표시 중 TTL 배너 남은 시간 강제 설정.
  - `scope` 토큰: `view` / `workspace:<i>` / `pane:<id>` / `tab:<pane>:<i>` / `surface:<id>` ([`BannerScope::from_token`]). 반환은 별도 구조 없이 "호출 함수 정보 + 인자값" 수준.

## 구조

배너 관리자는 `src/adapters/ui/banner.rs`에 있다. GUI가 없어도 쓰는 `BannerId`·`BannerScope`는 `crates/tasty-model/src/banner_kind.rs`에 둔다([model-view-split](../../dev-guide/model-view-split.md)).

| 타입 | 역할 |
|---|---|
| `BannerDef` | 고유 ID, TTL 여부, 콘텐츠 그리기 함수의 정적 정의. `defs::all_defs()`·`defs::find(&str)`로 조회 |
| `BannerState` | id·scope·ttl_ms·remaining_ms·content를 담는 인스턴스. `persistent`·`with_ttl`·`plugin_mesh`로 생성 |
| `BannerContentSource` | Host의 `content_fn`과 PluginMesh의 egui-mesh 콘텐츠 구분 |
| `BannerKey` | `Host(id)`·`Plugin(instance_id)`로 정적 배너와 동적 plugin 배너를 같은 큐에서 구분 |
| `BannerManager` | 스코프당 1개 표시·최대 5개 대기, TTL·배치·겹침·디밍·마우스 입력 관리 |

큐와 TTL 처리인 `push`·`close_shown`·`advance`는 egui 없이 단위 테스트로 확인한다. plugin 배너도 같은 수명 관리를 사용하며 채널 규칙은 [egui-mesh 가이드](../../dev-guide/egui-mesh-channel.md)의 banner 절을 따른다.

`draw()`는 `LayoutContext`에서 배치 영역을 계산한다. 호출자가 현재 더보기 메뉴의 scope를 `more_menu_open_for: Option<&BannerScope>`로 전달하며, BannerManager가 팝업을 직접 조회하지는 않는다. 반환값은 `BannerDrawResult { hovered, more_clicked }`다.

- `hovered`는 `AppState.banner_hovered`를 통해 [입력 계층](../../architecture/input-layer.md)에 전달한다.
- `more_clicked: Option<(BannerScope, egui::Rect)>`는 버튼의 scope와 사각형이다. 호출자가 이 값으로 대상 필드를 채우고 컨텍스트 메뉴를 연다.

마우스는 scope 전체가 아니라 실제 카드 영역에서만 소비한다. 그렇지 않으면 배너 아래 터미널 클릭까지 막힌다. 배치 영역(`banner_zone`)과 입력 영역(`card_rects`)을 구분하며, 카드 영역은 직전 프레임 값을 사용해 1프레임 늦게 반영된다. 위치가 고정된 persistent 배너에서는 이 지연이 드러나지 않는다.

모든 배너 문자열은 `t("banner.*")` 키 — `lang/{en,ko,ja}.toml` 세 파일 동시 추가([i18n](../../dev-guide/i18n.md)). 모든 색·치수는 Theme 토큰([theme.md](theme.md)).

## 관련

- [popup.md](popup.md) — 내부 팝업 시스템(독립 기능, 포커스 가짐)
- [toast.md](toast.md) — 휘발성 알림(info 만, 입력 통과)
- [concepts/ubiquitous-language](../../concepts/ubiquitous-language.md) — Modal/Popup/Toast/Banner 구분
- [identity](../../identity.md) — 사용자/에이전트 행동 분리(발생 정책 근거)
- [ADR-0036](../../adr/0036-overlay-scope-and-lifetime.md) — 배너의 입력과 수명 규칙
