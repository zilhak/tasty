# UI/렌더링 변경 시각 검증

스크린샷의 "눈으로 보이는지" 에만 의존하지 말고, **코드 레벨에서 논리적으로** + **픽셀 수치로** 검증한다. 자체 검증 일반 절차는 [dev-guide/self-verification](../dev-guide/self-verification.md).

## 스크린샷은 tasty 자체 `ui.screenshot` 우선

화면 캡처는 OS 캡처 도구가 아니라 **tasty 의 debug IPC `ui.screenshot`** 를 우선 쓴다 — 정확한 윈도우/surface 영역을 결정적으로 얻고, 좌표가 tasty 내부 레이아웃과 일치한다(OS 캡처는 데코·DPI·다른 창 혼입 위험). debug 빌드 전용이며 release 표면엔 없다 → [debug-ipc](../dev-guide/debug-ipc.md). 셀 색 검증은 `debug.glyph_color`(렌더러가 GPU 에 push 하는 실제 RGBA)도 함께.

## 체크리스트 ("보인다"고 말하기 전에)

### 1. 색상 대비

새 UI 요소의 색과 배경색의 RGB 차이를 계산한다. 배경과 같거나 유사하면 **확대해도 안 보인다.** 각 채널(R/G/B) 중 하나 이상이 충분히 차이나야 한다.

```
예: 배경 rgb(26,26,30) 에 경계선 rgb(60,60,75) → 차이 최대 45 (겨우 보임)
```

(실제 색은 Theme 토큰에서 — 하드코딩 금지, [color-policy](../dev-guide/color-policy.md). 대비 기준은 [theme.md](../design/systems/theme.md) "텍스트 대비 4.5:1".)

### 2. 렌더 레이어/순서

요소가 실제로 화면에 나타나는지 렌더 파이프라인 순서를 코드에서 추적한다.

- egui `LayerId::background()` 는 모든 패널 **뒤** → 터미널 위에 그리려면 `Order::Foreground`.
- 터미널 GPU 렌더는 **누적(accumulate) → flush 1회 → 단일 패스 + per-surface scissor** 모델이다. 한 surface 의 인스턴스가 다른 surface range 를 침범하지 않는지(scissor rect / instance range)를 확인한다 — 상세 [gpu-rendering](../dev-guide/gpu-rendering.md).

### 3. 픽셀 수치 검증

스크린샷(또는 `ui.screenshot`) 후 해당 영역의 RGB 를 직접 읽어 기대값과 비교한다.

```python
for x in range(expected_x - 5, expected_x + 5):
    idx = (y * width + x) * 3
    print(f'pixel({x},{y}) = rgb({data[idx]},{data[idx+1]},{data[idx+2]})')
```

기대 색이 그 좌표에 실제로 있는지 확인한 뒤에야 "보인다" 고 판단한다.

체크: ① 색이 배경과 다른가(코드) ② 레이어가 위에 그려지나(렌더 순서) ③ 픽셀 RGB 가 기대값과 일치하나(수치). 셋 다 통과 안 하면 "보인다" 고 말하지 않는다.

## 스크린샷 판단 휴리스틱 (인지 함정 방지)

픽셀 검증과 별개로, 눈으로 볼 때 빠지는 함정:

1. **전체를 훑지 말 것.** 변경 영역을 좌표로 특정하고 그 영역만 집중해서 본다 — 전체 인상으로 판단하면 변경분이 묻힌다.
2. **"안 보인다" 단정 전 좌표 재확인.** 작은 요소는 축소 스크린샷에서 안 보인다. "안 보인다" 는 자주 "내가 못 찾았다" 다 — 잘라 확대하거나 픽셀을 읽는다.
3. **코드 수치와 스크린샷 대조.** 알파 12 오버레이가 배경 위에서 실제로 어떤 차이를 내는지 직접 본다 — "12 면 약하다/강하다" 를 코드만 보고 추측하지 않는다.
4. **불확실하면 "잘 모르겠다" 라고 말한다.** 틀린 확신이 신뢰를 깎는다.

체크리스트는 *기준*, 휴리스틱은 *판단 과정의 함정 방지* 다.

## 관련

- [dev-guide/debug-ipc](../dev-guide/debug-ipc.md) — `ui.screenshot` / `debug.glyph_color`
- [dev-guide/gpu-rendering](../dev-guide/gpu-rendering.md) · [dev-guide/color-policy](../dev-guide/color-policy.md) · [design/systems/theme](../design/systems/theme.md)
