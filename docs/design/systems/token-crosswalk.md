# 토큰 크로스워크 (DTCG ↔ Rust Theme)

디자인 시스템의 DTCG 토큰과 Rust `Theme` 필드, 그리고 실제 `th.*`/`theme.*` 호출처를 잇는 매핑 참조. [theme.md](theme.md) 의 토큰 구조를 호출처 관점에서 보충한다.

> **범위 한정**: DTCG 토큰 파일(`tokens/tasty.tokens.json`)은 **claude design 산출물로 아직 repo 에 vendor 되지 않았다**(→ [theme.md](theme.md) "디자인 시스템 vendor"). 따라서 *DTCG primitive/semantic/component 전수 매핑*(96 semantic × Rust 필드)은 design-system 이 vendor 된 뒤에 채운다. 이 문서는 **Rust 측에서 지금 검증 가능한 부분** — 구조 모델과 다의성 핫스팟 — 만 확정한다.

## 구조 모델

```
DTCG:  primitive ─▶ semantic ─▶ component ─▶ UI        (3-tier)
Rust:  ThemeColors(평면 primitive) ─▶ Theme(펼친 필드 + 도출 overlay) ─▶ UI(th.<field>)
                    ▲ semantic / component tier 없음 — 의미를 호출처가 암묵적으로 들고 있다
```

- **Rust 에는 semantic·component tier 가 없다.** `ThemeColors`(`crates/tasty-type-appearance/src/theme.rs`)는 catppuccin 평면 primitive(neutral ramp 12 + accent hue 13 + 터미널 색 4 + ansi 16 + `surface_themes` map)만 노출한다.
- 그래서 *"이 primitive 가 지금 어떤 의미(role)로 쓰이나"* 는 코드 호출처가 들고 있다 — 같은 필드가 여러 role 로 갈린다(아래 핫스팟).
- 반투명 의미색(`hover_overlay`/`active_overlay`/`separator`)만 `is_light` 에서 **도출**된다(`derive_overlays`), primitive 가 아니다.

## 다의성 핫스팟 (Rust 필드 → 겹치는 role)

한 primitive 필드가 여러 의미 role 을 겸한다. 의미 기반 접근자([theme.md](theme.md) "Semantic 접근자 우선")로 옮길 때, 호출처별로 어느 role 인지 가려야 하는 지점이다. (필드명은 `theme.rs`, 실제 색값은 `crates/tasty-themes/src/fallback.rs` 가 출처. 현재 호출처는 `rg '\bth\.<field>\b'` 로 확인.)

| Rust 필드 | 겹치는 role | 갈래 판단 포인트 |
|-----------|-------------|------------------|
| `blue` | accent-primary · border-focus · ansi-blue | selection·hyperlink=primary, focus ring stroke=border-focus, 터미널 팔레트=ansi |
| `yellow` | accent-warning · ansi-yellow · search-match | 경고=warning, 검색 하이라이트=search-match, 팔레트=ansi |
| `red` | accent-danger · ansi-red | error/danger 버튼=danger, 팔레트=ansi |
| `green` | accent-success · ansi-green | 성공 표시=success, 팔레트=ansi |
| `mauve` | accent-agent · ansi-magenta | 에이전트 강조=agent, 팔레트=ansi |
| `surface0` | surface-raised · border-default | 채움 배경 vs 1px 선 |
| `surface1` | surface-hover · border-strong · ansi-black | hover 배경 vs 강조 선 vs 팔레트 |
| `surface2` | surface-active · selection-bg | active 배경 vs 터미널 선택 (동일값) |
| `subtext0` | text-muted (+ caption 혼용) | muted 본문 vs 보조/caption 라벨 (최다 호출) |
| `overlay1` | text-disabled (+ recording 강조) | 비활성 텍스트 vs keybinding 녹화 강조 |
| `text` | text-primary · ansi-bright-white | UI 본문 vs 팔레트 |
| `subtext1` | text-secondary · ansi-white | 보조 텍스트 vs 팔레트 |

## ANSI 팔레트는 배열로 한 번에 전달

ANSI 16색은 개별 `th.*` 호출이 아니라 `theme.ansi_palette()` 배열로 GPU 렌더러(`src/gfx/gpu/render_pass.rs`)에 한 번에 넘어간다. 다수가 neutral/accent 필드와 **동일값**이지만 별도 필드다(`ansi_black`=`surface1`, `ansi_blue`=`blue` 등 — 위 핫스팟의 "ansi-*" role).

## surface kind 색은 `surface_themes` map

터미널/마크다운의 focused/unfocused × bg/fg 색은 `ThemeColors.surface_themes: BTreeMap<String, SurfaceTheme>` 에 들어가 `theme.surface("terminal")` / `theme.surface("markdown")` 헬퍼로 읽는다. `focused_bg` 만 black/white role-remap(light/dark).

## vendor 후 채울 것

design-system 이 repo 에 vendor 되면 이 문서에 추가한다:

- DTCG semantic 토큰 ↔ Rust 필드 전수표 (Rust 미대응 토큰 명시: `text-on-accent`/`radius-sm`/`radius-pill`/`font-*`/`line-height-*`/`motion-*`/`ui-scale-*`/`brand-*` 등은 현재 Theme 필드 부재).
- component tier(버튼/입력/탭/토스트…) ↔ 호출처 매핑.

## 관련

- [theme.md](theme.md) — Theme 2계층 모델 + UI 디자인 규칙
- 코드: `crates/tasty-type-appearance/src/theme.rs` (필드) · `crates/tasty-themes/src/fallback.rs` (mocha 색값)
