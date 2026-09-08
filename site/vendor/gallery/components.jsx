// Tasty Gallery — Components. Live, interactive specimens of the DS
// primitives. Hover / focus work for real on the page; persistent
// states (active, disabled) are shown explicitly. Each carries a
// "when to use" note, the layout spec, and the tokens it consumes.
const { Section, Spec, Stage, Cluster, Meta, Note, Do, Dont, GIcon } = window.Gallery;
const {
  Button, IconButton, Badge, BadgeGroup, Tag, Kbd,
  Input, Select, MultiSelect, AutoComplete, Checkbox, Switch,
  Tab, TreeRow, MenuItem, StatusDot, Toast, Table, Spinner,
  HelpHint, Tooltip, ListCtrl,
} = window.TastyDesignSystem_41fd3f;

const NAV = [
  { id: "buttons", label: "Buttons" },
  { id: "chips", label: "Badge · Tag · Kbd" },
  { id: "forms", label: "Form controls" },
  { id: "nav", label: "Tab · TreeRow · Menu" },
  { id: "feedback", label: "StatusDot · Spinner · Toast" },
  { id: "helphint", label: "HelpHint · Tooltip" },
  { id: "text", label: "Hint text" },
  { id: "data", label: "Table" },
  { id: "listctrl", label: "ListCtrl" },
];

const ic = {
  plus: <GIcon d={<path d="M12 5v14M5 12h14" />} />,
  term: <GIcon d={<><rect x="3" y="4" width="18" height="16" rx="2" /><path d="m7 9 3 3-3 3M13 15h4" /></>} />,
  md: <GIcon d={<><rect x="3" y="5" width="18" height="14" rx="2" /><path d="M7 15V9l2.5 3L12 9v6M16 9v4m0 0 2-2m-2 2-2-2" /></>} />,
  search: <GIcon d={<><circle cx="11" cy="11" r="7" /><path d="m21 21-4.3-4.3" /></>} />,
  folder: <GIcon d={<path d="M4 20h16a1 1 0 0 0 1-1V8a1 1 0 0 0-1-1h-7l-2-2H4a1 1 0 0 0-1 1v13a1 1 0 0 0 1 1z" />} />,
  file: <GIcon d={<><path d="M14 3v4a1 1 0 0 0 1 1h4" /><path d="M17 21H7a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h7l5 5v11a2 2 0 0 1-2 2z" /></>} />,
  folderOpen: <GIcon d={<path d="M3 8a1 1 0 0 1 1-1h5l2 2h7a1 1 0 0 1 1 1v1H3z M3 11h18l-1.5 8a1 1 0 0 1-1 1H5.5a1 1 0 0 1-1-1z" />} />,
  split: <GIcon d={<><rect x="3" y="4" width="18" height="16" rx="2" /><path d="M12 4v16" /></>} />,
  copy: <GIcon d={<><rect x="9" y="9" width="11" height="11" rx="2" /><path d="M5 15V5a2 2 0 0 1 2-2h8" /></>} />,
  trash: <GIcon d={<path d="M4 7h16M10 11v6M14 11v6M5 7l1 13a1 1 0 0 0 1 1h10a1 1 0 0 0 1-1l1-13M9 7V4a1 1 0 0 1 1-1h4a1 1 0 0 1 1 1v3" />} />,
};

// AutoComplete specimen candidate lists (path-shaped — recent dirs / files).
const AC_DIRS = ["~/Downloads", "~/work/tasty", "~/work/tasty-ui/src", "~/work/tasty/crates/tasty-ui-widgets", "~/.config/tasty"];
const AC_FILES = ["~/work/tasty/README.md", "~/work/tasty/docs/architecture.md", "~/work/tasty/docs/design/systems/theme.md", "~/work/tasty/CHANGELOG.md"];
const AC_MANY = ["~/work/tasty", "~/work/tasty-ui/src", "~/work/tasty/crates/tasty-core", "~/work/tasty/crates/tasty-ui-widgets", "~/work/tasty/crates/tasty-gallery", "~/work/tasty/docs/adr", "~/work/tasty/docs/design/policies", "~/.config/tasty", "~/.local/share/tasty", "~/Downloads/exports"];

// MultiSelect specimen data — the DAG list status filter (first consumer).
const MS_STATES = [
  { value: "waiting", label: "Waiting" }, { value: "ready", label: "Ready" },
  { value: "running", label: "Running" }, { value: "done", label: "Done" },
  { value: "failed", label: "Failed" }, { value: "cancelled", label: "Cancelled" },
];
const MS_STATES_DISABLED = MS_STATES.map((o) => (o.value === "cancelled" ? { ...o, disabled: true } : o));
const MS_LONG = [
  { value: "a", label: "release-candidate-nightly-build-pipeline" },
  { value: "b", label: "integration-tests-postgres-and-redis" },
  { value: "c", label: "docs" },
];
const MS_MANY = Array.from({ length: 20 }, (_, i) => ({
  value: "opt-" + String(i + 1).padStart(2, "0"),
  label: "Workspace " + String(i + 1).padStart(2, "0"),
}));
// The injected i18n hook: 0 → null (falls back to placeholder) · N · all.
const msSummary = (n, total) => (n === 0 ? null : n === total ? "All" : n + " selected");

function Components() {
  return (
    <>
      {/* BUTTONS */}
      <Section id="buttons" title="Buttons">
        <Spec title="Button" badges={<HoverBadge />}
          when={<>The text action. <b>primary</b> for the one affirmative action, <b>secondary</b> (outlined) for alternates, <b>ghost</b> for low-emphasis/toolbar, <b>danger</b> for destructive, <b>agent</b> (mauve) when an AI agent is the actor. One primary per view.</>}>
          <Stage variant="column" style={{ gap: 18 }}>
            <Cluster label="variants — hover & click them">
              <Button variant="primary">Save</Button>
              <Button variant="secondary">Open folder</Button>
              <Button variant="ghost">Cancel</Button>
              <Button variant="danger">Force detach</Button>
              <Button variant="agent">Run agent task</Button>
            </Cluster>
            <Cluster label="sizes — sm 24 · md 28 · lg 32">
              <Button size="sm" variant="secondary">Small</Button>
              <Button size="md" variant="secondary">Medium</Button>
              <Button size="lg" variant="secondary">Large</Button>
            </Cluster>
            <Cluster label="with icons · disabled">
              <Button variant="secondary" leadingIcon={ic.plus}>New tab</Button>
              <Button variant="primary" trailingIcon={ic.search}>Search</Button>
              <Button variant="secondary" disabled>Disabled</Button>
            </Cluster>
          </Stage>
          <Meta
            specs={[["height", "28 / 24 / 32px"], ["padding", <>0 <span className="tok">--tasty-space-md</span></>], ["radius", <>4px <span className="tok">--tasty-radius</span></>], ["hover / active", "8% / 12% overlay"], ["focus", "2px accent ring"]]}
            tokens={[
              { tok: "--tasty-accent-primary", use: "primary fill", color: "var(--tasty-accent-primary)" },
              { tok: "--tasty-accent-danger", use: "danger fill", color: "var(--tasty-accent-danger)" },
              { tok: "--tasty-accent-agent", use: "agent fill", color: "var(--tasty-accent-agent)" },
              { tok: "--tasty-overlay-hover", use: "8% hover" },
              { tok: "--tasty-text-on-accent", use: "filled label" },
            ]} />
        </Spec>

        <Spec title="IconButton"
          when={<>Square, icon-only — toolbars, tab close, sidebar rails. Same heights as Button. <b>ghost</b> by default; <b>active</b> shows the persistent accent selection (e.g. an engaged rail tool).</>}>
          <Stage>
            <Cluster label="ghost · solid · active">
              <IconButton aria-label="Split">{ic.split}</IconButton>
              <IconButton variant="solid" aria-label="Search">{ic.search}</IconButton>
              <IconButton active aria-label="Terminal">{ic.term}</IconButton>
              <IconButton size="sm" aria-label="New">{ic.plus}</IconButton>
            </Cluster>
          </Stage>
          <Meta
            specs={[["size", "28px (sm 24)"], ["shape", "square, 4px radius"], ["active", <span className="tok">--tasty-accent-primary</span>]]}
            tokens={[{ tok: "--tasty-overlay-hover", use: "hover tint" }, { tok: "--tasty-accent-primary", use: "active", color: "var(--tasty-accent-primary)" }, { tok: "--tasty-text-muted", use: "rest icon", color: "var(--tasty-text-muted)" }]} />
        </Spec>
      </Section>

      {/* CHIPS */}
      <Section id="chips" title="Badge · Tag · Kbd">
        <Spec title="Badge"
          when={<>Compact count or status pill. <b>danger</b> by default for unread counts (<code>99+</code>); use <code>dot</code> for a bare presence indicator on a rail icon. Every variant is a <b>bg/fg token pair</b> (<span className="tok">--tasty-badge-&lt;variant&gt;-bg/-fg</span>) — a badge never reaches into an accent role directly. <b>primary</b> (blue) = Completion attention, <b>warning</b> (yellow) = NeedsInput attention; when a row shows both, wrap them in <code>BadgeGroup</code> — NeedsInput leads, Completion trails.</>}>
          <Stage>
            <Cluster label="counts"><Badge>3</Badge><Badge>99+</Badge><Badge variant="primary">12</Badge><Badge variant="warning">2</Badge><Badge variant="agent">new</Badge><Badge variant="success">ok</Badge></Cluster>
            <Cluster label="attention kinds"><BadgeGroup><Badge variant="warning">2</Badge><Badge variant="primary">5</Badge></BadgeGroup><BadgeGroup><Badge variant="warning">99+</Badge><Badge variant="primary">99+</Badge></BadgeGroup></Cluster>
            <Cluster label="dot"><Badge dot variant="danger" /><Badge dot variant="warning" /><Badge dot variant="primary" /><Badge dot variant="agent" /><Badge dot variant="success" /></Cluster>
          </Stage>
          <Meta specs={[["radius", <span className="tok">--tasty-radius-pill</span>], ["size", "caption 11px"], ["group gap", <span className="tok">--tasty-badge-group-gap</span>], ["overflow", "99+"]]}
            tokens={[{ tok: "--tasty-badge-danger-bg", use: "default", color: "var(--tasty-badge-danger-bg)" }, { tok: "--tasty-badge-warning-bg", use: "NeedsInput count", color: "var(--tasty-badge-warning-bg)" }, { tok: "--tasty-badge-primary-bg", use: "Completion count", color: "var(--tasty-badge-primary-bg)" }, { tok: "--tasty-badge-agent-bg", use: "agent", color: "var(--tasty-badge-agent-bg)" }, { tok: "--tasty-font-size-caption", use: "11px" }]} />
          <Note>Attention kinds and their rank live on <b>Layouts › Attention kinds</b>; this spec only shows the chip. Don't pick <b>warning</b>/<b>primary</b> for a non-attention count — the two colors are read as “blocked” and “finished” everywhere else in the product.</Note>
        </Spec>

        <Spec title="Tag"
          when={<>Small monospace label for <b>surface kinds</b> (<code>terminal</code>, <code>markdown</code>), permissions (<code>fs:read</code>), plugin origins, and IDs. Outlined by default; the <b>agent</b> variant marks agent-owned things.</>}>
          <Stage>
            <Cluster label="variants">
              <Tag>terminal</Tag><Tag variant="accent">markdown</Tag><Tag variant="agent">plugin</Tag>
              <Tag variant="success" dot>running</Tag><Tag variant="warning" dot>readonly</Tag><Tag variant="danger" dot>error</Tag>
              <Tag variant="info" dot>info</Tag>
            </Cluster>
          </Stage>
          <Meta specs={[["font", <span className="tok">--tasty-font-mono</span>], ["radius", <span className="tok">--tasty-radius-sm</span>], ["dot", "leading state dot"]]}
            tokens={[{ tok: "--tasty-font-mono", use: "label" }, { tok: "--tasty-border-default", use: "outline" }, { tok: "--tasty-accent-agent", use: "agent", color: "var(--tasty-accent-agent)" }]} />
        </Spec>

        <Spec title="Kbd"
          when={<>Keyboard shortcuts as keycaps, in settings, menus, and the command palette. Pass an array or a <code>"+"</code>-joined string; it splits and caps each key.</>}>
          <Stage>
            <Cluster label="shortcuts"><Kbd keys="Ctrl+K" /><Kbd keys="Ctrl+Shift+N" /><Kbd keys={["⌘", ","]} /><Kbd keys="Esc" /></Cluster>
          </Stage>
          <Meta tokens={[{ tok: "--tasty-font-mono", use: "keycap" }, { tok: "--tasty-surface-raised", use: "cap fill", color: "var(--tasty-surface-raised)" }, { tok: "--tasty-border-default", use: "cap border" }]} />
        </Spec>
      </Section>

      {/* FORMS */}
      <Section id="forms" title="Form controls">
        <Spec title="Input" badges={<HoverBadge label="focus me" />}
          when={<>Single-line field. Use <code>mono</code> for paths, IDs, regex, hex. <code>icon</code> for a leading affordance (search), <code>addon</code> for a trailing unit. <code>invalid</code> turns the border + ring red.</>}>
          <Stage variant="column" style={{ gap: 12 }}>
            <Cluster label="default · icon · addon — click to focus">
              <Input placeholder="Workspace name" style={{ width: 200 }} />
              <Input icon={ic.search} placeholder="Filter…" style={{ width: 200 }} />
              <Input mono defaultValue="14" addon="px" style={{ width: 110 }} />
            </Cluster>
            <Cluster label="mono · invalid · disabled">
              <Input mono defaultValue="s_01HXK9" style={{ width: 200 }} />
              <Input invalid defaultValue="bad value" style={{ width: 160 }} />
              <Input disabled placeholder="Disabled" style={{ width: 160 }} />
            </Cluster>
          </Stage>
          <Meta
            specs={[["height", <>28px <span className="tok">--tasty-control-height</span></>], ["border", "1px → focus ring 2px"], ["padding", <>0 <span className="tok">--tasty-space-sm</span></>]]}
            tokens={[{ tok: "--tasty-surface-raised", use: "fill", color: "var(--tasty-surface-raised)" }, { tok: "--tasty-border-focus", use: "ring", color: "var(--tasty-border-focus)" }, { tok: "--tasty-accent-danger", use: "invalid", color: "var(--tasty-accent-danger)" }, { tok: "--tasty-text-placeholder", use: "hint" }]} />
        </Spec>

        <Spec title="Select · Checkbox · Switch"
          when={<><b>Select</b> = native dropdown, styled to match Input. <b>Checkbox</b> for a standalone boolean in a list of settings (inside <b>MultiSelect</b> it is the option row). <b>Switch</b> for a single boolean setting that applies immediately.</>}>
          <Stage style={{ gap: 28 }}>
            <Cluster label="Select"><Select options={["Default (full rc)", "Tasty rc", "Custom"]} style={{ width: 180 }} /></Cluster>
            <Cluster label="Checkbox">
              <div style={{ display: "flex", flexDirection: "column", gap: 8 }}>
                <Checkbox label="Confirm on close" defaultChecked />
                <Checkbox label="Restore layout" />
              </div>
            </Cluster>
            <Cluster label="Switch">
              <div style={{ display: "flex", flexDirection: "column", gap: 8 }}>
                <Switch label="Ligatures" defaultChecked />
                <Switch label="Reduced motion" />
              </div>
            </Cluster>
          </Stage>
          <Meta
            specs={[["control height", "28px"], ["checkbox", "16px square"], ["accent", <span className="tok">--tasty-accent-primary</span>]]}
            tokens={[{ tok: "--tasty-accent-primary", use: "checked / on", color: "var(--tasty-accent-primary)" }, { tok: "--tasty-surface-raised", use: "track / box", color: "var(--tasty-surface-raised)" }, { tok: "--tasty-border-default", use: "outline" }]} />
        </Spec>

        <Spec title="MultiSelect — many values in one control" badges={<HoverBadge label="click me" />}
          when={<>The <b>Select sibling</b> (not a variant) for when a control holds <b>more than one value</b> — an OR filter like <span className="ic">waiting + ready + running</span>. Closed it is a <code>Select</code>: same 28px height, border, radius, font, chevron, so the two read as one family in a form. Open it is a <b>menu of <code>Checkbox</code> rows that stays open while you toggle</b> — outside click or <span className="ic">Esc</span> closes. The trigger never enumerates the selection; it shows a <b>caller-injected 3-branch summary</b> (0 → placeholder tone · N → <span className="ic">3 selected</span> · all → <span className="ic">All</span>) as <b>plain text</b>, never a <code>Badge</code>. Checked rows get <b>no background</b> — the checkmark is the only checked signal; backgrounds are reserved for pointer <b>hover</b> and the keyboard <span className="ic">↑↓</span> <b>active</b> row.</>}>
          <Stage variant="solo center" style={{ padding: 20, background: "var(--tasty-bg-app)", flexDirection: "column", gap: 22 }}>
            <div style={{ display: "grid", gridTemplateColumns: "repeat(3, 240px)", gap: 20, justifyContent: "center" }}>
              {[
                ["live — click to open, toggle freely", <MultiSelect key="l" block options={MS_STATES} defaultValue={["waiting", "ready", "running"]} summary={msSummary} placeholder="No status" />, 44],
                ["summary — 0 selected (placeholder tone)", <MultiSelect key="s0" block options={MS_STATES} value={[]} summary={msSummary} placeholder="No status" />, 44],
                ["summary — all selected", <MultiSelect key="sa" block options={MS_STATES} value={MS_STATES.map(o => o.value)} summary={msSummary} />, 44],
                ["focus / open — border-focus + 1px ring, chevron flipped", <MultiSelect key="o" block open options={MS_STATES} value={["waiting", "ready", "running"]} summary={msSummary} />, 230],
                ["rows — hover (row 4) vs keyboard-active (row 1)", <MultiSelect key="h" block open activeIndex={0} hoverIndex={3} options={MS_STATES} value={["waiting", "ready", "running"]} summary={msSummary} />, 230],
                ["per-option disabled (row 6: no cancelled runs)", <MultiSelect key="d" block open activeIndex={-1} options={MS_STATES_DISABLED} value={["running"]} summary={msSummary} />, 230],
                ["disabled control", <MultiSelect key="dd" block disabled options={MS_STATES} value={["running"]} summary={msSummary} />, 44],
                ["long labels — ellipsis in trigger and rows", <MultiSelect key="e" block open activeIndex={-1} options={MS_LONG} value={["a", "b"]} summary={(n) => n + " pipelines selected — very long summary"} />, 200],
                ["20 options → internal scroll at 220", <MultiSelect key="m" block open activeIndex={-1} options={MS_MANY} value={["opt-02", "opt-05"]} summary={msSummary} allToggle />, 230],
              ].map(([label, node, minH]) => (
                <div key={label} style={{ display: "flex", flexDirection: "column", gap: 8, minHeight: minH, alignItems: "stretch" }}>
                  <div style={{ fontSize: 11, color: "var(--tasty-text-muted)" }}>{label}</div>
                  <div style={{ position: "relative" }}>{node}</div>
                </div>
              ))}
            </div>
            <div style={{ display: "flex", gap: 16, alignItems: "flex-end", padding: "14px 16px", background: "var(--tasty-bg-panel)", border: "1px solid var(--tasty-border-default)", borderRadius: 4 }}>
              <div style={{ display: "flex", flexDirection: "column", gap: 6 }}>
                <div style={{ fontSize: 11, color: "var(--tasty-text-muted)" }}>Sort by — Select (one value)</div>
                <Select options={["Started", "Name", "Duration"]} style={{ width: 160 }} />
              </div>
              <div style={{ display: "flex", flexDirection: "column", gap: 6 }}>
                <div style={{ fontSize: 11, color: "var(--tasty-text-muted)" }}>Status — MultiSelect (many)</div>
                <MultiSelect options={MS_STATES} defaultValue={["waiting", "ready", "running"]} summary={msSummary} placeholder="No status" style={{ width: 160 }} />
              </div>
            </div>
          </Stage>
          <Meta
            specs={[["trigger", <>Select language — 28px <span className="tok">--tasty-multiselect-height</span> · 12 left · 28 chevron room</>], ["open trigger", "border-focus + 1px ring, chevron rotates 180°"], ["summary", "3 branches, caller-injected · plain text · single line + ellipsis"], ["menu", <>4 below the trigger · min-width = trigger · grows to content up to <span className="tok">--tasty-multiselect-menu-max-width</span> (320)</>], ["row", "28px · 12 padding-x · 8 box→label · Checkbox 16px"], ["checked row", "checkmark only — no background"], ["overflow", <>scrolls past 220 <span className="tok">--tasty-multiselect-menu-max-height</span> · <code>.tasty-scroll</code></>], ["bulk row", <><span className="tok">allToggle</span> — accent action row + separator, off below ~8 options</>], ["keys", "↓/↵/Space open · ↑↓ Home End move · Space/↵ toggle (menu stays) · Esc close"], ["motion", "border/chevron 120ms · check state 0ms"]]}
            tokens={[
              { tok: "--tasty-multiselect-bg", use: "trigger fill", color: "var(--tasty-multiselect-bg)" },
              { tok: "--tasty-multiselect-border", use: "trigger edge", color: "var(--tasty-multiselect-border)" },
              { tok: "--tasty-multiselect-border-focus", use: "focus / open", color: "var(--tasty-multiselect-border-focus)" },
              { tok: "--tasty-multiselect-summary-fg-empty", use: "0 selected", color: "var(--tasty-multiselect-summary-fg-empty)" },
              { tok: "--tasty-multiselect-menu-bg", use: "menu fill", color: "var(--tasty-multiselect-menu-bg)" },
              { tok: "--tasty-multiselect-row-bg-hover", use: "pointer hover", color: "var(--tasty-multiselect-row-bg-hover)" },
              { tok: "--tasty-multiselect-row-bg-active", use: "keyboard-active", color: "var(--tasty-multiselect-row-bg-active)" },
              { tok: "--tasty-multiselect-row-fg", use: "row label (primary, not muted)", color: "var(--tasty-multiselect-row-fg)" },
              { tok: "--tasty-checkbox-bg-checked", use: "checked box", color: "var(--tasty-checkbox-bg-checked)" },
              { tok: "--tasty-multiselect-all-fg", use: "bulk select/clear", color: "var(--tasty-multiselect-all-fg)" },
            ]} />
          <Note><b>Decisions (implementation spec).</b> <b>Sibling, not a <code>Select</code> variant</b> — the open surface is a different thing (checkbox menu, non-native, stays open), so it earns its own catalog entry; <code>Select</code> is untouched. <b>Open = focus treatment + flipped chevron</b>, no third visual state. <b>Bulk toggle exists but is opt-in</b> (<span className="tok">allToggle</span>): one accent action row that reads "Select all" and becomes "Clear all" once everything is on — a checkbox there would need an indeterminate state this system doesn't have. <b>Menu may outgrow the trigger</b> up to 320, because the trigger holds a short summary while the rows hold real labels. <b>Per-option disabled is supported</b> (a status with no runs, a permission-gated option) — row keeps <span className="tok">--tasty-state-disabled-opacity</span> and is not togglable. <b>Count is plain text</b>, not a <code>Badge</code>: badges live on tabs and rails, not inside form controls. <b>Zero selected is allowed</b> — for a filter, nothing checked should mean <i>no filter</i> (show all) rather than an empty view; forcing a minimum is consumer policy, not a control rule. Row labels are <span className="tok">--tasty-text-primary</span> (never muted) so Latte stays ≥4.5:1 on <span className="tok">--tasty-surface-raised</span>. First consumer: the DAG list status filter (<code>ui_kits/terminal/overlays/dag_view.jsx</code>) — 6 statuses, <span className="tok">allToggle</span> off.</Note>
        </Spec>

        <Spec title="AutoComplete — free-text trigger + candidate dropdown (typeahead)"
          when={<>A real <b>typeahead</b>: a free-text trigger (<code>Input</code> language) with a floating <b>candidate dropdown</b> (menu container + <code>MenuItem</code> language) that <b>narrows as you type</b>. Use it — not <b>Select</b> — when the user types free text and the list only <i>suggests</i> (paths, recent files, recent directories). <b>Select</b> stays the closed picker for a fixed value set. Path candidates render <b>middle-ellipsis</b> (filename tail preserved); the matched run is highlighted in <span className="tok">--tasty-accent-primary</span>. Two row states: pointer <b>hover</b> (<span className="tok">--tasty-overlay-hover</span>) vs keyboard <span className="ic">↑↓</span> <b>active</b> (<span className="tok">--tasty-surface-active</span>, stronger — wins when both). <b><span className="tok">maxDropdownHeight</span></b> caps the list; past it the list <b>scrolls internally</b> (<code>.tasty-scroll</code>) and shrinks-to-fit below.</>}>
          <Stage variant="solo center" style={{ padding: 20, background: "var(--tasty-bg-app)", flexDirection: "column", gap: 22 }}>
            <div style={{ display: "grid", gridTemplateColumns: "repeat(3, 260px)", gap: 20, justifyContent: "center" }}>
              {[
                ["idle — closed trigger", <AutoComplete key="i" block mono withGo icon={ic.folderOpen} rowIcon={ic.folderOpen} open={false} query="~/work/tasty" items={AC_DIRS} />, 44],
                ["open — full candidate list", <AutoComplete key="o" block mono withGo icon={ic.folderOpen} rowIcon={ic.folderOpen} open activeIndex={0} query="" items={AC_DIRS} maxDropdownHeight={220} />, 200],
                ["typing → filtered + highlight", <AutoComplete key="f" block mono withGo icon={ic.folderOpen} rowIcon={ic.folderOpen} open activeIndex={0} query="tasty" items={AC_DIRS} maxDropdownHeight={220} />, 160],
                ["overflow → internal scroll", <AutoComplete key="s" block mono withGo icon={ic.folderOpen} rowIcon={ic.folderOpen} open activeIndex={0} query="" items={AC_MANY} maxDropdownHeight={132} />, 170],
                ["empty / no match", <AutoComplete key="e" block mono withGo icon={ic.folderOpen} rowIcon={ic.folderOpen} open query="zzzz" items={AC_DIRS} emptyLabel="No matching path" />, 90],
                ["hover (row 3) vs keyboard-active (row 1)", <AutoComplete key="h" block mono withGo icon={ic.folderOpen} rowIcon={ic.folderOpen} open activeIndex={1} hoverIndex={3} query="" items={AC_DIRS} maxDropdownHeight={220} />, 200],
              ].map(([label, node, minH]) => (
                <div key={label} style={{ display: "flex", flexDirection: "column", gap: 8, minHeight: minH, alignItems: "stretch" }}>
                  <div style={{ fontSize: 11, color: "var(--tasty-text-muted)" }}>{label}</div>
                  <div style={{ position: "relative" }}>{node}</div>
                </div>
              ))}
            </div>
            <div style={{ display: "grid", gridTemplateColumns: "repeat(2, 260px)", gap: 20, justifyContent: "center" }}>
              {[
                ["explorer context — folderOpen · recent directories", ic.folderOpen, AC_DIRS],
                ["markdown context — file · recent files", ic.file, AC_FILES],
              ].map(([label, icon, items]) => (
                <div key={label} style={{ display: "flex", flexDirection: "column", gap: 8, minHeight: 200 }}>
                  <div style={{ fontSize: 11, color: "var(--tasty-text-muted)" }}>{label}</div>
                  <div style={{ position: "relative" }}><AutoComplete block mono withGo icon={icon} rowIcon={icon} open activeIndex={0} query="" items={items} maxDropdownHeight={220} /></div>
                </div>
              ))}
            </div>
          </Stage>
          <Meta
            specs={[["trigger", "Input language · leading icon slot · mono · focus ring on open"], ["dropdown", "menu container + MenuItem rows · popover lift"], ["row height", <>28px <span className="tok">--tasty-control-height</span></>], ["path row", "middle-ellipsis (filename tail kept)"], ["filter", "substring (default) · prefix · none"], ["row states", "hover (overlay-hover) · keyboard-active (surface-active, wins)"], ["overflow", <>scroll past <span className="tok">maxDropdownHeight</span> · <code>.tasty-scroll</code></>], ["empty", "one muted no-match row"]]}
            tokens={[{ tok: "--tasty-input-bg", use: "trigger fill", color: "var(--tasty-input-bg)" }, { tok: "--tasty-input-border-focus", use: "trigger border (open)", color: "var(--tasty-input-border-focus)" }, { tok: "--tasty-autocomplete-menu-bg", use: "dropdown fill", color: "var(--tasty-autocomplete-menu-bg)" }, { tok: "--tasty-autocomplete-menu-border", use: "dropdown edge", color: "var(--tasty-autocomplete-menu-border)" }, { tok: "--tasty-autocomplete-row-bg-hover", use: "pointer hover", color: "var(--tasty-autocomplete-row-bg-hover)" }, { tok: "--tasty-autocomplete-row-bg-active", use: "keyboard-active", color: "var(--tasty-autocomplete-row-bg-active)" }, { tok: "--tasty-autocomplete-match-fg", use: "match highlight", color: "var(--tasty-autocomplete-match-fg)" }, { tok: "--tasty-autocomplete-max-height", use: "default cap (220)" }, { tok: "--tasty-autocomplete-empty-fg", use: "no-match row", color: "var(--tasty-autocomplete-empty-fg)" }]} />
          <Note>Replaces the source-only <code>Combobox</code> widget (<code>crates/tasty-ui-widgets/src/combobox.rs</code>), which implied a <b>closed</b> selection it never was — its one real job is free-text + suggestions = <b>autocomplete</b>. <b>Match:</b> substring is the recommended default (paths match anywhere, not just the prefix); the matched run is highlighted — v1 source list was unfiltered, this design formalizes typeahead. <b>Scrollbar:</b> reuses the shared <code>.tasty-scroll</code> (neutral-400 thumb) — <b>no new token</b>. This is the candidate dropdown behind the shared <b>PathField</b> (see Plugins → Explorer): Explorer injects <span className="ic">folderOpen</span> + recent directories, Markdown injects <span className="ic">file</span> + recent files; the component emits only the confirmed path string. Source home for the extracted widget: rename <code>Combobox</code> → <code>AutoComplete</code>, compose it under <code>PathField</code>, wire both call sites — tracked as a separate implementation TODO.</Note>
        </Spec>
      </Section>

      {/* NAV */}
      <Section id="nav" title="Tab · TreeRow · MenuItem">
        <Spec title="Tab"
          when={<>One tab in the pane tab strip. <b>24px tall × 150px wide</b>, accent bar on the active tab, hover-revealed close, and a <b>busy dot</b> mirroring the product surface model (busy=green, idle=no dot). <code>attached</code> adds the lavender "claimed by another client" ring; <code>notif</code> tints the label. The 5-color owner×activity vocabulary lives on the workspace StatusDot, not on tabs — Tasty has no "unsaved" state.</>}>
          <Stage variant="tight">
            <div style={{ display: "flex", background: "var(--tasty-bg-sidebar)", borderBottom: "1px solid var(--tasty-separator)" }}>
              <Tab label="build.sh" icon={ic.term} active status="busy" />
              <Tab label="README.md" icon={ic.md} status="idle" attached />
              <Tab label="server.log" icon={ic.term} notif />
            </div>
          </Stage>
          <Meta
            specs={[["height", <>24px <span className="tok">--tasty-control-height-tab</span></>], ["width", <>150px <span className="tok">--tasty-tab-width</span></>], ["active", "accent top bar + panel fill"], ["close", "hover-revealed"]]}
            tokens={[{ tok: "--tasty-bg-panel", use: "active fill", color: "var(--tasty-bg-panel)" }, { tok: "--tasty-accent-primary", use: "active bar", color: "var(--tasty-accent-primary)" }, { tok: "--tasty-separator", use: "dividers" }]} />
        </Spec>

        <Spec title="TreeRow"
          when={<>A <b>22px</b> row for sidebars and file trees — indent per level (14px), disclosure chevron for folders, leading kind icon, label, trailing meta. The selected row gets <code>--tasty-surface-active</code>.</>}>
          <Stage variant="tight">
            <div style={{ background: "var(--tasty-bg-sidebar)", padding: "6px 4px", width: 320 }}>
              <TreeRow label="tasty" icon={ic.folder} expandable open level={0} meta="34" />
              <TreeRow label="crates" icon={ic.folder} expandable level={1} meta="39" />
              <TreeRow label="Cargo.toml" icon={ic.file} level={1} selected />
              <TreeRow label="README.md" icon={ic.file} level={1} meta="4.7k" />
            </div>
          </Stage>
          <Meta
            specs={[["height", <>22px <span className="tok">--tasty-control-height-tree</span></>], ["indent", "14px / level"], ["selected", <span className="tok">--tasty-surface-active</span>]]}
            tokens={[{ tok: "--tasty-surface-active", use: "selected", color: "var(--tasty-surface-active)" }, { tok: "--tasty-overlay-hover", use: "hover" }, { tok: "--tasty-text-muted", use: "meta + icon", color: "var(--tasty-text-muted)" }]} />
        </Spec>

        <Spec title="MenuItem"
          when={<>A <b>28px</b> row for context menus, the command palette, and tools menus — icon + label + trailing shortcut. <b>active</b> = keyboard-highlighted; <b>danger</b> for destructive; <code>separator</code> draws a thin rule.</>}>
          <Stage variant="tight">
            <div style={{ background: "var(--tasty-surface-raised)", border: "1px solid var(--tasty-border-default)", borderRadius: "var(--tasty-radius)", padding: 6, width: 280 }}>
              <MenuItem label="New tab" icon={ic.term} shortcut={<Kbd keys="Ctrl+T" />} active />
              <MenuItem label="Split pane" icon={ic.split} shortcut={<Kbd keys="Ctrl+D" />} />
              <MenuItem label="Copy path" icon={ic.copy} />
              <MenuItem separator />
              <MenuItem label="Move to Trash" icon={ic.trash} danger />
            </div>
          </Stage>
          <Meta
            specs={[["height", <>28px <span className="tok">--tasty-control-height</span></>], ["active", <span className="tok">--tasty-surface-active</span>], ["danger", <span className="tok">--tasty-accent-danger</span>]]}
            tokens={[{ tok: "--tasty-surface-active", use: "highlighted", color: "var(--tasty-surface-active)" }, { tok: "--tasty-accent-danger", use: "destructive", color: "var(--tasty-accent-danger)" }, { tok: "--tasty-text-muted", use: "shortcut", color: "var(--tasty-text-muted)" }]} />
        </Spec>
      </Section>

      {/* FEEDBACK */}
      <Section id="feedback" title="StatusDot · Spinner · Toast">
        <Spec title="StatusDot"
          when={<>Small state indicator for surfaces, panes, plugins. <b>agent</b> (mauve, pulsing) marks a live agent surface — the single most important signal in the product. <code>pulse</code> for live/running. <b>needs-input</b> and <b>completion</b> are <b>attention kinds</b>, not execution states — they share this one dot and win it by rank (needs-input › completion › running/agent/waiting › idle).</>}>
          <Stage style={{ gap: 24 }}>
            <Cluster label="states"><StatusDot status="running" pulse label="running" /><StatusDot status="agent" pulse label="agent" /><StatusDot status="waiting" label="waiting" /><StatusDot status="idle" label="idle" /><StatusDot status="error" label="error" /></Cluster>
            <Cluster label="attention kinds"><StatusDot status="needs-input" label="needs input" /><StatusDot status="completion" label="completion" /></Cluster>
          </Stage>
          <Meta
            specs={[["dot", "7px"], ["pulse", "expanding ring"], ["agent", <span className="tok">--tasty-accent-agent</span>], ["attention rank", "needs-input › completion › activity"]]}
            tokens={[{ tok: "--tasty-accent-success", use: "running", color: "var(--tasty-accent-success)" }, { tok: "--tasty-accent-agent", use: "agent", color: "var(--tasty-accent-agent)" }, { tok: "--tasty-accent-warning", use: "waiting", color: "var(--tasty-accent-warning)" }, { tok: "--tasty-accent-danger", use: "error", color: "var(--tasty-accent-danger)" }, { tok: "--tasty-status-dot-needs-input", use: "needs input", color: "var(--tasty-status-dot-needs-input)" }, { tok: "--tasty-status-dot-completion", use: "completion", color: "var(--tasty-status-dot-completion)" }]} />
        </Spec>

        <Spec title="Status resolution — always exactly one dot"
          when={<>A surface carries <b>two independent facts</b>: who <b>owns</b> it (user / agent) and what it's <b>doing</b> (running / waiting / error / idle). These never render as two dots — they collapse into <b>one</b> by a fixed priority. Live activity outranks ownership, so <b>agent + running shows running (green)</b>; the mauve <b>agent</b> dot appears only when the surface is otherwise idle.</>}>
          <Stage variant="solo column" style={{ gap: 0, padding: "4px 26px 10px" }}>
            <div style={{ display: "flex", gap: 10, padding: "8px 0", borderBottom: "1px solid var(--tasty-separator)",
              fontFamily: "var(--tasty-font-mono)", fontSize: 10, textTransform: "uppercase", letterSpacing: ".05em", color: "var(--tasty-text-muted)" }}>
              <span style={{ width: 92 }}>owner</span><span style={{ width: 92 }}>activity</span><span style={{ width: 20, textAlign: "center" }}>=</span><span>resolved dot</span>
            </div>
            {[["agent", "running", "running"], ["agent", "idle", "agent"], ["agent", "waiting", "waiting"], ["user", "running", "running"], ["user", "idle", "idle"], ["agent", "error", "error"]].map(([owner, activity, res]) => (
              <div key={owner + activity} style={{ display: "flex", alignItems: "center", gap: 10, padding: "9px 0", borderBottom: "1px solid var(--tasty-separator)" }}>
                <span style={{ width: 92, fontFamily: "var(--tasty-font-mono)", fontSize: 12, color: owner === "agent" ? "var(--tasty-accent-agent)" : "var(--tasty-text-secondary)" }}>{owner}</span>
                <span style={{ width: 92, fontFamily: "var(--tasty-font-mono)", fontSize: 12, color: "var(--tasty-text-secondary)" }}>{activity}</span>
                <span style={{ width: 20, textAlign: "center", color: "var(--tasty-text-muted)" }}>→</span>
                <StatusDot status={res} pulse={res === "running" || res === "agent" || res === "waiting"} label={res} />
              </div>
            ))}
          </Stage>
          <Meta
            specs={[["priority", "error › waiting › running › agent › idle"], ["rule", "live activity beats ownership"], ["dots shown", "always exactly 1"], ["helper", <span className="ic">resolveStatus(owner, activity)</span>]]}
            tokens={[{ tok: "--tasty-accent-success", use: "running (wins)", color: "var(--tasty-accent-success)" }, { tok: "--tasty-accent-agent", use: "agent (idle only)", color: "var(--tasty-accent-agent)" }, { tok: "--tasty-accent-danger", use: "error (top)", color: "var(--tasty-accent-danger)" }]} />
          <Dont><b>Don't</b> stack an ownership dot next to an activity dot. One surface = one dot. If you think you need two, you're encoding two facts that the priority order already resolves into one.</Dont>
        </Spec>

        <Spec title="Spinner"
          when={<>Indeterminate progress for <b>short background work</b> — a port scan, plugin install, surface attach, remote shell detection. A thin rotating arc on a faint track, inheriting the current text color so it sits inline in muted contexts or recolors on an accent button. <b>Not</b> for determinate progress (use a bar) or long waits. Honors <code>prefers-reduced-motion</code> — falls back to three static dots.</>}>
          <Stage style={{ gap: 28 }}>
            <Cluster label="sizes — 12 · 16 · 20 · 24">
              <Spinner size={12} /><Spinner size={16} /><Spinner size={20} /><Spinner size={24} />
            </Cluster>
            <Cluster label="inline with text">
              <span style={{ display: "inline-flex", alignItems: "center", gap: 8, fontSize: 13, color: "var(--tasty-text-muted)" }}>
                <Spinner size={14} /> Collecting…
              </span>
            </Cluster>
            <Cluster label="in a button · detecting row">
              <Button variant="secondary" disabled leadingIcon={<Spinner size={14} />}>Installing…</Button>
              <span style={{ display: "inline-flex", alignItems: "center", gap: 6, fontFamily: "var(--tasty-font-mono)", fontSize: 11, color: "var(--tasty-text-muted)" }}>
                <Spinner size={12} /> detecting shell…
              </span>
            </Cluster>
          </Stage>
          <Meta
            specs={[["sizes", "12 / 16 / 20 / 24px"], ["stroke", "2px arc + faint track"], ["spin", "0.9s linear"], ["color", <span className="tok">currentColor</span>], ["reduced motion", "→ 3 static dots"]]}
            tokens={[{ tok: "--tasty-text-muted", use: "default color", color: "var(--tasty-text-muted)" }, { tok: "--tasty-spinner-size", use: "diameter" }, { tok: "--tasty-size-16", use: "16px default" }]} />
          <Note>It inherits <span className="ic">color</span> — drop it on a filled button and it turns <span className="ic">--tasty-text-on-accent</span> automatically. Used by the Port Scanner scan state and the Remote tool’s shell-detection row.</Note>
        </Spec>

        <Spec title="Toast"
          when={<>Transient notification card with an accent rail by intent — <code>Copied</code>, <code>Path copied</code>, <code>Force detach</code>. Terse, second person. Use <b>agent</b> for agent-originated notices.</>}>
          <Stage variant="column" style={{ gap: 10, alignItems: "stretch", maxWidth: 380 }}>
            <Toast variant="success" hint={<Kbd keys="⌘C" />}>Path copied to clipboard</Toast>
            <Toast variant="agent">Agent opened 3 surfaces in background</Toast>
            <Toast variant="warning">Held by another client (readonly)</Toast>
            <Toast variant="danger">Force detach — connection dropped</Toast>
          </Stage>
          <Meta
            specs={[["rail", "3px accent left edge"], ["radius", <span className="tok">--tasty-radius</span>], ["fill", <span className="tok">--tasty-surface-raised</span>]]}
            tokens={[{ tok: "--tasty-accent-success", use: "ok rail", color: "var(--tasty-accent-success)" }, { tok: "--tasty-accent-agent", use: "agent rail", color: "var(--tasty-accent-agent)" }, { tok: "--tasty-surface-raised", use: "card", color: "var(--tasty-surface-raised)" }]} />
        </Spec>

        <Spec title="Toast stack"
          when={<>When several notices fire close together they <b>stack</b> rather than replace — newest on top, anchored to one corner, each keeping its own intent rail. Cap the visible count (older ones collapse into a “+N more” row) so the stack never walks off-screen. Same card as a single Toast; only the layout (vertical gap, z-order) is new.</>}>
          <Stage variant="solo center" style={{ padding: 24, background: "var(--tasty-bg-app)" }}>
            <div style={{ width: 320, display: "flex", flexDirection: "column", gap: 8 }}>
              <Toast variant="agent">Agent opened 3 surfaces in background</Toast>
              <Toast variant="success" hint={<Kbd keys="⌘C" />}>Path copied to clipboard</Toast>
              <Toast variant="warning">Held by another client (readonly)</Toast>
              <div style={{ display: "flex", alignItems: "center", justifyContent: "center", height: 22,
                fontFamily: "var(--tasty-font-mono)", fontSize: 11, color: "var(--tasty-text-muted)" }}>+2 more</div>
            </div>
          </Stage>
          <Meta
            specs={[["anchor", "one corner (bottom-right)"], ["order", "newest on top"], ["gap", <>8px <span className="tok">--tasty-space-sm</span></>], ["cap", "N visible → “+N more”"], ["width", "~320–380px"]]}
            tokens={[{ tok: "--tasty-space-sm", use: "stack gap" }, { tok: "--tasty-surface-raised", use: "each card", color: "var(--tasty-surface-raised)" }, { tok: "--tasty-text-muted", use: "overflow row", color: "var(--tasty-text-muted)" }]} />
          <Dont><b>Don’t</b> let the stack grow unbounded. Cap it and collapse the tail — a wall of toasts buries the newest signal, which is the one that matters.</Dont>
        </Spec>
      </Section>

      {/* HELPHINT / TOOLTIP */}
      <Section id="helphint" title="HelpHint · Tooltip">
        <Spec title="HelpHint"
          when={<>An inline circular <code>?</code> after a label that explains it on <b>hover</b> — how we move a below-control description up beside its label so a settings row stays a single line. Muted at rest, brightens on hover / focus; anchors a <span className="ic">Tooltip</span>. Hover-only, no click; cursor is <code>help</code>.</>}>
          <Stage style={{ gap: 30, alignItems: "flex-start", paddingTop: 34 }}>
            <Cluster label="in a settings row — hover the (?)">
              <div style={{ display: "flex", alignItems: "center", gap: 16 }}>
                <span style={{ width: 150, flex: "none", display: "inline-flex", alignItems: "center", gap: "var(--tasty-help-hint-gap)", fontSize: 13, color: "var(--tasty-text-secondary)" }}>Scrollback disk swap: <HelpHint label="Spill scrollback past the in-memory limit to a temp file instead of dropping the oldest lines." placement="bottom" /></span>
                <Switch defaultChecked />
              </div>
              <div style={{ display: "flex", alignItems: "center", gap: 16, marginTop: 8 }}>
                <span style={{ width: 150, flex: "none", display: "inline-flex", alignItems: "center", gap: "var(--tasty-help-hint-gap)", fontSize: 13, color: "var(--tasty-text-secondary)" }}>PTY polling (ms): <HelpHint label="How often the PTY read loop is polled. Lower = snappier output at higher CPU cost. Default 8ms." placement="bottom" /></span>
                <Input mono defaultValue="8" style={{ width: "var(--tasty-field-width-xs)" }} />
              </div>
            </Cluster>
            <Cluster label="rest / hover">
              <div style={{ display: "flex", alignItems: "center", gap: 22, color: "var(--tasty-text-secondary)", fontSize: 13 }}>
                <span style={{ display: "inline-flex", alignItems: "center", gap: 6 }}>rest <HelpHint /></span>
                <span style={{ display: "inline-flex", alignItems: "center", gap: 6, color: "var(--tasty-text-secondary)" }}>hover <span style={{ color: "var(--tasty-help-hint-color-hover)", display: "inline-flex" }}><GIcon d={<><circle cx="12" cy="12" r="10" /><path d="M9.09 9a3 3 0 0 1 5.83 1c0 2-3 3-3 3" /><path d="M12 17h.01" /></>} size={14} /></span></span>
              </div>
            </Cluster>
          </Stage>
          <Meta
            specs={[
              ["glyph", <>help-circle · <span className="tok">--tasty-help-hint-size</span> 14</>],
              ["rest", <span className="tok">--tasty-help-hint-color</span>],
              ["hover", <span className="tok">--tasty-help-hint-color-hover</span>],
              ["gap", <>4px <span className="tok">--tasty-help-hint-gap</span></>],
              ["cursor", "help — no click"],
            ]}
            tokens={[
              { tok: "--tasty-help-hint-color", use: "rest glyph", color: "var(--tasty-help-hint-color)" },
              { tok: "--tasty-help-hint-color-hover", use: "hover glyph", color: "var(--tasty-help-hint-color-hover)" },
              { tok: "--tasty-help-hint-gap", use: "label → (?)" },
            ]} />
          <Note>Replaces the below-control caption on dense pages (<b>Settings › Terminal › Performance</b>). The description copy is unchanged — it just moves into the bubble. Don't stack a HelpHint <i>and</i> a caption line for the same control.</Note>
        </Spec>

        <Spec title="Tooltip"
          when={<>The hover bubble itself — opaque raised card, 1px edge, popover lift, <b>no arrow</b> (calm, low-chrome). Wraps any anchor; shows on hover or keyboard focus after a short delay. Reusable beyond HelpHint. Shown here forced-open via <code>open</code>.</>}>
          <Stage style={{ gap: 56, alignItems: "center", justifyContent: "center", paddingTop: 46, paddingBottom: 46, minHeight: 150 }}>
            {["top", "bottom", "left", "right"].map((p) => (
              <Tooltip key={p} open placement={p} content={p === "left" || p === "right" ? "Terse hint." : "A short hint that wraps to two or three lines when the copy is longer."}>
                <Tag>{p}</Tag>
              </Tooltip>
            ))}
          </Stage>
          <Meta
            specs={[
              ["bg", <span className="tok">--tasty-tooltip-bg</span>],
              ["border", <>1px <span className="tok">--tasty-tooltip-border</span></>],
              ["radius", <span className="tok">--tasty-tooltip-radius</span>],
              ["max-width", <>240 <span className="tok">--tasty-tooltip-max-width</span></>],
              ["placement", "top / bottom / left / right"],
              ["delay", <>150ms <span className="tok">--tasty-tooltip-delay</span></>],
            ]}
            tokens={[
              { tok: "--tasty-tooltip-bg", use: "bubble fill", color: "var(--tasty-tooltip-bg)" },
              { tok: "--tasty-tooltip-border", use: "1px edge", color: "var(--tasty-tooltip-border)" },
              { tok: "--tasty-tooltip-fg", use: "copy", color: "var(--tasty-tooltip-fg)" },
              { tok: "--tasty-tooltip-shadow", use: "lift" },
            ]} />
          <Dont><b>Don't</b> put actions, links, or long paragraphs in a tooltip — it's a hover hint, not a popover. If it needs a click target, that's a MenuItem popup.</Dont>
        </Spec>
      </Section>

      {/* TEXT */}
      <Section id="text" title="Hint text">
        <Spec title="Hint text"
          when={<>The system’s <b>secondary helper line</b> — a sub-label under a field, a one-line keyboard hint in a dialog, the explanatory caption beneath a setting. Always <b>--tasty-text-muted</b>, 11–12px, sentence case, terse. It explains <i>without</i> competing with the primary label. Inline <code>Kbd</code> and <span className="ic">mono</span> spans are allowed; full sentences are not the goal.</>}>
          <Stage variant="column" style={{ gap: 16, alignItems: "stretch", maxWidth: 420 }}>
            <div style={{ display: "flex", flexDirection: "column", gap: 4 }}>
              <span style={{ fontSize: 13, color: "var(--tasty-text-primary)" }}>Remote tasty path</span>
              <Input mono placeholder="/usr/local/bin/tasty" style={{ width: "100%" }} />
              <span style={{ fontSize: 11, color: "var(--tasty-text-muted)", lineHeight: 1.5 }}>Leave empty to auto-detect on first connect.</span>
            </div>
            <div style={{ display: "flex", flexDirection: "column", gap: 4 }}>
              <span style={{ fontSize: 13, color: "var(--tasty-text-primary)" }}>Reduced motion</span>
              <span style={{ fontSize: 11, color: "var(--tasty-text-muted)", lineHeight: 1.5 }}>Disables the terminal cursor blink and spinner animation.</span>
            </div>
            <span style={{ fontSize: 12, color: "var(--tasty-text-muted)" }}>Press <Kbd keys="↵" /> to confirm, <Kbd keys="Esc" /> to cancel.</span>
          </Stage>
          <Meta
            specs={[["size", "11–12px"], ["color", <span className="tok">--tasty-text-muted</span>], ["case", "sentence case"], ["line-height", "1.5"], ["placement", "below the thing it explains"]]}
            tokens={[{ tok: "--tasty-text-muted", use: "hint color", color: "var(--tasty-text-muted)" }, { tok: "--tasty-font-size-caption", use: "11px" }, { tok: "--tasty-font-mono", use: "inline code" }]} />
          <Note>This is the same muted treatment used in the palette footer (<span className="ic">↑↓ navigate</span>), the Rename dialog hint, and every form sub-label — one consistent voice for “the quiet line.”</Note>
        </Spec>
      </Section>

      {/* DATA */}
      <Section id="data" title="Table">
        <Spec title="Table"
          when={<>Column-config data table for <b>system data</b> — listening ports, processes, sessions, keybindings. Data cells default to <b>--tasty-font-mono</b> so ports/PIDs/addresses align by glyph. Headers are <b>sortable</b> and <b>sticky</b> (wrap in a scroll container); rows hover and select. Right-align numeric columns.</>}>
          <Stage variant="solo" style={{ padding: 0, overflow: "hidden" }}>
            <div style={{ width: "100%", maxHeight: 280, overflow: "auto" }}>
              <PortsTableDemo />
            </div>
          </Stage>
          <Meta
            specs={[["row height", <>28px <span className="tok">--tasty-control-height</span> (dense 22)</>], ["header", <>sticky, <span className="tok">--tasty-bg-sidebar</span>, mono caption</>], ["cell font", <>mono columns → <span className="tok">--tasty-font-mono</span></>], ["selected", <span className="tok">--tasty-surface-active</span>], ["hairline", <span className="tok">--tasty-separator</span>]]}
            tokens={[
              { tok: "--tasty-bg-sidebar", use: "header row", color: "var(--tasty-bg-sidebar)" },
              { tok: "--tasty-surface-active", use: "selected row", color: "var(--tasty-surface-active)" },
              { tok: "--tasty-overlay-hover", use: "row hover" },
              { tok: "--tasty-accent-primary", use: "sort arrow", color: "var(--tasty-accent-primary)" },
              { tok: "--tasty-font-mono", use: "port/addr/pid cells" },
            ]} />
          <Note>Columns can be marked <span className="ic">tight</span> — zero horizontal padding + centered — for narrow icon/control columns (the Listening-ports star toggle sits in a tight 28px leading column).</Note>
          <Note>This is what the <b>Tools › Listening ports</b> popup is built from. Use <span className="ic">render(value, row)</span> to compose cells — e.g. a <span className="ic">StatusDot</span> for the state column or a <span className="ic">Tag</span> for the PID.</Note>
        </Spec>
      </Section>

      {/* LISTCTRL */}
      <Section id="listctrl" title="ListCtrl">
        <Spec title="ListCtrl — navigation list"
          when={<>A full-width, row-selectable <b>navigation</b> list — "pick one to <b>drill into</b>", not a data grid (that's <span className="ic">Table</span>). Each row is a <b>primary label</b> with an optional <b>description</b>, leading icon, a <b>trailing slot</b> (a Tag/Badge — e.g. <span className="ic">Active</span>), and a <b>drill-in chevron</b>. Rows share the list-idiom states: hover → <b>--tasty-overlay-hover</b>, selected → <b>--tasty-surface-active</b> + a 2px accent left bar, divided by the separator hairline. Pair it with the <b>Drill-down</b> layout (see Layouts) for the list → detail swap — this is the new <b>Settings › Keybindings › Preset</b> list.</>}>
          <Stage variant="solo" style={{ padding: 0, overflow: "hidden" }}>
            <div style={{ width: "100%" }}><ListCtrlDemo /></div>
          </Stage>
          <Meta
            specs={[["row", <>label · [description] · [trailing] · chevron</>], ["min height", <>36px <span className="tok">--tasty-listctrl-row-min-height</span></>], ["hover", <span className="tok">--tasty-overlay-hover</span>], ["selected", <><span className="tok">--tasty-surface-active</span> + 2px accent bar</>], ["chevron", "trailing drill-in affordance (default on)"], ["divider", <><span className="tok">--tasty-separator</span> hairline between rows</>], ["disabled", <>dimmed, no chevron, non-selectable</>]]}
            tokens={[
              { tok: "--tasty-listctrl-row-bg-hover", use: "row hover", color: "var(--tasty-overlay-hover)" },
              { tok: "--tasty-listctrl-row-bg-selected", use: "selected row", color: "var(--tasty-surface-active)" },
              { tok: "--tasty-listctrl-selected-bar", use: "accent left bar", color: "var(--tasty-accent-primary)" },
              { tok: "--tasty-listctrl-label-fg", use: "label", color: "var(--tasty-text-secondary)" },
              { tok: "--tasty-listctrl-desc-fg", use: "description", color: "var(--tasty-text-muted)" },
              { tok: "--tasty-listctrl-chevron-fg", use: "drill-in chevron", color: "var(--tasty-text-muted)" },
            ]} />
          <Do><b>Do</b> reach for <span className="ic">ListCtrl</span> when a row leads somewhere (drill-down, apply). <b>Don't</b> use it for tabular data with multiple comparable columns — that's <span className="ic">Table</span>. The <b>trailing slot</b> is for one status marker (Active / a count), not a second data column.</Do>
        </Spec>
      </Section>
    </>
  );
}

function ListCtrlDemo() {
  const [sel, setSel] = React.useState("default");
  const items = [
    { id: "default", label: "Default", description: "Tasty stock bindings", trailing: <Tag variant="success" dot>Active</Tag> },
    { id: "mac", label: "Mac", description: "⌘-based, native-app muscle memory" },
    { id: "emacs", label: "Emacs", description: "C-x / C-c prefix chords" },
    { id: "vim", label: "Vim", description: "modal, hjkl pane motions" },
    { id: "readline", label: "Readline", description: "GNU readline defaults", disabled: true },
  ];
  return <ListCtrl items={items} selectedId={sel} onSelect={(id) => setSel(id)} />;
}

function PortsTableDemo() {
  const [sort, setSort] = React.useState({ key: "port", dir: "asc" });
  const [sel, setSel] = React.useState(8080);
  const DATA = [
    { port: 22, proto: "tcp", addr: "0.0.0.0", proc: "sshd", pid: 712, state: "LISTEN" },
    { port: 3000, proto: "tcp", addr: "127.0.0.1", proc: "node", pid: 48213, state: "LISTEN" },
    { port: 5432, proto: "tcp", addr: "127.0.0.1", proc: "postgres", pid: 1192, state: "LISTEN" },
    { port: 8080, proto: "tcp", addr: "0.0.0.0", proc: "tasty-agent", pid: 50321, state: "LISTEN" },
    { port: 9229, proto: "tcp", addr: "127.0.0.1", proc: "node", pid: 48213, state: "CLOSE_WAIT" },
  ];
  const onSort = (key) => setSort((s) => ({ key, dir: s.key === key && s.dir === "asc" ? "desc" : "asc" }));
  const rows = [...DATA].sort((a, b) => {
    const av = a[sort.key], bv = b[sort.key];
    const c = typeof av === "number" ? av - bv : String(av).localeCompare(String(bv));
    return sort.dir === "asc" ? c : -c;
  });
  const columns = [
    { key: "port", header: "Port", align: "right", mono: true, sortable: true, width: "84px" },
    { key: "proto", header: "Proto", mono: true, width: "76px" },
    { key: "addr", header: "Address", mono: true, sortable: true },
    { key: "proc", header: "Process", strong: true, sortable: true,
      render: (v, row) => (<span style={{ display: "inline-flex", alignItems: "center", gap: 8 }}>{v}<Tag>{row.pid}</Tag></span>) },
    { key: "state", header: "State", width: "140px",
      render: (v) => <StatusDot status={v === "LISTEN" ? "running" : "waiting"} pulse={v === "LISTEN"} label={v} /> },
  ];
  return <Table columns={columns} rows={rows} rowKey="port" sort={sort} onSort={onSort}
    selectedKey={sel} onRowClick={(row) => setSel((k) => (k === row.port ? null : row.port))} />;
}

function HoverBadge({ label = "live" }) {
  return (
    <span style={{ fontFamily: "var(--tasty-font-mono)", fontSize: 10, color: "var(--tasty-accent-primary)",
      border: "1px solid var(--tasty-accent-primary)", borderRadius: "var(--tasty-radius-sm)", padding: "1px 6px", fontWeight: 400 }}>
      {label}
    </span>
  );
}

window.Gallery.mount(
  "components",
  NAV,
  {
    title: "Components",
    intro: "The reusable primitives, live. Hover and focus respond for real — that's how you read the interaction states. Each specimen states when to reach for it, its fixed dimensions, and the exact tokens it's built from.",
    howto: false,
  },
  <Components />
);
