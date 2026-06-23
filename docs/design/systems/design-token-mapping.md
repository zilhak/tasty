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

## 토큰이 아닌 raw 값 주의

디자인 inline style 에는 토큰이 아닌 raw px 도 섞여 있다 (전사 시 그대로 옮기되 기록):
- remote_tool 헤더 `gap: 9` — 토큰 아님 (spacing_sm=8 과 1px 차).
- remote_tool 헤더 title `fontSize: 14` — 토큰 아님 (tasty 엔 14 폰트 토큰 없음 → heading 13 사용, 1px 차).
- remote_tool TabBtn `height: 35` / 탭바 `padding 0 8` / 탭 `padding 0 13` / `gap 2` — raw.
