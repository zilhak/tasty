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

## 이미 갤러리에 있는 관련 항목 (참고)

`catalog/components/` 에 등록된 것: `command_palette` · `port_scanner` · `convert` ·
`approval` · `file_handler_picker` · `markdown_open` · `rename_popup` · `update` · `toast` ·
`sidebar` · `tab_bar` · `apply_preset`. 이들은 props 분리가 돼 있어 갤러리로 즉시 검증 가능.

## Primitive 컴포넌트 레이어 (Components)

디자인 `components/**` 의 atomic primitive ↔ `tasty-ui-widgets` 공용 함수 ↔ 갤러리
`Components` specimen 3자 매핑. 본체 팝업과 갤러리가 **동일** `tasty_ui_widgets::*` 를
호출(mirror 아님 — demo=main). 위젯의 집은 `crates/tasty-ui-widgets/`(메인+갤러리 양쪽 의존).

| 디자인 컴포넌트 | tasty-ui-widgets | 갤러리 specimen | 시각검증 |
|---|---|---|---|
| `core/IconButton` | `IconButton` (ghost/solid/active, sm/md) | `prim_icon_button` | ✓ port_scanner |
| `core/Button` | `Button` (primary/secondary/ghost/danger/agent × sm/md/lg) | `prim_button` | ✓ port_scanner |
| `forms/Input` | `Input` (icon/addon/mono/invalid/disabled, focus ring) | `prim_input` | ✓ port_scanner |
| `core/Tag` | `tag` (default/accent/agent/success/warning/danger + dot) | `prim_chips` | ✓ port_scanner(PID) |
| `core/Badge` | `badge` / `badge_dot` | `prim_chips` | 전사+빌드 |
| `core/Kbd` | `kbd`(키캡 시퀀스) | `prim_chips` | 전사+빌드 |
| `forms/Checkbox` | `checkbox` | `prim_forms` | ✓ port_scanner(필터) |
| `forms/Switch` | `switch` | `prim_forms` | 전사+빌드 |
| `forms/Select` | `select`(토큰 트리거 + egui popup) | `prim_forms` | 전사+빌드 |
| `feedback/StatusDot` | `status_dot`(kind+pulse) | `prim_status_dot` | ✓ port_scanner(state) |
| `navigation/MenuItem` | `menu_item` / `menu_separator` | `prim_nav` | 전사+빌드 |
| `navigation/TreeRow` | `tree_row` | `prim_nav` | 전사+빌드 |
| `navigation/Tab` | `horizontal_tab_bar_with_arrows`(기존) | Layouts `Pane Tab Bar` | — |
| `data/Table` | egui_extras(앱별) | Overlays `Port Scanner popup` | — |
| `feedback/Toast` | `src/adapters/ui/toast.rs` | Components `Toast (card visual)` | — |

**시각검증 주**: "✓ port_scanner" = 본체 격리 인스턴스 + `ui.screenshot`(ui_scale medium)로
대조 완료. "전사+빌드" = 디자인 토큰 충실 전사 + build/clippy 통과(갤러리는 IPC 스크린샷이
없고 OS 캡처는 권한 불가 → 격리 자동검증 미수행). 추가 검증은 해당 위젯을 본체 팝업에
adopt 한 뒤 `ui.screenshot` 으로 한다.

### Components 재분류

디자인 gallery `components.html` 구조에 맞춰 갤러리 `Components` = primitive 전용으로 정리.
통팝업/컴포지션 데모(Dialog/Convert/Port Scanner/Approval/Toast Stack)는 `Overlays` 로 이동.
