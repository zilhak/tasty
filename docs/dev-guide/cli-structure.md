# CLI 크레이트 내부 구조 — 세 갈래 대칭

`crates/tasty-cli` 는 명령 하나를 **세 갈래**로 나눠 다룬다. 새 CLI 명령을 추가할 때
어디를 고칠지는 이 표가 정한다.

| 디렉토리 | 답하는 질문 | 들어가는 것 | 들어가면 안 되는 것 |
|----------|-------------|-------------|---------------------|
| `commands/` | **무엇을 받나** | clap `Subcommand`/`Args` 선언, 도움말 문구(영어) | 실행 코드 (`pub fn` 0개), SSH·스트림·원격 계층 참조, 한국어 `///` |
| `request/` | 단발 RPC 면 **어디로** | `Commands` → JSON-RPC method/params 변환 | 통신 자체 |
| `local/` | 클라이언트 주도면 **무엇을** | `ClientCommand` 구현 + 그 실행부 | clap 선언 |

갈래 판정은 `dispatch.rs` 하나가 한다.

## 갈래 판정 (`dispatch.rs`)

```rust
pub enum Dispatch<'a> {
    Rpc,                                    // 단발 JSON-RPC — 보내고 응답 출력하면 끝
    ClientDriven(Box<dyn ClientCommand + 'a>),  // client 가 흐름을 쥔다
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
2. 단발 RPC 면 `request/` 에 변환을 추가한다. **끝** — 진입점(`run.rs`)은 열지 않는다.
3. 클라이언트 주도면 `local/` 에 실행 모듈 + `ClientCommand` 구현을 추가하고,
   `dispatch.rs` 의 `classify` 에 arm 하나를 더한다. 여기서도 `run.rs` 는 열지 않는다.

인자 조합 검증(`--ssh` + `--profile` 상호배타 등)은 `classify` 에서 끝낸다 —
검증 실패는 통신을 시작하기 전에 나야 한다.

## 도움말 문구

`commands/` 의 `///` doc comment 는 코드 주석이 아니라 **사용자 표면 문자열**이다 —
clap 이 첫 문단을 짧은 help(`-h`), 전체를 긴 help(`--help`)로 그대로 노출한다.
따라서 clap 항목(variant · `#[arg]` 필드) 의 `///` 는 영어로만 쓴다
([i18n.md](i18n.md) "하드코딩 허용 예외"). 값 허용 범위·상호배타 같은 사용자에게
필요한 상세는 긴 help 에 남기고, 설계 근거(불가침 원칙 번호, 내부 단계명 등)는
`//` 주석이나 `docs/` 로 내린다.

★ **면제는 "clap 항목이 아니다" 가 아니라 `#[cfg(test)]` 다.** 이것을 강제하는
`clap_help_text_is_english_only`(`tests/no_hardcoded_ui_strings.rs`)는 스캔 뿌리 안의
`///` 줄에 CJK 가 있으면 **그 주석이 무엇에 붙었는지 보지 않고** 문다 — 걷어내는 것은
`#[cfg(test)]` 아래(속성 앞의 doc 주석까지)뿐이다. 뿌리는 `commands/` 만이 아니라
`crates/tasty-cli/src/lib.rs` 도 포함한다. 실측으로 확인했다: clap 항목이 아닌 `use`
선언 위에 한국어 `///` 를 얹으면 그 자리에서 빨개진다.
모듈 헤더 `//!` 는 접두가 달라 실제로 대상 밖이다.

⇒ clap 과 무관한 배경 설명이라도 이 파일들에서는 `///` 가 아니라 `//` 로 쓴다.

## 진입점 (`run.rs`)

`run_client` 는 갈래를 묻고 `ClientDriven` 이면 넘긴 뒤, 나머지 단발 RPC 경로
(포트 파일 읽기 → 연결 → 요청 → 출력, `auto_wait` 폴링 포함)만 직접 수행한다.

## stdout 출력 (`out.rs`)

세 갈래 모두 stdout 에는 `crate::out` 의 `outln!` / `out!` 로만 쓴다 — `println!` /
`print!` 금지. 쓰기는 `anyhow::Result<()>` 를 돌려주므로 출력 함수는 `-> Result<()>`
이고 호출부는 `?` 로 올린다. 읽는 쪽이 파이프를 먼저 닫아 생기는 `BrokenPipe` 는
`StdoutClosed` 로 올라와 진입점(`run.rs` 의 `run_client` / `try_run_plugin_cli`,
`help.rs` 의 `print_augmented_help` / `print_command_tree`)에서 `quiet_if_stdout_closed`
가 **종료 코드 0** 으로 접는다. 정책·근거는 [error-handling](error-handling.md) "stdout
쓰기" 와 [ADR-0101](../adr/0101-cli-stdout-broken-pipe-exit-zero.md), 강제는
`tests/cli_stdout_broken_pipe.rs`.

## `debug` 갈래

`commands/debug.rs`(선언)와 `local/debug.rs`(실행) 둘 다 모듈째
`#![cfg(debug_assertions)]` 다. 사용자 입력 재현은 release 표면에 없다 —
[debug-ipc.md](debug-ipc.md), [identity.md](../identity.md) 원칙 1.

## 관련

- [api-conventions](api-conventions.md) — CLI/IPC 명명 + 안정성 정책
- [build](build.md) — 크레이트 **경계**(이 문서는 크레이트 **내부**)
- [attach-behavior](attach-behavior.md) — `local/attach.rs` 의 attach 세션 머신
