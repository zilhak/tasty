# 접근성 (Accessibility)

- **Status**: Implemented

### 개요
**Phase 1 — 수동 토글만**: 설정 → Accessibility 탭에서 직접 켜는 두 가지 옵션. OS 자동 감지(Windows `ANIMATIONS`, macOS `NSWorkspace`), AccessKit 통합, 색맹 팔레트, 스크린 리더 라벨은 Phase 2 이후.

### Reduced motion
- 설정: `accessibility.reduced_motion: bool` (기본 false)
- 동작: 활성 시 토스트 페이드인/페이드아웃이 0ms로 처리됨. lifetime 동안 100%, 만료 즉시 0%로 전환.
- 터미널 출력의 깜빡임/스크롤 등 콘텐츠 애니메이션은 영향 없음 (CLAUDE.md "터미널 콘텐츠 애니메이션: 절대 0ms" 원칙에 따라 이미 모션 없음).

### High contrast (placeholder)
- 설정: `accessibility.high_contrast: bool` — UI 체크박스는 비활성(disabled). Phase 2에서 Theme 분기 추가 예정.

### 구현
- crate: `tasty-settings` — `AccessibilitySettings { reduced_motion, high_contrast }`
- 토스트: `src/ui/toast.rs::ToastManager::draw(ctx, layout, reduced_motion: bool)` — 알파 계산 분기
- 설정 UI: `src/settings_ui/tabs.rs::draw_accessibility_tab` (탭 = `SettingsTab::Accessibility`)
- i18n: `settings.accessibility.*` 키 (en/ko/ja)
