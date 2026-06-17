# 터미널 링크 hover·클릭 (Terminal link)

- **Status**: Implemented
- **주체**: 로컬 사용자 전용 (마우스 + 수식키 — CLI/IPC 비노출)
- **ADR**: 없음 (원칙은 [identity](../../identity.md) §1)
- **코드**: `src/adapters/ui/terminal_link.rs` · 클릭 `src/view/main/mouse.rs` · `LinkModifier`(settings)
- **화면**: 링크 hover 하이라이트 (GPU)

## 목적

터미널 출력 속 URL/경로를 마우스로 바로 연다. **수식키 조합으로만** 발동해 일반 클릭(포커스/selection)과 충돌하지 않는다.

## 내부 동작

### 감지 대상

- URL: `http://`·`https://`·`ftp://`·`file://`, OSC 8 hyperlink.
- 스키마 없는 경로: Unix 절대(`/foo/bar`)·Windows 절대(`C:\foo`/`C:/foo`)·접두사 상대(`./`,`../`)·**접두사 없는 상대(`src/main.rs`)** — 단, **OSC 7 기반 CWD 기준 실제 존재할 때만** 링크로 판정(오탐 방지). 접두사 없는 상대는 **경로 구분자 1개 이상 토큰만** 후보(2단 방어: 슬래시 prefilter → `exists()` 검사 — `Makefile`·`and/or`·`TCP/IP` 제외).

### 트리거 (사용자 행동)

설정 수식키(`LinkModifier`, 기본 `Ctrl`, `Alt`/`없음` 선택 가능)를 누른 채:
- hover → blue 하이라이트 + PointingHand 커서.
- 좌클릭 → `webbrowser` 로 기본 브라우저/연결 프로그램 실행.

수식키+클릭이 링크 위가 아니면 no-op. selection 을 시작/변경하지 않는다.

### 원격(mirror) surface 라우팅

attach mirror surface(자식 PTY 없음 — `process_id().is_none()` 으로 판별)는 화면 경로가 **원격 호스트 경로**라 로컬 핸들러로 못 연다:
- 로컬 `exists()` 검증 건너뛰고 OSC 7 원격 cwd 기준 경로를 그대로 링크로 emit(원격 경로도 하이라이트).
- ctrl+클릭 시 로컬 핸들러 lookup 대신 **빈 핸들러 picker(empty-state placeholder)** 만 — 실제 파일 동작 없음(1차). 로컬 파일 오픈/브라우저 새는 것 방지.
- `http://` 등 스킴 있는 URL 은 mirror 여부 무관하게 `webbrowser` 로 연다.

> 비-목표(후속): 실제 원격 파일 열기(`ssh host vim {path}` 등)와 그 host 컨텍스트 주입. 1차는 placeholder 까지.

## 인터페이스

- **사용자**: 수식키 + hover/클릭.
- **AI Agent**: **없음(비노출)** — 사용자 입력 재현 금지([identity](../../identity.md) §1). 링크/경로 *데이터* 가 필요하면 읽기 전용 [terminal-output](../terminal-output/index.md) 의 `path`/`url`/`osc_link` 파서([reference/output-parsers](../../reference/output-parsers.md)).

## 비-목표

- 출력 감지 시 자동 열기 — 열기는 항상 사용자 클릭.
- 존재하지 않는 경로의 추측성 링크화 — CWD 실존 검사 통과분만.
- 수식키 없는 일반 클릭의 링크 동작 — 일반 클릭은 포커스/selection 전용.

## 관련

- [terminal](../terminal/index.md) · [terminal-output](../terminal-output/index.md)(읽기 경로) · [file-handler](../file-handler/index.md)(경로 핸들러)
