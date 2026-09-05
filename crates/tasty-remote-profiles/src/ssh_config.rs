//! 로컬 ssh config(`~/.ssh/config`)의 Host alias 열거 — **읽기 전용**.
//!
//! tasty 는 자기 프로필만 알고 사용자가 이미 `~/.ssh/config` 에 등록해 둔 alias 는
//! 모른다. 그 목록을 뽑아 "가져오기" 를 제안하는 것이 이 모듈의 유일한 용건이다.
//!
//! 계약 세 가지:
//!
//! - **프로세스 spawn 없음.** `ssh -G <host>` 는 OpenSSH 가 위치·Include·Match 를
//!   전부 해석해 최종값을 주지만 (1) alias *목록* 을 뽑는 기능이 아니고 (2)
//!   `Match exec "..."` 가 있으면 그 명령을 **실제로 실행**한다 — 목록 한 번 그리자고
//!   사용자 스크립트를 alias 수만큼 돌릴 수는 없다. 그래서 순수 파싱만 한다.
//! - **값을 해석하지 않는다.** tasty 는 alias 문자열만 있으면 접속할 수 있고
//!   (`SshView::host()` 가 `host` | `user@host` | alias 를 그대로 ssh 에 위임한다),
//!   `Host`/`Match`/`ProxyJump` 의 최종 해석은 ssh(1) 의 몫이다.
//! - **user config 만 읽는다.** `/etc/ssh/ssh_config` 는 관리자가 넣은 전역 설정이라
//!   "내가 등록한 것" 목록에 섞이면 안 된다.
//!
//! 실패(파일 없음 / 권한 없음 / 깨진 UTF-8)는 에러가 아니라 **빈 목록** 이다 — ssh
//! config 가 없는 사용자가 다수이고, 그건 고장이 아니라 정상 상태다. "없음" 은 debug,
//! 진짜 읽기 실패는 warn 로그로 갈린다([`read_text`]); 표면은 파일 존재 여부를 따로
//! 보고 "설정이 없다" 와 "alias 가 0건" 을 다르게 안내한다.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use tasty_utils::path::os_home_dir;

use crate::profile::{RemoteProfile, RemoteProfiles};

/// `Include` 재귀 깊이 상한. OpenSSH 자체 상한(16)과 같은 값을 쓴다 — 그보다 깊은
/// 설정은 ssh 도 읽지 않으므로 여기서 더 파고들 이유가 없다.
const MAX_INCLUDE_DEPTH: usize = 16;

/// ssh config 에서 발견한 Host alias 하나.
///
/// `hostname` / `user` / `port` 는 **표시 전용 hint** 다. 그 alias 블록에 *직접 적힌*
/// 값만 담으며, `Host *` 의 전역 설정이나 `Match` 블록이 실제 접속 시 이 값을 덮어쓸 수
/// 있어 정확성이 보장되지 않는다 — 목록에 캡션("gx10 / 10.0.0.5:2200")을 그리는 용도
/// 외에 **프로필 저장에 쓰지 않는다**. 상속(`Host *` 값을 개별 alias 에 합성)도 하지
/// 않는다: 합성하는 순간 "그 블록에 적힌 것" 이라는 단순한 계약이 깨지고 정확도 착시만
/// 커진다.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SshConfigHost {
    /// 접속 가능한 리터럴 alias (와일드카드·부정 패턴은 애초에 수집되지 않는다).
    pub alias: String,
    /// 이 alias 가 적힌 파일. `Include` 로 갈라진 설정에서 출처를 되짚을 수 있어야 한다.
    pub source: PathBuf,
    /// 같은 블록의 `HostName` (표시 전용).
    pub hostname: Option<String>,
    /// 같은 블록의 `User` (표시 전용).
    pub user: Option<String>,
    /// 같은 블록의 `Port` (표시 전용).
    pub port: Option<u16>,
}

/// user ssh config 기본 경로 — `<home>/.ssh/config`.
///
/// Windows 도 `%USERPROFILE%\.ssh\config` 라 플랫폼 분기가 없다(`BaseDirs` 가 흡수).
/// OpenSSH 에는 `SSH_CONFIG` 같은 환경변수 override 가 없고 override 수단은 `ssh -F`
/// 뿐인데, tasty 는 ssh 를 띄울 때 `-F` 를 주지 않는다 — 즉 여기서 읽는 파일과 tasty 가
/// 실제 접속에 쓰는 파일이 같음이 보장된다.
pub fn user_config_path() -> Option<PathBuf> {
    os_home_dir().map(|home| home.join(".ssh").join("config"))
}

/// 최상위 config 파일의 가용성 — "없다" 와 "있는데 못 읽는다" 를 가른다.
///
/// 열거([`enumerate_hosts`])는 두 경우 모두 **빈 목록**으로 떨어져서 결과만 봐서는
/// 구분이 안 된다. 그런데 호출자가 할 일은 정반대다 — 없으면 만들라고 안내하고,
/// 못 읽으면 권한을 고치라고 안내해야 한다. in-process 소비자(GUI)는 자기가 파일을
/// 보면 그만이지만 IPC 호출자에겐 응답에 실려야만 보이는 정보다.
///
/// **최상위 파일 한정이다.** `Include` 로 딸려오는 파일까지 권한을 훑지 않는다 —
/// 열거는 읽히는 것만 모으고 못 읽은 include 는 [`read_text`] 가 warn 을 남긴다.
/// 실사용에서 권한이 어긋나는 건 대개 최상위 파일 하나다.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ConfigAvailability {
    /// 그 경로에 무언가 있는가(정규 파일이 아니어도 true).
    pub exists: bool,
    /// **읽어서 파싱할 수 있는 정규 파일**인가. `exists` 가 false 면 항상 false 다.
    ///
    /// 열리기만 하는 것으로는 부족하다 — linux 에서 디렉토리는 `File::open` 이
    /// 성공해서, 경로가 디렉토리면 "읽히는데 Host 가 하나도 없다" 로 잘못 보고됐다.
    /// 그건 사용자에게 "Host 를 적어라" 라는 엉뚱한 안내로 이어진다. 정규 파일이
    /// 아니면 열거가 애초에 성립하지 않으므로 못 읽는 것으로 친다.
    pub readable: bool,
}

/// 주어진 경로의 [`ConfigAvailability`]. `None`(홈을 못 찾음)이면 둘 다 false.
pub fn config_availability(path: Option<&Path>) -> ConfigAvailability {
    let Some(p) = path else {
        return ConfigAvailability::default();
    };
    ConfigAvailability {
        exists: p.exists(),
        // 존재 여부와 **별개로** 실제 열기를 시도한다 — `exists` 만으로는 권한 실패를
        // 못 잡는다. 여기에 `is_file` 을 겹치는 건 디렉토리 때문이다: linux 는
        // 디렉토리 열기를 허용해서 open 성공만 보면 readable 이 true 로 샌다.
        // 심볼릭 링크는 따라간 대상 기준으로 판정된다(dangling 이면 둘 다 false).
        readable: std::fs::File::open(p).is_ok() && p.is_file(),
    }
}

/// 기본 user config 에서 alias 를 열거한다. 홈을 못 찾거나 파일이 없으면 빈 목록.
pub fn enumerate_hosts() -> Vec<SshConfigHost> {
    let Some(path) = user_config_path() else {
        tracing::warn!("ssh_config: cannot resolve home directory — skipping enumeration");
        return Vec::new();
    };
    enumerate_hosts_at(&path)
}

/// 주어진 config 파일에서 alias 를 열거한다(테스트 주입 지점).
///
/// 상대 `Include` 경로의 기준은 **이 파일이 있는 디렉토리** 다 — OpenSSH 가 user
/// config 의 상대 include 를 `~/.ssh/` 기준으로 푸는 것과 같은 규칙이며, 픽스처를
/// tempdir 에 쓰면 그 tempdir 이 그대로 기준이 된다.
pub fn enumerate_hosts_at(path: &Path) -> Vec<SshConfigHost> {
    let base_dir = path.parent().map(Path::to_path_buf).unwrap_or_default();
    let mut state = Scan {
        base_dir,
        visited: HashSet::new(),
        seen: HashSet::new(),
        hosts: Vec::new(),
    };
    state.read_file(path, 0);
    state.hosts
}

/// 파싱 진행 상태. `Include` 재귀가 같은 목록·같은 중복 판정을 공유해야 하므로
/// 파일 단위가 아니라 스캔 단위로 들고 다닌다.
struct Scan {
    /// 상대 `Include` 경로의 기준 디렉토리(= 루트 config 의 디렉토리).
    base_dir: PathBuf,
    /// 이미 읽은 파일(순환 방지). 정규화 실패 시 원본 경로로 대신 기록한다.
    visited: HashSet<PathBuf>,
    /// 이미 수집한 alias(중복 제거 — 첫 등장 우선).
    seen: HashSet<String>,
    hosts: Vec<SshConfigHost>,
}

impl Scan {
    fn read_file(&mut self, path: &Path, depth: usize) {
        if depth > MAX_INCLUDE_DEPTH {
            tracing::warn!(
                path = %path.display(),
                MAX_INCLUDE_DEPTH,
                "ssh_config: Include nested too deep — stopping here"
            );
            return;
        }
        let key = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
        if !self.visited.insert(key) {
            // 순환(a → b → a)이나 같은 파일 중복 Include. 이미 읽었으므로 조용히 끝낸다.
            return;
        }
        let Some(text) = read_text(path) else {
            return;
        };
        self.parse(&text, path, depth);
    }

    fn parse(&mut self, text: &str, path: &Path, depth: usize) {
        // 현재 Host 블록이 만든 항목들의 인덱스. hint 는 이 항목들에만 붙는다.
        let mut block: Vec<usize> = Vec::new();

        for line in text.lines() {
            let Some((keyword, args)) = split_line(line) else {
                continue;
            };
            match keyword.as_str() {
                "host" => {
                    // `Match` 블록 뒤에 와도 수집한다 — ssh(1) 이 이 줄을 Match 를
                    // 끝내는 **새 무조건 블록**으로 읽기 때문이다. 열거에서 빼면 사용자
                    // 눈에는 "내 호스트가 목록에 없다" 로만 보인다. 잘못 실어도 손해가
                    // 없다는 점도 근거다: 가져오기는 alias 문자열만 저장하고 최종 해석은
                    // 접속 시점의 ssh 가 하므로, 목록의 이름은 명령줄에 직접 친 이름과
                    // 정확히 같은 것을 가리킨다.
                    block.clear();
                    for token in args {
                        if !is_literal_alias(&token) {
                            continue;
                        }
                        if !self.seen.insert(token.clone()) {
                            continue; // 첫 등장 우선 — 뒤 파일의 같은 alias 는 버린다.
                        }
                        block.push(self.hosts.len());
                        self.hosts.push(SshConfigHost {
                            alias: token,
                            source: path.to_path_buf(),
                            hostname: None,
                            user: None,
                            port: None,
                        });
                    }
                }
                "match" => {
                    // Match 블록의 설정은 어느 Host 소속도 아니다 — 컨텍스트만 끊는다.
                    // (다음 `Host` 가 새 컨텍스트를 열 때까지 hint 는 갈 곳이 없다.)
                    block.clear();
                }
                "hostname" | "user" | "port" => {
                    let Some(value) = args.into_iter().next() else {
                        continue;
                    };
                    self.apply_hint(&block, &keyword, &value, path);
                }
                "include" => {
                    for arg in args {
                        for file in self.resolve_include(&arg) {
                            self.read_file(&file, depth + 1);
                        }
                    }
                }
                _ => {}
            }
        }
    }

    /// 현재 블록이 만든 항목들에 표시용 hint 를 채운다. ssh 는 "먼저 나온 값이 이긴다"
    /// 이므로 이미 채워진 자리는 덮지 않는다.
    fn apply_hint(&mut self, block: &[usize], keyword: &str, value: &str, path: &Path) {
        for &i in block {
            let host = &mut self.hosts[i];
            match keyword {
                "hostname" => {
                    if host.hostname.is_none() {
                        host.hostname = Some(value.to_string());
                    }
                }
                "user" => {
                    if host.user.is_none() {
                        host.user = Some(value.to_string());
                    }
                }
                _ => {
                    if host.port.is_none() {
                        match value.parse::<u16>() {
                            Ok(p) => host.port = Some(p),
                            Err(e) => tracing::warn!(
                                path = %path.display(),
                                value,
                                error = %e,
                                "ssh_config: unparsable Port — hint left empty"
                            ),
                        }
                    }
                }
            }
        }
    }

    /// `Include` 인자 하나를 실제 파일 목록으로 푼다 — `~` 확장 + 상대경로 해석 + glob.
    fn resolve_include(&self, arg: &str) -> Vec<PathBuf> {
        let expanded = expand_tilde(arg);
        let full = if expanded.is_absolute() {
            expanded
        } else {
            self.base_dir.join(expanded)
        };
        expand_glob(&full)
    }
}

/// config 파일 하나를 읽는다. 못 읽으면 `None` — **에러로 올리지 않는다.**
///
/// 없음과 진짜 실패는 로그 레벨로 가른다: ssh config 가 없는 사용자가 다수라 그건
/// 정상 상태이고(warn 을 띄우면 CLI stderr 에 "고장" 처럼 보인다), 권한·비 UTF-8 은
/// 원인을 남겨야 한다. 표면(CLI/IPC)은 파일 존재 여부를 따로 보고 "설정이 없다" 와
/// "alias 가 0건" 을 다르게 안내한다.
fn read_text(path: &Path) -> Option<String> {
    match std::fs::read_to_string(path) {
        Ok(t) => Some(t),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            tracing::debug!(path = %path.display(), "ssh_config: no such file — skipped");
            None
        }
        Err(e) => {
            tracing::warn!(path = %path.display(), error = %e, "ssh_config: unreadable — skipped");
            None
        }
    }
}

/// 접속에 쓸 수 있는 리터럴 alias 인지. 와일드카드(`*` `?`)나 부정(`!`)이 섞인 토큰은
/// 이름이 아니라 패턴이라 열거 대상이 아니다.
fn is_literal_alias(token: &str) -> bool {
    !token.is_empty() && !token.contains(['*', '?', '!'])
}

fn expand_tilde(arg: &str) -> PathBuf {
    let rest = arg
        .strip_prefix("~/")
        .or_else(|| arg.strip_prefix("~\\"))
        .or_else(|| if arg == "~" { Some("") } else { None });
    match (rest, os_home_dir()) {
        (Some(rest), Some(home)) => home.join(rest),
        // `~user/` 형태는 풀지 않는다 — ssh 도 흔히 쓰지 않고, 잘못 풀면 엉뚱한 파일을
        // 읽는다. 그대로 두면 존재하지 않는 경로가 되어 조용히 건너뛴다.
        _ => PathBuf::from(arg),
    }
}

/// glob 전개 — 컴포넌트 단위로 디렉토리를 훑는다. `*` 와 `?` 만 지원한다(OpenSSH 가
/// 쓰는 glob(3) 의 부분집합). 와일드카드가 없으면 경로 자체를 그대로 돌려주므로
/// 존재 판정은 호출 측(읽기 실패 → warn)이 맡는다.
fn expand_glob(pattern: &Path) -> Vec<PathBuf> {
    let has_wildcard = pattern
        .components()
        .any(|c| c.as_os_str().to_string_lossy().contains(['*', '?']));
    if !has_wildcard {
        return vec![pattern.to_path_buf()];
    }

    let mut current: Vec<PathBuf> = vec![PathBuf::new()];
    for comp in pattern.components() {
        let part = comp.as_os_str().to_string_lossy().to_string();
        if !part.contains(['*', '?']) {
            for p in &mut current {
                p.push(&part);
            }
            continue;
        }
        let mut next = Vec::new();
        for dir in &current {
            // 빈 경로는 현재 디렉토리를 뜻하지만, Include 경로는 항상 base_dir 이나
            // 절대경로에서 시작하므로 여기 오면 읽을 게 없다.
            let read_dir = if dir.as_os_str().is_empty() {
                continue;
            } else {
                std::fs::read_dir(dir)
            };
            let entries = match read_dir {
                Ok(e) => e,
                Err(e) => {
                    tracing::warn!(dir = %dir.display(), error = %e, "ssh_config: Include glob directory unreadable");
                    continue;
                }
            };
            let mut matched: Vec<PathBuf> = entries
                .filter_map(|entry| match entry {
                    Ok(entry) => Some(entry),
                    Err(e) => {
                        tracing::warn!(dir = %dir.display(), error = %e, "ssh_config: Include glob entry unreadable");
                        None
                    }
                })
                .filter(|entry| glob_match(&part, &entry.file_name().to_string_lossy()))
                .map(|entry| entry.path())
                .collect();
            // read_dir 순서는 OS 마다 다르다 — 같은 설정이 매번 같은 순서를 내도록 고정한다.
            matched.sort();
            next.extend(matched);
        }
        current = next;
    }
    current
}

/// `*`(0 자 이상) / `?`(1 자) 만 있는 단순 glob 매칭. 문자 클래스(`[...]`)는 지원하지
/// 않는다 — ssh config 의 Include 에서 실제로 쓰이는 형태가 `conf.d/*` 계열이라
/// 지원 범위를 좁히고 동작을 예측 가능하게 두는 쪽을 택했다.
fn glob_match(pattern: &str, name: &str) -> bool {
    let p: Vec<char> = pattern.chars().collect();
    let n: Vec<char> = name.chars().collect();
    // 표준 반복형 백트래킹 — `*` 의 마지막 위치를 기억했다가 실패 시 한 칸 물린다.
    let (mut pi, mut ni) = (0usize, 0usize);
    let (mut star, mut backtrack) = (None, 0usize);
    while ni < n.len() {
        if pi < p.len() && (p[pi] == '?' || p[pi] == n[ni]) {
            pi += 1;
            ni += 1;
        } else if pi < p.len() && p[pi] == '*' {
            star = Some(pi);
            backtrack = ni;
            pi += 1;
        } else if let Some(s) = star {
            pi = s + 1;
            backtrack += 1;
            ni = backtrack;
        } else {
            return false;
        }
    }
    p[pi..].iter().all(|&c| c == '*')
}

/// 설정 한 줄을 `(소문자 키워드, 인자들)` 로 자른다. 인자가 없거나 주석·빈 줄이면 `None`.
///
/// ssh_config(5) 문법: 키워드는 대소문자 무시, 키워드와 첫 인자는 공백 **또는** `=` 로
/// 구분(`Host=foo`), `#` 이후는 주석, 인자는 따옴표로 감쌀 수 있다.
fn split_line(line: &str) -> Option<(String, Vec<String>)> {
    let chars: Vec<char> = line.chars().collect();
    let mut i = 0;
    while i < chars.len() && chars[i].is_whitespace() {
        i += 1;
    }
    if i >= chars.len() || chars[i] == '#' {
        return None;
    }
    let start = i;
    while i < chars.len() && !chars[i].is_whitespace() && chars[i] != '=' {
        i += 1;
    }
    let keyword: String = chars[start..i].iter().collect::<String>().to_lowercase();

    // 키워드와 첫 인자 사이의 `=` 는 한 번만 구분자로 인정한다. 인자 안의 `=`
    // (`Include conf=d/x`)까지 자르면 경로가 깨진다.
    while i < chars.len() && chars[i].is_whitespace() {
        i += 1;
    }
    if i < chars.len() && chars[i] == '=' {
        i += 1;
    }

    let mut args = Vec::new();
    let mut cur = String::new();
    let mut quote: Option<char> = None;
    while i < chars.len() {
        let c = chars[i];
        i += 1;
        match quote {
            Some(q) if c == q => quote = None,
            Some(_) => cur.push(c),
            None if c == '"' || c == '\'' => quote = Some(c),
            // 따옴표 밖의 `#` 부터는 주석이다.
            None if c == '#' => break,
            None if c.is_whitespace() => {
                if !cur.is_empty() {
                    args.push(std::mem::take(&mut cur));
                }
            }
            None => cur.push(c),
        }
    }
    if !cur.is_empty() {
        args.push(cur);
    }
    if args.is_empty() {
        return None;
    }
    Some((keyword, args))
}

/// 가져오기가 거부되는 두 경우. 문구는 표면(CLI/IPC)이 각자 만든다 — 이 크레이트는
/// 사용자 대면 문자열을 갖지 않는다.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImportError {
    /// 로컬 ssh config 에 그런 alias 가 없다(오타 방지).
    UnknownAlias(String),
    /// 같은 이름의 tasty 프로필이 이미 있다. 덮어쓰기는 하지 않는다 — `upsert` 로
    /// 조용히 갈아엎으면 사용자가 손으로 맞춰둔 프로필이 사라진다.
    NameTaken(String),
}

/// ssh config alias 하나를 tasty ssh 프로필로 옮길 준비를 한다(저장은 호출자 몫).
///
/// **alias 문자열만 `host` 에 담는다.** `HostName`/`User`/`Port`/`ProxyJump`/
/// `IdentityFile` 을 펼쳐 복사하지 않는 이유는 두 가지다: (1) ssh 는 `host` 자리의
/// alias 를 그대로 해석하므로 복사 없이도 접속이 되고, (2) 복사하는 순간 ssh config
/// 가 바뀔 때마다 값이 어긋난다(drift). [`SshConfigHost`] 의 hint 를 여기에 쓰지
/// 않는 것도 같은 이유다 — hint 는 목록 표시 전용이다.
///
/// `shell` 은 `add-ssh` 의 기본값과 같은 `"auto"` 로 둔다. **셸 감지는 하지 않는다** —
/// 감지는 실제 SSH 접속을 발생시켜서, 목록에서 여러 건을 가져올 때 예기치 않은 접속이
/// 연쇄로 일어난다. 감지는 사용자가 `remote-profile detect` 로 따로 돌린다.
pub fn prepare_import(
    profiles: &RemoteProfiles,
    hosts: &[SshConfigHost],
    alias: &str,
    name: &str,
    label: Option<String>,
) -> Result<RemoteProfile, ImportError> {
    if !hosts.iter().any(|h| h.alias == alias) {
        return Err(ImportError::UnknownAlias(alias.to_string()));
    }
    if profiles.get(name).is_some() {
        return Err(ImportError::NameTaken(name.to_string()));
    }
    let mut p = RemoteProfile::new(name, "ssh");
    p.set_field("host", alias.to_string());
    p.set_field("shell", "auto".to_string());
    p.label = label;
    Ok(p)
}

/// 이 alias 를 이미 참조하는 ssh 프로필 이름(목록의 "IMPORTED" 열).
///
/// 판정 기준은 `host` 필드가 alias 와 **글자 그대로 같은가** 하나뿐이다. `user@alias`
/// 처럼 손으로 변형한 값은 같은 호스트를 가리키더라도 여기서 세지 않는다 — 그 해석은
/// ssh 의 몫이고, 목록이 추측을 시작하면 "가져왔다" 표시를 믿을 수 없게 된다.
pub fn imported_as<'a>(profiles: &'a RemoteProfiles, alias: &str) -> Option<&'a str> {
    profiles
        .profiles
        .iter()
        .find(|p| p.as_ssh().and_then(|v| v.host()) == Some(alias))
        .map(|p| p.name.as_str())
}

#[cfg(test)]
// 테스트 본문은 `let _ =` 사유 주석 정책의 범위 밖이다(전수 가드가 제외한다) —
// 여기 경고는 조치 대상이 될 수 없어 프로덕션 신호만 가린다. error-handling.md.
#[allow(clippy::let_underscore_must_use)]
mod tests {
    use super::*;

    /// 스펙 픽스처의 메인 config. 기대 결과는 `["gx10", "bastion", "work"]`.
    const MAIN: &str = r#"
Host gx10
    HostName 10.0.0.5
    User zilhak
    Port 2200

Host *
    ServerAliveInterval 30

Host bastion jump-*
    HostName jump.example.com

Match exec "test -n \"$WORK\""
    Host after-match-host

Include extra.conf
"#;

    const EXTRA: &str = r#"
Host work
    HostName work.internal
Host gx10
    Port 2201
"#;

    /// 픽스처 디렉토리. **유일 이름은 OS 가 준다** — `name` 은 사람이 읽으라고 붙이는
    /// 접두사일 뿐 격리를 지지 않는다.
    ///
    /// 이전 형태는 `temp_dir()/tasty-ssh-config-{name}` 고정 경로였고 "테스트마다 다른
    /// 이름을 주므로 병렬 실행에도 겹치지 않는다" 고 적었다. 그 문장은 **한 완주 안에서만**
    /// 참이다 — 동시에 도는 두 완주는 같은 이름을 쓰고, 이 헬퍼가 시작하며 `remove_dir_all`
    /// 을 부르므로 늦게 들어온 쪽이 **살아 있는 옆 완주의 픽스처를 통째로 지운다**
    /// (ADR-0129 형태 B, 고정 경로).
    ///
    /// pid 를 섞지 않는다는 원래 결정("이 모듈은 프로세스를 만지지 않는다는 계약을 테스트
    /// 코드에서도 지킨다")은 그대로 지킨다 — `tempfile` 은 pid 를 읽는 것이 아니라 OS 에
    /// 유일 경로를 요청하는 것이라 그 계약과 무관하다. 같은 레포의
    /// `tasty-plugin-agent-common` prompt_file 테스트가 이미 이 형태다.
    ///
    /// 반환값을 살려 둬야 한다 — [`tempfile::TempDir`] 의 Drop 이 디렉토리를 지운다.
    /// (고정 경로 형태는 지우는 주체가 *다음* 완주였고, 그래서 /tmp 에 잔재가 계속 남았다.)
    fn tmpdir(name: &str) -> tempfile::TempDir {
        tempfile::Builder::new()
            .prefix(&format!("tasty-ssh-config-{name}-"))
            .tempdir()
            .expect("tempdir")
    }

    fn write(dir: &Path, name: &str, body: &str) -> PathBuf {
        let p = dir.join(name);
        if let Some(parent) = p.parent() {
            std::fs::create_dir_all(parent).expect("parent dir");
        }
        std::fs::write(&p, body).expect("fixture written");
        p
    }

    #[test]
    fn collects_literal_aliases_only() {
        let dir = tmpdir("literal");
        let main = write(dir.path(), "config", MAIN);
        write(dir.path(), "extra.conf", EXTRA);
        let names: Vec<String> = enumerate_hosts_at(&main)
            .into_iter()
            .map(|h| h.alias)
            .collect();
        // `Host *` 와 `jump-*` 는 패턴이라 제외, 같은 줄의 `bastion` 은 수집.
        // `after-match-host` 는 Match 블록 뒤에 오지만 ssh(1) 기준 새 무조건
        // 블록이라 수집한다([`Scan::parse`] 의 "host" arm 주석 참조).
        assert_eq!(names, vec!["gx10", "bastion", "after-match-host", "work"]);
    }

    /// `Match` 는 Host 컨텍스트만 끊는다 — 그 안의 설정은 직전 Host 로 새지 않고,
    /// 뒤따르는 `Host` 는 (ssh(1) 과 같이) 새 블록으로 다시 수집된다.
    #[test]
    fn match_block_ends_host_context_but_not_collection() {
        let dir = tmpdir("match");
        let main = write(
            dir.path(),
            "config",
            "Host a\n  HostName ha\nMatch host b\n  User inside\nHost after-match\n  User me\n",
        );
        let hosts = enumerate_hosts_at(&main);
        assert_eq!(hosts.len(), 2);
        assert_eq!(hosts[0].alias, "a");
        // Match 안의 User 가 직전 Host 블록으로 새지 않는다.
        assert_eq!(hosts[0].user, None);
        // Match 뒤의 Host 는 살아 있고 자기 블록의 hint 를 갖는다.
        assert_eq!(hosts[1].alias, "after-match");
        assert_eq!(hosts[1].user.as_deref(), Some("me"));
    }

    #[test]
    fn include_resolves_relative_to_config_dir() {
        let dir = tmpdir("include-rel");
        let main = write(dir.path(), "config", "Include extra.conf\n");
        write(
            dir.path(),
            "extra.conf",
            "Host work\n  HostName work.internal\n",
        );
        let hosts = enumerate_hosts_at(&main);
        assert_eq!(hosts.len(), 1);
        assert_eq!(hosts[0].alias, "work");
        assert_eq!(hosts[0].source, dir.path().join("extra.conf"));
    }

    #[test]
    fn include_glob_expands_and_is_ordered() {
        let dir = tmpdir("include-glob");
        let main = write(dir.path(), "config", "Include conf.d/*.conf\n");
        write(dir.path(), "conf.d/b.conf", "Host bee\n");
        write(dir.path(), "conf.d/a.conf", "Host ay\n");
        write(dir.path(), "conf.d/skip.txt", "Host nope\n");
        let names: Vec<String> = enumerate_hosts_at(&main)
            .into_iter()
            .map(|h| h.alias)
            .collect();
        // 확장자가 안 맞는 파일은 제외되고, read_dir 순서와 무관하게 정렬된다.
        assert_eq!(names, vec!["ay", "bee"]);
    }

    #[test]
    fn include_cycle_does_not_hang() {
        let dir = tmpdir("cycle");
        let main = write(dir.path(), "config", "Host root\nInclude a.conf\n");
        write(dir.path(), "a.conf", "Host a\nInclude b.conf\n");
        write(dir.path(), "b.conf", "Host b\nInclude a.conf\n");
        let names: Vec<String> = enumerate_hosts_at(&main)
            .into_iter()
            .map(|h| h.alias)
            .collect();
        assert_eq!(names, vec!["root", "a", "b"]);
    }

    #[test]
    fn missing_config_returns_empty_not_error() {
        let dir = tmpdir("missing");
        let hosts = enumerate_hosts_at(&dir.path().join("nope").join("config"));
        assert!(hosts.is_empty());
    }

    #[test]
    fn duplicate_alias_kept_once_in_first_seen_order() {
        let dir = tmpdir("dup");
        let main = write(
            dir.path(),
            "config",
            "Host gx10\n  Port 2200\nInclude extra.conf\n",
        );
        write(dir.path(), "extra.conf", "Host gx10\n  Port 2201\n");
        let hosts = enumerate_hosts_at(&main);
        assert_eq!(hosts.len(), 1);
        // 뒤에 나온 블록의 hint 가 먼저 수집된 항목을 덮지 않는다.
        assert_eq!(hosts[0].port, Some(2200));
        assert_eq!(hosts[0].source, main);
    }

    #[test]
    fn keyword_case_and_equals_separator() {
        let dir = tmpdir("syntax");
        let main = write(
            dir.path(),
            "config",
            "host=gx10\n  hostname = 10.0.0.5\nHOST bastion\n  PORT\t2222\n",
        );
        let hosts = enumerate_hosts_at(&main);
        assert_eq!(hosts.len(), 2);
        assert_eq!(hosts[0].alias, "gx10");
        assert_eq!(hosts[0].hostname.as_deref(), Some("10.0.0.5"));
        assert_eq!(hosts[1].alias, "bastion");
        assert_eq!(hosts[1].port, Some(2222));
    }

    #[test]
    fn comments_and_quotes_are_handled() {
        let dir = tmpdir("comment");
        let main = write(
            dir.path(),
            "config",
            "# 전체 주석\nHost gx10 # 줄 끝 주석\n  User \"space name\"\n",
        );
        let hosts = enumerate_hosts_at(&main);
        assert_eq!(hosts.len(), 1);
        assert_eq!(hosts[0].alias, "gx10");
        assert_eq!(hosts[0].user.as_deref(), Some("space name"));
    }

    #[test]
    fn hint_captures_only_directives_in_own_block() {
        let dir = tmpdir("hint");
        let main = write(dir.path(), "config", MAIN);
        write(dir.path(), "extra.conf", EXTRA);
        let hosts = enumerate_hosts_at(&main);
        let gx10 = &hosts[0];
        assert_eq!(gx10.hostname.as_deref(), Some("10.0.0.5"));
        assert_eq!(gx10.user.as_deref(), Some("zilhak"));
        assert_eq!(gx10.port, Some(2200));
        let bastion = &hosts[1];
        // `Host *` 블록의 값이 상속되지 않는다.
        assert_eq!(bastion.hostname.as_deref(), Some("jump.example.com"));
        assert_eq!(bastion.user, None);
        assert_eq!(bastion.port, None);
    }

    #[test]
    fn unparsable_port_leaves_hint_empty() {
        let dir = tmpdir("port");
        let main = write(dir.path(), "config", "Host gx10\n  Port not-a-number\n");
        let hosts = enumerate_hosts_at(&main);
        assert_eq!(hosts.len(), 1);
        assert_eq!(hosts[0].port, None);
    }

    /// 스펙 픽스처를 그대로 쓴 열거 결과. `slot` 은 실패 시 어느 호출인지 읽으라고 붙는
    /// 접두사이고, 격리는 [`tmpdir`] 가 진다 — 같은 `slot` 을 두 번 줘도 다른 경로가 나온다.
    ///
    /// `dir` 을 바인딩으로 살려 두는 것이 필수다. `tmpdir(slot).path()` 로 바로 쓰면 임시
    /// 디렉토리가 그 식이 끝나는 자리에서 Drop 되어 사라진다.
    fn fixture_hosts(slot: &str) -> Vec<SshConfigHost> {
        let dir = tmpdir(slot);
        let main = write(dir.path(), "config", MAIN);
        write(dir.path(), "extra.conf", EXTRA);
        enumerate_hosts_at(&main)
    }

    #[test]
    fn import_stores_alias_in_host_field_only() {
        let hosts = fixture_hosts("import-fields");
        let p = prepare_import(&RemoteProfiles::default(), &hosts, "gx10", "my-gpu", None)
            .expect("import prepared");
        assert_eq!(p.kind, "ssh");
        let ssh = p.as_ssh().expect("ssh view");
        assert_eq!(ssh.host(), Some("gx10"));
        assert_eq!(ssh.shell(), "auto");
        // hint(HostName/User/Port)를 펼쳐 담지 않는다 — 해석은 ssh 몫.
        assert!(!p.fields.contains_key("user"));
        assert!(!p.fields.contains_key("port"));
        assert!(!p.fields.contains_key("extra_options"));
        assert!(p.passkey_ref.is_none());
        assert_eq!(p.fields.len(), 2, "host + shell 만 저장한다");
    }

    #[test]
    fn import_keeps_optional_label() {
        let hosts = fixture_hosts("import-label");
        let p = prepare_import(
            &RemoteProfiles::default(),
            &hosts,
            "gx10",
            "my-gpu",
            Some("GPU box".into()),
        )
        .expect("import prepared");
        assert_eq!(p.label.as_deref(), Some("GPU box"));
    }

    #[test]
    fn import_rejects_unknown_alias() {
        let hosts = fixture_hosts("import-unknown");
        let err = prepare_import(&RemoteProfiles::default(), &hosts, "nope", "x", None)
            .expect_err("unknown alias rejected");
        assert_eq!(err, ImportError::UnknownAlias("nope".into()));
    }

    #[test]
    fn import_rejects_existing_profile_name() {
        let hosts = fixture_hosts("import-conflict");
        let mut profiles = RemoteProfiles::default();
        profiles.upsert(RemoteProfile::new("my-gpu", "ssh").with_field("host", "other"));
        let err = prepare_import(&profiles, &hosts, "gx10", "my-gpu", None)
            .expect_err("name conflict rejected");
        assert_eq!(err, ImportError::NameTaken("my-gpu".into()));
        // 거부됐으므로 기존 프로필은 그대로다(덮어쓰기 없음).
        assert_eq!(
            profiles
                .get("my-gpu")
                .and_then(|p| p.as_ssh())
                .unwrap()
                .host(),
            Some("other")
        );
    }

    #[test]
    fn imported_as_matches_host_field_literally() {
        let mut profiles = RemoteProfiles::default();
        profiles.upsert(RemoteProfile::new("gpu", "ssh").with_field("host", "gx10"));
        profiles.upsert(RemoteProfile::new("other", "ssh").with_field("host", "zilhak@gx10"));
        assert_eq!(imported_as(&profiles, "gx10"), Some("gpu"));
        // 변형된 destination 은 같은 호스트를 가리켜도 "가져옴" 으로 세지 않는다.
        assert_eq!(imported_as(&profiles, "bastion"), None);
    }

    // ── config_availability (존재 / 권한 / 부재) ──────────────────────

    #[test]
    fn config_availability_reports_existing_readable_file() {
        let dir = tmpdir("avail-ok");
        let p = write(dir.path(), "config", "Host a\n");
        let a = config_availability(Some(&p));
        assert!(a.exists);
        assert!(a.readable);
    }

    #[test]
    fn config_availability_reports_missing_file_and_no_home() {
        let dir = tmpdir("avail-missing");
        let a = config_availability(Some(&dir.path().join("nope")));
        assert!(!a.exists);
        // 없는 파일은 열리지도 않는다 — 두 축이 어긋나지 않게 고정한다.
        assert!(!a.readable);
        // 홈을 못 찾은 경우(`user_config_path()` 가 None)도 같은 모양이다.
        assert_eq!(config_availability(None), ConfigAvailability::default());
    }

    /// 권한 때문에 못 읽는 파일은 **존재하지만 읽을 수 없다** 로 잡혀야 한다 — 이걸
    /// 못 가르면 IPC 호출자에게 "설정이 없다" 와 똑같이 보인다.
    #[cfg(unix)]
    #[test]
    fn config_availability_separates_unreadable_from_missing() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tmpdir("avail-perm");
        let p = write(dir.path(), "config", "Host a\n");
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o000)).expect("chmod 000");
        // root 는 퍼미션을 무시하고 읽는다 — 그 환경에서는 이 케이스를 만들 수 없으니
        // 거짓 실패를 내지 않고 건너뛴다(판정 로직이 아니라 픽스처의 한계다).
        if std::fs::File::open(&p).is_ok() {
            return;
        }
        let a = config_availability(Some(&p));
        assert!(a.exists, "파일은 그대로 있다");
        assert!(!a.readable, "권한이 없으면 readable 이 false 여야 한다");
        // 되돌려 둔다 — 0o000 파일이 남아도 `tmpdir` 이 다음 실행에서 통째로 지우지만,
        // 사람이 /tmp 를 뒤질 때 읽을 수 있는 편이 낫다.
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o644))
            .expect("소유자라 퍼미션 복구는 실패할 이유가 없다");
    }

    /// 경로가 디렉토리면 linux 에서 `File::open` 이 성공한다 — 그것만 보면
    /// "읽히는데 Host 가 없다" 로 새서 사용자에게 엉뚱한 안내가 나간다.
    #[test]
    fn config_availability_treats_a_directory_as_unreadable() {
        let dir = tmpdir("avail-dir");
        let as_config = dir.path().join("config");
        std::fs::create_dir(&as_config).expect("디렉토리 생성");
        let a = config_availability(Some(&as_config));
        assert!(a.exists, "경로에 무언가 있긴 하다");
        assert!(
            !a.readable,
            "정규 파일이 아니면 읽을 수 있는 것으로 치지 않는다"
        );
    }

    #[test]
    fn glob_match_basics() {
        assert!(glob_match("*.conf", "a.conf"));
        assert!(!glob_match("*.conf", "a.txt"));
        assert!(glob_match("a?c", "abc"));
        assert!(!glob_match("a?c", "ac"));
        assert!(glob_match("*", "anything"));
        assert!(glob_match("a*b*c", "aXXbYYc"));
        assert!(!glob_match("a*b*c", "aXXbYY"));
    }
}
