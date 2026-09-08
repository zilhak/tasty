// Tasty Gallery — Surfaces · Task DAG
// Specimens for the read-only Task DAG surface + workspace popup
// (design-request/dag-view.md). Parts live in ui_kits/terminal/overlays/
// dag_view.jsx + dag_surfaces.jsx; this page only catalogues them.
const { Section, Spec, Stage, Cluster, Meta, Note, Do, Dont } = window.Gallery;
const { Kbd, Tag, Icon } = window.TastyDesignSystem_41fd3f;
const { DAG_STATUS, DAG_STATUS_ORDER, DAG_KIND, DAG_REL, sTok, DagNode, RunnerBadge, ZoomCluster, Minimap,
  CycleBanner, DagEmpty, DagCanvas, DagDetail, DagSurface, DagWindow, dagRowItems, dagLayout, elbow,
  DAG_BUILD, DAG_INDEX, DAG_CYCLE, DAG_DENSE, DAG_LIST } = window.TastyDag;
const { ListCtrl } = window.TastyDesignSystem_41fd3f;

const NAV = [
  { id: "canvas", label: "Graph canvas" },
  { id: "node", label: "Node card · 8 states" },
  { id: "edges", label: "Edges · 3 relations" },
  { id: "chrome", label: "Canvas chrome" },
  { id: "runner", label: "Runner badge" },
  { id: "rows", label: "DAG list row" },
  { id: "detail", label: "Node detail" },
  { id: "states", label: "Empty · cycle" },
  { id: "surfaces", label: "Surface · popup" },
];

// A node card outside the canvas: the card is absolutely positioned inside the
// graph layer, so specimens give it a 168 × 48 relative box.
function NodeBox({ node, lod = "full", selected = false, dimmed = false, caption }) {
  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 6 }}>
      <div style={{ position: "relative", width: 168, height: 48 }}>
        <DagNode node={node} lod={lod} selected={selected} dimmed={dimmed} />
      </div>
      {caption && <span style={{ fontFamily: "var(--tasty-font-mono)", fontSize: 10, color: "var(--tasty-text-muted)" }}>{caption}</span>}
    </div>
  );
}

const mk = (over) => ({ id: "spec", name: "build:linux-x86_64", kind: "run", status: "running", dur: "12s", cmd: "cargo build", ...over });

function EdgeSpecimen({ rel }) {
  const meta = DAG_REL[rel];
  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 6, alignItems: "center" }}>
      <svg width="120" height="56" viewBox="0 0 120 56" aria-hidden="true">
        <path d={elbow({ x: 12, y: 8 }, { x: 108, y: 48 }, true, 4)} fill="none" stroke={meta.color} strokeWidth="1" strokeDasharray={meta.dash || undefined} />
        <polygon points="104,41 112,41 108,48" fill={meta.color} />
      </svg>
      <span style={{ fontFamily: "var(--tasty-font-mono)", fontSize: 11, color: meta.color }}>{rel}</span>
      <span style={{ fontSize: 11, color: "var(--tasty-text-muted)" }}>{meta.label}</span>
    </div>
  );
}

function PopupStage() {
  const [open, setOpen] = React.useState(true);
  return (
    <div style={{ position: "relative", width: "100%", height: 520, borderRadius: "var(--tasty-radius)", overflow: "hidden",
      border: "var(--tasty-border-width) solid var(--tasty-border-default)", background: "var(--tasty-bg-app)" }}>
      {!open && (
        <button type="button" onClick={() => setOpen(true)}
          style={{ position: "absolute", left: 12, top: 12, height: 28, padding: "0 12px", cursor: "pointer",
            borderRadius: "var(--tasty-radius)", border: "var(--tasty-border-width) solid var(--tasty-border-strong)",
            background: "var(--tasty-surface-raised)", color: "var(--tasty-text-secondary)", fontFamily: "var(--tasty-font-ui)", fontSize: 13 }}>
          Reopen popup
        </button>
      )}
      {open && <DagWindow onClose={() => setOpen(false)} />}
    </div>
  );
}

function Page() {
  const [selected, setSelected] = React.useState("build_linux");
  return (
    <>
      <Section id="canvas" title="Graph canvas">
        <Spec title="Task DAG canvas — read-only observation"
          when={<>The one canvas both surfaces embed. Nodes are laid out <b>top-down by dependency layer</b>, edges are <b>orthogonal</b> with 4px elbows. This is <b>not a node editor</b>: there are no ports, no sockets, no drag-to-move — the only gestures are <b>pan</b> (drag empty canvas or middle-drag), <b>zoom</b> (<Kbd keys="Ctrl" /> + wheel; bare wheel pans, because a terminal app reads bare wheel as scroll), <b>select</b> (click a node), and <Kbd keys="Esc" /> to clear.</>}>
          <Stage style={{ padding: 0, height: 460, display: "block" }}>
            <div style={{ height: 460, display: "flex" }}>
              <DagCanvas dag={DAG_BUILD} dir="td" selected={selected} onSelect={setSelected} />
            </div>
          </Stage>
          <Meta
            specs={[["node box", "168 × 48 (fixed at every zoom)"], ["layer gap", "32 along flow · 24 across"], ["zoom", "20% – 150%, 10% steps"], ["fit", "auto on open, capped at 100%"], ["background", "dot grid · 16px"], ["poll", "0.5s — colour only, never re-layout"]]}
            tokens={[{ tok: "--tasty-dag-canvas-bg", use: "canvas bed", color: "var(--tasty-dag-canvas-bg)" }, { tok: "--tasty-dag-canvas-dot", use: "grid mark", color: "var(--tasty-dag-canvas-dot)" }, { tok: "--tasty-dag-layer-gap", use: "32 flow depth" }, { tok: "--tasty-dag-sibling-gap", use: "24 across" }, { tok: "--tasty-dag-edge-corner-radius", use: "elbow" }]} />
          <Note><b>Position stability</b> is the contract that makes a 0.5s poll survivable: layout is computed from ids + dependency edges only — never from status, duration or counts — so a task turning <code>running → succeeded</code> repaints one card and moves nothing. Layering is longest-path and capped by node count, so a cyclic graph still terminates and still draws.</Note>
          <Dont>Never add a connect handle, a port dot, or a drag-to-move affordance. Anything that looks editable promises an edit the host cannot honour — DAGs are built by agents over CLI/IPC.</Dont>
        </Spec>
      </Section>

      <Section id="node" title="Node card · 8 states">
        <Spec title="Task node — every execution state"
          when={<>One card = one task. Three channels carry state so <b>colour is never alone</b>: the <b>leading 3px bar</b> (position), a <b>mono glyph</b> (◦ ❯ ◑ ✓ ✗ − ⊘ ?), and the <b>spelled label</b>. <code>running</code> is the loudest — accent border + accent wash — and it is <b>static</b>: the 0ms terminal-motion policy holds, so there is no spinner and no pulse on the card.</>}>
          <Stage variant="grid" style={{ display: "flex", flexWrap: "wrap", gap: 16, padding: 20, alignItems: "flex-start" }}>
            {DAG_STATUS_ORDER.map((s) => (
              <NodeBox key={s} node={mk({ status: s, name: `task:${s}`, dur: s === "waiting" || s === "ready" ? null : "12s" })}
                dimmed={s === "skipped" || s === "cancelled"} caption={s} />
            ))}
          </Stage>
          <Meta
            specs={[["card", "168 × 48 · radius 4"], ["padding", "4 / 8 · row gap 4"], ["status bar", "3px, full height"], ["name", "13 medium, 1 line, ellipsis"], ["meta row", "10 caps · duration mono, right"], ["dimmed", "skipped + cancelled at 0.4"]]}
            tokens={DAG_STATUS_ORDER.map((s) => ({ tok: `--tasty-dag-status-${s}`, use: DAG_STATUS[s].label.toLowerCase(), color: sTok(s) }))} />
          <Note><b>Colour vs. text:</b> the state colour is the <i>recede</i> channel — bar, border, glyph on a chip. The spelled label reads from its own <code>--tasty-dag-status-&lt;state&gt;-label</code> role: waiting takes <code>--tasty-text-muted</code>, and the two dimmed states take <code>--tasty-text-secondary</code>, which still measures 4.74:1 after the 0.75 card dim.</Note>
          <Note><b>Truncation rule (i18n):</b> the task name is a single line with ellipsis at the card edge — never wraps, never shrinks the font; the full name lives in the tooltip and the detail panel. On the meta row the <b>status label yields first</b>: it ellipses before the duration is dropped, so a long German or Japanese state string can't push the timing out of the card.</Note>
        </Spec>

        <Spec title="Task kinds"
          when={<>Four task kinds, told apart by the leading glyph: <code>run</code> (shell), <code>custom</code> (IPC call), <code>reduce</code> (result fan-in), <code>wait_barrier</code> (gate).</>}>
          <Stage variant="grid" style={{ display: "flex", flexWrap: "wrap", gap: 16, padding: 20 }}>
            {Object.keys(DAG_KIND).map((k) => (
              <NodeBox key={k} node={mk({ kind: k, status: "succeeded", name: `${DAG_KIND[k].label}:step`, dur: "4s" })} caption={k} />
            ))}
          </Stage>
          <Meta specs={[["glyph", "14px, muted, leading"], ["run", "terminal"], ["custom", "plug"], ["reduce", "layers"], ["wait_barrier", "lock"]]}
            tokens={[{ tok: "--tasty-dag-node-meta-fg", use: "kind glyph", color: "var(--tasty-dag-node-meta-fg)" }]} />
          <Note>New glyphs requested from <code>icons/</code>: none — <code>terminal</code>, <code>plug</code>, <code>layers</code> and <code>lock</code> already carry these four kinds. A dedicated <code>barrier</code> glyph would read better than <code>lock</code> if one is ever drawn.</Note>
        </Spec>

        <Spec title="Level of detail · selection · overflow"
          when={<>Zooming out never changes the <b>box</b>, only its <b>content</b>: <b>full</b> (≥70%) name + status + duration · <b>compact</b> (≥40%) glyph + name · <b>block</b> (&lt;40%) status fill only. At 200 nodes the block tier keeps every state readable as colour while the graph shape stays legible.</>}>
          <Stage variant="grid" style={{ display: "flex", flexWrap: "wrap", gap: 16, padding: 20, alignItems: "flex-start" }}>
            <NodeBox node={mk({ status: "running" })} lod="full" caption="full ≥ 70%" />
            <NodeBox node={mk({ status: "running" })} lod="compact" caption="compact ≥ 40%" />
            <NodeBox node={mk({ status: "running" })} lod="block" caption="block < 40%" />
            <NodeBox node={mk({ status: "failed", name: "build:linux-x86_64-with-a-very-long-target-triple" })} caption="overflow → ellipsis" />
            <NodeBox node={mk({ status: "succeeded" })} selected caption="selected (2px ring)" />
          </Stage>
          <Meta specs={[["full", "zoom ≥ 0.7"], ["compact", "0.4 – 0.7"], ["block", "< 0.4"], ["selection", "2px accent outline, 1px offset"], ["hover", "derived overlay-hover on the body"]]}
            tokens={[{ tok: "--tasty-dag-node-selected-ring", use: "selection", color: "var(--tasty-dag-node-selected-ring)" }, { tok: "--tasty-dag-node-hover-bg", use: "hover wash" }, { tok: "--tasty-dag-node-dim-opacity", use: "skipped path" }]} />
          <Stage style={{ padding: 0, height: 360, display: "block" }}>
            <div style={{ height: 360, display: "flex" }}><DagCanvas dag={DAG_DENSE} dir="td" /></div>
          </Stage>
          <Note>55 nodes, auto-fitted on open — the canvas lands in the <b>compact</b> or <b>block</b> tier and says so in the bottom-left hint chip, so a user who sees colour blocks knows why and knows the way back.</Note>
        </Spec>
      </Section>

      <Section id="edges" title="Edges · 3 relations">
        <Spec title="Dependency edges"
          when={<>Relation is carried by <b>dash pattern and colour together</b>: <code>depends_on</code> solid neutral, <code>fallback</code> long-dash attention, <code>reduce</code> fine-dash info. Every edge ends in an arrowhead at the <b>dependent</b> task, so the direction reads as "waits for".</>}>
          <Stage variant="grid" style={{ display: "flex", gap: 32, padding: 20, alignItems: "flex-start" }}>
            {Object.keys(DAG_REL).map((r) => <EdgeSpecimen key={r} rel={r} />)}
          </Stage>
          <Meta
            specs={[["routing", "orthogonal, 4px elbow"], ["width", "1px"], ["fallback dash", "6 3"], ["reduce dash", "2 3"], ["arrow", "8px triangle at target"], ["selected node", "its edges take the accent"]]}
            tokens={[{ tok: "--tasty-dag-edge-depends", use: "depends_on", color: "var(--tasty-dag-edge-depends)" }, { tok: "--tasty-dag-edge-fallback", use: "fallback", color: "var(--tasty-dag-edge-fallback)" }, { tok: "--tasty-dag-edge-reduce", use: "reduce", color: "var(--tasty-dag-edge-reduce)" }, { tok: "--tasty-dag-edge-dim-opacity", use: "dead path" }]} />
          <Do>Dim the <b>dead path</b>: an edge leaving a failed/cancelled task, or entering a skipped one, drops to 0.4 along with the node. The failure stays loud; everything it stranded recedes.</Do>
        </Spec>
      </Section>

      <Section id="chrome" title="Canvas chrome">
        <Spec title="Zoom cluster + minimap"
          when={<>Bottom-right overlay, 8px off the canvas edge: minimap above, zoom cluster below (− · % · + · fit · direction). Below <b>560px</b> of surface width the minimap is dropped; below <b>400px</b> the percentage readout goes too, leaving five 28px targets.</>}>
          <Stage variant="solo center" style={{ gap: 24, padding: 24, alignItems: "flex-end" }}>
            <Minimap g={dagLayout(DAG_BUILD, "td")} dag={DAG_BUILD} z={0.8} ox={40} oy={20} vw={520} vh={360} dir="td" />
            <div style={{ display: "flex", flexDirection: "column", gap: 12, alignItems: "flex-start" }}>
              <ZoomCluster z={0.8} onZoom={() => {}} onFit={() => {}} dir="td" onDir={() => {}} />
              <ZoomCluster z={0.8} compact onZoom={() => {}} onFit={() => {}} dir="td" onDir={() => {}} />
            </div>
          </Stage>
          <Meta
            specs={[["cluster", "28px row · 1px border"], ["inset", "8px from canvas edge"], ["minimap", "160 × 112"], ["viewport", "1px accent rect"], ["minimap cutoff", "surface < 560px"], ["compact cutoff", "surface < 400px"]]}
            tokens={[{ tok: "--tasty-dag-chrome-bg", use: "cluster fill", color: "var(--tasty-dag-chrome-bg)" }, { tok: "--tasty-dag-minimap-viewport", use: "viewport rect", color: "var(--tasty-dag-minimap-viewport)" }, { tok: "--tasty-dag-minimap-min-surface", use: "560 cutoff" }]} />
          <Note>The minimap paints each node in its <b>status colour</b>, so it doubles as a health strip: a red block anywhere in the graph is visible without zooming out.</Note>
        </Spec>
      </Section>

      <Section id="runner" title="Runner badge">
        <Spec title="Host runner state"
          when={<>Four values in one pill: on/off, crashed, <code>ready_count</code>, <code>running_count</code>. The case that must be <b>noticed</b> is <b>runner stopped with work ready</b> — nothing is advancing the graph — so it takes the warning tone and carries the resume hint beside it.</>}>
          <Stage variant="solo" style={{ flexDirection: "column", alignItems: "flex-start", gap: 12, padding: 20 }}>
            <RunnerBadge runner={{ running: true, crashed: false, ready: 3, active: 2 }} />
            <RunnerBadge runner={{ running: true, crashed: false, ready: 0, active: 0 }} />
            <RunnerBadge runner={{ running: false, crashed: false, ready: 4, active: 0 }} />
            <RunnerBadge runner={{ running: false, crashed: false, ready: 0, active: 0 }} />
            <RunnerBadge runner={{ running: false, crashed: true, ready: 2, active: 0 }} />
          </Stage>
          <Meta
            specs={[["pill", "22px · radius 2 · 1px border"], ["dot", "StatusDot, pulses only while work runs"], ["counts", "mono 11"], ["hint", "hidden on a narrow header"]]}
            tokens={[{ tok: "--tasty-dag-runner-stalled-fg", use: "stopped + ready", color: "var(--tasty-dag-runner-stalled-fg)" }, { tok: "--tasty-dag-runner-crashed-fg", use: "crashed", color: "var(--tasty-dag-runner-crashed-fg)" }, { tok: "--tasty-dag-runner-idle-fg", use: "stopped, no work", color: "var(--tasty-dag-runner-idle-fg)" }]} />
          <Note>"Stopped with no work" is <b>not</b> a warning — it is the resting state of a finished graph, so it stays muted. Only stopped-<i>with</i>-ready earns the yellow.</Note>
        </Spec>
      </Section>

      <Section id="rows" title="DAG list row">
        <Spec title="One row per DAG — across all workspaces"
          when={<>A <code>ListCtrl</code> row (36px, icon + label + description) plus the one new part: a trailing cluster of <b>origin tag</b>, <b>rollup state</b> (same 8-state vocabulary as a task) and a mono <b>done/total</b>. Progress is text, not a bar — at this density <code>7/12</code> is exact where a 60px bar is only a suggestion.</>}>
          <Stage style={{ padding: 0, display: "block" }}>
            <div style={{ background: "var(--tasty-bg-panel)" }}>
              <ListCtrl items={dagRowItems(DAG_LIST)} onSelect={() => {}} />
            </div>
          </Stage>
          <Meta
            specs={[["row", "36px min · ListCtrl density"], ["label", "13 — DAG name"], ["description", "11 — workspace · updated"], ["trailing", "origin tag · rollup · n/total"], ["scope", "all workspaces, filterable"]]}
            tokens={[{ tok: "--tasty-dag-row-height", use: "36 row", }, { tok: "--tasty-dag-row-count-fg", use: "n/total", color: "var(--tasty-dag-row-count-fg)" }, { tok: "--tasty-listctrl-row-bg-hover", use: "hover" }]} />
          <Note>The workspace name is part of the description line, not a separate column — the popup lists every workspace, and a column would waste the width the DAG name needs.</Note>
        </Spec>
      </Section>

      <Section id="detail" title="Node detail">
        <Spec title="Selected task — side panel · bottom sheet"
          when={<>Selecting a node opens a <b>288px right panel</b> on a wide surface and a <b>220px bottom sheet</b> when the surface is narrow (&lt; 640px) or inside the popup. Failure output can run to dozens of lines, so the error tail is its own bounded, scrollable, copyable block — never an expanding wall that pushes the graph off screen.</>}>
          <Stage style={{ padding: 0, display: "block" }}>
            <div style={{ display: "flex", height: 460, background: "var(--tasty-bg-panel)" }}>
              <DagDetail dag={DAG_BUILD} id="build_linux" onClose={() => {}} onSelect={() => {}} />
              <DagDetail dag={DAG_BUILD} id="unit" onClose={() => {}} onSelect={() => {}} />
            </div>
          </Stage>
          <Meta
            specs={[["panel", "288 wide · 12 padding"], ["sheet", "220 tall (narrow)"], ["error tail", "max 160, scrolls, copy button"], ["output tail", "max 112, scrolls"], ["deps", "22px rows, click to jump"]]}
            tokens={[{ tok: "--tasty-dag-detail-bg", use: "panel fill", color: "var(--tasty-dag-detail-bg)" }, { tok: "--tasty-dag-detail-log-bg", use: "log bed", color: "var(--tasty-dag-detail-log-bg)" }, { tok: "--tasty-dag-detail-log-max-height", use: "160 error tail" }]} />
          <Note>Dependency rows are the graph's keyboard-free navigation: each row shows the upstream task's glyph + relation and jumps the selection there, so a failure can be walked back to its cause without hunting on the canvas.</Note>
        </Spec>
      </Section>

      <Section id="states" title="Empty · cycle">
        <Spec title="Empty states and the cycle warning"
          when={<>Three non-happy states: <b>no DAGs in this workspace</b> (surface), <b>no filter match</b> (popup list), and <b>cycle detected</b> — a warning strip pinned to the canvas top. The cyclic graph <b>keeps rendering</b> behind it: this surface observes, it never hides state.</>}>
          <Stage variant="grid" style={{ flexDirection: "column", gap: 16, padding: 20, alignItems: "stretch" }}>
            <div style={{ display: "flex", minHeight: 160, border: "var(--tasty-border-width) solid var(--tasty-border-default)",
              borderRadius: "var(--tasty-radius)", background: "var(--tasty-bg-panel)" }}>
              <DagEmpty variant="surface" />
            </div>
            <div style={{ display: "flex", minHeight: 160, border: "var(--tasty-border-width) solid var(--tasty-border-default)",
              borderRadius: "var(--tasty-radius)", background: "var(--tasty-bg-panel)" }}>
              <DagEmpty variant="search" query="deploy" />
            </div>
            <CycleBanner ids={DAG_CYCLE.cycle} />
          </Stage>
          <Stage style={{ padding: 0, height: 320, display: "block" }}>
            <div style={{ height: 320, display: "flex" }}><DagCanvas dag={DAG_CYCLE} dir="td" /></div>
          </Stage>
          <Meta
            specs={[["banner", "28px · pinned to canvas top"], ["banner copy", "names the cycle path"], ["empty A", "surface — how a DAG appears"], ["empty B", "search — echoes the query"]]}
            tokens={[{ tok: "--tasty-dag-cycle-bg", use: "banner wash", color: "var(--tasty-dag-cycle-bg)" }, { tok: "--tasty-dag-cycle-fg", use: "banner text", color: "var(--tasty-dag-cycle-fg)" }, { tok: "--tasty-text-disabled", use: "empty glyph" }]} />
        </Spec>
      </Section>

      <Section id="surfaces" title="Surface · popup">
        <Spec title="Full-tab surface — wide and narrow"
          when={<>A whole terminal tab. Header: DAG name + task count, the DAG <code>Select</code>, the runner badge, refresh. Under <b>640px</b> the header wraps to two rows (identity above, controls below), the runner hint drops, the minimap goes, and the detail panel becomes a bottom sheet.</>}>
          <Stage style={{ padding: 16, display: "flex", gap: 16, alignItems: "stretch" }}>
            <div style={{ flex: 1, minWidth: 0, height: 520, border: "var(--tasty-border-width) solid var(--tasty-border-strong)",
              borderRadius: "var(--tasty-radius)", overflow: "hidden" }}>
              <DagSurface dags={[DAG_BUILD, DAG_INDEX, DAG_DENSE, DAG_CYCLE]} />
            </div>
            <div style={{ flex: "none", width: 320, height: 520, border: "var(--tasty-border-width) solid var(--tasty-border-strong)",
              borderRadius: "var(--tasty-radius)", overflow: "hidden" }}>
              <DagSurface dags={[DAG_BUILD, DAG_INDEX]} narrow />
            </div>
          </Stage>
          <Meta
            specs={[["header", "8/12 padding · wraps < 640"], ["canvas", "all remaining space"], ["detail", "288 side → 220 sheet"], ["narrow floor", "320px, nothing clipped"], ["DAG switch", "instant swap + auto-fit"]]}
            tokens={[{ tok: "--tasty-bg-sidebar", use: "header band", color: "var(--tasty-bg-sidebar)" }, { tok: "--tasty-separator", use: "header hairline" }, { tok: "--tasty-dag-detail-sheet-height", use: "220 sheet" }]} />
          <Note>Switching DAG in the header <code>Select</code> replaces the canvas instantly (0ms) and re-fits — a new graph has no position to preserve, so fit is the honest default. Within one DAG the view never re-fits on its own.</Note>
        </Spec>

        <Spec title="Workspace popup — list ⇄ single DAG"
          when={<>A movable 560 × 460 popup. The list carries search, a <b>this workspace only</b> toggle and a status filter; picking a row swaps the whole area for that DAG's canvas via <code>DrillDown</code> — instant, with a back bar that keeps the runner badge in view. The bottom sheet holds node detail here, because 560px has no room for a side panel.</>}>
          <Stage style={{ padding: 0, display: "block" }}><PopupStage /></Stage>
          <Meta
            specs={[["frame", "560 × 460 · resizable"], ["titlebar", "28px drag strip"], ["list rows", "36px · ~8 visible"], ["swap", "DrillDown, 0ms"], ["dismiss", <>outside click · <span className="ic">Esc</span> · ×</>]]}
            tokens={[{ tok: "--tasty-dag-popup-width", use: "560 frame" }, { tok: "--tasty-dag-popup-height", use: "460 frame" }, { tok: "--tasty-drilldown-backbar-height", use: "36 back bar" }, { tok: "--tasty-shadow-modal", use: "lift" }]} />
        </Spec>
      </Section>
    </>
  );
}

window.Gallery.mount("dag", NAV, {
  title: "Task DAG",
  intro: "The read-only view onto an agent-built task graph — a full-tab surface and a workspace popup, sharing one canvas. Observation only: no ports, no drag-to-connect, no editing.",
  howto: true,
}, <Page />);
