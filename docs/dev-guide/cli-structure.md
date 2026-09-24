# CLI 크레이트 내부 구조 — 세 갈래 대칭

`crates/tasty-cli` 는 명령 하나를 **세 갈래**로 나눠 다룬다. 새 CLI 명령을 추가할 때
어디를 고칠지는 이 표가 정한다.

| 디렉토리 | 답하는 질문 | 들어가는 것 | 들어가면 안 되는 것 |
|----------|-------------|-------------|---------------------|
| `commands/` | **무엇을 받나** | clap `Subcommand`/`Args` 선언, 도움말 문구(영어) | 실행 코드 (`pub fn` 은 `ValueEnum` 의 `as_str` 같은 선언 보조만), SSH·스트림·원격 계층 참조, 한국어 `///` |
| `request/` | 단발 RPC 면 **어디로** | `Commands` → JSON-RPC method/params 변환 | 통신 자체 |
| `local/` | 클라이언트 주도면 **무엇을** | `ClientCommand` 구현 + 그 실행부 | clap 선언 |

갈래 판정은 `dispatch.rs` 하나가 한다.

## 갈래 판정 (`dispatch.rs`)

```rust
pub enum Dispatch<'a> {
    Rpc,                                    // 단발 JSON-RPC — 보내고 응답 출력하면 끝
    ClientDriven(Box<dyn ClientCommand + 'a>),  // client가 실행 순서를 관리한다
}
```

분류 축은 하나다: **`request/` 가 만든 단발 JSON-RPC 하나로 끝나는가.**
아니면 전부 `ClientDriven` 이다 — 로컬 파일/프로세스 조작(`tasty port`,
`tool passkey`), raw 스트림(`remote attach`), 폴링 루프(`plugin audit-follow`),
SSH 터널 경유 조회(`remote workspaces`)가 여기 속한다. 용어 정의는
[ubiquitous-language.md](../concepts/ubiquitous-language.md) 의 "CLI 명령 갈래" 절.

**"로컬(local)" 은 통신 유무가 아니라 주도권을 뜻한다** — 이 갈래의 절반은 IPC 를
(여러 번) 탄다. variant 를 `Local` 로 부르지 않는 이유이기도 하다.

`Dispatch` 는 **명령을 빌린다**(`Dispatch<'a>`). 소유 형태로 만들면 clap enum 들에
`Clone` 을 새로 달아야 해서, 리팩터 편의로 다른 크레이트의 공개 표면이 넓어진다.
`Rpc` 가 요청을 담지 않는 것도 같은 이유다 — 진입점이 원 명령을 계속 들고 있다.

## 새 명령 추가 절차

1. `commands/` 에 clap 선언을 추가한다.
2. 단발 RPC 면 `request/` 에 변환을 추가한다. 진입점(`run.rs`) 수정은 필요 없다.
3. 클라이언트 주도면 `local/` 에 실행 모듈 + `ClientCommand` 구현을 추가하고,
   `dispatch.rs` 의 `classify` 에 arm 하나를 더한다. 여기서도 `run.rs`는 수정하지 않는다.

인자 조합 검증(`--ssh` + `--profile` 상호배타 등)은 `classify` 에서 끝낸다 —
검증 실패는 통신을 시작하기 전에 나야 한다.

## 도움말 문구

`commands/` 의 `///` doc comment 는 코드 주석이 아니라 **사용자에게 표시되는 도움말**이다 —
clap 이 첫 문단을 짧은 help(`-h`), 전체를 긴 help(`--help`)로 그대로 노출한다.
따라서 clap 항목(variant · `#[arg]` 필드) 의 `///` 는 영어로만 쓴다
([i18n.md](i18n.md) "하드코딩 허용 예외"). 값 허용 범위·상호배타 같은 사용자에게
필요한 상세는 긴 help 에 남기고, 설계 근거(불가침 원칙 번호, 내부 단계명 등)는
`//` 주석이나 `docs/` 로 내린다.

`clap_help_text_is_english_only`
(`crates/tasty-doc-guards/tests/no_hardcoded_ui_strings.rs`)는 `commands/`와
`crates/tasty-cli/src/lib.rs`의 `///`에 CJK 문자가 있는지 검사한다. clap 항목인지까지
구분하지 않으며, `#[cfg(test)]` 안의 코드와 그 앞 doc 주석만 제외한다.
모듈 설명인 `//!`는 대상이 아니다. 이 파일들의 clap과 무관한 설명은 필요할 때만
`//`로 짧게 남긴다.

실제 표시는 `help_i18n::command`로 얻은 번역 트리와 `help_frame` 템플릿을 사용한다.
`help_error`는 clap의 구조화된 파싱 오류를 표시하며, plugin 동적 파싱도 같은 경로로
들어간다. plugin 번역은 discovery에서 공용 카탈로그를 읽는다.
상세: [국제화](i18n.md#cli-도움말-clap-about--help), [ADR-0040](../adr/0040-locale-catalogs-and-display-text.md).

### 빈 설명 칸은 없다

모든 하위 명령에 about, 모든 인자에 help를 작성한다.
`crates/tasty-cli/tests/help_i18n_slots.rs`의 `every_subcommand_carries_an_about`와
`arguments_without_help_do_not_increase`가 clap 트리를 검사하므로 다른 크레이트의
명령(`tasty-tui-simulator`의 `debug sim` 등)도 포함한다.

현재 빈 설명의 허용 개수는 0이다. 검사는 허용 개수보다 많거나 적어도 실패하도록
작성돼 있어, 예외를 줄였을 때 허용값도 함께 줄여야 한다.

`///`는 바로 아래 항목에 붙으므로 선언을 끼워 넣을 때 설명의 대상이 바뀌지 않는지
확인한다. 검사는 설명이 비었는지만 확인하며 내용의 정확성은 판단하지 않는다.
기본값·누적/교체 방식·플랫폼 차이는 실행 코드를 읽고 설명하고, 같은 이름의 옵션도
각 명령에서 실제로 무엇을 뜻하는지 적는다.

## 진입점 (`run.rs`)

`run_client` 는 갈래를 묻고 `ClientDriven` 이면 넘긴 뒤, 나머지 단발 RPC 경로
(요청 매핑 → 포트 파일 읽기 → 연결 → 계약 확인 → 전송 → 출력, `auto_wait` 폴링 포함)만 직접 수행한다.

**요청 매핑(`request/`)은 연결보다 먼저다.** 매핑은 서버 없이 끝나는 검증(깨진 JSON 인자 ·
없는 `--cwd` 등)이라, 실패하면 그 자리에서 원인을 stderr에 출력하고 종료한다 — 인스턴스가 없어도
사용자는 "실행 중인 인스턴스가 없다" 가 아니라 자기 인자의 잘못을 받고, 호스트에는 아무것도
안 보낸 빈 연결이 생기지 않는다. 서버의 값이 있어야 하는 확인(계약 확인 `contract::ensure`)만
연결 뒤에 남는다. plugin 동적 명령(`try_run_plugin_cli`)도 같은 순서다. 그래서 인스턴스가 없을
때의 종료 코드도 그 인자 오류의 값이다(근거·대안은
[ADR-0043](../adr/0043-cli-errors-and-diagnostic-logs.md)).
시험은 `tests/cli_maps_args_before_connecting.rs`.

인자 오류의 코드는 해당 검사에서 정한 1 또는 2이며 서버 부재 때문에 1로 덮지 않는다.
`preset save --file -`는 연결 전에 stdin을 읽고, memory TTL의 expires_at도 요청 매핑
시점에 계산한다. 매핑에서는 서버를 조회하지 않는다.

## stdout 출력 (`out.rs`)

일반 CLI 출력은 `outln!`·`out!`·`flush`·`from_io`를 사용하고 오류를 Result로 전달한다.
`println!`·`print!`는 쓰기 실패를 panic으로 바꾸므로 사용하지 않는다.
BrokenPipe는 `StdoutClosed`로 구분해 CLI 진입점의 `quiet_if_stdout_closed`가 종료 코드 0으로
처리한다. 다른 I/O 오류는 오류 보고와 종료 코드 1로 끝낸다. 이 처리는 호스트에 적용하지 않는다.

출력 도중 즉시 process::exit하지 않고 오류를 전파해야 SSH 터널 등 Drop 정리가 수행된다.
폴링 명령은 다음 실제 출력 때 닫힌 파이프를 감지한다. 출력할 데이터가 없으면 빈 flush만으로
닫힘을 알 수 없어 기다릴 수 있다. raw attach bridge는 best-effort로 화면을 미러하므로
stdout 닫힘을 세션 detach로 승격하지 않는 별도 경로다.

stderr에는 `errln!`을 사용한다. 쓰기 실패는 더 보고할 채널이 없으므로 무시하지만,
원래 명령의 실패 1·파싱 오류 2 등 종료 코드는 유지한다. 실제 버그의 panic hook은 유지한다.
관련 시험은 `tests/cli_stdout_broken_pipe.rs`다.

## 호스트 오류 출력 (`rpc_error.rs`)

단발 RPC·동적 plugin 명령·auto_wait·폴링·계약 확인 오류는 `rpc_error::exit_with`를 사용한다.
첫 줄은 `Error (<code>): <message>`, 값이 있는 error.data는 둘째 줄
`data: <한 줄 JSON>`으로 그대로 출력하고 종료 코드 1로 끝낸다.
data가 없거나 null이면 둘째 줄을 출력하지 않는다. `data: ` 접두사는 번역하지 않는다.

`events follow`와 `plugin audit-follow`는 오류를 main으로 전달하므로 첫 줄의
`Error: Error (…)` 접두사를 유지한다. 이 경로에는 `with_data_line`을 사용해 둘째 줄을 더한다.
`JsonRpcCallError::Display` 자체는 바꾸지 않아 다른 진단 문구의 형식을 유지한다.
새 오류 경로도 직접 출력하는지 main으로 올리는지에 맞춰 두 함수를 선택한다.

## CLI와 IPC의 접근성

`METHOD_TABLE`과 `DEBUG_METHODS`의 각 메서드는 CLI 진입점이 있거나, 없는 이유가 있어야 한다.
응답이 플러그인 자신의 신원·설정·이벤트 수신처를 요구하면 일반 셸 명령으로 재현할 수 없다.
그 밖에 ID로 대상을 지정하거나 전역 상태를 읽는 기능은 CLI를 제공한다.
사유 표는 [API 규약](api-conventions.md)에 한 번만 관리한다.

`tests/cli_method_table_parity.rs`는 요청 매핑의 메서드, 명시적 method 필드,
번들 plugin 매니페스트의 ipc_method를 읽어 표와 양방향 대조한다.
플래그 뒤의 호출이나 통신하지 않는 클라이언트 명령도 있으므로 이름이나 네트워크 요청
개수만으로 진입점 부재를 단정하지 않는다. 문자열 추출은 실제 실행의 완전한 증명이 아니며
의심되는 항목은 요청 매핑과 실행 경로를 확인한다. 대안으로 안내한 CLI 명령도 실제 있어야 한다.

## 호스트 로그와 CLI 진단

공유 tracing 파일은 호스트로 확정된 뒤 `enable_host_file_log`에서 연다.
GUI와 headless가 대상이며, CLI는 stderr만 사용해 호스트 파일을 건드리지 않는다.
panic hook과 stderr tracing은 main 시작에 초기화해 동적 CLI 라우팅 중 오류도 남긴다.

host 파일은 실행할 때마다 비우므로 재시작을 넘는 영구 기록이 아니다.
CLI가 성공한 IPC 앞뒤에서 남긴 일반 warning은 hook-failures.log의 대상이 아닐 수 있다.
예를 들어 stdin JSON 파싱 경고는 stderr에서 확인한다.
파일 위치와 빌드별 필터는 [크래시 진단](crash-diagnostics.md)을 따른다.

## 새 워크스페이스의 윈도우와 작업 경로

`tasty new workspace --surface <숫자 ID>`는 그 서피스의 윈도우를 선택한다.
없는 ID는 거절하고 다른 윈도우로 대체하지 않는다. nickname·this나 `--window`는 받지 않는다.
생략하면 `TASTY_SURFACE_ID`를 적용하지 않고 기존의 대상 없는 요청 규칙을 따른다.
여러 윈도우를 제어하는 에이전트는 명시적으로 서피스를 지정한다.

terminal 워크스페이스의 cwd는 명시 `--cwd`가 우선이다. 생략하고 inherit_cwd가 켜져 있으면
지정한 서피스의 로컬 cwd를 상속한다. 서피스까지 생략하면 선택된 윈도우의 포커스 서피스에서
상속하는 기존 동작이다. 상속을 끄거나 원본이 mirror여서 로컬 cwd가 없으면 홈을 사용한다.
IPC의 숫자가 아닌 surface_id는 invalid_params다. 윈도우 선택은 라우터가, cwd 선택은
workspace 핸들러의 `resolve_create_cwd`·`inherit_cwd_for_create`가 맡는다.

## `debug` 갈래

`commands/debug.rs`(선언)와 `local/debug.rs`(실행) 둘 다 모듈째
`#![cfg(debug_assertions)]` 다. 사용자 입력 재현은 release 표면에 없다 —
[debug-ipc.md](debug-ipc.md), [identity.md](../identity.md) 원칙 1.

## 관련

- [api-conventions](api-conventions.md) — CLI/IPC 명명 + 안정성 정책
- [build](build.md) — 크레이트 **경계**(이 문서는 크레이트 **내부**)
- [attach-behavior](attach-behavior.md) — `local/attach.rs` 의 attach 세션 머신

## 에이전트 훅 전달 실패 기록

CLI는 포트 파일 부재, 연결 실패, JSON-RPC 오류를 접속 대상의
`<tasty_home>/hook-failures.log`에 기록한다. `TASTY_PARENT_HOME`은 접속 대상 홈을
정하지 않으므로 이 기록 위치에 사용하지 않는다. 마지막 메서드 이름이 `hook` 또는
`_hook`으로 끝나는 요청만 기록한다. 일반 대화형 명령은 stderr로 오류를 알린다.

레코드는 `<UTC> method=… event=… surface=… code=… reason=…` 한 줄이다.
이벤트나 JSON-RPC 코드가 없으면 `-`를 쓰며, 공백을 담는 `reason`은 마지막에 둔다.
실패 분류에는 앞의 필드를 사용한다. `reason`은 응답을 만든 쪽의 언어를 따르므로
plugin이 번역한 메시지도 올 수 있다. `DiagnosticEnglish`와 관련 소스 검사는 CLI가
직접 만드는 포트·연결 오류의 영어 진단만 보호하며 응답 메시지 전체를 보장하지 않는다.

정상 호출은 기록하지 않는다. 파일이 256 KiB에 이르면 `.log.1` 한 개로 교체하며
기록·로테이션 실패가 훅 실행을 추가로 실패시키지는 않는다. 따라서 크기는 엄격한 상한이
아니며 파일 권한이나 디스크 문제로 기록이 누락될 수도 있다. loopback 연결에는 3초 상한을 둔다.

설치 래퍼는 `TASTY_SURFACE_ID`가 없으면 호출하지 않는다. POSIX 래퍼의 `|| true`는
전달 실패가 외부 에이전트 턴을 막지 않도록 한다. 실패 상태 자체는 로그로 확인한다.
설치 명령을 바꾸면 기존 사용자 설정에도 다시 설치해야 하며, 같은 matcher 안의 사용자
핸들러를 보존하면서 Tasty 항목만 갱신하는지 확인한다.
