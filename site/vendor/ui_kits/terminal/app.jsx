// Tasty UI kit — interactive app shell.
const { TitleBar, Sidebar, TabStrip, TerminalPane, MarkdownSurface, StatusBar,
        Prompt, S, CommandPalette, SettingsWindow, PortsWindow, PluginsWindow, RemoteTool, ToolsMenu } = window.TastyKit;

const line = {
  green: S("var(--tasty-color-green)"), blue: S("var(--tasty-color-blue)"), mauve: S("var(--tasty-color-mauve)"),
  yellow: S("var(--tasty-color-yellow)"), peach: S("var(--tasty-color-peach)"), dim: S("var(--tasty-color-neutral-700)"),
  red: S("var(--tasty-color-red)"), teal: S("var(--tasty-color-teal)"),
};

const agentSession = {
  id: "s_01HX", label: "build · cargo", owner: "agent", activity: "idle",
  lines: [
    <><span style={{ color: "var(--tasty-color-mauve)" }}>❯</span> cargo build --release</>,
    <span style={{ color: "var(--tasty-color-neutral-700)" }}>   Compiling tasty-themes v0.7.0</span>,
    <span style={{ color: "var(--tasty-color-neutral-700)" }}>   Compiling tasty-type-appearance v0.7.0</span>,
    <span style={{ color: "var(--tasty-color-neutral-700)" }}>   Compiling tasty-agent v0.7.0</span>,
    <><span style={{ color: "var(--tasty-color-green)" }}>    Finished</span> <span style={{ color: "var(--tasty-color-yellow)" }}>release</span> [optimized] in 42.18s</>,
    <><span style={{ color: "var(--tasty-color-blue)" }}>agent.task</span> <span style={{ color: "var(--tasty-color-green)" }}>done</span> id=t_build exit=0</>,
  ],
};

const userSession = {
  id: "s_02JK", label: "zsh · ~/tasty", owner: "user", activity: "running",
  // plain-text mirror of `lines` — what the search bar (Ctrl+F) matches against
  plainLines: [
    "~/tasty main via \u{1F980} v1.84",
    "❯ tasty surface list --json | jq '.[].kind'",
    '"terminal"',
    '"markdown"',
    '"terminal"',
    "~/tasty main via \u{1F980} v1.84",
    "❯ tasty agent wait --id t_build",
    "blocking until child claude is idle…",
  ],
  lines: [
    <Prompt branch="main" />,
    <><span style={{ color: "var(--tasty-color-mauve)" }}>❯</span> tasty surface list --json | jq '.[].kind'</>,
    <span style={{ color: "var(--tasty-color-green)" }}>"terminal"</span>,
    <span style={{ color: "var(--tasty-color-green)" }}>"markdown"</span>,
    <span style={{ color: "var(--tasty-color-green)" }}>"terminal"</span>,
    <Prompt branch="main" />,
    <><span style={{ color: "var(--tasty-color-mauve)" }}>❯</span> tasty agent wait --id t_build</>,
    <span style={{ color: "var(--tasty-color-neutral-700)" }}>blocking until child claude is idle…</span>,
  ],
};

function App() {
  const [theme, setTheme] = React.useState("mocha");
  const [overlay, setOverlay] = React.useState(null);
  const [toolsAnchor, setToolsAnchor] = React.useState(null); // DOMRect of the sidebar Tools button
  const [searchOpen, setSearchOpen] = React.useState(false);  // Ctrl+F search bar on the focused terminal
  const [activeWs, setActiveWs] = React.useState("prod");
  const [activeTab, setActiveTab] = React.useState("t1");
  const [toast, setToast] = React.useState(null);
  const [sidebarCollapsed, setSidebarCollapsed] = React.useState(false);
  const [uiScale, setUiScale] = React.useState("md"); // sm | md | lg — chrome zoom
  React.useEffect(() => {
    document.documentElement.style.setProperty("--tasty-ui-scale", `var(--tasty-ui-scale-${uiScale})`);
  }, [uiScale]);
  // Tab dots encode LIVE ACTIVITY (owner×activity → resolveStatus), not "unsaved":
  // t1 is an agent build that's running (green); t2 has a pending notification
  // (yellow label); t3 is idle (no dot). Tasty has no dirty/unsaved concept.
  const [tabs, setTabs] = React.useState([
    { id: "t1", title: "build · cargo", kind: "terminal", owner: "agent", activity: "running" },
    { id: "t2", title: "README.md", kind: "markdown", owner: "user", activity: "idle", notif: true },
    { id: "t3", title: "scratch", kind: "terminal", owner: "user", activity: "idle" },
  ]);
  const seq = React.useRef(4);

  React.useEffect(() => { document.documentElement.dataset.theme = theme === "latte" ? "latte" : ""; }, [theme]);
  React.useEffect(() => {
    const h = (e) => {
      if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "k") { e.preventDefault(); setOverlay("palette"); setToolsAnchor(null); }
      if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "f") { e.preventDefault(); setSearchOpen(true); }
      if (e.key === "Escape") { setOverlay(null); setToolsAnchor(null); setSearchOpen(false); }
    };
    window.addEventListener("keydown", h);
    return () => window.removeEventListener("keydown", h);
  }, []);

  // Workspace = title (name) + optional subtitle + optional description.
  // All four field combinations shown here on purpose (see chrome.jsx WorkspaceRow).
  const workspaces = [
    // title only
    { id: "prod", name: "agents-prod", status: "running", notif: 0 },
    // title + subtitle — MIRROR of a remote instance (sky glyph + subtitle shows origin), idle dot
    { id: "infra", name: "infra", subtitle: "mirror → prod-web", status: "idle", notif: 0,
      mirror: true, mirrorTarget: "prod-web" },
    // title + description
    { id: "api", name: "api-gateway", description: "deploy to staging on every push to main",
      status: "agent", notif: 2, attached: true },
    // title + subtitle + description — MIRROR + running dot + notif (all three axes at once)
    { id: "data", name: "data-pipeline", subtitle: "mirror → gb10",
      description: "nightly ETL across the warehouse, last run 18m ago", status: "running", notif: 1,
      mirror: true, mirrorTarget: "gb10" },
    // title only
    { id: "scratch", name: "scratch", status: "idle", notif: 0 },
  ];

  // Workspace categories (sidebar folders) — the reserved `normal` category is
  // always first (label = "Workspaces"), then user categories in list order.
  // `Archived` is intentionally empty + collapsed to show those states. In the
  // product this whole grouping is gated on Settings › General › Workspace
  // categories (folders) (default off → flat single "Workspaces" list).
  const wsById = (id) => workspaces.find((w) => w.id === id);
  const categories = [
    { id: "normal", reserved: true, collapsed: false, workspaces: [wsById("prod"), wsById("scratch")] },
    { id: "services", name: "Services", collapsed: false, workspaces: [wsById("infra"), wsById("api"), wsById("data")] },
    { id: "archived", name: "Archived", collapsed: true, workspaces: [] },
  ];

  const flash = (msg) => { setToast(msg); clearTimeout(flash._t); flash._t = setTimeout(() => setToast(null), 1800); };

  const newTab = () => {
    const id = "t" + seq.current++;
    setTabs((t) => [...t, { id, title: "terminal " + (seq.current - 1), kind: "terminal", owner: "user", activity: "idle" }]);
    setActiveTab(id);
  };
  const closeTab = (id) => {
    setTabs((t) => {
      const next = t.filter((x) => x.id !== id);
      if (id === activeTab && next.length) setActiveTab(next[0].id);
      return next;
    });
  };
  const runCmd = (label) => {
    setOverlay(null);
    if (label.startsWith("Toggle Theme")) setTheme((t) => (t === "latte" ? "mocha" : "latte"));
    else if (label === "Settings") setOverlay("settings");
    else if (label.startsWith("Listening ports")) setOverlay("ports");
    else if (label === "New Terminal") newTab();
    else flash(label);
  };

  // Tools menu dispatch — mirrors tools_menu.rs: built-ins open their popup
  // (CenteredFocused); Presets opens a separate window; plugin tools fire events.
  const runTool = (id) => {
    setToolsAnchor(null);
    if (id === "palette") setOverlay("palette");
    else if (id === "ports") setOverlay("ports");
    else if (id === "remote") setOverlay("remote");
    else if (id === "presets") flash("Layout Presets");
    else if (id === "clipboard") flash("Clipboard History");
    else if (id === "git") flash("git-helper: open panel");
  };

  const tab = tabs.find((t) => t.id === activeTab);

  // Search is scoped to the focused surface — close it when the tab changes.
  React.useEffect(() => { setSearchOpen(false); }, [activeTab]);

  return (
    <div style={{ position: "absolute", inset: 0, display: "flex", flexDirection: "column",
      background: "var(--tasty-bg-app)", overflow: "hidden" }}>
      <TitleBar workspace={workspaces.find((w) => w.id === activeWs)?.name || "tasty"} />
      <div style={{ flex: 1, display: "flex", minHeight: 0 }}>
        <Sidebar workspaces={workspaces} activeWs={activeWs} onWs={setActiveWs}
          categories={categories} workspacesHeading="Workspaces"
          collapsed={sidebarCollapsed} onToggle={() => setSidebarCollapsed((v) => !v)}
          pluginAlert={(window.TastyKit && window.TastyKit.pluginAttentionCount) || 0}
          onSettings={() => setOverlay("settings")} onPlugins={() => setOverlay("plugins")}
          onTools={(rect) => setToolsAnchor((a) => (a ? null : rect))} />
        <div style={{ flex: 1, display: "flex", flexDirection: "column", minWidth: 0 }}>
          <TabStrip tabs={tabs} active={activeTab} onSelect={setActiveTab} onClose={closeTab} onNew={newTab}
            onSearch={() => setSearchOpen((v) => !v)} />
          <div style={{ flex: 1, display: "flex", gap: "var(--tasty-space-xs)", padding: "var(--tasty-space-xs)", minHeight: 0,
            background: "var(--tasty-bg-panel)" }}>
            {tab && tab.kind === "markdown" ? (
              <MarkdownSurface />
            ) : (
              <>
                <TerminalPane session={agentSession} focused={false} />
                <TerminalPane session={userSession} focused
                  searchOpen={searchOpen} onSearchClose={() => setSearchOpen(false)} />              </>
            )}
          </div>
          <StatusBar surfaceId={tab && tab.kind === "markdown" ? "s_03MD" : "s_02JK"} theme={theme}
            onTheme={() => setTheme((t) => (t === "latte" ? "mocha" : "latte"))} onPalette={() => setOverlay("palette")} />
        </div>
      </div>

      {toolsAnchor && <ToolsMenu anchor={toolsAnchor} onClose={() => setToolsAnchor(null)} onAction={runTool} />}
      {overlay === "palette" && <CommandPalette onClose={() => setOverlay(null)} onRun={runCmd} />}
      {overlay === "settings" && <SettingsWindow theme={theme} onTheme={setTheme}
        uiScale={uiScale} onUiScale={setUiScale} onClose={() => setOverlay(null)} />}
      {overlay === "ports" && <PortsWindow onClose={() => setOverlay(null)} onFlash={flash} />}
      {overlay === "remote" && <RemoteTool onClose={() => setOverlay(null)} onFlash={flash} />}
      {overlay === "plugins" && <PluginsWindow onClose={() => setOverlay(null)} onFlash={flash}
        onConfigure={() => setOverlay("settings")} />}

      {toast && (
        <div style={{ position: "absolute", bottom: "var(--tasty-size-36)", left: "50%", transform: "translateX(-50%)", zIndex: 60 }}>
          {React.createElement(window.TastyDesignSystem_41fd3f.Toast, { variant: "info" }, toast)}
        </div>
      )}
    </div>
  );
}

ReactDOM.createRoot(document.getElementById("root")).render(<App />);
