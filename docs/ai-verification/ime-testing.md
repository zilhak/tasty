# IME 시뮬레이션 검증

`surface.ime_*` IPC(window-local, **local-only** — 로컬 caller 만, `crates/tasty-ipc/src/method_meta.rs::PREFIX_RULES`)로 IME 입력을 프로그래밍 방식으로 시뮬레이션해 한글/CJK 입력 파이프라인 버그를 재현·검증한다. 핸들러는 `src/adapters/ipc/handler/ime.rs`. (정상 모드 포트 `~/.tasty/tasty.port`.)

메서드: `surface.ime_enable` · `surface.ime_preedit {text}` · `surface.ime_commit {text}` · `surface.ime_status` · `surface.ime_disable`.

```python
# call() = ipc-usage.md 의 헬퍼 (read_line 사용)
call("surface.ime_enable")
call("surface.ime_preedit", {"text": "ㅎ"}); time.sleep(0.1)   # 조합 단계
call("surface.ime_preedit", {"text": "한"}); time.sleep(0.1)
call("surface.ime_commit",  {"text": "한"})                    # 확정 → PTY 전송
call("surface.ime_status")   # {"active": true, "preedit_text": null, "has_preedit": false}
call("surface.ime_disable")
```

## 검증 시나리오

1. **Preedit 렌더링 위치** — 터미널에 텍스트 입력 후 `ime_enable`+`ime_preedit "한"` → [스크린샷](screenshot-methods.md)으로 preedit 오버레이가 커서 위치에 파란 배경으로 셀 그리드 정렬되는지.
2. **연속 커밋 후 위치 이동** — `preedit "한"`→`commit "한"`→`preedit "글"` → preedit 이 오른쪽으로 이동하는지. (셸 에코 처리 전 다음 preedit 시작 시 커서 미갱신 — `sleep 0.1`.)
3. **IME 활성 중 ASCII** — ASCII 는 `surface.send` 로(KeyboardInput 통과), 한글은 IME 경로 → `tasty read since-mark --strip-ansi` 로 결과 확인.
4. **분할 패널** — `tasty split --level surface --target-surface this --direction horizontal` 후 오른쪽 surface 에서 preedit 위치 검증.

## 검증 체크리스트

- [ ] `ime_enable` → `ime_status` 가 `active: true`
- [ ] `ime_preedit "한"` → `preedit_text: "한"`, `ime_preedit ""` → 클리어
- [ ] `ime_commit "한"` → PTY 전송(`read since-mark`)
- [ ] `ime_disable` → `active: false` + preedit 클리어
- [ ] 연속 preedit→commit 시 텍스트 누적
- [ ] 스크린샷에서 preedit 오버레이 위치·파란 배경 정렬

## 제한

- 윈도우 레벨 상태(`MainView.ime_active`/`ime_preedit`)를 직접 조작 — OS 실제 IME 엔진 비관여.
- `surface_id` 지정 미지원 — 항상 포커스된 surface.
- 마우스 클릭에 의한 preedit 커밋은 시뮬레이션 불가(별도 경로).
- OS IME 후보창 위치(`set_ime_cursor_area`)는 호출되나 실제 OS IME 는 열리지 않음.
