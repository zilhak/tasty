# Design ↔ Gallery ↔ Host 3자 매핑

`design-parity` 작업의 컴포넌트 매핑 기록. 디자인 jsx 하위 컴포넌트 ↔ tasty 호스트 함수 ↔
갤러리(`tasty-gallery`) 카탈로그 항목을 1:1 로 연결한다. 다음 작업이 바로 찾도록 한다.

> 디자인 정합은 **구조 축 + 토큰 축** 둘 다다. 이 매핑(구조 축)으로 함수를 찾았으면, 구조 전사
> 함정은 [design-parity-notes.md](design-parity-notes.md), **토큰 규칙(Theme 토큰 강제·4px·14px·1px·
> 하드코딩 금지)은 [theme.md "UI 디자인 규칙"](theme.md#ui-디자인-규칙-필수)** 을 함께 본다.

갤러리 실행: `cargo run -p tasty-gallery` (상단 toolbar 에서 theme·UI scale 토글, 좌측
카탈로그 선택). 등록: `crates/tasty-gallery/src/catalog/{components,widgets}/<name>.rs` 의
`draw(ui, theme)` + `catalog.rs::pages()` 의 해당 페이지에 `section(...)`/`spec(...)` 한 줄.

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
| (디자인 원본 없음 — 로컬 SSH config 섹션) | `draw_local_ssh_section` / `draw_local_ssh_row` | `components/remote.rs` `local_ssh_header` / `local_ssh_row` (`remote` spec) |
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
쓴다 — 갤러리 미러가 참조하는 `gallery/overlays-shared.jsx`와 본체가 따르는 `ui_kits` jsx,
두 디자인 소스 자체가 서로 다른 세그먼트 색을 쓰고 있어 생긴 차이다. 갤러리는 각자의 디자인
소스를 그대로 전사하므로 이 차이를 임의로 통일하지 않는다.

로컬 SSH config 섹션은 `remote_tool.jsx` 에 대응 컴포넌트가 없다 — 확정 목업이 이 레포에서
정해진 기존 목록의 확장이라, 기존 행 구조(`ProfileRow` 3행 레이아웃의 축약형) + `hsep` +
`Theme` 토큰만 조합해 만든다. 갤러리 specimen 은 본체와 같은 모양을 전사한다.

**갤러리 미등록 사유**: `draw_remote_tool_popup` 시그니처가 `(ui, &mut AppState, &mut
CoreState)` 로 호스트 상태에 의존한다(UiState 를 egui ctx memory 에 저장, `RemoteProfiles::
load()` / `Passkeys::load()` 로 파일 IO). 갤러리 `Spec.draw` 는 `(ui, &Theme)` 뿐이라
직접 호출 불가. view-only props 분리(model-view-split) 가 선행돼야 등록 가능. → **후속 과제.**
그 전까지 검증은 본체 `debug.host_popup.open remote_tool` + `ui.screenshot` 로 한다.

프로토콜 필터(`filter_button` / `draw_protocol_filter`)도 같은 예외에 포함된다 — `draw_profile_list`
하위에서 `egui::Context` memory(`read_filter`/`write_filter`, `FILTER_MEMORY_ID`)와 popup
상태(`FILTER_POPUP_ID`)에 의존하므로 `(ui, &Theme)` 시그니처로 분리 불가. 컨테이너가 등록 가능해질
때 함께 등록한다. 검증 경로 동일(`debug.host_popup.open remote_tool` + `ui.screenshot`).

## remote_attach — RA02 "Add remote workspace" (Overlays)

디자인 `ui_kits/terminal/overlays/remote_attach.jsx` `RemoteAttach` (+ 갤러리 미러
`gallery/overlays-shared.jsx` `RemoteAttachFrame({state})`) ↔ 본체
`src/adapters/ui/popup/remote_attach.rs`.

| 디자인 jsx 컴포넌트 | 갤러리 항목 (`catalog/components/remote_attach.rs`) | 본체 함수 |
|---|---|---|
| `RemoteAttach`(container) | `ra_card` (`header`+`body`+`footer`, 680×460 프레임) | `draw_remote_attach_popup` |
| `RaAttachProfileRow` | `profile_row` (`remote-workspace-attach` spec 좌 pane) | `profile_row` |
| `RaNewWsRow` | `new_ws_row` + `dot_slot_glyph` / `new_ws_error` / `row_separator` (`remote-workspace-attach-new-row` spec, 5상태) | ✗ 미배선 (아래) |
| `RaRemoteWsRow` | `ws_row` (+ `dot_slot_status`) | `ws_row` |
| `RaCenterState` | `center_state` (`remote-workspace-attach-states` spec) | `center_state` |
| `RaInUseBadge` | `badge` | `badge` |
| loaded 렌더 경로(`conn==="loaded"`) | `loaded_pane` (+ `empty_line`) | `draw_right_pane` 의 `Loaded` 분기 |
| footer `Connect` / `Create & connect` | `footer` | `draw_footer` |

**"+ New workspace" 행 (RA02).** 우측 목록의 **첫 행**으로, 원격에 워크스페이스를 하나
만들어 그것을 mirror 하는 경로다(이름/cwd 를 묻지 않는다 — 원격 기본값). 버튼이 아니라
**목록 행**이라 이웃 ws 행과 같은 select-then-confirm 을 따르고, 확정은 footer 가 한다
(그때 라벨이 `Create & connect` 로 바뀐다). 실제 ws 행과는 **세 채널 동시**로 구분한다 —
`plus` 글리프 · accent 라벨 · 행 아래 1px 구분선. 색 하나로만 구분하지 않는다.

- **empty(원격 ws 0개)는 center-state 가 아니다.** loaded 렌더 경로는 **하나**이고, ws 가
  없으면 caps 헤더 + 새 행 하나 + muted 한 줄로 degrade 한다. 그 행은 **미리 선택**돼 있어
  pane 이 뜬 순간부터 footer 가 살아 있다. center-state + CTA 버튼 안은 같은 동작의 확정
  방식이 원격 상태에 따라 둘로 갈리므로 채택하지 않았다.
- **selected 에서만 라벨이 accent → text-primary 로 바뀐다.** accent 를 `surface-active`
  위에 남기면 3.17:1 이라 고른 순간 가장 안 읽힌다. 구분은 글리프·구분선·accent 바가 계속
  진다.
- **글리프는 status-dot 슬롯(8px) 안에서 center.** 14px `plus` 가 슬롯 좌우로 대칭
  overflow 하므로 이름 열의 좌측 정렬선이 아래 ws 행들과 픽셀 동일하다. 갤러리에서는 ws
  행의 dot 도 같은 `dot_slot` 으로 슬롯을 잡는다 — `status_dot` 위젯이 라벨이 비어도 dot
  뒤에 자기 gap 을 할당해서, 그대로 부르면 두 행의 이름 열이 6px 어긋난다.
- **생성 중 / 실패는 행 인라인.** 왕복이 1~3초라 pane 을 통째로 바꾸면 사용자가 읽던 목록을
  버린다(생성 중엔 아래 목록 dim + inert). 실패도 목록을 가리지 않는다 — 실패 후 다음 수가
  보통 기존 워크스페이스 선택이기 때문. 원격 메시지는 3줄 clamp + 전문은 tooltip.

**본체 미배선**: 갤러리 specimen 이 gallery-first 로 먼저 들어갔고, 본체
`draw_right_pane`/`draw_ws_list` 배선(+ 원격 `workspace.create` 왕복)은 후속 작업이다
(ADR-0020, [gallery-first](../../dev-guide/gallery-first.md)).

## switch_overlay (Overlays)

디자인 `gallery/overlays.jsx` "Switch-number overlay" 섹션 ↔ 본체 draw 는 **P2 예정**
(`src/adapters/ui/tab_bar.rs` 탭 스트립 + `sidebar/{full,collapsed}.rs`). 갤러리 specimen
은 P1 에서 본체보다 먼저 추가됨 (gallery-first, ADR-0020).

| 디자인 jsx 컴포넌트 | 갤러리 항목 (`catalog/components/switch_overlay.rs`) | 본체 함수 |
|---|---|---|
| `NumCap`(키캡) | `num_cap` (헬퍼) — 공용 위젯 `tasty_ui_widgets::num_keycap` 호출 | ✅ `switch_overlay::paint_keycap` (공통, P2a) — 같은 그림을 `paint_num_keycap` 으로 호출 |
| `TabStripMock` | `tab_strip` → `draw_tab` (`switch-tab` specimen) | ✅ `tab_bar.rs` `draw_pane_tab_bars_view` (leading 교체, P2a) |
| `WsRowMock` / `SidebarMock` | `full_ws` → `draw_workspace` (`switch-ws` specimen, full) | ✅ `sidebar/view.rs` `draw_workspace_card` (status dot 교체, P2b) |
| `RailMock` | `rail_ws` → `draw_workspace` (collapsed cluster) | ✅ `sidebar/view.rs` `draw_collapsed_sidebar_view` (letter avatar 교체, P2b) |
| `CatSwitchSidebarMock` | `full_cat` → `draw_category` (`switch-cat` specimen, full) | ✅ `sidebar/view.rs` (헤더 우측 키캡, `category_switch_held`) |
| `CatSwitchRailMock` | `rail_cat` → `draw_category` (collapsed cluster) | ✅ `sidebar/view.rs` `draw_rail_category_button` (`---` 중앙 키캡) |

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

**카테고리 quick-switch (기본 Ctrl+Shift, `draw_category`)**: 카테고리는 자기 modifier 필드
(`category_switch_modifier`, 기본 `"ctrl+shift"`)를 갖는 **독립 1급 축**이다 — 과거 "workspace
오버레이(Alt) + Shift 파생" 방식은 폐기됐다. `switch_target_for` 가 세 축(탭/워크스페이스/카테고리)
각각의 modifier 조합을 `Combo::parse_modifiers` 로 파싱해 현재 눌린 조합과 **정확히 일치**할 때만
그 축을 반환하므로(modifier-exclusive, 우선순위 로직 없음) 세 축이 서로 새지 않는다. full 은
카테고리 헤더 **우측**에 키캡(chevron 은 load-bearing 이라 교체 안 함, status dot 없음), rail 은 `---` 경계
**중앙**에 키캡. 번호는 reserved normal("Workspaces")=1, 1–9 then 0(10th), 11th+ 없음. 전환 시 접힘이면 자동
확장(슬롯 파일 영속) + 그 카테고리 last-active 착지(`state/workspace.rs` `switch_to_category`, 다음/이전
카테고리 자체 전환은 `next_category`/`prev_category` 가 이 함수를 재사용). folders 토글 게이트.
discoverability 는 modifier-hint 패널의 `HintRole::CategorySwitch`(폴더 글리프, folders on).

**"개별 지정" 모드와 오버레이**: 세 축 중 하나라도 modifier 를 "개별 지정"
(`KeybindingSettings::INDIVIDUAL_SWITCH_MODIFIER`)으로 바꾸면 그 축은 `switch_target_for` 가 절대
반환하지 않으므로(sentinel 파싱 실패) 이 switch-number 오버레이가 그 축에서 자동으로 뜨지 않는다 —
슬롯마다 콤보가 달라 통일된 숫자 힌트를 그릴 근거가 없기 때문(의도된 동작, [keybindings](../../features/keybindings/index.md) 참조).

**등록**: `catalog.rs` Overlays 페이지 `section("switch", "Switch-number overlay", [spec("switch-tab",
…, draw_tab), spec("switch-ws", …, draw_workspace), spec("switch-cat", …, draw_category)])` — search 와
approval 사이(디자인 순서와 동일). 3 specimen(tab / workspace / category), workspace·category 는 released /
held-full / released-rail / held-rail cluster.

**왜 painter 갈래가 따로 있나**: 본체 `kbd()`·`num_keycap()` 은 inline egui 위젯(자체
allocate)이라 탭 스트립/사이드바 중간의 *정해진 16px slot 좌표*에 끼워 그릴 수 없다. 그래서
`chip.rs` 가 그림을 `paint_num_keycap(painter, theme, center, ..)` 로 뽑아 두고, `num_keycap`
은 자리를 할당해 그것을 부르고 본체 `paint_keycap` 은 좌표를 넘겨 그것을 부른다 — **갈리는
것은 자리 계산까지고 형상은 한 벌**이라 레시피 동기화가 필요 없다. 색·치수는
`switch-overlay-*` component 토큰(전부 `kbd-*` 별칭, active 만 accent_primary /
text_on_accent)에서 온다. 신규 Theme 필드 없음(P0 확정).

## preset demo-layout (Overlays)

디자인 `gallery/preset_editor.jsx` (`SurfaceView`/`Pane`/`PaneTree`/`SurfaceBox`) ↔ 갤러리
`catalog/components/preset_editor.rs` ↔ 본체 `src/adapters/ui/preset/demo_layout.rs`. 저장된
`Preset*` 트리를 **구조만** 축소 렌더하는 read-only 미리보기다. 라이브 surface
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
규약). 호스트 lang 에 빌트인 `terminal`/`empty` 키를 추가했고(`lang/{en,ko,ja}.toml`
`[surface.kind]`), plugin kind(markdown/image/…)는 각 plugin lang 의 `[surface.kind]` 가 제공.
`PresetView` 는 main engine 의 공유 `surface_registry` Arc 를 받아 프레임마다 경량 스냅샷
(`KindCatalog`)을 파생한다 — kind 드롭다운 후보는 런타임 등록 kind 를 반영하고, 표시명은
registry `display_name_i18n_key` 로 해석하며(미번역/미등록이면 `fallback_kind_label` capitalize
로 graceful fallback), `empty` 는 후보에서 제외한다. registry 미주입(갤러리·main
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

## 공용 crate view specimen (복제 0 — 본체와 같은 함수 호출)

본체 view 를 `crates/tasty-ui-widgets` 로 올려 **본체 wrapper 와 갤러리 specimen 이 같은
함수를 호출**하는 항목. 아래 "시각 복제 specimen" 과 달리 레이아웃·색·치수를 갤러리가
재선언하지 않으므로 시각 동기화가 자동이며 수동 검증이 필요 없다. 새 bar/패널은 복제보다
이 경로를 우선한다([gallery-first](../../dev-guide/gallery-first.md)).

| 디자인 canonical | 공용 crate view | 본체 wrapper | 갤러리 specimen |
|---|---|---|---|
| `ui_kits/terminal/work.jsx` `StatusBar` (하단 24px 바, 좌 컨텍스트 / 우 액션) | `tasty_ui_widgets::draw_status_bar_view` (`crates/tasty-ui-widgets/src/status_bar.rs`, `StatusBarData`→`StatusBarDrawResult`) | `src/adapters/ui/status_bar.rs::draw_status_bar` (Area·z-order·i18n 라벨 주입·action 적용) | `statusbar` (Layouts › Status bar, `components/status_bar.rs::draw`) |
| `gallery/overlays.jsx` `NumCap` (16px 숫자 키캡) | `tasty_ui_widgets::paint_num_keycap` (`crates/tasty-ui-widgets/src/chip.rs`; 레이아웃 갈래는 같은 파일의 `num_keycap`) | `src/adapters/ui/switch_overlay.rs::paint_keycap` (slot 좌표·등장 페이드 alpha) | `switch-overlay` (Overlays, `components/switch_overlay.rs::keycap_at`) |

crate 쪽 view 가 **소유하지 않는 것**(=본체 wrapper 잔류): `egui::Area` 와 `LayerId`
(부유 배치·z-order 는 본체 정책), i18n 라벨·tooltip 문자열(위젯 crate 는 `tasty-i18n`
비의존 — `multi_select` 와 동일 정책), 글로벌 `theme()` 를 읽는 `status_bar_bottom_inset`.

## Overlay 시각 복제 specimen (본체 의존 0)

본체 view 의 시각만 로컬 mock props 로 복제한 Overlays 항목. 본체 binary crate(`tasty`)에
의존 불가하므로 layout·색·폰트·간격·보더는 모두 Theme 토큰에서 가져오고 상태는 mock 으로
주입한다. 본체 view 변경 시 시각 동기화는 수동 검증.

| 디자인 canonical | 본체 view | 갤러리 specimen |
|---|---|---|
| `overlays/search_bar.jsx` (360×28) | `src/adapters/ui/search_bar.rs::draw_search_bar` | `search_bar` (Overlays) |
| `overlays/tools_menu.jsx` (160px) | `src/adapters/ui/tools_menu.rs::draw_tools_menu` | `tools_menu` (Overlays) |
| (시안 없음 — 확정 토큰 + `icons.json` `close`/`fit` 조합뿐이라 신규 시각 결정이 없었다, 근거 → [fullscreen-stage §디자인 소스](fullscreen-stage.md#디자인-소스--신규-시안-없이-만든-이유)) | `src/adapters/ui/fullscreen.rs::draw_fullscreen_stage`(셸: scrim+제목+종료 버튼) | `fullscreen-stage` (Overlays, `components/fullscreen_stage.rs::draw`) |
| (시안 없음 — 기존 타이틀바 + `fit` 글리프, 근거 위와 같음) | `src/adapters/ui/popup/draw.rs`(타이틀바 전체화면 버튼) | `fullscreen-stage-titlebar` (Overlays, `components/fullscreen_stage.rs::draw_titlebar`) |

## Overlays — plugins window

디자인 `ui_kits/terminal/overlays/plugins_window.jsx` (820×540 모달) ↔ 본체 `src/view/plugins/`
↔ 갤러리 `Plugins manager window` (Overlays). 본체 binary 의존 0 — 로컬 mock 데이터로 시각 복제.

본체는 `TopBottomPanel`/`SidePanel` 을 `Context` 에 직접 붙여 창 전체를 채우므로 갤러리가 그
함수를 호출할 수 없다 — 같은 구조를 rect 기준으로 전사한다. 전사할 고정 창 크기가 본체에
없어서 무대 크기는 토큰으로 조립한다(`LIST_W + measure_md` × `measure_sm`). 디자인의 820×540
은 여기 들어오지 않는다. (Layouts 의 `1-depth (general shell)` specimen
`crates/tasty-gallery/src/catalog/widgets/layout_1depth.rs` 은 이 창이 아니라 **리스트→상세
배치 관용구 자체**를 보이는 별개 specimen 이다.)

| 디자인 jsx 컴포넌트 | 본체 | 갤러리 함수 (`crates/tasty-gallery/src/catalog/components/plugins_window.rs`) |
|---|---|---|
| `PluginsWindow`(container) | `src/view/plugins/ui.rs` `draw_plugins_panel` | `window` + `stage_size` — 탭 상태 `Tab`(Installed / Attention / Add{preview}) 로 본문이 갈린다 |
| header + `Seg` 세그먼트 | 같음(헤더 밴드) | `header` / `segment_tab` — Installed \| Attention(danger 배지) \| Add plugin. 필터 입력은 Installed 탭에서만 |
| installed list+detail | `src/view/plugins/ui/list.rs` `draw_list_tab` | `plugins_window/installed.rs`: `list_pane` / `detail_pane` — 상세 블록 열셋 전량(빈 상태 · health error 박스 · Status/Configure · Surface kinds · Permissions · Commands · Install path/Log · Uninstall 2 분기 포함) |
| `AttentionPanel` (4케이스) | `src/view/plugins/ui/attention.rs` `draw_attention_tab` | `plugins_window/attention.rs`: `list_pane` / `detail_pane` / `banner` / `reason_detail` / `action_bar` / `reason_cards` |
| `AddPluginForm` (trust 흐름) | `src/view/plugins/ui/add.rs` `draw_add_tab` | `plugins_window/add.rs`: `input_pane` / `preview_pane` / `untrusted_warning` |
| `PluginAvatar` | (없음) | (없음) — 디자인에만 있는 컴포넌트다 |

severity 는 본체 `src/view/plugins/ui.rs` `is_danger` 를 따른다 — 서명 계열만 danger, 권한
변경·런타임 오류는 warning. Installed 목록의 health dot 과는 다른 축이다(health dot 은 실행 중
실패 하나만 본다).

검증: specimen 이 여덟 상태(Installed 넷 — 선택 · health error · 무선택 · uninstall 확인,
Attention 둘 — 목록 있음 · 빈 상태, Add 둘 — 경로입력 · 매니페스트 프리뷰)를 세로로 모두
그리므로 탭 전환 없이 대조한다. Installed 무대만 상세가 길어 `measure_xl` 로 높다 — 본체는
그 자리를 `ScrollArea` 로 접지만 갤러리는 접으면 캡처에서 사라진다. 페이지는 Overlays(idx 3)
이고 이 섹션은 그 페이지 맨 아래라 스크롤 오프셋을 준다 — 정확한 y 는 위에 섹션이 늘면 밀리므로
오프셋 몇 개를 한 배치로 훑어 고른다([screenshot-methods](../../ai-verification/screenshot-methods.md)).

```bash
TASTY_GALLERY_SIZE=1400x2500 TASTY_GALLERY_SHOT="3@36500:/abs/a.png,3@39000:/abs/b.png,3@41500:/abs/c.png" \
  ./target/debug/tasty-gallery
```

본체 대조는 Plugins 창을 띄우고 그 창 id 로 찍는다. 이 창은 사이드바 버튼에서만 열리고 그
경로를 여는 IPC 가 없으므로(원칙 1 — 사용자 조작 재현은 release 에 없다), 열기는 창 클릭으로
한다. 창 제목은 `Tasty Plugins` 다([screenshot-methods](../../ai-verification/screenshot-methods.md)
의 창 제목 표).

```bash
tasty screenshot --path /abs/host.png --window <Tasty Plugins 창 id>
```

## Specimen 공용 헬퍼 (dedup)

specimen 간 중복 chrome 을 한 곳으로 모은 카탈로그 헬퍼 (`crates/tasty-gallery/src/catalog/`):

| 헬퍼 | 제공 | 쓰는 곳 |
|---|---|---|
| `spec.rs` | `section` / `spec` / `stage`(`StageVariant`) / `cluster` / `meta`(`TokenChip`) / `note` / `do_` / `dont` | 카탈로그 106 개 `.rs` 중 96 개 |
| `toast_card.rs` | `accent_color` / `draw_card` (`CardColors`) | toast(components/widgets) |
| `popup_frame.rs` | `draw` (`ContentInset` · `TitleButtons`) — surface-raised 프레임 + border-strong + 타이틀바 우측 버튼군(`draw_title_buttons`: close X / 전체화면 `fit`) | approval · convert · file_handler_picker · dialog · fullscreen_stage |

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
| `forms/MultiSelect` | `multi_select` / `multi_select_summary` / `multi_select_popup_id` (`select` 와 같은 트리거 토큰 + checkbox 행 팝업 + `CloseOnClickOutside` + 요약 라벨 3분기 + 메뉴 max-height 스크롤/max-width 클램프 + 행 단위 disabled 마스크 + 일괄 선택/해제 액션 행(opt-in, accent + separator, 스크롤 밖 고정) + 키보드 내비(↓/Enter/Space 열기 · ↑↓/Home/End active 행 이동(disabled 건너뜀) · Space/Enter 토글(안 닫힘) · Esc 닫기(포커스 유지) · Tab 닫고 이동, active 행은 `surface_active` 배경)) | `prim_forms` | ✓ gallery |
| `forms/AutoComplete` | `AutoComplete` / `autocomplete_dropdown` (Input 트리거 + menu container + MenuItem 행 middle-ellipsis + substring 필터 + match highlight + max-height 스크롤) | `prim_autocomplete` | ✓ gallery |
| `plugins.jsx/PathField`(:59) | `PathField` / `PathFieldOutcome` (AutoComplete 트리거 + Go IconButton, 편집/이동/원복 결정 = markdown `addr_outcome` 포팅, idle=secondary/editing=primary) | `prim_path_field` | ✓ gallery |
| `feedback/StatusDot` | `status_dot`(kind+pulse) | `prim_status_dot` | ✓ port_scanner(state) |
| `feedback/Spinner` | `Spinner`(size/color, 모션은 `Theme` 이 결정 · reduced_motion 은 override) | `prim_spinner` | ✓ port_scanner(loading) |
| `feedback/Tooltip` | `Tooltip`(text/placement/id_source) | `prim_help_hint` | — |
| `feedback/HelpHint` | `HelpHint`(text/placement/open/id_source) — `(?)` 글리프 painter 직접 드로잉 + `Tooltip` 조합 | `prim_help_hint` | — |
| `navigation/MenuItem` | `menu_item` / `menu_separator` | `prim_nav` | ✓ gallery |
| `navigation/TreeRow` | `tree_row` | `prim_nav` | ✓ gallery |
| `navigation/Tab` | `horizontal_tab_bar_with_arrows`(기존) | Layouts `Pane Tab Bar` | — |
| `navigation/DrillDown` | `DrillDown` / `DrillDownView` / `DrillDownOutput` (controlled list⇄detail content-swap, back bar ←(ghost IconButton sm)+제목+actions 슬롯, 본문 내부 스크롤, 0ms 즉시 전환 — opt-in animate 는 장식이라 미전사) | `prim_drilldown` | — |
| `data/Table` | `Table`(컬럼 정의[제목·폭·정렬]·정렬 인디케이터·sticky 헤더·행 선택) | Overlays `Port Scanner popup` | ✓ port_scanner |
| `data/ListCtrl` | `ListCtrl` / `ListCtrlItem` / `ListCtrlOutput` (label+description+leading icon+trailing 슬롯+drill-in chevron, divided 헤어라인, selected surface-active+2px accent 좌측 바, disabled, empty_label) | `prim_listctrl` | — |
| `feedback/Toast` | `src/adapters/ui/toast.rs` | Components `Toast (card visual)` | — |

**primitive 케이스 커버리지**: 디자인 jsx 의 변형까지 specimen 에 포함 — Button
`leadingIcon`/`trailingIcon`(prim_button), Input `block`(width 미지정 시 가용폭 채움),
Select `block`(가용폭을 width 로 전달), MenuItem `disabled`(enabled=false).

**시각검증 주**: primitive 15종 전부 시각검증 완료(multi_select: gallery readback — 닫힘/열림/연속 3토글 후에도 팝업 유지/바깥클릭 닫힘, 요약 라벨 3분기, 트리거 치수가 단일 `select` 와 동일(28×160px)함, 옵션 20종에서 메뉴가 max-height(220)에서 멈추고 긴 라벨이 max-width(320)에서 말줄임됨 확인. 키보드 내비는 갤러리에 키 주입 경로가 없어 본체 DAG 목록 필터에서 `debug.inject_egui_key` + `ui.screenshot` 으로 검증 — ↓↓ 로 짚은 행 배경이 `surface_active`(rgb 88,91,112) 로 실측되고 메뉴 배경(30,30,46)과 갈림, Space/Enter 토글 뒤에도 팝업 유지 + 트리거 요약 즉시 갱신, Esc 는 드롭다운만 닫고 부모 popup 은 유지하며 트리거 포커스가 남아 곧바로 ↓ 로 다시 열림. autocomplete: gallery scroll readback — idle/open/filtered+highlight/overflow→scroll/empty/keyboard-active·middle-ellipsis 확인). "✓ port_scanner" = 본체 격리 인스턴스 +
`ui.screenshot`(ui_scale medium) 대조. "✓ gallery" = 갤러리 GPU readback 스크린샷
(`TASTY_GALLERY_SHOT=<idx>:<png> ./target/debug/tasty-gallery`, 지정 specimen 선택→4프레임
settle→캡처→종료)으로 디자인 `components.html` 과 대조. 갤러리는 IPC/OS 캡처가 없어 이
env 일회성 캡처가 격리 자동검증 경로다.

## Layouts (composition specimens)

상위 화면 idiom 데모. 본체 binary 의존 0 — layout·색·폰트·간격은 Theme 토큰, 상태는
thread-local mock. `crates/tasty-gallery/src/catalog/widgets/<name>.rs`.

### 1 depth (general list → detail)

`crates/tasty-gallery/src/catalog/widgets/layout_1depth.rs`(`onedepth`). **대응하는 본체
함수가 없다** — 특정 창이 아니라 좌측 고정 리스트(200) → 우측 detail 배치 관용구 자체를
보이는 데모다. Plugins 창의 미러는 이것이 아니라
`crates/tasty-gallery/src/catalog/components/plugins_window.rs` 이고, 그쪽은 목록 폭을 본체와
같은 접근자 `Theme::plugins_side_panel_width`(240)에서 읽고 행 높이도 40 이다(위
[Overlays — plugins window](#overlays--plugins-window) 절). 필터가 놓이는 자리도 다르다 —
본체 Plugins 창의 필터는 헤더 밴드 우측이고 이 idiom 데모는 목록 안이다.

### 2 depth (Settings idiom)

디자인 `ui_kits/terminal/overlays/settings_window.jsx` ↔ 본체
`src/view/settings/ui.rs`(+ `settings/ui/tabs/*`, `keybindings_tab.rs`) ↔ 갤러리
`components/settings.rs` (Overlays `settings` specimen). 그 L2 200 · L1 44 는 본체
`SETTINGS_SIDEBAR_WIDTH`(200) · `SETTINGS_HEADER_HEIGHT`(44) **값과 일치**하나 컴파일 연동은
아니다 — 갤러리 크레이트가 본체 bin 의 비공개 상수를 참조할 수 없어 값을 로컬로 들고
관례로 맞춘다(200 은 리터럴, 44 는 `titlebar_height + spacing_sm` 도출).
Layouts 의 `widgets/layout_2depth.rs`(`twodepth`)는 이 미러가 아니라 특정 창에 매이지
않는 일반 2-depth idiom(168/40, 토큰 도출)이다 — 혼동 금지.

| 디자인 jsx 컴포넌트 | 본체 (`src/view/settings/ui.rs`) | 갤러리 (`components/settings.rs`) | 비고 |
|---|---|---|---|
| `SettingsWindow`(container, 824×472) | `draw_settings_panel` | `draw` | 모달 고정폭 `MODAL_W/H` |
| L1 top tabs (underline) | `draw_l1_tab_band` | `l1_band` / `l1_tab` | `gallery-alignment §3`: underline fork 금지 (underline = 스킨). **공유 위젯을 쓰지 않는다** — 양쪽 다 자기 `Frame` 으로 밴드를 그린다. 좌측 타이틀·세로 구분선이 탭과 같은 줄에 들어가야 해서 탭만 담는 컨테이너에 안 맞는다 |
| L2 sidebar(필터+리스트, 200) | `draw_l2_sidebar` | `l2_sidebar` / `l2_item` | 필터 Input + sub-section 리스트. **양쪽 다 200** 이고 공유 위젯을 쓰지 않는다 — 본체는 모달 셸이 소유하는 `SidePanel`(오른쪽 1px vline), 갤러리는 같은 폭의 `Frame`. `tasty_ui_widgets::two_depth_layout_filtered` 는 콘텐츠 안에 놓이는 둥근 테두리 패널(`SUB_TAB_PANEL_WIDTH` 150)이라 **다른 idiom** 이다 |
| `Row`(label-150 + 컨트롤) | 공통 헬퍼 없음 — 탭마다 따로(`tabs/remote_transfer.rs` `settings_row` · `tabs/appearance.rs` `plugin_setting_row` 등) | `row` | gap 16(space-lg)·min-h 32(`--tasty-settings-row-min-height`). `hint` 있는 행은 라벨 뒤 `HelpHint`(placement Bottom, gap space-xs) 인라인 — 아래 `Note` 설명줄과 중복 금지. 본체 적용: `tabs/performance.rs`(2행) · `tabs/appearance.rs::label_with_tooltip`(4곳) · `keybindings_tab/entries.rs`(2행 — `close_active`/`quit`, right-to-left 라벨 컬럼이라 HelpHint를 라벨보다 먼저 add) |
| `Mono`(섹션 헤딩) | — | `mono` | micro(10)·uppercase·text-muted |
| `Note` | — | `note` | `measure-md`(400) 폭·text-muted |
| 색 스와치(16, radius 2) | — | `theme_swatch` | `swatch-size`16·`corner_radius_sm`2·`border_strong` 보더 |
| footer Cancel/Save | `draw_settings_footer` | `footer` | ghost/primary, gap 8 |

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

디자인 `ui_kits/terminal/overlays/clipboard_viewer.jsx` ↔ plugin
`crates/tasty-plugin-clipboard-viewer/src/view.rs::draw`(egui-mesh 자가 렌더, B4) ↔ 갤러리
`catalog/components/clipboard_viewer.rs` (Plugins › `Clipboard viewer popup`). 좌측 rail
master-detail 레이아웃은 폐기됐다 — header→type-bar→body→footer 4단 수직 스택으로
구조 전사. 갤러리는 plugin crate 비의존이라 그 *구성*을 Theme 토큰 painter mock 으로
전사 — 픽셀 동일성 비목표.

| plugin view.rs | 토큰 | 갤러리 함수 |
|---|---|---|
| header(아이콘+타이틀+snapshot 뱃지+close) | `text-muted`/`font-size-max`/`tag` Default | `header_row` |
| type-bar(≤1: 뱃지, ≥2: 세그먼트) | `bg-sidebar` 행 + `tag` Accent(≤1) / `border-default`+`accent-primary`(≥2) | `type_bar_row`(text) / `type_bar_segmented_row`(Text/Files) / `image_type_bar_row`(image, 우측에 meta 텍스트) / `type_bar_row_html`(우측 Pretty print 체크박스) / `other_type_bar_row`(Other 뱃지) |
| body well(text/html) | `bg-app` fill + `separator`+`border-width` + `corner-radius`, mono 스크롤 | `body_row` / `body_row_text`(임의 문자열) |
| body well(files) | 위와 동일 + 아이콘(`text-muted`)+mono 경로 한 줄씩 | `files_body_row` |
| body well(image — 인라인 렌더 없음) | 위와 동일 fill/border, 콘텐츠는 중앙 정렬(아이콘 30px 고정 + `text-muted` + mono caption 메타 + `text-disabled` italic 안내) | `image_body_row` |
| body well(other) | 위와 동일 fill/border, 포맷 블록마다 이름(`text-secondary` 굵게)+크기(`text-muted`) 같은 줄 + 미리보기(`text-primary`), 블록 사이 `separator` 1px | `other_body_row` |
| footer(mime+Close) | `font-size-caption` mono + `Button` Secondary mock | `footer_row`(text) / `footer_row_files` / `image_footer_row`(image, `image/rgba8`) / `footer_row_html`(`{mime} · {meta}`) / `other_footer_row`(`{n} unrecognized formats` 가 mime 을 대체) |
| CenterState(empty/read-failed/already-open) | 아이콘(28px) + `font-size-body` 굵은 타이틀 + `font-size-term-sm` 옅은 부제 | `center_popup` |

화면 전용 고정값 480×360 은 module const(token-policy §c). 9 상태(data-text/data-files/image/
html-raw/html-pretty/other/empty/read-failed/already-open) 를 `StageVariant::Wrap` 으로 나란히
노출. `SEG_COMPACT_AT`(5) 이상의 압축 세그먼트는 실 데이터가 5종(Text/Files/Image/Html/Other)뿐
이라 동시에 전부 co-occur 하는 시나리오가 흔치 않아 아직 specimen 에 없다.

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

egui-mesh surface(`image`) + webview surface/chrome(`markdown`/`html`) 의 Plugins 페이지
specimen 묶음(각 surface 가 독립 Section). plugin crate 비의존 — plugin render 경로의 토큰·구성만
painter/egui 로 전사. markdown 은 [ADR-0065](../../adr/0065-markdown-webview-render-channel.md)로
Stage B 부터 image 와 다른 채널(webview)로 이동했지만, html 과 달리 (콘텐츠가 없는 chrome-only
specimen 이 아니라) 실제 CSS 출력 내용까지 손으로 전사한다 — plugin 이 아직 host chrome 을 얹지
않는 대신 문서 자체(주소창 포함)를 통째로 생성하기 때문.

| surface | plugin draw | 갤러리 specimen | 핵심 토큰 |
|---|---|---|---|
| markdown | `crates/tasty-plugin-markdown/src/render.rs` (`pulldown-cmark` → `ammonia` sanitize → CSS custom property 주입, native OS WebView 가 렌더) | `components/markdown_viewer.rs` | 본문 `text-secondary`(=override subtext1) · 링크 `accent-primary` · 코드 `surface-raised` · 헤딩 `font-size-prose-h1`(h1)↔`font-size-body`(h6) CSS 5단계 선형보간(`prose-h2`·`line-height-prose` 은퇴 유지 — CSS custom property `--md-h1`..`--md-h6` 로 대체) |
| image | `crates/tasty-plugin-image/src/render.rs` | `components/image_viewer.rs` | 캔버스 `bg-sidebar` · 버튼 `surface-raised`/`border-default` · 파일명·zoom `text-muted` · fallback `IMAGE` glyph |
| html | OS native WebView overlay (`engine/surface_registry/webview_kind.rs`) | `components/html_chrome.rs` | 콘텐츠 토큰 무관 — chrome 만: `bg-panel`/`border-default` 경계 · `GLOBE` glyph · `Spinner` 로딩 · `ALERT_CIRCLE`+`accent-danger` 에러 |

신규 glyph: `icons.rs` SURFACES 에 `IMAGE`(image fallback) · `GLOBE`(webview) 추가. image 는
`viewer`/`no-image` 2 cluster, html 은 `boundary`/`placeholder`/`loading`/`error` 4 cluster,
markdown 은 단일 문서(`StageVariant::Solo`). 화면 전용 고정값(560/360/300, control 버튼 24×20/30×20)은
module const(token-policy §c).

## Misc · Scripts (Lua script manager) — 05 (ADR-0031)

설정 modal Misc 탭 › Scripts 관리 창. 디자인: `ui_kits/terminal/overlays/settings_window.jsx`
(`ScriptManager`/`ScriptRow`/`ScriptPath`/`ScriptChangedBadge`). 갤러리 미러:
`gallery/overlays-shared.jsx` `ScriptManagerFrame({empty})`.

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
**신규 토큰 없음** — `script_manager.rs`/`draw_scripts_subtab`가 쓰는 토큰은 전부 기존
`spacing_*`/`font_size_*`/`text_*`/`accent_warning`/`border_default` 등 범용 접근자다.
i18n 12키(`settings.misc.scripts` · `settings.scripts.{description,add,file,display_name,browse,unbound,changed_badge,changed_help,empty_title,empty_body,remove_confirm}`).

## Settings › Keybindings › Preset drill-down (settings-preset-drilldown)

디자인 `ui_kits/terminal/overlays/settings_window.jsx` `PresetSubtab`/`PresetDiffTable` ↔ 본체
`src/view/settings/ui/keybindings_tab/preset.rs`. 구 좌(120px)/우 split 을
공용 위젯 [`DrillDown`+`ListCtrl`](#primitive-컴포넌트-레이어-components) 소비로 재작성 —
두 위젯의 첫 본체 소비처.

| 디자인 jsx | 본체 | 비고 |
|---|---|---|
| `PresetSubtab` (DrillDown 루트, `view` controlled) | `draw_preset_subtab` | 뷰 상태 = `selected_preset: Option<String>` (None=List / Some=Detail) |
| list wrapper (`padding: space-md space-lg`, gap space-sm) | list 클로저 `Frame::inner_margin(symmetric(lg, md))` | 인트로 `<p>`(12/muted/measure-md) = `intro_note` |
| `<ListCtrl items selectedId={activeId}>` | `ListCtrl::show(..., active_idx)` | Active(사용 중) = draft 와 전 일반 바인딩 일치 프리셋. trailing `Tag`(success·dot) "Active" |
| back bar `actions` = Apply(primary sm, disabled=Applied) | `DrillDownActions` 클로저 + `Button` | 클릭 신호는 `Cell` 로 회수 (`&dyn Fn` 불변 계약) |
| `PresetDiffTable` (grid `minmax(0,1.6fr) 1fr 1fr`) | `draw_preset_diff_table` (수동 갤리 페인트) | 헤더 mono micro(10) uppercase muted + separator 헤어라인. 셀 padding space-sm/space-md. Action=body(13) text-secondary, 바인딩 2열=mono term-sm(12), 변경=`accent-primary`(색상만, bold 없음) |
| `fullBleed` (Keybindings›Preset 만 표준 래퍼 우회) | `ui.rs` content 디스패치 `full_bleed` 분기 | DrillDown 이 자체 패딩+내부 스크롤 소유 |

**헤더 close ✕ 제거 (Request 1)**: `draw_l1_tab_band` 의 `marginLeft:auto` ghost close ✕ 삭제
(닫기 = footer Cancel + OS 타이틀바). 갤러리 `components/settings.rs` L1 밴드 미러도 동일.
**신규 토큰 없음** (위젯 토큰은 [design-token-mapping §drilldown/listctrl](design-token-mapping.md) 참조).
i18n: `settings.keybindings.preset_*` 신규 10키 + `select_preset_label`/`preset_col_before` 문구 갱신,
`preset_col_after` 제거 (3열 헤더 = 프리셋 이름).

## Settings › Handler 탭 서브탭 콘텐츠 (S13)

L1 "File Handler" 를 **Handler** 로 일반화(내부 key `FileHandler` 유지)하고 Hook Handlers
서브탭을 추가한 개편. 디자인: `ui_kits/terminal/overlays/settings_window.jsx`
(`L1_LABEL`·`L2.FileHandler`·`HookHandlers`/`HookRow`/`SEED_HOOKS`/`HOOK_ORIGIN`).

| 디자인 컴포넌트 | 본체 draw | 갤러리 specimen | 핵심 토큰 |
|---|---|---|---|
| `body()` File Extension Mapping 분기 | `view/settings/ui/file_handler_tab/extension_mapping.rs` | `catalog/components/settings_handler.rs::draw_extension_mapping` | ext mono 12 `text-secondary` · row `settings-row-min-height` · `separator`(마지막 행 없음) |
| `body()` File Detectors 분기 | `view/settings/ui/file_handler_tab/detectors.rs` | `::draw_detectors` | name 13 `text-secondary` + desc 12 `text-muted` · Switch 우측 |
| `body()` File Handlers 분기 | `view/settings/ui/file_handler_tab/handlers.rs` | `::draw_file_handlers` | name 13 + `Tag`(kind) + Switch(marginLeft auto) |
| `HookHandlers` (intro+add card+list) | `view/settings/ui/file_handler_tab/hook_handlers.rs::draw_hook_handlers` | `::draw_hook_handlers` | intro 12 `text-muted`/`measure-md` · add card `surface-raised`+`border-default`+`radius`, 라벨폭 100 |
| `HookRow` (2줄 행) | `hook_handlers.rs::draw_hook_row` | specimen 내 `draw_hook_row` | id mono 13/600 `text-primary` · origin `Tag`(plugin=`agent` variant) · `prio N` mono `font-size-micro` · disabled 시 row `opacity-disabled` · 하단 `separator` · Shell cmd 라벨폭 74/`font-size-caption` + mono `Input` |

**전사 노트**:
- jsx `headStyle`(mono 10 uppercase `letter-spacing-caps`)은 egui letter-spacing 미지원 —
  기존 관례(mono `font-size-micro` uppercase `text-muted`)로 전사.
- **신규 토큰 0** — `hook_handlers.rs`/`settings_handler.rs`가 쓰는 토큰은 전부 기존
  `spacing_*`/`font_size_*`/`text_*`/`border_*` 등 범용 접근자이며 이 기능 전용으로
  추가된 Theme 필드가 없다. 화면 전용 고정값(라벨폭 74/100, priority step 10)은
  module const(token-policy §c).
- 본체 Hook Handlers 는 레지스트리 정책 적용으로 jsx 와 두 곳이 다르다: 제거 버튼은
  user-origin 행만(호스트/플러그인 base 는 finalize 가 되살림), IpcSequence 행은 인라인
  편집 대신 mono 요약. intro copy 의 priority 방향은 엔진 규약(낮을수록 먼저)으로 기술.

## Settings › General › Remote transfer (07-UI)

General L1 에 5번째 L2 서브탭 "Remote transfer" 추가 — 원격 mirror 파일 전송(06/07)
수신측 저장 정책(`RemoteTransferSettings{dir, max_mb}`) 편집. 디자인:
`gallery/overlays-shared.jsx` `SettingsRemoteTransferFrame` + `gallery/overlays-windows.jsx`
"Settings · General › Remote transfer" spec. 백엔드는 이미 merge(d6eeecf5), 이번은 UI 만.

| 디자인 jsx 컴포넌트 | 본체 함수 | 갤러리 항목 |
|---|---|---|
| `SettingsRemoteTransferFrame`(콘텐츠 컬럼) | `src/view/settings/ui/tabs/remote_transfer.rs::draw_remote_transfer_tab` | `components/settings_remote_transfer.rs::draw` (`settings` 섹션 `settings-remote-transfer` spec) |
| `Mono`("Received files") | `mono` 헤딩(micro uppercase muted) | `mono_head` |
| `Row`(Save folder, grid 150px + control) | `settings_row` + right_to_left(Browse→Input) | `xfer_row` |
| `Input block mono` + `Button secondary sm folder`(Browse…) | `Input::mono` + `Button::Secondary/Sm/FOLDER` + `rfd::FileDialog::pick_folder` | 동(rfd 없이 시각만) |
| `Row`(Maximum size) + `Input mono width88` + 정적 `MiB` | `settings_row` + `Input::mono.width(field_width_xs)` + mono muted "MiB" 라벨 (정수 버퍼 파싱, `draw_plugin_number` 선례) | `xfer_row` + 동 |
| `Note`(행별 muted 설명) | `row_desc`(caption muted) | `row_desc` |
| 행 사이 `borderTop separator` | `row_separator`(`th.separator` hline) | `separator_line` |

**전사 노트**:
- 라벨 컬럼 150px(`gridTemplateColumns: "150px 1fr"`)·행 gap 12(space-md)·행 높이
  `settings_row_min_height`(32). 콘텐츠 wrapper 패딩은 공유 `tab_content_frame`(space-lg)
  가 제공(형제 탭 관례 — 재패딩 안 함).
- **size Input 폭**: 디자인 `width: 88` 은 field-width 토큰 세트(90/110/160/200) 밖 specimen
  값 → mono narrow numeric 토큰 `field_width_xs`(90)로 매핑(host·gallery 동일, 2px 차).
- **"MiB" 는 필드 밖 정적 mono suffix**(Toast 의 " s" 와 동형, addon/Tag 아님). i18n 단위
  기호 예외로 리터럴.
- **신규 Theme 필드 0** — 전부 기존 접근자(`settings_row_min_height`/`field_width_xs`/
  `separator`/`text_muted`/`font_size_micro`/`font_size_caption`)·기존 위젯(`Input`/`Button`).
  i18n 8키(`settings.tab.remote_transfer` + `settings.remote_transfer.{section,dir,dir_placeholder,dir_desc,browse,max_capacity,max_capacity_desc}`).
- 갤러리는 본체 미의존이라 host `draw_remote_transfer_tab`(Settings 저장소 의존)을 직접
  못 부르고 같은 위젯·토큰으로 미러(settings_handler 서브탭 specimen 전례). rfd 폴더 피커는
  specimen 에서 no-op.

## File picker (Overlays) — gallery-first 반영 완료, 본체 배선됨

디자인 `gallery/overlays-shared.jsx` `FilePickerFrame`/`FpRow`/`FpCrumbs`/`FpHostBadge`
+ `gallery/overlays-windows.jsx` `#filepicker` Section(스펙 3개) ↔ 갤러리
`catalog/components/file_picker.rs`.
design-request: `design-request/07151555-design-request-remote-file-picker.md`. **본체
(egui `PopupDef`) 구현 완료** — `src/adapters/ui/popup/file_picker.rs`(`FILE_PICKER_POPUP_ID`
= `"file_picker"`, `draw_file_picker`)가 `defs.rs`에 등록되어 있다(커밋 `519d98f0`,
2026-07-23).

640×480 단일 컴포넌트가 로컬/원격 두 모드를 겸한다 — 차이는 헤더 host indicator 와
브레드크럼 root 뿐, 레이아웃은 불변. §6.1 열린 결정(원격 표시 A 배지 / B 글리프 /
C 프레임보더) 중 **A 배지가 사용자 확정**되어 갤러리는 A만 코드화한다 — B/C 는
미채택 대안이라 반영하지 않는다.

| 디자인 jsx 컴포넌트 | 갤러리 함수 (`file_picker.rs`) | 비고 |
|---|---|---|
| `FilePickerFrame`(container) | `card` | 640×480 · bg-panel · border-strong · modal shadow |
| header(glyph·title·host indicator·✕) | `header` | 글리프 항상 `FILE`(B안의 remote 글리프 스왑 미반영) |
| host 배지(§6.1 A안, 채택) | `host_badge` | mono `user@host` · `accent-info` 14%/45% 배경/보더 |
| path bar(`FpCrumbs`+refresh) | `path_bar` → `crumbs` | root=mono, 중간=accent 링크, current=bold 비클릭 |
| list header(NAME/SIZE/MODIFIED) | `list_header` | loaded/multi 상태만, `cols()` 좌표 공유 |
| `FpRow` | `row` | selected=surface-active+2px accent 좌측바, focus=1px accent outline(선택과 구분) |
| 로딩/빈폴더/에러(권한·연결끊김) | `center`(state 분기) | Spinner · folderOpen · ALERT_TRIANGLE + Retry/Reconnect |
| footer(name field+type filter+Cancel/Open) | `footer` + `type_filter_chip` | `kit::field` 재사용, Open 은 loaded 상태에서만 활성 |

**갤러리 vs 디자인 차이**: 긴 파일명 말줄임은 jsx `text-overflow:ellipsis`(CSS 네이티브)
대신 `elide()`(문자 단위 폭 측정 후 컷 + `…`)로 근사. 브레드크럼 세그먼트별
`maxWidth:180` ellipsis 는 미반영(시드 문자열이 짧아 발생하지 않음 — 실 데이터 연결 시
`elide()` 재사용 검토). **신규 Theme 필드 0** — 전부 기존 semantic 접근자
(`accent_info`/`surface_active`/`accent_primary`/`text_placeholder`/`bg_sidebar` 등)와
기존 위젯(`kit::field`/`checkbox`/`Spinner`/`Button`/`IconButton`)으로 해소.

## Remote file transfer 팝업 (Overlays) — 진행 + 실패 (09)

디자인 `gallery/overlays-shared.jsx` `TransferProgressFrame`(09a) / `TransferErrorFrame`(09b)
↔ 본체 `src/adapters/ui/popup/transfer.rs`(PopupDef `transfer_progress` / `transfer_error`)
↔ 갤러리 `catalog/components/transfer.rs`. 06 bulk 전송 + 08 이미지 paste 업로드에 대한
사용자 피드백 UI. **진행은 시스템 최초 determinate progress bar**(indeterminate `Spinner` 와 구분).

| 디자인 jsx | 본체 함수 (`popup/transfer.rs`) | 갤러리 함수 (`components/transfer.rs`) |
|---|---|---|
| `TransferProgressFrame`(container) | `draw_transfer_progress` (PopupDef draw_fn) | `progress_card` |
| 헤더(download glyph + "Receiving file" + mono pct) | `header_band` | `header_band` |
| 파일 행(file glyph + mono ellipsis name) | `progress_row` | `progress_row` |
| determinate 4px bar(track+fill) | `progress_bar` | `progress_bar` |
| done/total · rate (mono muted, space-between) | `progress_row` 내 | `progress_row` 내 |
| ghost Cancel(footer) | `footer_buttons` + `Button::Ghost` | `footer_buttons` + `Button::Ghost` |
| `TransferErrorFrame`(container) | `draw_transfer_error` (PopupDef draw_fn) | `error_card` |
| 헤더(warn glyph + "Transfer failed") | `header_band`(ALERT_TRIANGLE·accent-danger) | `header_band` |
| prose(`<b>name</b> could not be received.`) | `horizontal_wrapped` mono bold + 산문 | 동 |
| reason well(command-well: bg-app+separator, mono danger) | `reason_well` | `reason_well` |
| Dismiss / (mid-transfer)Retry (danger-fill 금지) | `footer_buttons`(Secondary/Ghost) | 동 |

**본체 vs 갤러리 차이**: 갤러리는 main 바이너리 비의존이라 `draw_transfer_*`(DialogState 의존)을
직접 못 부르고 같은 구조·토큰으로 미러(정적 seed 데이터). scrim dim 은 본체 `draw.rs` 가 그리므로
갤러리 specimen 은 프레임을 클러스터에 **직접** 렌더한다(scrim 스테이지 미사용 — file_picker 관례,
[design-parity-notes](design-parity-notes.md) "transfer — scrim_backdrop 스테이지…" 참조). 진행
determinate bar 는 `Spinner` 처럼 위젯화하지 않고 painter 인라인(track `bg_app` + fill `accent_primary`,
0ms). **신규 Theme 필드 0** — 전부 기존 접근자([design-token-mapping §transfer](design-token-mapping.md#remote-file-transfer-progresserror-09) 참조).
i18n 6키(`transfer.progress.{title,cancel}` · `transfer.error.{title,body_suffix,dismiss,retry}`).

**본체 배선(06/08)**: 진행률은 `upload_file_over_bulk` 에 `on_progress(sent,total)` 콜백을 추가해
청크마다 통지 → 08 워커가 `transfer_progress` 채널로 흘림 → `drain_transfer_progress` 가 행 갱신.
실패는 08 `drain_image_upload_results` 의 `Err` 분기를 (구) Warning toast 에서 실패 팝업으로 승격 —
`BULK_REJECT_PREFIX`(원격 거부) 면 Dismiss 단독, 아니면 Retry(재큐잉). 상세
[features/remote-attach](../../features/remote-attach/index.md).

## Attention kind — NeedsInput 배지/dot/테두리/탭 제목 (surfaces, ADR-0062)

디자인 `components/core/Badge.jsx`(variant `warning`) + `components/feedback/StatusDot.jsx`
(status `needs-input`/`completion`) ↔ 본체 `src/adapters/ui/{divider,tab_bar,sidebar/view}.rs`
↔ 갤러리 `catalog/components/{occupancy_borders,sidebar,tab_bar}.rs`(surfaces 섹션 기존
specimen 확장 — 신규 파일 없음). 요청·확정 절차는 [`ADR-0062`](../../adr/0062-attention-store-kind-aware-primitive.md)
가 정한 kind-aware 모델을 그대로 따르며, 토큰 값은 [design-token-mapping §attention
kind](design-token-mapping.md#attention-kind--needsinputcompletion-surface-highlight-adr-0062)
참조.

| 디자인 컴포넌트/variant | 본체 함수 | 갤러리 함수 | 비고 |
|---|---|---|---|
| `Badge variant="warning"` | `sidebar/view.rs::paint_workspace_count_badge`(`BadgeVariant::Warning`) | `sidebar.rs::paint_ws_badge_pair`/`paint_ws_count_badge_at` | NeedsInput 개수 배지(좌측 슬롯) |
| `Badge variant="primary"`(기존) | 동 함수(`BadgeVariant::Primary`) | 동 | Completion 개수 배지(우측, 기존 자리) — 색 로직만 variant 분기로 리팩터, 렌더 값 불변 |
| `BadgeGroup`(gap) | `right_to_left` 레이아웃 + `ui.add_space(spacing_xs)` | `paint_ws_badge_pair` offset 계산 | `badge-group-gap` 전사, 위젯화하지 않고 인라인 |
| `StatusDot status="needs-input"` | `sidebar/view.rs::draw_collapsed_avatar` 우상단 dot 분기 | `sidebar.rs::attention_rail_demo` | collapsed rail — kind 우선순위로 대표색 1개 |
| `StatusDot status="completion"`(기존 notif) | 동 | 동 | 값 불변(파랑), 분기 순서만 needs-input 다음으로 |
| surface border(occPane 확장) | `divider.rs::highlight_stroke_color`/`regions_from_state` | `occupancy_borders.rs::occ_pane`(`Kind::NeedsInput`) | 우선순위: NeedsInput > 점유 > Completion |
| 탭 제목 색(위계) | `tab_bar.rs` `text_color` match(kind) | `tab_bar.rs::attention_strip` | 기존 "divergence: accent_warning 값-보존" 주석 해소(Completion 이 이제 정말 파랑) |

**신규 Theme 필드 0** — 전부 기존 semantic 접근자(`accent_warning`/`accent_primary`/
`text_on_accent`/`focus_ring_width`/`spacing_xs`)로 해소([design-token-mapping
§attention kind](design-token-mapping.md#attention-kind--needsinputcompletion-surface-highlight-adr-0062)
참조). `AttentionKind`/`AttentionLevel`(host, `src/core/state/attention.rs`)이 색 선택의
SoT — 갤러리는 binary 비의존이라 동일 우선순위·색을 정적 데모 데이터로 미러한다(라이브
attention 상태에 연결되지 않음, 다른 surfaces specimen과 동일 관례).

## Task DAG — surface · canvas · node (Layouts)

디자인 `gallery/dag.jsx` (카탈로그 페이지) + `ui_kits/terminal/overlays/dag_view.jsx`
(상태 어휘 · 노드 카드 · 러너 배지 · 크롬 · 빈 상태) + `dag_surfaces.jsx` (캔버스 · 노드
상세 · 풀탭 서피스 · 워크스페이스 popup) ↔ 본체 `src/adapters/ui/surface/dag_graph/` +
`src/adapters/ui/popup/dag_list.rs` ↔ 갤러리 `catalog/components/dag/` (Layouts 페이지
`dag-graph` · `dag-shell` · `dag-list` 세 섹션).

| 디자인 jsx 컴포넌트 | tasty 함수 | 갤러리 항목 |
|---|---|---|
| `DagCanvas` | `canvas::draw_canvas` | `dag/canvas.rs::paint` (`dag-canvas` spec, 전사 미러) |
| `dagLayout()` | `tasty_dag_layout::layout_dag` | 동 crate 직접 호출 (미러 아님 — 아래) |
| `elbow()` | `canvas::orthogonalize` + `round_corners` | `dag/edges.rs::elbow` / `orthogonalize` / `round_corners` |
| `DagNode` | `node::paint_node` | `dag/node.rs::paint_card` (`dag-node`/`dag-kinds`/`dag-lod` spec) |
| `DAG_STATUS` / `DAG_KIND` / `DAG_REL` | `model::{DagStatus, DagRelation}` | `dag.rs::{Status, Kind, Rel}` |
| `RunnerBadge` | `chrome::runner_badge` + `resume_hint` (헤더 우측) | `dag/runner.rs::paint_badge` + `row` (`dag-runner` spec) |
| 재개 힌트 캡션 | `chrome::resume_hint` — lead 비례폭 + 명령 mono 2 조각 | `dag/runner.rs::row` (동일 2 조각) |
| `ZoomCluster` | `chrome::draw_zoom_cluster` (캔버스 우하단 — `draw_canvas_chrome` 안) | `dag/chrome.rs::paint_zoom_cluster` (`dag-chrome` spec) |
| `Minimap` | `chrome::paint_minimap` (줌 클러스터 바로 위) | `dag/chrome.rs::paint_minimap` (`dag-chrome` spec) |
| `CycleBanner` | `chrome::draw_cycle_banner` | `dag/chrome.rs::paint_cycle_banner` (`dag-states` spec) |
| LOD 힌트 칩 | `chrome::paint_lod_chip` | `dag/chrome.rs::paint_lod_chip` (캔버스 안) |
| `DagEmpty` | `chrome::draw_empty` | `dag/chrome.rs::paint_empty` (`dag-states` spec) |
| `DagDetail` / `DetailRow` / `LogBlock` | `detail::draw_detail` / `row` / `labeled_block` | `dag/detail.rs::draw_body` (`dag-detail` spec) |
| `DagSurface` | `render::draw_dag_graph` (헤더는 `chrome::draw_header`) | `dag/surface.rs::paint` (`dag-surface` spec) |
| `dagRowItems` (DAG 목록 행) | `popup::dag_list::draw_row_trailing` | `dag/rows.rs::trailing` (`dag-rows` spec) |
| `DagWindow` (워크스페이스 popup) | `popup::dag_list::draw_dag_list_popup` | `dag/window.rs::paint` (`dag-window` spec) |

**전사 미러인 이유**: `render::draw_dag_graph` 는 `(ui, &DagGraphSurface, &mut DagGraphView)`
로 호스트 상태(폴링 스냅샷 · 줌/오프셋 · 선택)에 의존하고, 갤러리는 main 바이너리를 의존할
수 없다 — `remote_tool` 컨테이너 미등록 사유와 같다. 다만 DAG 는 **좌표 계산만은 미러가 아니라
같은 코드**다: `tasty-dag-layout` 이 egui/Theme 를 모르는 순수 계산 crate 라 갤러리가 그대로
의존한다(`crates/tasty-gallery/Cargo.toml`). 디자인 jsx 의 `dagLayout()` 은 시안용 최단 구현
(longest-path + 중앙정렬)이라 sugiyama 결과와 좌표가 다르고, 갤러리는 **본체가 실제로 그리는
좌표**를 보여야 하므로 엔진 쪽을 따른다.

**의도적 디자인 대비 차이 (갤러리·본체 공통)**

- **상태 글리프**: 시안의 `❯`(U+276F) `✓`(U+2713) `✗`(U+2717) 은 Dingbats 블록이라 UI 비례
  폰트에서 tofu 로 떨어진다. 본체는 기하 도형(`◦ ▷ ◑ ● × ⊘ ◇ ?`)으로 치환했고
  `tests/design_token_adherence.rs::no_raw_pictographic_glyph` 가 그 블록을 host UI 소스에서
  금지한다. 갤러리도 같은 치환 세트를 쓴다 — 렌더되지 않는 글자를 전시하면 정합 판정 자체가
  무의미하기 때문이다.
- **러너 재개 힌트 문구**: 시안은 `tasty dag runner start` 를 적지만 그런 CLI 는 없다. 본체와
  갤러리 모두 실제 명령(`tasty agent task-run --workspace-id <N> --action start`)을 쓴다.
- **기본 방향**: 시안 기본은 top-down, 본체 기본은 left-right(`DagDirection::LeftRight` —
  `agent.task_graph --format dot` 의 `rankdir=LR` 과 멘탈 모델 일치, 168×48 카드가 가로로
  길어 화면 폭을 아낌). 갤러리 specimen 은 시안대로 top-down 으로 전시한다.
- **줌 클러스터의 fit / 방향 아이콘**: 시안·specimen 은 `move` 와 `swap` 을 쓰지만 본체는
  `fit` 과 방향을 그대로 비추는 `arrow-right`/`arrow-down` 을 쓴다. `move` 는 "끌어서
  옮긴다" 로 읽혀 실제 동작(그래프 전체가 들어오게 배율을 맞춘다)과 어긋나고, `swap` 은
  방향이 바뀐다는 것만 말할 뿐 **지금** 어느 방향인지를 못 보여준다 — 방향 버튼은 눌러서
  바뀔 결과가 아니라 현재 상태를 읽는 쪽이 그래프와 대조하기 쉽다. 클러스터의 위치 ·
  크기 · 셀 구성(`− % + | fit dir`) · 토큰은 시안 그대로다.
- **popup 디테일 뷰의 헤더**: 시안 `DagWindow` 의 디테일은 헤더 없이 캔버스 + 시트만 두고
  러너 배지를 back bar 의 actions 슬롯에 놓는다. 본체는 그래프 화면 한 벌
  (`render::draw_dag_graph`)을 통째로 재사용하므로 **헤더가 함께 온다** — 러너 배지 · DAG
  선택 · 새로고침이 그 안에 있고(줌 클러스터는 캔버스 우하단), 중복을 피하려 actions 슬롯은
  비운다. 렌더를 두 벌로 가르지 않는 쪽을 택한 결과다.

  갤러리 `dag-window` specimen 은 시안 구조(헤더 없음 · back bar 에 배지)를 그대로 전사하고
  **그대로 둔다** — 러너 배지의 정합 기준 specimen 은 풀탭 서피스 쪽 `dag-surface`
  (`dag/surface.rs::paint`) 이고, 그쪽이 본체 헤더(배지 · picker · 새로고침)와 1:1 로 맞는다.
  `dag-window` 를 본체에 맞춰 고치면 시안 원본을 잃고, 본체를 `dag-window` 에 맞추면 렌더가
  두 벌로 갈린다. 두 specimen 은 **같은 화면의 두 배치**를 보여주는 것이고, 본체는 그중
  풀탭 배치 하나만 구현한다.
