//! `tasty remote ...` subcommand — 원격(SSH) attach 1급 표면.
//!
//! 원칙 1 ②: 원격 attach 는 에이전트의 정당한 행동(다른 호스트의 surface/workspace
//! 를 mirror)이라 release 표면에 노출한다. 로컬 self(loopback) attach 는 사용자 입력
//! 재현 성격이라 `tasty debug attach`(debug 빌드)로 격리한다(`commands/debug/attach.rs`).
//!
//! 실제 SSH 터널 + attach 세션 머신은 `commands/attach.rs` 의 `run_attach_ssh` /
//! `run_attach_workspace_ssh` 에 공유 보존된다 — 이 네임스페이스는 디스패치만 한다.

use clap::Subcommand;

#[derive(Subcommand)]
pub enum RemoteCommands {
    /// SSH 너머 원격 surface/workspace 에 attach (1회성). 내부적으로 ssh -L 터널 +
    /// 단계 4 attach 결합. 원격성은 `--ssh`/`--profile` 이 흡수한다.
    Attach {
        /// 대상 surface_id (포커스 비의존 — ID 직접 지정). `--workspace` 와 상호배타.
        surface: Option<u32>,
        /// 대상 workspace_id — 그 안 모든 터미널을 트리째 mirror.
        /// `surface` positional 과 상호배타. 비-터미널은 placeholder 로 숨김.
        #[arg(long)]
        workspace: Option<u32>,
        /// mirror-dump: attach 후 N ms 동안 출력 수집 → mirror 화면을 stdout 출력 후 종료
        /// (GUI 없이 자동 검증용). workspace 모드는 surface 별 화면을 섹션으로 출력.
        #[arg(long)]
        dump_after: Option<u64>,
        /// attach 직후 1 회 전송할 입력 (escape 디코딩: \n \r \t \xNN). 비대화형 검증용.
        #[arg(long)]
        send: Option<String>,
        /// workspace 모드에서 `--send` 입력을 보낼 대상 remote surface_id.
        #[arg(long)]
        send_to: Option<u32>,
        /// raw 브리지 모드: stdin/stdout passthrough (detach = Ctrl+\).
        #[arg(long)]
        raw: bool,
        /// 점유된 surface/workspace 의 attach 락을 강제로 끊는다 (서버 권한, attach 하지
        /// 않음). 원격 관리=에이전트 작업이라 release 에 노출 — 이 서버에 붙은 원격
        /// 클라이언트의 락을 해제하는 로컬 JSON-RPC(`attach.force_detach`)다.
        /// `--ssh` 와는 상호배타(터널 너머 force-detach 는 미지원).
        #[arg(long)]
        force_detach: bool,
        /// SSH 너머 원격 대상. 예: --ssh user@host, --ssh gx10. `--profile` 과 상호배타.
        #[arg(long)]
        ssh: Option<String>,
        /// 저장된 프로필명으로 원격 attach. `~/.tasty/remote-profiles.toml` 의 프로필을
        /// resolve 해 user/port/identity/extra-options 를 결선한다. `--ssh` 와 상호배타.
        /// 이 경우 `--remote-tasty`/`--remote-port-mode` 는 프로필 값으로 대체된다.
        #[arg(long)]
        profile: Option<String>,
        /// 원격 tasty 바이너리 경로 (auto 포트 발견 체인의 subcommand 단계
        /// `ssh host <path> port` 에서 사용). 기본 "tasty" (원격 PATH 가정).
        #[arg(long, default_value = "tasty")]
        remote_tasty: String,
        /// 원격 포트 발견 모드: auto(기본) | subcommand | file-unix | file-windows.
        #[arg(long, default_value = "auto")]
        remote_port_mode: String,
        /// 자동 재연결 비활성 (기본: SSH 끊김 시 백오프 재연결).
        #[arg(long)]
        no_reconnect: bool,
        /// 이 명령을 받은 *로컬 GUI* 가 client 가 되어 원격 워크스페이스를 mirror 로
        /// 재구성하게 한다(`attach.into_gui` IPC). `--workspace` 와 `--target-port` 필요.
        #[arg(long)]
        into_gui: bool,
        /// `--into-gui` 의 원격 tasty 서버 loopback 포트(GUI 가 접속할 대상).
        #[arg(long)]
        target_port: Option<u16>,
    },
    /// SSH 너머 원격 tasty 인스턴스의 생존 여부를 확인한다(introspection).
    ///
    /// 포트 발견(`tasty port`/포트파일) 만으로는 stale 포트 파일(이미 죽은
    /// 인스턴스가 남긴 파일)을 살아있다고 오판할 수 있으므로, 포트 발견 후
    /// `ssh -L` 터널을 수립하고 그 포트로 가벼운 IPC(`system.info`) 1 회를
    /// 실제로 보내 응답이 와야만 alive 로 판정한다. 연결 거부/타임아웃 =
    /// dead(stale 포트). 인자 이름은 `remote attach` 와 통일한다.
    Check {
        /// SSH 너머 원격 대상. 예: --ssh user@host, --ssh gx10. `--profile` 과 상호배타.
        #[arg(long)]
        ssh: Option<String>,
        /// 저장된 프로필명으로 원격 생존 확인. `~/.tasty/remote-profiles.toml` 의
        /// 프로필을 resolve 해 user/port/identity/extra-options 를 결선한다.
        /// `--ssh` 와 상호배타. 이 경우 `--remote-tasty`/`--remote-port-mode` 는
        /// 프로필 값으로 대체된다.
        #[arg(long)]
        profile: Option<String>,
        /// 원격 tasty 바이너리 경로 (auto 포트 발견 체인의 subcommand 단계
        /// `ssh host <path> port` 에서 사용). 기본 "tasty" (원격 PATH 가정).
        #[arg(long, default_value = "tasty")]
        remote_tasty: String,
        /// 원격 포트 발견 모드: auto(기본) | subcommand | file-unix | file-windows.
        #[arg(long, default_value = "auto")]
        remote_port_mode: String,
    },
    /// SSH 너머 원격 tasty 인스턴스의 워크스페이스 목록을 조회한다(browse).
    ///
    /// attach 프로필/ssh 대상에 붙어 원격 인스턴스의 `workspace.list` +
    /// `attach.list` 를 받아 병합한다 — 각 워크스페이스의 id/name/pane_count/
    /// busy/attached(타 client 점유 여부)를 반환한다. `remote attach` 가 대상
    /// workspace id 를 미리 알아야 하는 것과 달리, 이 명령이 그 id 를 발견한다.
    /// 순수 조회라 로컬 사용자 상태(focus 등)에 닿지 않는다(원칙 1). `--ssh
    /// 127.0.0.1:<port>` 로 loopback 직결(터널 없이 로컬 e2e).
    ///
    /// 로컬 IPC method `remote.workspaces` 와 동일한 능력을 공유한다(원칙 2 —
    /// 에이전트가 CLI 없이 소켓만으로도 브라우징 가능).
    Workspaces {
        /// SSH 너머 원격 대상. 예: --ssh user@host, --ssh gx10,
        /// --ssh 127.0.0.1:45123. `--profile` 과 상호배타.
        #[arg(long)]
        ssh: Option<String>,
        /// 저장된 tasty-attach 프로필명으로 조회. `--ssh` 와 상호배타.
        /// 이 경우 `--remote-tasty`/`--remote-port-mode` 는 프로필 값으로 대체된다.
        #[arg(long)]
        profile: Option<String>,
        /// 원격 tasty 바이너리 경로 (auto 포트 발견 체인의 subcommand 단계). 기본 "tasty".
        #[arg(long, default_value = "tasty")]
        remote_tasty: String,
        /// 원격 포트 발견 모드: auto(기본) | subcommand | file-unix | file-windows.
        #[arg(long, default_value = "auto")]
        remote_port_mode: String,
        /// 사람이 읽는 텍스트 대신 JSON 배열로 출력(스크립트/팝업 소비용).
        #[arg(long)]
        json: bool,
    },
    /// SSH 너머 원격 tasty 인스턴스에 워크스페이스를 **새로 만든다**(원격 mutate).
    ///
    /// `remote workspaces`(조회)와 같은 자리의 변경 1건이다. 출력된 id 를 그대로
    /// `tasty remote attach --workspace <id>` 에 넘기면 "원격에 만들고 그 자리에서
    /// attach" 가 완성된다 — attach 세션 3갈래 분기(raw/into-gui/force-detach)를
    /// 건드리지 않으려고 생성을 독립 서브커맨드로 분리했다.
    ///
    /// 원격의 활성 워크스페이스는 바뀌지 않는다(IPC = Agent origin, 원칙 1).
    /// 로컬 IPC 는 `remote.attach` 의 `new_workspace` 옵션이 같은 능력을 노출한다
    /// (원칙 2 — CLI/IPC 양면, 같은 `remote_create` 코어 공유).
    NewWorkspace {
        /// SSH 너머 원격 대상. 예: --ssh user@host, --ssh gx10,
        /// --ssh 127.0.0.1:45123. `--profile` 과 상호배타.
        #[arg(long)]
        ssh: Option<String>,
        /// 저장된 tasty-attach 프로필명으로 생성. `--ssh` 와 상호배타.
        /// 이 경우 `--remote-tasty`/`--remote-port-mode` 는 프로필 값으로 대체된다.
        #[arg(long)]
        profile: Option<String>,
        /// 원격 tasty 바이너리 경로 (auto 포트 발견 체인의 subcommand 단계). 기본 "tasty".
        #[arg(long, default_value = "tasty")]
        remote_tasty: String,
        /// 원격 포트 발견 모드: auto(기본) | subcommand | file-unix | file-windows.
        #[arg(long, default_value = "auto")]
        remote_port_mode: String,
        /// 새 워크스페이스 이름 (미지정 시 원격 기본값).
        #[arg(long)]
        name: Option<String>,
        /// 새 워크스페이스의 작업 디렉토리 — **원격 파일시스템 기준**. 원격에서
        /// 존재 검증되며 없으면 `cwd does not exist` 로 거절된다.
        #[arg(long)]
        cwd: Option<String>,
        /// 사람이 읽는 텍스트 대신 JSON 객체로 출력(스크립트/에이전트 소비용).
        #[arg(long)]
        json: bool,
    },
}
