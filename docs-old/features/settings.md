# 설정 시스템

- **Status**: Implemented

### TOML 기반 설정 파일
- 설정 파일 경로: `~/.tasty/config.toml` (전 플랫폼 통일)
- `directories` 크레이트로 플랫폼별 홈 디렉토리 추상화
- `toml` + `serde` 기반 직렬화/역직렬화
- 설정 파일이 없거나 파싱 실패 시 기본값으로 폴백

### 설정 카테고리
- **General**: 레이아웃 저장/복원 (기본 off): 체크 시 워크스페이스/페인/탭/서피스 구조를 `~/.tasty/layout.json`에 저장하고 다음 시작 시 복원. 마지막 윈도우 닫기 동작 (ask / minimize / quit).
- **Terminal**: 셸 경로 (OS별 자동 감지: COMSPEC/SHELL), 셸 모드 (default / tasty). **셸 모드는 Windows 전용 개념** — 모드는 "어떤 사용자 rc 를 source 하느냐" 만 결정하고, OSC 7/UTF-8/MSYS PATH 같은 tasty 빌트인은 **두 모드 모두에서 강제 주입**된다(Windows 는 cwd 상속이 OSC 7 에만 의존하기 때문). 구체적으로: `default` 모드는 `~/.tasty/bashrc.default` (`BUILTIN + source ~/.bashrc + BUILTIN PROMPT`) 를 `--rcfile` 로 띄워 사용자 시스템 `~/.bashrc` 를 source 하고, `tasty` 모드는 `~/.tasty/bashrc` (`BUILTIN + ~/.tasty/bashrc.user + BUILTIN PROMPT`) 를 띄워 tasty 가 관리하는 사용자 영역을 source 한다. 어느 쪽이든 BUILTIN 의 PROMPT_COMMAND 설정이 *맨 마지막* 에 와서 사용자 rc 가 PROMPT_COMMAND 를 덮어쓰더라도 `__tasty_osc7` 이 prepend 된다. 비-Windows 에서는 셸별로 `--rcfile` 등을 모르거나 무시하여 셸이 죽거나 의미가 없고 cwd 상속도 OS 조회로 이미 되므로, 셸 모드 UI 자체를 노출하지 않고 사용자 셸을 그대로 띄운다(빌트인 미적용). 그래서 빌트인 편집(Misc 탭)도 Windows 에서만 노출된다. 기존 설정 파일의 `"fast"`/`"custom"` 같은 unknown 값은 `default` 와 동일하게 처리된다. 그 외: 시작 명령, 스크롤백 줄 수 (기본 10,000), 실행 중 프로세스 닫기 확인, 작업 디렉토리 상속 (기본 on), 링크 클릭 수식키 (ctrl / alt / none). 데이터는 여전히 `settings.general.*`에 저장되며 UI 탭만 분리되어 있다.
- **Appearance**: 폰트 패밀리 (기본값: 시스템 모노스페이스), 폰트 크기, 테마 (dark/light), 배경 투명도, 사이드바 너비, focused surface 배경색, Font DPI 스케일링 모드 (auto: 모니터 DPI에 맞춰 동일 물리 크기 유지, 기본값 / fixed: 픽셀 고정), **host UI zoom** (`ui_scale` — `small` 0.85x / `medium` 1.0x / `large` 1.2x: 사이드바·popup·헤더·설정창 등 host UI 영역만 일관 스케일. **탭 바와 터미널 콘텐츠는 영향 받지 않음** — 탭바는 자체 zoom 미적용 토큰, 터미널 폰트는 별도 시스템. 변경은 `UiIntent::AppearanceChanged` broadcast 로 main + 모든 modal 윈도우에 즉시 반영되며 polling 아님). sub-tab 은 호스트 정적 항목 (Theme / Terminal / Explorer 등) + plugin contribute 동적 항목의 합성으로 구성된다 — 활성 plugin 의 `[[contributes.settings_pages]]` (category=`appearance`) 가 SettingsPageRegistry 를 통해 sub-tab 으로 합류하며, plugin 비활성 시 자동으로 사라진다 (dead-setting 비표시 정책). plugin 측 sub-tab 의 라벨·항목·storage_key 는 plugin 자체 manifest 가 결정한다. 외관 탭은 `appearance` 카테고리만 필터링하므로 `plugin` 카테고리 page 는 별도 플러그인 탭에 노출된다 (아래 Plugin 탭 항목 참조).
- **Plugin**: 전 OS 노출. 활성 plugin 의 `[[contributes.settings_pages]]` 중 `category = "plugin"` 인 page 들이 좌측 2depth sub-tab 으로 합성된다 (외관 탭과 동일한 `two_depth_layout`). 등록된 page 가 0 개면 좌측 메뉴는 비고 우측에 "항목이 없습니다." 안내만 표시. 권한 게이트는 외관 탭과 동일하게 `ui.settings_page` 1 개만. plugin disable 시 sub-tab 이 사라지고 활성 sub-tab 은 None 으로 리셋되어 안내 메시지로 fallback 한다.
- **Clipboard**: OS별 기본 활성화 (macOS: Alt+C/V, Linux: Ctrl+Shift+C/V, Windows: Ctrl+C/V)
- **Notifications**: 알림 활성화, 시스템 알림, 사운드, 병합 간격(ms)
- **Keybindings**: 서브탭으로 분류된 단축키 설정 (General / Workspace / Pane / Tab / Surface / Clipboard / Zoom / Preset). 유비쿼터스 언어 계층 구조(Workspace → Pane → Tab → Surface) 순서. 각 서브탭 내부 항목은 생성/분할 → 탐색 → 수정 → 닫기 순서로 정렬
  - 중복 바인딩 방지: 녹화한 조합이 다른 액션에 이미 할당되어 있으면 확인 팝업 표시. Enter/Y/Overwrite 수락 시 기존 바인딩을 비우고 새 필드에 적용, Esc/N/Cancel 취소 시 값 변경 없음. 팝업이 열린 동안 녹화 버튼은 비활성화됨.
  - **Preset 서브탭**: 좌측에 프리셋 목록, 우측에 미리보기 패널 (3열 테이블 — 기능 / 이전 / 이후). 변경되는 행은 bold 강조. 하단 "적용" 버튼으로 Draft에 반영 (실제 저장은 하단 Save 버튼). Draft가 이미 프리셋과 동일하면 적용 버튼 비활성화.
- **General 그룹 잠정 흡수 섹션**: 구 "Misc" 탭은 2-level IA 개편으로 해체되어 아래 섹션들이 General L1 의 L2 사이드바로 이관됨 (Performance / Tastyrc 는 디자인 IA 에 정의 없음 — 기능 회귀 방지용 잠정 배치, 최종 귀속은 제품 결정 대기).
  - **tastyrc 섹션** (Windows 한정 노출): Tasty 모드 bashrc 편집기. 사용자 편집분은 `~/.tasty/bashrc.user`에 저장되고, 빌트인 블록(OSC 7 emission / UTF-8 / PATH 등 PRE 부분과 PROMPT_COMMAND 설정의 POST 부분)은 코드 상수로 유지되어 Save 시마다 `~/.tasty/bashrc`가 `BUILTIN_PRE + user + BUILTIN_PROMPT` 형태로 자동 재생성된다. PROMPT_COMMAND 설정이 사용자 본문 *뒤* 에 와서 사용자가 PROMPT_COMMAND 를 덮어쓰더라도 `__tasty_osc7` 이 prepend 되도록 보장한다. 빌트인 템플릿이 업데이트되면 기존 사용자에게도 즉시 반영된다. Reset 버튼으로 user 파트를 초기 기본값으로 되돌릴 수 있다.
  - **접근성 섹션**: reduced motion (UI fade/slide 애니메이션 스킵, 토스트 fade-in/out 비활성화 — 터미널 콘텐츠 애니메이션은 영향 없음), high contrast (placeholder).
  - **성능 섹션**: targeted PTY polling, scrollback disk swap, lazy PTY init (background 탭 생성 시 PTY를 즉시 spawn하지 않고 최초 접근 시점에 spawn — 레이아웃 복원으로 만들어진 비활성 워크스페이스의 deferred 터미널은 사용자가 워크스페이스를 전환하거나, 에이전트가 send/`surface.wake` IPC로 접근하는 시점에 PTY가 자동 생성된다. `surface.list`/`tree` 결과의 `pty_ready` 필드로 현재 상태를 확인할 수 있다).

### GUI 설정 윈도우
- Ctrl+, 단축키로 설정 윈도우 토글
- 2-level IA: 상단 L1 탭 4개 (General / Appearance / Keybindings / Plugins) + 각 L1 의 좌측 L2 사이드바 (상단 섹션 필터 입력 포함, `tasty_ui_widgets::two_depth_layout_filtered` 공용 헬퍼). L1 전환 시 L2 필터 초기화. General L1 의 L2 = General / Terminal / Clipboard / Notifications / Accessibility / Updates / Performance / File Handler / (Tastyrc, Windows). 디자인 정의(`ui_kits/.../settings_window.jsx`)에 없는 Performance / File Handler / Tastyrc / Appearance>HTML / Keybindings>Plugins 는 기능 회귀 방지용 잠정 배치 (최종 귀속 제품 결정 대기).
- egui에 시스템 CJK 폰트 로드: Windows(맑은 고딕), macOS(AppleSDGothicNeo), Linux(Noto Sans CJK)
- 편집 중 원본 설정을 보존하는 드래프트 패턴
- Save 버튼: 디스크에 저장 후 즉시 적용
- Cancel 버튼: 변경 사항 폐기

### 설정 로드/저장
- `Settings::load()`: 설정 파일 로드, 없으면 기본값 반환
- `Settings::save()`: 설정 디렉토리 자동 생성 후 TOML 형식으로 저장
- `Settings::config_path()`: 플랫폼 독립적 설정 파일 경로 반환
- `Settings::normalize()`: enum-like 필드(`appearance.ui_scale`, `appearance.*_font.font_scale_mode`, `general.shell_mode`, `general.close_behavior`, `general.link_click_modifier`)에 알려진 값 외의 문자열이 있으면 안전한 기본값으로 치환하고 `NormalizeReport`를 반환. `appearance.theme` 은 legacy id 매핑(`catppuccin-mocha` → `mocha`, `catppuccin-latte` → `latte`)만 수행 — 실제 valid 검증/fallback 은 부팅 흐름의 `tasty_themes::apply_theme()` 가 담당한다. `general.language`는 사용자가 `~/.tasty/lang/{code}.toml` 로 임의 코드를 추가할 수 있으므로 정규화 대상에서 제외
- 부팅 경로(첫 윈도우 `init_app_state`, 새 윈도우 `create_new_window`, shell-setup 종료)에서는 `Settings::load()` 직후 `normalize()`를 호출하고 `report.changed`면 즉시 `save()`. 결과로 디스크의 invalid 값이 한 번에 정리되어, 다음 부팅·다음 윈도우에서 같은 popup·warning이 반복되지 않음
- 앱 시작 시 자동 로드, AppState에 통합

### 설정 연동
- `settings.general.shell`: Terminal 생성 시 커스텀 셸 경로 사용 (비어있으면 OS 기본 셸)
- `settings.general.startup_command`: 새로 생성되는 모든 터미널에 prompt 직후 1회 자동 실행할 명령 (`send_fast_init`에서 전송). split/새 탭/새 워크스페이스/새 윈도우/레이아웃 복구 모두 적용. 공백·빈 문자열이면 전송 안 함. `surface.respawn_terminal` IPC는 `send_fast_init` 미호출 경로라 적용되지 않음 (플러그인 PTY 갈아끼우기 용도이므로 의도된 제외). (tasty 모드의 bashrc는 PTY 입력이 아니라 셸 `--rcfile` 인자로 source된다 — Windows 전용. `effective_shell_args` 참조.)
- `settings.appearance.default_font`: 기본 폰트 5종 묶음 (`font_family`, `font_size`, `custom_font_path`, `line_height`, `font_scale_mode`). Terminal·Markdown·Explorer 모두에 일괄 적용되며, 각 surface는 아래 override 그룹으로 항목별 재정의 가능. 설정 UI에서는 Theme 서브탭 하단의 "기본 폰트 설정" 섹션에서 편집
- `settings.appearance.terminal_font`: 호스트 자체 surface (terminal) 의 per-field override. 5개 필드 모두 `Option<T>`이며 `None`이면 `default_font`를 사용
- `settings.appearance.plugin_font_overrides`: plugin contribute 된 surface 의 폰트 override 를 담는 `HashMap<String, FontOverride>` (key = `[[contributes.settings_pages.items]] kind = "font_override"` 의 `storage_key`). 예: markdown plugin 은 `plugin_font_overrides.markdown` 슬롯을 사용. plugin 비활성 시에도 값은 보존되어 재활성 시 복원된다 — host 는 `effective_font_for_kind(kind)` 로 surface 별 effective font 를 조회한다. 마이그레이션: 기존 `[markdown_font]` / `[explorer_font]` TOML 섹션은 로드 시 자동으로 `plugin_font_overrides.<kind>` 로 이전된다 (값 손실 없음)
- `font_family`: cosmic-text(터미널) 또는 egui FontDefinitions(Markdown/Explorer)에 전달. 빈 문자열이나 "monospace"이면 번들 D2Coding ligature를 사용. 다른 폰트를 지정해도 D2Coding은 폰트 DB에 남아 fallback face로 동작. 설정 UI에서 시스템 폰트 목록(번들 `D2Coding ligature` 포함)을 검색 가능한 드롭다운으로 선택
- `font_size`: 픽셀 단위. 기본값 14.0. 단축키 `Ctrl+/-/0`은 포커스된 surface(Terminal/Markdown/Explorer)의 `font_size` override만 변경하며, `Ctrl+0`은 override를 제거해 기본값으로 회귀
- `custom_font_path`: 커스텀 폰트 파일(.ttf/.otf) 경로. 지정 시 FontSystem 또는 egui FontDefinitions에 해당 파일을 추가 로드한 후 `font_family`로 참조 가능
- `line_height`: 행간 배수. 1.0(기본, 틈 없음 - ASCII 아트에 최적) ~ 2.0. 값이 클수록 행 간격이 넓어짐
- `font_scale_mode`: "auto"는 `font_size * scale_factor`(고DPI에서 동일 물리 크기 유지), "fixed"는 픽셀 크기 고정
- `settings.appearance.theme`: 현재 선택된 테마 id (= `~/.tasty/themes/<id>.toml` 의 파일명 stem). 빌트인은 `mocha`(기본 다크), `latte`(라이트). 사용자는 themes 폴더에 자유롭게 `*.toml` 추가 가능. 알려지지 않은 id 는 부팅 시 `tasty_themes::apply_theme()` 가 mocha 로 fallback 하고 InfoModal 로 사용자에게 알린다. 상세는 [docs/design/systems/theme.md](design/systems/theme.md), 사용자 가이드는 [docs/agent-guide/themes.md](agent-guide/themes.md)
- `settings.appearance.theme_base`: 누적된 테마 색상 풀 세트 (`ThemeColors`). 테마 변경 시 새 테마의 partial 이 이 위에 덮어쓰여진다 — 누락 필드는 보존되므로 partial 테마도 자연스럽게 적용
- `settings.appearance.theme_overrides`: 사용자가 픽커로 직접 손댄 색상 흔적 (`PartialColors`, 모든 필드 `Option`). 테마 변경 시 클리어
- `settings.appearance.theme_is_light`: 라이트/다크 플래그. `hover_overlay` / `active_overlay` / `separator` 같은 반투명 의미 색이 이 값에서 자동 도출됨
- `settings.appearance.background_opacity`: wgpu clear color의 알파 값으로 적용. 0.0(투명)~1.0(불투명)
- surface 종류별(focused/unfocused × bg/fg) 색은 `theme.surface_themes` map 에 들어있다. 빌트인 mocha 가 `"terminal"`, `"markdown"` entry 를 채우고, theme TOML 의 `[surfaces.<id>]` sub-table 로 사용자/plugin 이 추가 가능. 렌더러는 `theme().surface(id)` 로 접근하며 미정의 id 는 `FALLBACK_SURFACE` 로 안전하게 동작
- `settings.appearance.sidebar_width`: 사이드바 너비가 UI, GPU 렌더러, 터미널 rect 계산에 반영. 렌더 루프에서 설정값과 자동 동기화
- `settings.clipboard.history_enabled`: 클립보드 히스토리 기록 여부
- `settings.clipboard.history_max`: 히스토리 최대 항목 수 (기본 100)
- `settings.clipboard.poll_interval_ms`: 시스템 클립보드 폴링 주기(ms, 재시작 필요)
- `settings.keybindings.copy` / `settings.keybindings.paste`: 복사·붙여넣기 단축키 (다중 바인딩). 플랫폼별 기본값 — Windows: `ctrl+c` / `ctrl+v`, Linux: `ctrl+shift+c` / `ctrl+shift+v`, macOS: `alt+c` / `alt+v`
- `settings.keybindings.zoom_in` / `zoom_out` / `zoom_reset`: 줌 단축키 (다중 바인딩). 플랫폼별 기본값 — Windows/Linux: `ctrl+=` / `ctrl+-` / `ctrl+0`, macOS: `alt+=` / `alt+-` / `alt+0`
- `settings.notification.enabled`: 알림 활성화/비활성화. 비활성 시 알림 수집 및 시스템 알림 모두 차단
- `settings.notification.system_notification`: OS 네이티브 알림 개별 제어
- `settings.notification.coalesce_ms`: NotificationStore 생성 시 병합 간격 전달
- `settings.notification.sound`: true 일 때 신규 알림 발화 시 OS 기본 beep 1 회 재생 (Phase F.E — macOS `NSBeep` / Windows `MessageBeep(MB_OK)` / Linux `paplay → aplay → stderr \a` 3 단 폴백). headless 빌드는 `NoopPlayer` 로 대체. 상세는 "알림 사운드" 절 참조
- `settings.keybindings.*`: UI에 미노출. 현재 main.rs에서 하드코딩된 단축키 사용 (TODO: 파싱 및 적용)
