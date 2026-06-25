# Design ↔ Gallery ↔ Host 3자 매핑

`design-parity` 작업의 컴포넌트 매핑 기록. 디자인 jsx 하위 컴포넌트 ↔ tasty 호스트 함수 ↔
갤러리(`tasty-gallery`) 카탈로그 항목을 1:1 로 연결한다. 다음 작업이 바로 찾도록 한다.

갤러리 실행: `cargo run -p tasty-gallery` (상단 toolbar 에서 theme·UI scale 토글, 좌측
카탈로그 선택). 등록: `crates/tasty-gallery/src/catalog/{components,widgets}/<name>.rs` 의
`draw(ui, theme)` + `catalog.rs::all()` 에 `CatalogItem` 한 줄.

## remote_tool (Overlays)

디자인 `ui_kits/terminal/overlays/remote_tool.jsx` ↔ `src/adapters/ui/popup/remote_tool.rs`.

| 디자인 jsx 컴포넌트 | tasty 함수 | 갤러리 항목 |
|---|---|---|
| `RemoteTool`(container) | `draw_remote_tool_popup` | ✗ 미등록 (사유 아래) |
| `TabBtn`(내부) | `draw_tab_bar` | — |
| `WarnBadge` | `warn_badge` | — |
| `ListShell` | `draw_profile_list` / `draw_passkey_list` (add-bar+scroll 합침) | — |
| `ProtocolFilter`(add-bar 버튼) | `filter_button` (`draw_profile_list` 내, funnel+라벨) | ✗ 미등록 (remote_tool 예외 동일) |
| `ProtocolFilter`(드롭다운/팝오버) | `draw_protocol_filter` (체크박스 + Apply-on-confirm) | ✗ 미등록 (remote_tool 예외 동일) |
| `ProfileRow` | `draw_profile_row` | — |
| `ProfileForm` | `draw_profile_form` | — |
| `PasskeyRow` | `draw_passkey_row` | — |
| `PasskeyForm` | `draw_passkey_form` | — |
| `ConfirmDelete` | `draw_confirm_delete` | — |
| `PasskeySelect` | `passkey_dropdown_row` | — |

**갤러리 미등록 사유**: `draw_remote_tool_popup` 시그니처가 `(ui, &mut AppState, &mut
CoreState)` 로 호스트 상태에 의존한다(UiState 를 egui ctx memory 에 저장, `RemoteProfiles::
load()` / `Passkeys::load()` 로 파일 IO). 갤러리 `CatalogItem.draw` 는 `(ui, &Theme)` 뿐이라
직접 호출 불가. view-only props 분리(model-view-split) 가 선행돼야 등록 가능. → **후속 과제.**
그 전까지 검증은 본체 `debug.host_popup.open remote_tool` + `ui.screenshot` 로 한다.

프로토콜 필터(`filter_button` / `draw_protocol_filter`)도 같은 예외에 포함된다 — `draw_profile_list`
하위에서 `egui::Context` memory(`read_filter`/`write_filter`, `FILTER_MEMORY_ID`)와 popup
상태(`FILTER_POPUP_ID`)에 의존하므로 `(ui, &Theme)` 시그니처로 분리 불가. 컨테이너가 등록 가능해질
때 함께 등록한다. 검증 경로 동일(`debug.host_popup.open remote_tool` + `ui.screenshot`).

## 이미 갤러리에 있는 관련 항목 (참고)

`catalog/components/` 에 등록된 것: `command_palette` · `port_scanner` · `convert` ·
`approval` · `file_handler_picker` · `markdown_open` · `rename_popup` · `toast` ·
`sidebar` · `tab_bar` · `apply_preset`. 이들은 props 분리가 돼 있어 갤러리로 즉시 검증 가능.

## Overlay 시각 복제 specimen (본체 의존 0)

본체 view 의 시각만 로컬 mock props 로 복제한 Overlays 항목. 본체 binary crate(`tasty`)에
의존 불가하므로 layout·색·폰트·간격·보더는 모두 Theme 토큰에서 가져오고 상태는 mock 으로
주입한다. 본체 view 변경 시 시각 동기화는 수동 검증.

| 디자인 canonical | 본체 view | 갤러리 specimen |
|---|---|---|
| `overlays/search_bar.jsx` (360×28) | `src/adapters/ui/search_bar.rs::draw_search_bar` | `search_bar` (Overlays) |
| `overlays/tools_menu.jsx` (160px) | `src/adapters/ui/tools_menu.rs::draw_tools_menu` | `tools_menu` (Overlays) |

## Layouts — plugins window (1-depth idiom)

디자인 `ui_kits/terminal/overlays/plugins_window.jsx` (820×540 모달) ↔ 본체 `src/view/plugins/`
↔ 갤러리 `1 depth (Plugins idiom)` (Layouts). 본체 binary 의존 0 — 로컬 mock 데이터로 시각 복제.

| 디자인 jsx 컴포넌트 | 갤러리 함수 (`widgets/layout_1depth.rs`) | 비고 |
|---|---|---|
| `PluginsWindow`(container) | `draw_modal` | 820×540 고정, 48px 헤더 |
| header + `Seg` 세그먼트 | `draw_header` / `draw_segments` | Installed \| Attention(danger badge) \| Add |
| installed list+detail | `draw_installed` (`with_list_panel`/`with_detail`) | 288 리스트 + 디테일 + 액션바 |
| `AttentionPanel` (4케이스) | `draw_attention` / `reason_banner` / `reason_detail` | unknown-key·signature-invalid·permissions-changed·health-error |
| `AddPluginForm` (trust 흐름) | `draw_add` (`add_path_picker`/`add_manifest_preview`) | 매니페스트 프리뷰 + 미신뢰 배너 + Trust & add |
| `PluginAvatar` | `cat_avatar` / `draw_avatar` | color-mix → `mix`/`alpha` 헬퍼 |

검증: `TASTY_GALLERY_SHOT=31:<png>` (Installed)·기본탭 임시 변경으로 Attention 4케이스/Add 캡처.
좌표 ±1px(모달 820×540, 헤더 48, 리스트 288), RGB 정확(bg_sidebar/bg_panel/border_strong). 화면전용
고정값(820/540/48/288/26/14/22)은 token-policy §c verbatim const, 브랜드 마크색은 테마불변 const.

## Specimen 공용 헬퍼 (dedup)

specimen 간 중복 chrome 을 한 곳으로 모은 카탈로그 헬퍼 (`crates/tasty-gallery/src/catalog/`):

| 헬퍼 | 제공 | 쓰는 곳 |
|---|---|---|
| `specimen.rs` | `caption` / `case_title` | 전 prim_* + rename_popup·sidebar |
| `toast_card.rs` | `accent_color` / `draw_card` (`CardColors`) | toast(components/widgets) |
| `popup_frame.rs` | `draw` (`ContentInset`) — surface-raised 프레임 + border-strong | approval · convert · file_handler_picker · dialog |

## Primitive 컴포넌트 레이어 (Components)

디자인 `components/**` 의 atomic primitive ↔ `tasty-ui-widgets` 공용 함수 ↔ 갤러리
`Components` specimen 3자 매핑. 본체 팝업과 갤러리가 **동일** `tasty_ui_widgets::*` 를
호출(mirror 아님 — demo=main). 위젯의 집은 `crates/tasty-ui-widgets/`(메인+갤러리 양쪽 의존).

| 디자인 컴포넌트 | tasty-ui-widgets | 갤러리 specimen | 시각검증 |
|---|---|---|---|
| `core/IconButton` | `IconButton` (ghost/solid/active, sm/md) | `prim_icon_button` | ✓ port_scanner |
| `core/Button` | `Button` (primary/secondary/ghost/danger/agent × sm/md/lg, leading_icon/trailing_icon) | `prim_button` | ✓ port_scanner |
| `forms/Input` | `Input` (icon/addon/mono/invalid/disabled, focus ring) | `prim_input` | ✓ port_scanner |
| `core/Tag` | `tag` (default/accent/agent/success/warning/danger + dot) | `prim_chips` | ✓ port_scanner(PID) |
| `core/Badge` | `badge` / `badge_dot` | `prim_chips` | ✓ gallery |
| `core/Kbd` | `kbd`(키캡 시퀀스) | `prim_chips` | ✓ gallery |
| `forms/Checkbox` | `checkbox` | `prim_forms` | ✓ port_scanner(필터)+gallery |
| `forms/Switch` | `switch` | `prim_forms` | ✓ gallery |
| `forms/Select` | `select`(토큰 트리거 + egui popup) | `prim_forms` | ✓ gallery |
| `feedback/StatusDot` | `status_dot`(kind+pulse) | `prim_status_dot` | ✓ port_scanner(state) |
| `feedback/Spinner` | `Spinner`(size/color/reduced_motion) | `prim_spinner` | ✓ port_scanner(loading) |
| `navigation/MenuItem` | `menu_item` / `menu_separator` | `prim_nav` | ✓ gallery |
| `navigation/TreeRow` | `tree_row` | `prim_nav` | ✓ gallery |
| `navigation/Tab` | `horizontal_tab_bar_with_arrows`(기존) | Layouts `Pane Tab Bar` | — |
| `data/Table` | `Table`(컬럼 정의[제목·폭·정렬]·정렬 인디케이터·sticky 헤더·행 선택) | Overlays `Port Scanner popup` | ✓ port_scanner |
| `feedback/Toast` | `src/adapters/ui/toast.rs` | Components `Toast (card visual)` | — |

**primitive 케이스 커버리지**: 디자인 jsx 의 변형까지 specimen 에 포함 — Button
`leadingIcon`/`trailingIcon`(prim_button), Input `block`(width 미지정 시 가용폭 채움),
Select `block`(가용폭을 width 로 전달), MenuItem `disabled`(enabled=false).

**시각검증 주**: primitive 13종 전부 시각검증 완료. "✓ port_scanner" = 본체 격리 인스턴스 +
`ui.screenshot`(ui_scale medium) 대조. "✓ gallery" = 갤러리 GPU readback 스크린샷
(`TASTY_GALLERY_SHOT=<idx>:<png> ./target/debug/tasty-gallery`, 지정 specimen 선택→4프레임
settle→캡처→종료)으로 디자인 `components.html` 과 대조. 갤러리는 IPC/OS 캡처가 없어 이
env 일회성 캡처가 격리 자동검증 경로다.

## Layouts (composition specimens)

상위 화면 idiom 데모. 본체 binary 의존 0 — layout·색·폰트·간격은 Theme 토큰, 상태는
thread-local mock. `crates/tasty-gallery/src/catalog/widgets/<name>.rs`.

### 2 depth (Settings idiom)

디자인 `ui_kits/terminal/overlays/settings_window.jsx` ↔ 본체
`src/view/settings/ui.rs`(+ `settings/ui/tabs/*`, `keybindings_tab.rs`) ↔ 갤러리
`widgets/layout_2depth.rs` (Layouts `2 depth (Settings idiom)`).

| 디자인 jsx 컴포넌트 | tasty 함수 (갤러리) | 비고 |
|---|---|---|
| `SettingsWindow`(container, 824×472) | `draw` | 모달 고정폭 `MODAL_W/H` |
| L1 top tabs (underline) | `draw_top_tabs` → `horizontal_tab_bar_with_arrows` | `gallery-alignment §3`: underline fork 금지, scroll-arrows 공유 위젯 유지 (underline = 스킨) |
| L2 sidebar(필터+리스트, 200) | `draw_split` → `two_depth_layout_filtered` | 필터 Input + sub-section 리스트. 패널 폭은 공유 위젯값(`tab_width` 150) — 디자인 settings sidebar 200 과 차이는 공유 위젯 fork 회피로 미적용 |
| `Row`(label-150 + 컨트롤) | `form_row` | gap 16(space-lg)·min-h 32(`--tasty-settings-row-min-height`) |
| `Mono`(섹션 헤딩) | `section_heading` | micro(10)·uppercase·text-muted |
| `Note` | `note` | `measure-md`(400) 폭·text-muted |
| 색 스와치(16, radius 2) | `swatch` | `swatch-size`16·`corner_radius_sm`2·`border_strong` 보더 |
| footer Cancel/Save | `draw_bottom_buttons` | ghost/primary, gap 8 |

form-control 폭: `field-width-{xs,color,md,lg}` = 90/110/160/200 (specimen const, 디자인
`tokens/semantic.css` 미러). content 는 Appearance 탭(Theme/Tasty)을 대표 골격으로 보여준다
(전 7탭 전수 구현 아님 — skeleton).

### Components 재분류

디자인 gallery `components.html` 구조에 맞춰 갤러리 `Components` = primitive 전용으로 정리.
통팝업/컴포지션 데모(Dialog/Convert/Port Scanner/Approval/Toast Stack)는 `Overlays` 로 이동.
