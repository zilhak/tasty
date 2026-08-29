# CLI 크레이트 내부 구조 — 세 갈래 대칭

`crates/tasty-cli` 는 명령 하나를 **세 갈래**로 나눠 다룬다. 새 CLI 명령을 추가할 때
어디를 고칠지는 이 표가 정한다.

| 디렉토리 | 답하는 질문 | 들어가는 것 | 들어가면 안 되는 것 |
|----------|-------------|-------------|---------------------|
| `commands/` | **무엇을 받나** | clap `Subcommand`/`Args` 선언, 도움말 문구 | 실행 코드 (`pub fn` 0개), SSH·스트림·원격 계층 참조 |
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

## 진입점 (`run.rs`)

`run_client` 는 갈래를 묻고 `ClientDriven` 이면 넘긴 뒤, 나머지 단발 RPC 경로
(포트 파일 읽기 → 연결 → 요청 → 출력, `auto_wait` 폴링 포함)만 직접 수행한다.

## `debug` 갈래

`commands/debug.rs`(선언)와 `local/debug.rs`(실행) 둘 다 모듈째
`#![cfg(debug_assertions)]` 다. 사용자 입력 재현은 release 표면에 없다 —
[debug-ipc.md](debug-ipc.md), [identity.md](../identity.md) 원칙 1.

## 관련

- [api-conventions](api-conventions.md) — CLI/IPC 명명 + 안정성 정책
- [build](build.md) — 크레이트 **경계**(이 문서는 크레이트 **내부**)
- [attach-behavior](attach-behavior.md) — `local/attach.rs` 의 attach 세션 머신
