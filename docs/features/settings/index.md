# 설정 (Settings)

- **Status**: Implemented
- **주체**: 로컬 사용자 (원격 접속 사용자는 mirror 로 봄)
- **ADR**: 없음
- **코드**: `src/view/settings.rs`, `src/view/settings/ui/`
- **화면**: [screens/settings.md](screens/settings.md)

## 목적

[사이드바](../sidebar/index.md) 설정 버튼이 여는 **설정 창**. tasty 의 환경설정을 2-level IA(상단 L1 탭 + 좌측 L2 섹션)로 편집한다. `SettingsView`(모달 계열 View, [구조 계층](../../concepts/hierarchy.md))이다.

## 내부 동작

### 2-level IA — L1 4탭 × L2 섹션

- **General** — L2: General / Terminal / Clipboard / Notifications / Accessibility / Updates (+ 현재 Performance / FileHandler / Windows 전용 Tastyrc 가 잠정 귀속).
- **Appearance** — L2: Theme / General / Tasty / Terminal / HtmlViewer (+ 플러그인 기여 페이지).
- **Keybindings** — 단축키 편집 (아래).
- **Plugins** — 플러그인 기여 설정 페이지.

L2 섹션은 좌측에 목록으로 뜨고 **필터 텍스트로 검색** 가능 (L1 전환 시 클리어).

### draft / save 모델

편집은 **작업 사본(`draft`)** 에 쌓이고, Save 시 영속 `Settings` 로 커밋, Cancel 시 폐기. 일부 항목(FileHandler 의 Extension Mapping 등)은 Save 시 user TOML 에 직접 atomic write.

### 단축키 탭 (Keybindings)

키 조합을 직접 녹화해 바인딩 할당. 충돌 시 확인 팝업으로 수락/거부. (모든 단축키는 `KeybindingSettings` 경유 — 코드 하드코딩 금지.)

### 플러그인 기여 페이지

플러그인이 설정 페이지를 contribute 하면 Appearance 의 sub-tab + Plugins 탭에 `(plugin_id, page_id)` 복합키로 나타난다. 등록된 plugin page 가 없으면 Plugins 탭/해당 sub-tab 이 비거나 사라진다.

## 인터페이스

- **사용자**: 사이드바 설정 버튼 → 모달, L1/L2 탐색, 편집 → Save/Cancel.
- **각 설정 도메인은 해당 기능으로 연결** (연결 개념 — 설정 창은 편집 UI, 도메인 규칙은 각 문서):
  - Keybindings → [`features/keybindings/`](../keybindings/index.md) / 키 매핑 정책 [`design/policies/key-mapping`](../../design/policies/key-mapping.md)
  - Appearance/Theme → [`design/systems/theme`](../../design/systems/theme.md)
  - Clipboard → [`features/clipboard/`](../clipboard/index.md) · Notifications → [`features/notifications/`](../notifications/index.md) · Updates → [`features/auto-update/`](../auto-update/index.md) · FileHandler → [`features/file-handler/`](../file-handler/index.md)
  - Plugins → [`features/plugin-system/`](../plugin-system/index.md)

## 비-목표

- 각 설정 항목의 *도메인 동작* (테마가 무엇을 바꾸나, 단축키가 무엇을 하나 등) — 설정 창은 *편집 표면* 일 뿐. 도메인은 각 기능/시스템 문서.

## Acceptance Criteria

- [ ] 사이드바 설정 버튼 클릭 시 설정 모달이 열린다 (L1 4탭).
- [ ] L1 탭 전환 시 좌측 L2 섹션 목록이 그 탭의 것으로 바뀌고 필터가 클리어된다.
- [ ] 편집 후 Save 시 영속 Settings 에 반영되고, Cancel 시 폐기된다.
- [ ] Keybindings 에서 키 조합 녹화 시 충돌이 있으면 확인 팝업이 뜬다.
- [ ] 플러그인이 설정 페이지를 contribute 하면 Plugins 탭/Appearance sub-tab 에 나타난다.

> 모달 창이라 시각 검증은 스크린샷, draft/save·plugin page 등록은 시나리오로 검증.

## 구현

- `src/view/settings.rs` — `SettingsView`, `SettingsUiState`(draft/active_tab/sub-tab 상태).
- `src/view/settings/ui.rs` — `SettingsTab`(L1 4탭), `GeneralSubTab`/`AppearanceSubTab`/`PluginSubTab`(L2), L2 필터.
- 탭별: `src/view/settings/ui/tabs/*` + `keybindings_tab.rs` + `file_handler_tab.rs`.

## 화면

- [screens/settings.md](screens/settings.md) — 설정 창 레이아웃(L1 탭바 / L2 섹션 / 콘텐츠 / Save·Cancel)과 섹션별 연결.
