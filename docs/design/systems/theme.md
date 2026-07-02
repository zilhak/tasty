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
| `theme_base` | `ThemeColors`(풀 세트) | 적용된 테마 파일의 누적 결과 (**앱 소유**) | partial 덮어쓰기로 누적 |
| `theme_overrides` | `PartialColors`(모든 필드 `Option`) | 사용자가 손댄 색 (**사용자 커스터마이징의 단일 출처**) | **클리어(의도된 설계)** |

화면 적용 색 = `theme_base ▷ theme_overrides`(override 의 `Some` 필드만 덮어쓰기). partial 테마(일부 색만 정의)를 적용하면 누락 필드는 이전 base 값을 유지한다(누적).

#### 커스터마이징 모델 (불변)

- **`theme_base` 는 앱 소유다.** 테마 파일(`<id>.toml`)의 내용은 앱이 관리하며, 사용자가 직접 편집하는 경로가 아니다(빌트인은 부팅 시 임베드 정본으로 동기화됨 → "빌트인 테마 정책" 참고).
- **사용자 색 변경은 오직 `theme_overrides` 로만 들어간다.** settings 가 보관하는 partial 레이어로, base 위에 resolve 시점에 얹힌다. base(파일)를 어떻게 바꾸거나 동기화해도 사용자 override 는 보존된다 — 두 레이어가 분리돼 충돌이 없다.
- **override 를 기록하는 정식 경로 = Settings › Appearance › Colors picker.** 픽커는 flat `PartialColors` 46색(Surfaces·Overlays·Text·Accents·Terminal-specific·ANSI 16) 을 그룹별 collapsible 로 노출한다. 각 행의 "Default" 체크 = 그 필드 `None`(프리셋 base 추종), 해제 = `Some(hex)`. base 값은 resolved `theme_base` 에서 읽어 시드한다(하드코딩 없음). 행/그룹/전체 3단계 reset 으로 `None` 복귀. 저장 시 `theme_overrides` 변화가 감지되면 `AppearanceChanged` 가 발화돼 전 윈도우에 라이브 반영된다. `surface_themes`(맵 구조)는 이 flat 픽커에서 분리돼 `Tasty`/`Terminal` 섹션의 curated shortcut 으로 남되 같은 `theme_overrides` 에 기록된다.
- **테마를 바꾸면 `theme_overrides` 를 비운다(설계).** `apply_theme` 의 `theme_overrides.clear()` 는 부수효과가 아니라 의도다 — 테마 전환 = 그 테마의 색을 깨끗하게 적용하고 이전 테마에 얹어둔 사용자 변경분은 폐기한다. 픽커가 채운 override 도 함께 비워진다.

### Crate 책임

| crate | 책임 | IO |
|-------|------|----|
| `tasty-type-appearance::color` | `HexColor`, `GpuRgba`/`GpuRgb` newtype | 없음 |
| `tasty-type-appearance::theme` | `Theme` · `ThemeColors` · `PartialColors` · `ThemeSizing`/`SIZING` · `SurfaceTheme`/`FALLBACK_SURFACE` · `derive_overlays` · `Theme::surface(id)` | 없음 |
| `tasty-themes` | 전역 `RwLock<Theme>` + `theme()/set_theme()` · `ThemeFile`(TOML) · mocha/latte 임베드 · scan/load/apply/resolve/install · `first_run_init`/`sync_builtin_themes` | `~/.tasty/themes/` |
| `tasty-settings::appearance` | `AppearanceSettings.{theme,theme_base,theme_overrides,theme_is_light,ui_scale}` | settings IO |
| `tasty-design-tokens` | 디자인 DTCG export vendor(`dtcg/tasty.tokens.json`, 488 토큰) + 치수 const 생성(`src/generated/` — primitive 는 `pub(crate)` 로 3-tier 규율 강제) + **component tier 접근자 생성**(`tasty-type-appearance/src/generated_component.rs` 로 산출 — `&Theme` 경유 치수·색 접근자, 아래 "Component tier 접근자") + freshness/`SIZING` 정합/mocha·latte 색 드리프트 가드 테스트. 생성 const 는 초기값·정합용 — 런타임 소비는 `&Theme` 경유(zoom 우회 금지). vendor 갱신 절차는 crate README | 없음 |

의존: `type-geometry ← type-appearance ← tasty-themes ← tasty-settings`. 순환 없음 — `tasty-core` 는 시각 schema 를 모른다(GUI-free). `tasty-design-tokens` 는 `type-geometry` 만 런타임 의존(정합 테스트만 dev-deps 로 type-appearance/themes 참조) — 본체·egui 미의존.

## 빌트인 테마 정책

빌트인 테마 파일은 **앱 소유**다. 부팅 시 `sync_builtin_themes()` 가 디스크 복사본을 임베드 정본과 맞춘다 — 빌트인 색/스키마가 바뀌면 이미 풀려있던 옛 파일도 자동 갱신된다. 사용자 색 변경은 파일이 아니라 `theme_overrides` 에 있으므로 동기화가 사용자 커스터마이징을 덮어쓰지 않는다.

- **mocha**: 항상 정본 보장. 임베드 `MOCHA_TOML_TEXT` + `MOCHA_FALLBACK_COLORS` const. 부팅 시 sync 가 누락/파싱 실패/**내용 불일치** 면 임베드로 덮어쓴다. 로드 실패해도 const 가 fallback. unit test 가 `parse(MOCHA_TOML_TEXT) == MOCHA_FALLBACK_COLORS` 강제.
- **latte**: first-run(themes 폴더가 완전히 빈 경우)에 자동 풀림. 이후엔 **파일이 있으면 임베드와 동기화**, 사용자가 지우면 존중하고 다시 풀지 않음(fallback 없음).
- **사용자 테마**: 자동 동기화/복구 없음. 로드 실패 시 mocha fallback.
- **"사용자 의도 존중" 의 범위 = 파일 삭제(부재)뿐.** 빌트인 파일의 *내용* 은 존중 대상이 아니다 — 사용자가 손으로 고쳐도 다음 부팅에 정본으로 되돌아간다(편집 경로가 아님, 커스터마이징은 `theme_overrides`).

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
- **빌트인(`mocha`/`latte`) 파일은 직접 편집하지 말 것** — 앱 소유라 부팅 시 임베드 정본으로 되돌아간다. 커스텀 테마는 별도 id(예: `my-theme.toml`)로 만들고, 기존 테마 위 색 조정은 settings 의 `theme_overrides` 로 한다.

## 부팅 흐름 (`window_lifecycle.rs::boot_apply_theme`)

`first_run_init()`(빈 폴더면 mocha+latte 시드) → `sync_builtin_themes()`(빌트인을 임베드 정본과 동기화) → `rescan()` → `apply_theme(요청 id)`(실패 시 mocha) → `install_global(resolve())`. 요청 id ≠ 적용 id 면 InfoModal 로 알림.

## UI 코드의 색상 접근

```rust
let th = crate::theme::theme();                       // = tasty_themes::theme()
ui.painter().rect_filled(rect, 0.0, th.blue);         // HexColor → Color32
let bg  = th.terminal_bg.to_float();                  // GPU 셰이더
let pad = th.spacing_sm;                               // sizing 동일 방식
// ❌ egui::Color32::from_rgb(80,140,255)             // 하드코딩 금지 (clippy 차단)
```

- **색 생성 경로 단일화**: GPU 버퍼 struct 는 newtype(`GpuRgba` 등)을 받아 `[f32;4]` 대입이 컴파일 에러. `from_rgb` 직접 호출은 clippy 차단. 상세 [`dev-guide/color-policy`](../../dev-guide/color-policy.md).
- **premultiplied 주의**: `hover_overlay`/`active_overlay`/`separator` 는 premultiplied 바이트라 `to_egui_premultiplied()` 를 써야 한다. `to_egui()` 를 쓰면 sRGB-aware premultiplication 이 한 번 더 적용돼 색이 어긋난다.
- **Semantic 접근자 우선**: 평면 primitive(`th.blue`) 외에 의미 기반 접근자(`accent_primary()`/`surface_raised()`/`text_muted()`)를 제공. 신규/수정 UI 는 의미가 드러나는 접근자를 우선(같은 primitive 가 여러 role 로 갈리는 다의성 표현). primitive 직접접근도 유효(additive, 픽셀 동일)하나 의미가 호출처에 묻힌다 — 전수 이식 전까지 clippy 강제는 보류. 매핑·다의성 핫스팟은 [`token-crosswalk`](token-crosswalk.md).

## Component tier 접근자

DTCG component tier(치수+색) 토큰은 `crates/tasty-type-appearance/src/generated_component.rs` 의 **생성된 `&Theme` 메서드**로 노출된다 (`tasty-design-tokens` 생성기가 산출, `DO NOT EDIT`). `generated::component` 의 raw const 를 위젯이 직접 읽으면 `with_colors_and_zoom` 의 zoom resolve/제외 정책을 우회하므로, 위젯은 **반드시 이 접근자를 경유**한다.

- **치수 접근자**(→ `LogicalPx`) 3형태: alias 체인이 (a) zoom 정책이 이미 박힌 `Theme` 필드에 닿으면 그 필드 반환, (b) 다른 component 접근자에 닿으면 그 접근자 호출, (c) primitive 에 직접 닿으면 `Theme.ui_zoom` 을 곱해 계산(`LogicalPx((v*ui_zoom).round())`). 예: `button_gap()`=`spacing_sm`, `button_height_lg()`=`(32*ui_zoom).round()`.
- **색 접근자**(→ `HexColor`): semantic 접근자 체인 또는 component→component 상호 호출. 예: `button_primary_bg()`=`accent_primary()`. `banner_*`/`titlebar_*` 색은 기존 수기 접근자와 이름 충돌이라 생성 제외(수기 유지).
- **소비처**: `tasty-ui-widgets` 위젯(button/chip/input/menu_item/select/toggle/tree_row/icon_button/status_dot/table 등)이 이 접근자를 소비. host chrome(sidebar/titlebar, `src/adapters/ui/`)은 후속 시리즈에서 전환.
- **zoom 회귀/값불변 테스트**: `tasty-type-appearance` 에 zoom 1.0 값 불변(이식 전후 동일)·zoom 1.5 스케일 단위 테스트 존재.

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

테마 위에 얹히는 **시각 정책**(=토큰 축). 새 UI 를 그릴 때 따른다. 디자인을 구현에 정합시키는
작업이라면 이 토큰 축만으로 끝나지 않는다 — 레이아웃을 컴포넌트·소스 단위로 1:1 전사하는 **구조
축**([design-parity-notes.md](design-parity-notes.md) · [design-gallery-mapping.md](design-gallery-mapping.md))을
함께 충족한다(두 축은 독립적으로 어긋날 수 있다).

| 항목 | 규칙 |
|------|------|
| 기본 테마 | Mocha(fallback 보장) + Latte(first-run 자동) |
| 색 팔레트 | Catppuccin Mocha 톤 기준 |
| 간격 | **4px 그리드** — `spacing_xs/sm/md/lg/xl` 만 |
| 간격 API | **`add_space`/`inner_margin`/`Margin::same|symmetric` 에 숫자 리터럴 직접 전달 금지** — `tasty-ui-widgets` 의 typed 헬퍼(`vspace`/`hspace`/`margin_all`/`margin_sym`)에 `th.spacing_*`(LogicalPx)를 넘긴다. 그리드 밖 미세 구조 간격(1/2/3px)은 `tasty_ui_widgets::tokens::STRUCT_GAP_1/2/3` (DTCG `primitive.size-1/2/3` 대응). 시리즈 03 에서 lint/guard 게이트로 강제 예정 |
| UI 폰트 최대 | **14px**(`font_size_body`) |
| 보더 | 항상 **1px**(`border_width`) |
| 포커스 링 | 2px accent-primary(`focus_ring_width`) |
| 호버 오버레이 | `hover_overlay`(라이트 검정 8% / 다크 흰색 8% 자동 도출) — 직접 값 금지 |
| 활성 오버레이 | `active_overlay`(12%) — 선택/active 행, hover(8%)와 구분 |
| 텍스트 대비 | 최소 **4.5:1**. 위반 시 [`ai-verification/visual-verification`](../../ai-verification/visual-verification.md) 체크리스트 |
| 터미널 콘텐츠 애니메이션 | **0ms** — 셀/스크롤엔 어떤 transition 도 금지(입력 응답성 우선) |
| UI 위젯 애니메이션 | 짧게(보통 100–150ms), 입력 직후 피드백 한정 |

코드에 하드코딩이 보이면 `Theme` 필드로 옮긴다. 새 시각 규칙은 이 표에 추가 후 `Theme` 에 필드 신설.

표·드롭다운·버튼처럼 이름이 곧 정체성인 보편 컴포넌트는 인라인으로 그리지 말고 공용 위젯으로 추출한다 — [공용 위젯 제작 정책](../policies/shared-widgets.md).

### Host UI zoom

`AppearanceSettings.ui_scale`(`small/medium/large` = `0.85/1.0/1.2`). `install_global_with_zoom` 이 sizing 토큰 자체에 배율을 곱해 전역 `Theme` 재빌드 — UI 코드는 곱셈 무지(`theme().spacing_*` 가 이미 zoomed).

- **zoom 받음**: `spacing_*` · `font_size_*` · `corner_radius` · `focus_ring_width` · `item_height_*` · 사이드바 sizing 토큰들.
- **zoom 제외**: `border_width`(1px 정책) · 탭바 토큰(`tab_width`/`tab_bar_*`) · CSD 타이틀바 토큰 · 터미널 콘텐츠 폰트(별도 `effective_terminal_font` 경로로 GPU 셰이더에 전달).
- **4px 그리드 + zoom**: 비정수(`12×1.2=14.4`)는 `round_ui()`/`f32::round()` 로 GPU 픽셀 정수 흡수.
- **라이브 갱신**: settings save / IPC update 시 `UiIntent::AppearanceChanged` 발화 → `cascade_appearance_changed` 가 전 윈도우 GpuState 에 broadcast(polling 아님, 변경 시 1회).
- **불변식 — `set_theme`/`install_global*` 은 렌더 밖에서만**: 전역 `THEME` 는 std `RwLock`(재진입 불가)이라, egui 렌더 클로저는 `theme()`(=`THEME.read()`) read guard 를 보유한다. 렌더 도중 `set_theme`(=`THEME.write()`)을 호출하면 자기 read guard 때문에 self-deadlock 으로 hang 한다. 따라서 테마 install 은 항상 인텐트 dispatch(`about_to_wait` / cascade) 단계에서만 수행하고, 렌더 핸들러(설정 모달 Save 등)는 `UpdateSettings` 인텐트만 큐잉한다(install 직접 호출 금지).

## 코드 위치

- schema: `crates/tasty-type-appearance/src/{color,theme}.rs`
- 전역/IO: `crates/tasty-themes/`(`theme()`, `apply_theme`, `resolve`, `install_global[_with_zoom]`, mocha/latte 임베드)
- settings: `crates/tasty-settings/src/appearance.rs`
- 부팅: `src/app/window_lifecycle.rs::boot_apply_theme`
</content>
