# Design Parity 히스토리 — 디자인(html/CSS) ↔ 구현(winit/egui) 구조적 차이

`design-parity` 스킬이 발동 시 먼저 읽는 노트. **검증으로 확인된 사실만** 적는다(추정이면
명시). 같은 함정을 두 번 파지 않기 위함. 형식: 증상 / 원인 / 처방 / 근거.

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
  따르면 얽힘이 사라진다. 단계 0 "컴포넌트 구조 정합" 을 테이블에도 적용할 것.
- **근거**: port_scanner 2026-06-20. floor 제거 후 `주소 복사`+`닫기` 둘 다 온전.

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

### Motion 계약 (디자인 changelog 2026-06-21-motion-contract)
rest/hover/active/focus/disabled **정지 상태가 canonical** — 파리티는 정지상태로 판정.
상태 사이 트랜지션(hover 틴트 fade)은 **장식** → 즉시모드 **스냅 허용**. 단 **기능적 외형은
즉시**(focus-ring 가시성, invalid 보더, checked/selected/active) — fade 금지. 터미널 0ms 별개.

### 검증
갤러리는 IPC 스크린샷이 없고 OS 캡처는 권한 불가 → 본체 격리 인스턴스
(`HOME=tmp ./target/debug/tasty --launch`, debug 포트 `tasty-debug.port`) + `ui.screenshot`
JSON-RPC + `debug.host_popup.open` 으로 검증. primitive 는 본체 팝업에 adopt 한 뒤 대조한다.
