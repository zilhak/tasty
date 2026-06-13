# Tasty 테마 시스템

## 핵심 모델

```text
[ 디스크 ] ~/.tasty/themes/<id>.toml          ← partial TOML, 파일명 stem = id
                  │
                  ▼  (사용자가 settings 에서 테마 선택)
[ tasty-themes ] apply_theme(settings, id)
                  ├── settings.theme_base.apply_partial(file)   ← 누락 필드는 보존
                  └── settings.theme_overrides.clear()
                  │
                  ▼  (사용자가 픽커로 색 변경)
              theme_overrides.field = Some(new)
                  │
                  ▼
              resolve(settings) = theme_base ▷ theme_overrides
                                  + is_light 도출 overlay + SIZING
                  │
                  ▼
[ tasty-themes ] 전역 Theme 인스턴스 (한 개, RwLock)
                  │
                  ▼
              theme().crust / theme().spacing_sm / theme().is_light  ← UI 코드
```

## 두 레이어

`AppearanceSettings` 에 직렬화되는 두 색상 레이어:

| 레이어 | 타입 | 의미 | 테마 변경 시 |
|--------|------|------|--------------|
| `theme_base` | `ThemeColors` (풀 세트) | 지금까지 적용된 테마들의 누적 결과 | partial 덮어쓰기로 누적 |
| `theme_overrides` | `PartialColors` (모든 필드 `Option`) | 사용자가 픽커로 손댄 흔적 | **클리어** |

실제 화면에 적용되는 색상 = `theme_base ▷ theme_overrides` (override 의 `Some` 필드만 덮어쓰기).

### 누적의 효과

partial 테마(일부 색상만 정의)를 적용하면 누락된 필드는 이전 상태에서 유지된다.

예: mocha 적용 → custom(`accent.blue = #00ff00` 만 정의된 partial) 적용
- `theme_base.blue` = `#00ff00` 으로 갱신
- 그 외 모든 색상은 mocha 값 그대로

다음에 또 다른 partial `custom2(red = #ff0000)` 적용:
- `theme_base.blue` = `#00ff00` (유지)
- `theme_base.red` = `#ff0000` (덮어쓰기)
- 나머지는 mocha

## Crate 책임

색상/시각 schema 와 그 위의 도메인/IO 가 명확히 분리된다.

| crate | 책임 | IO |
|-------|------|----|
| `tasty-type-appearance::color` | `HexColor` (`#RRGGBB` / `#RRGGBBAA` 직렬화), `GpuRgba`/`GpuRgb` newtype | 없음 |
| `tasty-type-appearance::theme` | `Theme`, `ThemeColors`, `PartialColors`, `ThemeSizing`/`SIZING`, `SurfaceTheme`/`PartialSurfaceTheme` + `FALLBACK_SURFACE`, 인스턴스 메서드 (`with_colors`/`extract_colors`/`apply_colors`/`set_is_light`/`ansi_palette`/`apply_partial`/`Theme::surface(id)`), `derive_overlays` | 없음 |
| `tasty-themes` | `MOCHA_FALLBACK_COLORS`/`MOCHA_FALLBACK` const, 전역 `RwLock<Theme>` + `theme()/set_theme()/mutate_theme()`, `ThemeApplyContext` trait, `ThemeFile` (TOML 표면), `MOCHA_TOML_TEXT`/`LATTE_TOML_TEXT` 임베드, scan/load/apply/resolve/install, `ensure_mocha_exists`/`first_run_init` | **있음** (`~/.tasty/themes/`) |
| `tasty-settings::appearance` | `AppearanceSettings.theme / theme_base / theme_overrides / theme_is_light` 필드, `ThemeApplyContext` 구현 | settings IO |
| 본 바이너리 | `crate::theme::*` (= `tasty_themes::*`) 호출 (부팅, modal save, settings UI) | — |

의존:
```
tasty-type-geometry ← tasty-type-appearance ← tasty-themes ← tasty-settings
                                                       ↖
                                                         core (paths/i18n 만)
```
순환 없음. `tasty-core` 는 시각 schema 를 알지 않는다 (GUI-free).

## 빌트인 테마 정책

- **mocha**: 항상 존재 보장.
  - `tasty-themes::fallback` 에 `MOCHA_FALLBACK_COLORS: ThemeColors` const + `MOCHA_TOML_TEXT` (`include_str!`) 임베드.
  - 부팅 시 `ensure_mocha_exists()` 가 `~/.tasty/themes/mocha.toml` 없거나 파싱 실패면 임베드 텍스트를 다시 풀어둔다.
  - `load_theme("mocha")` 가 어떤 이유로든 실패해도 const 가 fallback.
  - unit test 가 `parse(MOCHA_TOML_TEXT) == MOCHA_FALLBACK_COLORS` 를 강제 — 임베드 텍스트와 const 가 어긋나면 빌드 시 실패.
- **latte**: first-run 1회만 자동 풀어둠.
  - `first_run_init()` 이 `~/.tasty/themes/` 가 완전히 비어있을 때만 `LATTE_TOML_TEXT` 도 같이 쓴다.
  - 사용자가 명시적으로 `latte.toml` 만 지웠다면 의도 존중하고 다시 풀지 않음.
  - fallback 없음. 지워지면 사라짐.
- **사용자 테마**: 로드 실패 시 mocha 로 fallback.

## ThemeFile TOML 포맷

`~/.tasty/themes/<id>.toml` (파일명 stem = id):

```toml
label = "표시할 이름"   # 선택. 없으면 id 그대로.
is_light = false        # 선택. 없으면 이전 is_light 보존.

[palette]
crust = "#11111b"
mantle = "#181825"
# ... (모든 색상 optional)

[accent]
blue = "#89b4fa"
# ...

[terminal]
selection_bg = "#585b70"
search_match_bg = "#f9e2af4d"          # 8자리 hex 로 alpha 지정 가능
search_match_active_bg = "#f9e2afb3"

[ansi]
black = "#45475a"
red = "#f38ba8"
# ... (16 키: black..white + bright_black..bright_white)

[surfaces.terminal]
focused_bg = "#000000"
focused_fg = "#cdd6f4"
unfocused_bg = "#1e1e2e"
unfocused_fg = "#a6adc8"

[surfaces.markdown]
focused_bg = "#000000"
focused_fg = "#cdd6f4"
unfocused_bg = "#181825"
unfocused_fg = "#a6adc8"
```

**모든 색상 필드는 optional**. 일부만 정의한 partial 테마도 정상 동작 — 누락 필드는 이전 base 의 값을 유지한다. `[surfaces.<id>]` sub-table 도 entry 단위 merge — 빌트인 mocha 의 `surface_themes` 위에 `[surfaces.terminal]` 의 일부 필드만 덮어쓸 수 있다.

### Plugin 의 surface kind 확장

`[surfaces.<id>]` 의 `<id>` 는 미리 정의된 enum 이 아니라 자유 문자열이다. plugin 이 자기 surface kind 를 등록하면 그 id 로 sub-table 을 정의할 수 있다. 예:

```toml
[surfaces.acme-chat]
focused_bg = "#0a0e14"
focused_fg = "#cdd6f4"
unfocused_bg = "#15191f"
unfocused_fg = "#a6adc8"
```

`theme().surface(id)` 가 해당 id 를 찾으면 그 SurfaceTheme 을, 못 찾으면 `FALLBACK_SURFACE` (검은 배경 + Mocha 톤 글자) 를 리턴한다. 즉 plugin 이 색을 정의하지 않아도 안전하게 동작한다.

`hover_overlay` / `active_overlay` / `separator` 같은 반투명 의미 색은 TOML 에 없다. `is_light` 로부터 자동 도출 (라이트 = 검정 +8%/+12%, 다크 = 흰색 +8%/+12%).

UI 크기/간격(`spacing_*`, `border_width`, `item_height_*`, `font_size_*` 등)도 TOML 에 없다. 모든 테마 공통 `tasty_type_appearance::theme::SIZING` const.

### HexColor 형식

- `#RGB` (3자리 단축) — `#abc` = `#aabbcc`
- `#RRGGBB` (6자리, alpha=255)
- `#RRGGBBAA` (8자리, alpha 보존)

leading `#` 은 optional. 직렬화는 alpha=255 면 6자리, 아니면 8자리.

## 부팅 흐름

`src/app/window_lifecycle.rs::boot_apply_theme()`:

1. `first_run_init()` — themes 폴더가 완전히 빈 상태였으면 mocha + latte 풀어둠.
2. `ensure_mocha_exists()` — mocha 누락/파싱 실패 시 임베드 텍스트로 복구.
3. `rescan()` — 디스크 스캔 결과를 캐시에 반영.
4. `apply_theme(&mut settings.appearance, &settings.appearance.theme)` — 요청 id 적용. 실패 시 mocha fallback.
5. `install_global(&settings.appearance)` — `resolve()` 결과를 전역 Theme 에 박는다.
6. 요청 id 와 적용된 id 가 다르면 InfoModal 로 사용자에게 알린다.

## 사용자 액션

### 테마 선택 (settings UI > Appearance > Theme)

`src/view/settings/ui/tabs/appearance.rs::draw_appearance_theme()`:
1. `tasty_themes::rescan()` — settings 화면 진입 시 디스크 변경 반영.
2. `scan_themes()` 결과를 `selectable_label` 로 나열.
3. 선택 시 `tasty_themes::apply_theme(&mut settings.appearance, &entry.id)` 호출.
4. settings 저장 시 (modal close 시) `install_global` 로 전역 Theme 갱신.

### 색상 픽커 편집 (현재: surface_colors 만)

surface 색 picker 는 v0.6.0 에서 제거됐다. theme TOML 의 `[surfaces.<id>]` sub-table 을 직접 편집하면 된다. 향후 palette/accent 전체에 대한 override picker 가 도입되면 surface 색도 함께 다뤄질 예정.

**theme_overrides 픽커는 Phase 1 범위 밖**. palette/accent 전 필드 픽커는 별도 후속.

## 색 생성 경로 강제 — newtype + clippy

색을 디자인하는 경로는 컴파일 단계 + lint 양쪽으로 단일화된다. GPU 버퍼
struct (`BgInstance.bg_color: GpuRgba` 등) 는 `tasty-type-appearance` 의 newtype 을
받으므로 `[f32; 4]` array literal 대입이 컴파일 에러. `HexColor::from_rgb` /
`egui::Color32::from_rgb*` 직접 호출도 clippy 로 차단된다.

상세는 [docs/dev-guide/color-policy.md](../../dev-guide/color-policy.md) 참고.

## UI 코드의 색상 접근 규칙

```rust
// ✅ 올바름 — 본 바이너리는 crate::theme 재수출을 통한다 (= tasty_themes::theme())
let th = crate::theme::theme();
ui.painter().rect_filled(rect, 0.0, th.blue);          // → Color32 (From<HexColor>)
let bg = th.terminal_bg.to_float();                    // GPU 셰이더용
ui.label(egui::RichText::new("x").color(th.text));
let pad = th.spacing_sm;                                // sizing 도 같은 방식
let light = th.is_light;                                // 플래그

// ❌ 금지
let color = egui::Color32::from_rgb(80, 140, 255);     // 하드코딩
```

`hover_overlay` / `active_overlay` / `separator` 만은 **premultiplied 바이트**로 저장되므로 변환 메서드를 골라 써야 한다:

```rust
// ✅ premultiplied 전용 변환
ui.painter().rect_filled(rect, 0.0, th.hover_overlay.to_egui_premultiplied());

// ❌ 일반 변환 쓰면 sRGB-aware premultiplication 이 한 번 더 적용되어 색이 어긋남
ui.painter().rect_filled(rect, 0.0, th.hover_overlay.to_egui());
```

## 새 테마 추가하기

사용자 입장에서:

1. `~/.tasty/themes/mocha.toml` 등을 복사해 `my-theme.toml` 로 이름 변경.
2. 원하는 색상 수정. 일부만 바꾸고 싶으면 다른 필드는 지워도 됨 — partial 적용된다.
3. Tasty 재시작 또는 Settings > Appearance > Theme 진입 (자동 rescan).
4. 목록에서 `my-theme` 선택.

빌트인 fallback 을 명시적으로 복원하려면 `~/.tasty/themes/mocha.toml` 을 지우고 재시작.

## UI 디자인 규칙 (필수)

테마 메커니즘 위에 얹히는 **시각 정책**. 모든 색상·크기·간격 값은 `Theme` 에서 가져오고, 아래 규칙은 새 UI 를 그릴 때 따라야 하는 기준이다.

| 항목 | 규칙 |
|------|------|
| 기본 빌트인 테마 | Mocha (fallback 보장) + Latte (first-run 자동 풀림) |
| 색상 팔레트 | Catppuccin Mocha 톤을 기준으로 함 |
| 모든 간격 | 4px 그리드 — `spacing_xs`/`sm`/`md`/`lg` 만 사용 |
| UI 폰트 최대 크기 | 14px (`font_size_body`) |
| 보더 두께 | 항상 1px (`border_width`) |
| 포커스 링 | 2px accent-primary outline (`focus_ring_width`, egui `selection.stroke` 에 적용) |
| 호버 오버레이 | `theme().hover_overlay` 사용 (라이트=검정 8%, 다크=흰색 8% 자동 도출). 직접 값 쓰지 말 것 |
| 활성 오버레이 | `theme().active_overlay` (12% overlay) — 선택/active 행에 사용, hover(8%) 와 구분 |
| 텍스트 대비 | 최소 4.5:1. 위반 시 [`ai-verification/visual-verification.md`](../../ai-verification/visual-verification.md) 의 색상 대비 체크리스트 적용 |
| 터미널 콘텐츠 애니메이션 | **0ms**. 터미널 셀/스크롤 영역엔 어떤 transition 도 넣지 않는다 — 입력 응답성 우선 |
| UI 위젯 애니메이션 | 짧게 (보통 100–150ms). 사용자 입력 직후의 시각 피드백에 한정 |
| Host UI zoom | `AppearanceSettings.ui_scale` (`small / medium / large` = `0.85 / 1.0 / 1.2`). `Theme::with_colors_and_zoom` 이 빌드 시점에 토큰에 직접 곱해 박는다. UI 코드는 곱셈 무지 — `theme().spacing_xs` 가 이미 zoomed 값. |

위 값 중 코드에 하드코딩으로 등장하는 곳이 발견되면 `Theme` 의 해당 필드로 옮긴다. 새 시각 규칙이 필요해지면 본 표에 추가한 뒤 `Theme` 에 필드를 신설한다.

### Host UI zoom 정책

`AppearanceSettings.ui_scale` 변경은 `tasty-themes::install_global_with_zoom` 로 전역 `Theme` 을 재빌드하여 sizing 토큰 자체에 zoom 배율을 곱한다. UI 코드는 `theme().spacing_*` / `font_size_*` 등을 그대로 호출하면 자동으로 zoom 영향을 받는다.

**zoom 영향 받는 토큰** — `spacing_xs/sm/md/lg/xl`, `font_size_caption/body/heading/max`, `corner_radius`, `focus_ring_width`, `item_height_tree/interactive/tab`, `sidebar_logo_size`, `sidebar_logo_collapsed_size`, `sidebar_wordmark_font_size`, `sidebar_section_heading_font_size`, `sidebar_button_label_font_size`, `sidebar_collapsed_slot_width`, `sidebar_collapsed_icon_height`, `sidebar_collapsed_workspace_height`.

**zoom 영향 받지 않는 토큰** —
- `border_width` (1px 보더 정책 — zoom 시 깨지면 안 됨)
- `tab_width`, `tab_bar_height`, `tab_bar_label_font_size`, `tab_bar_arrow_font_size` (탭바 zoom 제외 정책)
- 터미널 콘텐츠 폰트는 `effective_terminal_font` 별도 경로로 GPU 셰이더에 전달되므로 host UI zoom 과 무관

**14px 폰트 상한** — `font_size_max` 도 같이 곱셈만 적용 (cap 없음). zoom 변경은 사용자의 명시적 의도이므로 상한을 강제하지 않는다.

**4px 그리드** — zoom 적용 시 비정수 값 (예: `12 × 1.2 = 14.4`) 이 발생하면 `LogicalPx::round_ui()` 또는 `f32::round()` 로 GPU 픽셀 정수로 흡수한다. 그리드 자체는 zoom 배수의 그리드로 자연스럽게 어긋난다.

**라이브 갱신** — 설정 모달 Save / IPC settings update 등으로 `AppearanceSettings` 가 바뀌면 `UiIntent::AppearanceChanged` 가 발화되고, `App::cascade_appearance_changed` 가 모든 윈도우 (main + Settings/Plugins/Preset/Quit modal) 의 GpuState 에 broadcast 한다. polling 아님 — 변경 시점에 한 번만 발화.

### 호버 오버레이 변환 주의

`hover_overlay` / `active_overlay` / `separator` 는 **premultiplied 바이트**로 저장되므로 egui 에 넘길 때 변환 메서드를 골라야 한다:

```rust
// ✅ premultiplied 전용
ui.painter().rect_filled(rect, 0.0, th.hover_overlay.to_egui_premultiplied());

// ❌ 일반 변환을 쓰면 sRGB-aware premultiplication 이 한 번 더 적용되어 색이 어긋남
ui.painter().rect_filled(rect, 0.0, th.hover_overlay.to_egui());
```
