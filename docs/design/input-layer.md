# 마우스 입력 계층 (Input Layer)

> **상태: 설계 완료, 미구현.**

## 원칙

**렌더링 z-order와 입력 z-order는 일치해야 한다.** 화면에서 위에 그려진 요소가 마우스 입력을 먼저 받는다. 아래에 가려진 요소는 입력을 받지 않는다.

## 기본 동작: 입력 소비 (Consume)

마우스 이벤트(클릭, 이동, 스크롤)는 z-order 최상위 레이어부터 hit-test를 수행한다. 마우스 좌표가 해당 레이어의 영역 안에 있으면, 그 레이어가 이벤트를 **소비(consume)**한다. 소비된 이벤트는 하위 레이어에 전달되지 않는다.

```
마우스 클릭 (x, y)
  → Layer 4 (최상위): hit? → Yes → 소비. 끝.
  → Layer 3: (도달하지 않음)
  → Layer 2: (도달하지 않음)
  → Layer 1 (최하위): (도달하지 않음)
```

이것이 유일한 기본 동작이다. 투과(pass-through)는 기본이 아니다.

## 예외: 버블링 (Bubble)

특정 레이어는 자신을 **버블링 가능(bubble-through)**으로 선언할 수 있다. 버블링 레이어는 이벤트를 받되, 처리 후 하위 레이어에도 전달한다.

```
마우스 클릭 (x, y)
  → Layer 4 (bubble-through): hit? → Yes → 처리 + 하위에 전달
  → Layer 3: hit? → Yes → 소비. 끝.
  → Layer 2: (도달하지 않음)
```

용도: 반투명 오버레이, 그림자, 시각적 효과 레이어 등 입력을 가로채지 않으면서 시각적으로 존재해야 하는 요소.

**주의**: 버블링은 "자기 자신을 통과"하는 것이지, 아무것도 안 하고 투과하는 것이 아니다. 버블링 레이어도 이벤트를 수신하며, 필요하면 커서 변경 등의 처리를 할 수 있다. 단지 소비하지 않을 뿐이다.

## 입력 계층 순서

z-order 최상위부터:

| 순서 | 레이어 | 영역 | 소비 방식 |
|------|--------|------|-----------|
| 1 | egui 위젯 | 사이드바, 설정 오버레이 등 | egui가 consumed=true 반환 |
| 2 | Popup | PopupManager의 열린 팝업들 (자체 z-order) | popup rect 안이면 소비 |
| 3 | Divider | pane/surface 분할 경계선 | threshold 범위 안이면 소비 |
| 4 | Terminal/Surface | pane 내부 콘텐츠 영역 | 최하위, 항상 소비 |

## 커서 결정

커서 아이콘도 입력 계층을 따른다. 이벤트를 소비한 레이어가 커서를 결정한다.

| 레이어 | 커서 |
|--------|------|
| egui 위젯 | egui가 결정 (보통 Default 또는 PointingHand) |
| Popup 타이틀바 | Grab |
| Popup 콘텐츠 | Default (또는 콘텐츠별 커서) |
| Divider | ColResize / RowResize |
| Terminal | Text |
| Explorer/Markdown | Default |

## 현재 문제

현재 구현에서 Popup은 입력 계층에 참여하지 않는다:

- Popup은 egui 위젯이 아닌 `layer_painter`로 렌더링됨 → `egui_consumed=false`
- hit-test는 PopupManager 내부에서 수동으로 수행하지만, 결과가 마우스 핸들러에 전달되지 않음
- 결과: 팝업 위에서 클릭하면 터미널이 클릭을 받고, 커서도 Text로 표시됨

## 구현 방향

### PopupManager에서 hit 상태 노출

`PopupManager::draw()`의 반환값에 hit 정보를 포함:

```rust
pub struct PopupDrawResult {
    /// 닫힌 팝업 ID 목록
    pub closed: Vec<PopupId>,
    /// 현재 마우스가 어떤 팝업 위에 있는지
    pub hovered_popup: Option<PopupId>,
}
```

### 마우스 핸들러에서 계층 적용

`handle_cursor_moved`와 `handle_mouse_input`에서:

```
1. egui_consumed? → egui가 처리. 끝.
2. popup_hovered? → popup이 소비. 커서=Default. 끝.
3. divider_hit?   → divider가 소비. 커서=Resize. 끝.
4. terminal       → 터미널이 처리. 커서=Text.
```

### popup_hovered 상태 전달 경로

`PopupManager::draw()`는 렌더 시점에 호출되고, 마우스 이벤트는 이벤트 시점에 처리된다. 타이밍이 다르므로 `popup_hovered`를 프레임 간 상태로 저장해야 한다.

저장 위치: `AppState`에 `pub popup_hovered: bool` 필드 추가. `PopupManager::draw()` 결과로 매 프레임 갱신. 마우스 핸들러에서 참조.
