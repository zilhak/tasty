# ADR-0335: `state.db` 는 GUI 부팅만 열고, 접근자의 `None` 은 뜻이 하나다

- **Status**: Accepted
- **Date**: 2026-09-20
- **Tags**: storage, headless, ownership, naming

## Context

`~/.tasty/` 아래에 SQLite 가 둘 있다 — `state.db`(최근 파일·튜토리얼 진행)와
`memory.db`(에이전트 메모리). 둘은 연결 pragma 를 거는 함수 하나만 공유하고 나머지는
별개인데, 코드와 문서가 그 경계를 흐리게 적고 있었다.

`src/db.rs` 의 머리말은 접근 규칙을 이렇게 적었다 — "전역 `static` 싱글톤을 통해 **어떤
코드라도** `with_db(|db| ...)` 로 접근 가능". 실측하면 반대다. 이 저장소를 여는 자리는
GUI 부팅 하나뿐이고(`init` 과 `default_db_path` 가 `cfg(feature = "gui")`), 접근자를 부르는
자리는 저장소 두 모듈의 다섯 줄이다. "어떤 코드라도" 는 기술이 아니라 초대였고, 티켓이
이 저장소를 구조 문제로 든 근거이기도 했다.

이름도 경계를 흐렸다. 접근자가 `with_db` 인데 `memory.db` 쪽 접근자는 `with_memory` 다.
드문 쪽이 일반적인 이름을 갖고 있고(호출 수 5 대 218), 두 이름이 같은 파일에 함께 나오는
자리가 하나도 없다. 그래서 `src/store/` 를 여는 사람은 호출부만 보고는 어느 저장소인지
알 수 없고, 잘못 읽어도 아무 신호가 없다.

무엇보다 접근자가 돌려주는 `None` 의 뜻이 어디에도 적혀 있지 않았다. 헤드리스 빌드에서
그 값이 무엇을 의미하는지 재는 채널이 없다는 것이 사전 조사가 열어 둔 칸이었다.

## Decision

`state.db` 를 여는 책임은 **GUI 부팅 하나**로 확정하고, 그 사실을 이 저장소의 계약으로
적는다. 접근자는 `with_state_db` 로 이름을 바꿔 `with_memory` 와 호출부에서 갈리게 한다.
그리고 접근자가 돌려주는 `None` 은 **"아직 열리지 않았다" 한 가지 뜻만** 갖는다고 못
박는다 — 락 poison 은 그 함수가 복구해 `Some` 으로 돌려주므로 여기 오지 않고, GUI 의 열기
실패는 사용자 안내 후 앱 종료로 끝나므로 살아 있는 창에서는 관측되지 않는다. 소비자는 그
`None` 을 오류가 아니라 "이 빌드에는 영속 저장이 없다" 로 읽고 기본값으로 떨어진다.

동작은 바꾸지 않는다. 다섯 호출부는 전부 이미 그렇게 행동하고 있었고, 이 결정은 그 행동에
이름과 계약을 붙인다.

## Consequences

- **얻은 것**: 호출부 한 줄만 보고 어느 저장소인지 갈린다. 헤드리스에서 `None` 을 받은
  소비자가 그것을 오류로 승격시킬 근거가 사라진다. 모듈 머리말이 초대장이 아니라
  경계 서술이 된다.
- **잃은 것**: 접근자 이름이 길어졌다. 그리고 `memory.db` 쪽은 여전히 `with_memory` 라
  "state" 에 대응하는 접두어가 한쪽에만 붙는다 — **접근자 형태만 세도 201 자리**
  (`grep -rn 'with_memory(|' src/ crates/`)라, 옮기는 것은 이 결정의 값에 비해 비싸다고
  봤다. `with_memory(` 로 넓게 세면 218 이지만 그 좌변은 세터(`CoreBuilder::with_memory`)·
  시험 헬퍼·상류 `sysinfo` 의 동명 메서드를 함께 세므로 이 문장이 말하려는 수가 아니다.
- **운영 비용 / 유지 부담**: 이름을 다시 바꾸면 **문서 두 개의 네 줄**이 함께 움직여야
  한다 — `docs/dev-guide/build.md` 두 줄, `docs/design/systems/storage.md` 두 줄. 재는
  법: `grep -rn 'with_state_db' docs/ --include='*.md' | grep -v '^docs/adr/'` → **4**.
  **ADR 은 그 모수에서 뺀다** — 이 문서 자신의 세 줄을 포함해, ADR 의 코드 인용은 결정
  시점의 기록이라 나중 결정이 옮긴 이름을 따라가지 않는다(`docs/adr/template.md` 의 "좌표
  예외"). 이름이 또 바뀌면 그것은 이 결정을 대체하는 새 ADR 이고, 이 문서는 그때의 기록
  으로 남는다. 어느 쪽이든 그 정합을 보는 자동 채널은 없다.

## Alternatives Considered

- **A: 다섯 호출부에 `&mut Db` 를 주입하고 전역을 없앤다** — 사전 조사가 제안한 형태다.
  실측하면 다섯 중 셋은 **이미 주입형**이다(`RecentFiles::for_db(&mut Db)`,
  `tutorial_progress::{load, save}(conn, …)`) — 전역은 가장 바깥 래퍼에만 있다. 남은 둘은
  `RecentFiles::add` 와 `prune_kind` 인데, 그 호출자(`AppState::record_recent`)가 `Db` 를
  들고 있지 않아 인자를 창 상태까지 끌어올려야 한다. 리팩토링 회차의 기본값인 행동 보존
  안에서 할 크기가 아니고, 무엇보다 **전역이 지금 만들고 있는 문제가 관측되지 않았다** —
  `src/core/` 는 이 저장소를 전혀 보지 않는다.
- **B: 헤드리스에서 접근자 자체를 컴파일에서 뺀다** — `None` 을 타입으로 만들어 뜻을
  확정하는 가장 강한 형태다. 그러나 최근 파일 조회는 헤드리스에서도 IPC 로 답해야 하고
  (`recent.query` → 빈 목록), 그 경로가 접근자를 지난다. 빼면 소비자마다 `cfg` 가 갈라져
  한 사실이 여러 자리에 적힌다.
- **C: 이름을 그대로 두고 문서만 고친다** — 오독이 조용하다는 것이 문제의 성질이라,
  호출부에서 안 보이면 문서는 읽히지 않는다.

## Reconsideration Triggers

**채널이 붙는 것** — 판정 시점에 레포가 읽을 수 있는 사실이다.

- `init` 또는 `default_db_path` 에서 `cfg(feature = "gui")` 가 빠지는 것. 그러면 "여는 쪽은
  GUI 부팅 하나" 가 거짓이 되고 `None` 의 뜻도 하나가 아니게 된다.
- `with_state_db` 호출부가 저장소 두 모듈(`src/store/`, `src/adapters/ui/tutorial/`) 밖으로
  나가는 것. 소비면이 넓어지면 대안 A 의 비용 계산이 달라진다.

**원리적으로 안 붙는 것** — 사람이 관측해야 한다. 재는 법을 함께 적는다.

- 헤드리스에서 `None` 을 오류로 승격시키는 소비자가 생기는 것. 재는 법: 격리 `TASTY_HOME`
  으로 `--no-default-features` 빌드를 `--headless` 로 띄우고, 그 홈에 `state.db` 가 안
  생기는 것과 `recent.query` 가 오류가 아니라 빈 목록으로 답하는 것을 함께 본다.

## References

- [저장소 시스템](../design/systems/storage.md) — 두 DB 의 경계와 접근 규칙
- [빌드 경계](../dev-guide/build.md) — 어느 정의가 어느 빌드에 들어가는가
- [ADR-0275](0275-recent-cache-belongs-to-the-state-database.md) — 최근 캐시를 이 DB 가 소유한다는 앞선 결정
- [ADR-0316](0316-a-database-reports-the-pragma-that-took-not-the-one-requested.md) — 두 DB 가 공유하는 유일한 것(연결 pragma)
- 코드 근거(결정이 실현된 현재 위치): `src/db.rs` 의 `with_state_db` 와 모듈 머리말
