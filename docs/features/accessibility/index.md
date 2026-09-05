# 접근성 (Accessibility)

- **Status**: Implemented (Phase 1 — 수동 토글)
- **주체**: 로컬 사용자
- **ADR**: [ADR-0174](../../adr/0174-theme-carries-reduced-motion.md)(모션 감소를 `Theme` 이 실어 나른다)
- **코드**: `AccessibilitySettings` · `ModifierHintSettings` · `Settings::theme_runtime()`(`tasty-settings`), `ThemeRuntime`(`tasty-themes`), `Theme.reduced_motion`(`tasty-type-appearance`), 토스트 알파 분기(`src/adapters/ui/toast.rs`), 스피너(`crates/tasty-ui-widgets/src/spinner.rs`), switch 오버레이 페이드(`src/adapters/ui/switch_overlay.rs`), 모달 흔들기(`src/app/modal/shake.rs`), modifier-hint 콘텐츠 모델(`src/adapters/ui/input/shortcuts/modifier_hint.rs`) · 오버레이 본체(`src/adapters/ui/modifier_hint_overlay.rs`)
- **화면**: [설정 창](../settings/screens/settings.md) Accessibility 탭

## 목적

설정 → Accessibility 탭에서 직접 켜는 옵션. **Phase 1 은 수동 토글만** — OS 자동 감지(Windows ANIMATIONS / macOS NSWorkspace), AccessKit 통합, 색맹 팔레트, 스크린 리더 라벨은 Phase 2 이후.

## 내부 동작

### Reduced motion

`accessibility.reduced_motion: bool`(기본 false). 활성 시:

- **토스트** 페이드인/아웃 0ms — lifetime 동안 100%, 만료 즉시 0%.
- **스피너**(`tasty-ui-widgets`) 회전 정지 → 3-dot 정적 표시.
- **switch-number 오버레이** 등장/퇴장 페이드 0ms(즉시 표시/소거).
- **모달 흔들기**(닫기 거부 피드백)는 아예 시작하지 않는다 — 창 자체를 움직이는 물리적 모션이라 모션 감소가 막으려는 것 그 자체다.
- **modifier 힌트** 페이드는 생략하되 표시 지연은 유지(위 Modifier key hints 항 참조).
- **터미널 콘텐츠**는 영향 없음([theme](../../design/systems/theme.md) "터미널 콘텐츠 애니메이션 0ms" 원칙상 이미 모션 없음).

값은 `Theme.reduced_motion` 이 실어 나르고 **위젯의 기본 동작이 그것을 읽는다** — 호출부가 넘기는 형태였을 때 실제로 넘기는 자리가 하나도 없어 설정이 무력했기 때문이다([ADR-0174](../../adr/0174-theme-carries-reduced-motion.md)). 채우는 자리는 `Settings::theme_runtime()` 하나이고, 전역 `Theme` 설치 경로가 그것을 그대로 나른다. 위젯 쪽 override(`Spinner::reduced_motion`)는 실행 중 설정과 무관하게 두 상태를 나란히 보여야 하는 갤러리 specimen 전용이다.

plugin 프로세스는 아직 이 값을 모른다(`ThemeWire` 에 필드 없음) — 현재 plugin 이 그리는 모션 위젯이 없어 잠재 구멍이다.

### Modifier key hints

`modifier_hint.enabled: bool`(기본 **true**). 접근성 탭의 두 번째 토글. **modifier 키를 홀드하면** 눌린 **조합을 포함하는(부분집합)** 조합의 단축키 목록 오버레이가 200ms 페이드(opacity 0.2→1.0)로 사이드바 하단에 뜨고, **키를 떼면 즉시 사라진다**. 조합을 좁혀 누르면(예: Ctrl→Ctrl+Shift) 목록도 **즉시 좁혀진다**. 표시 지연은 기본 **500ms**, 단 **Shift 단독** 홀드만 **1200ms**(Shift 는 대문자·기호 입력에 상시 눌려 스침이 잦으므로 타이핑 중 오버레이가 튀는 것을 억제 — [ADR-0035](../../adr/0035-modifier-hint-combo-narrowing-and-shift-delay.md)). 홀드를 유지한 채 **등록된 tasty 단축키를 실제로 실행**하면 그 시점부터 지연 타이머가 다시 시작된다(아직 표시되지 않았을 때만 — [ADR-0064](../../adr/0064-modifier-hint-reveal-timer-reset-on-shortcut.md)). `reduced_motion` 이면 페이드는 생략되지만 **지연은 유지**된다(지연은 모션이 아니라 실수 스침 억제 게이트).

Modal/Popup/Toast/Banner 어디에도 안 맞는 **5번째 오버레이 요소** — 키보드 포커스를 절대 안 받고(입력은 그대로 터미널로), **마우스만 소비**한다(드래그 스트립으로 이동 · 테두리/코너 그립으로 리사이즈 · X 로 이번 홀드 세션 dismiss · 리스트 세로 휠 스크롤). `modifier_hint_hovered` 플래그가 `mouse.rs` 4지점에서 하위 surface 로의 전파(click-to-activate/휠/드래그)를 막는다([input-layer](../../architecture/input-layer.md)).

- **휠 스크롤(modifier 무시)**: 이 오버레이는 **modifier 를 홀드한 채** 떠 있으므로 egui 의 기본 처리(`Ctrl+휠`=zoom, `Shift+휠`=가로 스크롤)로는 세로 `ScrollArea` 가 안 움직인다. `modifier_free_wheel_y()` 가 포인터가 패널 위일 때 raw `MouseWheel` 이벤트를 modifier 무관하게 다시 읽어(egui 와 동일 단위 스케일) 세로 성분만 `scroll_with_delta` 로 주입한다 → **어떤 modifier 를 눌러도 휠은 순수 세로 스크롤**. alt/option 단독은 egui 가 이미 세로로 처리하므로 이중 스크롤을 피해 Ctrl·Cmd·Shift 홀드 시에만 주입.

- **콘텐츠**: `build_hint_sections(held: Combo, …)`(modifier-hint 콘텐츠 모델) — 눌린 4축 조합 `held` 를 부분집합으로 포함하는 조합(`combos_containing_all`, `Combo::contains_all`)만 노출. 고정 호스트 액션 + 사용자 스크립트 + 특수 역할(탭/워크스페이스 전환·마우스 캡처 우회·링크 열기)을 조합 크기·우선순위로 정렬. 다축 홀드 시 첫 섹션이 홀드 조합 자신이라 헤더와 일치. 빈 조합(바인딩·역할 모두 없음)도 섹션을 **유지**하며, 오버레이가 ChordHead 아래에 muted "바인딩 없음" 플레이스홀더(`modifier_hint.empty`, 키캡·wash·글리프 없음, min-height 20px·내부 간격 3px)를 그린다 — 미할당 조합을 홀드해도 패널이 뜨고 부재를 명시([ADR-0038](../../adr/0038-modifier-hint-empty-combo-placeholder.md)). plugin 단축키 노출은 후속 배선(PluginManager 가 App 소유라 draw 경로 미도달).
- **ChordHead 키캡**: `combo_keycap_parts`(`modifier_hint_overlay.rs`)가 `GeneralSettings::{alt,option,shift}_display_style` 이 `"symbol"` 인 축을 텍스트("⌘"/"⌥"/"⇧") 대신 벡터 아이콘(`tasty_icons::{CMD_KEY,OPTION_KEY,SHIFT_KEY}`)으로 그린다 — egui 폰트 fallback 체인에 없는 glyph 라 텍스트로 두면 tofu box 로 깨지기 때문([key-mapping.md](../../design/policies/key-mapping.md) "symbol 표시" 참고). `kbd_parts`(`tasty-ui-widgets`)가 텍스트/아이콘 키캡을 같은 배경·테두리로 렌더링.
- **홀드 판정**: winit `ModifiersChanged`(실사용자 입력)만 반영 — IPC/CLI 로 강제 표시 불가(원칙1). `held: Option<Combo>` 가 현재 눌린 4축을 그대로 담고, 조합이 바뀌면 `update_hold` 가 **항상 dirty** 를 반환해 즉시 좁힘. 타이머(`hold_since`)는 최초 press 에만 시작하고 **조합이 바뀌는 것만으로는** 리셋하지 않는다(ADR-0035 A) — 단 등록된 단축키가 **키 입력 경로**에서 실제로 소비되면(Command Palette 등 공용 진입점 제외 — 원칙1) 아직 표시 전인 홀드에 한해 `reset_reveal_timer_if_not_shown` 이 타이머를 다시 시작한다(ADR-0064). 창 포커스 상실 시 clear.
- **표시 지연**: `reveal_delay_ms(held, theme)` 가 Shift 단독이면 `motion_hold_reveal_shift()`(1200ms), 그 외 `modhint_hold_delay()`(500ms, 생성 토큰). 매 프레임 재평가라 Shift 단독 대기 중 modifier 를 추가하면 지연이 500ms 로 떨어지고 경과가 넘었으면 즉시 표시. 두 값 모두 `Theme` 경유이며 위젯이 직접 리터럴을 쓰지 않는다 — 다만 **1200ms 쪽은 대응 디자인 토큰이 없어** 아직 `theme.rs` 에 손으로 남아 있다.
- **지오메트리**: `modifier_hint.pos` / `modifier_hint.size`(`Option<(LogicalPx, LogicalPx)>`, 기본 `None`)에 영속. 기본 180×400, 최소 180×240. 사용자가 이동/리사이즈해 놓는 시점에 `UpdateSettings` 로 저장(사이드바 폭과 동일 성질, 전역 공유 + last-write-wins). 윈도우 축소로 화면 밖이 되어도 **저장값은 불변**이고 클램프는 렌더 단계 책임이다. 지오메트리는 접근성 의미가 아닌 오버레이 UI 상태라 `ModifierHintSettings`(별도 루트 섹션)에 둔다.

## 인터페이스

- **사용자**: Settings Accessibility 탭 토글(`settings.accessibility.modifier_hint*`). 오버레이 표시는 실 modifier 홀드 · 드래그/리사이즈/X 는 사용자 마우스. i18n 키 `modifier_hint.*`(held / hide_tooltip / role.*).
- **에이전트**: release 없음 — IPC/CLI 로 오버레이를 띄우거나 조작할 수 없다(원칙1, focus 독립성). **debug 빌드 한정** 검증용으로 `debug.modifier_hint.hold`(홀드 조합 force-state + 타이머 백데이트) / `debug.modifier_hint.state`(렌더 상태 덤프)가 있다([debug-ipc](../../dev-guide/debug-ipc.md)). `#[cfg(all(debug_assertions, feature = "gui"))]` 격리라 release 미노출.

## 관련

- [settings](../settings/index.md) · [design/systems/toast](../../design/systems/toast.md) · [design/systems/theme](../../design/systems/theme.md) · [architecture/input-layer](../../architecture/input-layer.md) · [ubiquitous-language](../../concepts/ubiquitous-language.md)
