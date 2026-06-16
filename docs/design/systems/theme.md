# 테마 시스템 (운영 상세)

색상·타이포·간격의 단일 출처인 `Theme` 와 그 위의 **UI 디자인 규칙**. 모든 UI 는 색/크기/간격을 `Theme` 에서 가져온다(하드코딩 금지).

## 핵심 모델

```text
[ 디스크 ] ~/.tasty/themes/<id>.toml          ← partial TOML, 파일명 stem = id
                  │  (settings 에서 테마 선택)
[ tasty-themes ] apply_theme(settings, id)
                  ├── theme_base.apply_partial(file)   ← 누락 필드 보존(누적)
                  └── theme_overrides.clear()
                  │  (픽커로 색 변경 시) theme_overrides.field = Some(new)
              resolve(settings) = theme_base ▷ theme_overrides + is_light overlay + SIZING
                  │
[ tasty-themes ] 전역 Theme 인스턴스 (1개, RwLock)
                  │
              theme().crust / theme().spacing_sm / theme().is_light   ← UI 코드
```

### 두 레이어

| 레이어 | 타입 | 의미 | 테마 변경 시 |
|--------|------|------|--------------|
| `theme_base` | `ThemeColors`(풀 세트) | 적용된 테마들의 누적 결과 | partial 덮어쓰기로 누적 |
| `theme_overrides` | `PartialColors`(모든 필드 `Option`) | 사용자가 픽커로 손댄 흔적 | **클리어** |

화면 적용 색 = `theme_base ▷ theme_overrides`(override 의 `Some` 필드만 덮어쓰기). partial 테마(일부 색만 정의)를 적용하면 누락 필드는 이전 base 값을 유지한다(누적).

### Crate 책임

| crate | 책임 | IO |
|-------|------|----|
| `tasty-type-appearance::color` | `HexColor`, `GpuRgba`/`GpuRgb` newtype | 없음 |
| `tasty-type-appearance::theme` | `Theme` · `ThemeColors` · `PartialColors` · `ThemeSizing`/`SIZING` · `SurfaceTheme`/`FALLBACK_SURFACE` · `derive_overlays` · `Theme::surface(id)` | 없음 |
| `tasty-themes` | 전역 `RwLock<Theme>` + `theme()/set_theme()` · `ThemeFile`(TOML) · mocha/latte 임베드 · scan/load/apply/resolve/install · `first_run_init`/`ensure_mocha_exists` | `~/.tasty/themes/` |
| `tasty-settings::appearance` | `AppearanceSettings.{theme,theme_base,theme_overrides,theme_is_light,ui_scale}` | settings IO |

의존: `type-geometry ← type-appearance ← tasty-themes ← tasty-settings`. 순환 없음 — `tasty-core` 는 시각 schema 를 모른다(GUI-free).

## 빌트인 테마 정책

- **mocha**: 항상 존재 보장. 임베드 `MOCHA_TOML_TEXT` + `MOCHA_FALLBACK_COLORS` const. 부팅 시 `ensure_mocha_exists()` 가 누락/파싱 실패면 복구. 로드 실패해도 const 가 fallback. unit test 가 `parse(MOCHA_TOML_TEXT) == MOCHA_FALLBACK_COLORS` 강제.
- **latte**: first-run(themes 폴더가 완전히 빈 경우)에만 자동 풀림. 사용자가 지우면 존중하고 다시 풀지 않음(fallback 없음).
- **사용자 테마**: 로드 실패 시 mocha fallback.

## ThemeFile TOML

`~/.tasty/themes/<id>.toml`(stem = id). **모든 색상 필드 optional** — partial 정상 동작.

```toml
label = "표시 이름"   # 선택
is_light = false      # 선택. 없으면 이전 is_light 보존

[palette]   crust = "#11111b"   # ...
[accent]    blue = "#89b4fa"    # ...
[terminal]  selection_bg = "#585b70"   search_match_bg = "#f9e2af4d"  # 8자리 hex = alpha
[ansi]      black = "#45475a"   # 16 키: black..white + bright_*
[surfaces.terminal]   focused_bg = "#000000"  focused_fg = "#cdd6f4"  unfocused_bg = "#1e1e2e"  unfocused_fg = "#a6adc8"
```

- `[surfaces.<id>]` 의 `<id>` 는 자유 문자열 — plugin 이 등록한 surface kind 도 정의 가능. `theme().surface(id)` 가 없는 id 엔 `FALLBACK_SURFACE`(검은 배경 + Mocha 톤) 반환 → plugin 이 색을 안 줘도 안전.
- `hover_overlay`/`active_overlay`/`separator` 같은 반투명 의미색은 TOML 에 없다 — `is_light` 로부터 자동 도출(라이트=검정 +8%/+12%, 다크=흰색 +8%/+12%).
- UI 크기/간격(`spacing_*`/`border_width`/`item_height_*`/`font_size_*`)도 TOML 에 없다 — 모든 테마 공통 `SIZING` const.
- **HexColor**: `#RGB` / `#RRGGBB`(alpha=255) / `#RRGGBBAA`. 직렬화는 alpha=255 면 6자리, 아니면 8자리.

## 부팅 흐름 (`window_lifecycle.rs::boot_apply_theme`)

`first_run_init()`(빈 폴더면 mocha+latte) → `ensure_mocha_exists()` → `rescan()` → `apply_theme(요청 id)`(실패 시 mocha) → `install_global(resolve())`. 요청 id ≠ 적용 id 면 InfoModal 로 알림.

## UI 코드의 색상 접근

```rust
let th = crate::theme::theme();                       // = tasty_themes::theme()
ui.painter().rect_filled(rect, 0.0, th.blue);         // HexColor → Color32
let bg  = th.terminal_bg.to_float();                  // GPU 셰이더
let pad = th.spacing_sm;                               // sizing 동일 방식
// ❌ egui::Color32::from_rgb(80,140,255)             // 하드코딩 금지 (clippy 차단)
```

- **색 생성 경로 단일화**: GPU 버퍼 struct 는 newtype(`GpuRgba` 등)을 받아 `[f32;4]` 대입이 컴파일 에러. `from_rgb` 직접 호출은 clippy 차단. 상세 `dev-guide/color-policy` *(재작성 예정)*.
- **premultiplied 주의**: `hover_overlay`/`active_overlay`/`separator` 는 premultiplied 바이트라 `to_egui_premultiplied()` 를 써야 한다. `to_egui()` 를 쓰면 sRGB-aware premultiplication 이 한 번 더 적용돼 색이 어긋난다.
- **Semantic 접근자 우선**: 평면 primitive(`th.blue`) 외에 의미 기반 접근자(`accent_primary()`/`surface_raised()`/`text_muted()`)를 제공. 신규/수정 UI 는 의미가 드러나는 접근자를 우선(같은 primitive 가 여러 role 로 갈리는 다의성 표현). primitive 직접접근도 유효(additive, 픽셀 동일)하나 의미가 호출처에 묻힌다 — 전수 이식 전까지 clippy 강제는 보류. 매핑은 `token-crosswalk` *(재작성 예정)*.

## CSD 타이틀바 토큰

[윈도우 크롬](../../features/window-chrome/index.md)이 쓰는 전용 토큰. 색은 semantic 접근자 조합, 길이는 `ThemeSizing`.

| 접근자 | → semantic | 용도 |
|--------|-----------|------|
| `titlebar_bg()` / `titlebar_bg_inactive()` | `bg_app` / `bg_sidebar` | 타이틀바 배경(active/inactive) |
| `titlebar_border()` | `separator` | 하단 1px 보더 |
| `titlebar_fg()` / `titlebar_fg_inactive()` | `text_secondary` / `text_muted` | 전경(active/디밍) |
| `accent_window_close()` / `text_on_window_close()` | `#c42b1c` 리터럴 / white | close 버튼 hover — **테마 불변 OS 리터럴**(다크/라이트 동일, 테스트 고정) |

길이 토큰: `titlebar_height`(36px = `top_inset`) · `traffic_size`(12px, 예약) · `caption_width`(46px, Windows) · `window_button_size`(24px, Linux). 모두 **host UI zoom 제외**(OS 데코 관습상 고정 px).

## UI 디자인 규칙 (필수)

테마 위에 얹히는 **시각 정책**. 새 UI 를 그릴 때 따른다.

| 항목 | 규칙 |
|------|------|
| 기본 테마 | Mocha(fallback 보장) + Latte(first-run 자동) |
| 색 팔레트 | Catppuccin Mocha 톤 기준 |
| 간격 | **4px 그리드** — `spacing_xs/sm/md/lg` 만 |
| UI 폰트 최대 | **14px**(`font_size_body`) |
| 보더 | 항상 **1px**(`border_width`) |
| 포커스 링 | 2px accent-primary(`focus_ring_width`) |
| 호버 오버레이 | `hover_overlay`(라이트 검정 8% / 다크 흰색 8% 자동 도출) — 직접 값 금지 |
| 활성 오버레이 | `active_overlay`(12%) — 선택/active 행, hover(8%)와 구분 |
| 텍스트 대비 | 최소 **4.5:1**. 위반 시 `ai-verification/visual-verification` *(재작성 예정)* 체크리스트 |
| 터미널 콘텐츠 애니메이션 | **0ms** — 셀/스크롤엔 어떤 transition 도 금지(입력 응답성 우선) |
| UI 위젯 애니메이션 | 짧게(보통 100–150ms), 입력 직후 피드백 한정 |

코드에 하드코딩이 보이면 `Theme` 필드로 옮긴다. 새 시각 규칙은 이 표에 추가 후 `Theme` 에 필드 신설.

### Host UI zoom

`AppearanceSettings.ui_scale`(`small/medium/large` = `0.85/1.0/1.2`). `install_global_with_zoom` 이 sizing 토큰 자체에 배율을 곱해 전역 `Theme` 재빌드 — UI 코드는 곱셈 무지(`theme().spacing_*` 가 이미 zoomed).

- **zoom 받음**: `spacing_*` · `font_size_*` · `corner_radius` · `focus_ring_width` · `item_height_*` · 사이드바 sizing 토큰들.
- **zoom 제외**: `border_width`(1px 정책) · 탭바 토큰(`tab_width`/`tab_bar_*`) · CSD 타이틀바 토큰 · 터미널 콘텐츠 폰트(별도 `effective_terminal_font` 경로로 GPU 셰이더에 전달).
- **4px 그리드 + zoom**: 비정수(`12×1.2=14.4`)는 `round_ui()`/`f32::round()` 로 GPU 픽셀 정수 흡수.
- **라이브 갱신**: settings save / IPC update 시 `UiIntent::AppearanceChanged` 발화 → `cascade_appearance_changed` 가 전 윈도우 GpuState 에 broadcast(polling 아님, 변경 시 1회).

## 코드 위치

- schema: `crates/tasty-type-appearance/src/{color,theme}.rs`
- 전역/IO: `crates/tasty-themes/`(`theme()`, `apply_theme`, `resolve`, `install_global[_with_zoom]`, mocha/latte 임베드)
- settings: `crates/tasty-settings/src/appearance.rs`
- 부팅: `src/app/window_lifecycle.rs::boot_apply_theme`
</content>
