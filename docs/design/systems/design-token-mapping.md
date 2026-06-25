# Design Token → tasty Theme 매핑

claude design(`Tasty Design System`)의 semantic 토큰을 tasty `Theme` 필드로 옮길 때 쓰는
확정 매핑표. `design-parity` 스킬이 참조한다. **hex 가 아니라 Theme 필드로 접근한다**
(테마가 바뀌면 hex 는 달라지므로).

출처: 디자인 루트의 `tokens/semantic.css` + `tokens/primitives.css` (mocha 기준).

## 색

| 디자인 토큰 | tasty Theme | mocha hex | 비고 |
|---|---|---|---|
| `bg-sidebar` | `mantle` | `#181825` | 사이드바·**탭 바** 등 한 단계 더 어두운 면 |
| `bg-panel` | `base` | `#1e1e2e` | 패널형 팝업 본문 (remote_tool / port_scanner) |
| `surface-raised` | `surface0` | `#313244` | 카드·입력·메뉴·command_palette 본문, secondary 버튼 fill |
| `border-default` | `surface0` | `#313244` | |
| `border-strong` | `surface1` | `#45475a` | 팝업 외곽선·강한 구분 |
| `separator` | 흰색 8% 알파 | — | **구역 bg 위 블렌드.** base 위 → ≈surface0, mantle 위 → ≈surface1 근사. egui 는 `ui.separator()`(비가시) 대신 surface1 `hline` 으로 그린다 |
| `text-primary` | `text` | `#cdd6f4` | |
| `text-secondary` | `subtext1` | `#bac2de` | |
| `text-muted` | `subtext0` | `#a6adc8` | |
| `text-disabled` | `overlay1` | `#7f849c` | |
| `accent-primary` | `accent_primary()` | `#89b4fa` | primary 버튼·포커스·활성 탭 언더라인 |
| `accent-danger` | `accent_danger()` | — | |
| `os-macos-close` | `accent_macos_close()` | `#ec6a5e` | macOS 신호등 close (테마 불변 OS 리터럴, const `OS_MACOS_CLOSE`) |
| `os-macos-min` | `accent_macos_min()` | `#f4bf4f` | macOS 신호등 minimize (const `OS_MACOS_MIN`) |
| `os-macos-zoom` | `accent_macos_zoom()` | `#61c554` | macOS 신호등 zoom (const `OS_MACOS_ZOOM`) |

## 불투명도

| 디자인 토큰 | tasty Theme | 값 | 비고 |
|---|---|---|---|
| `opacity-disabled` | `opacity_disabled()` | `0.5` | disabled 컨트롤 공통 디밍. const `OPACITY_DISABLED`. 모든 위젯이 disabled 시 이 값으로 `gamma_multiply` |

## 치수 (px → LogicalPx)

| 디자인 토큰 | 값 | tasty |
|---|---|---|
| space xs / sm / md / lg | 4 / 8 / 12 / 16 | `spacing_xs/sm/md/lg` |
| control-height (버튼·입력) | 28 | |
| control-height-tab | 24 | |
| control-height-tree | 22 | |
| font-size body / caption / heading | 13 / 11 / 13(weight 600) | `font_size_body/caption/heading` |
| font-size micro | 10 | `font_size_micro` — Badge/Tag/Kbd·command_palette footer 힌트 |
| font-size prose-h1 / prose-h2 | 20 / 14 | `font_size_prose_h1` / `font_size_prose_h2` — markdown prose (UI cap 면제) |
| font-size term-sm / term / term-lg | 12 / 14 / 16 | `font_size_term_sm` / `font_size_term` / `font_size_term_lg` — 터미널 스케일 |
| icon-size xs | 12 | `icon_glyph_size_xs` |
| icon-size md | 16 | `icon_glyph_size_md` (기존) — Button leading/trailing·MenuItem 글리프 |
| radius / radius-sm | 4 / 2 | `corner_radius` |
| border-width | 1 (항상) | `border_width` |

## token-policy 값 변동 (★)

디자인 token-policy 정합으로 다음 위젯 치수가 on-grid 값으로 확정됐다 (`crates/tasty-ui-widgets/`):

| 위젯 | 항목 | 값 | 비고 |
|---|---|---|---|
| Switch | track | 28×16 | on-grid (이전 32×18 의 off-grid 18 제거) |
| TreeRow | per-level indent | 12 | `--tasty-tree-row-indent` = space-md(12) → `theme.spacing_md` |
| Badge / Tag / Kbd | pill 높이 | 16 | size-16. Kbd min-width 도 16 |
| Badge / Tag / Kbd | 폰트 | micro(10) | `font_size_micro` |
| MenuItem | 아이콘 글리프 | 16 | icon-size-md (15 → 16 snap), `icon_glyph_size_md` |
| (공용) | disabled opacity | 0.5 | `opacity_disabled()` |

## switch-number overlay (chrome)

디자인 `tokens/components.css` 의 `CHROME · SWITCH-NUMBER OVERLAY` 블록(8 토큰). modifier 홀드 중
탭/워크스페이스의 leading indicator(탭 아이콘 / 워크스페이스 status dot / collapsed letter avatar)를
숫자 키캡으로 제자리 교체하는 패턴. **비active 키캡은 기존 `Kbd` 키캡 그대로** (디자인도
`var(--tasty-kbd-*)` 로 alias) → tasty 의 `kbd()`(`crates/tasty-ui-widgets/src/chip.rs`) 가 이미 커버.
**신규는 active(현재 탭/ws) 키캡의 accent-filled bg/fg 뿐**이고, 그조차 기존 semantic 접근자로
표현된다 → **신규 Theme 필드 0개** (component→semantic 직접 매핑, button-primary-bg→accent_primary() 와 동일 관습).

| 디자인 토큰 | 디자인 체인 | tasty Theme / 위젯 | 비고 |
|---|---|---|---|
| `--tasty-switch-overlay-size` | → `kbd-size` → `size-16` (16px) | `chip.rs` `KBD_HEIGHT`/`KBD_MIN_W = 16.0` (위젯 상수) | 키캡 footprint = 아이콘/dot slot. Theme 필드 아님(Kbd 위젯 상수) |
| `--tasty-switch-overlay-bg` | → `kbd-bg` → `surface-raised` | `Theme::surface0` | 비active 키캡 fill |
| `--tasty-switch-overlay-fg` | → `kbd-fg` → `text-secondary` | `Theme::subtext1` | 비active 키캡 숫자 |
| `--tasty-switch-overlay-border` | → `kbd-border` → `border-strong` | `Theme::surface1` | 키캡 외곽선 |
| `--tasty-switch-overlay-shadow-depth` | → `kbd-shadow-depth` → `size-2` (2px) | `chip.rs` `KBD_BOTTOM_BORDER = 2.0` (위젯 상수) | 키캡 하단 3D edge. Theme 필드 아님 |
| `--tasty-switch-overlay-active-bg` | → `accent-primary` → `color-blue` | `Theme::accent_primary()` | **현재 항목 = accent-filled 키캡 bg.** 기존 접근자 |
| `--tasty-switch-overlay-active-fg` | → `text-on-accent` → `color-neutral-0` | `Theme::text_on_accent()` | accent fill 위 숫자. 기존 접근자 (⚠ text_on_accent 는 잠정 `crust` 매핑 — mocha OK, latte white 미반영. button-primary-fg/checkbox-check 등과 공유하는 선재 한계, switch-overlay 고유 이슈 아님) |
| `--tasty-switch-overlay-fade` | → `motion-ui-fast` → `duration-90` (90ms) | (없음 — 모션 토큰 미보유) | 등장 90ms ease, release 0ms. egui immediate-mode 는 end-state 로 snap = readme 상 compliant. P2 draw 의 선택적 연출, Theme 필드 불필요 |

> **결론(검증 완료)**: switch-number overlay 8 토큰 모두 **기존 Theme 접근자(`accent_primary()`/`text_on_accent()`/`surface0`/`subtext1`/`surface1`)·위젯 상수·`font_size_micro` 로 커버** → P0 에서 추가할 신규 Theme 필드 없음. P2(draw)는 비active 키캡=`kbd()` 재사용, active 키캡=`accent_primary()` fill + `text_on_accent()` 숫자로 그린다.

## 토큰이 아닌 raw 값 주의

디자인 inline style 에는 토큰이 아닌 raw px 도 섞여 있다 (전사 시 그대로 옮기되 기록):
- remote_tool 헤더 `gap: 9` — 토큰 아님 (spacing_sm=8 과 1px 차).
- remote_tool 헤더 title `fontSize: 14` — 토큰 아님 (tasty 엔 14 폰트 토큰 없음 → heading 13 사용, 1px 차).
- remote_tool TabBtn `height: 35` / 탭바 `padding 0 8` / 탭 `padding 0 13` / `gap 2` — raw.
