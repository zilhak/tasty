# DPI 배율(scale factor) 검증

DPI≠1 환경을 Xvfb 에서 재현해, 논리↔물리 변환이 실제로 맞는지 확인하는 절차.

**이 문서가 다루는 축은 하나다.** DPI scale factor 는 논리 픽셀을 물리 픽셀로 옮기는
배율이고, `AppearanceSettings.ui_scale`(UI zoom)은 sizing 토큰 자체에 곱하는 배율이다.
한 화면에서 둘이 동시에 작용하므로 **한 번에 하나만 바꾼다** — 둘을 같이 움직이면 어느
쪽이 원인인지 갈리지 않는다.

## 배율을 거는 법

winit 이 X11 환경변수를 직접 읽는다. 코드 변경도 설정도 필요 없다.

```bash
WINIT_X11_SCALE_FACTOR=2 <바이너리>
```

## 걸렸는지 무엇으로 아는가 — 신호를 먼저 정한다

**"적용됐는데 효과가 없다" 와 "적용 자체가 안 됐다" 는 다른 결론이다.** 하나만 보면
갈리지 않으므로 두 신호를 함께 읽는다. 둘 다 `debug.fullscreen.state`(debug 빌드 전용)
가 낸다.

| 신호 | 답하는 질문 | 배율 1 | 배율 2 |
|---|---|---|---|
| `monitor.scale_factor` | winit 이 배율을 **받았는가** | `1.0` | `2.0` |
| `inner_size`(물리 px) | 그것이 **효과를 냈는가** | `1280×720` | `2560×1440` |
| `monitor.size` | (대조) X 화면 자체는 안 변해야 한다 | 화면 그대로 | 화면 그대로 |

```bash
tasty debug fullscreen state --window-id <ID>
```

`inner_size` 는 winit `Window::inner_size()` 이므로 **물리 픽셀**이다. 창의 논리 크기는
배율과 무관하게 같으므로, 물리 크기가 배율만큼 커지는 것이 "효과가 났다" 의 뜻이다.
`monitor.size` 가 함께 변하면 배율이 아니라 화면 설정이 바뀐 것이다.

## 실행 절차

1. **gui 조합으로 다시 빌드한다.** `cargo test --no-default-features` 를 돌린 적이
   있으면 `target/debug/tasty` 가 헤드리스본으로 덮여 있다. `cargo build --bin tasty`.
2. **바이너리가 맞는지 관측 가능한 값으로 확인한다.** `--version` 으로는 두 조합이
   안 갈린다. `tasty list windows` 의 **창 수가 1 이상**인 것으로 확인한다 — 헤드리스본은
   창을 만들지 않는다.
3. **격리 `TASTY_HOME` 으로 띄운다.** 사용자 인스턴스를 건드리지 않기 위해서다.
   화면 크기는 아래 "화면이 창보다 커야 한다" 를 따른다.
4. **CLI 환경에서 부모의 `TASTY_*` 를 전부 뗀다** — 특히 `TASTY_SESSION_TOKEN`.
   그것은 사용자 인스턴스의 토큰이라 격리본이 거부하고, 증상은
   `permission_denied: session_token unknown/expired/revoked` 다. 배율과 무관한 실패라
   여기서 막히면 원인을 엉뚱한 데서 찾게 된다.
5. **배율 없이 한 번, 배율을 걸고 한 번** 띄워 위 표의 신호를 각각 읽는다.
6. 대상 화면(webview·banner·popup mesh·탭바 높이·네이티브 메뉴 좌표)의 좌표·크기를
   두 배율에서 비교한다. 캡처 방법과 Xvfb 함정은
   [screenshot-methods](screenshot-methods.md) 를 따른다 — X11 캡처는 검게 나오므로
   GPU 경유 `screenshot --window` 를 쓰고, 캡처 전에 포인터를 한 번 움직여 재렌더를
   유발한다.
7. 정리는 **저장한 PID** 로 한다. `xvfb-run` 을 쓰면 `$!` 는 래퍼이므로, 안의 프로세스는
   `/proc/<pid>/environ` 의 격리 `TASTY_HOME` 으로 찾는다. 패턴 매칭으로 죽이지 않는다.

## 화면이 창보다 커야 한다

배율 2 에서 창의 물리 크기는 `2560×1440` 이다. 화면이 `1600x1200` 이면 창이 화면을
넘어 잘리고, 그 잘림이 배율 결함처럼 보인다. **화면을 창의 물리 크기보다 크게 잡는다**
(예 `3200x2400`). 화면을 키워도 `inner_size` 는 `2560×1440` 로 같다 — 창 크기는 화면이
아니라 논리 크기와 배율이 정한다.

## 알려진 실측값

기준 화면 `1600x1200`(6번 항목의 비교에는 `3200x2400`), 기본 창.

| 조건 | `scale_factor` | `inner_size` | 창 수 |
|---|---|---|---|
| 지정 없음 | `1.0` | `1280×720` | 1 |
| `WINIT_X11_SCALE_FACTOR=2` | `2.0` | `2560×1440` | 1 |

두 신호가 **함께** 움직이므로 배율이 적용됐고 효과도 났다. 하나만 움직였다면 그
자체가 결함 신호다.

## 관련

- [screenshot-methods](screenshot-methods.md) — 캡처 수단과 Xvfb 함정
- [visual-verification](visual-verification.md) — 시각 판정 휴리스틱
- [`docs/concepts/typed-length.md`](../concepts/typed-length.md) — 이 검증이 지키려는 정책
- [`docs/adr/0145-typed-length-constructors-stay-open-for-now.md`](../adr/0145-typed-length-constructors-stay-open-for-now.md) — 정적 축의 현재 결정
