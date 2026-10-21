# X11 IME 서버 복구

Linux X11의 winit IME는 ibus 같은 XIM 서버에 동기 요청을 보낸다. libX11에는 이 응답을
기다리는 시간 제한이 없으므로, 이미 종료되거나 교체된 서버로 요청을 보내면 GUI와 IPC가
함께 멈출 수 있다. [고정한 winit 포크](dep-issues.md)의 보호는 아래 범위에 적용된다.
Wayland·macOS·Windows의 입력 경로는 이 보호를 사용하지 않는다.

## 생존 확인과 교체

IM을 열 때 libX11이 실제로 사용한 locale modifiers를 읽고, XIM_SERVERS의 이름·별칭과
selection owner를 기록한다. 이 연결이 owner 창에 선택한 StructureNotify도 확인한다.
같은 XID를 새 서버가 재사용해도 owner 값만으로 기존 서버라고 판단하지 않는다.
서버를 식별할 수 없으면 Unverified로 두고 XGetIMValues부터 생략한다.
local/none과 확인된 Thai 로컬 입력은 서버 왕복을 요구하지 않는다. 빈 이름은 서버에도
연결될 수 있어 무조건 local로 취급하지 않으며 명시적 local 후보로 Compose를 제공한다.

focus/unfocus, IC 생성·파괴, spot·허용 상태 변경, IM 교체·종료와 키 XFilterEvent의
동기 요청 전에 owner를 확인한다. 불일치하면 해당 서버 창의 대기 중인 DestroyNotify만
꺼내 XFilterEvent에 넘긴다. 다른 이벤트의 순서는 바꾸지 않는다. 필터가 콜백을 실행할
수 있으므로 IM·IC·context 참조를 잡기 전에 처리하고, 반환 뒤 다시 조회한다.

owner 불일치만으로 libX11의 IM이 해제됐다고 판단하지 않는다. DestroyNotify가 아직
없으면 suspect로 두고 요청을 생략한다. 실제 destroy 콜백은 현재 IM인지 확인한 뒤
파괴 상태와 재연결 요청을 기록한다. 이미 교체한 옛 IM의 destroy는 현재 IM에 적용하지 않는다.

## fallback과 재연결

named primary를 열 수 없으면 local fallback을 시도한다. instantiate 콜백은 Pending만
기록한다. 초기 im이 아직 없는 등록 직후 콜백도 새 IM을 열지 않는다. 초기화가 끝나거나
XFilterEvent가 반환한 안전한 경계에서 요청을 한 번 처리한다.

libX11은 instantiate 콜백을 호출하기 전에 내부 call flag를 설정하며, flag 초기화는
등록 때와 현재 locale modifiers가 일치해야 한다. local을 연 뒤에는 등록이 남아 있어도
다시 콜백이 오지 않을 수 있다. 따라서 포크는 root의 XIM_SERVERS/NewValue를 필터 전에
기록하고, 이벤트가 소비돼도 바깥 처리 경계에서 재연결을 시도한다.

성공한 primary는 Idle로 돌아가고 instantiate 등록을 해제한다. fallback 또는 복구 가능한
실패는 WaitingForServer로 남아 다음 광고를 기다린다. 일반 키나 이벤트 루프 반복만으로
재시도하지 않는다. 처음부터 local만 쓰는 세션은 서버 광고 때문에 재연결하지 않는다.
사용할 입력기를 전혀 열 수 없는 기존 초기화·복구 오류 정책까지 바꾸는 기능은 아니다.

## 자원 수명과 한계

IC를 일부 만든 뒤 교체가 실패하면 새 IM과 만든 context들을 함께 정리한다. 서버가
여전히 확인되면 XCloseIM이 C의 IC를 해제한 뒤 Rust callback data를 내려놓는다.
서버를 확인할 수 없으면 동기 close/destroy를 보내지 않고 callback data를 유지한다.
이때 forget한 Rust Box·텍스트 버퍼·sender는 libX11이 나중에 XIC를 해제해도 자동 회수되지
않으며 **프로세스 수명 동안 누수**한다. 반복 실패 때 누적될 수 있다. 전체 ImeInner를
영구 보존한다는 뜻이나 모든 callback 수명 오류가 검증됐다는 뜻은 아니다.

이 보호로 다음 대기까지 막을 수는 없다.

- XOpenIM의 연결 협상 중 서버가 종료되는 경우.
- 생존 확인 직후 또는 요청을 보낸 뒤 서버가 종료되는 경우.
- 서버가 살아 있지만 응답하지 않는 경우.

## 검증 범위

Linux Xvfb/ibus의 실제 XTest 한글 입력과 IPC로 서버 종료 후 FocusOut(A), 서버 교체(B)의
복구를 확인한다. 새 pin의 비계측 실행과 검증용 계측 실행은 별도로 기록한다.
debug IME Commit 주입은 OS 입력기를 거치지 않으므로 재연결 근거로 쓰지 않는다.

포크 검증의 startup/재연결 실패 대조는 XOpenIM 뒤 named primary를 Unverified로 강제한
계측 실행이다. 실제 owner 경쟁이나 C private call flag를 직접 측정한 것은 아니다.
Reconnect 상태 시험 3건과 기존 서버 식별 시험 5건은 통과했지만 FFI·메모리 안전성 전체를
보장하지 않는다. 과거 popup·후보 위치·partial IC 실패 자료는 해당 불변 경로에 한정한다.
키 비용 자료 320쌍은 XTest→PTY 도착이며 키→화면 렌더 지연이 아니다.

포크의 cap-lints allow/warn 결과는 strict clippy 통과가 아니다. warn 실행에는 기존
iter_kv_map 경고가 남는다. Thai runtime, ASan/Miri, 다른 OS의 신규 실측은 이 근거에
포함하지 않는다. 격리·PID 소유 확인과 명령의 실제 종료 코드 판독은
[자체 검증](self-verification.md), IME 주입의 범위는 [IME 검증](../ai-verification/ime-testing.md)을 따른다.
