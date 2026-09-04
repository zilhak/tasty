//! The landing page. One layout, one copy deck per language.

use crate::md::html_escape;
use crate::shell::{self, ICON_GITHUB, REPO, Shell, Strings};

struct Copy {
    badge: &'static str,
    /// The catchphrase under the eyebrow: the wordmark, then the pun that
    /// only lands in English (`Tasty. Tasty terminal.`).
    tagline: &'static str,
    title_lead: &'static str,
    title_accent: &'static str,
    title_tail: &'static str,
    meta_description: &'static str,
    cta_primary: &'static str,
    cta_secondary: &'static str,
    /// Sentence before the installation-guide link, and the link label.
    install_note: &'static str,
    install_note_link: &'static str,
    /// Primary button label once the visitor's OS is detected; `{os}` is
    /// replaced by the OS name.
    dl_for: &'static str,
    /// Ghost button next to the primary one, pointing at the release page.
    cta_other: &'static str,

    why_title: &'static str,
    why_body: &'static str,
    why_points: &'static [(&'static str, &'static str, &'static str)],

    features_kicker: &'static str,
    features_title: &'static str,
    features_body: &'static str,
    cards: &'static [(&'static str, &'static str, &'static str, &'static str)],

    agents_kicker: &'static str,
    agents_title: &'static str,
    agents_body: &'static str,
    agents_caption: &'static str,

    platform_kicker: &'static str,
    platform_title: &'static str,
    platform_body: &'static str,
    stats: &'static [(&'static str, &'static str)],

    cta_title: &'static str,
    cta_body: &'static str,
    cta_docs: &'static str,
    cta_download: &'static str,
}

const KO_COPY: Copy = Copy {
    badge: "Windows · macOS · Linux",
    tagline: "맛있는 터미널.",
    title_lead: "AI 에이전트와",
    title_accent: "함께 조작하는",
    title_tail: "터미널",
    meta_description: "에이전트에게 일을 맡겨두고, 나는 옆 탭에서 하던 일을 계속합니다. \
           포커스도 스크롤도 그대로입니다.",
    cta_primary: "다운로드",
    cta_secondary: "가이드 보기",
    install_note: "OS 별 설치 절차와 첫 실행은",
    install_note_link: "설치 가이드 →",
    dl_for: "{os} 용 다운로드",
    cta_other: "다른 플랫폼",

    why_title: "에이전트가 일해도 내 자리는 그대로입니다",
    why_body: "에이전트가 탭을 만들든 명령을 보내든, 내가 보던 화면은 움직이지 않습니다. \
               잡아둔 선택도, 스크롤 위치도 그대로입니다. 사용자 입력을 흉내 내는 기능은 아예 없습니다.",
    why_points: &[
        (
            "01",
            "내 조작과 분리",
            "에이전트가 무슨 일을 하든 포커스와 선택, 스크롤, 닫은 탭 기록에는 손대지 않습니다. \
             내가 다른 탭을 보고 있어도 자기 터미널 안에서만 움직입니다.",
        ),
        (
            "02",
            "ID 로 지정",
            "에이전트는 조작할 터미널을 ID 로 찍어서 부릅니다. 지금 무엇이 활성이냐에 따라 \
             엉뚱한 곳에 입력이 들어가는 일이 없습니다.",
        ),
        (
            "03",
            "명령 하나로 전부",
            "분할부터 훅까지 전부 tasty 명령 하나로 합니다. \
             에이전트에게 알려줄 것은 명령어 목록뿐입니다.",
        ),
        (
            "04",
            "창 없이도",
            "서버나 CI 에서 창 없이 띄워도 같은 명령이 그대로 돕니다.",
        ),
    ],

    features_kicker: "기능",
    features_title: "터미널이 해야 할 일과, 에이전트가 필요로 하는 일",
    features_body: "자주 손이 가는 것만 골랐습니다. 나머지는 가이드가 순서대로 다룹니다.",
    cards: &[
        (
            "grid",
            "GPU 렌더링",
            "셀 하나하나를 GPU 가 그립니다. 분할을 열 개 넘게 띄워도 버벅이지 않습니다.",
            "using/panes-tabs-splits.html",
        ),
        (
            "stack",
            "워크스페이스와 프리셋",
            "일감마다 워크스페이스를 따로 둡니다. 자주 쓰는 배치는 프리셋으로 저장해 두고 꺼내 씁니다.",
            "using/workspaces.html",
        ),
        (
            "files",
            "터미널만 있는 게 아닙니다",
            "탐색기와 마크다운, 이미지, 웹 화면을 터미널 옆에 나란히 띄웁니다.",
            "using/files.html",
        ),
        (
            "agents",
            "여러 에이전트를 한 번에",
            "Claude 와 Codex 자식을 띄워두면 끝나는 대로 알려줍니다.",
            "agents/claude-codex.html",
        ),
        (
            "graph",
            "작업 DAG",
            "할 일을 의존 관계로 묶어 두면 순서대로 돕니다. 어디까지 갔는지는 그래프로 봅니다.",
            "agents/tasks.html",
        ),
        (
            "terminal",
            "CLI 로 조작",
            "분할도 명령 전송도 출력 읽기도 tasty 명령 하나입니다. 에이전트가 자기 터미널을 직접 다룹니다.",
            "agents/cli.html",
        ),
        (
            "parse",
            "명령 단위 출력",
            "셸 프롬프트 경계를 알아채서, 방금 돌린 명령의 출력만 딱 읽습니다.",
            "using/terminal.html",
        ),
        (
            "gauge",
            "훅과 알림",
            "프로세스 종료나 특정 출력, 유휴 시간에 훅을 걸어두고 알림을 받습니다.",
            "agents/hooks-notifications.html",
        ),
        (
            "script",
            "Lua 스크립트",
            "단축키나 창 · 탭 이벤트에 스크립트를 걸어두면 손 갈 일이 줄어듭니다.",
            "customize/scripts.html",
        ),
        (
            "link",
            "원격 attach",
            "다른 머신에서 돌고 있는 워크스페이스를 SSH 로 그대로 가져와 봅니다.",
            "remote/attach.html",
        ),
        (
            "plug",
            "플러그인",
            "탐색기와 마크다운, 이미지, git 보기가 기본으로 들어 있습니다. 권한을 확인하고 켜거나 끕니다.",
            "plugins/index.html",
        ),
        (
            "palette",
            "테마",
            "Mocha 나 Latte 를 고르거나, TOML 로 직접 만듭니다.",
            "customize/themes.html",
        ),
    ],

    agents_kicker: "다중 에이전트",
    agents_title: "여러 에이전트를 띄워두고, 끝나는 대로 확인합니다",
    agents_body: "자식을 띄우는 명령은 기다리지 않고 바로 끝납니다. 완료 훅은 알아서 걸립니다. \
                  자식이 멈추거나 입력을 기다리면 띄운 쪽 터미널이 먼저 압니다.",
    agents_caption: "사람이 GUI 에서 하는 일은 에이전트도 CLI 로 똑같이 합니다.",

    platform_kicker: "플랫폼",
    platform_title: "세 OS 모두 1 급",
    platform_body: "세 OS 에서 기능도 단축키도 CLI 도 똑같습니다. \
                    설치 파일은 dmg, msi, deb, rpm, AppImage 로 냅니다.",
    stats: &[
        ("3", "운영체제"),
        ("7", "번들 플러그인"),
        ("3", "UI 언어"),
        ("MIT", "라이선스"),
    ],

    cta_title: "가이드부터 읽어도 되고, 바로 설치해도 됩니다",
    cta_body: "설치부터 에이전트 연동, 원격 attach 까지 가이드가 순서대로 짚어 줍니다.",
    cta_docs: "가이드 읽기",
    cta_download: "다운로드",
};

const EN_COPY: Copy = Copy {
    badge: "Windows · macOS · Linux",
    tagline: "Tasty terminal.",
    title_lead: "A terminal you and",
    title_accent: "your AI agent",
    title_tail: "drive together",
    meta_description: "Hand a terminal to an agent and your own screen holds still. The agent works in \
           its own tab through tasty commands and tells you when it lands. Drawn on the GPU, \
           so a wall of splits still feels immediate.",
    cta_primary: "Download",
    cta_secondary: "Read the guide",
    install_note: "Per-OS install steps and the first launch:",
    install_note_link: "Installation guide →",
    dl_for: "Download for {os}",
    cta_other: "Other platforms",

    why_title: "The agent works, and your seat stays yours",
    why_body: "An agent can open tabs and send commands, and the screen you were looking at, \
               the text you selected, and your scroll position do not move. \
               Nothing in the product imitates user input.",
    why_points: &[
        (
            "01",
            "Separate from your hands",
            "What an agent does never touches focus, selection, scrolling, or the closed-tab \
             history. You can look at another tab while it works in its own.",
        ),
        (
            "02",
            "Addressed by ID",
            "An agent names the terminal it wants to drive by ID. Whatever happens to be active \
             right now never receives input meant for somewhere else.",
        ),
        (
            "03",
            "One command for everything",
            "Splits, sending commands, reading output, notifications, hooks — all through the \
             single tasty command. The only thing an agent needs to learn is the command list.",
        ),
        (
            "04",
            "Windowless too",
            "On a server or in CI, run it with no window and drive terminals with the same \
             commands.",
        ),
    ],

    features_kicker: "Features",
    features_title: "What a terminal owes you, and what an agent needs from one",
    features_body: "The ones people reach for most. The guide takes the rest in order.",
    cards: &[
        (
            "grid",
            "GPU rendering",
            "Every cell is drawn on the GPU. Stays smooth well past ten splits.",
            "using/panes-tabs-splits.html",
        ),
        (
            "stack",
            "Workspaces and presets",
            "One workspace per job, and a layout you keep coming back to saved as a preset.",
            "using/workspaces.html",
        ),
        (
            "files",
            "More than terminals",
            "A file explorer, Markdown, images and web pages sit in the same splits as your shells.",
            "using/files.html",
        ),
        (
            "agents",
            "Several agents at once",
            "Spawn Claude and Codex children and hear back as each one finishes.",
            "agents/claude-codex.html",
        ),
        (
            "graph",
            "Task DAG",
            "Tie work together by dependency and it runs in order. Watch how far it got as a graph.",
            "agents/tasks.html",
        ),
        (
            "terminal",
            "Driven from the CLI",
            "Splits, sending commands, reading output, notifications — one tasty command. An agent drives its own terminal directly.",
            "agents/cli.html",
        ),
        (
            "parse",
            "Per-command output",
            "Recognises shell prompt boundaries, so an agent reads exactly the output of the command it just ran.",
            "using/terminal.html",
        ),
        (
            "gauge",
            "Hooks and notifications",
            "Hook process exit, output patterns, and idle time, and get notified.",
            "agents/hooks-notifications.html",
        ),
        (
            "script",
            "Lua scripts",
            "Bind a script to a shortcut or to window and tab events and let it do the repetitive part.",
            "customize/scripts.html",
        ),
        (
            "link",
            "Remote attach",
            "Bring a workspace from a tasty running on another machine over SSH and see it as is.",
            "remote/attach.html",
        ),
        (
            "plug",
            "Plugins",
            "Explorer, Markdown, image, and git views come bundled. See each one's permissions and switch it on or off.",
            "plugins/index.html",
        ),
        (
            "palette",
            "Themes",
            "Pick a bundled theme such as Mocha or Latte, or write your own in TOML.",
            "customize/themes.html",
        ),
    ],

    agents_kicker: "Multi-agent",
    agents_title: "Spawn several agents, hear back as each one lands",
    agents_body: "The command that spawns a child does not wait around — it returns at once, and \
                  the completion hook arms itself. When a child stalls or wants input, the \
                  terminal that spawned it hears about it first.",
    agents_caption: "Whatever a person does in the GUI, an agent can do from the CLI.",

    platform_kicker: "Platforms",
    platform_title: "All three, first class",
    platform_body: "Nothing differs across the three: not the features, not the shortcuts, not \
                    the CLI. Ships as dmg, msi, deb, rpm, or AppImage.",
    stats: &[
        ("3", "operating systems"),
        ("7", "bundled plugins"),
        ("3", "UI languages"),
        ("MIT", "license"),
    ],

    cta_title: "Start with the guide, or just install it",
    cta_body: "From installing it to wiring up an agent to attaching a remote, the guide walks it in order.",
    cta_docs: "Read the guide",
    cta_download: "Download",
};

pub fn render(strings: &Strings, root: &str, docs: &str) -> String {
    let copy = if strings.lang == "ko" {
        &KO_COPY
    } else {
        &EN_COPY
    };
    let body = format!(
        r##"<main class="landing" id="content">
{hero}
{why}
{features}
{agents}
{platform}
{cta}
</main>"##,
        hero = hero(copy, root, docs),
        why = why(copy),
        features = features(copy, root, docs),
        agents = agents(copy, strings),
        platform = platform(copy),
        cta = cta_band(copy, root, docs),
    );

    shell::document(&Shell {
        strings,
        title: String::new(),
        description: strip_emphasis(copy.meta_description),
        root: root.to_string(),
        body,
        active: "",
        ko_href: "ko/index.html".to_string(),
        en_href: "index.html".to_string(),
        docs_prefix: docs,
        search_index: strings.search_index,
    })
}

fn strip_emphasis(s: &str) -> String {
    s.replace('*', "")
}

fn hero(copy: &Copy, root: &str, docs: &str) -> String {
    format!(
        r##"<section class="hero">
  <div class="hero__intro">
    <span class="hero__eyebrow"><span class="dot"></span>{badge} · v{version}</span>
    <p class="hero__tagline"><span class="wordmark">Tasty<b>.</b></span> {tagline}</p>
    <h1>{lead} <span class="accent">{accent}</span> {tail}</h1>
    <div class="cta-row">
      <a class="btn btn--primary" id="dl-primary" href="{releases}" data-label="{dl_for}"{primary_data}>{primary}</a>
      <a class="btn btn--ghost" href="{releases}">{other}</a>
      <a class="btn btn--ghost" href="{root}{docs}index.html">{secondary}</a>
      <a class="btn btn--ghost" href="{repo}">{github} GitHub</a>
    </div>
    <p class="hero__note">{note} <a href="{root}{docs}getting-started/install.html">{note_link}</a></p>
  </div>
  {mock}
</section>"##,
        badge = html_escape(copy.badge),
        tagline = html_escape(copy.tagline),
        version = crate::version(),
        lead = html_escape(copy.title_lead),
        accent = html_escape(copy.title_accent),
        tail = html_escape(copy.title_tail),
        root = root,
        docs = docs,
        primary = html_escape(copy.cta_primary),
        secondary = html_escape(copy.cta_secondary),
        repo = REPO,
        github = ICON_GITHUB,
        note = html_escape(copy.install_note),
        note_link = html_escape(copy.install_note_link),
        releases = releases_url(),
        dl_for = html_escape(copy.dl_for),
        other = html_escape(copy.cta_other),
        primary_data = primary_data_attrs(),
        mock = mock(root),
    )
}

/// The product window, transcribed structurally from the design system's
/// `ui_kits/terminal` kit (`app.jsx` composition: TitleBar → Sidebar | TabStrip
/// → split surfaces → StatusBar). Every dimension is a design pixel (`--u`) and
/// every colour maps to the semantic token the app itself renders, so the
/// window re-themes live with the toggle instead of being a baked screenshot.
fn mock(root: &str) -> String {
    let version = crate::version();
    format!(
        r##"<div class="mock-wrap"><div class="mock" aria-hidden="true">
  <div class="mock__titlebar">
    <span class="mock__traffic"><i></i><i></i><i></i></span>
    <span class="mock__title"><img src="{root}assets/tasty-melon.svg" alt="" width="16" height="16"><span>agents-prod</span><span class="mock__title-app">— tasty</span></span>
    <span class="mock__titlebar-spacer"></span>
  </div>
  <div class="mock__body">
    <div class="mock__sidebar">
      <div class="mock__sb-head"><img src="{root}assets/tasty-melon.svg" alt="" width="22" height="22"><span class="mock__wordmark">tasty<b>.</b></span><span class="mock__iconbtn">{chevrons}</span></div>
      <div class="mock__heading">Workspaces</div>
      <div class="mock__ws-list">
        <div class="mock__ws" data-active><span class="mock__dot-slot"><span class="mock__dot mock__dot--running mock__dot--pulse"></span></span><span class="mock__ws-text"><span class="mock__ws-name">agents-prod</span></span></div>
        <div class="mock__ws"><span class="mock__dot-slot"><span class="mock__dot"></span></span><span class="mock__ws-text"><span class="mock__ws-name">infra</span><span class="mock__mirror">{mirror}remote</span><span class="mock__ws-sub">mirror → prod-web</span></span></div>
        <div class="mock__ws"><span class="mock__dot-slot"><span class="mock__dot mock__dot--agent mock__dot--pulse mock__dot--attached"></span></span><span class="mock__ws-text"><span class="mock__ws-name">api-gateway</span><span class="mock__ws-desc">deploy to staging on every push to main</span></span><span class="mock__dot-slot"><span class="mock__badge">2</span></span></div>
        <div class="mock__ws"><span class="mock__dot-slot"><span class="mock__dot"></span></span><span class="mock__ws-text"><span class="mock__ws-name">scratch</span></span></div>
      </div>
      <div class="mock__sb-new"><span class="mock__ghost">{plus}New Workspace</span></div>
      <div class="mock__sb-foot">
        <span class="mock__ghost">{tools}Tools</span>
        <span class="mock__ghost">{plug}Plugins</span>
        <span class="mock__ghost">{settings}Settings</span>
      </div>
    </div>
    <div class="mock__work">
      <div class="mock__tabs">
        <span class="mock__tab" data-active><span class="mock__tab-icon">{terminal}</span><span class="mock__tab-label">build · cargo</span><span class="mock__dot mock__dot--running"></span><span class="mock__tab-close">{close}</span></span>
        <span class="mock__tab" data-notif><span class="mock__tab-icon">{markdown}</span><span class="mock__tab-label">README.md</span></span>
        <span class="mock__tab"><span class="mock__tab-icon">{terminal}</span><span class="mock__tab-label">scratch</span></span>
        <span class="mock__tabs-add"><span class="mock__iconbtn">{plus}</span></span>
        <span class="mock__tabs-right"><span class="mock__iconbtn">{split}</span><span class="mock__iconbtn">{search}</span></span>
      </div>
      <div class="mock__panes">
        <div class="mock__term">
          <div class="l"><span class="m">❯</span> cargo build --release</div>
          <div class="l dim">   Compiling tasty-themes v{version}</div>
          <div class="l dim">   Compiling tasty-plugin-claude v{version}</div>
          <div class="l dim">   Compiling tasty v{version}</div>
          <div class="l"><span class="g">    Finished</span> <span class="y">release</span> [optimized] in 42.18s</div>
          <div class="l"><span class="m">❯</span> tasty notify "build ok" --title cargo</div>
          <div class="l"><span class="b">notification</span> <span class="g">sent</span> id=n_0142</div>
        </div>
        <div class="mock__term" data-focused>
          <div class="l"><span class="g">~/tasty</span> <span class="b">main</span> <span class="dim">via</span> <span class="p">🦀 v1.84</span></div>
          <div class="l"><span class="m">❯</span> tasty claude spawn --workspace review --role verifier</div>
          <div class="l"><span class="b">child</span> <span class="g">spawned</span> surface=41 notify=armed</div>
          <div class="l"><span class="m">❯</span> tasty read since-mark --surface 41 --strip-ansi</div>
          <div class="l dim">verifier: clippy clean · 212 tests passed</div>
          <div class="l"><span class="g">~/tasty</span> <span class="b">main</span> <span class="dim">via</span> <span class="p">🦀 v1.84</span></div>
          <div class="l"><span class="m">❯</span> <span class="cursor"></span></div>
        </div>
      </div>
      <div class="mock__status">
        <span class="mock__cell mock__cell--branch"><i></i>main</span>
        <span class="mock__cell">s_02JK</span>
        <span class="mock__cell">zsh · 120×34</span>
        <span class="mock__cell mock__cell--right"><kbd>Ctrl+K</kbd> palette</span>
        <span class="mock__cell"><i class="mock__theme-dot"></i><span class="mock__theme-name" data-dark>Mocha</span><span class="mock__theme-name" data-light>Latte</span></span>
      </div>
    </div>
  </div>
</div></div>"##,
        root = root,
        version = version,
        chevrons = glyph(r#"<path d="m11 17-5-5 5-5M18 17l-5-5 5-5"/>"#),
        mirror = glyph(r#"<path d="M4 17l6-6-6-6M12 19h8"/>"#),
        plus = glyph(r#"<path d="M12 5v14M5 12h14"/>"#),
        tools = glyph(
            r#"<path d="M14.7 6.3a4 4 0 0 1-5.4 5.4L4 17v3h3l5.3-5.3a4 4 0 0 1 5.4-5.4l-2.7 2.7-2-2 2.7-2.7z"/>"#
        ),
        plug = glyph(r#"<path d="M9 2v6M15 2v6M7 8h10v3a5 5 0 0 1-10 0V8zM12 16v6"/>"#),
        settings = glyph(
            r#"<circle cx="12" cy="12" r="3"/><path d="M12 2v2m0 16v2M4.9 4.9l1.4 1.4m11.4 11.4 1.4 1.4M2 12h2m16 0h2M4.9 19.1l1.4-1.4M17.7 6.3l1.4-1.4"/>"#
        ),
        terminal = glyph(
            r#"<rect x="3" y="4" width="18" height="16" rx="2"/><path d="m7 9 3 3-3 3M13 15h4"/>"#
        ),
        markdown = glyph(
            r#"<rect x="3" y="5" width="18" height="14" rx="2"/><path d="M7 15V9l2.5 3L12 9v6M16 9v4m0 0 2-2m-2 2-2-2"/>"#
        ),
        close = glyph(r#"<path d="M18 6 6 18M6 6l12 12"/>"#),
        split = glyph(r#"<rect x="3" y="4" width="18" height="16" rx="2"/><path d="M12 4v16"/>"#),
        search = glyph(r#"<circle cx="11" cy="11" r="7"/><path d="m21 21-4.3-4.3"/>"#),
    )
}

/// One glyph from the design system's canonical icon set (`icons/*.svg`):
/// a 24-unit box, 2px stroke, round caps. The size comes from CSS.
fn glyph(paths: &str) -> String {
    format!(
        r#"<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">{paths}</svg>"#
    )
}

/// The format the OS-detected primary button offers: the installer on
/// macOS/Windows, and the distro-agnostic AppImage (x86_64) on Linux.
const PRIMARY_FORMATS: &[(&str, &str)] = &[
    ("macos", "-macos-arm64.dmg"),
    ("windows", "-windows-x64.msi"),
    ("linux", "-x86_64.AppImage"),
];

fn releases_url() -> String {
    format!("{REPO}/releases/latest")
}

/// Browser download URL of the release asset whose file name ends with
/// `suffix`, if the release manifest is present and has it.
fn asset_url(suffix: &str) -> Option<&'static str> {
    crate::release()?
        .assets
        .iter()
        .find(|(name, _)| name.ends_with(suffix))
        .map(|(_, url)| url.as_str())
}

/// `data-<os>` attributes on the primary button; `site.js` swaps the
/// button to the visitor's OS when the matching asset exists.
fn primary_data_attrs() -> String {
    PRIMARY_FORMATS
        .iter()
        .filter_map(|(os, suffix)| asset_url(suffix).map(|url| format!(" data-{os}=\"{url}\"")))
        .collect()
}

fn why(copy: &Copy) -> String {
    let points = copy
        .why_points
        .iter()
        .map(|(k, title, body)| {
            format!(
                "<li><span class=\"k\">{k}</span><span class=\"v\"><strong>{t}</strong>\
                 <span>{b}</span></span></li>",
                k = html_escape(k),
                t = html_escape(title),
                b = html_escape(body),
            )
        })
        .collect::<String>();

    format!(
        r##"<section class="section">
  <div class="split">
    <div class="section__head" style="margin-bottom:0">
      <h2>{title}</h2>
      <p>{body}</p>
    </div>
    <ul class="feature-list">{points}</ul>
  </div>
</section>"##,
        title = html_escape(copy.why_title),
        body = html_escape(copy.why_body),
        points = points,
    )
}

fn features(copy: &Copy, root: &str, docs: &str) -> String {
    let cards = copy
        .cards
        .iter()
        .map(|(icon, title, body, href)| {
            format!(
                r##"<a class="card" href="{root}{docs}{href}">
  <span class="card__icon">{icon}</span>
  <h3>{title}</h3>
  <p>{body}</p>
  <span class="card__more">→</span>
</a>"##,
                root = root,
                docs = docs,
                href = href,
                icon = icon_svg(icon),
                title = html_escape(title),
                body = html_escape(body),
            )
        })
        .collect::<String>();

    format!(
        r##"<section class="section">
  <div class="section__head">
    <div class="section__kicker">{kicker}</div>
    <h2>{title}</h2>
    <p>{body}</p>
  </div>
  <div class="card-grid">{cards}</div>
</section>"##,
        kicker = html_escape(copy.features_kicker),
        title = html_escape(copy.features_title),
        body = html_escape(copy.features_body),
        cards = cards,
    )
}

fn agents(copy: &Copy, strings: &Strings) -> String {
    let snippet = r#"<span class="g">$</span> tasty claude spawn --workspace build \
    --role <span class="y">implementer</span> --prompt <span class="g">"fix the flaky test"</span>
<span class="dim">→ child surface 52 · completion hook armed</span>

<span class="g">$</span> tasty agent barrier wait --name <span class="y">review-ready</span> --parties 3
<span class="dim">→ 2/3 arrived …</span>

<span class="g">$</span> tasty read queue --surface 52
<span class="m">[from 52]</span> done · 1 file changed, 12 insertions"#;

    format!(
        r##"<section class="section">
  <div class="split split--rev">
    <div class="mock" aria-hidden="true">
      <div class="mock__chrome">
        <span class="mock__tab" data-active>{tab}</span>
      </div>
      <div class="mock__body" style="grid-template-columns:1fr">
        <div class="mock__pane" style="min-height:0">{snippet}</div>
      </div>
    </div>
    <div class="section__head" style="margin-bottom:0">
      <div class="section__kicker">{kicker}</div>
      <h2>{title}</h2>
      <p>{body}</p>
      <p style="font-size:14px;color:var(--overlay1);margin-top:16px">{caption}</p>
    </div>
  </div>
</section>"##,
        tab = if strings.lang == "ko" {
            "오케스트레이터"
        } else {
            "orchestrator"
        },
        snippet = snippet,
        kicker = html_escape(copy.agents_kicker),
        title = html_escape(copy.agents_title),
        body = html_escape(copy.agents_body),
        caption = html_escape(copy.agents_caption),
    )
}

fn platform(copy: &Copy) -> String {
    let stats = copy
        .stats
        .iter()
        .map(|(n, l)| {
            format!(
                "<div class=\"stat\"><div class=\"stat__n\">{n}</div>\
                 <div class=\"stat__l\">{l}</div></div>",
                n = html_escape(n),
                l = html_escape(l),
            )
        })
        .collect::<String>();

    format!(
        r##"<section class="section">
  <div class="section__head">
    <div class="section__kicker">{kicker}</div>
    <h2>{title}</h2>
    <p>{body}</p>
  </div>
  <div class="platform-row">
    <span class="platform">{i_win} Windows</span>
    <span class="platform">{i_mac} macOS</span>
    <span class="platform">{i_lin} Linux</span>
  </div>
  <div class="stat-row">{stats}</div>
</section>"##,
        kicker = html_escape(copy.platform_kicker),
        title = html_escape(copy.platform_title),
        body = html_escape(copy.platform_body),
        i_win = icon_svg("windows"),
        i_mac = icon_svg("apple"),
        i_lin = icon_svg("linux"),
        stats = stats,
    )
}

fn cta_band(copy: &Copy, root: &str, docs: &str) -> String {
    format!(
        r##"<section class="cta-band">
  <h2>{title}</h2>
  <p>{body}</p>
  <div class="cta-row">
    <a class="btn btn--primary" href="{root}{docs}index.html">{docs_label}</a>
    <a class="btn btn--ghost" href="{releases}">{download}</a>
  </div>
</section>"##,
        title = html_escape(copy.cta_title),
        body = html_escape(copy.cta_body),
        root = root,
        docs = docs,
        docs_label = html_escape(copy.cta_docs),
        releases = releases_url(),
        download = html_escape(copy.cta_download),
    )
}

/// Wraps icon path data in a consistent `<svg>` element.
macro_rules! icon {
    ($paths:expr) => {
        concat!(
            r#"<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" "#,
            r#"stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">"#,
            $paths,
            "</svg>"
        )
    };
}

fn icon_svg(name: &str) -> &'static str {
    match name {
        "grid" => icon!(
            r#"<rect x="3" y="3" width="7" height="7" rx="1"/><rect x="14" y="3" width="7" height="7" rx="1"/><rect x="3" y="14" width="7" height="7" rx="1"/><rect x="14" y="14" width="7" height="7" rx="1"/>"#
        ),
        "agents" => icon!(
            r#"<circle cx="12" cy="5" r="2.4"/><circle cx="5" cy="18" r="2.4"/><circle cx="19" cy="18" r="2.4"/><path d="M12 7.4v3.4M12 10.8 6.4 15.8M12 10.8l5.6 5"/>"#
        ),
        "terminal" => icon!(
            r#"<rect x="2.5" y="4" width="19" height="16" rx="2"/><path d="m7 9 3 3-3 3M13 15h4"/>"#
        ),
        "keyboard" => icon!(
            r#"<rect x="2" y="6" width="20" height="12" rx="2"/><path d="M6 10h.01M10 10h.01M14 10h.01M18 10h.01M8 14h8"/>"#
        ),
        "plug" => {
            icon!(r#"<path d="M9 3v6M15 3v6M6 9h12v3a6 6 0 0 1-12 0z"/><path d="M12 18v3"/>"#)
        }
        "link" => icon!(
            r#"<path d="M10 13a5 5 0 0 0 7.5.5l2-2a5 5 0 0 0-7-7l-1 1"/><path d="M14 11a5 5 0 0 0-7.5-.5l-2 2a5 5 0 0 0 7 7l1-1"/>"#
        ),
        "parse" => icon!(r#"<path d="M4 6h10M4 12h16M4 18h7"/><path d="m17 15 3 3-3 3"/>"#),
        "gauge" => icon!(
            r#"<path d="M4 18a8 8 0 1 1 16 0"/><path d="m12 14 4-4"/><circle cx="12" cy="18" r="1.4"/>"#
        ),
        "palette" => icon!(
            r#"<path d="M12 3a9 9 0 1 0 0 18 2 2 0 0 0 1.6-3.2 2 2 0 0 1 1.6-3.2H18a3 3 0 0 0 3-3 9 9 0 0 0-9-8.6z"/><circle cx="8" cy="10" r="1"/><circle cx="12" cy="7.5" r="1"/><circle cx="16" cy="10" r="1"/>"#
        ),
        "stack" => icon!(
            r#"<path d="M12 3 3 7.5 12 12l9-4.5z"/><path d="m3 12 9 4.5 9-4.5"/><path d="m3 16.5 9 4.5 9-4.5"/>"#
        ),
        "files" => icon!(
            r#"<path d="M3 6.5A1.5 1.5 0 0 1 4.5 5H9l2 2.5h8.5A1.5 1.5 0 0 1 21 9v8.5a1.5 1.5 0 0 1-1.5 1.5h-15A1.5 1.5 0 0 1 3 17.5z"/>"#
        ),
        "graph" => icon!(
            r#"<rect x="2.5" y="4" width="6" height="5" rx="1"/><rect x="2.5" y="15" width="6" height="5" rx="1"/><rect x="15.5" y="9.5" width="6" height="5" rx="1"/><path d="M8.5 6.5h3.5v5.5h3.5M8.5 17.5h3.5V12"/>"#
        ),
        "script" => icon!(
            r#"<path d="M6 3h8l4 4v14H6z"/><path d="M14 3v4h4"/><path d="m10 12.5-1.5 1.5L10 15.5M13.5 12.5 15 14l-1.5 1.5"/>"#
        ),
        "windows" => icon!(
            r#"<path d="M3 6.5 10 5.4v6.1H3zM11.5 5.2 21 4v7.5h-9.5zM3 12.5h7v6.1L3 17.5zM11.5 12.5H21V20l-9.5-1.2z"/>"#
        ),
        "apple" => icon!(
            r#"<path d="M16.2 12.6c0-2.3 1.9-3.4 2-3.5-1.1-1.6-2.8-1.8-3.4-1.9-1.4-.2-2.8.9-3.5.9s-1.8-.8-3-.8c-1.5 0-2.9.9-3.7 2.3-1.6 2.7-.4 6.8 1.1 9 .8 1.1 1.7 2.3 2.9 2.3 1.2 0 1.6-.7 3-.7s1.8.7 3 .7 2-1.1 2.8-2.1c.9-1.2 1.2-2.4 1.2-2.5 0 0-2.4-.9-2.4-3.7z"/><path d="M14 5.4c.6-.8 1-1.8.9-2.9-.9 0-2 .6-2.6 1.4-.6.7-1.1 1.8-.9 2.8 1 .1 2-.5 2.6-1.3z"/>"#
        ),
        "linux" => icon!(
            r#"<path d="M9 3.8c0-1 .9-1.8 2-1.8h2c1.1 0 2 .8 2 1.8v3.4c0 1 .5 1.8 1.2 2.6 1.4 1.6 2.3 3.4 2.3 5.2 0 .9-.3 1.6-.8 2.1.4.8.8 1.6.6 2.2-.3 1-1.6 1.2-3.2 1.2h-6.2c-1.6 0-2.9-.2-3.2-1.2-.2-.6.2-1.4.6-2.2-.5-.5-.8-1.2-.8-2.1 0-1.8.9-3.6 2.3-5.2C8.5 9 9 8.2 9 7.2z"/><path d="M10.5 6.2h.01M13.5 6.2h.01"/>"#
        ),
        _ => icon!(r#"<circle cx="12" cy="12" r="8"/>"#),
    }
}
