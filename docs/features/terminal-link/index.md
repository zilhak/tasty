# 터미널 링크 hover·클릭 (Terminal link)

- **Status**: Implemented
- **주체**: 로컬 사용자 전용 (마우스 + 수식키 — CLI/IPC 비노출)
- **ADR**: 없음 (원칙은 [identity](../../identity.md) §1)
- **코드**: `src/adapters/ui/terminal_link.rs` · 클릭 `src/view/main/mouse.rs` · `LinkModifier`(settings) · 드래그선택 우클릭 메뉴 `src/view/main/redraw.rs`(`handle_terminal_surface_native_menu`)
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

### 소프트 wrap(멀티라인) 링크

터미널 폭을 넘는 OSC 8 하이퍼링크가 화면상 여러 줄로 소프트 wrap 되면, hover 하이라이트는 마우스가 그 체인의 몇 번째 줄 위에 있든 **체인에 속한 모든 화면상 줄을 동시에** 하이라이트한다. 판정 기준은 두 조건의 AND: 인접 행이 소프트 wrap(다음 행이 논리적 연속)이고, 그 다음/이전 행이 **동일 URI**의 OSC 8 링크로 이어지는 경우만 병합한다 — 위/아래 양방향, 3줄 이상 체인도 재귀적으로 훑는다. plain-text URL/경로(regex 검출)는 wrap continuation 행에 스킴 프리픽스가 없어 그 행 자체에서 애초에 매치가 나지 않으므로(따라서 URI 도 일치하지 않으므로) 별도 처리 없이 병합 대상에서 자연히 제외된다 — 단일 wrap 행 안에서만 표시된다.

### 원격(mirror) surface 라우팅

attach mirror surface(자식 PTY 없음 — `process_id().is_none()` 으로 판별)는 화면 경로가 **원격 호스트 경로**라 로컬 핸들러로 못 연다:
- 로컬 `exists()` 검증 건너뛰고 OSC 7 원격 cwd 기준 경로를 그대로 링크로 emit(원격 경로도 하이라이트).
- ctrl+클릭 시 로컬 핸들러 lookup 대신 **빈 핸들러 picker(empty-state placeholder)** 만 — 실제 파일 동작 없음(1차). 로컬 파일 오픈/브라우저 새는 것 방지.
- `http://` 등 스킴 있는 URL 은 mirror 여부 무관하게 `webbrowser` 로 연다.

> 비-목표(후속): 실제 원격 파일 열기(`ssh host vim {path}` 등)와 그 host 컨텍스트 주입. 1차는 placeholder 까지.

### 드래그/더블클릭 선택 → 우클릭 메뉴 "경로 열기"

hover+수식키 클릭과는 별개 입력 경로 — 사용자가 드래그(또는 더블/트리플클릭)로 이미 확정한 selection 텍스트가 대상이다. 선택 모드(문자/단어/줄/블록) 구분 없이 `extract_selected_text` 가 뽑아준 문자열을 그대로 1차 후보로 쓴다(hover 경로의 `path_regex()` 는 재사용하지 않음 — 비-ASCII 후행 문자가 매치 단계에서 잘려나가면 아래 축약 재검사 자체가 무의미해지기 때문).

- **판별**: `longest_existing_selection_path`(`terminal_link.rs`). 1차 후보(선택 문자열 trim + `trim_trailing_punct`)가 (cwd 결합 후) 존재하지 않으면 마지막 `/` 앞까지 잘라 재검사하고, 실패하면 그 앞 `/`로 계속 반복 — 실재하는 가장 긴 접두사를 채택한다. 예: 실제 경로 뒤로 슬래시 없이 문자(한글 조사 등)가 붙어 선택돼도, 그 앞 경로 접두사가 실재하면 그 경로로 채택.
- **노출 조건**: 우클릭한 surface 와 `text_selection.surface_id` 가 같을 때만 "경로 열기" 항목을 추가한다 — surface 별로 독립적인 드래그 상태를 가질 수 있어, 다르면 노출하지 않는다(기존 복사 메뉴는 surface 무관 전역 selection 관례 그대로 유지).
- **라벨**: `Path::is_dir()` 로 파일/폴더를 구분해 다른 라벨을 표시(`terminal_context_menu.open_file`/`open_folder`).
- **열기**: `crate::platform::reveal::open_path`(explorer 컨텍스트 메뉴의 "시스템에서 열기"와 동일 함수).
- **mirror(원격 attach) surface**: 로컬 파일 관리자로 원격 경로를 여는 배선이 아직 없어 제외(`terminal.process_id().is_none()`이면 항목 자체를 노출하지 않음). 배선이 생기면 재검토.

## 인터페이스

- **사용자**: 수식키 + hover/클릭, 또는 드래그/더블클릭 선택 후 우클릭 → "경로 열기".
- **AI Agent**: **없음(비노출)** — 사용자 입력 재현 금지([identity](../../identity.md) §1). 링크/경로 *데이터* 가 필요하면 읽기 전용 [terminal-output](../terminal-output/index.md) 의 `path`/`url`/`osc_link` 파서([reference/output-parsers](../../reference/output-parsers.md)).

## 비-목표

- 출력 감지 시 자동 열기 — 열기는 항상 사용자 클릭.
- 존재하지 않는 경로의 추측성 링크화 — CWD 실존 검사 통과분만.
- 수식키 없는 일반 클릭의 링크 동작 — 일반 클릭은 포커스/selection 전용.

## 관련

- [terminal](../terminal/index.md) · [terminal-output](../terminal-output/index.md)(읽기 경로) · [file-handler](../file-handler/index.md)(경로 핸들러)
