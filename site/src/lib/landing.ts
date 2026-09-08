/**
 * Landing-page copy, one set per site language. Chrome strings live in `strings.ts`.
 *
 * The words are the site's, not this file's: they were written for the page and
 * carried across when the generator changed. Only the shape moved — a Rust
 * struct became this, and the `.html` targets became routes.
 */
export interface Point {
  key: string;
  title: string;
  body: string;
}
export interface Card {
  /** A name from the design system's icon registry (`src/ds/core/Icon.jsx`). */
  icon: string;
  title: string;
  body: string;
  /** Guide page this card leads to, relative to the guide root. */
  href: string;
}
export interface Copy {
  badge: string;
  /** The wordmark line: the pun only lands in English. */
  tagline: string;
  titleLead: string;
  titleAccent: string;
  titleTail: string;
  ctaPrimary: string;
  ctaSecondary: string;
  ctaOther: string;
  /** Primary button label once the visitor's OS is known; `{os}` is replaced. */
  dlFor: string;
  installNote: string;
  installNoteLink: string;

  whyTitle: string;
  whyBody: string;
  whyPoints: Point[];

  featuresKicker: string;
  featuresTitle: string;
  featuresBody: string;
  cards: Card[];

  agentsKicker: string;
  agentsTitle: string;
  agentsBody: string;
  agentsCaption: string;
  agentsTab: string;

  platformKicker: string;
  platformTitle: string;
  platformBody: string;
  stats: [string, string][];

  ctaTitle: string;
  ctaBody: string;
  ctaDocs: string;
  ctaDownload: string;

  /** Strings rendered inside the app mockup itself. */
  shellHeading: string;
  shellTask: string;
}

export const KO: Copy = {
  badge: "Windows · macOS · Linux",
  tagline: "맛있는 터미널.",
  titleLead: "AI 에이전트와",
  titleAccent: "함께 조작하는",
  titleTail: "터미널",
  ctaPrimary: "다운로드",
  ctaSecondary: "가이드 보기",
  ctaOther: "다른 플랫폼",
  dlFor: "{os} 용 다운로드",
  installNote: "OS 별 설치 절차와 첫 실행은",
  installNoteLink: "설치 가이드 →",

  whyTitle: "에이전트가 일해도 내 자리는 그대로입니다",
  whyBody:
    "에이전트가 탭을 만들든 명령을 보내든, 내가 보던 화면은 움직이지 않습니다. 잡아둔 선택도, 스크롤 위치도 그대로입니다. 사용자 입력을 흉내 내는 기능은 아예 없습니다.",
  whyPoints: [
    { key: "01", title: "내 조작과 분리", body: "에이전트가 무슨 일을 하든 포커스와 선택, 스크롤, 닫은 탭 기록에는 손대지 않습니다. 내가 다른 탭을 보고 있어도 자기 터미널 안에서만 움직입니다." },
    { key: "02", title: "ID 로 지정", body: "에이전트는 조작할 터미널을 ID 로 찍어서 부릅니다. 지금 무엇이 활성이냐에 따라 엉뚱한 곳에 입력이 들어가는 일이 없습니다." },
    { key: "03", title: "명령 하나로 전부", body: "분할부터 훅까지 전부 tasty 명령 하나로 합니다. 에이전트에게 알려줄 것은 명령어 목록뿐입니다." },
    { key: "04", title: "창 없이도", body: "서버나 CI 에서 창 없이 띄워도 같은 명령이 그대로 돕니다." },
  ],

  featuresKicker: "기능",
  featuresTitle: "터미널이 해야 할 일과, 에이전트가 필요로 하는 일",
  featuresBody: "자주 손이 가는 것만 골랐습니다. 나머지는 가이드가 순서대로 다룹니다.",
  cards: [
    { icon: "layoutGrid", title: "GPU 렌더링", body: "셀 하나하나를 GPU 가 그립니다. 분할을 열 개 넘게 띄워도 버벅이지 않습니다.", href: "using/panes-tabs-splits/" },
    { icon: "layers", title: "워크스페이스와 프리셋", body: "일감마다 워크스페이스를 따로 둡니다. 자주 쓰는 배치는 프리셋으로 저장해 두고 꺼내 씁니다.", href: "using/workspaces/" },
    { icon: "folderOpen", title: "터미널만 있는 게 아닙니다", body: "탐색기와 마크다운, 이미지, 웹 화면을 터미널 옆에 나란히 띄웁니다.", href: "using/files/" },
    { icon: "rocket", title: "여러 에이전트를 한 번에", body: "Claude 와 Codex 자식을 띄워두면 끝나는 대로 알려줍니다.", href: "agents/claude-codex/" },
    { icon: "gitTree", title: "작업 DAG", body: "할 일을 의존 관계로 묶어 두면 순서대로 돕니다. 어디까지 갔는지는 그래프로 봅니다.", href: "agents/tasks/" },
    { icon: "terminal", title: "CLI 로 조작", body: "분할도 명령 전송도 출력 읽기도 tasty 명령 하나입니다. 에이전트가 자기 터미널을 직접 다룹니다.", href: "agents/cli/" },
    { icon: "textLeft", title: "명령 단위 출력", body: "셸 프롬프트 경계를 알아채서, 방금 돌린 명령의 출력만 딱 읽습니다.", href: "using/terminal/" },
    { icon: "bell", title: "훅과 알림", body: "프로세스 종료나 특정 출력, 유휴 시간에 훅을 걸어두고 알림을 받습니다.", href: "agents/hooks-notifications/" },
    { icon: "scriptFile", title: "Lua 스크립트", body: "단축키나 창 · 탭 이벤트에 스크립트를 걸어두면 손 갈 일이 줄어듭니다.", href: "customize/scripts/" },
    { icon: "remote", title: "원격 attach", body: "다른 머신에서 돌고 있는 워크스페이스를 SSH 로 그대로 가져와 봅니다.", href: "remote/attach/" },
    { icon: "plug", title: "플러그인", body: "탐색기와 마크다운, 이미지, git 보기가 기본으로 들어 있습니다. 권한을 확인하고 켜거나 끕니다.", href: "plugins/" },
    { icon: "theme", title: "테마", body: "Mocha 나 Latte 를 고르거나, TOML 로 직접 만듭니다.", href: "customize/themes/" },
  ],

  agentsKicker: "다중 에이전트",
  agentsTitle: "여러 에이전트를 띄워두고, 끝나는 대로 확인합니다",
  agentsBody:
    "자식을 띄우는 명령은 기다리지 않고 바로 끝납니다. 완료 훅은 알아서 걸립니다. 자식이 멈추거나 입력을 기다리면 띄운 쪽 터미널이 먼저 압니다.",
  agentsCaption: "사람이 GUI 에서 하는 일은 에이전트도 CLI 로 똑같이 합니다.",
  agentsTab: "오케스트레이터",

  platformKicker: "플랫폼",
  platformTitle: "세 OS 모두 1 급",
  platformBody: "세 OS 에서 기능도 단축키도 CLI 도 똑같습니다. 설치 파일은 dmg, msi, deb, rpm, AppImage 로 냅니다.",
  stats: [["3", "운영체제"], ["7", "번들 플러그인"], ["3", "UI 언어"], ["MIT", "라이선스"]],

  ctaTitle: "가이드부터 읽어도 되고, 바로 설치해도 됩니다",
  ctaBody: "설치부터 에이전트 연동, 원격 attach 까지 가이드가 순서대로 짚어 줍니다.",
  ctaDocs: "가이드 읽기",
  ctaDownload: "다운로드",

  shellHeading: "워크스페이스",
  shellTask: "release 0.7.1 준비",
};

export const EN: Copy = {
  badge: "Windows · macOS · Linux",
  tagline: "Tasty terminal.",
  titleLead: "A terminal you and",
  titleAccent: "your AI agent",
  titleTail: "drive together",
  ctaPrimary: "Download",
  ctaSecondary: "Read the guide",
  ctaOther: "Other platforms",
  dlFor: "Download for {os}",
  installNote: "Per-OS install steps and the first launch:",
  installNoteLink: "Installation guide →",

  whyTitle: "The agent works, and your seat stays yours",
  whyBody:
    "An agent can open tabs and send commands, and the screen you were looking at, the text you selected, and your scroll position do not move. Nothing in the product imitates user input.",
  whyPoints: [
    { key: "01", title: "Separate from your hands", body: "What an agent does never touches focus, selection, scrolling, or the closed-tab history. You can look at another tab while it works in its own." },
    { key: "02", title: "Addressed by ID", body: "An agent names the terminal it wants to drive by ID. Whatever happens to be active right now never receives input meant for somewhere else." },
    { key: "03", title: "One command for everything", body: "Splits, sending commands, reading output, notifications, hooks — all through the single tasty command. The only thing an agent needs to learn is the command list." },
    { key: "04", title: "Windowless too", body: "On a server or in CI, run it with no window and drive terminals with the same commands." },
  ],

  featuresKicker: "Features",
  featuresTitle: "What a terminal owes you, and what an agent needs from one",
  featuresBody: "The ones people reach for most. The guide takes the rest in order.",
  cards: [
    { icon: "layoutGrid", title: "GPU rendering", body: "Every cell is drawn on the GPU. Stays smooth well past ten splits.", href: "using/panes-tabs-splits/" },
    { icon: "layers", title: "Workspaces and presets", body: "One workspace per job, and a layout you keep coming back to saved as a preset.", href: "using/workspaces/" },
    { icon: "folderOpen", title: "More than terminals", body: "A file explorer, Markdown, images and web pages sit in the same splits as your shells.", href: "using/files/" },
    { icon: "rocket", title: "Several agents at once", body: "Spawn Claude and Codex children and hear back as each one finishes.", href: "agents/claude-codex/" },
    { icon: "gitTree", title: "Task DAG", body: "Tie work together by dependency and it runs in order. Watch how far it got as a graph.", href: "agents/tasks/" },
    { icon: "terminal", title: "Driven from the CLI", body: "Splits, sending commands, reading output, notifications — one tasty command. An agent drives its own terminal directly.", href: "agents/cli/" },
    { icon: "textLeft", title: "Per-command output", body: "Recognises shell prompt boundaries, so an agent reads exactly the output of the command it just ran.", href: "using/terminal/" },
    { icon: "bell", title: "Hooks and notifications", body: "Hook process exit, output patterns, and idle time, and get notified.", href: "agents/hooks-notifications/" },
    { icon: "scriptFile", title: "Lua scripts", body: "Bind a script to a shortcut or to window and tab events and let it do the repetitive part.", href: "customize/scripts/" },
    { icon: "remote", title: "Remote attach", body: "Bring a workspace from a tasty running on another machine over SSH and see it as is.", href: "remote/attach/" },
    { icon: "plug", title: "Plugins", body: "Explorer, Markdown, image, and git views come bundled. See each one's permissions and switch it on or off.", href: "plugins/" },
    { icon: "theme", title: "Themes", body: "Pick a bundled theme such as Mocha or Latte, or write your own in TOML.", href: "customize/themes/" },
  ],

  agentsKicker: "Multi-agent",
  agentsTitle: "Spawn several agents, hear back as each one lands",
  agentsBody:
    "The command that spawns a child does not wait around — it returns at once, and the completion hook arms itself. When a child stalls or wants input, the terminal that spawned it hears about it first.",
  agentsCaption: "Whatever a person does in the GUI, an agent can do from the CLI.",
  agentsTab: "orchestrator",

  platformKicker: "Platforms",
  platformTitle: "All three, first class",
  platformBody: "Nothing differs across the three: not the features, not the shortcuts, not the CLI. Ships as dmg, msi, deb, rpm, or AppImage.",
  stats: [["3", "operating systems"], ["7", "bundled plugins"], ["3", "UI languages"], ["MIT", "license"]],

  ctaTitle: "Start with the guide, or just install it",
  ctaBody: "From installing it to wiring up an agent to attaching a remote, the guide walks it in order.",
  ctaDocs: "Read the guide",
  ctaDownload: "Download",

  shellHeading: "Workspaces",
  shellTask: "cut release 0.7.1",
};

export const copyFor = (lang: "ko" | "en"): Copy => (lang === "ko" ? KO : EN);
