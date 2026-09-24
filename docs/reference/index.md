# 레퍼런스 (Reference)

IPC·이벤트·출력 파서의 이름과 데이터 형식을 찾는 참고 자료다. 기능의 목적과 동작은 [features/](../features/index.md)에서 설명한다.

| 문서 | 내용 | 구현 기준 |
|------|------|----------|
| [api.md](api.md) | 주요 IPC/CLI — 네임스페이스별 메서드·권한·시스템/윈도우 관측 범위 | `crates/tasty-ipc/src/method_meta.rs` |
| [event-catalog.md](event-catalog.md) | Event Bus 1.0 wire 계약 (plugin 공개 API) | `tasty_plugin_protocol::events` |
| [output-parsers.md](output-parsers.md) | 터미널 출력 파서 카탈로그 | `tasty-output` |
| [environments.md](environments.md) | OS별 경로·에이전트 실행 준비 | — |
| [plan.schema.json](plan.schema.json) | 공유 컨텍스트 Plan 의 JSON Schema (memory 키 `tasty.plan.<id>`) | `crates/tasty-memory/src/plan.rs` |

> 메서드 시그니처·권한의 *정답*은 항상 코드(`method_meta.rs` / 각 핸들러)다. 이 문서들은 사람이 읽기 위한 요약이며, 충돌 시 코드를 따른다.
