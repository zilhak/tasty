//! The landing page. One layout, one copy deck per language.

use crate::md::html_escape;
use crate::shell::{self, ICON_GITHUB, REPO, Shell, Strings};

struct Copy {
    badge: &'static str,
    title_lead: &'static str,
    title_accent: &'static str,
    title_tail: &'static str,
    lede: &'static str,
    cta_primary: &'static str,
    cta_secondary: &'static str,
    install_note: &'static str,

    why_kicker: &'static str,
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
    cta_source: &'static str,

    docs_language_note: &'static str,
}

const KO_COPY: Copy = Copy {
    badge: "Windows · macOS · Linux",
    title_lead: "AI 에이전트가",
    title_accent: "직접 조작하는",
    title_tail: "터미널",
    lede: "GPU 가속 네이티브 터미널에 좌표 하나를 더했다. \
           모든 표면이 키보드·마우스와 IPC·CLI 양쪽에 똑같이 열려 있어서, \
           사람과 에이전트가 같은 터미널을 동시에 쓴다.",
    cta_primary: "설치하기",
    cta_secondary: "문서 보기",
    install_note: "소스에서 빌드하거나 릴리스에서 DMG · MSI · AppImage 를 받는다.",

    why_kicker: "설계 원칙",
    why_title: "사용자의 조작과 에이전트의 조작을 섞지 않는다",
    why_body: "에이전트가 무언가를 해도 사용자의 포커스·선택·스크롤·히스토리는 움직이지 않는다. \
               사용자 입력을 재현하는 기능은 release 빌드에 아예 존재하지 않는다.",
    why_points: &[
        (
            "01",
            "행동 분리",
            "IPC · CLI 의 부수효과가 사용자 상태에 닿지 않는다. 키 주입이나 강제 포커스 전환 같은 \
             입력 재현 API 는 debug 빌드로 격리되어 있다.",
        ),
        (
            "02",
            "포커스 독립",
            "모든 명령이 대상을 ID 로 직접 지정한다. 조회는 전 워크스페이스를 순회하고, \
             무엇이 활성 상태인지에 따라 동작이 달라지지 않는다.",
        ),
        (
            "03",
            "양면 노출",
            "에이전트가 쓸 수 있는 기능은 IPC 와 CLI 양쪽으로 모두 제공한다. \
             GUI 에서만 가능한 에이전트 기능은 두지 않는다.",
        ),
        (
            "04",
            "헤드리스",
            "창 없이도 표면을 만들고 입출력을 주고받는다. CI 나 서버에 그대로 들어간다.",
        ),
    ],

    features_kicker: "기능",
    features_title: "터미널이 해야 할 일과, 에이전트가 필요로 하는 일",
    features_body: "45 개 기능이 각각 기획 문서와 화면 문서로 정리되어 있다. \
                    아래는 그중 자주 쓰이는 것들이다.",
    cards: &[
        (
            "grid",
            "GPU 렌더링",
            "셀 단위 셰이더로 그린다. 표면을 10 개 이상 띄워도 prepare/draw 가 안정적이다.",
            "features/terminal/index.html",
        ),
        (
            "agents",
            "다중 에이전트 협업",
            "task DAG 와 barrier · semaphore · lease · reduce · rate-limit 프리미티브로 \
             병렬 작업을 조율한다.",
            "features/agent-collaboration/index.html",
        ),
        (
            "terminal",
            "헤드리스 PTY",
            "표면 없이 PTY 만 띄우고 exit code 를 받는다. 필요할 때 화면으로 승격한다.",
            "features/headless-pty/index.html",
        ),
        (
            "keyboard",
            "vi 복사 모드",
            "hjkl 이동, visual 선택, 검색까지 키보드만으로. 커서는 GPU 로 그린다.",
            "features/clipboard/index.html",
        ),
        (
            "plug",
            "플러그인 SDK",
            "매니페스트 스키마와 권한 시스템을 갖춘 SDK. 번들 플러그인 8 종이 같은 규약으로 동작한다.",
            "features/plugin-system/index.html",
        ),
        (
            "link",
            "원격 attach",
            "이미 떠 있는 원격 tasty 의 워크스페이스를 로컬로 mirror 한다. \
             신뢰 경계는 SSH 에 위임한다.",
            "features/remote-attach/index.html",
        ),
        (
            "parse",
            "출력 구조화",
            "셸 프롬프트 경계를 인식해 \"이 명령의 출력\" 만 정확히 떼어낸다.",
            "features/terminal-output/index.html",
        ),
        (
            "gauge",
            "토큰 비용 상한",
            "사용량을 집계하고 상한을 넘으면 자동으로 차단한다.",
            "features/telemetry/index.html",
        ),
        (
            "palette",
            "테마",
            "TOML 로 직접 만든다. 4px 그리드와 14px 폰트 상한 위에 올린 토큰 시스템.",
            "features/themes/index.html",
        ),
    ],

    agents_kicker: "다중 에이전트",
    agents_title: "여러 에이전트를 띄우고, 끝나는 대로 통지받는다",
    agents_body: "자식 인스턴스를 spawn 하면 명령은 즉시 반환하고 완료 훅이 자동으로 등록된다. \
                  각 자식이 idle · 입력 대기 · 종료 상태가 되면 호출한 표면으로 통지가 온다. \
                  Blackboard · Plan · Cache 로 같은 작업 맥락을 공유한다.",
    agents_caption: "CLI 로 하는 모든 일은 IPC 로도 똑같이 할 수 있다.",

    platform_kicker: "플랫폼",
    platform_title: "세 OS 모두 1 급",
    platform_body: "플랫폼 분기는 조건부 컴파일로 처리하고, 한 OS 전용 기능이 있어도 \
                    다른 OS 의 빌드를 깨뜨리지 않는다. 육각형 아키텍처로 \
                    모델 · 포트 · 어댑터 · 뷰를 분리했다.",
    stats: &[
        ("46", "워크스페이스 크레이트"),
        ("45", "문서화된 기능"),
        ("3", "빌드 프로필"),
        ("MIT", "라이선스"),
    ],

    cta_title: "문서부터 읽어도 되고, 바로 빌드해도 된다",
    cta_body: "설계 원칙 · 기능 명세 · CLI 레퍼런스 · ADR 까지 전부 공개되어 있다.",
    cta_docs: "문서 인덱스",
    cta_source: "소스 보기",

    docs_language_note: "",
};

const EN_COPY: Copy = Copy {
    badge: "Windows · macOS · Linux",
    title_lead: "A terminal",
    title_accent: "an AI agent",
    title_tail: "can drive itself",
    lede: "Tasty adds one more coordinate on top of a GPU-accelerated native terminal: \
           every surface is equally open to keyboard and mouse *and* to IPC and CLI, \
           so a person and an agent can work the same terminal at once.",
    cta_primary: "Install",
    cta_secondary: "Read the docs",
    install_note: "Build from source, or grab a DMG · MSI · AppImage from Releases.",

    why_kicker: "Design principles",
    why_title: "User actions and agent actions never bleed into each other",
    why_body: "Nothing an agent does moves your focus, selection, scroll position, or history. \
               Anything that would replay user input simply is not in the release build.",
    why_points: &[
        (
            "01",
            "Separated actions",
            "Side effects of IPC and CLI calls never touch user state. Input-replay APIs \
             — key injection, forced focus changes — exist only under debug isolation.",
        ),
        (
            "02",
            "Focus independence",
            "Every command addresses its target by ID. Listings walk all workspaces, and \
             nothing behaves differently based on what happens to be active.",
        ),
        (
            "03",
            "Both surfaces, always",
            "Anything an agent can do is exposed over IPC and over the CLI. \
             There are no GUI-only agent features.",
        ),
        (
            "04",
            "Headless",
            "Create surfaces and drive their I/O with no window at all — it drops straight \
             into CI and server environments.",
        ),
    ],

    features_kicker: "Features",
    features_title: "What a terminal owes you, and what an agent needs from one",
    features_body: "45 features, each with its own behavior and screen documentation. \
                    Here are the ones people reach for most.",
    cards: &[
        (
            "grid",
            "GPU rendering",
            "Cell-based shaders. Prepare and draw stay stable past ten concurrent surfaces.",
            "features/terminal/index.html",
        ),
        (
            "agents",
            "Multi-agent collaboration",
            "A task DAG plus barrier, semaphore, lease, reduce, and rate-limit primitives \
             coordinate parallel work.",
            "features/agent-collaboration/index.html",
        ),
        (
            "terminal",
            "Headless PTY",
            "Run a PTY with no surface and collect its exit code — promote it to a visible \
             surface when you need one.",
            "features/headless-pty/index.html",
        ),
        (
            "keyboard",
            "vi copy mode",
            "hjkl movement, visual selection, and search from the keyboard alone, with a \
             GPU-drawn cursor.",
            "features/clipboard/index.html",
        ),
        (
            "plug",
            "Plugin SDK",
            "A manifest schema and a permission system. The eight bundled plugins run on the \
             same contract yours would.",
            "features/plugin-system/index.html",
        ),
        (
            "link",
            "Remote attach",
            "Mirror a workspace from a tasty instance already running elsewhere. \
             The trust boundary is SSH.",
            "features/remote-attach/index.html",
        ),
        (
            "parse",
            "Structured output",
            "Recognises shell prompt boundaries, so you can capture exactly one command's output.",
            "features/terminal-output/index.html",
        ),
        (
            "gauge",
            "Token cost caps",
            "Aggregates agent usage and blocks automatically once a cost cap is exceeded.",
            "features/telemetry/index.html",
        ),
        (
            "palette",
            "Themes",
            "Written as TOML, on a token system built over a 4px grid and a 14px type ceiling.",
            "features/themes/index.html",
        ),
    ],

    agents_kicker: "Multi-agent",
    agents_title: "Spawn several agents, hear back as each one lands",
    agents_body: "Spawning a child returns immediately and registers its completion hook for you. \
                  When a child goes idle, needs input, or exits, the calling surface is notified. \
                  Blackboard, Plan, and Cache let them share one working context.",
    agents_caption: "Everything the CLI does is available over IPC too.",

    platform_kicker: "Platforms",
    platform_title: "All three, first class",
    platform_body: "Platform differences live behind conditional compilation, and an OS-specific \
                    feature never breaks the build on the others. A hexagonal architecture keeps \
                    model, ports, adapters, and view apart.",
    stats: &[
        ("46", "workspace crates"),
        ("45", "documented features"),
        ("3", "build profiles"),
        ("MIT", "license"),
    ],

    cta_title: "Start with the docs, or just build it",
    cta_body: "Design principles, feature specs, the CLI reference, and every ADR are public.",
    cta_docs: "Documentation index",
    cta_source: "Browse the source",

    docs_language_note: "The reference docs are being translated from Korean; untranslated pages show the original with a notice.",
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
        description: strip_emphasis(copy.lede),
        root: root.to_string(),
        body,
        active: "",
        ko_href: "ko/index.html".to_string(),
        en_href: "index.html".to_string(),
        docs_prefix: docs,
        search_index: if strings.lang == "en" {
            "assets/search-index.en.json"
        } else {
            "assets/search-index.json"
        },
    })
}

fn strip_emphasis(s: &str) -> String {
    s.replace('*', "")
}

fn hero(copy: &Copy, root: &str, docs: &str) -> String {
    format!(
        r##"<section class="hero">
  <div>
    <span class="hero__eyebrow"><span class="dot"></span>{badge} · v{version}</span>
    <h1>{lead} <span class="accent">{accent}</span> {tail}</h1>
    <p class="hero__lede">{lede}</p>
    <div class="cta-row">
      <a class="btn btn--primary" href="{root}{docs}installation.html">{primary}</a>
      <a class="btn btn--ghost" href="{root}{docs}index.html">{secondary}</a>
      <a class="btn btn--ghost" href="{repo}">{github} GitHub</a>
    </div>
    <div class="install-line">
      <span class="prompt">$</span>
      <code>cargo build --release &amp;&amp; ./target/release/tasty</code>
    </div>
    <p class="hero__lede" style="font-size:14px;margin:12px 0 0">{note}</p>
  </div>
  {mock}
</section>"##,
        badge = html_escape(copy.badge),
        version = crate::version(),
        lead = html_escape(copy.title_lead),
        accent = html_escape(copy.title_accent),
        tail = html_escape(copy.title_tail),
        lede = html_escape(&strip_emphasis(copy.lede)),
        root = root,
        docs = docs,
        primary = html_escape(copy.cta_primary),
        secondary = html_escape(copy.cta_secondary),
        repo = REPO,
        github = ICON_GITHUB,
        note = html_escape(copy.install_note),
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
        terminal = glyph(r#"<rect x="3" y="4" width="18" height="16" rx="2"/><path d="m7 9 3 3-3 3M13 15h4"/>"#),
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
      <div class="section__kicker">{kicker}</div>
      <h2>{title}</h2>
      <p>{body}</p>
    </div>
    <ul class="feature-list">{points}</ul>
  </div>
</section>"##,
        kicker = html_escape(copy.why_kicker),
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
    let note = if copy.docs_language_note.is_empty() {
        String::new()
    } else {
        format!(
            "<p style=\"font-size:13px;color:var(--overlay1);margin-top:20px\">{}</p>",
            html_escape(copy.docs_language_note)
        )
    };
    format!(
        r##"<section class="cta-band">
  <h2>{title}</h2>
  <p>{body}</p>
  <div class="cta-row">
    <a class="btn btn--primary" href="{root}{docs}index.html">{docs_label}</a>
    <a class="btn btn--ghost" href="{repo}">{source}</a>
  </div>
  {note}
</section>"##,
        title = html_escape(copy.cta_title),
        body = html_escape(copy.cta_body),
        root = root,
        docs = docs,
        docs_label = html_escape(copy.cta_docs),
        repo = REPO,
        source = html_escape(copy.cta_source),
        note = note,
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
