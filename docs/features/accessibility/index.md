# 접근성 (Accessibility)

- **Status**: Implemented (Phase 1 — 수동 토글)
- **주체**: 로컬 사용자
- **ADR**: 없음
- **코드**: `AccessibilitySettings` · `ModifierHintSettings`(`tasty-settings`), 토스트 알파 분기(`src/adapters/ui/toast.rs`)
- **화면**: [설정 창](../settings/screens/settings.md) Accessibility 탭

## 목적

설정 → Accessibility 탭에서 직접 켜는 옵션. **Phase 1 은 수동 토글만** — OS 자동 감지(Windows ANIMATIONS / macOS NSWorkspace), AccessKit 통합, 색맹 팔레트, 스크린 리더 라벨은 Phase 2 이후.

## 내부 동작

### Reduced motion

`accessibility.reduced_motion: bool`(기본 false). 활성 시 토스트 페이드인/아웃이 0ms — lifetime 동안 100%, 만료 즉시 0%. 터미널 콘텐츠는 영향 없음([theme](../../design/systems/theme.md) "터미널 콘텐츠 애니메이션 0ms" 원칙상 이미 모션 없음).

### Modifier key hints

`modifier_hint.enabled: bool`(기본 **true**). 접근성 탭의 두 번째 토글로 노출된다. Modifier 키를 홀드하면 사용 가능한 단축키 안내 오버레이가 뜨는 기능의 표시 on/off 스위치. 오버레이 위치·크기는 `modifier_hint.pos` / `modifier_hint.size`(`Option<(LogicalPx, LogicalPx)>`, 기본 `None`)에 영속되며, 사용자가 이동/리사이즈하면 갱신된다. 윈도우 축소로 화면 밖이 되어도 저장값은 불변이고 클램프는 렌더 단계 책임이다.

지오메트리는 접근성 의미가 아닌 오버레이 UI 상태이므로 `AccessibilitySettings` 가 아닌 별도 루트 섹션 `ModifierHintSettings` 에 둔다. **현재 이 계층은 설정·영속 슬롯만** — 오버레이 렌더 자체는 별도 기능이다.

## 인터페이스

- **사용자**: Settings Accessibility 탭 토글. i18n 키 `settings.accessibility.*`.

## 관련

- [settings](../settings/index.md) · [design/systems/toast](../../design/systems/toast.md) · [design/systems/theme](../../design/systems/theme.md)
