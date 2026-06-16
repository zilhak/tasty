# 테마 추가/관리

Tasty 의 색상 테마는 `~/.tasty/themes/` 의 TOML 파일로 정의된다. 파일 하나가 한 테마이고, **파일명 stem 이 곧 테마 id** 다. 폴더 위치는 고정 — 다른 경로는 인식하지 않는다.

## 빌트인

- `mocha.toml` — 기본 다크 테마 (Catppuccin Mocha). 항상 존재가 보장된다. 사용자가 지우거나 망가뜨려도 부팅 시 임베드 텍스트로 자동 복구된다.
- `latte.toml` — 라이트 테마 (Catppuccin Latte). first-run (themes 폴더가 완전히 빈 상태) 에만 자동으로 풀린다. 사용자가 지웠다면 의도 존중하고 다시 풀지 않는다.

빌트인을 복원하려면 해당 파일을 지우고 재시작 (mocha) 하거나 themes 폴더 전체를 비우고 재시작 (mocha + latte).

## TOML 포맷

```toml
label = "Nord"          # 선택. 없으면 파일명 그대로.
is_light = false        # 선택. 없으면 현재 is_light 유지.

[palette]
crust    = "#2e3440"
mantle   = "#3b4252"
base     = "#434c5e"
surface0 = "#4c566a"
surface1 = "#5e6779"
surface2 = "#6e7787"
overlay0 = "#7a8290"
overlay1 = "#88909e"
overlay2 = "#9098a6"
text     = "#eceff4"
subtext1 = "#d8dee9"
subtext0 = "#b8c0cc"
placeholder = "#7a8290"

[accent]
blue      = "#81a1c1"
green     = "#a3be8c"
red       = "#bf616a"
yellow    = "#ebcb8b"
peach     = "#d08770"
mauve     = "#b48ead"
teal      = "#8fbcbb"
sky       = "#88c0d0"
lavender  = "#5e81ac"
flamingo  = "#d08770"
pink      = "#b48ead"
maroon    = "#bf616a"
rosewater = "#d08770"

[terminal]
fg = "#eceff4"
bg = "#2e3440"
selection_bg = "#4c566a"
search_match_bg = "#ebcb8b4d"          # 8자리 hex → alpha=0x4d (≈30%)
search_match_active_bg = "#ebcb8bb3"   # alpha=0xb3 (≈70%)

[ansi]
black   = "#3b4252"
red     = "#bf616a"
green   = "#a3be8c"
yellow  = "#ebcb8b"
blue    = "#81a1c1"
magenta = "#b48ead"
cyan    = "#88c0d0"
white   = "#e5e9f0"
bright_black   = "#4c566a"
bright_red     = "#bf616a"
bright_green   = "#a3be8c"
bright_yellow  = "#ebcb8b"
bright_blue    = "#81a1c1"
bright_magenta = "#b48ead"
bright_cyan    = "#8fbcbb"
bright_white   = "#eceff4"
```

### 모든 필드는 optional

테마 파일에 **일부 색상만 정의**하면 누락된 필드는 이전 적용된 테마의 값을 유지한다.

예: `tweak.toml` 이 `accent.blue` 만 정의하고 다른 모든 필드가 없다면:

```toml
label = "Tweak Blue"
[accent]
blue = "#00ff00"
```

mocha → tweak 적용 시 → `blue` 만 `#00ff00`, 나머지는 mocha 값 그대로.

### HexColor 형식

- `#RGB` (3자리 shorthand) — `#abc` 는 `#aabbcc` 와 동일.
- `#RRGGBB` (6자리, alpha=255).
- `#RRGGBBAA` (8자리, alpha 직접 지정).

leading `#` 은 optional. `terminal.search_match_bg` 처럼 반투명이 필요한 필드에서 8자리 hex 를 쓴다.

### 자동 도출 필드

다음 색상은 TOML 에 정의하지 않는다 — `is_light` 에서 자동으로 만들어진다:

- `hover_overlay` (~8% 오버레이)
- `active_overlay` (~12% 오버레이)
- `separator` (~8% 구분선)

`is_light = true` 면 검정 기반, `false` 면 흰색 기반.

### UI 크기/간격

`spacing_*`, `border_width`, `font_size_*`, `item_height_*`, `corner_radius`, `tab_width` 같은 UI 크기/간격은 테마에서 정의할 수 없다. 모든 테마가 4px 그리드 기반 공통값을 쓴다.

## 적용 흐름

테마를 변경하면:

1. `theme_base` 에 새 파일의 partial 이 누적 적용 (누락 필드는 보존).
2. `theme_overrides` (사용자가 픽커로 손댄 색) **클리어**.
3. 전역 Theme 인스턴스 갱신.
4. `~/.tasty/config.toml` 에 위 변경 저장.

즉 테마를 바꾸면 직전 픽커 편집 흔적은 사라진다. 픽커 편집은 "현재 테마 위에 덧칠" 의미라, 테마가 바뀌면 의미를 잃기 때문.

## 로드 실패 처리

- 잘못된 hex, 유효하지 않은 TOML 등 — 그 파일은 `tracing::warn!` 로 로그 남기고 스캔에서 스킵.
- 적용 요청 id 가 디스크에 없거나 깨졌으면 자동으로 `mocha` 로 fallback. InfoModal 로 사용자에게 알림.
- `mocha.toml` 자체가 깨졌으면 임베드 텍스트로 즉시 자동 복구.

## 파일 위치 정리

| 경로 | 의미 |
|------|------|
| `~/.tasty/themes/mocha.toml` | 빌트인 mocha (자동 복구) |
| `~/.tasty/themes/latte.toml` | 빌트인 latte (first-run 1회 자동) |
| `~/.tasty/themes/<id>.toml` | 사용자 테마 |
| `~/.tasty/config.toml` 의 `[appearance]` | `theme = "<id>"`, `theme_base` (풀세트), `theme_overrides` (픽 흔적), `theme_is_light` |
