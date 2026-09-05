# AI 자체 검증 지침

tasty 를 개발하는 AI 에이전트가 UI/렌더링/입력을 **스스로 재현·검증** 하는 방법.

| 문서 | 내용 |
|------|------|
| [visual-verification](visual-verification.md) | 시각 변경 체크리스트 + 스크린샷 판단 휴리스틱 |
| [screenshot-methods](screenshot-methods.md) | `tasty screenshot` / `ui.screenshot`(focus 독립, surface/window ID) vs OS 캡처, 격리 실행, 전후 diff 판정의 양성 대조·노이즈 바닥 |
| [ipc-usage](ipc-usage.md) | IPC 로 조작·검증 + `\r`/`read_line` 함정 + 실 PTY 로 대화형 작업 수행 |
| [dpi-scale-verification](dpi-scale-verification.md) | `WINIT_X11_SCALE_FACTOR` 로 DPI≠1 재현 + 배율이 걸렸는지 가르는 두 신호 |
| [ime-testing](ime-testing.md) | `surface.ime_*`(debug 전용) 로 한글/CJK 입력 시뮬레이션 |

> 검증은 커밋 전 직접 수행한다([dev-guide/self-verification](../dev-guide/self-verification.md)). 개발용 격리는 [dev-guide/independent-verification](../dev-guide/independent-verification.md).

## 이 문서군의 절차 중 무엇이 자동화 대상인가

"검증 절차에 자동 실행 채널이 없다" 를 결함으로 세기 전에 **단위를 정해야 한다.**
문서 단위로 세면 다섯 문서 전부가 "판정이 사람" 으로 보이지만, 절차 단위로 가르면
사람의 판정이 답인 것은 소수다. 아래는 그 갈래다.

**세는 단위**: 문서가 *수행하라고 지시하는 최소 단위* — 번호 목록의 한 항목, 또는
번호가 없는 `###` 절차 절 안의 한 단계. 사실 서술(무엇을 캡처할 수 있는가), 배경 설명
(왜 되는가), 참조표(알려진 실측값)는 절차가 아니라 세지 않는다. 최상위 번호 항목만
기계로 세는 명령은 아래 "재는 명령" 에 있다 — 그것이 이 표의 하한이다.

| 문서 | 절차 | 판정이 사람 | 전제·함정 회피 | 판정이 기계적 |
|---|---|---|---|---|
| [visual-verification](visual-verification.md) | 8 | 5 | 1 | 2 |
| [screenshot-methods](screenshot-methods.md) | 23 | 0 | 14 | 9 |
| [ime-testing](ime-testing.md) | 15 | 4 | 0 | 11 |
| [dpi-scale-verification](dpi-scale-verification.md) | 11 | 0 | 6 | 5 |
| [ipc-usage](ipc-usage.md) | 2 | 0 | 2 | 0 |
| 합 | 59 | 9 | 23 | 27 |

(`ipc-usage` 의 나머지 둘 — "적용 예" · "제약" — 은 검증 절차가 아니라 실 PTY 로 대화형
작업을 수행하는 방법이라 이 표 밖이다.)

세 갈래가 서로 다른 것을 말한다.

- **판정이 사람 (9)** — 결론이 지각·의미 판단이다. "preedit 오버레이가 셀 그리드에
  정렬됐는가", "전체를 훑지 말고 변경 영역만 봐라", "불확실하면 모르겠다고 말해라".
  자동 채널이 없는 것이 **결함이 아니라 성질**이다([ci-gates](../dev-guide/ci-gates.md)
  "자동 채널이 없는 것이 결함이 아닌 갈래").
- **전제·함정 회피 (23)** — 판정이 아니라 **측정을 유효하게 만드는 준비**다. Xvfb 의
  Xauthority 상속, 창 id 를 크기로 고르지 않기, 측정 전 plugin 바이너리 최신 확인,
  저장한 PID 로만 정리. 이쪽은 자동화의 대상이 아니라 **틀리면 앞의 판정을 통째로
  무효로 만드는 지식**이고, 그래서 절차 텍스트의 최대 덩어리다.
- **판정이 기계적 (27)** — 종료 코드나 수치가 답을 준다. 그중 둘은 이미 테스트가 있다
  (`tests/gui_tests.rs` 의 `test_ime_preedit_flushed_on_non_popup_shortcut` ·
  `test_ime_preedit_cleared_on_popup_focus_shortcut`). 그 둘이 자동으로 안 도는 것은
  이 문서군의 문제가 아니라 **`gui_tests` 에 자동 채널이 없다**는 별개 축이다
  ([ci-gates](../dev-guide/ci-gates.md) "사람이 돌리는 것").
  **다만 그 둘은 "돌리면 기계가 답한다" 까지 가지 못한다** — 그 스위트를 손으로 돌려도
  지금은 끝까지 안 간다(막는 것 셋을 그 절에서 쟀다). 그러니 이 갈래에 있다는 것은
  *판정의 성질*이 기계적이라는 뜻이지 오늘 답이 나온다는 뜻이 아니다.

## 기계가 볼 수 있는 경계 — 실재는 보이고, 돌았는지는 안 보인다

문서가 "이 명령을 돌려라" 라고 적으면 **그 명령이 실재하는지**는 기계가 본다. 반면
**누가 그것을 실제로 돌렸는지**는 못 본다 — 절차의 실행은 git 에도 CI 로그에도 흔적을
남기지 않는다. 위 표의 어느 갈래도 이 경계를 바꾸지 않는다: "판정이 기계적" 인 27 개도
*자동으로 도는 것*이 아니라 *돌리면 기계가 답하는 것*이다.

**그 실재 쪽에 관측자를 두지 않았다 — 사는 것이 0 이기 때문이고, 그 0 은 재서 나왔다.**

- 이 문서군이 인용하는 CLI 명령 13 형태와 IPC 메서드 13 개는 오늘 **전부 해소된다.**
- 문서 최초 커밋(2026-03-25) 이후 이 문서군 또는 IPC/CLI 소스를 건드린 **489 리비전**을
  걸어 각 시점의 인용 메서드가 그 시점 소스에 있었는지 봤다. 참조 집합이 비지 않은
  487 리비전 중 **죽은 참조가 있던 리비전은 0** 이다.
- 그 0 이 계측기 탓이 아님을 [양성 대조](screenshot-methods.md)로 확인했다 — 같은
  창에서 `method_meta.rs` 의 이름 **18 개가 실제로 사라졌다**(345 중 327 생존). 잡을
  것이 있었는데 이 문서군은 하나도 인용하지 않았다.

**그리고 관측자를 만들었다면 위양성이 지배했을 것이다.** 문서 전체의 백틱 `tasty …`
인용 193 형태 중 해소 안 되는 것이 11 인데, 그중 **9 는 옳은 인용**이다 — 그 명령이
없다는 것이 문장의 주어인 자리(제거를 기록한 ADR 등). 즉 술어의 정밀도가 2/11 이다.
남은 2 를 고친 것이 이 문서군 밖의 부수 결과다. 메서드 축도 같다: `markdown.recent` 는
host 표에 없지만 plugin 이 제공해 살아 있고, `fs.pick_file` · `script.reload` 은 죽었지만
문서가 **죽었다고** 적은 것이다. 셋 다 "이 이름이 표에 있나" 로는 안 갈린다.

이 갈래는 [cited_coordinates_exist](../../crates/tasty-doc-guards/tests/cited_coordinates_exist.rs)
가 자기 판정 규칙으로 이미 거절한 형태다 — 그 가드는 경로만 본다. 경로는 **스스로 좌표를
들고 있어** 산문을 안 읽어도 풀리지만, 명령·메서드 이름은 어느 네임스페이스인지와 문장이
"제거됐다" 라고 말하는지에 답이 달려 있다. 산문 문맥을 추론하는 술어는 양방향으로 틀리면서
초록일 때 아무것도 보장하지 못한다.

### 재는 명령

    # 최상위 번호 절차 항목 (위 표의 하한)
    grep -c '^[0-9]\+\. ' docs/ai-verification/*.md

    # 인용된 CLI 형태 — 각각을 `target/debug/tasty <형태> --help` 로 해소해 본다
    grep -rhoE '`tasty [a-z][a-z0-9-]*( [a-z][a-z0-9-]*)*' docs/ai-verification/*.md

    # 인용된 IPC 메서드 — 소스에 문자열로 있는지
    grep -rhoE '`[a-z][a-z_]+(\.[a-z][a-z_]+)+`' docs/ai-verification/*.md
