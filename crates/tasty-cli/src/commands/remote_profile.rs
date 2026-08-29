//! `tasty tool remote-profile ...` — 원격 접속 프로필 통합 CRUD 의 **clap 선언**.
//!
//! 실행은 [`crate::local::remote_profile`] 이 한다(로컬 파일, IPC 미경유).
//!
//! 2-레이어 모델(ADR-0032): **ssh** = 순수 연결 정보(host/user/port/identity/options/
//! shell), **tasty-attach** = attach 스펙(ssh_ref 참조 또는 인라인 연결 + remote_tasty/
//! port_mode/port_file). attach 동작 자체는 `tasty tool attach` 에서 tasty-attach
//! 프로필을 소비한다.

use clap::Subcommand;

#[derive(Subcommand)]
pub enum RemoteProfileCommands {
    /// 저장된 프로필 목록 출력(ssh + tasty-attach).
    List {
        #[arg(long)]
        json: bool,
        /// kind 필터: ssh | tasty-attach.
        #[arg(long)]
        kind: Option<String>,
    },
    /// 한 프로필 상세 출력.
    Show {
        #[arg(long)]
        name: String,
        #[arg(long)]
        json: bool,
    },
    /// ssh 연결 프로필 추가(순수 연결 정보 — attach 스펙 없음).
    AddSsh {
        /// 프로필 고유 식별자.
        #[arg(long)]
        name: String,
        /// ssh destination: host | user@host | ssh config alias.
        #[arg(long)]
        host: String,
        /// ssh 유저(host 에 user@ 가 없을 때).
        #[arg(long)]
        user: Option<String>,
        /// ssh 포트(기본: ssh config / 22).
        #[arg(long)]
        port: Option<u16>,
        /// identity 파일 경로(-i). path kind passkey `<name>-key` 로 분리 저장된다.
        #[arg(long)]
        identity: Option<String>,
        /// 추가 ssh -o 옵션(반복 가능). 예: --option ServerAliveInterval=30
        #[arg(long = "option")]
        options: Vec<String>,
        /// 원격 셸: powershell | cmd | bash | zsh | auto(기본).
        #[arg(long, default_value = "auto")]
        shell: String,
        /// UI 표시용 라벨(옵션).
        #[arg(long)]
        label: Option<String>,
    },
    /// tasty-attach 프로필 추가. 연결은 `--ssh-ref <name>` 참조 또는 인라인 필드(host/…).
    AddAttach {
        /// 프로필 고유 식별자.
        #[arg(long)]
        name: String,
        /// 참조할 ssh 프로필 name(라이브 팔로우). 지정 시 인라인 연결 필드는 무시된다.
        #[arg(long = "ssh-ref")]
        ssh_ref: Option<String>,
        /// 인라인 연결: ssh destination(host | user@host | alias). `--ssh-ref` 없을 때.
        #[arg(long)]
        host: Option<String>,
        /// 인라인 연결: ssh 유저.
        #[arg(long)]
        user: Option<String>,
        /// 인라인 연결: ssh 포트.
        #[arg(long)]
        port: Option<u16>,
        /// 인라인 연결: identity 파일 경로(-i). path kind passkey 로 분리 저장.
        #[arg(long)]
        identity: Option<String>,
        /// 인라인 연결: 추가 ssh -o 옵션(반복 가능).
        #[arg(long = "option")]
        options: Vec<String>,
        /// 원격 tasty 바이너리 경로(포트 발견용). 기본 "tasty".
        #[arg(long, default_value = "tasty")]
        remote_tasty: String,
        /// 원격 포트 발견 모드: auto(기본) | subcommand | file-unix | file-windows.
        #[arg(long, default_value = "auto")]
        port_mode: String,
        /// 원격 port 파일의 명시 경로(비표준 위치). 지정 시 관례 경로보다 최우선.
        #[arg(long)]
        port_file: Option<String>,
        /// UI 표시용 라벨(옵션).
        #[arg(long)]
        label: Option<String>,
    },
    /// 기존 프로필의 일부 필드 갱신(지정한 필드만 덮어쓴다). kind 는 유지된다.
    Edit {
        #[arg(long)]
        name: String,
        #[arg(long)]
        host: Option<String>,
        #[arg(long)]
        user: Option<String>,
        #[arg(long)]
        port: Option<u16>,
        #[arg(long)]
        identity: Option<String>,
        #[arg(long = "option")]
        options: Vec<String>,
        /// tasty-attach: 참조 ssh 프로필 name 갱신.
        #[arg(long = "ssh-ref")]
        ssh_ref: Option<String>,
        /// tasty-attach: 원격 tasty 바이너리 경로.
        #[arg(long)]
        remote_tasty: Option<String>,
        /// tasty-attach: 원격 포트 발견 모드.
        #[arg(long)]
        port_mode: Option<String>,
        /// tasty-attach: 원격 port 파일 경로.
        #[arg(long)]
        port_file: Option<String>,
        /// ssh: 원격 셸(powershell | cmd | bash | zsh | auto).
        #[arg(long)]
        shell: Option<String>,
        #[arg(long)]
        label: Option<String>,
    },
    /// 프로필 제거(참조 passkey 는 공유 가능성 때문에 보존).
    Remove {
        #[arg(long)]
        name: String,
    },
    /// 프로필을 재감지한다(ssh: 셸 감지 프로브 / tasty-attach: 원격 포트 검증). SSH 접속 발생.
    Detect {
        #[arg(long)]
        name: String,
    },
    /// 로컬 ssh config(`~/.ssh/config` + Include)의 Host alias 목록. 접속하지 않는다.
    ListLocal {
        #[arg(long)]
        json: bool,
    },
    /// 로컬 ssh config alias 를 ssh 프로필로 가져온다(alias 만 저장 — 값 해석은 ssh 몫).
    Import {
        /// 가져올 ssh config alias (`list-local` 의 ALIAS 열).
        #[arg(long)]
        from: String,
        /// 새로 만들 프로필 고유 식별자.
        #[arg(long)]
        name: String,
        /// UI 표시용 라벨(옵션).
        #[arg(long)]
        label: Option<String>,
    },
}
