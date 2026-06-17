# 접근성 (Accessibility)

- **Status**: Implemented (Phase 1 — 수동 토글)
- **주체**: 로컬 사용자
- **ADR**: 없음
- **코드**: `AccessibilitySettings`(`tasty-settings`), 토스트 알파 분기(`src/adapters/ui/toast.rs`)
- **화면**: [설정 창](../settings/screens/settings.md) Accessibility 탭

## 목적

설정 → Accessibility 탭에서 직접 켜는 옵션. **Phase 1 은 수동 토글만** — OS 자동 감지(Windows ANIMATIONS / macOS NSWorkspace), AccessKit 통합, 색맹 팔레트, 스크린 리더 라벨은 Phase 2 이후.

## 내부 동작

### Reduced motion

`accessibility.reduced_motion: bool`(기본 false). 활성 시 토스트 페이드인/아웃이 0ms — lifetime 동안 100%, 만료 즉시 0%. 터미널 콘텐츠는 영향 없음([theme](../../design/systems/theme.md) "터미널 콘텐츠 애니메이션 0ms" 원칙상 이미 모션 없음).

### High contrast (placeholder)

`accessibility.high_contrast: bool` — UI 체크박스는 **비활성(disabled)**. Phase 2 에서 Theme 분기 추가 예정.

## 인터페이스

- **사용자**: Settings Accessibility 탭 토글. i18n 키 `settings.accessibility.*`.

## 관련

- [settings](../settings/index.md) · [design/systems/toast](../../design/systems/toast.md) · [design/systems/theme](../../design/systems/theme.md)
