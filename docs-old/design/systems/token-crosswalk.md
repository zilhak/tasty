# 토큰 크로스워크 (DTCG ↔ Rust Theme ↔ 호출처)

> **목적**: 디자인 시스템의 3-tier DTCG 토큰(`tokens/tasty.tokens.json`)과 Rust `ThemeColors`/`ThemeSizing` 필드, 그리고 실제 `th.*` / `theme.*` 호출처를 잇는 **정답지(crosswalk)**. 후속 작업 A2(Rust semantic tier 도입)가 "어떤 의미 alias 를 만들고, 어떤 호출처를 어느 role 로 가를지" 판단하는 근거 문서다.
>
> **이 문서는 매핑 기록일 뿐 코드를 바꾸지 않는다.** 현재 Rust 는 semantic tier 가 없고, **평면 primitive 필드** 만 노출하며 의미 매핑을 호출처가 암묵적으로 들고 있다.

생성 기준 데이터:
- DTCG: `Tasty Design System (1)/tokens/tasty.tokens.json` — primitive 73 / semantic 96 / component 148 (jq 검증, 아래 "검증 메모").
- Rust: `crates/tasty-type-appearance/src/theme.rs` (`ThemeColors` / `ThemeSizing` / `Theme`).
- Mocha 실제 색값: `crates/tasty-themes/src/fallback.rs` (alias 동일값 확인의 1차 출처).
- 호출처: 전 코드베이스 `rg '\bth\.'` + `rg '\btheme\.'`.

---

## 0. 구조 요약 (현재 상태)

```
DTCG:   primitive ──▶ semantic ──▶ component ──▶ UI(CSS var)
Rust:   ThemeColors(평면 primitive) ──▶ Theme(펼친 필드 + 도출 overlay) ──▶ UI(th.<field>)
                       ▲ semantic tier 없음 — 의미가 호출처에 암묵 매핑됨
```

- **DTCG semantic 96개** = `bg-*`(3) `surface-*`(3) `text-*`(6) `accent-*`(6) `border-*`(3) `overlay-*`+`separator`(3) `space-*`(5) 크기/타이포/모션 토큰들 + `surface-terminal/markdown-*`(8) + 터미널 색(`selection`/`vi-cursor`/`search-match*`)(4) + `ansi-*`(16) + `brand-melon-*`(3).
- **Rust `ThemeColors`** = 평면 primitive 만: neutral ramp 12 필드 + accent hue 13 필드 + 터미널 색 4 + ansi 16 + `surface_themes` map. semantic alias 없음.
- **핵심 다의성**: 같은 primitive 필드가 DTCG 에선 여러 semantic role 로 분기한다 (예: `surface1` = `surface-hover` + `border-strong` + `ansi-black`). A2 가 분기점을 판단해야 하는 곳. → §3.

---

## 1. Neutral ramp ↔ ThemeColors 필드 (elevation-role 번호)

DTCG `primitive.color-neutral-N` 은 **밝기가 아닌 elevation role** 로 번호가 매겨진다(`-0`=가장 깊은 배경, `-1100`=가장 강한 전경). Rust 의 catppuccin 12-step 필드와 1:1.

| neutral-N | Mocha hex | Latte hex | ThemeColors 필드 | 비고 |
|-----------|-----------|-----------|------------------|------|
| color-neutral-0    | #11111b | #dce0e8 | `crust`     | |
| color-neutral-100  | #181825 | #e6e9ef | `mantle`    | |
| color-neutral-200  | #1e1e2e | #eff1f5 | `base`      | |
| color-neutral-300  | #313244 | #ccd0da | `surface0`  | |
| color-neutral-400  | #45475a | #bcc0cc | `surface1`  | |
| color-neutral-500  | #585b70 | #acb0be | `surface2`  | |
| color-neutral-600  | #6c7086 | #9ca0b0 | `overlay0` **및** `placeholder` | 두 필드 동일값 (fallback.rs:40 `placeholder = overlay0`) |
| color-neutral-700  | #7f849c | #8c8fa1 | `overlay1`  | |
| color-neutral-800  | #9399b2 | #7c7f93 | `overlay2`  | DTCG semantic 소비처 없음 (실 UI 미사용) |
| color-neutral-900  | #a6adc8 | #6c6f85 | `subtext0`  | |
| color-neutral-1000 | #bac2de | #5c5f77 | `subtext1`  | |
| color-neutral-1100 | #cdd6f4 | #4c4f69 | `text`      | |

## 2. Accent hue ↔ ThemeColors 필드 (1:1, ramp 없음)

DTCG `primitive.color-<hue>` = ThemeColors `<hue>` 필드 그대로. Catppuccin 은 hue 당 1값이라 ramp 가 없다.

| primitive | Mocha hex | Latte hex | ThemeColors 필드 | semantic 소비처 |
|-----------|-----------|-----------|------------------|-----------------|
| color-blue      | #89b4fa | #1e66f5 | `blue`      | accent-primary, border-focus, ansi-blue, ansi-bright-blue |
| color-green     | #a6e3a1 | #40a02b | `green`     | accent-success, ansi-green, ansi-bright-green |
| color-red       | #f38ba8 | #d20f39 | `red`       | accent-danger, ansi-red, ansi-bright-red |
| color-yellow    | #f9e2af | #df8e1d | `yellow`    | accent-warning, ansi-yellow, ansi-bright-yellow, search-match |
| color-peach     | #fab387 | #fe640b | `peach`     | (semantic 직접 소비 없음 — 호출처 직접 사용) |
| color-mauve     | #cba6f7 | #8839ef | `mauve`     | accent-agent, ansi-magenta, ansi-bright-magenta |
| color-teal      | #94e2d5 | #179299 | `teal`      | ansi-cyan |
| color-sky       | #89dceb | #04a5e5 | `sky`       | accent-info, ansi-bright-cyan |
| color-lavender  | #b4befe | #7287fd | `lavender`  | vi-cursor-bg |
| color-flamingo  | #f2cdcd | #dd7878 | `flamingo`  | **semantic 소비처 없음** (스와치 전용) |
| color-pink      | #f5c2e7 | #ea76cb | `pink`      | **semantic 소비처 없음** (스와치 전용) |
| color-maroon    | #eba0ac | #e64553 | `maroon`    | **semantic 소비처 없음** (스와치 전용) |
| color-rosewater | #f5e0dc | #dc8a78 | `rosewater` | **semantic 소비처 없음** (스와치 전용) |

> `peach`/`flamingo`/`pink`/`maroon`/`rosewater` 는 어떤 DTCG semantic 토큰도 가리키지 않는다. `peach` 만 호출처에서 직접 사용(`plugins/ui/list.rs:218` 경고색), 나머지 4개는 `crates/tasty-gallery/src/catalog/theme.rs` 스와치 표시 외 실사용 0.

---

## 3. semantic 96개 전체 크로스워크 (정답지 본표)

각 행 = `(해석 primitive, ThemeColors/ThemeSizing 필드, 대표 호출처, 의미 라벨, 비고)`. alias 체인은 끝까지 resolve 했다. "없음(Rust 미대응)" = 해당 semantic 에 대응하는 Rust 필드가 존재하지 않음(존재하지 않는 필드를 지어내지 않기 위해 명시). 호출처가 진짜 없으면 "없음" 으로 표기.

### 3.1 배경 / 표면 (bg / surface)

| DTCG semantic | 해석 primitive | Rust 필드 | 대표 호출처 (file:line) | 의미 라벨 | 비고(다의성) |
|---|---|---|---|---|---|
| `bg-app` | color-neutral-0 | `crust` | `src/gfx/gpu/shell_setup.rs:40` | bg/app | **혼동주의**: shell_setup 가 `crust` 를 지역 alias `bg_panel` 로 명명. DTCG 상 app 배경 |
| `bg-sidebar` | color-neutral-100 | `mantle` | `src/gfx/gpu/shell_setup.rs:41` | bg/sidebar | shell_setup 지역 alias `bg_card` |
| `bg-panel` | color-neutral-200 | `base` | `src/gfx/gpu/render_pass.rs:16` | bg/panel | terminal unfocused 배경으로도 사용(§3.7) |
| `surface-raised` | color-neutral-300 | `surface0` | `src/gfx/gpu/shell_setup.rs:42` | surface/raised | **다의**: `surface0` = surface-raised + border-default (shell_setup 는 `border` 로 명명) |
| `surface-hover` | color-neutral-400 | `surface1` | `src/gfx/gpu/shell_setup.rs:47` | surface/hover | **다의**: `surface1` = surface-hover + border-strong + ansi-black (shell_setup 는 `accent_dis` 로 명명) |
| `surface-active` | color-neutral-500 | `surface2` | `src/gfx/gpu/shell_setup.rs:182` | surface/active | **다의**: `surface2` = surface-active + selection-bg (동일값) |

### 3.2 텍스트 (text)

| DTCG semantic | 해석 primitive | Rust 필드 | 대표 호출처 (file:line) | 의미 라벨 | 비고(다의성) |
|---|---|---|---|---|---|
| `text-primary` | color-neutral-1100 | `text` | `src/gfx/gpu/shell_setup.rs:82` | text/primary | **다의**: `text` = text-primary + ansi-bright-white (동일값). 호출 60회 |
| `text-secondary` | color-neutral-1000 | `subtext1` | `src/plugin_bridge/ui_tree_render.rs:679` | text/secondary | **다의**: `subtext1` = text-secondary + ansi-white (동일값) |
| `text-muted` | color-neutral-900 | `subtext0` | `src/gfx/gpu/shell_setup.rs:43` | text/muted | **최다 호출(99회)**. 대부분 muted, 일부 caption/보조 라벨색으로 혼용 — A2 분기 후보 |
| `text-disabled` | color-neutral-700 | `overlay1` | `src/view/settings/ui/keybindings_tab/entries.rs:121` | text/disabled | **다의**: `overlay1` 은 disabled 외 keybinding "recording" 강조색으로도 쓰임 |
| `text-placeholder` | color-neutral-600 | `placeholder` | `crates/tasty-egui-theme/src/lib.rs:27` | text/placeholder | `placeholder` 와 `overlay0` 가 같은 값(neutral-600). DTCG 는 단일 role |
| `text-on-accent` | color-neutral-0 (mocha) / color-white (latte) | 없음(Rust 미대응) | 없음 | text/on-accent | **role-remap**: mocha=neutral-0(=crust 동일값), latte=white. Rust 전용 필드 없어 latte remap 미반영 — A2 신설 후보 |

### 3.3 accent (의미색 6종)

| DTCG semantic | 해석 primitive | Rust 필드 | 대표 호출처 (file:line) | 의미 라벨 | 비고(다의성) |
|---|---|---|---|---|---|
| `accent-primary` | color-blue | `blue` | `src/view/main/mouse.rs:86` · `crates/tasty-egui-theme/src/lib.rs:89` | primary | **다의 핵심**: `blue` = accent-primary(selection/hyperlink) + border-focus(focus ring, lib.rs:91) + ansi-blue. 호출 22회 — A2 가 primary/focus/ansi 로 갈라야 함 |
| `accent-info` | color-sky | `sky` | `crates/tasty-gallery/src/catalog/theme.rs:53` (스와치) | info | 실 UI 직접 호출처 없음(스와치 전용). ansi-bright-cyan 로만 간접 사용 — **확인 필요**(info role 미구현) |
| `accent-success` | color-green | `green` | `src/gfx/gpu/shell_setup.rs:46` | success | **다의**: `green` = success + ansi-green. 호출 23회 |
| `accent-warning` | color-yellow | `yellow` | `src/gfx/gpu/shell_setup.rs:44` | warning | **다의**: `yellow` = warning + ansi-yellow + search-match base |
| `accent-danger` | color-red | `red` | `src/gfx/gpu/shell_setup.rs:45` | danger | **다의**: `red` = danger(error_fg lib.rs:95) + ansi-red. 호출 17회 |
| `accent-agent` | color-mauve | `mauve` | `src/view/plugins/ui/list.rs:113` | agent | **다의**: `mauve` = agent + ansi-magenta |

### 3.4 border / overlay / separator

| DTCG semantic | 해석 primitive | Rust 필드 | 대표 호출처 (file:line) | 의미 라벨 | 비고(다의성) |
|---|---|---|---|---|---|
| `border-default` | color-neutral-300 | `surface0` | `src/gfx/gpu/shell_setup.rs:42` | border/default | `surface0` 공유(=surface-raised). 호출처가 surface vs border 의도 구분 안 됨 |
| `border-strong` | color-neutral-400 | `surface1` | `src/plugin_bridge/popup_render.rs:109` | border/strong | `surface1` 공유(=surface-hover). `border_width` 와 함께 Stroke 로 사용 |
| `border-focus` | color-blue | `blue` | `crates/tasty-egui-theme/src/lib.rs:91` | border/focus | `blue` 공유(=accent-primary). focus ring stroke |
| `overlay-hover` | alpha-white-8 (mocha) / alpha-black-8 (latte) | `hover_overlay` (도출) | `src/adapters/ui/popup/file_open.rs:152` | overlay/hover | `is_light` 에서 도출(theme.rs `derive_overlays`). premultiplied. 호출 15회 |
| `overlay-active` | alpha-white-12 (mocha) / alpha-black-12 (latte) | `active_overlay` (도출) | `src/adapters/ui/tab_bar.rs:372` | overlay/active | 도출 필드. 호출 5회 |
| `separator` | alpha-white-8 (mocha) / alpha-black-8 (latte) | `separator` (도출) | 없음 | separator | **실사용 0**: `Theme.separator` 필드는 존재하나 호출처 없음(코드의 `ui.separator()` 는 egui 자체 위젯, 무관) |

### 3.5 간격 / 크기 / 반경 (ThemeSizing)

| DTCG semantic | 해석 primitive | Rust 필드 | 대표 호출처 (file:line) | 의미 라벨 | 비고 |
|---|---|---|---|---|---|
| `space-xs` | size-4 | `spacing_xs` | `src/adapters/ui/popup/ssh_tool.rs:347` | space/xs | |
| `space-sm` | size-8 | `spacing_sm` | `src/adapters/ui/surface/image.rs:24` | space/sm | |
| `space-md` | size-12 | `spacing_md` | `crates/tasty-gallery/src/catalog/spacing.rs:11` | space/md | 실 UI 직접 호출처는 gallery 외 미확인 — **확인 필요** |
| `space-lg` | size-16 | `spacing_lg` | `src/adapters/ui/popup/ssh_tool.rs:351` | space/lg | |
| `space-xl` | size-24 | `spacing_xl` | `crates/tasty-gallery/src/catalog/spacing.rs:13` | space/xl | gallery 외 직접 호출처 미확인 — **확인 필요** |
| `border-width` | size-1 | `border_width` | `src/plugin_bridge/popup_render.rs:109` | border-width | zoom 미적용(theme.rs 정책) |
| `focus-ring-width` | size-2 | `focus_ring_width` | `crates/tasty-egui-theme/src/lib.rs:91` | focus-ring-width | |
| `radius` | radius-4 | `corner_radius` | `src/adapters/ui/toast.rs:136` | radius | **다의**: DTCG 는 radius/radius-sm/radius-pill 3종, Rust 는 `corner_radius` 단일 |
| `radius-sm` | radius-2 | 없음(Rust 미대응) | 없음 | radius/sm | Rust 미대응 — A2 신설 후보 |
| `radius-pill` | radius-full (9999px) | 없음(Rust 미대응) | 없음 | radius/pill | Rust 미대응 (switch/badge pill 형태 미구현) |
| `control-height-tree` | size-22 | `item_height_tree` | 없음 | control-height/tree | **실사용 0**(필드만 존재) |
| `control-height` | size-28 | `item_height_interactive` | `src/adapters/ui/popup.rs:102` | control-height | |
| `control-height-tab` | size-24 | `item_height_tab` | `src/adapters/ui/sidebar/view.rs:110` | control-height/tab | |
| `tab-width` | size-150 | `tab_width` | 없음 | tab-width | **실사용 0**: `Theme.tab_width` 호출처 없음(`settings.appearance.tab_width` 는 별개 설정값) |

### 3.6 타이포 / 모션 / 스케일 (대부분 Rust 미대응)

| DTCG semantic | 해석 primitive | Rust 필드 | 대표 호출처 (file:line) | 의미 라벨 | 비고 |
|---|---|---|---|---|---|
| `font-ui` | font-family-sans (var) | 없음(Rust 미대응) | 없음 | font/ui | 폰트 패밀리는 Theme 밖에서 관리 |
| `font-mono` | font-family-mono (var) | 없음(Rust 미대응) | 없음 | font/mono | 〃 |
| `font-size-caption` | font-size-11 | `font_size_caption` | `src/view/settings/ui/tabs/appearance.rs:905` | font-size/caption | 호출 39회 |
| `font-size-body` | font-size-13 | `font_size_body` | `src/adapters/ui/toast.rs:110` | font-size/body | 호출 48회(최다 sizing) |
| `font-size-heading` | font-size-13 | `font_size_heading` | `crates/tasty-gallery/src/catalog/components/port_scanner.rs:131` | font-size/heading | body 와 동일 13px, weight(semibold)로만 구분 |
| `font-size-max` | font-size-14 | `font_size_max` | 없음 | font-size/max | **실사용 0**(상한 클램프용 필드만 존재) |
| `font-size-term-sm` | font-size-12 | 없음(Rust 미대응) | 없음 | font-size/term-sm | 터미널 폰트 크기는 별도 경로 — **확인 필요** |
| `font-size-term` | font-size-14 | 없음(Rust 미대응) | 없음 | font-size/term | 〃 |
| `font-size-term-lg` | font-size-16 | 없음(Rust 미대응) | 없음 | font-size/term-lg | 〃 |
| `font-weight-normal` | font-weight-400 | 없음(Rust 미대응) | 없음 | font-weight/normal | egui `FontId` 가 weight 미보유 |
| `font-weight-medium` | font-weight-500 | 없음(Rust 미대응) | 없음 | font-weight/medium | 〃 |
| `font-weight-semibold` | font-weight-600 | 없음(Rust 미대응) | 없음 | font-weight/semibold | 〃 |
| `font-weight-bold` | font-weight-700 | 없음(Rust 미대응) | 없음 | font-weight/bold | 〃 |
| `line-height-tight` | line-height-100 | 없음(Rust 미대응) | 없음 | line-height/tight | |
| `line-height-term` | line-height-120 | 없음(Rust 미대응) | 없음 | line-height/term | |
| `line-height-ui` | line-height-140 | 없음(Rust 미대응) | 없음 | line-height/ui | |
| `line-height-prose` | line-height-160 | 없음(Rust 미대응) | 없음 | line-height/prose | |
| `letter-spacing-ui` | letter-spacing-0 | 없음(Rust 미대응) | 없음 | letter-spacing/ui | |
| `letter-spacing-caps` | letter-spacing-04 | 없음(Rust 미대응) | 없음 | letter-spacing/caps | |
| `motion-term` | duration-0 | 없음(Rust 미대응) | 없음 | motion/term | 터미널 0ms 애니 정책은 코드에 암묵 |
| `motion-ui` | duration-120 | 없음(Rust 미대응) | 없음 | motion/ui | |
| `motion-ui-fast` | duration-90 | 없음(Rust 미대응) | 없음 | motion/ui-fast | |
| `ease-ui` | easing-standard | 없음(Rust 미대응) | 없음 | ease/ui | |
| `ui-scale-sm` | scale-80 | 없음(Rust 미대응) | 없음 | ui-scale/sm | host `ui_zoom`(with_colors_and_zoom)으로 별도 처리 |
| `ui-scale-md` | scale-100 | 없음(Rust 미대응) | 없음 | ui-scale/md | 〃 |
| `ui-scale-lg` | scale-120 | 없음(Rust 미대응) | 없음 | ui-scale/lg | 〃 |
| `ui-scale` | {semantic.ui-scale-md} → scale-100 | 없음(Rust 미대응) | 없음 | ui-scale | semantic→semantic alias(2단) |

### 3.7 surface kind (terminal / markdown) — `surface_themes` map

DTCG 의 8개 `surface-<kind>-<focus>-<part>` 토큰은 Rust `ThemeColors.surface_themes: BTreeMap<String, SurfaceTheme>` 의 entry 로 매핑된다. 호출은 `theme.surface("terminal")` / `theme.surface("markdown")` 헬퍼 경유. focused_bg 만 black/white role-remap.

| DTCG semantic | 해석 primitive/semantic | Rust 필드 | 대표 호출처 (file:line) | 의미 라벨 | 비고 |
|---|---|---|---|---|---|
| `surface-terminal-focused-bg` | color-black (mocha) / color-white (latte) | `surface_themes["terminal"].focused_bg` | `src/gfx/gpu/render_pass.rs:62` | surface/terminal/focused-bg | role-remap. fallback.rs:89 = #000000 |
| `surface-terminal-focused-fg` | {text-primary} → neutral-1100 | `surface_themes["terminal"].focused_fg` | `src/gfx/gpu/render_pass.rs:62` | surface/terminal/focused-fg | = `text` 동일값 |
| `surface-terminal-unfocused-bg` | {bg-panel} → neutral-200 | `surface_themes["terminal"].unfocused_bg` | `src/gfx/gpu/render_pass.rs:62` | surface/terminal/unfocused-bg | = `base` 동일값 (fallback.rs:91) |
| `surface-terminal-unfocused-fg` | {text-muted} → neutral-900 | `surface_themes["terminal"].unfocused_fg` | `src/gfx/gpu/render_pass.rs:62` | surface/terminal/unfocused-fg | = `subtext0` 동일값 |
| `surface-markdown-focused-bg` | color-black (mocha) / color-white (latte) | `surface_themes["markdown"].focused_bg` | `src/adapters/ui/egui_panels.rs:92` | surface/markdown/focused-bg | role-remap. fallback.rs:100 = #000000 |
| `surface-markdown-focused-fg` | {text-primary} → neutral-1100 | `surface_themes["markdown"].focused_fg` | `src/adapters/ui/egui_panels.rs:92` | surface/markdown/focused-fg | = `text` 동일값 |
| `surface-markdown-unfocused-bg` | {bg-sidebar} → neutral-100 | `surface_themes["markdown"].unfocused_bg` | `src/adapters/ui/egui_panels.rs:92` | surface/markdown/unfocused-bg | = `mantle` 동일값. terminal 보다 한 단계 어두움(fallback.rs:102) |
| `surface-markdown-unfocused-fg` | {text-muted} → neutral-900 | `surface_themes["markdown"].unfocused_fg` | `src/adapters/ui/egui_panels.rs:92` | surface/markdown/unfocused-fg | = `subtext0` 동일값 |

### 3.8 터미널 색 (selection / vi-cursor / search-match)

| DTCG semantic | 해석 primitive | Rust 필드 | 대표 호출처 (file:line) | 의미 라벨 | 비고 |
|---|---|---|---|---|---|
| `selection-bg` | color-neutral-500 | `selection_bg` | `src/view/main/mouse.rs:87` | selection-bg | `surface2` 와 동일값(neutral-500) — A2 가 alias 할지 분리할지 판단 |
| `vi-cursor-bg` | color-lavender | `vi_cursor_bg` | `src/gfx/gpu/render_pass.rs:118` | vi-cursor-bg | `lavender` 와 동일값. selection_bg 와 시각 구분 필요(theme.rs 주석) |
| `search-match-bg` | color-mix(yellow 30%) | `search_match_bg` | `src/gfx/gpu/render_pass.rs:144` | search-match | yellow @ ~30% alpha (fallback.rs:58) |
| `search-match-active-bg` | color-mix(yellow 70%) | `search_match_active_bg` | `src/gfx/gpu/render_pass.rs:145` | search-match/active | yellow @ ~70% alpha (fallback.rs:59) |

### 3.9 ANSI 16색

ANSI 필드는 개별 `th.*` 호출이 아니라 `theme.ansi_palette()`(theme.rs:844) 배열로 GPU 렌더러에 한 번에 넘어간다. 대표 호출처는 모두 `src/gfx/gpu/render_pass.rs:64`. 다수가 neutral/accent 필드와 **동일값**(별도 필드지만 같은 색).

| DTCG semantic | 해석 primitive | Rust 필드 | 대표 호출처 (file:line) | 의미 라벨 | 비고(동일값) |
|---|---|---|---|---|---|
| `ansi-black` | color-neutral-400 | `ansi_black` | `src/gfx/gpu/render_pass.rs:64` | ansi/black | = `surface1` |
| `ansi-red` | color-red | `ansi_red` | `src/gfx/gpu/render_pass.rs:64` | ansi/red | = `red` |
| `ansi-green` | color-green | `ansi_green` | `src/gfx/gpu/render_pass.rs:64` | ansi/green | = `green` |
| `ansi-yellow` | color-yellow | `ansi_yellow` | `src/gfx/gpu/render_pass.rs:64` | ansi/yellow | = `yellow` |
| `ansi-blue` | color-blue | `ansi_blue` | `src/gfx/gpu/render_pass.rs:64` | ansi/blue | = `blue` |
| `ansi-magenta` | color-mauve | `ansi_magenta` | `src/gfx/gpu/render_pass.rs:64` | ansi/magenta | = `mauve` |
| `ansi-cyan` | color-teal | `ansi_cyan` | `src/gfx/gpu/render_pass.rs:64` | ansi/cyan | = `teal` |
| `ansi-white` | color-neutral-1000 | `ansi_white` | `src/gfx/gpu/render_pass.rs:64` | ansi/white | = `subtext1` |
| `ansi-bright-black` | color-neutral-600 | `ansi_bright_black` | `src/gfx/gpu/render_pass.rs:64` | ansi/bright-black | = `overlay0` |
| `ansi-bright-red` | color-red | `ansi_bright_red` | `src/gfx/gpu/render_pass.rs:64` | ansi/bright-red | = `red` |
| `ansi-bright-green` | color-green | `ansi_bright_green` | `src/gfx/gpu/render_pass.rs:64` | ansi/bright-green | = `green` |
| `ansi-bright-yellow` | color-yellow | `ansi_bright_yellow` | `src/gfx/gpu/render_pass.rs:64` | ansi/bright-yellow | = `yellow` |
| `ansi-bright-blue` | color-blue | `ansi_bright_blue` | `src/gfx/gpu/render_pass.rs:64` | ansi/bright-blue | = `blue` |
| `ansi-bright-magenta` | color-mauve | `ansi_bright_magenta` | `src/gfx/gpu/render_pass.rs:64` | ansi/bright-magenta | = `mauve` |
| `ansi-bright-cyan` | color-sky | `ansi_bright_cyan` | `src/gfx/gpu/render_pass.rs:64` | ansi/bright-cyan | = `sky` |
| `ansi-bright-white` | color-neutral-1100 | `ansi_bright_white` | `src/gfx/gpu/render_pass.rs:64` | ansi/bright-white | = `text` |

### 3.10 brand (수박 로고색)

| DTCG semantic | 해석 primitive | Rust 필드 | 대표 호출처 (file:line) | 의미 라벨 | 비고 |
|---|---|---|---|---|---|
| `brand-melon-flesh` | color-melon-flesh (#f25d6b) | 없음(Rust 미대응) | 없음 | brand/melon-flesh | `ThemeColors` 에 brand 필드 없음. 로고색은 Theme 밖 |
| `brand-melon-rind` | color-melon-rind (#1e7d4f) | 없음(Rust 미대응) | 없음 | brand/melon-rind | 〃 |
| `brand-melon-seed` | color-melon-seed (#11111b) | 없음(Rust 미대응) | 없음 | brand/melon-seed | 〃 (neutral-0 와 같은 hex 지만 별도 primitive) |

> **행 수 합계**: 3.1(6) + 3.2(6) + 3.3(6) + 3.4(6) + 3.5(14) + 3.6(27) + 3.7(8) + 3.8(4) + 3.9(16) + 3.10(3) = **96**. (= DTCG semantic 96개 전부)

---

## 4. A2 가 분기 판단해야 할 다의성 핫스팟 (요약)

같은 Rust primitive 필드가 여러 의미 role 로 쓰이는 지점. A2 가 semantic alias 를 도입할 때 호출처별로 어느 role 인지 가려야 한다.

| Rust 필드 | 겹치는 DTCG role | 갈래 판단 포인트 |
|---|---|---|
| `blue` | accent-primary / border-focus / ansi-blue | selection·hyperlink=primary, focus ring stroke=border-focus, 터미널 팔레트=ansi |
| `yellow` | accent-warning / ansi-yellow / search-match | 경고 아이콘=warning, search highlight=search-match, 팔레트=ansi |
| `red` | accent-danger / ansi-red | error_fg·danger 버튼=danger, 팔레트=ansi |
| `green` | accent-success / ansi-green | 성공 표시=success, 팔레트=ansi |
| `mauve` | accent-agent / ansi-magenta | 에이전트 강조=agent, 팔레트=ansi |
| `surface0` | surface-raised / border-default | 채움=raised, 1px 선=border |
| `surface1` | surface-hover / border-strong / ansi-black | hover 배경=hover, 강조 선=border-strong, 팔레트=ansi-black |
| `surface2` | surface-active / selection-bg | active 배경 vs 터미널 선택 |
| `subtext0` | text-muted (+caption 혼용) | 호출 99회 — muted 본문 vs 보조/caption 라벨 |
| `overlay1` | text-disabled (+recording 강조) | 비활성 텍스트 vs keybinding 녹화 강조 |
| `text` | text-primary / ansi-bright-white | UI 본문 vs 팔레트 |
| `subtext1` | text-secondary / ansi-white | 보조 텍스트 vs 팔레트 |
| `crust` | bg-app (shell_setup 가 bg_panel 로 오인명명) | 지역 alias 명명이 DTCG role 과 어긋남 — 정합화 필요 |

---

## 5. component tier (148개) — Rust 대응 여부

DTCG component 토큰은 항상 `{semantic.*}`(드물게 `{primitive.*}`) 한 단계만 가리킨다. Rust 에는 **component tier 가 존재하지 않는다** — 호출처가 semantic 의도를 직접 들고 컴포넌트를 그린다.

### 5.1 Rust 에 "개념적" 대응이 있는 그룹 (semantic 경유로 간접 표현됨)

해당 컴포넌트 UI 가 코드에 실제로 그려지며, component 토큰이 가리키는 semantic 의 Rust 필드를 호출처가 직접 쓴다.

| component 그룹 | 토큰 수(대략) | 가리키는 주요 semantic | Rust 표현 방식 |
|---|---|---|---|
| `button-*` / `icon-button-*` | 24 | accent-primary/agent/danger, surface-raised, border-*, text-*, control-height, radius, space-* | 호출처가 `blue`/`mauve`/`red`/`surface0`/`corner_radius` 등 직접 사용. primary/agent/danger 변종은 호출처 분기 |
| `input-*` / `select-*` | 18 | surface-raised, border-default/focus, text-primary/placeholder, accent-danger | `surface0`/`blue`/`placeholder` 직접. focus·invalid 상태는 호출처 판단 |
| `tab-*` | 12 | bg-sidebar/bg-panel, text-muted/secondary/primary, accent-primary, separator | tab_bar.rs 가 `mantle`/`base`/`subtext0`/`blue` 직접. 단 `tab_bar_*` sizing 은 zoom 제외 별도 필드 |
| `menu-*` | 9 | surface-raised, border-default, text-*, overlay-hover, control-height | popup/menu 렌더가 직접 |
| `toast-*` | 14 | surface-raised, accent-info/success/warning/danger/agent, text-primary, space-* | `toast.rs` 가 `font_size_body`/`corner_radius` 직접. accent 변종은 호출처 분기 |
| `tree-row-*` | 6 | control-height-tree, text-secondary/primary, overlay-hover, surface-active | sidebar tree 렌더 |
| `checkbox-*` / `switch-*` | 13 | accent-primary, surface-raised/active, border-strong/focus, text-on-accent | 부분 구현. `text-on-accent` 는 Rust 필드 부재(§3.2) |

### 5.2 Rust 미대응 (웹 전용 / 필드 부재)

| component 그룹 | 토큰 수(대략) | 미대응 사유 |
|---|---|---|
| `badge-*` / `tag-*` / `kbd-*` | 16 | 해당 UI 위젯이 Rust 에 미구현(또는 텍스트로만 표현). `radius-sm`(§3.5) 미대응과 연동 |
| `status-dot-*` | 7 | accent-info(=sky) 등 status role 미구현 — `sky`/`accent-info` 가 실 UI 호출처 없음(§3.3) |
| `spinner-*` | 4 | 스피너 위젯 미구현(`motion`/`duration` Rust 미대응) |
| `table-*` | 11 | 테이블 위젯 미구현(`font-mono`/`separator` Rust 미대응) |
| `switch-*` pill 형태 | (5.1 중복) | `radius-pill` 미대응으로 pill 토글 형태 미구현 |
| 모든 `*-padding-*`/`*-gap`/`*-radius`/`*-font-*` 치수 토큰 | 다수 | component tier 자체가 없어 semantic sizing(`spacing_*`/`corner_radius`/`font_size_*`)으로 직접 흡수. component 단위 override 불가 |

> **A2 관점**: component tier 도입은 본 작업(A2: semantic tier) 범위 밖. 다만 `text-on-accent`·`radius-sm`·`radius-pill`·`accent-info` 처럼 **semantic 이 없어 component 가 떠 있는** 케이스는 A2 가 semantic 을 신설하면 자연히 받침이 생긴다.

---

## 검증 메모

완료 조건 3개를 다음 절차로 직접 확인했다.

### (1) semantic 96개 전부가 4값 행으로 존재 (행 수 == 96)

- 카운트 명령:
  ```
  jq '.semantic | keys | map(select(. != "$description")) | length' tasty.tokens.json  # → 96
  jq '.primitive | ... | length'  # → 73,  '.component | ... | length'  # → 148
  ```
- 본표 §3.1~§3.10 행 수 합계 = 6+6+6+6+14+27+8+4+16+3 = **96**. jq 가 출력한 96개 키 순서대로 빠짐없이 매핑(§3 각 절이 키 목록을 그대로 따름).
- 각 행은 `(해석 primitive · Rust 필드 · 대표 호출처 · 의미 라벨 · 비고)` 5열을 모두 채움. Rust 미대응 토큰은 필드열 "없음(Rust 미대응)", 호출처 없으면 "없음" 으로 명시(추측 금지).

### (2) resolve 무결 — 참조한 Rust 필드가 theme.rs 실제 필드와 일치 (존재하지 않는 필드 0건)

- `theme.rs` 의 `ThemeColors`(라인 200~266) / `ThemeSizing`(110~154) / 도출 필드(`hover_overlay`/`active_overlay`/`separator`, 599~603)와 대조.
- 본표가 참조한 실존 필드: neutral 12(`crust`/`mantle`/`base`/`surface0..2`/`overlay0..2`/`subtext0`/`subtext1`/`text`) + `placeholder` + accent 13 + 터미널 4(`selection_bg`/`vi_cursor_bg`/`search_match_bg`/`search_match_active_bg`) + ansi 16 + 도출 3 + sizing(`spacing_xs/sm/md/lg/xl`/`border_width`/`focus_ring_width`/`corner_radius`/`item_height_tree`/`item_height_interactive`/`item_height_tab`/`tab_width`/`font_size_caption/body/heading/max`) + `surface_themes` map entry. **전부 theme.rs 에 실재.**
- 미존재 필드를 지어낸 곳 0건: `text-on-accent`/`radius-sm`/`radius-pill`/`font-*`/`line-height-*`/`letter-spacing-*`/`motion-*`/`ease-*`/`ui-scale-*`/`brand-*`/`font-size-term*` 는 모두 "없음(Rust 미대응)" 으로 표기.
- 동일값 alias(예: `placeholder`=`overlay0`, `selection_bg`=`surface2`, `ansi_*`=neutral/accent)는 `fallback.rs`(mocha 실제 hex)로 교차검증.

### (3) th.* 대표 호출처 재검색 stale 0 (샘플)

`rg` 재검색으로 표의 대표 위치 실재 확인. 샘플:
- `rg -n '\bth\.subtext0\b'` → `shell_setup.rs:43` 등 99건 존재.
- `rg -n '\bth\.blue\b'` → `mouse.rs:86` 외 22건. `crates/tasty-egui-theme/src/lib.rs:89,91` (selection/focus ring) 존재.
- `rg -n 'theme\.surface\("terminal"\)'` → `render_pass.rs:62` 존재. `theme.ansi_palette()` → `render_pass.rs:64` 존재.
- `rg -n 'theme\.vi_cursor_bg'` → `render_pass.rs:118`, `theme.search_match_bg` → `render_pass.rs:144` 존재.
- 호출처 "없음" 으로 적은 필드(`separator`/`font_size_max`/`item_height_tree`/`tab_width`(Theme 필드)/accent 미사용분)는 재검색으로 **실 UI 호출 0건** 임을 역확인(스와치·테스트·필드 정의 제외).

> 한계: 호출처는 `th.` / `theme.` 두 바인딩만 수집. 다른 지역 alias(예: shell_setup 의 `bg_panel = th.crust`)는 원천 `th.<field>` 라인을 대표로 기록했다. 전수 호출 빈도가 아니라 **대표 1곳 + 빈도 메모** 방침.
