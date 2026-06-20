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
