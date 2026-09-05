# Design Parity 히스토리 — 디자인(html/CSS) ↔ 구현(winit/egui) 구조적 차이

`design-parity` 스킬이 발동 시 먼저 읽지만, **UI/갤러리를 디자인에 정합시키는 모든 작업에서**
(스킬을 명시적으로 부르지 않아도) 참조하는 구조 정합 원칙·함정 노트다. **검증으로 확인된
사실만** 적는다(추정이면 명시). 같은 함정을 두 번 파지 않기 위함. 형식: 증상 / 원인 / 처방 / 근거.

---

## 구조 전사 (structural transcription) — 핵심 원칙

디자인을 구현에 정합시키는 1차 작업은 **레이아웃 구조의 1:1 전사**다: 디자인의 grid·컬럼
정의·패딩·정렬·요소 경계를 egui 소스 구조에 그대로 옮긴다. egui flow(자동 spacing·기본 정렬·
auto-shrink)로 결과만 비슷하게 **눈대중하지 않는다.** 색·간격·치수 같은 **토큰 정합과는 별개
축**이며, 토큰이 맞아도 구조가 어긋나면 specimen·본체가 드리프트한다([gallery-first](../../dev-guide/gallery-first.md)
1 단계의 두 축). 아래 함정 노트들은 이 전사 과정에서 egui 가 디자인 구조를 왜곡하는 구체
사례와 처방이다 — 새 함정을 만나면 같은 형식으로 추가한다.

> **두 축은 함께 충족해야 한다.** 이 문서(구조 축)만 따르고 토큰 축을 빠뜨리면 정합이 절반만
> 된다. **토큰 축 규칙은 반드시 [theme.md "UI 디자인 규칙"](theme.md#ui-디자인-규칙-필수)을 함께
> 본다** — 색·폰트크기·선굵기·간격은 전부 `Theme`(디자인 토큰 매핑)에서 가져오고 raw px·`from_rgb`
> 하드코딩 금지, 4px 그리드·14px 폰트 상한·1px 보더. 구조를 1:1 전사하면서 새로 넣는 모든 치수·색은
> 토큰에서 끌어온다.

---

## 일반 — egui `item_spacing` 자동 삽입이 divider·gap 을 밀어낸다

- **증상**: 구역 사이/위젯 사이에 의도치 않은 간격이 생겨 divider 위치가 디자인보다 밀린다.
- **원인**: egui 는 위젯마다 `item_spacing`(기본 ~6px)을 자동 삽입한다. CSS 와 달리 "내 값 +
  egui 기본값 = 결과".
- **처방**: 구역 배치는 `item_spacing.y = 0` 으로 죽이고 간격은 명시 `add_space`/Frame
  inner_margin 으로만. divider 는 add_space 뒤 커서가 아니라 구역 Frame 의 실제 `rect.bottom()`
  좌표에 그린다.
- **주의(회귀)**: `item_spacing` 을 vec2(0,0) 으로 통째 죽이면 **콘텐츠 행 내부 가로 gap 까지
  사라진다**(텍스트가 다 붙음). y 만 0 으로 하거나, 콘텐츠 영역 진입 시 원래 spacing 을 복원할 것.
- **근거**: remote_tool 2026-06-20. 최상위 vec2(0,0) → "gb10...ssh", "passkey:—shell:bash"
  처럼 행 내부가 붙음. y 만 0 + 콘텐츠에서 saved_spacing 복원으로 해결.

## 일반 — 픽셀 검증 시 터미널 unfocused 배경이 popup `base` 와 동색

- **증상**: 스크린샷에서 popup 경계를 배경색 전환으로 찾으려 하면 실패한다.
- **원인**: 터미널 surface 의 unfocused_bg = `base`(#1e1e2e) = 패널형 popup 배경과 같은 색.
  또 `bg-sidebar`(mantle) 는 tasty 사이드바와도 동색이라 가로 mantle run 에 사이드바가 섞인다.
- **처방**: popup 경계/구역을 **고유 색 랜드마크**로 잡는다. remote_tool 은 탭바 `mantle`
  띠가 터미널엔 없으므로 그걸 기준점으로. 사이드바 혼입은 x 범위 필터(x>700 등)로 배제.
- **근거**: remote_tool 2026-06-20 검증.

## 일반 — ui_scale(zoom)이 popup default_size 에만 곱해진다 (비균일)

- **증상**: ui_scale=large(1.2) 에서 popup 은 1.2배 커지는데 내부 하드코딩 px(탭 높이 등)는
  안 커져, 디자인 대비 내부 요소가 작아 보인다(탭바 35→측정 28.8).
- **원인**: `PopupManager::register_def` 가 `default_size * ui_zoom` 만 적용. draw_fn 내부의
  logical px 는 zoom 곱이 없다. → zoom≠1.0 이면 popup 과 내부의 비율이 깨진다.
- **처방**: **디자인 픽셀 검증은 ui_scale=medium(1.0) 에서 한다**(config.toml
  `appearance.ui_scale="medium"` 임시 변경 → 검증 → 복원). 1.0 이면 popup·내부 모두 device
  scale 만 적용돼 디자인과 1:1.
- **근거**: remote_tool 2026-06-20. large(scale 2.4) → medium(scale 2.0) 전환 후 측정이
  디자인과 일치.
- **추정**: popup 도 egui ctx zoom 으로 균일 처리하면 근본 해결이나, 현재 구조는 default_size
  곱 방식 — 별도 과제.

## remote_tool — CSS line-height vs egui 텍스트 박스 높이 (헤더 8px 얕음)

- **증상**: 헤더가 디자인보다 ~8px(logical) 얕아 divider Y 가 위로 밀린다(40 vs 48).
- **원인**: 디자인 헤더 콘텐츠 높이는 title `fontSize:14` 의 line-height(~24)가 결정. egui 의
  label/icon 텍스트 박스는 더 낮다(~18). 같은 폰트 크기여도 박스 높이가 다르다.
- **처방**: 헤더 행에 `ui.set_min_height(<디자인 콘텐츠 높이>)` 로 높이를 강제. remote_tool 은
  26 (디자인 24 + popup border Outside 보정 2)에서 divider Y 48.0 일치.
- **근거**: remote_tool 2026-06-20. min_height 26 + tab_h 36 → header divider 48.0/tab
  divider 84.0 (diff 0).

## remote_tool — popup border 가 stroke Outside → 콘텐츠가 1px 위에서 시작

- **증상**: 구역 Y 좌표가 디자인보다 1~2px 일정하게 위에 있다.
- **원인**: `draw.rs` 가 popup 외곽선을 `StrokeKind::Outside` 로 그린다 → 콘텐츠(content_rect)
  는 popup_rect.top 부터이고, 측정한 popup border 는 그 1px 바깥. 디자인은 border 가 컨테이너
  안쪽(box-sizing border-box).
- **처방**: 콘텐츠 시작 좌표 계산 시 +1~2px 보정(또는 min_height 등에 흡수). 정밀(±1px) 단계
  에서만 신경 쓰면 된다.
- **근거**: remote_tool 2026-06-20.

## command_palette — surface0 배경은 popup 밖에도 쓰여 색 bbox 가 오염된다

- **증상**: surface0(=`surface-raised`) 픽셀의 bounding box 로 popup 을 잡으면 화면 우측 끝까지
  잡혀 폭이 틀린다(scale 오검출).
- **원인**: command_palette 본문 bg 는 surface0 인데, 같은 surface0 가 비활성 탭·hover
  오버레이·스크롤바 등 **popup 밖 여러 위젯**에도 쓰인다. 단색 bbox 로는 popup 만 못 가린다.
- **처방**: popup 의 **전체폭 가로 divider(surf1 line)** 를 랜드마크로 쓴다. x 범위 안에서
  surf1 픽셀이 일정 수 이상인 행 = search/footer divider + popup top/bottom border. 그 행들의
  surf1 run 으로 popup 좌우, 행 Y 로 구역 경계를 잡는다.
- **근거**: command_palette 2026-06-20. wide-surf1 행 [top, search_div, footer_div, bottom]
  → search divider 49.6(design 50), footer h 31.3(design 31). diff <1.

## command_palette — height 가변(콘텐츠 맞춤) vs tasty 고정 popup

- **증상**: 디자인은 항목 수에 따라 카드 높이가 변하고 footer 가 list 바로 아래 붙는다. tasty
  popup 은 default_size 고정이라 빈 공간/잘림이 생긴다.
- **처방**: command_palette 는 실사용에서 거의 항상 항목이 많아 list 가 maxHeight(320) 꽉
  차므로, default_size.height 를 "꽉 찬" 콘텐츠 높이(search 49 + list 332 + footer 31 ≈ 412)로
  두고 footer 를 바닥 고정. 항목이 적을 때만(검색 좁힘) 약간 다르다(허용).
- **추정**: 완전 일치는 popup 높이를 매 프레임 콘텐츠로 재계산하는 기능이 필요 — 별도 과제.
- **근거**: command_palette 2026-06-20. default_size 360→412 + 구역별 패딩으로 디자인 비율 일치.

## port_scanner — 테이블 컬럼 floor 가 footer 잘림의 근본 원인 (디자인 Table 구조 미준수)

- **증상**: footer 의 `주소 복사`/`닫기` 버튼이 popup 우측에서 잘린다.
- **원인**: 디자인 Table 은 고정폭 4개(port 84/proto 76/ws 120/state 140) + **flex 3개
  (addr/proc/tab, CSS `max-width:0`+ellipsis, 최소폭 없음)** 구조다. 구현이 flex 컬럼에
  `at_least` floor 를 줘서 floor 합이 가용폭을 넘으면 테이블이 popup 폭을 초과 → footer 의
  right_to_left 기준점이 화면 밖으로 밀려 버튼이 잘렸다. "footer 높이"가 아니라 **테이블 가로
  오버플로**가 진짜 원인.
- **처방**: 디자인 구조 그대로 — 고정폭은 `Column::exact`, flex 는 `Column::remainder().
  clip(true)`(floor 없음, 말줄임). exact 합 + flex 분배 = 항상 컨테이너 폭에 fit → 잘림 자체가
  구조적으로 안 생긴다. Tab 은 대개 "—" 라 디자인상 좁으므로(measured 62) flex 에서 빼 exact
  62 로 두면 addr/proc 가 89 로 넓어진다(egui remainder 는 균등 분배라 Tab 까지 flex 면
  addr/proc 가 좁아짐).
- **교훈**: "회귀 위험" 으로 우회할 게 아니라, 디자인 컴포넌트 구조(컬럼 정의)를 그대로
  따르면 얽힘이 사라진다. 위 **구조 전사** 핵심 원칙을 테이블 컬럼 정의에도 적용할 것.
- **근거**: port_scanner 2026-06-20. floor 제거 후 `주소 복사`+`닫기` 둘 다 온전.
- **전환(2026-06-26)**: "항상 fit(말줄임)" 결정을 **가로 스크롤 도입**으로 안전하게 뒤집었다.
  이제 컬럼별 **최소폭** + slack 분배(`compute_column_widths`)로 폭을 정하고, 보이는 컬럼
  최소폭 합 > 본문 폭이면 `Table::horizontal_scroll(true)`(본문을 `ScrollArea::horizontal`
  로 감쌈)로 좌우 스크롤한다. 과거 footer 잘림은 테이블이 *outer ui 폭*을 부풀려 생긴 것이라,
  가로 스크롤을 **본문 영역에만** 가두면(footer 는 여전히 popup 전체폭 rect 에 그림) 재발하지
  않는다. sticky 헤더 띠는 스크롤 컨텐츠 폭(고정폭 합)에 맞춰 칠해 본문과 수평 동기 이동.

## port_scanner — 테이블 헤더 th 배경(mantle)은 painter 로 직접 칠한다

- **증상**: egui_extras Table 은 헤더 셀 배경 API 가 없어 디자인 th `bg-sidebar`(mantle) 가
  안 칠해진다.
- **처방**: TableBuilder 전에 header 영역 rect(`cursor.top` ~ `+header_h`, 전체폭)를 계산해
  `painter().rect_filled(mantle)`. sticky header 텍스트는 그 위에 그려진다.
- **근거**: port_scanner 2026-06-20.

## port_scanner — 테이블 셀 정렬/패딩 + footer 가 ui 폭 확장에 밀린다

- **증상 1**: 표 컬럼 텍스트가 컬럼 경계에 붙고(패딩 0), Port 가 좌측 정렬(디자인은 우측).
- **원인 1**: egui_extras Table 은 셀 패딩/정렬 API 가 없다. 디자인 td `padding 0 12` + Port
  `align:right` 가 그냥 안 들어간다.
- **처방 1**: 셀 콘텐츠를 헬퍼로 감싼다 — 좌측 정렬 컬럼은 `ui.add_space(12)` 후 콘텐츠,
  Port 는 `with_layout(right_to_left)` + `add_space(12)`. 헤더 셀(draw_header_cell)도 동일.
- **증상 2**: 셀 패딩을 넣자 footer 의 `닫기` 버튼이 popup 우측에서 잘렸다(재발).
- **원인 2**: 테이블이 **컬럼 사이 `item_spacing.x`(기본 ~8 × 6 gap)** 만큼 자기 ui 폭을
  popup 폭보다 넓힌다. footer 를 그 ui 에 그리면 `right_to_left` 기준 우측이 popup 밖으로
  밀려 버튼이 잘린다. (remote_tool 에서 본 "콘텐츠가 ui 폭 확장 → footer 밀림" 과 같은 패턴.)
- **처방 2**: footer 를 `ui.allocate_new_ui(UiBuilder::max_rect(popup 전체폭 rect))` 안에
  그려 ui 폭 확장과 무관하게 popup 폭에 고정. → `주소 복사`+`닫기` 온전.
- **교훈**: footer 등 우측 기준 레이아웃은 **테이블/콘텐츠가 부풀린 ui 폭이 아니라 popup
  전체폭 rect 에 고정**해 그린다.
- **근거**: port_scanner 2026-06-20.

## port_scanner — tasty mono(D2Coding)가 디자인 폰트보다 넓다 (셀 말줄임)

- **증상**: 디자인 addr 88px 에서 "127.0.0.1"이 보이는데 tasty 같은 폭에서 1~2px 넘쳐 말줄임.
- **원인**: tasty 본문 mono 폰트(D2Coding)의 glyph advance 가 디자인 미리보기 폰트보다 넓다
  (폰트 메트릭 차이). 셀 좌우 패딩 24 까지 빼면 빠듯해진다.
- **처방**: 디자인 폭을 대체로 따르되, 거의 "—" 인 Tab 컬럼을 디자인 62→56 으로 조금 좁혀
  addr/proc 에 폭을 양보(보정). 폰트 자체 차이라 완전 일치는 불가 — 디자인도 긴 데이터는
  말줄임(seed 의 ws "serv…").
- **근거**: port_scanner 2026-06-20.

## port_scanner — 테이블 행 구분선이 divider 자동 측정을 교란

- **증상**: wide-surf1 라인 랜드마크로 구역 divider 를 찾으면 테이블 행마다의 borderBottom
  (surf1)이 다수 잡혀 header/filter/footer divider 를 가려낼 수 없다.
- **처방**: 테이블 헤더의 `mantle` 띠를 기준으로 잡고(행엔 mantle 없음), 그 위/아래로 구역
  divider 를 센다. 정밀 ±1px 가 어려우면 구역 패딩을 디자인값 그대로(코드 상수) 넣어 구조로
  보장하고 시각 비교로 갈음.
- **근거**: port_scanner 2026-06-20. 구역 패딩 header 12/14·filter 8/14·footer 9/14 상수 적용.

## remote_tool — 컨테이너 패딩 0 + 구역별 패딩 (통짜 패딩 금지)

- **증상**: 단일 Frame inner_margin 으로 전체를 감싸면 헤더(14L/12R)·탭바(8)·리스트(14)의
  서로 다른 패딩을 못 맞춘다.
- **처방**: popup content_margin 을 0 으로(`popup.rs` content_rect 에서 id 분기) 두고, 헤더/
  탭바/콘텐츠를 각자 Frame inner_margin 으로 디자인 패딩만큼 들여쓴다. 탭바 `bg-sidebar`
  배경은 전체폭 `rect_filled(mantle)` 로 직접 칠한다(Frame.fill 은 자식 폭만큼이라 전체폭이
  안 됨).
- **근거**: remote_tool 2026-06-20.

## 공용 위젯 레이어 (2026-06-21) — primitive 컴포넌트화에서 얻은 것

### 위젯의 집 = `crates/tasty-ui-widgets` (신규 디렉토리 아님)
갤러리(`tasty-gallery`)는 별도 크레이트라 메인 바이너리(`src/`)를 의존할 수 없다. 공용
위젯을 `src/adapters/ui/` 에 두면 갤러리가 또 mirror 를 떠야 한다. `tasty-ui-widgets` 는
**메인+갤러리 양쪽이 이미 의존**하고 `&Theme` 명시·본체 미의존 → primitive 의 올바른 집.
효과: 팝업과 갤러리 specimen 이 동일 함수 호출(demo=main, mirror 불필요).

### egui 세금 (디자인 → 즉시모드 변환 시)
- **폰트 weight**(medium/semibold/bold): egui 는 별도 bold family 없이는 굵기 재현 불가.
  크기+색(또는 `.strong()` 색 보정)만 따른다 — 디자인 weight 차이는 시각상 미세 손실.
- **radius-pill**(완전 둥금): egui CornerRadius 로 `height/2` 사용.
- **`color-mix(accent 40%, transparent)`**(Tag 상태 보더): `accent.gamma_multiply(0.4)`.
- **`::after` 오버레이 틴트**(Button/IconButton hover/active): pseudo-element 없음 →
  `rect_filled(overlay_*.to_egui_premultiplied())` 수동.
- **focus ring**(box-shadow 0 0 0 1px): box-shadow 없음 → `rect_stroke(outer.expand(bw),
  border_focus, Outside)`. **Motion 계약상 즉시**(focus-ring/invalid/checked 는 기능 → fade 금지).
- **separator 토큰**: 디자인 `--tasty-separator`(white@8%)는 tasty 에서 `surface1` hline 으로.
- **icon 글리프**: 위젯이 색을 상태별로 정해 `IconPainter` 클로저에 전달(아이콘 시스템은
  호출측 소유 — 본체 `icons::Icon`, 갤러리 mock 모두 동일 인터페이스).

### Motion 계약
rest/hover/active/focus/disabled **정지 상태가 canonical** — 파리티는 정지상태로 판정.
상태 사이 트랜지션(hover 틴트 fade)은 **장식** → 즉시모드 **스냅 허용**. 단 **기능적 외형은
즉시**(focus-ring 가시성, invalid 보더, checked/selected/active) — fade 금지. 터미널 0ms 별개.

### 검증
갤러리는 IPC 스크린샷이 없고 OS 캡처는 권한 불가 → 본체 격리 인스턴스
(`TASTY_HOME=tmp ./target/debug/tasty --launch`, debug 는 별도 루트로 격리 — [independent-verification](../../dev-guide/independent-verification.md)) + `ui.screenshot`
JSON-RPC + `debug.host_popup.open` 으로 검증. primitive 는 본체 팝업에 adopt 한 뒤 대조한다.

## 팝업 — egui Area 미등록 → ScrollArea 스크롤 불가 + 클립 누출 (2026-06-21)

팝업 콘텐츠가 bare `Ui::new(layer_id)` 라 **egui Area 미등록** → `Memory::layer_id_at`
이 팝업 레이어를 못 찾음 → `ScrollArea::ui_contains_pointer()`=false → **휠/드래그
스크롤 입력 무시**(모든 팝업 공통). 위젯 클릭은 widget hit-test(다른 경로)라 정상이라
"클릭은 되는데 스크롤만 안 됨" 으로 드러난다.

수정: 콘텐츠를 동일 layer_id 의 `egui::Area`(movable(false)+sense(hover))로 등록.
부수 함정 2개:
- Area 는 콘텐츠에 auto-shrink → footer(allocate_new_ui 별도 배치)가 빠져 hit-rect 가
  줄어 layer_id_at 이 팝업 하단을 못 잡음 → `set_min_size(content_rect)` 로 강제.
- `Ui::new(max_rect(r))` 는 clip_rect=r 였지만 Area 는 기본 clip 이 더 넓음 → State 컬럼
  긴 라벨(ESTABLISHED)·선택 하이라이트·스크롤바가 팝업 경계 밖으로 누출 →
  `set_clip_rect(content_rect)` 로 클립 복원.

검증은 스크롤 주입 수단이 없어 `ctx.layer_id_at(content중심)` 이 Background→팝업
Foreground area 로 전환됨을 로깅으로 확인(기계적 증명) + 팝업 스크린샷 z-order 회귀 확인.
상세 아키텍처: [`dev-guide/popup-implementation.md`](../../dev-guide/popup-implementation.md) "콘텐츠 레이어".

## port_scanner — State 컬럼 긴 라벨(ESTABLISHED)이 140px 초과 (폰트 메트릭, 클립으로 가림)

State 컬럼 `Column::exact(140)` + `.clip(true)` 미적용. tasty 폰트가 디자인보다 넓어
가장 긴 상태값(`ESTABLISHED`)이 140 을 넘쳐 셀 밖으로 그려진다. 팝업 클립이 이를 경계
에서 자르므로(위 항목) "ESTABL" 로 보인다. 디자인 스펙은 140 이나 디자인 mockup 상태값은
LISTEN/CLOSE_WAIT 로 더 짧았다. 완전 표시하려면 State 폭을 넓혀 flex(addr/proc)에서
양보해야 하며, 이는 폰트 메트릭 보정(디자인 변경 아님). 현재는 디자인 140 유지 + 클립.

## Spinner — egui 엔 `prefers-reduced-motion` 매체 질의 없음 → 파라미터로 받음

- **증상**: 디자인 Spinner 는 `prefers-reduced-motion` 에서 회전을 멈추고 3-dot fallback 을
  쓰는데, egui 엔 그 매체 질의가 없다.
- **처방**: `Spinner` 가 `reduced_motion: bool` 을 호출부 파라미터(빌더)로 받는다(StatusDot
  의 pulse 와 동일 패턴 — 모션 결정을 호출측이 소유). true 면 정지 3-dot.
- **근거**: `crates/tasty-ui-widgets/src/spinner.rs`.

## Button variant — egui 엔 CSS variant 없음 → fill/stroke 수동 조합

- **증상**: 디자인 Button 의 primary/ghost 같은 variant 는 CSS 클래스로 갈리는데 egui 엔
  variant 개념이 없다.
- **처방**: variant 별로 fill·stroke 를 수동 조합해 그린다. remote_tool 은 view-local
  `primary_button`(accent fill) / `ghost_button`(투명 fill + 보더) 헬퍼로 분리.
- **근거**: `src/adapters/ui/popup/remote_tool.rs`. (primitive 레이어는 `tasty_ui_widgets::
  Button` 의 `ButtonVariant` 가 동일 역할.)

## 폼 라벨 — egui Grid 컬럼폭 고정 미지원 → 고정폭 우측정렬 흉내

- **증상**: 디자인 ProfileForm 은 `gridTemplateColumns: 112px 1fr` 로 라벨 컬럼이 112px
  고정·우측정렬인데, egui `Grid` 는 컬럼 폭을 고정값으로 못 박는다(콘텐츠 맞춤).
- **처방**: `field_label` 이 `allocate_ui_with_layout(112px, right_to_left)` 로 고정폭 우측정렬
  컬럼을 흉내(`LABEL_COL_WIDTH=112`). hint/error 는 `112 + columnGap(12)` 만큼 들여써 입력
  컬럼에 정렬.
- **근거**: `src/adapters/ui/popup/remote_tool.rs`.

## footer 우측정렬 — flex `justify-end` 흉내

- **증상**: 디자인 footer 버튼군은 `justify-content: flex-end` 로 우측에 붙는데 egui 엔
  flex justify 가 없다.
- **처방**: `Layout::right_to_left(Align::Center)` 로 우측부터 배치(먼저 add 한 위젯이 우측
  끝). port_scanner footer·remote_tool form 액션이 이 패턴.
- **근거**: `src/adapters/ui/popup/remote_tool.rs`, `crates/tasty-gallery/src/catalog/components/
  port_scanner.rs`. (우측 기준 레이아웃을 ui 폭 확장과 무관하게 고정하는 건 위 port_scanner
  footer 항목 참고.)

## 타이포그래피 — letter-spacing / line-height / font-weight 세분화 미지원

- **증상**: 디자인 토큰엔 letter-spacing(ui 0 / caps 0.04em)·line-height(tight 1.0 / term 1.2
  / ui 1.4 / prose 1.6)·세분 font-weight 가 있으나 egui `Label` 은 이를 직접 제어하지 못한다.
- **처방**: 재현 불가 — typography specimen 에 토큰 값만 기록해 둔다(weight 는 크기+색으로만
  근사, 위 "공용 위젯 레이어 — 폰트 weight" 항목과 동일 한계).
- **근거**: `crates/tasty-gallery/src/catalog/typography.rs`.

## settings_window — 디자인 flex Row 의 gap 은 모든 자식 사이에 적용된다

- **증상**: settings 폼 Row(`Accent:` 등)에서 Input↔색스와치 간격을 `add_space(space-sm)` 로
  주면 디자인보다 좁다(또는 row item_spacing 과 겹쳐 과넓음).
- **원인**: 디자인 `Row` 는 `display:flex; gap:16` 이고 children 이 fragment(`<Input/><span
  swatch/>`)면 **label·Input·swatch 가 전부 형제 flex 자식** → 셋 사이 간격이 모두 16. 한
  쌍만 좁게 본 것은 오독.
- **처방**: row 진입 시 `ui.spacing_mut().item_spacing.x = space-lg(16)` 한 번만 설정하고
  형제 위젯 사이엔 `add_space` 를 추가하지 않는다(item_spacing 이 곧 flex gap). label 은
  `allocate_exact_size(150)` 고정컬럼 + `painter().text` 로 그려도 다음 위젯에 item_spacing 이
  붙는다.
- **근거**: `crates/tasty-gallery/src/catalog/widgets/layout_2depth.rs` 2026-06-24. settings_window.jsx `Row`.

## gallery — tasty-gallery 는 i18n(t()) 을 쓰지 않는다 (specimen 하드코딩 라벨)

- **증상**: 갤러리 specimen 에 `t()` 를 적용하려다 의존성/관례 충돌.
- **원인**: `tasty-gallery` 는 `tasty-i18n` 에 의존하지 않는다(Cargo.toml). 모든 specimen 이
  하드코딩 mock 영문 라벨을 쓴다(Storybook 류 격리 시각 카탈로그 — `host_shell.rs` 도 "본체
  i18n 시스템 미사용" 명시). CLAUDE.md 의 `t()` 규칙은 *본체 shipping UI* 대상이며 갤러리
  specimen 은 범위 밖.
- **처방**: 갤러리 specimen 라벨은 대상 디자인 jsx 의 영문 라벨을 그대로 미러(하드코딩). i18n
  의존 추가는 단일 specimen 작업 범위 밖.
- **근거**: `crates/tasty-gallery/Cargo.toml`(i18n 미의존) + 30+ 기존 specimen 관례. 2026-06-24.
## Kbd 우측정렬 — `ui.horizontal` 이 부모 RTL 을 상속해 키캡 순서 역전

- **증상**: command 행에서 `Kbd`("Ctrl+Alt+G")를 `with_layout(right_to_left)` 안에서 그리면
  키캡이 "G + Alt + Ctrl" 로 뒤집혀 렌더된다.
- **원인(검증)**: egui `Ui::horizontal` 은 부모의 `prefer_right_to_left()` 를 상속한다. RTL
  부모 안에서 `kbd`(내부 `ui.horizontal` 사용) 를 호출하면 키캡 시퀀스가 RTL 로 배치된다.
- **처방**: RTL 레이아웃은 그대로 쓰되 **그 안에 들어가는 키캡을 하나로 제한한다.** 키캡이
  하나면 시퀀스가 없으므로 역전될 것이 없다. 여러 키캡의 조합(chord)은 RTL 밖에서 그린다.
- **근거**: 우측 Kbd 를 그리는 두 자리가 모두 이 형태다 —
  `crates/tasty-gallery/src/catalog/components/modifier_hint.rs`(`hint_row`, 행 키캡은
  `sec.chord` 접두를 뗀 leaf 한 개) 와 `src/adapters/ui/modifier_hint_overlay.rs`(`draw_row`,
  `binding_leaf` 가 modifier 를 벗긴 leaf 한 개). 조합 전체를 보여주는 자리는 RTL 밖의
  `chord_head` 이고, 그래서 `kbd_parts` 가 부모 RTL 을 상속해도 뒤집힐 시퀀스가 없다.
  폭을 재서 spacer 로 미는 방식은 채택되지 않았다(그런 헬퍼는 저장소에 없다).

## color-mix(in srgb …) 재현 — lerp / alpha 헬퍼

- **증상**: 디자인이 아바타·배너에 `color-mix(in srgb, C 18%, surface)`(불투명 블렌드)와
  `color-mix(in srgb, C 11%, transparent)`(알파 감소)를 쓴다. egui 엔 color-mix 가 없다.
- **처방**: 두 케이스를 분리. 불투명 블렌드 = srgb 바이트 선형보간 `mix_srgb(a, ratio, b)`
  (비율이 가운데 인자다 — `a` 를 `ratio` 만큼 `b` 에 섞고, 결과 알파는 배경 `b` 의 것을 따른다).
  transparent 믹스 = 색은 그대로 두고 알파만 얹는다(= C 를 알파 t 로). 후자는 Tag 위젯의
  `gamma_multiply(0.4)`(=border 40% transparent) 와 같은 의도.
- **근거**: 두 케이스가 각각 구현돼 있다. 불투명 블렌드는
  `crates/tasty-type-appearance/src/theme.rs` 의 `mix_srgb(a, ratio, b)` — 그 파일의 wash
  접근자들이 유일한 호출자다. 알파 감소는 `HexColor::with_alpha`(`crates/tasty-type-appearance/src/color.rs`)
  로 상수 알파를 얹는 형태이고, 최종 변환이 `Color32::from_rgba_unmultiplied` 다
  (`HexColor::to_egui`, 같은 파일). Tag 의 `gamma_multiply(0.4)` 는
  `crates/tasty-ui-widgets/src/chip.rs` 의 `TagVariant::Info` 보더에 살아 있다.

---

## 일반 — component-tier 디자인 토큰은 신규 Theme 필드를 만들지 않는다 (semantic 접근자 직접 매핑)

- **증상**: 디자인 `tokens/components.css` 에 새 컴포넌트 토큰 블록이 추가되면(예
  `switch-overlay-active-bg: var(--tasty-accent-primary)`), Theme 에 대응 필드를 새로
  만들어야 할 것처럼 보인다.
- **원인(검증)**: tasty `Theme` 은 **semantic-tier 값만 필드/접근자로 보유**하고, component-tier
  토큰은 코드가 그 접근자를 직접 호출해 표현한다. `components.css` 의 `button-primary-bg`
  · `checkbox-bg-checked` · `switch-track-bg-on` 이 모두 `--tasty-accent-primary` alias 지만
  Theme 엔 전용 필드가 없고 호출부가 `accent_primary()` 를 직접 부른다(매핑표 `design-token-mapping.md`
  에 dedicated 필드 0개). 즉 디자인의 3-tier(component→semantic→primitive)에서 tasty Theme 은
  semantic tier 에 해당하고, component tier 는 호출부의 책임.
- **처방**: 새 component 토큰이 기존 semantic 접근자(`accent_primary()`/`text_on_accent()`/
  `surface0`/`subtext1`/`surface1` 등)나 위젯 상수로 해석되면 **신규 필드를 만들지 말고 매핑만
  기록**. 새 *semantic* 색/치수가 진짜로 도입될 때만 Theme 필드를 추가한다.
- **근거(2026-06-25)**: switch-number overlay 8 토큰 전부 기존 접근자/`Kbd` 위젯 상수(`chip.rs`
  `KBD_HEIGHT/KBD_BOTTOM_BORDER`)·`font_size_micro` 로 커버 → P0 에서 theme.rs 무변경. 매핑은
  `design-token-mapping.md` "switch-number overlay (chrome)" 섹션.

---

## 갤러리 — inline 키캡 위젯은 좌표 slot 에 못 끼운다 → 형상 재현

- **증상**: switch-number overlay 는 탭 스트립/사이드바 행 중간의 *정해진 16px slot*(아이콘/
  dot 자리)에 키캡을 그려야 한다. 본체 `kbd()`(`crates/tasty-ui-widgets/src/chip.rs`)를 그대로
  부르고 싶지만, kbd 는 `ui.horizontal` + `allocate_exact_size` 로 **자체 레이아웃 흐름에 inline
  배치**하는 위젯이라 임의 좌표 slot 에 끼울 수 없다.
- **원인**: 갤러리 mock(tab_bar/sidebar specimen)은 `ui.painter_at(rect)` 로 좌표 painting 한다.
  inline 위젯(kbd)과 좌표 painting 은 배치 모델이 달라 섞이지 않는다.
- **처방**: 키캡 *형상*을 painter 로 재현하는 헬퍼(`num_cap`)를 둔다. 레시피는 `chip.rs` 의 kbd
  와 1:1 — `corner_radius_sm` radius, `border_width` 1px stroke(Inside), 하단 2px line(=
  switch-overlay-shadow-depth=size-2), `font_size_micro` mono, fill `surface_raised`/border
  `border_strong`/fg `text_secondary`. active 변종만 `accent_primary` fill + `text_on_accent`
  숫자. tab_bar.rs 가 본체 tab 시각을 painter 로 재현하는 것과 같은 방식.
- **근거(2026-06-25)**: `crates/tasty-gallery/src/catalog/components/switch_overlay.rs`. 신규
  Theme 필드 없음(P0). 본체 P2 draw 도 같은 좌표 painting 이 될 것이므로 형상 로직 공유 가능.

---

## switch-number overlay — modifier 상태 소스 & 이벤트 구동 redraw

- **증상**: "modifier 를 누르고 있는 동안" 만 보이는 오버레이는 (a) 무엇으로 modifier 상태를
  읽을지, (b) 다른 키 입력 없이 modifier press/release 만으로 redraw 가 도는지가 관건.
- **원인/사실(검증)**:
  - tasty 는 `WindowEvent::ModifiersChanged` 를 egui 에 전달하고(`src/view/main.rs:256-258`),
    egui 가 반환하는 `repaint` 가 true 면 `mark_dirty()` → RedrawRequested → `run_egui_frame`.
    즉 **bare Ctrl press/release 도 redraw 를 유발**한다(별도 배선 불필요). focus 상실 시
    `base.modifiers = empty()` (main.rs:289) 로도 정리되고 egui 도 동일.
  - draw 단계 modifier 소스는 **egui `ctx.input(|i| i.modifiers)`** 가 가장 깔끔. winit→egui
    raw_input 으로 들어온 **실제 사용자 입력만** 반영 → IPC/에이전트가 raw_input 에 주입
    불가 → 사용자↔에이전트 분리 자동 충족. tasty `base.modifiers`(MainView) 를 draw 까지
    plumbing 할 필요 없음.
- **처방**: 공통 모듈 `src/adapters/ui/switch_overlay.rs` 에 ① modifier↔대상 판정
  (`switch_target_for`, numeric.rs 규칙 1:1; 사이드바용 얇은 래퍼 `workspace_switch_held`) ②
  키캡 painter(`paint_keycap`) 를 모은다. wrapper 가 **순수 view props 로 전달**(view 는 settings
  비의존 유지, model-view-split).
- **focused pane 한정(탭)**: 탭 전환 단축키는 focused pane 의 탭만 전환하므로, 키캡도 focused
  pane 의 탭바에만 그린다. `tab_bar.rs` wrapper 는 `ctx.input` 대신 `state.switch_overlay()`
  스냅샷(`Tab` 대상일 때 focused pane id 동봉)에서 `switch_overlay_pane: Option<u32>` 를 뽑아
  `PaneTabBarsProps` 로 넘기고, view 는 `tab_keycap_for(switch_overlay_pane, pane_id, i)` 로 매칭되는
  pane 에서만 키캡(비-focused pane 은 held 여도 아이콘 유지). 사이드바는 워크스페이스 전역 전환이라
  pane 한정이 없어 `ctx.input` modifier → `workspace_switch_held` bool 직접 사용.
- **근거(2026-06-27)**: `tab_bar.rs` (`PaneTabBarsProps.switch_overlay_pane`), `accessors.rs`
  (`switch_overlay()`), `switch_overlay.rs` (`tab_keycap_for`). P2b 사이드바도 같은 모듈의
  `workspace_switch_held`/`workspace_digit`/`paint_keycap` 재사용.

---

## command_palette — 키캡은 본체 custom draw, 갤러리는 menu_item kit-widget (미러 비대칭)

- **증상**: 명령 팔레트 단축키를 디자인 Kbd(키별 keycap)로 정렬하는 작업에서, 본체와 갤러리
  미러의 구현 아키텍처가 다르다.
- **사실(검증, 2026-06-25)**:
  - 본체 `src/adapters/ui/popup/command_palette.rs` 는 `draw_keycaps()` 로 **좌표 painting** 해
    키별 keycap + muted `+` 구분자를 그린다(우측 정렬). casing 은 `KeybindingSettings::
    format_display_parts()`(crud.rs:413, `ctrl++` 모호성 안전 토큰화) 결과를 `shortcut_keys:
    Vec<String>` 로 받는다. 빈 쿼리 무강조는 `row_highlighted(query_empty,…)`. 색은
    surface_raised/border_strong/text_secondary, radius 는 `corner_radius_sm`. 모두 단위 테스트 있음.
  - 갤러리 `crates/tasty-gallery/src/catalog/components/command_palette.rs` 는 공유
    `tasty_ui_widgets::menu_item` 위젯에 **단일 문자열 shortcut**(`"⌘T"`)을 넘기는 kit-widget
    표현(WIDTH=480)이다 — 본체의 custom draw_keycaps 를 줄단위 복제하지 않는다.
- **처방/한계**: 본체 키캡에 디자인 Kbd 의 하단 2px edge(`--tasty-kbd-shadow-depth`=size-2)를
  `line_segment` 로 덧그려 깊이감을 맞춘다(chip.rs `kbd()` 와 동일 근사). 갤러리 미러에 키별
  keycap 을 넣으려면 **공유 `menu_item` 위젯**이 keycap 벡터를 지원하도록 바꿔야 해(모든
  menu_item 사용처 영향) 단일 컴포넌트 작업 범위를 넘는다 — 별도 결정 필요.
- **근거**: design (3) `components/core/Kbd.jsx`(키별 `<kbd>`, `border-bottom-width: kbd-shadow-depth`),
  `command_palette.jsx:50`(`active = n===0 && q!==""`). 디자인 Kbd 토큰 치수는 size-16/micro(10)
  인데 본체 draw_keycaps 는 18/caption(11) 로 그려 미세 치수 차가 남아 있다(재조정은 별도 판단).

---

## sidebar 카테고리 헤더 — 패딩 대칭화 + 고아 구분선 제거 (2026-07-02 디자인 변경 반영)

- **증상**: 카테고리 헤더가 상 12/하 4 비대칭 패딩이라 top-heavy 로 보이고, 그룹 행 리스트가
  상+하 보더를 둘 다 그려 다음 헤더 위 gap 에 이전 그룹의 하단 보더가 떠 있었다(고아 구분선).
- **원인**: 구 디자인은 헤더 top 패딩(space-md)이 섹션 간 간격을 겸했다. 2026-07-02 디자인이
  두 역할을 분리 — 헤더는 상하 space-xs(4) 대칭, 섹션 간격은 그룹 컨테이너로 이동(컨테이너
  paddingTop space-sm 8 + 비-첫 섹션 marginTop space-sm 8). 그룹 행 리스트는 `rowList(…,
  bottomBorder=false)` 로 **상단 보더만**(헤더 → 선 → 행). 평면 모드는 상+하 유지, 레일 불변.
- **처방(전사)**: `view.rs::draw_category_header` pad_top 을 `spacing_xs` 로(상하 대칭 4,
  헤더 총 34→26). 그룹 렌더 분기의 목록 하단 `draw_list_separator` 삭제(상단만). 섹션 루프
  enumerate 로 비-첫 섹션 앞 `add_space(spacing_sm)` — `sec_start` 캡처 **뒤**에 두어 gap 이
  해당 섹션 드롭존(section_spans)에 포함. 스크롤 시작 `add_space(8.0)`(그룹/평면 공통)이
  디자인 컨테이너 paddingTop 8, New Workspace 앞 `add_space(4.0)` 이 paddingBottom 4 대응 —
  둘 다 무변경. 갤러리 `sidebar.rs::full_categories` 는 schematic 유지 + 3항목만 미러(헤더
  상하 xs 대칭 인셋 / 비-첫 spacing_sm / 헤더 아래 1px separator rule 추가 — 하단 rule 없음).
- ~~알려진 잔차(의도적 비변경)~~ **해소됨 (design-tokens 02 간격 이식)**: 본체 헤더 패널
  하단 6px 는 `spacing_xs`(4) 로, 상단 10px 는 `spacing_md`(12) 로 스냅되어 chrome.jsx
  Sidebar 헤더 padding(space-md _ space-xs)과 정합. 첫 헤더 위 실효 간격 12 = 디자인 12.
- **근거**: `chrome.jsx` `CategoryHeader`(padding xs/sm)·`rowList(bottomBorder)`·컨테이너
  paddingTop/marginTop.

## preset 편집기 — 정적 specimen 은 존/× hover·crosshair 를 재현 못 한다

- **증상**: 갤러리 `preset_editor` specimen 의 편집 직접조작(경계 split 존·mini tab close ×·
  add-tab +)이 본체 `demo_layout.rs` 의 live 동작과 100% 동형이 아니다.
- **원인**: 갤러리 specimen 은 binary 미의존 **정적**(Theme-only) 렌더라 마우스 hover·pointer
  추적·커서 아이콘이 없다. 경계 split 존은 커서 위치로 활성 변을 고르고(`pick_zone`) crosshair
  커서로 바뀌며, tab × 는 `active || hover` 일 때만, add-tab hover fill 도 실시간 pointer 로
  결정되는데 — 정적 캔버스엔 이 입력 축이 존재하지 않는다.
- **처방(전사)**: specimen 은 이 상태들을 **고정 상태 예시**로 전사한다 — `draw_edit_direct_mock`
  이 Left 존을 활성 예시로 항상 그리고(`draw_split_zone_overlay_mock`), 탭 하나는 active 의 ×
  rest 상태, 다른 하나는 hover 상태(overlay_active fill), add-tab 은 hover fill 상태로 굳혀
  보여준다. crosshair 커서는 정적에서 표현 불가라 생략(밴드+2px 분할선 시각만 전사). 색·치수는
  본체와 **동일 토큰**(`preset_split_zone_bg/border`, `overlay_active/hover`, 14×14 ×, 22×20 +,
  30% 밴드)이라 구조·토큰 축은 정합하고, 오직 "입력 상태 전이"만 정적↔live 로 갈린다.
- **근거**: `gallery/preset_editor.jsx` (`SurfaceBox`/`pickZone`/`AddTabBtn`).

## explorer GridCell — 아이콘 축소 + 파일명 3줄 wrap 말줄임 (2026-07-09 디자인 확정 반영)

- **증상**: explorer grid(아이콘) 셀이 28px 아이콘 + **1줄 12자 하드컷**(`truncate(&e.name,12)`
  → `rust-toolch…`) 이라 긴 파일명 식별이 어렵고, 아이콘이 셀 높이를 대부분 차지했다.
- **원인/전사 포인트**: 디자인 `GridCell`(gallery/plugins.jsx L301, `WebkitLineClamp:3`)은
  폭 기준 3줄 wrap + 마지막 줄 말줄임을 규정하는데, egui `p.text(Align2::CENTER_CENTER, …)` 은
  **단일 행 렌더**라 이 구조를 못 담는다. 구조 전사에는 다행 wrap+말줄임을 네이티브로 처리하는
  `LayoutJob` 이 필요하다.
- **처방(구조 축)**: 라벨을 `LayoutJob { halign: Align::Center, wrap: TextWrapping {
  max_width: CELL_W - spacing_xs*2(=72), max_rows: 3, overflow_character: Some('…') } }` +
  `ui.fonts(|f| f.layout_job(job))` galley → `p.galley()` 로 교체. `halign::Center` 로 각
  행이 x=0 중심 정렬되므로 `p.galley(center.x, …)` 가 다행 중앙정렬을, pos.y 를 라벨 블록 상단에
  두어 **top 정렬**(구 `label_h/2` 수직 중앙 아님)을 준다. 셀 높이는 `label_line_h ×3`(round(11×1.3)
  =14 ×3) 고정 예약으로 짧은 이름도 3줄분을 잡아 `horizontal_wrapped` 그리드 행을 균일화한다.
  `CELL_W` 80 유지, 아이콘→라벨 `spacing_xs`(4)·블록 상하 `spacing_sm`(8) 리듬 불변. dead 가 된
  `truncate()` 함수 제거.
- **처방(토큰 축)**: 아이콘 `item_height_interactive`(28) → `icon_glyph_size_md`(16), 라벨 폰트
  `font_size_body`(13) → `font_size_caption`(11)(사용자 explorer 폰트를 caption 상한으로 clamp).
  라벨색 **상태 의존**으로 — 선택 `text_primary` / 비선택 `text_secondary`(디자인이 함께 확정,
  기존엔 상태 무관 항상 primary). glyph 색(폴더/파일 text-muted·이미지 accent-info)·선택/hover/cut
  스타일은 불변. 신규 Theme 필드 0(기존 토큰 재사용).
- **egui 세금/검증 함정**: `Galley::text()` 는 **원본 소스 문자열**을 돌려주므로 말줄임 삽입
  여부를 여기서 못 본다 — 실제 렌더 결과는 `galley.rows[].glyphs[].chr` 로 재구성해야 확인된다.
  실 폰트 레이아웃으로 디자인 샘플(src·rust-toolchain.toml·THIRD_PARTY_LICENSES.md) 전부 ≤3행,
  초장문 unbreakable 이름은 3행 클램프 + glyph 에 `…` 삽입을 확인(임시 test, 검증 후 제거).
- **근거**: `gallery/plugins.jsx` `GridCell`(L301)·`ExpGridMini`(L314, 긴 이름 샘플). 본체
  `src/adapters/ui/surface/explorer.rs` `grid_cell()`, specimen
  `crates/tasty-gallery/src/catalog/components/explorer_view_cells.rs`
  `grid_cell()`(GRID 긴 이름 샘플 + meta `"glyph 16 + 3-line label (…) · fixed height"`).

## PathField — AutoComplete + Go 합성 공용 위젯 (편집/이동/원복 결정 포팅, 2026-07-09)

- **무엇**: 두 주소창(Explorer / Markdown)이 공유할 편집형 경로 필드를 `tasty-ui-widgets` 에
  신설(`path_field.rs`). 디자인 `plugins.jsx` `PathField`(:59) 전사 — 트리거 = `AutoComplete`
  (Input 언어 + 후보 드롭다운) + 우측 Go `IconButton`(sm, arrow-right). idle=mono text-secondary,
  editing=text-primary + focus ring + caret(Input 기본).
- **구조 축**: 디자인 `PathField` 는 `editing && candidates` 면 `<AutoComplete withGo …/>`, 아니면
  필드 div + `<IconButton Go/>` 두 브랜치다. 소스 `AutoComplete` 에는 `withGo` 가 없어(markdown 이
  Go 를 따로 그렸음) PathField 가 **AutoComplete + Go IconButton 을 `ui.horizontal` 한 행에** 합성
  한다: 필드폭 = 총폭 − control-height(sm 28) − `spacing_sm`(6) gap. 드롭다운은 트리거 rect 아래
  floating(AutoComplete 소유)이라 Go 버튼과 겹치지 않는다.
- **토큰 축**: 색·간격·행높이 전부 `theme.*` accessor — 필드 fill=`surface-raised`(input-bg),
  idle=`text-secondary`, editing=`text-primary`, match=`accent-primary`, Go 버튼=`IconButton`(sm)
  자체 토큰. raw px/`from_rgb` 0. 신규 Theme 필드 0(AutoComplete/IconButton 토큰 재사용).
- **결정 로직 포팅**: markdown `addr_outcome(action, lost_focus)` → 위젯 순수함수 `decide(action,
  lost_focus, go_clicked)`. 우선순위 **Esc(Cancel) > Pick(행 확정) > Submit(버퍼) > Go 클릭 >
  확정없는 blur(원복) > None**. Go 클릭은 같은 프레임 `lost_focus` 를 유발하지만 이동 확정이므로
  blur-원복보다 앞선다(이 순서가 회귀 방지 핵심 — 단위테스트 `decide_go_click_navigates_buffer_over_blur_revert`).
  상태(buffer/editing/active)는 호출측 소유, 위젯이 매 프레임 `&mut` 갱신(글로벌 상태 0).
- **egui 세금/specimen**: editing 의 focus ring/caret 은 실제 포커스에서만 Input 이 그린다 → 정적
  specimen 은 focus 테두리를 못 고정한다. `prim_path_field` 는 idle/editing+list 를 정적 전사(필드
  행 + `autocomplete_dropdown`)하되, 실제 편집·포커스링·키내비·이동/원복은 **라이브 `PathField`
  인스턴스**(context 별 click-to-edit)로 노출한다(gallery-first).
- **근거**: 디자인 `gallery/plugins.jsx` `PathField`(:59). 소스
  `crates/tasty-ui-widgets/src/path_field.rs`, specimen
  `crates/tasty-gallery/src/catalog/components/prim_path_field.rs`. 소비처 전환(markdown/explorer)은
  후속 TODO.

## transfer 팝업 — scrim_backdrop 스테이지가 카드보다 짧으면 클러스터가 겹친다 (2026-07-23)

- **증상**: 갤러리 transfer specimen 을 `kit::scrim_backdrop`(고정 height) 안에 카드를 얹어
  그렸더니, 다중 파일(2행) 진행 카드가 스테이지 높이(240)를 넘쳐 아래 클러스터와 겹쳐 렌더됐다.
- **원인(검증)**: `scrim_backdrop` 은 고정 rect 를 allocate 하고 모달을 `new_child` **오버레이**로
  그린다 — 카드는 부모 flow 의 커서를 진행시키지 않아(오버레이) 스테이지보다 크면 아래로 새어
  다음 cluster 위에 겹친다. 카드 높이가 가변(행 수)이라 고정 스테이지로는 담을 수 없다.
- **처방**: 실제 scrim dim 은 본체 `draw.rs`(scrim id-set)가 그리므로, 갤러리 specimen 은
  scrim 스테이지 없이 프레임을 **클러스터에 직접 렌더**한다(`egui::Frame` 이 flow 에서 정상
  공간 확보). file_picker specimen 이 이미 이 관례(card 를 cluster 에 직접) — 동일하게 맞췄다.
- **근거**: `crates/tasty-gallery/src/catalog/components/transfer.rs`. 캡처 검증(Overlays 페이지
  임시 상단 배치 → `TASTY_GALLERY_SHOT=3` → 겹침 해소 확인).

## tab_bar — attention kind 도입으로 옛 "값-보존" divergence 가 해소됨 (2026-08-10)

- **배경**: kind 가 `Completion` 1종뿐이던 시절, `tab_bar.rs` 의 탭 제목 강조색은 실제로는
  파랑(`accent-primary`)이어야 할 자리에 노랑(`accent_warning()`)을 쓰고 있었다 — 전용
  "notification" 토큰이 없어 시맨틱이 다른 `accent_warning` 값을 그대로 재사용한
  값-보존 divergence였다(주석: "divergence: notif 강조. warning 과 값 동일하나 의미는
  notification — 전용 토큰 부재로 accent_warning() 값-보존").
- **해소**: `NeedsInput` kind 추가로 노랑(`accent_warning`)이 진짜 의미(응답 대기)를 갖는
  전용 색이 되고, `Completion` 은 원래 의도대로 파랑(`accent_primary`)으로 바로잡혔다 —
  값-보존이 필요 없어졌다. 두 kind 가 서로 다른 사용자 행동(완료 확인 vs 즉시 응답)을
  요구하므로 둘을 구분되는 색으로 분리하는 것이 이번 kind 도입의 목적 자체이며, 노랑은
  이미 `accent_warning` 으로 존재하던 "주의 필요" 시맨틱과 자연스럽게 맞고 파랑은
  워크스페이스 배지·로고 등 기존 `accent_primary` 용례(중립적 정보 강조)와 맞아
  완료 쪽에 배정했다.
- **근거**: `src/adapters/ui/tab_bar.rs` `text_color` match(`AttentionKind` 분기). 상세는
  [design-token-mapping §attention kind](design-token-mapping.md#attention-kind--needsinputcompletion-surface-highlight-adr-0062)
  · [design-gallery-mapping §Attention kind](design-gallery-mapping.md#attention-kind--needsinput-배지dot테두리탭-제목-surfaces-adr-0062).
