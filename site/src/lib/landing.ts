/**
 * Landing-page copy, one set per site language. Chrome strings live in `strings.ts`.
 *
 * Introduce the shared workflow first, then explain each feature in everyday terms.
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
  titleAccent: "함께 일하는",
  titleTail: "터미널",
  ctaPrimary: "다운로드",
  ctaSecondary: "가이드 보기",
  ctaOther: "다른 플랫폼",
  dlFor: "{os}용 다운로드",
  installNote: "설치가 처음이라면",
  installNoteLink: "설치 가이드를 참고하세요 →",

  whyTitle: "에이전트에게 맡기고, 내 작업도 이어가세요",
  whyBody:
    "Tasty에서는 AI 에이전트가 터미널을 열고, 명령을 실행하고, 결과까지 직접 확인할 수 있습니다. 에이전트에게 테스트를 맡기고, 다른 탭에서 코드를 살펴보세요.",
  whyPoints: [
    { key: "01", title: "내 작업 흐름은 그대로", body: "에이전트가 탭을 열고 명령을 실행해도 내가 보던 탭의 포커스, 선택한 텍스트, 스크롤 위치를 바꾸지 않습니다." },
    { key: "02", title: "필요한 터미널을 정확히", body: "각 터미널에는 고유한 ID가 있습니다. 에이전트는 현재 열린 탭에 의존하지 않고 작업할 터미널을 직접 지정합니다." },
    { key: "03", title: "터미널 조작도 명령으로", body: "화면 분할, 명령 실행, 출력 확인을 tasty CLI로 할 수 있습니다. 에이전트가 작업에 필요한 터미널 환경을 직접 구성합니다." },
    { key: "04", title: "서버에서도 같은 방식으로", body: "창을 띄우지 않는 헤드리스 모드를 지원합니다. 서버나 CI에서도 CLI로 터미널을 만들고 작업을 실행할 수 있습니다." },
  ],

  featuresKicker: "기능",
  featuresTitle: "매일 하는 개발 작업을 한곳에서",
  featuresBody: "프로젝트별로 터미널을 정리하고, 코드 옆에서 문서를 읽고, 반복하는 일은 자동화하세요. 에이전트와 함께 쓸 때도 필요한 도구를 가까이 둘 수 있습니다.",
  cards: [
    { icon: "layoutGrid", title: "GPU로 그리는 터미널", body: "GPU 가속으로 터미널 화면을 그립니다. 여러 터미널을 나란히 열고 작업할 수 있습니다.", href: "using/panes-tabs-splits/" },
    { icon: "layers", title: "프로젝트별 작업 공간", body: "프로젝트마다 워크스페이스를 나누세요. 자주 쓰는 화면 배치는 프리셋으로 저장해 다시 불러올 수 있습니다.", href: "using/workspaces/" },
    { icon: "folderOpen", title: "코드 옆에 문서와 이미지", body: "파일 탐색기, 마크다운 문서, 이미지, 웹 페이지를 터미널 옆에 열어두고 함께 확인하세요.", href: "using/files/" },
    { icon: "rocket", title: "Claude와 Codex 연동", body: "여러 에이전트에게 작업을 나눠 맡기세요. 연동을 설정하면 작업 완료와 입력 요청을 알림으로 확인할 수 있습니다.", href: "agents/claude-codex/" },
    { icon: "gitTree", title: "작업 순서 관리", body: "먼저 끝나야 하는 작업을 연결해 두면 그 순서에 맞춰 실행합니다. 진행 상황은 그래프로 확인하세요.", href: "agents/tasks/" },
    { icon: "terminal", title: "에이전트가 직접 다루는 터미널", body: "에이전트가 CLI로 터미널을 나누고, 명령을 실행하고, 출력을 읽습니다. 작업할 때마다 직접 탭을 준비해 줄 필요가 없습니다.", href: "agents/cli/" },
    { icon: "textLeft", title: "필요한 명령의 출력만", body: "셸 연동으로 명령별 출력 범위를 구분합니다. 긴 로그 전체를 뒤지지 않고 필요한 명령의 결과를 확인하세요.", href: "using/terminal/" },
    { icon: "bell", title: "필요한 순간에 알림", body: "명령이 끝나거나 특정 메시지가 나오면 알림을 받도록 설정하세요. 출력을 계속 지켜보지 않아도 됩니다.", href: "agents/hooks-notifications/" },
    { icon: "scriptFile", title: "반복 작업 자동화", body: "Lua 스크립트를 단축키나 창·탭 이벤트에 연결하세요. 자주 반복하는 동작을 내 작업 방식에 맞게 자동화할 수 있습니다.", href: "customize/scripts/" },
    { icon: "remote", title: "원격 작업도 내 화면에서", body: "SSH로 다른 컴퓨터의 Tasty에 연결하세요. 그곳에서 실행 중인 워크스페이스를 내 컴퓨터에서 보고 조작할 수 있습니다.", href: "remote/attach/" },
    { icon: "plug", title: "필요한 도구를 플러그인으로", body: "마크다운, 이미지, Git 보기 등을 플러그인으로 제공합니다. 필요한 권한을 확인하고 사용할 도구를 선택하세요.", href: "plugins/" },
    { icon: "theme", title: "내 취향에 맞는 테마", body: "Mocha와 Latte 테마를 골라 쓰거나, TOML 파일로 나만의 테마를 만들어 보세요.", href: "customize/themes/" },
  ],

  agentsKicker: "여러 에이전트와 함께",
  agentsTitle: "구현과 테스트, 나눠서 진행하세요",
  agentsBody:
    "Claude Code와 Codex CLI를 연결해 여러 에이전트에게 일을 나눠 맡길 수 있습니다. 연동을 설정하면 작업을 마치거나 답변이 필요할 때 알려주니, 탭을 하나씩 열어 확인할 필요가 줄어듭니다.",
  agentsCaption: "에이전트도 다른 에이전트에게 작업을 맡기고 완료 알림을 받을 수 있습니다.",
  agentsTab: "작업 관리",

  platformKicker: "플랫폼",
  platformTitle: "익숙한 OS에서 그대로 시작하세요",
  platformBody: "Windows, macOS, Linux를 지원합니다. 내 컴퓨터에 맞는 버전을 설치하고, 쓰던 셸과 개발 도구로 작업하세요.",
  stats: [["3", "운영체제"], ["GPU", "화면 렌더링"], ["3", "UI 언어"], ["MIT", "라이선스"]],

  ctaTitle: "다음 프로젝트는 Tasty에서 시작해 보세요",
  ctaBody: "터미널을 열고, AI 에이전트를 연결해 보세요. 설치부터 첫 작업까지 가이드에서 안내합니다.",
  ctaDocs: "가이드 읽기",
  ctaDownload: "다운로드",

  shellHeading: "워크스페이스",
  shellTask: "다음 릴리스 준비",
};

export const EN: Copy = {
  badge: "Windows · macOS · Linux",
  tagline: "Tasty terminal.",
  titleLead: "A terminal for",
  titleAccent: "you and your AI agents",
  titleTail: "to work together",
  ctaPrimary: "Download",
  ctaSecondary: "Read the guide",
  ctaOther: "Other platforms",
  dlFor: "Download for {os}",
  installNote: "Setting up for the first time?",
  installNoteLink: "Follow the installation guide →",

  whyTitle: "Give your agent a task. Keep working on yours.",
  whyBody:
    "In Tasty, an AI agent can open terminals, run commands, and read the results itself. Let it run the tests while you review code in another tab.",
  whyPoints: [
    { key: "01", title: "Stay with your own work", body: "An agent can open tabs and run commands without changing your focus, selected text, or scroll position." },
    { key: "02", title: "Choose the right terminal", body: "Each terminal has its own ID. Agents address the terminal they need directly, regardless of which tab you have open." },
    { key: "03", title: "Control terminals from the CLI", body: "The tasty CLI lets agents create splits, run commands, and read output. They can set up the terminals they need for a task." },
    { key: "04", title: "Run on servers, too", body: "Headless mode runs without a window. Use the same CLI to create terminals and run tasks on a server or in CI." },
  ],

  featuresKicker: "Features",
  featuresTitle: "Your everyday development tools, together",
  featuresBody: "Organize terminals by project, read docs beside your code, and automate routine tasks. Keep the tools you and your agents need in the same workspace.",
  cards: [
    { icon: "layoutGrid", title: "GPU rendering", body: "Tasty uses the GPU to draw your terminal. Open several terminals side by side to follow different parts of your work.", href: "using/panes-tabs-splits/" },
    { icon: "layers", title: "A workspace for each project", body: "Keep projects in separate workspaces. Save a layout as a preset and bring it back when you need it.", href: "using/workspaces/" },
    { icon: "folderOpen", title: "Docs and images beside your code", body: "Open a file explorer, Markdown documents, images, and web pages alongside your terminals.", href: "using/files/" },
    { icon: "rocket", title: "Connect Claude and Codex", body: "Give different tasks to several agents. Once configured, the integration notifies you when work finishes or an agent needs input.", href: "agents/claude-codex/" },
    { icon: "gitTree", title: "Put tasks in order", body: "Connect tasks to the work they depend on, and Tasty runs them in that order. Follow progress in a graph.", href: "agents/tasks/" },
    { icon: "terminal", title: "Terminals agents can operate", body: "Agents can create splits, run commands, and read output through the CLI, without you setting up each tab for them.", href: "agents/cli/" },
    { icon: "textLeft", title: "Read the output you need", body: "Shell integration identifies the output of each command. Check a result without searching through the entire terminal history.", href: "using/terminal/" },
    { icon: "bell", title: "Know when to check back", body: "Set notifications for a command finishing or a message appearing in its output. You can move on instead of watching the terminal.", href: "agents/hooks-notifications/" },
    { icon: "scriptFile", title: "Automate routine work", body: "Connect Lua scripts to shortcuts or window and tab events. Automate repeated actions to suit the way you work.", href: "customize/scripts/" },
    { icon: "remote", title: "Bring remote work to your screen", body: "Connect over SSH to Tasty on another computer. View and operate the workspace running there from your own machine.", href: "remote/attach/" },
    { icon: "plug", title: "Add tools with plugins", body: "Use plugins for Markdown, images, Git views, and more. Check their permissions and choose the tools you want to enable.", href: "plugins/" },
    { icon: "theme", title: "Make it feel like yours", body: "Choose Mocha or Latte, or write your own theme in a TOML file.", href: "customize/themes/" },
  ],

  agentsKicker: "Working with multiple agents",
  agentsTitle: "Split the work between your agents",
  agentsBody:
    "Connect Claude Code and Codex CLI to let several agents work on different tasks. Once configured, Tasty lets you know when they finish or need an answer, so you spend less time checking tabs.",
  agentsCaption: "An agent can also delegate work to other agents and receive their completion notifications.",
  agentsTab: "task coordinator",

  platformKicker: "Platforms",
  platformTitle: "Start on the OS you already use",
  platformBody: "Tasty runs on Windows, macOS, and Linux. Install the version for your computer and keep using your familiar shells and development tools.",
  stats: [["3", "operating systems"], ["GPU", "rendering"], ["3", "UI languages"], ["MIT", "license"]],

  ctaTitle: "Try your next project in Tasty",
  ctaBody: "Open a terminal and connect an AI agent. The guide walks you through installation and your first task.",
  ctaDocs: "Read the guide",
  ctaDownload: "Download",

  shellHeading: "Workspaces",
  shellTask: "prepare the next release",
};

export const copyFor = (lang: "ko" | "en"): Copy => (lang === "ko" ? KO : EN);
