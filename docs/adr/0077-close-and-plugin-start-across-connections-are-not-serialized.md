# ADR-0077: 다른 연결의 닫기와 plugin 기동은 순서를 맞추지 않고 새 프로세스에서 회수한다

- **Status**: Accepted
- **Date**: 2026-10-07
- **Tags**: close, retirement, ipc, plugin, ordering
- **Group**: foundation

## Context

remote·webview surface 를 연 채 plugin 이 다시 뜨면 호스트는 남은 surface 를 새 프로세스에 다시 게시한다.
같은 surface 의 닫기와 `plugin.enable` 이 서로 다른 IPC 연결로 거의 동시에 오면 둘 중 어느 쪽이 먼저 처리될지 정해져 있지 않다.
IPC 서버는 연결마다 스레드가 요청 줄을 읽어 한 명령 큐에 넣고, accept 루프는 대기 중일 때 100ms 마다 확인한다.
그래서 클라이언트가 닫기를 먼저 보냈어도 enable 이 먼저 처리될 수 있다.

[ADR-0074](0074-close-receipt-wait-does-not-pause-observation.md) 는 닫기를 접수한 뒤 처리되는 enable 이 닫는 surface 를 다시 게시하지 않게 한다.
남은 경우는 enable 이 닫기 접수보다 먼저 처리되는 경우다.
이때 닫힌 surface 의 인스턴스가 새 plugin 프로세스에 남으면 안 된다.

## Decision

서로 다른 연결로 온 요청 사이의 처리 순서는 맞추지 않는다.
enable 이 닫기 접수보다 먼저 처리되면 그 시점에 살아 있는 surface 를 새 프로세스에 게시한다.
뒤이은 닫기는 surface 의 게시 상태가 새 세대를 가리키므로 새 프로세스에 `surface.destroy` 를 보내고, 그 응답을 회수 증거로 삼아 확정한다.
plugin SDK 는 호스트 요청을 워커 스레드 하나에서 받은 순서대로 처리하므로 게시와 파괴의 순서가 뒤바뀌지 않는다.

이 경로는 사용자의 포커스·선택·닫은 항목 기록을 바꾸지 않는다. 닫기는 요청한 대상만 회수한다.
현재 동작은 [닫기 시퀀스](../architecture/close-sequence.md)에 있다.

## Consequences

닫힌 surface 는 enable 과의 처리 순서와 관계없이 새 프로세스에 남지 않는다.
enable 이 먼저 처리된 경우에는 새 프로세스가 그 surface 를 잠깐 열었다가 파괴 요청으로 닫는다.
그 사이 plugin 이 호스트에 보낸 호출은 이미 사라진 surface 를 가리켜 실패할 수 있다(예: markdown 의 `webview.set_url` 경고 로그).

파괴 요청이 큐 포화 등으로 전달되지 않거나 응답이 오지 않으면 닫기 결과는 성공으로 단정하지 않고 `Uncertain` 으로 남는다. 이 처리는 ADR-0074 의 receipt 규칙을 따른다.

## Alternatives Considered

- **같은 surface 에 대한 요청을 보낸 순서로 정렬**: 서버가 아는 것은 처리 큐에 들어온 순서뿐이다. 두 클라이언트 사이에는 인과 관계가 없어 "보낸 순서" 를 정의할 수 없다. 정렬하려면 enable 을 앞으로 올지 모를 닫기 뒤로 미루는 지연 정책이 필요하고, 이는 다른 에이전트 요청의 지연을 늘린다. receipt 대기 동안에도 관측을 멈추지 않는다는 ADR-0074 와도 방향이 반대다.
- **enable 처리 중 닫기 요청을 막거나 거절**: 닫기를 보낸 에이전트가 다른 에이전트의 plugin 조작 때문에 실패를 받게 되고, 재시도 책임이 호출자에게 넘어간다.

## Reconsideration Triggers

코드와 설정에서 확인:
- plugin SDK 가 호스트 요청을 여러 워커에서 병렬로 처리하게 되면 게시와 파괴의 순서 보장이 사라진다. `tasty-plugin-sdk` 의 `runtime::run` 워커 구성을 확인한다.
- 닫기가 게시 상태와 관계없이 옛 세대 회수만으로 확정하게 바뀌면 새 프로세스에 인스턴스가 남는다. `a_close_after_republication_destroys_the_instance_in_the_new_process` 시험이 이 경로를 지킨다.

실행 결과로 확인:
- 격리 debug 인스턴스에서 markdown surface 를 닫으면서 별도 연결로 `plugin.enable` 을 보낸 뒤, 새 프로세스의 문서 맵에 그 surface 가 남는지 잰다. 남는 경우가 생기면 다시 검토한다.

## References

- [ADR-0074](0074-close-receipt-wait-does-not-pause-observation.md)
- [닫기 시퀀스](../architecture/close-sequence.md)
- [플러그인 개발](../dev-guide/plugin-development.md)의 Surface kind 절
- `crates/tasty-host-plugin/src/manager/pump/retirement.rs`, `crates/tasty-host-plugin/src/manager/reattach.rs`
