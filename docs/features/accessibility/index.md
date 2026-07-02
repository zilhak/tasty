# 접근성 (Accessibility)

- **Status**: Implemented (Phase 1 — 수동 토글)
- **주체**: 로컬 사용자
- **ADR**: 없음
- **코드**: `AccessibilitySettings` · `ModifierHintSettings`(`tasty-settings`), 토스트 알파 분기(`src/adapters/ui/toast.rs`), modifier-hint 콘텐츠 모델(`src/adapters/ui/input/shortcuts/modifier_hint.rs`) · 오버레이 본체(`src/adapters/ui/modifier_hint_overlay.rs`)
- **화면**: [설정 창](../settings/screens/settings.md) Accessibility 탭

## 목적

설정 → Accessibility 탭에서 직접 켜는 옵션. **Phase 1 은 수동 토글만** — OS 자동 감지(Windows ANIMATIONS / macOS NSWorkspace), AccessKit 통합, 색맹 팔레트, 스크린 리더 라벨은 Phase 2 이후.

## 내부 동작

### Reduced motion

`accessibility.reduced_motion: bool`(기본 false). 활성 시 토스트 페이드인/아웃이 0ms — lifetime 동안 100%, 만료 즉시 0%. 터미널 콘텐츠는 영향 없음([theme](../../design/systems/theme.md) "터미널 콘텐츠 애니메이션 0ms" 원칙상 이미 모션 없음).

### Modifier key hints

`modifier_hint.enabled: bool`(기본 **true**). 접근성 탭의 두 번째 토글. **modifier 키를 500ms 홀드하면** 그 키를 포함하는 조합의 단축키 목록 오버레이가 200ms 페이드(opacity 0.2→1.0)로 사이드바 하단에 뜨고, **키를 떼면 즉시 사라진다**. `reduced_motion` 이면 페이드는 생략되지만 **500ms 지연은 유지**된다(지연은 모션이 아니라 실수 스침 억제 게이트).

Modal/Popup/Toast/Banner 어디에도 안 맞는 **5번째 오버레이 요소** — 키보드 포커스를 절대 안 받고(입력은 그대로 터미널로), **마우스만 소비**한다(드래그 스트립으로 이동 · 테두리/코너 그립으로 리사이즈 · X 로 이번 홀드 세션 dismiss). `modifier_hint_hovered` 플래그가 `mouse.rs` 4지점에서 하위 surface 로의 전파(click-to-activate/휠/드래그)를 막는다([input-layer](../../architecture/input-layer.md)).

- **콘텐츠**: `build_hint_sections`(modifier-hint 콘텐츠 모델) — 고정 호스트 액션 + 사용자 스크립트 + 특수 역할(탭/워크스페이스 전환·마우스 캡처 우회·링크 열기)을 조합 크기·우선순위로 정렬. 빈 조합(바인딩·역할 모두 없음)은 섹션 자체 생략. plugin 단축키 노출은 후속 배선(PluginManager 가 App 소유라 draw 경로 미도달).
- **홀드 판정**: winit `ModifiersChanged`(실사용자 입력)만 반영 — IPC/CLI 로 강제 표시 불가(원칙1). anchor 는 최초 눌린 축, 조합이 바뀌어도(예: Ctrl→Ctrl+Shift) 타이머 리셋 없이 콘텐츠만 갱신. 창 포커스 상실 시 clear.
- **지오메트리**: `modifier_hint.pos` / `modifier_hint.size`(`Option<(LogicalPx, LogicalPx)>`, 기본 `None`)에 영속. 기본 220×400, 최소 200×240. 사용자가 이동/리사이즈해 놓는 시점에 `UpdateSettings` 로 저장(사이드바 폭과 동일 성질, 전역 공유 + last-write-wins). 윈도우 축소로 화면 밖이 되어도 **저장값은 불변**이고 클램프는 렌더 단계 책임이다. 지오메트리는 접근성 의미가 아닌 오버레이 UI 상태라 `ModifierHintSettings`(별도 루트 섹션)에 둔다.

## 인터페이스

- **사용자**: Settings Accessibility 탭 토글(`settings.accessibility.modifier_hint*`). 오버레이 표시는 실 modifier 홀드 · 드래그/리사이즈/X 는 사용자 마우스. i18n 키 `modifier_hint.*`(held / hide_tooltip / role.*).
- **에이전트**: 없음 — IPC/CLI 로 오버레이를 띄우거나 조작할 수 없다(원칙1, focus 독립성).

## 관련

- [settings](../settings/index.md) · [design/systems/toast](../../design/systems/toast.md) · [design/systems/theme](../../design/systems/theme.md) · [architecture/input-layer](../../architecture/input-layer.md) · [ubiquitous-language](../../concepts/ubiquitous-language.md)
