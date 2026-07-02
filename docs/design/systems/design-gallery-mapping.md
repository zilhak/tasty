# Design ↔ Gallery ↔ Host 3자 매핑

`design-parity` 작업의 컴포넌트 매핑 기록. 디자인 jsx 하위 컴포넌트 ↔ tasty 호스트 함수 ↔
갤러리(`tasty-gallery`) 카탈로그 항목을 1:1 로 연결한다. 다음 작업이 바로 찾도록 한다.

> 디자인 정합은 **구조 축 + 토큰 축** 둘 다다. 이 매핑(구조 축)으로 함수를 찾았으면, 구조 전사
> 함정은 [design-parity-notes.md](design-parity-notes.md), **토큰 규칙(Theme 토큰 강제·4px·14px·1px·
> 하드코딩 금지)은 [theme.md "UI 디자인 규칙"](theme.md#ui-디자인-규칙-필수)** 을 함께 본다.

갤러리 실행: `cargo run -p tasty-gallery` (상단 toolbar 에서 theme·UI scale 토글, 좌측
카탈로그 선택). 등록: `crates/tasty-gallery/src/catalog/{components,widgets}/<name>.rs` 의
`draw(ui, theme)` + `catalog.rs::all()` 에 `CatalogItem` 한 줄.

## remote_tool (Overlays)

디자인 `ui_kits/terminal/overlays/remote_tool.jsx` ↔ `src/adapters/ui/popup/remote_tool.rs`.

| 디자인 jsx 컴포넌트 | tasty 함수 | 갤러리 항목 |
|---|---|---|
| `RemoteTool`(container) | `draw_remote_tool_popup` | ✗ 미등록 (사유 아래) |
| `TabBtn`(내부, 3탭) | `draw_tab_bar` | `components/remote.rs` `tab_bar` (specimen 미러) |
| `WarnBadge` | `warn_badge` | `components/remote.rs` `warn_pill` (specimen 미러 — 아이콘 없는 pill, gallery jsx 형) |
| `ListShell` | `draw_profile_list` / `draw_attach_list` / `draw_passkey_list` (add-bar+scroll 합침) | — |
| `ProtocolFilter`(add-bar 버튼) | `filter_button` (`draw_profile_list` 내, funnel+라벨) | ✗ 미등록 (remote_tool 예외 동일) |
| `ProtocolFilter`(드롭다운/팝오버) | `draw_protocol_filter` (체크박스 + Apply-on-confirm) | ✗ 미등록 (remote_tool 예외 동일) |
| `ProfileRow` | `draw_profile_row` | `components/remote.rs` `profile_row` (`remote` spec) |
| `ProfileForm` | `draw_profile_form` | — |
| `AttachRow` | `draw_attach_row` | `components/remote.rs` `attach_row` (`remote-attach` spec) |
| `AttachForm` | `draw_attach_form` | `components/remote.rs` `attach_form_card` (`remote-attach-form` spec, ref/inline 2변종) |
| `PasskeyRow` | `draw_passkey_row` | — |
| `PasskeyForm` | `draw_passkey_form` | — |
| `ConfirmDelete` | `draw_confirm_delete` | — |
| `PasskeySelect` | `passkey_dropdown_row` | — |

Attach 갤러리 specimen 은 디자인 **gallery 미러**(`gallery/overlays-shared.jsx` `RemoteFrame
tab="attach"` / `RemoteFormFrame` variant `attach-ref`·`attach-inline`)를 전사한 것으로, 본체
함수 호출이 아니다(컨테이너 미등록 사유와 동일 — 본체는 상태/IO 의존). 디자인 미러가 세그먼트
active 를 `surface-active` 로 그리는 반면 본체(ui_kits jsx)는 `accent-primary` 세그먼트를
쓴다 — changelog(2026-07-01-remote-attach-tab) 명시 사항으로 갤러리/본체가 의도적으로 다르다.

**갤러리 미등록 사유**: `draw_remote_tool_popup` 시그니처가 `(ui, &mut AppState, &mut
CoreState)` 로 호스트 상태에 의존한다(UiState 를 egui ctx memory 에 저장, `RemoteProfiles::
load()` / `Passkeys::load()` 로 파일 IO). 갤러리 `CatalogItem.draw` 는 `(ui, &Theme)` 뿐이라
직접 호출 불가. view-only props 분리(model-view-split) 가 선행돼야 등록 가능. → **후속 과제.**
그 전까지 검증은 본체 `debug.host_popup.open remote_tool` + `ui.screenshot` 로 한다.

프로토콜 필터(`filter_button` / `draw_protocol_filter`)도 같은 예외에 포함된다 — `draw_profile_list`
하위에서 `egui::Context` memory(`read_filter`/`write_filter`, `FILTER_MEMORY_ID`)와 popup
상태(`FILTER_POPUP_ID`)에 의존하므로 `(ui, &Theme)` 시그니처로 분리 불가. 컨테이너가 등록 가능해질
때 함께 등록한다. 검증 경로 동일(`debug.host_popup.open remote_tool` + `ui.screenshot`).

## switch_overlay (Overlays)

디자인 `gallery/overlays.jsx` "Switch-number overlay" 섹션 ↔ 본체 draw 는 **P2 예정**
(`src/adapters/ui/.../tab_bar.rs` 탭 스트립 + `sidebar/{full,collapsed}.rs`). 갤러리 specimen
은 P1 에서 본체보다 먼저 추가됨 (gallery-first, ADR-0020).

| 디자인 jsx 컴포넌트 | 갤러리 항목 (`catalog/components/switch_overlay.rs`) | 본체 함수 |
|---|---|---|
| `NumCap`(키캡) | `num_cap` (헬퍼) — 본체 `kbd()`(`chip.rs`) 형상 재현 + active accent 변종 | ✅ `switch_overlay::paint_keycap` (공통, P2a) |
| `TabStripMock` | `tab_strip` → `draw_tab` (`switch-tab` specimen) | ✅ `tab_bar.rs` `draw_pane_tab_bars_view` (leading 교체, P2a) |
| `WsRowMock` / `SidebarMock` | `full_ws` → `draw_workspace` (`switch-ws` specimen, full) | ✅ `sidebar/view.rs` `draw_workspace_card` (status dot 교체, P2b) |
| `RailMock` | `rail_ws` → `draw_workspace` (collapsed cluster) | ✅ `sidebar/view.rs` `draw_collapsed_sidebar_view` (letter avatar 교체, P2b) |

**본체 배선 (P2a 탭 + P2b 사이드바 모두 구현 완료)**: 공통 모듈 `src/adapters/ui/switch_overlay.rs`
— modifier↔대상 판정(`switch_target_for`, numeric.rs 규칙 1:1) + 키캡
painter(`paint_keycap`, 갤러리 `num_cap` 와 동일 레시피) + 숫자 매핑(`tab_digit` 0~9/`workspace_digit`
1~9). **탭(P2a)**: `tab_bar.rs` wrapper 가 `state.switch_overlay()` 스냅샷(`ModifiersChanged` 로만
갱신, `Tab` 대상이면 focused pane id 동봉)에서 `switch_overlay_pane: Option<u32>` 를 뽑아
`PaneTabBarsProps` 로 전달 → view 는 `tab_keycap_for(switch_overlay_pane, pane_id, i)` 로 **focused
pane 의 탭바에서만** 키캡을 그린다(비-focused pane 은 held 여도 아이콘 유지 — 단축키가 focused pane
탭만 전환하므로). **사이드바(P2b)**: `sidebar/{full,collapsed}.rs` wrapper 가 `ctx.input` modifier +
`engine.settings.keybindings` 로 `workspace_switch_held` bool 을 계산해 `Sidebar{Full,Collapsed}Props.
workspace_switch_held` 로 전달(워크스페이스 전환은 전역이라 pane 한정 불필요). view 가 leading
indicator(탭 아이콘 / ws status dot / rail letter avatar) 자리에 `paint_keycap`. 모두 16px slot
in-place 교체라 리플로 0, release 시 원복. 사용자 입력 modifier 만 보므로 IPC/에이전트 강제
표시 불가(사용자 입력 전용).

**등록**: `catalog.rs` Overlays 페이지 `section("switch", "Switch-number overlay", [spec("switch-tab",
…, draw_tab), spec("switch-ws", …, draw_workspace)])` — search 와 approval 사이(디자인 순서와 동일).
2 specimen(tab / workspace), workspace 는 released / held-full / held-rail 3 cluster.

**키캡 형상 재현 근거**: 본체 `kbd()` 는 inline egui 위젯(자체 allocate)이라 탭 스트립/사이드바
중간의 *정해진 16px slot 좌표*에 끼워 그릴 수 없다. 그래서 tab_bar/sidebar specimen 과 동일하게
painter + Theme 토큰으로 키캡을 좌표 painting 한다(`num_cap`). 레시피는 `chip.rs` 와 1:1
(corner_radius_sm / border_width / 하단 2px / font_size_micro / surface_raised·border_strong·
text_secondary); active 만 accent_primary fill + text_on_accent. 신규 Theme 필드 없음(P0 확정).

## preset demo-layout (Overlays)

디자인 `gallery/preset_editor.jsx` (`SurfaceView`/`Pane`/`PaneTree`/`SurfaceBox`) ↔ 갤러리
`catalog/components/preset_editor.rs` ↔ 본체 `src/adapters/ui/preset/demo_layout.rs`. 저장된
`Preset*` 트리를 **구조만** 축소 렌더하는 read-only 미리보기(TODO 07 Phase 1). 라이브 surface
렌더(터미널 GPU/WebView)는 재사용하지 않고 전용 placeholder 위젯으로 그린다.

| 디자인 jsx 컴포넌트 | 갤러리 (`preset_editor.rs`) | 본체 (`demo_layout.rs`) |
|---|---|---|
| `SurfaceBox` (leaf, kind 라벨만) | `draw_surface_box` | `draw_surface_box` (`Leaf{kind,label}`) |
| `SurfaceView` (하위 surface split, 1px hairline) | `draw_surf` | `draw_surf` (`SurfNode`) |
| `Pane` (mini tab strip + 활성 탭 본문) | `draw_pane_card` | `draw_pane_card` — strip **클릭 가능**(live) |
| `PaneTree` (상위 pane split, 5px bg-app gap) | `draw_pane_tree` | `draw_pane_tree` (`PaneNode`) |
| `PreviewBody` (scope 분기) | `draw_scope_body` | `DemoLayout::show` (`Root::Panes`/`TabFrame`) |
| `KINDS`(아이콘/accent) | `Kind::{icon,accent}` (정적 4종) | `kind_icon`/`kind_accent` (kind str→`icons::Icon`, plugin kind 중립 fallback) |
| `activeKind`(탭 대표 kind) | `tab_kind` | `SurfNode::rep_kind` |
| `SurfaceBox` edit 핸들(remove 단독) | `draw_handle_cluster_mock` | `draw_handle_cluster` (split-right/down 제거 — 경계 존이 대체) |
| `pickZone`/경계 split 존 overlay | `draw_split_zone_overlay_mock` (Left 고정 예시) | `pick_zone` + `draw_split_zone_overlay` (커서 기반 4변 · crosshair · before/row 매핑) |
| mini tab close `×` | `draw_edit_direct_mock` (active rest + hover 예시) | `draw_pane_card` 탭 루프(`show_close` · `Act::RemoveTab`) |
| `AddTabBtn` `+` (22×20 hover) | `draw_edit_direct_mock` (hover 고정) | `draw_pane_card` add-tab(`ADD_TAB_W` · overlay_hover) |

**갤러리 vs 본체 차이**: 갤러리 specimen 은 binary 미의존(정적 샘플 트리·정적 라벨, mini-tab 클릭
전환 없음). 편집 직접조작(경계 split 존·tab ×·add-tab)은 정적이라 hover/pointer/crosshair 축이
없어 **고정 상태 예시**로만 전사한다(정적↔live 차이는 [design-parity-notes](design-parity-notes.md)
"preset 편집기 — 정적 specimen…" 참조). 본체는 실제 `WorkspacePreset`/`TabPreset`/`PanePreset` 을 공통 preview 모델(`SurfNode`/
`PaneNode`/`Root`)로 정규화하고, leaf 라벨을 주입 resolver 로 해석한다. split 방향은 라이브
모델 의미(`Vertical`=좌우/row, `Horizontal`=상하/column, capture·apply 와 일치)를 따른다.

**kind→표시명 (i18n)**: 라벨은 `surface.kind.<kind>` 키로 해석(= registry `display_name_i18n_key`
규약). 호스트 lang 에 빌트인 `terminal`/`empty`/`attached` 키를 추가했고(`lang/{en,ko,ja}.toml`
`[surface.kind]`), plugin kind(markdown/image/…)는 각 plugin lang 의 `[surface.kind]` 가 제공.
`PresetView` 는 main engine 의 공유 `surface_registry` Arc 를 받아 프레임마다 경량 스냅샷
(`KindCatalog`)을 파생한다 — kind 드롭다운 후보는 런타임 등록 kind 를 반영하고, 표시명은
registry `display_name_i18n_key` 로 해석하며(미번역/미등록이면 `fallback_kind_label` capitalize
로 graceful fallback), `empty`/`attached` 는 후보에서 제외한다. registry 미주입(갤러리·main
부재)이면 빈 catalog → 정적 목록으로 떨어진다.

**배선**: `draw_preset_panel`(`src/adapters/ui/preset.rs`)이 선택 preset 으로 `DemoLayout` 을
빌드해 egui temp memory 에 `(key, layout)` 으로 유지(탭 클릭 전환 지속), 남은 영역에 캔버스
프레임 + `DemoLayout::show`/`show_edit` 렌더. `PresetView` 가 파생한 `KindCatalog` 를
`draw_preset_panel → draw_preview → DemoLayout` 으로 흘려 편집 드롭다운·mutation 라벨의
kind 소스로 쓴다.

## workspace-category (Layouts / Overlays)

사이드바 폴더(카테고리) — 확장 그룹 / 축소 레일 `---` / 컨텍스트 메뉴 / 생성·이름변경·삭제
다이얼로그 / 레일 팝업. 갤러리 specimen 은 binary 미의존 정적 재현(Theme 토큰).

| 디자인 jsx 컴포넌트 | 본체 함수 | 갤러리 항목 |
|---|---|---|
| `chrome.jsx` `CategoryHeader` | `sidebar/view.rs::draw_category_header` | `sidebar` "Categories · full" (`sidebar.rs::full_categories`) |
| `chrome.jsx` `Sidebar`(grouped) | `sidebar/view.rs::draw_full_sidebar_view`(+`full.rs::build_category_sections`) | `sidebar` "Categories · full" |
| `chrome.jsx` `RailCategoryBtn` | `sidebar/view.rs::draw_rail_category_button` | `sidebar` "Categories · rail" (`sidebar.rs::rail_categories`) |
| `chrome.jsx` `CollapsedSidebar`(grouped) | `sidebar/view.rs::draw_collapsed_sidebar_view` | `sidebar` "Categories · rail" |
| `overlays/sidebar_context_menu.jsx` `RailCategoryPopup` | `popup/rail_category.rs::draw_rail_category_popup` | `workspace-categories` "Rail popup" (`category_dialogs.rs::rail_popup`) |
| `overlays/sidebar_context_menu.jsx` `SidebarContextMenu` | `view/main/redraw.rs`(native menu: Workspace/WorkspaceCategoryHeader/SidebarBackground) | ✗ native OS 메뉴 — 갤러리 미대상 |
| `overlays-dialogs.jsx` `CategoryEditFrame` | `dialog.rs::draw_rename_popup`(+`RenameTarget::NewCategory`/`CategoryName`, 라이브 검증) | `workspace-categories` "Create / rename" · "Validation error" (`category_dialogs.rs::edit_dialog`) |
| `overlays-dialogs.jsx` `CategoryDeleteFrame` | `popup/confirm_delete_category.rs::draw_confirm_delete_category` | `workspace-categories` "Delete confirm" (`category_dialogs.rs::delete_confirm`) |

**갤러리 vs 본체 차이**: 컨텍스트 메뉴는 OS native(`show_context_menu`) 라 갤러리 정적 재현 대상이
아니다(서브메뉴 미지원 → "카테고리로 이동" 은 평면 나열, 선택지 B). 나머지는 Theme 토큰으로 시각만
재현하며 상태(접힘/빈 카테고리/검증 에러)는 mock 데이터로 주입한다.

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
| `feedback/Tooltip` | `Tooltip`(text/placement/id_source) | `prim_help_hint` | — |
| `feedback/HelpHint` | `HelpHint`(text/placement/open/id_source) — `(?)` 글리프 painter 직접 드로잉 + `Tooltip` 조합 | `prim_help_hint` | — |
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
| `Row`(label-150 + 컨트롤) | `form_row` | gap 16(space-lg)·min-h 32(`--tasty-settings-row-min-height`). `hint` 있는 행은 라벨 뒤 `HelpHint`(placement Bottom, gap space-xs) 인라인 — 아래 `Note` 설명줄과 중복 금지. 본체 적용: `tabs/performance.rs`(2행) · `tabs/appearance.rs::label_with_tooltip`(4곳) |
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

## Plugin settings page (16-B) — `Tasty Design System (3)`

디자인 `ui_kits/terminal/overlays/settings_window.jsx:240-248`(Appearance › HTML viewer 페이지) ↔
본체 `src/view/settings/ui/tabs/appearance.rs` `draw_plugin_settings_page`(+ `plugin_setting_row` /
`draw_plugin_toggle` / `draw_plugin_select` / `draw_plugin_number`) ↔ 갤러리
`components/plugin_settings.rs::draw` (Components › `Plugin settings page`).

**미러 방식**: 갤러리는 main 바이너리에 비의존이므로 본체 렌더러(`Settings` 저장소 read/write 포함)를
그대로 호출할 수 없다. 따라서 행 레이아웃·토큰만 공유 위젯(`tasty_ui_widgets::{switch,select}`)으로
**미러**한다(렌더러 공유크레이트 이전 불필요 — `prim_forms`/`settings` specimen 과 동일 패턴).

| 디자인 jsx | 본체 함수 | 갤러리 미러 | 비고 |
|---|---|---|---|
| `Row`(label 좌 + 컨트롤 우) | `plugin_setting_row` | `row` | `add_space spacing_sm` → horizontal: label(`th.text`) 좌, `right_to_left` 컨트롤 우 |
| `Mono`("HTML viewer") | 페이지 헤더 | mono micro · text-muted | |
| `Default zoom:` `Input`(mono)+`%` | `draw_plugin_number` | `DragValue` + suffix(text-muted) | **차이**: 디자인 text Input ↔ 본체/갤러리 egui `DragValue` (본체 일치 우선). min/max clamp |
| `Color scheme:` `Select` | `draw_plugin_select` | `select`(width `field_width_md`) | follow/light/dark |
| `Allow remote content:` `Switch` | `draw_plugin_toggle` | `switch`(28×16) off | |
| `Sandbox scripts:` `Switch` | `draw_plugin_toggle` | `switch`(28×16) on | |
| `Note` | Note 라벨 | caption · text-muted | |

## Banner (banner-02) — `gallery/overlays.jsx` `#banner` Section

디자인 `gallery/overlays.jsx` 의 `#banner` Section(3 Spec) ↔ 갤러리
`crates/tasty-gallery/src/catalog/widgets/banner.rs` (Overlays › `Banner — the floating
top notice`). 네 번째 overlay 패밀리(Modal / Popup / Toast / **Banner**)의 specimen.
본체 구현(banner-03)보다 먼저 만든 gallery-first 산출물.

**전사 방식**: 디자인 Spec 의 정적 레이아웃(shell chrome · 행 구성 · 우상단 슬롯 ·
스택 z-order)을 1:1 전사한다. hover/카운트다운/큐 같은 시간·상호작용 상태는 egui
immediate-mode 정적 specimen 이므로 **각 상태를 나란히 노출**(toast 스택 데모와 동일
관습 — 라이브 상호작용은 kit `banner.html` 담당).

| 디자인 jsx 함수 | 갤러리 함수 | 비고 |
|---|---|---|
| `BannerShellG` | `banner_shell` | surface-raised fill + 1px border-strong + radius-8 + popover shadow, padding 12/8. `opacity`<1 → 전 색 디밍(recessed) |
| `BannerScope` | `faux_scope` | 탭 스트립(28, 비워둠) + 디밍 콘텐츠 + 배너 존(탭바 아래 8px, 양옆 8px). 배너가 탭바를 덮지 않는 위치 관계 전사 |
| `MouseCaptureBannerG` (Spec 1) | `draw` | 예시 배너: mouse 글리프 + 제목 + 본문 + `Shift` kbd hint + action 2(Secondary/Ghost). × 는 기본 숨김 |
| Spec 2 plain/TTL | `draw_dismiss` | plain(× 노출 상태) + TTL(check 글리프 + 우상단 카운트다운 `6`) 두 행을 Column 으로 |
| `TtlBannerG` countdown | `countdown` | mono micro(10)·text-muted·tabular 숫자 |
| `StackDemoG` (Spec 3) | `draw_stack` | 하위(Pane, 40% 디밍, 후면) + 상위(Workspace, warn 글리프, 전면) 두 shell 을 overlap child Ui 로 |

글리프: mouse/check 는 `icons.rs` 에 `MOUSE`/`CHECK` 글리프 추가(warn = 기존
`ALERT_TRIANGLE`, × = 기존 `CLOSE`). 카탈로그 등록은 Overlays 페이지에 `banner`
Section 1개(3 Spec) 추가 — scrim 바로 다음(디자인 NAV 순서).

## warning-callout (Components)

디자인 `ui_kits/terminal/overlays/settings_window.jsx:623-632` (Settings › Terminal ›
TUI 섹션의 OSC 52 경고 박스) ↔ 위젯 `crates/tasty-ui-widgets/src/warning_callout.rs::warning_callout`
↔ 갤러리 `catalog/widgets/warning_callout.rs::draw` (Components › `Warning callout`).
플레인 경고 텍스트(`accent-warning` + `.small()`)를 대체하는, 아이콘 + caption 을
보더 + 틴트 배경으로 감싼 bordered callout.

| 디자인 jsx (css) | 토큰 | 위젯/갤러리 |
|---|---|---|
| `border: 1px solid color-mix(accent-warning 40%, transparent)` | `accent-warning`.gamma_multiply(0.4) + `border_width` | `warning_callout` stroke |
| `background: color-mix(accent-warning 12%, transparent)` | `accent-warning`.gamma_multiply(0.12) | `warning_callout` fill |
| 라운드 박스 | `corner_radius` | Frame corner_radius |
| padding | `spacing_md`(x) / `spacing_sm`(y) | Frame inner_margin |
| 삼각 경고 아이콘 | `icon_glyph_size_sm` + `accent-warning` | `IconPainter` 주입(`ALERT_TRIANGLE`) |
| 본문 문구 | `font_size_caption` + `text-secondary` | wrapping `Label` |

아이콘은 crate 경계상 위젯이 직접 못 그린다 → `IconPainter` 클로저로 외부 주입(본체
`icons::ALERT_TRIANGLE`, 갤러리 `catalog::icons::ALERT_TRIANGLE`). color-mix 는
`gamma_multiply` 알파 감쇠 근사(chip/banner 전례). 카탈로그 등록은 Components 페이지
Hint text Section 바로 다음에 `Warning callout` Section 1개.

## clipboard-viewer (Plugins)

plugin `crates/tasty-plugin-clipboard-viewer/src/view.rs::draw` (egui-mesh 자가 렌더, B4)
↔ 갤러리 `catalog/components/clipboard_viewer.rs` (Plugins › `Clipboard viewer popup`).
갤러리는 plugin crate 비의존이라 *구성*(master-detail + 버튼 목록 + mono 미리보기)을
Theme 토큰 painter mock 으로 전사 — 픽셀 동일성 비목표.

| plugin view.rs | 토큰 | 갤러리 함수 |
|---|---|---|
| 좌우 분할(LEFT_RATIO 0.3) | `separator` 1px divider | `master_detail` (split_x = w×0.3) |
| 선택 타입 Button(primary) | `accent-primary` + `text-on-accent` | `master_detail` 타입 루프(selected) |
| 유휴 타입 Button(secondary) | `surface-raised` + `text-secondary` | 동(idle) |
| 상세 mono 미리보기 | mono `text-primary` | `master_detail` 미리보기 루프 |
| empty 분기(중앙 한 줄) | `text-muted` | `state_box`(empty) |
| read_error 분기 | `accent-danger` | `state_box`(read failed) |

화면 전용 고정값 480×360 / ratio 0.3 은 module const(token-policy §c). 3 상태(types/empty/
read-failed) 를 `StageVariant::Wrap` 으로 나란히 노출.

## git-viewer (Plugins)

디자인 `ui_kits/terminal/overlays/git_viewer.jsx` ↔ plugin `crates/tasty-plugin-git-viewer/src/render.rs`
(egui-mesh 자가 렌더) ↔ 갤러리 `catalog/components/git_viewer.rs` (Plugins › `Git worktree viewer
popup`). git-viewer 팝업은 UiNode tree 가 아니라 **egui-mesh** 로 그린다(ADR-0028 / B3) — plugin 이
자기 egui Context 에서 새 디자인을 직접 페인트하고 host 는 셸(scrim/border/Esc/outside-click)만
소유한다. 갤러리는 plugin crate 비의존이라 같은 구성을 Theme 토큰 mock 으로 전사한다. **specimen
포함 확정**(ADR-0020 완전성). 토큰·구조 정합 목표, 픽셀 동일성 비목표.

| 디자인(jsx) | plugin render.rs | 갤러리 함수 |
|---|---|---|
| `Header`(Git + `Refresh` secondary) | `header` | `header` |
| context strip(worktree · branch · oid pill · path) | `context_strip` | `context_strip` |
| `PaneHead`(uppercase 섹션 strip + count) | `pane_head` | `section_head` |
| `WtRow`(2줄: name+type pill / oid+state pill) | `wt_row` | `wt_row` |
| `ChRow`(status pill + dir/file) | `ch_row` | `ch_row` |
| `CmRow`(oid + refs + summary + author + time) | `cm_row` | `cm_row` |
| `DiffLine`(거터 + 부호 + ± tint / hunk band) | `diff_line`(+`draw_diff` well) | `diff_line`(+`diff_pane`) |
| oid·refs·`main`·hunk = sky | `accent_info` (Tag `Info` 톤) | 동일 |
| current·added·`+` / locked·modified / invalid·deleted·`-` | `accent_success`/`-warning`/`-danger` | 동일 |

`normal`(rail \| Changes/Commits) / `diff` 두 cluster(`StageVariant::Column`)로 하단 pane 의
Commits↔Diff 교체를 함께 노출. Tag `Info`(sky) 톤은 `tasty-ui-widgets` `chip.rs` 에 추가되어
host gallery Tag specimen(prim_chips)에도 노출된다.

## surface viewers (Plugins)

egui-mesh surface(`markdown`/`image`) + webview chrome(`html`) 의 Plugins 페이지 specimen
묶음(각 surface 가 독립 Section). plugin crate 비의존 — plugin render 경로의 토큰·구성만
painter/egui 로 전사.

| surface | plugin draw | 갤러리 specimen | 핵심 토큰 |
|---|---|---|---|
| markdown | `crates/tasty-plugin-markdown/src/render.rs` (`pulldown-cmark` + 토큰 기반 prose 렌더러) | `components/markdown_viewer.rs` | 본문 `text-secondary`(=override subtext1) · 링크 `accent-primary` · 코드 `surface-raised` · 헤딩 `font-size-prose-h1`(20)/`font-size-prose-h2`(14) |
| image | `crates/tasty-plugin-image/src/render.rs` | `components/image_viewer.rs` | 캔버스 `bg-sidebar` · 버튼 `surface-raised`/`border-default` · 파일명·zoom `text-muted` · fallback `IMAGE` glyph |
| html | OS native WebView overlay (`engine/surface_registry/webview_kind.rs`) | `components/html_chrome.rs` | 콘텐츠 토큰 무관 — chrome 만: `bg-panel`/`border-default` 경계 · `GLOBE` glyph · `Spinner` 로딩 · `ALERT_CIRCLE`+`accent-danger` 에러 |

신규 glyph: `icons.rs` SURFACES 에 `IMAGE`(image fallback) · `GLOBE`(webview) 추가. image 는
`viewer`/`no-image` 2 cluster, html 은 `boundary`/`placeholder`/`loading`/`error` 4 cluster,
markdown 은 단일 문서(`StageVariant::Solo`). 화면 전용 고정값(560/360/300, control 버튼 24×20/30×20)은
module const(token-policy §c).

## Misc · Scripts (Lua script manager) — 05 (ADR-0031)

설정 modal Misc 탭 › Scripts 관리 창. 디자인: `ui_kits/terminal/overlays/settings_window.jsx`
(`ScriptManager`/`ScriptRow`/`ScriptPath`/`ScriptChangedBadge`). 갤러리 미러:
`gallery/overlays-shared.jsx` `ScriptManagerFrame({empty})`. changelog: `changelog/2026-07-01-lua-script-manager.md`.

| 디자인 컴포넌트 | 본체 draw | 갤러리 specimen | 핵심 토큰 |
|---|---|---|---|
| `ScriptManager` (헤더+add card+list/empty) | `view/settings/ui/tabs/misc.rs::draw_scripts_subtab` | `catalog/components/script_manager.rs::draw`(list) / `::draw_empty` | 제목 `font-size-max`/semibold · 설명 `text-muted`/`measure-md` |
| `ScriptRow` (glyph/name/path/kbd/actions) | `draw_script_row` | specimen 내 `Row` | 행 하단 `separator` 보더 · name 13/600 `text-primary` |
| `ScriptChangedBadge` | inline | inline | `accent-warning` color-mix(40% border/12% bg) · mono `font-size-micro`(10) + warn glyph 12 |
| `ScriptPath` (중간생략) | `draw_script_path` | inline `Path` | dir=`text-muted` ellipsis-first / file=`text-secondary` full · mono 12 |
| Add card | inline | (list variant만) | `surface-raised` bg + `border-default` + `radius` · 라벨폭 100 · row `settings-row-min-height` |
| Empty state | inline | `empty` variant | 중앙 script glyph 26 + "No scripts registered" 14/`text-secondary` + `measure-sm` 프롬프트 |

**전사 스펙 (jsx inline style → LogicalPx / Theme)**:
- ScriptRow: `align-items:flex-start`, `gap: space-md`(12), `padding: space-sm space-xs`(8/4), 하단 `1px separator`. glyph 16 `text-muted` `margin-top:2`. 중앙 flex1 `min-width:0` col `gap:2`. 우측 `flex:none` `gap: space-sm`(8).
- 우측: 바운드=`Kbd`, 미바운드=이탤릭 "Unbound" `text-disabled`(overlay1) 12. IconButton sm ×3(bind kbd 16 / edit 16 / trash 16).
- rename: inline Input + Save(primary sm)/Cancel(ghost sm), Enter=commit/Esc=cancel.
- remove: inline "Remove?" `text-secondary` 12 + Cancel(ghost sm)/Remove(secondary sm, `accent-danger` 톤).
- 헤더: 좌 "Scripts" `font-size-max` semibold + muted 설명(`measure-md`/`line-height-ui`), 우 "Add script"(secondary sm, plus leadingIcon).

**신규 glyph 필요** (gallery `icons.rs`): `SCRIPT`(file+lines: `M14 3v4a1 1 0 0 0 1 1h4` / `M17 21H7a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h7l5 5v11a2 2 0 0 1-2 2z` / `M9 13h4M9 17h6`), `KEYBOARD`(`rect x2 y6 w20 h12 rx2` + `M6 10h.01…M8 14h8`). 기존 재사용: PLUS/EDIT/TRASH/FOLDER/ALERT_TRIANGLE.
**신규 토큰 없음**(changelog 확인). i18n 12키(`settings.misc.scripts` · `settings.scripts.{description,add,file,display_name,browse,unbound,changed_badge,changed_help,empty_title,empty_body,remove_confirm}`).
