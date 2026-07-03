# 단축키 (Keybindings)

- **Status**: Implemented
- **주체**: 로컬 사용자
- **ADR**: 없음 (정책은 [design/policies/key-mapping](../../design/policies/key-mapping.md))
- **코드**: `crates/tasty-settings/src/keybindings.rs` (+ `crud.rs` · `presets.rs`)
- **화면**: [설정 창](../settings/screens/settings.md) Keybindings 탭

## 목적

tasty 의 **모든 단축키는 `KeybindingSettings` 한 곳에서 정의**되며 코드에 하드코딩되지 않는다([CLAUDE.md](../../../CLAUDE.md) "단축키" 필수 정책). 사용자가 Settings 의 Keybindings 탭에서 액션별 키 조합을 추가/삭제/변경한다. OS 메뉴(macOS NSMenu / Windows AcceleratorTable)의 key equivalent 도 이 binding 을 따라간다.

## 내부 동작

### 액션 ↔ 바인딩 목록

각 액션은 **바인딩 문자열의 `Vec`** 를 가진다(다중 바인딩 — 한 액션에 여러 키 조합 허용). 예: `copy`, `paste`, `enter_copy_mode`, `apply_workspace_preset` 등. 빈 `vec` 이면 그 액션엔 단축키가 없다(메뉴엔 단축키 없는 항목으로 표시).

바인딩 문자열은 **OS 독립 표기**다 — 위치 기반 추상화로 macOS 에선 `alt`→⌘ 등으로 매핑된다([key-mapping](../../design/policies/key-mapping.md)).

### 탭/워크스페이스 quick-switch (raw 키)

번호 전환·다음/이전 이동은 **콤보가 아니라 raw 키 하나**만 저장하는 별도 필드로 다룬다. modifier 는 `tab_switch_modifier`/`workspace_switch_modifier`(각각 기본 `ctrl`/`alt`)에서 dispatch 시점에 조합되므로, modifier 드롭다운을 바꾸면 모든 슬롯이 즉시 재조합된다. 이 필드들은 콤보 시스템(`GENERAL_BINDING_FIELDS`/`get_bindings`)과 분리되며 index 기반 accessor(`tab_slot_key`/`set_tab_slot_key` 등, `crud.rs`)로 접근한다.

| 필드 | 타입 | 기본값 | 의미 |
|------|------|--------|------|
| `tab_switch_slot_keys` | `[String; 10]` | `["1".."9","0"]` | 탭 1~10번 슬롯 |
| `workspace_switch_slot_keys` | `[String; 9]` | `["1".."9"]` | 워크스페이스 1~9번 슬롯(0번 없음) |
| `category_switch_slot_keys` | `[String; 10]` | `["1".."9","0"]` | 카테고리 1~10번 슬롯(reserved normal=1). `workspace_switch_modifier`+Shift 로 합성 |
| `tab_switch_next_key` / `tab_switch_prev_key` | `String` | `"l"` / `"h"` | 탭 다음/이전 |
| `workspace_switch_next_key` / `workspace_switch_prev_key` | `String` | `"j"` / `"k"` | 워크스페이스 다음/이전 |

이 필드들 모두 필드별 `#[serde(default = "…")]` 를 가져, 신규 필드가 없는 구버전 config 를 읽어도 빈 값이 아니라 위 기본값으로 복원된다. 자유 콤보용 `next_tab`/`prev_tab` 필드와는 별개다(Command Palette·더블탭 경로 전용, quick-switch 가 건드리지 않음). `category_switch_slot_keys` 는 별도 modifier 필드 없이 `workspace_switch_modifier`+Shift 를 재사용하며(index accessor `category_slot_key`/`set_category_slot_key`), 현재 Settings quick-switch UI 에는 편집기가 없어 기본값으로 동작한다(대상 판정·dispatch 만 배선).

#### quick-switch 섹션 UI (Tab/Workspace 서브탭)

Tab·Workspace 서브탭의 일반 콤보 목록 아래에 **quick-switch 섹션**이 있다(`keybindings_tab/quick_switch.rs`). 구성:

1. **modifier 드롭다운** — `tab_switch_modifier`/`workspace_switch_modifier`(Ctrl/Alt) 선택.
2. **슬롯 1~N 버튼** — 탭 1~10번 / 워크스페이스 1~9번. 각 버튼 라벨은 저장된 raw 키를 현재 modifier 와 **표시 시점에 합성**한 `"{Modifier}+{Key}"`(예: `Ctrl+1`). modifier 드롭다운을 바꾸면 저장값(raw 키) 변경 없이 라벨이 즉시 재조합된다.
3. **다음/이전 버튼 2개** — `*_next_key`/`*_prev_key`.

버튼을 누르면 **bare-key 녹화**로 진입한다(`capture_bare_key`). 일반 콤보 녹화(`capture_winit_key_combo`)와 정반대로, **modifier 가 하나라도 눌리면 그 입력은 무효**(대기 유지)이고 modifier 없는 순수 키 하나만 유효하다. Escape 는 슬롯을 비운다. 캡처 분기는 `RecordingSlot.field_kind`(`Combo`/`BareKey(BareTarget)`)로 결정되고, `SettingsUiState::recording_is_bare_key()` 가 winit 이벤트 캡처 경로를 가른다(`view/settings.rs`).

**충돌 검사**는 합성 콤보 `"{modifier}+{key}"` 기준으로 두 축을 본다: ① 일반 액션과의 충돌(`find_conflict` — `next_tab`/`prev_tab` 포함), ② 다른 quick-switch 슬롯과의 중복(슬롯 배열 자체 순회, 탭↔워크스페이스 교차 포함). 충돌 시 기존 확인 팝업(`PendingBinding`)을 재사용하며, accept 시 상대가 일반 필드면 그 바인딩을, 다른 슬롯이면 그 슬롯을 비운다. 또한 modifier 변경 등으로 현재 슬롯 콤보가 일반 액션과 겹치면 섹션 하단에 경고 라벨을 표시해 조용히 넘기지 않는다.

#### dispatch — 키 입력 → 전환

실제 키 소비는 `input/shortcuts/numeric.rs`(`handle_numeric_switch_shortcuts`)가 담당한다. `Key::Character` 이면(슬롯 키가 `"q"` 같은 문자일 수 있으므로 숫자 여부를 따지지 않는다) 대상(Tab/Workspace/Category)을 switch-number 오버레이와 **단일 소스**인 `switch_target_for(kb, ctrl, shift, alt)` 로 판정한다 — Tab/Workspace 는 modifier 가 `tab_switch_modifier`/`workspace_switch_modifier` 와 **단독** 일치할 때, Category 는 `workspace_switch_modifier`+Shift 일 때 잡히므로(상호 배타) 맨 키 오검출이 없다. 대상이 잡히면 **next/prev 키를 먼저**(커스텀 슬롯 키가 next/prev 키와 겹칠 때 next/prev 우선), 그 다음 슬롯 배열을 `position` 검색한다. 매칭 결과:

- 탭: next/prev → `next_tab_in_pane`/`prev_tab_in_pane`, 슬롯 index → `goto_tab_in_pane(index)`.
- 워크스페이스: next/prev → `next_workspace_in_active_category`/`prev_workspace_in_active_category`, 슬롯 local → 카테고리 토글 on 이면 `switch_workspace_in_active_category(local)`, off 면 `switch_workspace(local)`.
- 카테고리: `category_switch_slot_keys` 슬롯 section → `switch_to_category(section)`(folders 토글 off 면 no-op·비소비). 자동 확장 + last-active 착지는 `switch_to_category` 가 처리한다. → [워크스페이스 카테고리](../workspace-category/index.md).

어느 것도 매칭 안 되면 조용히 `false` 를 돌려 다른 단축키 매칭으로 넘긴다. 이 메서드들은 focused pane 의 active_tab · active_workspace(= **사용자 포커스 상태**)를 바꾸므로 **사용자 키 입력 경로에서만** 호출되며 release IPC/CLI 로 노출되지 않는다(원칙 1/3). Command Palette·더블탭 파리티는 범위 밖.

#### switch-number 오버레이 — 표시 = 동작

modifier 홀드 중 탭바(`tab_bar.rs`)·사이드바(`sidebar/view.rs`)에 뜨는 숫자 키캡은 고정 상수가 아니라 **설정된 슬롯 키**를 그린다(`switch_overlay::tab_digit(kb, index)`/`workspace_digit(kb, local_idx)`). 슬롯을 `"q"` 로 바꾸면 키캡도 `Q` 로 뜬다(눌러서 가는 곳 = 표시). 워크스페이스 사이드바는 카테고리 토글 on 시 **active 카테고리 내 로컬 인덱스**로 키캡을 매기고 **비활성 카테고리 행에는 키캡을 표시하지 않는다** — 슬롯 단축키가 active 카테고리 로컬 순서로 전환하기 때문(전역 인덱스로 표시하던 과거 불일치를 제거). 오버레이 modifier 상태는 egui raw_input(실제 사용자 키)만 반영하므로 IPC/에이전트로는 강제 표시할 수 없다.

### 편집 — 녹화 + 충돌

Settings Keybindings 탭에서 키 조합을 직접 **녹화**해 할당한다. 충돌(같은 조합이 다른 액션에 이미) 시 확인 팝업으로 수락/거부. 편집은 draft 에 쌓이고 Save 시 커밋(`crud.rs`). quick-switch 슬롯의 bare-key 녹화·충돌 흐름은 위 [quick-switch 섹션 UI](#quick-switch-섹션-ui-tabworkspace-서브탭) 참조.

### 프리셋

키바인딩 **프리셋**(기본 세트 전환)을 제공한다(`keybindings/presets.rs`). 레이아웃 프리셋(`tasty-presets` crate, Workspace/Tab/Pane)과는 별개 — 이름만 비슷한 다른 시스템이다.

### 설정 탭 구성 (서브탭·항목 순서)

Settings 의 Keybindings 탭은 액션을 서브탭으로 묶고, 그 순서는 **유비쿼터스 언어 계층**을 따른다. 서브탭 enum: `KeybindingsSubTab`(`src/view/settings/ui/keybindings_tab.rs`).

```
General → Workspace → Pane → Tab → Surface → Clipboard → Zoom → Image → Preset → Plugins
          \________ 계층 순서 ________/
```

- **General / Clipboard / Zoom / Image**: 계층에 속하지 않는 전역·기능별 단축키.
- **Workspace → Pane → Tab → Surface**: [구조 계층](../../concepts/hierarchy.md) 순서.
- **Preset / Plugins**: 프리셋 적용 · 플러그인 기여 단축키(항상 끝).

각 서브탭 *내부* 항목 순서: **① 생성/분할 → ② 탐색(next/prev/focus) → ③ 수정(rename/convert) → ④ 닫기 → ⑤ 수식키(modifier, separator 로 구분)**.

**어느 서브탭에 두는가** — 그 동작의 *대상 엔티티* 이름을 가진 서브탭에 둔다. `new_tab`→Tab, `split_pane_*`→Pane, `close_surface`→Surface. 수식키도 대상 엔티티 서브탭(`tab_switch_modifier`→Tab, `workspace_switch_modifier`→Workspace). cascade 인 `close_active` 는 가장 먼저 닫히는 대상이 탭이라 Tab. `open_markdown` 은 새 탭으로 열려 Tab.

> explorer / html 이 plugin 으로 분리되며 `open_explorer`·`convert_to_explorer` 호스트 키바인딩은 사라졌다(plugin 이 자기 command 로 기여). 현재 Surface 의 convert 계열은 `convert_surface`·`convert_to_markdown` 만 호스트에 남는다.

## 인터페이스

- **사용자**: Settings → Keybindings 탭에서 녹화/편집. (단축키는 사용자 행동이라 release IPC/CLI 로 *발동* 하지 않는다 — 키 주입은 debug 전용, [debug-ipc](../../dev-guide/debug-ipc.md).)

## 비-목표

- 각 액션이 *무엇을 하는가* — 그 도메인 동작은 해당 기능 문서. 여기선 *키 ↔ 액션 매핑* 만.
- 위치 기반 modifier 매핑 규칙 — [design/policies/key-mapping](../../design/policies/key-mapping.md).

## 관련

- [design/policies/key-mapping](../../design/policies/key-mapping.md) — modifier 매핑·OS 메뉴 key equivalent 정책
- [settings](../settings/index.md) — 편집 표면
