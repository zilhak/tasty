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

### 편집 — 녹화 + 충돌

Settings Keybindings 탭에서 키 조합을 직접 **녹화**해 할당한다. 충돌(같은 조합이 다른 액션에 이미) 시 확인 팝업으로 수락/거부. 편집은 draft 에 쌓이고 Save 시 커밋(`crud.rs`).

### 프리셋

키바인딩 **프리셋**(기본 세트 전환)을 제공한다(`keybindings/presets.rs`). 레이아웃 프리셋(`tasty-presets` crate, Workspace/Tab/Pane)과는 별개 — 이름만 비슷한 다른 시스템이다.

## 인터페이스

- **사용자**: Settings → Keybindings 탭에서 녹화/편집. (단축키는 사용자 행동이라 release IPC/CLI 로 *발동* 하지 않는다 — 키 주입은 debug 전용, [debug-ipc](../../dev-guide/debug-ipc.md).)

## 비-목표

- 각 액션이 *무엇을 하는가* — 그 도메인 동작은 해당 기능 문서. 여기선 *키 ↔ 액션 매핑* 만.
- 위치 기반 modifier 매핑 규칙 — [design/policies/key-mapping](../../design/policies/key-mapping.md).

## 관련

- [design/policies/key-mapping](../../design/policies/key-mapping.md) — modifier 매핑·OS 메뉴 key equivalent 정책
- [settings](../settings/index.md) — 편집 표면
