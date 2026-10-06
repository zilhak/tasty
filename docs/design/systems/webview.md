# WebView 호스트 계약

WebView는 winit 창 안에 놓이는 별도의 OS 자식 창이나 뷰다. Linux는 X11 child window, macOS는 WKWebView subview, Windows는 child HWND를 사용한다. 콘텐츠는 OS의 웹 엔진이 그리고 호스트는 위치·표시·수명·키보드 입력을 관리한다.

이 문서는 세 백엔드가 공통으로 제공할 동작을 설명한다. 선택 이유는 [ADR-0029](../../adr/0029-webview-host-integration.md)에 있다.

<a id="계약의-형태--trait-이-아니라-이름이다"></a>

## 공통 API와 플랫폼별 구현

PlatformWebView는 OS별 타입 중 cfg가 하나를 선택한다. 공통 호출부가 쓰는 메서드와 시그니처는
해당 OS 컴파일이 검사한다. 같은 빌드에서 런타임 다형성을 쓰지 않으므로 별도 backend trait은 두지 않는다.
선택 근거는 [Webview 통합 ADR](../../adr/0029-webview-host-integration.md)에 있다.

컴파일 성공은 실제 크기·focus·navigation·종료 행동이 같다는 증거가 아니다.
백엔드 변경 때는 아래 계약으로 세 구현을 비교하고 해당 OS에서 동작을 확인한다.

<a id="표면--열둘--drop"></a>

## 제공하는 연산

| 연산 | 입력 | 출력 | 백엔드 차이 |
|------|------|------|-------------|
| `new` | 부모 창 handle · `WebViewBounds` · `scale_factor` · `surface_id` · `Rc<dyn WebViewKeySink>` | `Result<Self, WebViewCreateError>` | Linux 만 `HasDisplayHandle` 을 추가로 요구한다(X11 display 포인터). macOS 는 실패 셋을 전부 `Permanent` 로 분류한다 |
| `set_bounds` | `WebViewBounds` · `scale_factor` | — | macOS 는 Cocoa 가 논리 좌표를 그대로 받아 물리 변환을 쓰지 않는다 |
| `set_visible` | `bool` | — | |
| `release_keyboard_focus` | — | — | 회수는 조건부다 — 아래 "포커스" |
| `load_url` | `&str` | — | |
| `load_html` | `&str` | — | |
| `nav_state` | — | `NavState` | |
| `take_pending_navigations` | — | `Vec<PendingNavigation>` | `user_gesture` 는 Linux 가 `is_user_gesture`, Windows 가 `IsUserInitiated` 에서 옮긴다. macOS 는 늘 `false` 다 — 아래 "탐색" |
| `set_zoom` | `f64` | — | |
| `set_javascript_enabled` | `bool` | — | 스크립트 게이트가 붙은 webview 에서는 값을 전역 sandbox 설정으로 `gate.set_sandbox` 에 넘기고, 적용하는 값은 문서별 판정이다([ADR-0053](../../adr/0053-html-script-detection-and-per-document-allowance.md)). macOS 는 전역 preference 를 켜 두고 문서별 값을 navigation 정책의 webpage preferences 로 준다 |
| `attach_script_gate` | `ScriptGate` | — | 첫 로드 전에 부른다. Linux 만 page cache 를 꺼서 뒤로·앞으로 가기도 응답 단계를 거치게 한다([ADR-0053](../../adr/0053-html-script-detection-and-per-document-allowance.md)) |
| `reload` | — | — | |
| `debug_history` | `DebugHistoryAction` | — | debug 빌드 전용이다. 뒤로·앞으로·다시 불러오기·중지를 재현한다 |
| `set_color_scheme` | `ColorScheme` | — | |
| `set_remote_content_allowed` | `bool` | — | Linux 는 WebKit content filter 로 막는다([ADR-0029](../../adr/0029-webview-host-integration.md)) |
| `Drop` | — | — | 아래 "수명" |

## 생성 — 부모 handle 과 실패 분류

부모 창은 raw-window-handle로 전달한다. Linux는 Xlib만, macOS는 AppKit ns_view,
Windows는 Win32 hwnd를 받는다. 지원하지 않는 handle은 Permanent 오류이며 Linux Wayland는 미지원이다.

Linux는 winit Xlib 연결에서 자식창을 만든 뒤 XSync로 서버 처리를 기다리고 별도 GDK 연결에서 조회한다.
XFlush만으로는 연결 사이 순서가 보장되지 않는다.
foreign_gdk_window가 FFI NULL을 Err로 반환하며 panic하는 foreign_new_for_display 바인딩은 직접 쓰지 않는다.
실패하면 방금 만든 X 창을 정리한다. native menu의 XID 변환도 같은 helper를 사용한다.

WebViewCreateError의 Permanent는 즉시 중단하고 Transient는 최대 8 회 시도한다.
창 생성·파괴 이벤트가 다음 시도를 깨울 수 있어 무한 재시도를 하지 않는다.
소스 검사는 알려진 생성→동기화→조회 순서와 NULL 처리만 확인하며 모든 X 경합이나 자원 정리를 증명하지 않는다.

## 좌표 — 논리와 물리를 타입 이름에 남긴다

`WebViewBounds`(논리) 와 `PhysicalWebViewBounds`(물리)는 `to_physical` / `from_physical`
로만 오간다. 레이아웃과 플랫폼 창 API가 각자 `* scale_factor` ·
`/ scale_factor` 를 손으로 적으면 한쪽만 고쳤을 때 조용히 어긋나기 때문이다. 두 타입이
`f32` 가 아니라 `f64` 인 이유(플랫폼 API 와 `scale_factor` 가 `f64` · 소비자가 `as i32` 로
절단)는 타입 정의에 붙어 있다. 왕복은 단위 시험이 세 OS 모두에서 고정한다.

크기 변경은 container와 실제 렌더 target 양쪽에 도달해야 한다.

| 플랫폼 | 크기 전달 |
|--------|-----------|
| macOS | WKWebView setFrame |
| Windows | SetWindowPos와 controller.SetBounds |
| Linux | XMoveResizeWindow와 gtk_window.size_allocate |

Linux foreign GdkWindow 구성에서는 GTK resize·size request만으로 allocation이 갱신되지 않았다.
foreign bind와 직접 size_allocate는 함께 유지하거나 함께 제거한다.
X 창에는 물리 px를 주고, GTK resize·size_allocate에는 GDK 창 배율로 나눈 논리 px(올림)를 준다. GTK 크기 단위는 논리 px라 물리 px를 그대로 주면 GDK 배율이 2일 때 페이지 viewport가 X 창의 두 배가 된다. winit 배율과 GDK 배율은 출처가 달라(`Xft.dpi`·`WINIT_X11_SCALE_FACTOR` / `GDK_SCALE`·XSETTINGS) 서로 대신하지 않는다. 이 규칙은 `GDK_SCALE=2`에서 측정했고, XSETTINGS로 배율이 정해지는 GNOME 세션에서는 측정하지 않았다.
활성·비활성 탭 생성, 전환, 확대·축소, 분할에서 부모와 렌더 자식 크기를 비교한다.
원인을 ConfigureNotify 누락 하나로 확정하지 않는다.

<a id="스레드--셋-다-send-지만-강제의-세기가-다르다"></a>

## 스레드 제약과 추가 검사

세 타입 모두 `Rc` · raw pointer · COM 객체를 필드로 가져 **auto-trait 상 자연 `!Send`** 다.
안전한 Rust 로는 다른 스레드로 옮길 수 없다. 추가 검사는 백엔드마다 다르다.

| 백엔드 | 컴파일 시점 | 실행 시점 |
|--------|-------------|-----------|
| Linux | `!Send` | `assert_origin_thread()` 가 생성 스레드와 현재 스레드를 비교해 **release 에서도** panic 한다 |
| macOS | `!Send` + `MainThreadMarker::new()` 가 생성 자체를 main thread 로 제한 | 없음(생성이 이미 막는다) |
| Windows | `!Send` | 없음. `CoInitializeEx(COINIT_APARTMENTTHREADED)` 가 같은 스레드 가정을 세운다 |

Linux는 X11 핸들을 잘못된 스레드에서 쓰면 정의되지 않은 동작(UB)이 생길 수 있어 release에서도 생성 스레드를 검사한다.

<a id="수명--부모가-먼저-죽을-수-있다"></a>

## 수명 — 부모 창이 먼저 닫히는 경우

PlatformWebView의 Drop이 OS 자원을 정리하며 부모 winit 창이 먼저 사라진 경우도 처리한다.

- Linux: 정리 구간에 GDK error trap을 설치한다. webview.destroy → gtk_window.hide+GTK pump →
  gtk_window.close+pump → GDK 연결 sync → XDestroyWindow+XSync+pump 순서다. hide·close는 예약형이므로 pump가 실제 순서를 완성한다.
  X 창은 winit 연결로 파괴하고 GDK는 자기 연결로 요청을 낸다. X 서버는 연결 사이의 처리 순서를 보장하지 않으므로 파괴 전에 GDK 연결을 sync한다.
  trap은 push 이후의 요청만 잡는다. drop 직전 `set_visible(false)`의 hide처럼 trap 전에 낸 GDK 요청도 이 sync가 파괴보다 먼저 처리시킨다.
  남은 X 오류는 warning으로 기록한다. trap만 두고 순서를 생략하지 않는다.
- macOS: removeFromSuperview 뒤 ARC가 자원을 정리한다.
- Windows: controller.Close 뒤 DestroyWindow를 호출한다. 이미 닫힌 경우는 trace로 기록한다.

종료 회귀를 재현할 때는 실제 native webview가 생성된 것을 먼저 확인하고 탭 닫기와 창 닫기를 구별한다.
보이던 webview가 overlay 없이 곧바로 제거되는 경로(원격 attach mirror의 surface 변환처럼 구조 변경이 왕복 뒤에 도착하는 경우)도 따로 확인한다.

<a id="탐색-상태--소유는-도메인-모델이다"></a>

## 탐색 상태와 실행 소유

`PlatformWebView`와 native handle, geometry·표시 cache는 `MainView`가 소유한다.
[App의 동기화 adapter](../../../src/app/webview_sync.rs)는 원 Engine의 실행 차용과 그 View의 native handle을 함께 받아 gate 설치, host-load 표시, load·reload, 탐색 상태 반영을 실행한다. 첫 로드 전에 script gate와 sandbox 값을 설치한다. surface 실행 인스턴스가 바뀌면 기존 native binding과 navigation proof를 폐기한다.

NavState(Idle·Loading·Done·Failed)는 tasty-model의 OS 비의존 값이다.
App adapter가 native backend의 현재 상태를 읽어 EngineRuntime의 `RemoteSurface` 관측값에 반영하고, egui chrome은 `SurfaceRead::remote_webview()`의 조회 값으로 표시한다. CoreState의 확정 구조가 native handle이나 탐색 실행을 소유하지 않는다.
`nav_state`는 현재 상태를 읽고 `take_pending_navigations`는 쌓인 요청을 꺼내 비운다.

HTML 배너는 [값 snapshot과 요청](../../../src/runtime/html_script.rs)을 사용한다. Allow·Dismiss·Reshow 요청은 표시 당시 `DocumentRecord`, `SurfaceBinding`, 원 `HtmlScriptState`의 weak identity에 묶이고 한 번만 소비된다. App은 현재 surface와 문서가 일치할 때만 요청을 적용한다. 프레임에서는 배너 렌더 뒤 요청을 적용하고 `finish_redraw` 뒤 같은 호출 흐름에서 native 동기화를 수행하므로, 허용 상태 갱신이 reload보다 먼저다. 문서별 허용 정책은 [ADR-0053](../../adr/0053-html-script-detection-and-per-document-allowance.md)을 유지한다.

사용자 navigation proof의 원본은 AppServices가 보유한 `NavigationProofs`다. App adapter는 plugin 통지 전에 원 View·surface·페이지 작성자·resource identity를 기록하고, 프레임 끝에 owner takeover와 사라진 surface를 반영한다. IPC `RequestScope`는 원 View의 weak identity와 현재 Engine 실행 문맥으로 proof를 한 번 소비한다. 닫힌 View나 교체된 resource의 기록은 새 대상에 사용할 수 없다.

PendingNavigation은 URL과 user_gesture를 포함한다. Linux는 WebKit is_user_gesture,
Windows는 IsUserInitiated를 사용하며 실패 시 warning과 false로 처리한다. macOS는 항상 false다.
이 값과 webview.set_url의 페이지 작성자를 이용한 파일 열기 판정은
[파일 핸들러의 origin 규칙](../../features/file-handler/index.md#origin-소유권과-비동기-완료)을 따른다.
그 절에 기록의 한 번 소비·작성자 전이·늦게 도착한 navigation 한계를 함께 둔다.

## 키보드 — 별도 계약

native 자식 창의 키는 winit으로 자동 전달되지 않는다. 각 backend가 논리 키·PhysicalKey·modifier를
WebViewKeySink에 보내고 공통 bridge가 소비 여부를 동기 반환한다. 소비한 키는 페이지로 보내지 않으며
실제 host 액션은 다음 프레임에 큐를 비워 실행한다.

| 메서드 | 역할 |
|--------|------|
| capture_key | 키 소비 여부를 반환한다 |
| note_focus | native 클릭·focus 획득을 알려 모델 focus를 맞춘다 |

backend는 Rc<dyn WebViewKeySink>만 받는다. 정책 교체와 큐 관리는 구체 bridge가 맡는다.
callback은 main thread에서 실행하므로 Rc·RefCell을 사용한다.
HostShortcutPolicy는 ShortcutSources의 host·page_reserved·plugin 목록을 받는다.
설정 field·quick-switch·사용자 script·활성 plugin의 effective binding 생성은 webview_claims가 맡는다.

modifier 없는 키와 Shift 전용 키는 페이지 입력에 남긴다.
find·copy·cut·paste·select_all은 host 액션 field에서 제외하고 이 예약 조합과 동등한 plugin 조합도 제외한다.
대소문자와 modifier 순서는 실제 binding parser로 비교한다.
다른 host 액션에 사용자가 같은 조합을 준 경우까지 추가 제외하지는 않는다.
plugin scope는 bridge에서 미리 거르지 않으므로 소비된 뒤 현재 host focus 규칙에 따라 실행되지 않는 키가 있을 수 있다.
비활성 plugin은 제외한다.

설정과 PluginCommandRegistry·PluginsConfig의 전역 revision이 바뀔 때만 정책 스냅숏을 다시 만든다.
modifier가 있으면 winit과 같은 US physical-key 변환을 쓰고 표 밖 키는 논리 키로 돌아간다.
key-up은 전달하지 않고 host의 modifier 저장값도 바꾸지 않는다.
macOS·Windows는 repeat를 거르며 Linux/GDK는 정상 press 오판을 피하려고 반복을 허용한다.

키 도착만으로 모델 focus를 바꾸지 않는다. X11에서는 포인터가 자식 위에 있다는 이유로 키가 올 수 있다.
Linux만 보이는 webview와 활성 창이 있을 때 16ms GTK poll을 사용하고 숨김·비활성 때 취소한다.
macOS의 performKeyEquivalent도 실제 first responder가 자기 뷰 안에 있을 때만 가로챈다.

## 포커스 — 회수는 **조건부**다

host가 webview를 가릴 때 host 창이 활성이고 실제 focus가 해당 자식 안에 있을 때만
부모 winit 창으로 회수한다. 가리는 경로는 두 가지다.

- overlay를 열 때: 모든 webview를 대상으로 개폐마다 한 번 회수한다.
- 탭·workspace 전환으로 surface가 활성 탭에서 빠질 때: 그 surface만 전이마다 한 번 회수한다.
  숨기기 전에 회수한다. 숨긴 뒤에는 backend가 focus가 이미 자식 밖에 있다고 판정할 수 있다.
  창이 비활성이면 창이 활성이 될 때까지 미루고, 그 사이 다시 활성 탭에 들어오거나 닫힌 surface는 뺀다.

탐색이 끝나지 않아 활성 탭의 webview를 숨기는 경우와 surface를 닫는 경우는 회수하지 않는다.
다시 보이거나 overlay가 닫혀도 native 자식 focus를 자동 복원하지 않는다.

| 플랫폼 | 자식 focus 확인 | 회수 대상 |
|--------|----------------|-----------|
| Linux | XGetInputFocus에서 부모 체인을 따라 자기 X 창인지 확인. None·PointerRoot 제외 | 부모 X 창 |
| macOS | firstResponder가 자기 뷰 또는 하위 뷰인지 확인 | contentView(winit 뷰), nil 사용 금지 |
| Windows | GetFocus가 자기 HWND 또는 IsChild인지 확인 | 부모 HWND |

창 활성 판정은 `host_window_has_os_focus`가 맡는다. base.focused가 참이거나, 어느 webview의
holds_keyboard_focus가 참이면 창이 활성이다. base.focused는 winit의 Focused 이벤트를 따르는데,
Linux/X11에서는 자식 창이 focus를 가져가면 winit이 부모 창에 Focused(false)를 보낸다(실측).
Windows의 winit도 WM_KILLFOCUS를 Focused(false)로 바꾼다(winit 소스 확인).
그래서 base.focused만으로는 회수가 필요한 바로 그 상태에서 회수가 막힌다.

holds_keyboard_focus는 위 표의 자식 focus 확인을 다시 쓰며 다른 앱이 OS focus를 가진 동안에는 거짓이어야 한다.

| 플랫폼 | 다른 앱이 focus를 가질 때 거짓인 이유 |
|--------|--------------------------------------|
| Linux | X focus는 서버 전역이다 |
| Windows | GetFocus는 호출 스레드의 큐가 비활성이면 널을 돌려준다 |
| macOS | first responder는 비활성 창에도 남으므로 key window 여부를 함께 확인한다 |

OS 조회는 base.focused가 거짓이고 회수할 대상이 있을 때만 한다. 탭 전환 경로는 회수할 surface가 없는 프레임에 조회하지 않는다.
창 활성 판정 없이 backend의 회수 조건(자식 focus 확인)만 쓰면 macOS에서 IPC로 overlay를 열거나 탭을 바꿀 때 비활성 창의 first responder가 바뀐다.
컴파일만으로 이 동작을 확인할 수 없으므로 활성·비활성 창에서 각각 재현한다.
Linux/X11(Xvfb)에서는 X focus를 자식 창에 직접 넣은 상태와 다른 X 클라이언트가 focus를 가진 상태를 확인했다.
실제 클릭은 X focus를 부모 창에 남겨 앞의 상태를 만들지 않았다. Windows·macOS는 실행 환경에서 확인하지 않았다.

<a id="도메인-라이브러리로는-아무것도-새지-않는다"></a>

## 도메인과 OS 구현의 경계

native WebKit·AppKit·WebView2 타입은 호스트 OS adapter에 둔다.
도메인에서는 NavState 같은 OS 비의존 값만 사용한다.
의존성은 crates/*/Cargo.toml과 webview 타입의 실제 사용처를 확인한다. 주석에 이름이 등장하는 것과 타입 의존은 구별한다.
headless가 사용하지 않는 GUI 의존성은 [빌드 경계](../../dev-guide/headless-build-boundaries.md)를 따른다.

<a id="이-문서가-못-말하는-것"></a>

## 플랫폼별 검증 한계

각 OS의 컴파일은 공유 호출부와 타입을 검사한다. 실제 클릭·단축키·IME·크기·종료·리소스 차단까지 검사한 것은 아니다.
기존 근거에서 Linux/X11의 실행 확인과 macOS·Windows의 소스·컴파일 확인은 구별돼 있다.
Windows의 fragment navigation과 사용자 제스처 전달, macOS 메뉴바와 키 중복 처리는 실제 환경에서 확인해야 하는 항목이다.
이 문서의 소스 설명만으로 해당 플랫폼의 실행 검증을 대신하지 않는다.

## 원격 리소스 차단

allow_remote_content=false는 navigation과 페이지 내부 HTTP·HTTPS 요청을 함께 제한한다.
Linux는 `^https?://` WebKit content filter를 홈의 webkit-content-filters 저장소에 비동기 컴파일한다.
완료 callback은 그때의 설정을 다시 읽고, 토글은 기존 필터를 제거한 뒤 필요하면 붙인다.
macOS는 WKContentRuleList, Windows는 WebResourceRequested 거절을 사용한다. data URI는 원격 규칙에 해당하지 않는다.

Linux는 필터 컴파일이 끝나기 전까지 차단하지 못하는 시간이 있으며 실패하면 경고를 남긴다. 첫 로드에서 원격 리소스가 차단되는지는 실제 화면·요청으로 확인한다.
안전 바인딩에 없는 API만 FFI로 호출하고 boxed filter handle은 Drop에서 unref한다.
상류가 해당 API를 제공하면 직접 FFI를 대체한다.
