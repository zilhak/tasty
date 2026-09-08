// Tasty Gallery — Plugin surfaces. Surface kinds contributed by bundled plugins
// that fill a work-area tile: the Explorer file manager and the content viewers
// (Markdown / HTML / Image …). Kept on their OWN page because plugin surfaces will
// grow without bound — they must not crowd the general structural Layouts.
const { Section, Spec, Stage, Meta, Note, Do, Dont, GIcon } = window.Gallery;
const { IconButton, Button, Input, AutoComplete, MenuItem, Switch, Kbd, Spinner, Checkbox, Select, Tag } = window.TastyDesignSystem_41fd3f;

const NAV = [
  { id: "explorer", label: "Explorer (file manager)" },
  { id: "markdown", label: "Markdown viewer" },
  { id: "html", label: "HTML viewer" },
  { id: "image", label: "Image viewer" },
];

const ic = {
  folder: <GIcon d={<path d="M4 20h16a1 1 0 0 0 1-1V8a1 1 0 0 0-1-1h-7l-2-2H4a1 1 0 0 0-1 1v13a1 1 0 0 0 1 1z" />} />,
  file: <GIcon d={<><path d="M14 3v4a1 1 0 0 0 1 1h4" /><path d="M17 21H7a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h7l5 5v11a2 2 0 0 1-2 2z" /></>} />,
  x: <GIcon d={<path d="M18 6 6 18M6 6l12 12" />} size={12} />,
  plus: <GIcon d={<path d="M12 5v14M5 12h14" />} />,
  back: <GIcon d={<path d="m15 18-6-6 6-6" />} />,
  fwd: <GIcon d={<path d="m9 18 6-6-6-6" />} />,
  up: <GIcon d={<path d="M12 19V5M5 12l7-7 7 7" />} />,
  refresh: <GIcon d={<path d="M21 12a9 9 0 1 1-2.6-6.4M21 3v6h-6" />} />,
  grid: <GIcon d={<><rect x="3" y="3" width="7" height="7" rx="1" /><rect x="14" y="3" width="7" height="7" rx="1" /><rect x="3" y="14" width="7" height="7" rx="1" /><rect x="14" y="14" width="7" height="7" rx="1" /></>} />,
  list: <GIcon d={<path d="M8 6h13M8 12h13M8 18h13M3 6h.01M3 12h.01M3 18h.01" />} />,
  detail: <GIcon d={<path d="M3 5h18M3 12h18M3 19h18M3 5v14" />} />,
  star: <GIcon d={<path d="m12 3 2.9 5.9 6.5.9-4.7 4.6 1.1 6.5L12 18l-5.8 3 1.1-6.5L2.6 9.8l6.5-.9z" />} />,
  starFill: <GIcon fill="currentColor" d={<path d="m12 3 2.9 5.9 6.5.9-4.7 4.6 1.1 6.5L12 18l-5.8 3 1.1-6.5L2.6 9.8l6.5-.9z" />} />,
  image: <GIcon d={<><rect x="3" y="3" width="18" height="18" rx="2" /><circle cx="9" cy="9" r="2" /><path d="m21 15-5-5L5 21" /></>} />,
  chevD: <GIcon d={<path d="m6 9 6 6 6-6" />} />,
  chevRsm: <GIcon d={<path d="m9 18 6-6-6-6" />} />,
  copy: <GIcon d={<><rect x="9" y="9" width="11" height="11" rx="2" /><path d="M5 15V5a2 2 0 0 1 2-2h8" /></>} />,
  scissors: <GIcon d={<><circle cx="6" cy="6" r="3" /><circle cx="6" cy="18" r="3" /><path d="M20 4 8.1 15.9M14.5 12.5 20 20M8.1 8.1 12 12" /></>} />,
  paste: <GIcon d={<><path d="M16 4h2a2 2 0 0 1 2 2v14a2 2 0 0 1-2 2H6a2 2 0 0 1-2-2V6a2 2 0 0 1 2-2h2" /><rect x="8" y="2" width="8" height="4" rx="1" /></>} />,
  trash: <GIcon d={<path d="M3 6h18M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6m3 0V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2" />} />,
  rename: <GIcon d={<path d="M12 20h9M16.5 3.5a2.12 2.12 0 0 1 3 3L7 19l-4 1 1-4z" />} />,
  external: <GIcon d={<path d="M15 3h6v6M10 14 21 3M18 13v6a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V8a2 2 0 0 1 2-2h6" />} />,
  link: <GIcon d={<path d="M10 13a5 5 0 0 0 7 0l3-3a5 5 0 0 0-7-7l-1.5 1.5M14 11a5 5 0 0 0-7 0l-3 3a5 5 0 0 0 7 7l1.5-1.5" />} />,
  lock: <GIcon d={<><rect x="5" y="11" width="14" height="10" rx="2" /><path d="M8 11V7a4 4 0 0 1 8 0v4" /></>} />,
  folderOpen: <GIcon d={<path d="M3 8a1 1 0 0 1 1-1h5l2 2h7a1 1 0 0 1 1 1v1H3z M3 11h18l-1.5 8a1 1 0 0 1-1 1H5.5a1 1 0 0 1-1-1z" />} />,
  globe: <GIcon d={<><circle cx="12" cy="12" r="9" /><path d="M3 12h18M12 3a15 15 0 0 1 0 18M12 3a15 15 0 0 0 0 18" /></>} />,
  alert: <GIcon d={<><circle cx="12" cy="12" r="9" /><path d="M12 8v5M12 16h.01" /></>} />,
};

// Recent-path candidate lists for the shared PathField's AutoComplete dropdown.
const EXP_RECENT = ["~/Downloads", "~/work/tasty", "~/work/tasty-ui/src", "~/work/tasty/crates/tasty-ui-widgets", "~/.config/tasty"];
const MD_RECENT = ["~/work/tasty/README.md", "~/work/tasty/docs/architecture.md", "~/work/tasty/docs/design/systems/theme.md", "~/work/tasty/CHANGELOG.md"];

// ════════════════════════════════════════════════════════════
//  SHARED — editable path field (Explorer + Markdown address bars)
// ════════════════════════════════════════════════════════════
// One component both surfaces share. Display: mono path (ellipsis, secondary)
// behind a per-surface leading icon, trailing Go (arrow-right). Click → edit
// (focus ring + caret, text-primary); ↵/Go navigate, Esc reverts. Field tokens
// align to forms/Input (--tasty-input-*). The ONLY per-surface parameter is the
// leading `icon` (Explorer=folderOpen, Markdown=file); the outer bar is not part
// of this component. Real-side home: crates/tasty-ui-widgets (Input + IconPainter).
const goGlyph = <GIcon d={<path d="M5 12h14M13 6l6 6-6 6" />} />;
function PathField({ icon, path, editing = false, candidates = null, open = false, activeIndex = 0, query, maxDropdownHeight = 220 }) {
  // Editing WITH candidates → the shared AutoComplete dropdown (typeahead of
  // recent paths). Same trigger tokens as the idle field below; Explorer feeds
  // recent directories, Markdown recent files. The component emits the path.
  if (editing && candidates) {
    return (
      <AutoComplete block mono withGo icon={icon} rowIcon={icon}
        open={open} activeIndex={activeIndex} query={query != null ? query : path}
        items={candidates} maxDropdownHeight={maxDropdownHeight} emptyLabel="No matching path" />
    );
  }
  return (
    <div style={{ flex: 1, minWidth: 0, display: "flex", alignItems: "center", gap: 6 }}>
      <div style={{ flex: 1, minWidth: 0, display: "flex", alignItems: "center", gap: 6, height: 28, padding: "0 8px",
        background: "var(--tasty-input-bg)", borderRadius: "var(--tasty-radius)",
        border: "1px solid " + (editing ? "var(--tasty-input-border-focus)" : "var(--tasty-input-border)"),
        boxShadow: editing ? "0 0 0 2px color-mix(in srgb, var(--tasty-border-focus) 35%, transparent)" : "none" }}>
        <span style={{ display: "inline-flex", flex: "none", color: "var(--tasty-input-icon-fg)" }}>{icon}</span>
        <span style={{ flex: 1, minWidth: 0, fontFamily: "var(--tasty-font-mono)", fontSize: 12,
          color: editing ? "var(--tasty-text-primary)" : "var(--tasty-text-secondary)",
          overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>
          {path}
          {editing && <span style={{ display: "inline-block", width: 1, height: 13, background: "var(--tasty-accent-primary)", marginLeft: 1, verticalAlign: "text-bottom", animation: "tasty-blink 1.1s step-end infinite" }} />}
        </span>
      </div>
      <IconButton size="sm" aria-label="Go">{goGlyph}</IconButton>
    </div>
  );
}

// ════════════════════════════════════════════════════════════
//  EXPLORER (file-manager surface)
// ════════════════════════════════════════════════════════════
function ExpTab({ label, active, closable = true }) {
  return (
    <div style={{ display: "flex", alignItems: "center", gap: 6, height: "100%", padding: "0 8px 0 10px",
      maxWidth: 140, fontSize: 12, cursor: "default",
      color: active ? "var(--tasty-text-primary)" : "var(--tasty-text-muted)",
      background: active ? "var(--tasty-bg-panel)" : "transparent",
      borderRight: "1px solid var(--tasty-separator)",
      boxShadow: active ? "inset 0 2px 0 var(--tasty-accent-primary)" : "none" }}>
      <span style={{ display: "inline-flex", flex: "none", color: "var(--tasty-text-muted)" }}>{ic.folder}</span>
      <span style={{ flex: 1, minWidth: 0, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>{label}</span>
      {closable && <span style={{ display: "inline-flex", flex: "none", opacity: 0.6 }}>{ic.x}</span>}
    </div>
  );
}

function ExpInternalTabs() {
  return (
    <div style={{ display: "flex", alignItems: "stretch", height: 28, flex: "none",
      background: "var(--tasty-bg-sidebar)", borderBottom: "1px solid var(--tasty-separator)" }}>
      <ExpTab label="Downloads" active />
      <ExpTab label="src" />
      <div style={{ display: "flex", alignItems: "center", padding: "0 4px" }}><IconButton size="sm" aria-label="New tab">{ic.plus}</IconButton></div>
    </div>
  );
}

function SegToggle() {
  const items = [["grid", ic.grid], ["list", ic.list], ["detail", ic.detail]];
  return (
    <div style={{ display: "flex", alignItems: "center", flex: "none", height: 28, padding: 2, gap: 2,
      background: "var(--tasty-surface-raised)", border: "1px solid var(--tasty-border-default)", borderRadius: "var(--tasty-radius)" }}>
      {items.map(([k, glyph]) => {
        const active = k === "detail";
        return (
          <span key={k} aria-label={k} title={k} style={{ width: 24, height: 22, display: "inline-flex", alignItems: "center", justifyContent: "center",
            borderRadius: "var(--tasty-radius-sm)", color: active ? "var(--tasty-text-primary)" : "var(--tasty-text-muted)",
            background: active ? "var(--tasty-surface-active)" : "transparent" }}>{glyph}</span>
        );
      })}
    </div>
  );
}

function ExpToolbar() {
  const navBtn = (label, glyph, disabled) => (
    <span style={{ opacity: disabled ? "var(--tasty-state-disabled-opacity)" : 1, pointerEvents: disabled ? "none" : "auto", display: "inline-flex" }}>
      <IconButton size="sm" aria-label={label}>{glyph}</IconButton>
    </span>
  );
  return (
    <div style={{ display: "flex", alignItems: "center", gap: 8, flex: "none", height: 44, padding: "0 8px",
      background: "var(--tasty-bg-panel)", borderBottom: "1px solid var(--tasty-separator)" }}>
      <div style={{ display: "flex", alignItems: "center", gap: 1 }}>
        {navBtn("Back", ic.back)}
        {navBtn("Forward", ic.fwd, true)}
        {navBtn("Up", ic.up)}
        {navBtn("Refresh", ic.refresh)}
      </div>
      <PathField icon={ic.folderOpen} path="~/Downloads" />
      <SegToggle />
    </div>
  );
}

function SideHead({ children, trailing }) {
  return (
    <div style={{ display: "flex", alignItems: "center", padding: "10px 10px 4px" }}>
      <span style={{ flex: 1, fontFamily: "var(--tasty-font-mono)", fontSize: 10, textTransform: "uppercase", letterSpacing: ".07em", color: "var(--tasty-text-muted)" }}>{children}</span>
      {trailing}
    </div>
  );
}

function TreeNode({ label, depth = 0, open, leaf, active }) {
  return (
    <div style={{ display: "flex", alignItems: "center", gap: 4, height: 22, paddingRight: 8, paddingLeft: 6 + depth * 12,
      borderRadius: "var(--tasty-radius-sm)", fontSize: 13, cursor: "default",
      color: active ? "var(--tasty-text-primary)" : "var(--tasty-text-secondary)",
      background: active ? "var(--tasty-surface-active)" : "transparent" }}>
      <span style={{ width: 12, display: "inline-flex", color: "var(--tasty-text-muted)", flex: "none" }}>{leaf ? null : (open ? ic.chevD : ic.chevRsm)}</span>
      <span style={{ display: "inline-flex", color: "var(--tasty-text-muted)", flex: "none" }}>{ic.folder}</span>
      <span style={{ flex: 1, minWidth: 0, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>{label}</span>
    </div>
  );
}

// Favorites empty state — the section CAPTION stays even at 0 favorites, so the
// feature is always discoverable; a muted line + faint hint fill the slot. Tone
// matches the content-area "This folder is empty" state (text-muted). i18n key:
// explorer.sidebar.favorites_empty ("아직 즐겨찾기가 없습니다").
function FavoritesEmpty() {
  return (
    <div style={{ padding: "2px 10px 10px", display: "flex", flexDirection: "column", gap: 3 }}>
      <div style={{ display: "flex", alignItems: "center", gap: 6, color: "var(--tasty-text-muted)" }}>
        <span style={{ display: "inline-flex", flex: "none", opacity: 0.55 }}>{ic.star}</span>
        <span style={{ fontSize: 12 }}>No favorites yet</span>
      </div>
      <div style={{ fontSize: 11, lineHeight: 1.45, color: "var(--tasty-text-placeholder)" }}>
        Right-click a folder → <span style={{ color: "var(--tasty-text-muted)" }}>Add to favorites</span>.
      </div>
    </div>
  );
}

// ExpSidebar — 2-REGION PINNED layout (design SoT for explorer.rs sidebar()).
// NOT one scroll column: "Files" owns the top region with its OWN scroll, and
// "Favorites" is pinned to the bottom at a computed fixed height, so a long tree
// can never push the favorites list below the fold. The boundary between them is
// a fixed 1px --tasty-explorer-split-border at a screen coordinate — it does not
// travel with content (a short tree leaves empty bg above the line, it does not
// pull the line up). Both regions scroll independently.
//   pin height: --tasty-explorer-favorites-pin-height (240) when the sidebar BODY
//   height >= --tasty-explorer-favorites-pin-threshold (600); below that,
//   --tasty-explorer-favorites-pin-ratio (0.4) x body height, snapped to the 4px
//   grid and floored at --tasty-explorer-favorites-pin-min-height (120).
const FAV_PIN = { base: 240, threshold: 600, ratio: 0.4, min: 120 };
function favPinHeight(bodyH) {
  if (!bodyH || bodyH >= FAV_PIN.threshold) return FAV_PIN.base;
  return Math.max(FAV_PIN.min, Math.round((bodyH * FAV_PIN.ratio) / 4) * 4);
}

const TREE_SHORT = [
  { label: "Home", depth: 0, open: true },
  { label: "Downloads", depth: 1, open: true, active: true },
  { label: "figma-exports", depth: 2, leaf: true },
  { label: "Documents", depth: 1 },
  { label: "Projects", depth: 1 },
];
const TREE_LONG = [
  { label: "Home", depth: 0, open: true },
  { label: "Downloads", depth: 1, open: true, active: true },
  { label: "figma-exports", depth: 2, leaf: true },
  { label: "invoices", depth: 2, leaf: true },
  { label: "Documents", depth: 1, open: true },
  { label: "contracts", depth: 2, leaf: true },
  { label: "notes", depth: 2, leaf: true },
  { label: "Projects", depth: 1, open: true },
  { label: "tasty", depth: 2, open: true },
  { label: "crates", depth: 3, leaf: true },
  { label: "docs", depth: 3, leaf: true },
  { label: "src", depth: 3, leaf: true },
  { label: "target", depth: 3, leaf: true },
  { label: "design-system", depth: 2, leaf: true },
  { label: "playground", depth: 2, leaf: true },
  { label: "Pictures", depth: 1, open: true },
  { label: "screenshots", depth: 2, leaf: true },
  { label: "wallpapers", depth: 2, leaf: true },
  { label: "Music", depth: 1 },
  { label: "Videos", depth: 1 },
  { label: ".config", depth: 1 },
  { label: ".cache", depth: 1 },
];

const FAVS_DEFAULT = [["tasty", true], ["Documents", false], ["screenshots", false]];
const FAVS_MANY = [["tasty", true], ["crates", false], ["design-system", false], ["Documents", false],
  ["figma-exports", false], ["screenshots", false], ["wallpapers", false], ["invoices", false],
  [".config", false], ["playground", false]];

function FavRow({ name, active }) {
  return (
    <div style={{ display: "flex", alignItems: "center", gap: 6, height: 22, flex: "none", padding: "0 6px", borderRadius: "var(--tasty-radius-sm)",
      fontSize: 13, color: active ? "var(--tasty-text-primary)" : "var(--tasty-text-secondary)", background: active ? "var(--tasty-surface-active)" : "transparent" }}>
      <span style={{ display: "inline-flex", color: "var(--tasty-accent-warning)", flex: "none" }}>{ic.starFill}</span>
      <span style={{ flex: 1, minWidth: 0, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>{name}</span>
    </div>
  );
}

function ExpSidebar({ favorites = FAVS_DEFAULT, tree = "short", height }) {
  const rows = tree === "long" ? TREE_LONG : TREE_SHORT;
  const pin = favPinHeight(height);
  return (
    <div style={{ width: "var(--tasty-explorer-sidebar-width)", height: height || "100%", flex: "none", display: "flex", flexDirection: "column",
      background: "var(--tasty-bg-sidebar)", borderRight: "var(--tasty-border-width) solid var(--tasty-separator)" }}>
      {/* region A — Files: fills the remainder, scrolls on its own */}
      <div style={{ flex: 1, minHeight: 0, display: "flex", flexDirection: "column" }}>
        <SideHead>Files</SideHead>
        <div className="tasty-scroll" style={{ flex: 1, minHeight: 0, overflowY: "auto", padding: "0 6px 6px", display: "flex", flexDirection: "column", gap: 1 }}>
          {rows.map((r, i) => <TreeNode key={i} {...r} />)}
        </div>
      </div>
      {/* region B — Favorites: pinned to the bottom, fixed height, own scroll */}
      <div style={{ height: pin, flex: "none", display: "flex", flexDirection: "column", overflow: "hidden",
        borderTop: "var(--tasty-border-width) solid var(--tasty-explorer-split-border)" }}>
        <SideHead>Favorites</SideHead>
        {favorites.length === 0 ? (
          <FavoritesEmpty />
        ) : (
          <div className="tasty-scroll" style={{ flex: 1, minHeight: 0, overflowY: "auto", padding: "0 6px 8px", display: "flex", flexDirection: "column", gap: 1 }}>
            {favorites.map(([n, a]) => <FavRow key={n} name={n} active={a} />)}
          </div>
        )}
      </div>
    </div>
  );
}

function DetailRow({ glyph, name, size, date, type, state, glyphColor }) {
  const bg = state === "selected" ? "var(--tasty-surface-active)" : state === "hover" ? "var(--tasty-overlay-hover)" : "transparent";
  return (
    <div style={{ display: "grid", gridTemplateColumns: "1fr 80px 132px 92px", alignItems: "center", height: 26, padding: "0 10px",
      fontSize: 13, background: bg, opacity: state === "cut" ? 0.5 : 1, cursor: "default",
      color: state === "selected" ? "var(--tasty-text-primary)" : "var(--tasty-text-secondary)" }}>
      <span style={{ display: "flex", alignItems: "center", gap: 8, minWidth: 0 }}>
        <span style={{ display: "inline-flex", flex: "none", color: glyphColor || "var(--tasty-text-muted)" }}>{glyph}</span>
        <span style={{ overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>{name}</span>
      </span>
      <span style={{ fontFamily: "var(--tasty-font-mono)", fontSize: 11, color: "var(--tasty-text-muted)", textAlign: "right", paddingRight: 8 }}>{size}</span>
      <span style={{ fontFamily: "var(--tasty-font-mono)", fontSize: 11, color: "var(--tasty-text-muted)" }}>{date}</span>
      <span style={{ fontSize: 12, color: "var(--tasty-text-muted)" }}>{type}</span>
    </div>
  );
}

function DetailHeader() {
  const cols = [["Name", true], ["Size", false], ["Date modified", false], ["Type", false]];
  return (
    <div style={{ display: "grid", gridTemplateColumns: "1fr 80px 132px 92px", alignItems: "center", height: 26, padding: "0 10px",
      background: "var(--tasty-bg-sidebar)", borderBottom: "1px solid var(--tasty-separator)",
      fontFamily: "var(--tasty-font-mono)", fontSize: 10, textTransform: "uppercase", letterSpacing: ".06em", color: "var(--tasty-text-muted)" }}>
      {cols.map(([c, sorted], i) => (
        <span key={c} style={{ display: "flex", alignItems: "center", gap: 3, justifyContent: i === 1 ? "flex-end" : "flex-start",
          paddingRight: i === 1 ? 8 : 0, color: sorted ? "var(--tasty-text-secondary)" : "var(--tasty-text-muted)" }}>
          {c}{sorted && <span style={{ fontSize: 9 }}>▲</span>}
        </span>
      ))}
    </div>
  );
}

function ExpDetail() {
  return (
    <div style={{ flex: 1, minWidth: 0, display: "flex", flexDirection: "column", background: "var(--tasty-bg-panel)" }}>
      <DetailHeader />
      <div style={{ flex: 1, overflow: "hidden" }}>
        <DetailRow glyph={ic.folder} name="figma-exports" size="—" date="2026-06-20 14:30" type="Folder" />
        <DetailRow glyph={ic.file} name="report.pdf" size="2.4 MB" date="2026-06-24 09:12" type="PDF" />
        <DetailRow glyph={ic.image} name="diagram.png" size="488 KB" date="2026-06-26 18:05" type="PNG" state="selected" glyphColor="var(--tasty-accent-info)" />
        <DetailRow glyph={ic.file} name="notes.md" size="12 KB" date="2026-06-27 11:40" type="Markdown" state="hover" />
        <DetailRow glyph={ic.file} name="archive.zip" size="64 MB" date="2026-06-18 22:01" type="Archive" state="cut" />
      </div>
    </div>
  );
}

function ExplorerFrame() {
  return (
    <div style={{ width: "100%", maxWidth: 760, display: "flex", flexDirection: "column", height: 420,
      background: "var(--tasty-bg-panel)", border: "1px solid var(--tasty-border-default)", borderRadius: "var(--tasty-radius)", overflow: "hidden" }}>
      <ExpInternalTabs />
      <ExpToolbar />
      <div style={{ flex: 1, minHeight: 0, display: "flex" }}>
        <ExpSidebar height={348} />
        <ExpDetail />
      </div>
    </div>
  );
}

// GridCell — icon-view file cell. Design SoT for the Rust grid_cell().
//   icon   : icon_glyph_size_md (16), down from item_height_interactive (28)
//   label  : font_size_caption (11), 3-line wrap-to-width + '…' overflow (LayoutJob
//            wrap.max_rows=3 · overflow_character='…' on the real side)
//   width  : CELL_W = 80 (unchanged)
//   height : FIXED 3-line — the label reserves 3 lines always, so short names keep
//            grid rows uniform (stable horizontal_wrapped alignment). Label is
//            top-aligned within the reserved block.
//   gaps   : spacing_sm (8) top/bottom · spacing_xs (4) icon→label
const GRID_CELL_W = 80;
const GRID_LABEL_LINES = 3;
const GRID_LABEL_LINE_H = 14; // round(font_size_caption 11 × 1.3)
function GridCell({ glyph, name, state, glyphColor }) {
  const bg = state === "selected" ? "var(--tasty-surface-active)" : state === "hover" ? "var(--tasty-overlay-hover)" : "transparent";
  return (
    <div style={{ width: GRID_CELL_W, display: "flex", flexDirection: "column", alignItems: "center", gap: 4, padding: "8px 4px",
      borderRadius: "var(--tasty-radius)", background: bg, opacity: state === "cut" ? 0.5 : 1, cursor: "default" }}>
      <span style={{ display: "inline-flex", color: glyphColor || "var(--tasty-text-muted)", height: 16, alignItems: "center" }}>{glyph}</span>
      <span style={{ fontSize: 11, textAlign: "center", lineHeight: `${GRID_LABEL_LINE_H}px`, height: GRID_LABEL_LINES * GRID_LABEL_LINE_H,
        color: state === "selected" ? "var(--tasty-text-primary)" : "var(--tasty-text-secondary)", wordBreak: "break-word",
        display: "-webkit-box", WebkitLineClamp: GRID_LABEL_LINES, WebkitBoxOrient: "vertical", overflow: "hidden" }}>{name}</span>
    </div>
  );
}

function ExpGridMini() {
  return (
    <div style={{ flex: 1, background: "var(--tasty-bg-panel)", padding: 8, display: "flex", flexWrap: "wrap", gap: 4, alignContent: "flex-start" }}>
      <GridCell glyph={ic.folder} name="figma-exports" />
      <GridCell glyph={ic.file} name="rust-toolchain.toml" />
      <GridCell glyph={ic.image} name="diagram.png" state="selected" glyphColor="var(--tasty-accent-info)" />
      <GridCell glyph={ic.file} name="notes.md" state="hover" />
      <GridCell glyph={ic.file} name="THIRD_PARTY_LICENSES.md" />
      <GridCell glyph={ic.folder} name="node_modules" />
    </div>
  );
}

function ExpListMini() {
  const rows = [[ic.folder, "figma-exports", null], [ic.file, "report.pdf", null], [ic.image, "diagram.png", "selected"], [ic.file, "notes.md", "hover"], [ic.file, "build.sh", null], [ic.file, "archive.zip", "cut"]];
  return (
    <div style={{ flex: 1, background: "var(--tasty-bg-panel)", padding: "6px 4px" }}>
      {rows.map(([g, n, st], i) => (
        <div key={i} style={{ display: "flex", alignItems: "center", gap: 8, height: 24, padding: "0 8px", borderRadius: "var(--tasty-radius-sm)",
          fontSize: 13, opacity: st === "cut" ? 0.5 : 1,
          color: st === "selected" ? "var(--tasty-text-primary)" : "var(--tasty-text-secondary)",
          background: st === "selected" ? "var(--tasty-surface-active)" : st === "hover" ? "var(--tasty-overlay-hover)" : "transparent" }}>
          <span style={{ display: "inline-flex", flex: "none", color: g === ic.image ? "var(--tasty-accent-info)" : "var(--tasty-text-muted)" }}>{g}</span>{n}
        </div>
      ))}
    </div>
  );
}

function ViewModeColumn({ title, sub, children }) {
  return (
    <div style={{ flex: 1, minWidth: 0, display: "flex", flexDirection: "column" }}>
      <div style={{ marginBottom: 6 }}>
        <div style={{ fontSize: 12, fontWeight: 600, color: "var(--tasty-text-secondary)" }}>{title}</div>
        <div style={{ fontSize: 11, color: "var(--tasty-text-muted)" }}>{sub}</div>
      </div>
      <div style={{ height: 200, display: "flex", flexDirection: "column", border: "1px solid var(--tasty-separator)", borderRadius: "var(--tasty-radius)", overflow: "hidden" }}>{children}</div>
    </div>
  );
}

function CtxMenu({ title, children }) {
  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 6 }}>
      <div style={{ fontSize: 11, color: "var(--tasty-text-muted)" }}>{title}</div>
      <div style={{ width: 220, background: "var(--tasty-menu-bg)", border: "1px solid var(--tasty-menu-border)",
        borderRadius: "var(--tasty-menu-radius)", padding: 6, boxShadow: "var(--tasty-shadow-popover)" }}>
        {children}
      </div>
    </div>
  );
}

function ExpState({ glyph, glyphColor, title, sub }) {
  return (
    <div style={{ flex: 1, minWidth: 0, height: 180, display: "flex", flexDirection: "column", alignItems: "center", justifyContent: "center", gap: 8,
      background: "var(--tasty-bg-panel)", border: "1px solid var(--tasty-separator)", borderRadius: "var(--tasty-radius)", textAlign: "center", padding: 16 }}>
      <span style={{ display: "inline-flex", color: glyphColor || "var(--tasty-text-muted)", transform: "scale(1.6)" }}>{glyph}</span>
      <div style={{ fontSize: 13, color: glyphColor === "var(--tasty-accent-warning)" ? "var(--tasty-accent-warning)" : "var(--tasty-text-secondary)" }}>{title}</div>
      {sub && <div style={{ fontSize: 11, color: "var(--tasty-text-muted)", maxWidth: 200 }}>{sub}</div>}
    </div>
  );
}

// ════════════════════════════════════════════════════════════
//  CONTENT VIEWERS (markdown / html / image)
// ════════════════════════════════════════════════════════════
const MD_H = {
  1: { fontSize: "var(--tasty-font-size-prose-h1)", fontWeight: 700, color: "var(--tasty-text-primary)", margin: "0 0 10px", lineHeight: 1.3 },
  2: { fontSize: "var(--tasty-font-size-max)", fontWeight: 700, color: "var(--tasty-text-primary)", margin: "20px 0 8px", lineHeight: 1.3 },
  3: { fontSize: "var(--tasty-font-size-max)", fontWeight: 600, color: "var(--tasty-text-primary)", margin: "16px 0 6px" },
  4: { fontSize: "var(--tasty-font-size-body)", fontWeight: 600, color: "var(--tasty-text-secondary)", margin: "14px 0 4px" },
  5: { fontSize: "var(--tasty-font-size-body)", fontWeight: 600, color: "var(--tasty-text-muted)", margin: "12px 0 4px" },
  6: { fontSize: "var(--tasty-font-size-body)", fontWeight: 500, color: "var(--tasty-text-muted)", margin: "12px 0 4px", textTransform: "uppercase", letterSpacing: ".06em" },
};
function MdH({ level, children }) { return <div style={MD_H[level]}>{children}</div>; }
const mdBody = { fontSize: "var(--tasty-font-size-body)", color: "var(--tasty-text-secondary)", lineHeight: 1.6, margin: "0 0 10px" };
function InlineCode({ children }) {
  return <code style={{ fontFamily: "var(--tasty-font-mono)", fontSize: 12, background: "var(--tasty-md-code-bg)", color: "var(--tasty-text-primary)", padding: "1px 5px", borderRadius: "var(--tasty-radius-sm)" }}>{children}</code>;
}
function MdLink({ children }) { return <span style={{ color: "var(--tasty-accent-primary)", textDecoration: "underline", textUnderlineOffset: 2, cursor: "pointer" }}>{children}</span>; }

// Markdown inline GFM table — grid + zebra, read-only (no hover/selection/sort;
// SEPARATE from the shared Table widget). Every color/size reads a --tasty-md-table-*
// token so gallery + real render.rs table() transcribe 1:1. Grid line (surface1) is
// the lightest element → always visible; header (surface0) is the distinct band;
// zebra (mantle) is one subtle step below the base row (base).
const MDT_B = "var(--tasty-border-width) solid var(--tasty-md-table-border)";
function MdTable({ cols, rows }) {
  const pad = "var(--tasty-md-table-cell-padding-y) var(--tasty-md-table-cell-padding-x)";
  return (
    <div style={{ border: MDT_B, borderRadius: "var(--tasty-radius)", overflow: "hidden", marginBottom: 12 }}>
      <table style={{ borderCollapse: "collapse", width: "100%", background: "var(--tasty-md-table-row-bg)",
        fontSize: "var(--tasty-font-size-body)", color: "var(--tasty-md-table-cell-fg)", lineHeight: 1.6 }}>
        <thead>
          <tr>
            {cols.map((c, j) => (
              <th key={j} style={{ padding: pad, textAlign: c.num ? "right" : "left", verticalAlign: "top",
                width: c.w || "auto", background: "var(--tasty-md-table-header-bg)", color: "var(--tasty-md-table-header-fg)",
                fontWeight: "var(--tasty-font-weight-normal)", fontFamily: c.num ? "var(--tasty-font-mono)" : "inherit",
                borderRight: j < cols.length - 1 ? MDT_B : "none", borderBottom: MDT_B }}>{c.h}</th>
            ))}
          </tr>
        </thead>
        <tbody>
          {rows.map((r, i) => (
            <tr key={i}>
              {r.map((cell, j) => (
                <td key={j} style={{ padding: pad, textAlign: cols[j].num ? "right" : "left", verticalAlign: "top",
                  fontFamily: cols[j].num ? "var(--tasty-font-mono)" : "inherit",
                  background: i % 2 === 1 ? "var(--tasty-md-table-row-bg-zebra)" : "transparent",
                  borderRight: j < r.length - 1 ? MDT_B : "none",
                  borderBottom: i < rows.length - 1 ? MDT_B : "none" }}>{cell}</td>
              ))}
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}

function TypeScaleSheet() {
  const rows = [["h1", 1], ["h2", 2], ["h3", 3], ["h4", 4], ["h5", 5], ["h6", 6]];
  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 8, width: 360 }}>
      {rows.map(([tag, lvl]) => (
        <div key={tag} style={{ display: "flex", alignItems: "baseline", gap: 12 }}>
          <span style={{ width: 24, fontFamily: "var(--tasty-font-mono)", fontSize: 10, color: "var(--tasty-text-muted)" }}>{tag}</span>
          <div style={{ ...MD_H[lvl], margin: 0, flex: 1 }}>The quick brown fox</div>
        </div>
      ))}
      <div style={{ display: "flex", alignItems: "baseline", gap: 12 }}>
        <span style={{ width: 24, fontFamily: "var(--tasty-font-mono)", fontSize: 10, color: "var(--tasty-text-muted)" }}>p</span>
        <div style={{ ...mdBody, margin: 0, flex: 1 }}>Body — 13px, line-height 1.6, secondary.</div>
      </div>
      <div style={{ display: "flex", alignItems: "baseline", gap: 12 }}>
        <span style={{ width: 24, fontFamily: "var(--tasty-font-mono)", fontSize: 10, color: "var(--tasty-text-muted)" }}>small</span>
        <div style={{ fontSize: 11, color: "var(--tasty-text-muted)", flex: 1 }}>Caption — body × 0.85, muted.</div>
      </div>
    </div>
  );
}

// Markdown surface chrome — browser-style address bar (path display/edit) + Go
// button, above the body. `editing` = clicked-into edit mode (focus ring + caret).
function MdAddressBar({ editing = false, path = "~/work/tasty/docs/architecture.md" }) {
  return (
    <div style={{ display: "flex", alignItems: "center", gap: 6, height: 40, flex: "none", padding: "0 8px",
      background: "var(--tasty-bg-sidebar)", borderBottom: "1px solid var(--tasty-separator)" }}>
      <PathField icon={ic.file} path={path} editing={editing} />
    </div>
  );
}

// Large-file (>1MB) confirm — surface-scoped popup, same 360px shell as markdown_open.
function MdLargeFilePopup() {
  return (
    <div style={{ width: 360, maxWidth: "100%", background: "var(--tasty-bg-panel)", border: "1px solid var(--tasty-border-strong)",
      borderRadius: "var(--tasty-radius)", overflow: "hidden", boxShadow: "var(--tasty-shadow-modal)" }}>
      <div style={{ padding: "14px 14px 0" }}>
        <div style={{ fontSize: 14, fontWeight: 600, marginBottom: 3 }}>Open large file?</div>
        <div style={{ fontFamily: "var(--tasty-font-mono)", fontSize: 11, color: "var(--tasty-text-muted)", marginBottom: 10, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>~/work/tasty/docs/CHANGELOG-full.md</div>
        <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
          <Tag variant="warning">3.2 MB</Tag>
          <span style={{ fontSize: 12, color: "var(--tasty-text-secondary)", lineHeight: 1.5 }}>Over 1 MB — rendering may be slow.</span>
        </div>
      </div>
      <div style={{ display: "flex", justifyContent: "flex-end", gap: 8, padding: 14 }}>
        <Button variant="ghost">Cancel</Button>
        <Button variant="primary">Open</Button>
      </div>
    </div>
  );
}

function MarkdownDoc({ editing = false }) {
  return (
    <div style={{ width: "100%", maxWidth: 640, height: 460, display: "flex", flexDirection: "column", background: "var(--tasty-md-doc-bg)",
      border: "1px solid var(--tasty-border-default)", borderRadius: "var(--tasty-radius)", overflow: "hidden" }}>
      <MdAddressBar editing={editing} />
      <div style={{ flex: 1, minHeight: 0, overflow: "auto", padding: "20px 24px" }}>
      <MdH level={1}>Markdown surface</MdH>
      <div style={mdBody}>A read-only viewer that reloads on file change. Text is selectable; links show a URL tooltip on hover. Body uses <InlineCode>--tasty-font-size-body</InlineCode> at the 14px cap, line-height <b style={{ color: "var(--tasty-text-secondary)" }}>1.6</b>.</div>
      <MdH level={2}>Headings &amp; emphasis</MdH>
      <div style={mdBody}>Inline runs: <b style={{ color: "var(--tasty-text-secondary)" }}>bold</b>, <i>italic</i>, <b><i>both</i></b>, <span style={{ textDecoration: "line-through", color: "var(--tasty-text-muted)" }}>strikethrough</span>, a <MdLink>link</MdLink>, and <InlineCode>inline code</InlineCode>.</div>
      <MdH level={3}>Lists</MdH>
      <div style={{ ...mdBody, margin: "0 0 6px" }}>
        <div style={{ display: "flex", gap: 8 }}><span style={{ color: "var(--tasty-text-muted)" }}>•</span><span>Bullet item with wrapped text running onto a second line for rhythm.</span></div>
        <div style={{ display: "flex", gap: 8, paddingLeft: 16 }}><span style={{ color: "var(--tasty-text-muted)" }}>◦</span><span>Nested bullet</span></div>
        <div style={{ display: "flex", gap: 8 }}><span style={{ color: "var(--tasty-text-muted)", fontFamily: "var(--tasty-font-mono)", fontSize: 12 }}>1.</span><span>Ordered item</span></div>
        <div style={{ display: "flex", gap: 8, alignItems: "center" }}><Checkbox checked readOnly /><span>Task done</span></div>
        <div style={{ display: "flex", gap: 8, alignItems: "center" }}><Checkbox readOnly /><span>Task to do</span></div>
      </div>
      <MdH level={3}>Code block</MdH>
      <pre style={{ margin: "0 0 12px", background: "var(--tasty-md-code-bg)", border: "1px solid var(--tasty-md-code-border)", borderRadius: "var(--tasty-radius)", padding: 12, overflow: "auto",
        fontFamily: "var(--tasty-font-mono)", fontSize: 12, color: "var(--tasty-text-secondary)", lineHeight: 1.5 }}>
{`fn main() {
    println!("hi from tasty");
}`}
      </pre>
      <MdH level={3}>Table</MdH>
      {/* Markdown inline table — grid + zebra (read-only: no hover/selection/sort).
          Opaque base fill + border-strong grid + surface0 header + mantle zebra so it
          reads on both the focused surface (~#000) and this bg-panel card. Value ladder
          dark→light: mantle(zebra) < base(row) < surface0(header) < surface1(grid). */}
      <MdTable
        cols={[{ h: "Resource" }, { h: "Kind" }, { h: "Count", num: true, w: "80px" }]}
        rows={[["surface", "viewer", "12"], ["popup", "overlay", "8"], ["banner", "notice", "3"]]}
      />
      <MdH level={3}>Blockquote</MdH>
      <div style={{ borderLeft: "2px solid var(--tasty-md-quote-bar)", paddingLeft: 12, margin: "0 0 12px", color: "var(--tasty-md-quote-fg)", fontSize: 13, lineHeight: 1.6 }}>
        Quoted text reads one tone down (muted) with a left bar.
        <div style={{ borderLeft: "2px solid var(--tasty-md-quote-bar)", paddingLeft: 12, marginTop: 6 }}>Nested quote, one level deeper.</div>
      </div>
      <div style={{ height: 1, background: "var(--tasty-md-rule)", margin: "16px 0" }} />
      <div style={{ fontSize: 11, color: "var(--tasty-text-muted)" }}>Horizontal rule above · trailing space below.</div>
      </div>
    </div>
  );
}

// ── Markdown color injection — the ONLY thing parity binds in md CONTENT.
// egui_commonmark reads all color from egui::Visuals; we override those fields
// with Tasty theme tokens. Size ladder / leading / block spacing stay the
// library's (library-driven). Rendered as text+swatch so the mapping is the
// source of truth, not a pixel drawing.
function Sw({ c }) {
  return <span style={{ display: "inline-block", width: 12, height: 12, flex: "none", borderRadius: 2, border: "1px solid var(--tasty-separator)", background: c, verticalAlign: "middle" }} />;
}
const VIS_MAP = [
  ["override_text_color", "body text", "--tasty-text-secondary"],
  ["strong_text_color", "headings · strong · table header", "--tasty-text-primary"],
  ["hyperlink_color", "links", "--tasty-accent-primary"],
  ["code_bg_color", "inline code fill", "--tasty-surface-raised"],
  ["extreme_bg_color", "code-block fill", "--tasty-surface-raised"],
  ["weak_text_color", "blockquote accent bar", "--tasty-border-strong"],
  ["noninteractive.bg_stroke", "code-block border", "--tasty-separator"],
];
const SYNTECT_MAP = [
  ["keyword", "--tasty-color-mauve"],
  ["string", "--tasty-color-green"],
  ["function · method", "--tasty-color-blue"],
  ["number · constant · bool", "--tasty-color-peach"],
  ["type · class", "--tasty-color-yellow"],
  ["comment (italic)", "--tasty-text-muted"],
  ["operator · punctuation", "--tasty-text-secondary"],
  ["variable · identifier", "--tasty-text-primary"],
];
function MdInjectionMap() {
  const cell = { padding: "6px 10px", fontSize: 12, color: "var(--tasty-text-secondary)", borderBottom: "1px solid var(--tasty-separator)", textAlign: "left", verticalAlign: "middle" };
  const head = { ...cell, fontSize: 11, color: "var(--tasty-text-muted)", fontWeight: "var(--tasty-font-weight-normal)", textTransform: "uppercase", letterSpacing: ".05em" };
  const mono = { fontFamily: "var(--tasty-font-mono)", fontSize: 11 };
  return (
    <div style={{ display: "flex", flexWrap: "wrap", gap: 24, alignItems: "flex-start" }}>
      <div style={{ flex: "1 1 380px", minWidth: 320 }}>
        <div style={{ fontSize: 11, color: "var(--tasty-text-muted)", marginBottom: 6, textTransform: "uppercase", letterSpacing: ".05em" }}>egui::Visuals → Tasty token</div>
        <div style={{ border: "1px solid var(--tasty-border-default)", borderRadius: "var(--tasty-radius)", overflow: "hidden", background: "var(--tasty-bg-panel)" }}>
          <table style={{ borderCollapse: "collapse", width: "100%" }}>
            <thead><tr><th style={head}>Visuals field</th><th style={head}>draws</th><th style={head}>token</th></tr></thead>
            <tbody>
              {VIS_MAP.map(([f, d, t], i) => (
                <tr key={i}>
                  <td style={{ ...cell, ...mono, color: "var(--tasty-accent-agent)" }}>{f}</td>
                  <td style={cell}>{d}</td>
                  <td style={{ ...cell, borderBottom: i === VIS_MAP.length - 1 ? "none" : cell.borderBottom }}>
                    <span style={{ display: "inline-flex", alignItems: "center", gap: 6 }}><Sw c={`var(${t})`} /><span style={mono}>{t}</span></span>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      </div>
      <div style={{ flex: "1 1 240px", minWidth: 220 }}>
        <div style={{ fontSize: 11, color: "var(--tasty-text-muted)", marginBottom: 6, textTransform: "uppercase", letterSpacing: ".05em" }}>syntect code theme (Tasty palette)</div>
        <div style={{ border: "1px solid var(--tasty-border-default)", borderRadius: "var(--tasty-radius)", overflow: "hidden", background: "var(--tasty-bg-panel)" }}>
          <table style={{ borderCollapse: "collapse", width: "100%" }}>
            <tbody>
              {SYNTECT_MAP.map(([role, t], i) => (
                <tr key={i}>
                  <td style={{ ...cell, borderBottom: i === SYNTECT_MAP.length - 1 ? "none" : cell.borderBottom }}>{role}</td>
                  <td style={{ ...cell, textAlign: "right", borderBottom: i === SYNTECT_MAP.length - 1 ? "none" : cell.borderBottom }}>
                    <span style={{ display: "inline-flex", alignItems: "center", gap: 6, justifyContent: "flex-end" }}><span style={{ ...mono, color: `var(${t})` }}>{t.replace("--tasty-", "")}</span><Sw c={`var(${t})`} /></span>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      </div>
    </div>
  );
}

function HtmlTile({ state }) {
  const center = (glyph, glyphColor, title, titleColor, sub) => (
    <div style={{ flex: 1, display: "flex", flexDirection: "column", alignItems: "center", justifyContent: "center", gap: 8, textAlign: "center", padding: 16 }}>
      <span style={{ display: "inline-flex", color: glyphColor, transform: "scale(1.5)" }}>{glyph}</span>
      <div style={{ fontSize: 13, color: titleColor }}>{title}</div>
      {sub && <div style={{ fontSize: 11, fontFamily: "var(--tasty-font-mono)", color: "var(--tasty-text-disabled)", maxWidth: 220, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>{sub}</div>}
    </div>
  );
  const body = {
    boundary: center(ic.globe, "var(--tasty-text-muted)", "WebView region", "var(--tasty-text-muted)", "https://docs.tasty.sh"),
    placeholder: center(ic.globe, "var(--tasty-text-disabled)", "No page loaded", "var(--tasty-text-muted)"),
    loading: center(<Spinner />, "var(--tasty-spinner-indicator)", "Loading…", "var(--tasty-text-muted)"),
    error: center(ic.alert, "var(--tasty-accent-danger)", "Failed to load", "var(--tasty-accent-danger)", "https://offline.example"),
  }[state];
  return (
    <div style={{ width: 220, height: 150, display: "flex", flexDirection: "column", background: "var(--tasty-bg-panel)",
      border: "1px solid var(--tasty-border-default)", borderRadius: "var(--tasty-radius)", overflow: "hidden" }}>{body}</div>
  );
}

function HtmlSettings() {
  const Row = ({ label, children }) => (
    <div style={{ display: "flex", alignItems: "center", gap: 12, minHeight: 28 }}>
      <span style={{ flex: 1, fontSize: 13, color: "var(--tasty-text-secondary)" }}>{label}</span>{children}
    </div>
  );
  return (
    <div style={{ width: 320, display: "flex", flexDirection: "column", gap: 8, padding: 14,
      background: "var(--tasty-bg-panel)", border: "1px solid var(--tasty-border-default)", borderRadius: "var(--tasty-radius)" }}>
      <div style={{ fontFamily: "var(--tasty-font-mono)", fontSize: 10, textTransform: "uppercase", letterSpacing: ".06em", color: "var(--tasty-text-muted)" }}>HTML viewer</div>
      <Row label="Default zoom"><div style={{ display: "flex", alignItems: "center", gap: 4 }}><Input style={{ width: 72 }} defaultValue="100" /><span style={{ fontSize: 12, color: "var(--tasty-text-muted)" }}>%</span></div></Row>
      <Row label="Color scheme"><Select options={["follow", "light", "dark"]} style={{ width: 160 }} /></Row>
      <Row label="Allow remote content"><Switch /></Row>
      <Row label="Sandbox scripts"><Switch defaultChecked /></Row>
      <div style={{ fontSize: 11, color: "var(--tasty-text-muted)", lineHeight: 1.5 }}>Page pixels are drawn by the OS WebView — these settings configure it; tasty doesn't theme the content.</div>
    </div>
  );
}

function ImgBtn({ glyph, label, disabled }) {
  return (
    <span style={{ opacity: disabled ? "var(--tasty-state-disabled-opacity)" : 1, pointerEvents: disabled ? "none" : "auto", display: "inline-flex" }}>
      <IconButton size="sm" aria-label={label}>{glyph}</IconButton>
    </span>
  );
}
function ZoomGroup({ pct = "100%" }) {
  return (
    <div style={{ display: "flex", alignItems: "center", gap: 2, flex: "none" }}>
      <Button variant="secondary" size="sm">Fit</Button>
      <ImgBtn glyph={ic.plus} label="Zoom in" />
      <span style={{ fontFamily: "var(--tasty-font-mono)", fontSize: 12, color: "var(--tasty-text-muted)", minWidth: 40, textAlign: "center" }}>{pct}</span>
      <IconButton size="sm" aria-label="Zoom out"><GIcon d={<path d="M5 12h14" />} /></IconButton>
    </div>
  );
}
function ImgCanvas({ children, panned }) {
  return (
    <div style={{ flex: 1, minHeight: 0, position: "relative", background: "var(--tasty-bg-sidebar)", display: "flex", alignItems: "center", justifyContent: "center", overflow: "hidden" }}>
      <div style={{ position: "relative", width: panned ? 280 : 200, height: panned ? 200 : 132, transform: panned ? "translate(24px,-12px)" : "none",
        background: "linear-gradient(135deg, var(--tasty-surface-raised), var(--tasty-bg-panel))",
        border: "1px solid var(--tasty-border-default)", borderRadius: "var(--tasty-radius-sm)", display: "flex", alignItems: "center", justifyContent: "center",
        color: "var(--tasty-text-disabled)" }}>
        <span style={{ display: "inline-flex", transform: "scale(1.4)" }}>{ic.image}</span>
        {children}
      </div>
    </div>
  );
}
function FloatingSelection() {
  const handle = (style) => <span style={{ position: "absolute", width: 6, height: 6, background: "var(--tasty-accent-primary)", ...style }} />;
  return (
    <div style={{ position: "absolute", top: "50%", left: "50%", transform: "translate(-50%,-50%)", width: 96, height: 64,
      border: "1px solid var(--tasty-accent-primary)", background: "var(--tasty-bg-panel)", display: "flex", alignItems: "center", justifyContent: "center", color: "var(--tasty-text-muted)" }}>
      <span style={{ fontSize: 10, fontFamily: "var(--tasty-font-mono)" }}>pasted</span>
      {handle({ top: -3, left: -3 })}{handle({ top: -3, left: "calc(50% - 3px)" })}{handle({ top: -3, right: -3 })}
      {handle({ top: "calc(50% - 3px)", left: -3 })}{handle({ top: "calc(50% - 3px)", right: -3 })}
      {handle({ bottom: -3, left: -3 })}{handle({ bottom: -3, left: "calc(50% - 3px)" })}{handle({ bottom: -3, right: -3 })}
    </div>
  );
}
function ImgBar({ children }) {
  return <div style={{ display: "flex", alignItems: "center", gap: 8, flex: "none", height: 40, padding: "0 8px", background: "var(--tasty-bg-panel)", borderBottom: "1px solid var(--tasty-separator)" }}>{children}</div>;
}
function ImgSurface({ mode, panned, floating }) {
  return (
    <div style={{ width: "100%", maxWidth: 460, height: 320, display: "flex", flexDirection: "column",
      background: "var(--tasty-bg-panel)", border: "1px solid var(--tasty-border-default)", borderRadius: "var(--tasty-radius)", overflow: "hidden" }}>
      {mode === "viewer" ? (
        <ImgBar>
          <div style={{ display: "flex", gap: 1 }}><ImgBtn glyph={ic.back} label="Previous" /><ImgBtn glyph={ic.fwd} label="Next" /><ImgBtn glyph={ic.refresh} label="Refresh" /><ImgBtn glyph={ic.rename} label="Edit" /><ImgBtn glyph={ic.plus} label="New image" /></div>
          <span style={{ fontFamily: "var(--tasty-font-mono)", fontSize: 11, color: "var(--tasty-text-muted)" }}>diagram.png (2/5)</span>
          <span style={{ flex: 1 }} />
          <ZoomGroup pct={panned ? "250%" : "100%"} />
        </ImgBar>
      ) : (
        <ImgBar>
          <Button variant="secondary" size="sm">Save</Button>
          <Button variant="ghost" size="sm">Cancel</Button>
          <ImgBtn glyph={<GIcon d={<path d="M9 14 4 9l5-5M4 9h11a5 5 0 0 1 0 10h-1" />} />} label="Undo" />
          <ImgBtn glyph={<GIcon d={<path d="m15 14 5-5-5-5M20 9H9a5 5 0 0 0 0 10h1" />} />} label="Redo" disabled />
          <span style={{ width: 1, height: 18, background: "var(--tasty-separator)" }} />
          <span style={{ fontSize: 11, color: "var(--tasty-text-muted)" }}>Brush</span>
          <span style={{ width: 60, height: 4, borderRadius: 2, background: "var(--tasty-surface-active)", position: "relative" }}><span style={{ position: "absolute", left: "40%", top: -3, width: 10, height: 10, borderRadius: "50%", background: "var(--tasty-text-muted)" }} /></span>
          <span style={{ fontSize: 11, color: "var(--tasty-text-muted)" }}>Color</span>
          <span style={{ width: 16, height: 16, borderRadius: "var(--tasty-radius-sm)", background: "var(--tasty-accent-danger)", border: "1px solid var(--tasty-border-strong)" }} />
          <span style={{ flex: 1 }} />
          <ZoomGroup />
        </ImgBar>
      )}
      <ImgCanvas panned={panned}>{floating && <FloatingSelection />}</ImgCanvas>
    </div>
  );
}

function Page() {
  return (
    <>
      <Section id="explorer" title="Explorer — the file-manager surface">
        <Spec title="Editable path field — shared address component (Explorer + Markdown)"
          when={<>Both the Explorer toolbar and the Markdown chrome now share <b>one</b> editable path field — a single design SoT for the “path address” role. <b>Display</b>: one mono path line (ellipsis, <span className="tok">--tasty-text-secondary</span>) behind a leading icon, with a trailing <b>Go</b> (arrow-right) button. <b>Click to edit</b>: focus ring + blink caret, text goes <span className="tok">--tasty-text-primary</span>; <span className="ic">↵</span> or <b>Go</b> navigate to the typed path, <span className="ic">Esc</span> reverts. While editing, a <b><a href="components.html#forms">AutoComplete</a> candidate dropdown</b> hangs below the field — a typeahead of recent paths (Explorer = recent directories, Markdown = recent files) that narrows as you type; <span className="ic">↑↓</span> move the keyboard-active row, <span className="ic">↵</span>/Go confirm. The only per-surface parameters are the <b>leading icon</b> slot (Explorer <span className="ic">folderOpen</span> / Markdown <span className="ic">file</span>) and the candidate list. Field tokens align to <code>forms/Input</code>: <span className="tok">--tasty-input-bg</span> / <span className="tok">--tasty-input-border</span> (idle) → <span className="tok">--tasty-input-border-focus</span> (edit) + focus ring.</>}>
          <Stage variant="solo center" style={{ padding: 20, background: "var(--tasty-bg-app)", flexDirection: "column", gap: 18 }}>
            <div style={{ display: "grid", gridTemplateColumns: "repeat(2, 300px)", gap: 20, justifyContent: "center" }}>
              {[["Explorer context — folderOpen · recent directories", ic.folderOpen, "~/Downloads", EXP_RECENT], ["Markdown context — file · recent files", ic.file, "~/work/tasty/docs/architecture.md", MD_RECENT]].map(([label, icon, path, candidates]) => (
                <div key={label} style={{ display: "flex", flexDirection: "column", gap: 8 }}>
                  <div style={{ fontSize: 11, color: "var(--tasty-text-muted)" }}>{label}</div>
                  <div style={{ display: "flex", alignItems: "center", gap: 10 }}>
                    <span style={{ width: 54, flex: "none", fontFamily: "var(--tasty-font-mono)", fontSize: 10, color: "var(--tasty-text-muted)" }}>idle</span>
                    <PathField icon={icon} path={path} />
                  </div>
                  <div style={{ display: "flex", alignItems: "center", gap: 10 }}>
                    <span style={{ width: 54, flex: "none", fontFamily: "var(--tasty-font-mono)", fontSize: 10, color: "var(--tasty-text-muted)" }}>editing</span>
                    <PathField icon={icon} path={path} editing />
                  </div>
                  <div style={{ display: "flex", gap: 10, minHeight: 200 }}>
                    <span style={{ width: 54, flex: "none", fontFamily: "var(--tasty-font-mono)", fontSize: 10, color: "var(--tasty-text-muted)", paddingTop: 6 }}>editing<br/>+ list</span>
                    <div style={{ flex: 1, position: "relative" }}>
                      <PathField icon={icon} path={path} editing candidates={candidates} open activeIndex={1} query="tasty" />
                    </div>
                  </div>
                </div>
              ))}
            </div>
          </Stage>
          <Meta
            specs={[["display", "mono path · ellipsis · text-secondary"], ["editing", "focus ring (2px) + caret · text-primary"], ["suggestions", <>editing → <a href="components.html#forms">AutoComplete</a> dropdown (recent paths)</>], ["leading icon", "per-surface slot — folderOpen / file"], ["candidates", "per-surface — recent dirs / recent files"], ["Go", "arrow-right · IconButton sm · ↵ also confirms"], ["navigate", "↵ / Go → typed path · Esc reverts"], ["height / radius", <>28 · <span className="tok">--tasty-radius</span></>]]}
            tokens={[{ tok: "--tasty-input-bg", use: "field fill", color: "var(--tasty-input-bg)" }, { tok: "--tasty-input-border", use: "idle border", color: "var(--tasty-input-border)" }, { tok: "--tasty-input-border-focus", use: "editing border", color: "var(--tasty-input-border-focus)" }, { tok: "--tasty-border-focus", use: "focus ring (35%)", color: "var(--tasty-border-focus)" }, { tok: "--tasty-autocomplete-menu-bg", use: "dropdown fill", color: "var(--tasty-autocomplete-menu-bg)" }, { tok: "--tasty-autocomplete-row-bg-active", use: "keyboard-active row", color: "var(--tasty-autocomplete-row-bg-active)" }, { tok: "--tasty-input-icon-fg", use: "leading icon", color: "var(--tasty-input-icon-fg)" }, { tok: "--tasty-accent-primary", use: "caret + Go glyph", color: "var(--tasty-accent-primary)" }]} />
          <Note>Open questions resolved (design SoT): <b>(1) leading icon</b> is a per-surface slot — Explorer <span className="ic">folderOpen</span>, Markdown <span className="ic">file</span> — mirroring <code>forms/Input</code>'s leading-icon slot. <b>(2)</b> Explorer's <b>breadcrumb</b> (<code>Crumb</code> + chevron) is <b>dropped</b> for the plain editable field; a hybrid crumb↔edit form was <b>rejected</b>. <b>(3) Go</b> button on both surfaces. <b>(4)</b> only the field + dropdown are shared; the outer bar stays per-surface. <b>(5 — new)</b> the edit-time candidate dropdown is the formal <b><a href="components.html#forms">AutoComplete</a></b> component (full state matrix — filtered / overflow-scroll / empty / hover vs keyboard-active — lives on the Components page), replacing the source-only <code>Combobox</code>. Source home: extract <code>PathField</code> into <code>crates/tasty-ui-widgets</code> composing the renamed <code>AutoComplete</code> (was <code>Combobox</code>); recent-path candidate source + wiring/i18n tracked separately from this design pass.</Note>
        </Spec>
        <Spec title="Explorer surface — full layout (Detail view)"
          when={<>A full file manager living in a work-area tile, like the terminal/markdown surfaces. Top→bottom: an <b>internal directory tab bar</b> (lighter than the pane tab strip — each tab is its own cwd), a <b>toolbar</b> (Back / Forward / Up / Refresh · editable path field · grid/list/detail view toggle), then a body of a <b>196px sidebar</b> (Files tree over a Favorites list) feeding the <b>content area</b>. Default view is <b>Detail</b>; the address is an <b>editable path field</b> shared with the Markdown surface (click to type a path, <span className="ic">↵</span>/Go to jump to that directory).</>}>
          <Stage variant="solo center" style={{ padding: 20, background: "var(--tasty-bg-app)" }}>
            <ExplorerFrame />
          </Stage>
          <Meta
            specs={[["surface", "fills a work-area tile"], ["internal tabs", <>28px · per-cwd, with <span className="ic">×</span> + <span className="ic">＋</span></>], ["toolbar", "44px · nav · path field · view toggle"], ["sidebar", <>196px — Files tree + Favorites</>], ["splitter", <>1px <span className="tok">--tasty-separator</span>, drag to resize</>], ["row height", <>26px (Detail) <span className="tok">--tasty-control-height-tree</span> family</>], ["selected row", <span className="tok">--tasty-surface-active</span>]]}
            tokens={[{ tok: "--tasty-bg-panel", use: "surface + content", color: "var(--tasty-bg-panel)" }, { tok: "--tasty-bg-sidebar", use: "tabs + sidebar + header", color: "var(--tasty-bg-sidebar)" }, { tok: "--tasty-surface-raised", use: "view toggle", color: "var(--tasty-surface-raised)" }, { tok: "--tasty-input-bg", use: "path field", color: "var(--tasty-input-bg)" }, { tok: "--tasty-surface-active", use: "selected / current", color: "var(--tasty-surface-active)" }, { tok: "--tasty-accent-warning", use: "favorite star", color: "var(--tasty-accent-warning)" }, { tok: "--tasty-accent-primary", use: "active tab bar", color: "var(--tasty-accent-primary)" }]} />
          <Note>Defaults chosen (briefs left these open): <b>Detail</b> is the default view; the right-hand preview panel is <b>dropped</b> in favour of a wider content area (re-add later as a toggle if needed); no in-toolbar filter search in this pass; the tree is rooted at <b>Home</b>. Favorites are global across surfaces.</Note>
        </Spec>

        <Spec title="Sidebar Favorites — populated vs. empty state"
          when={<>The sidebar <b>Favorites</b> section caption is <b>always shown</b>, even at zero favorites — so the feature never disappears and reads as unimplemented. With favorites it lists STAR rows (left); empty, the caption stays and an <b>empty state</b> fills the slot: a faint outline star + <b>"No favorites yet"</b> in <span className="tok">--tasty-text-muted</span>, then a dimmer one-line hint pointing at the add path. Same tone as the content-area "This folder is empty" state — left-aligned to fit the 196px column.</>}>
          <Stage variant="solo center" style={{ padding: 20, background: "var(--tasty-bg-app)", gap: 18, flexWrap: "wrap" }}>
            <div style={{ display: "flex", flexDirection: "column", gap: 6 }}>
              <div style={{ fontSize: 11, color: "var(--tasty-text-muted)" }}>with favorites</div>
              <div style={{ height: 300, display: "flex", border: "1px solid var(--tasty-border-default)", borderRadius: "var(--tasty-radius)", overflow: "hidden" }}><ExpSidebar height={300} /></div>
            </div>
            <div style={{ display: "flex", flexDirection: "column", gap: 6 }}>
              <div style={{ fontSize: 11, color: "var(--tasty-text-muted)" }}>empty — caption persists</div>
              <div style={{ height: 300, display: "flex", border: "1px solid var(--tasty-border-default)", borderRadius: "var(--tasty-radius)", overflow: "hidden" }}><ExpSidebar height={300} favorites={[]} /></div>
            </div>
          </Stage>
          <Meta
            specs={[["caption", "always shown — micro caps, text-muted"], ["empty line", <>outline star + "No favorites yet" · <span className="tok">--tasty-text-muted</span></>], ["hint", <>1 line · <span className="tok">--tasty-text-placeholder</span></>], ["align", "left, inside 196px column"], ["condition", "was: hide section at 0 → now: always show"]]}
            tokens={[{ tok: "--tasty-text-muted", use: "caption + empty line", color: "var(--tasty-text-muted)" }, { tok: "--tasty-text-placeholder", use: "hint line", color: "var(--tasty-text-placeholder)" }, { tok: "--tasty-accent-warning", use: "filled star (populated)", color: "var(--tasty-accent-warning)" }]} />
          <Note>New string <InlineCode>explorer.sidebar.favorites_empty</InlineCode> — en "No favorites yet" · ko "아직 즐겨찾기가 없습니다" · add to <InlineCode>lang/&#123;en,ko,ja&#125;.toml</InlineCode>. The hint reuses existing "Add to favorites" copy. Implementer: replace <InlineCode>if !favorites.is_empty()</InlineCode> in <InlineCode>explorer.rs sidebar()</InlineCode> with an always-rendered caption + empty branch.</Note>
        </Spec>

        <Spec title="Sidebar layout — Favorites PINNED to the bottom (2-region split)"
          when={<>The sidebar is <b>not one scroll column</b> any more. It splits into two independent regions: <b>Files</b> takes the remainder at the top and scrolls on its own, and <b>Favorites</b> is <b>pinned to the bottom</b> at a computed fixed height — so however deep the tree gets, the favorites list is always on screen. The boundary is a <b>fixed 1px <span className="tok">--tasty-explorer-split-border</span></b> at a screen coordinate: it does <b>not</b> flow with content, so a short tree leaves empty <span className="tok">--tasty-bg-sidebar</span> above the line rather than pulling the line up. Pin height = <b>240</b> (<span className="tok">--tasty-explorer-favorites-pin-height</span>) while the sidebar body is ≥ <b>600</b> (<span className="tok">--tasty-explorer-favorites-pin-threshold</span>); below that it becomes <b>40%</b> of the body height (<span className="tok">--tasty-explorer-favorites-pin-ratio</span>), snapped to the 4px grid and floored at <b>120</b> (<span className="tok">--tasty-explorer-favorites-pin-min-height</span>). Row, caption and empty-state visuals are unchanged — this is a layout change only.</>}>
          <Stage variant="solo center" style={{ padding: 20, background: "var(--tasty-bg-app)", flexDirection: "column", gap: 18 }}>
            <div style={{ display: "flex", gap: 18, flexWrap: "wrap", justifyContent: "center" }}>
              {[["long tree — Files scrolls, pin holds", "tree overflows · favorites intact", { tree: "long", favorites: FAVS_DEFAULT }],
                ["short tree — empty bg above a fixed line", "boundary does not move up", { tree: "short", favorites: FAVS_DEFAULT }],
                ["Favorites empty — caption + hint persist", "pinned region keeps its height", { tree: "long", favorites: [] }],
                ["many favorites — region scrolls itself", "list never expands past the pin", { tree: "long", favorites: FAVS_MANY }]].map(([label, sub, props]) => (
                <div key={label} style={{ display: "flex", flexDirection: "column", gap: 6 }}>
                  <div style={{ fontSize: 11, color: "var(--tasty-text-secondary)" }}>{label}</div>
                  <div style={{ fontSize: 11, color: "var(--tasty-text-muted)" }}>{sub}</div>
                  <div style={{ display: "flex", border: "var(--tasty-border-width) solid var(--tasty-border-default)", borderRadius: "var(--tasty-radius)", overflow: "hidden" }}>
                    <ExpSidebar height={620} {...props} />
                  </div>
                  <div style={{ fontFamily: "var(--tasty-font-mono)", fontSize: 10, color: "var(--tasty-text-muted)" }}>body 620 → pin {favPinHeight(620)}</div>
                </div>
              ))}
            </div>
            <div style={{ display: "flex", gap: 18, flexWrap: "wrap", justifyContent: "center", alignItems: "flex-start" }}>
              {[620, 560, 420, 300].map((h) => (
                <div key={h} style={{ display: "flex", flexDirection: "column", gap: 6 }}>
                  <div style={{ fontFamily: "var(--tasty-font-mono)", fontSize: 10, color: "var(--tasty-text-muted)" }}>body {h} → pin {favPinHeight(h)}{h >= 600 ? " (240 flat)" : h === 300 ? " (min clamp)" : " (40%)"}</div>
                  <div style={{ display: "flex", border: "var(--tasty-border-width) solid var(--tasty-border-default)", borderRadius: "var(--tasty-radius)", overflow: "hidden" }}>
                    <ExpSidebar height={h} tree="long" />
                  </div>
                </div>
              ))}
            </div>
          </Stage>
          <Meta
            specs={[["structure", "2 regions · independent scroll state"], ["Files region", "flex 1 · own scroll · caption pinned at its top"], ["Favorites region", <>fixed height, bottom-pinned · <span className="tok">--tasty-explorer-favorites-pin-height</span></>], ["pin height", "body ≥ 600 → 240 · else 40% (4px-snapped) · min 120"], ["boundary", <>1px <span className="tok">--tasty-explorer-split-border</span> at a fixed coordinate</>], ["sidebar width", <>196 unchanged · <span className="tok">--tasty-explorer-sidebar-width</span></>], ["resize", "recomputed from the live body height — no drag handle"], ["row visuals", "unchanged (tree row · star row · empty state)"]]}
            tokens={[{ tok: "--tasty-explorer-sidebar-width", use: "196 column" }, { tok: "--tasty-explorer-favorites-pin-height", use: "pinned region height" }, { tok: "--tasty-explorer-favorites-pin-threshold", use: "small-surface switch" }, { tok: "--tasty-explorer-favorites-pin-min-height", use: "lower clamp" }, { tok: "--tasty-explorer-split-border", use: "fixed boundary line", color: "var(--tasty-separator)" }, { tok: "--tasty-bg-sidebar", use: "both regions' fill", color: "var(--tasty-bg-sidebar)" }, { tok: "--tasty-accent-warning", use: "filled star", color: "var(--tasty-accent-warning)" }]} />
          <Note>Open decisions resolved (design SoT): <b>(1) threshold basis</b> = the <b>sidebar body height</b> (the split container itself), not the whole explorer tab — the pin only competes with the tree, and this keeps the rule local to <InlineCode>sidebar()</InlineCode>. <b>(2) scrollbar</b> = the shared <InlineCode>.tasty-scroll</InlineCode> behaviour, hover-revealed like every other scroll area — the pinned region gets no special always-on bar. <b>(3) slack space</b> at 0–1 favorites stays <b>empty <span className="tok">--tasty-bg-sidebar</span></b>: no filler padding, no vertical centering, so rows always start directly under the caption. <b>(4) boundary weight</b> = the <b>same</b> 1px separator as before — no shadow, no tint; "pinned" is communicated by behaviour (the line never moves, each side scrolls alone), not by extra decoration. <b>(5) 240 ↔ 40% jump</b> is left as a <b>hard switch</b> (no interpolation) as the requester allows, with a <b>120 floor</b> added so a very short surface still shows caption + ~3 rows. Implementer: split the single <InlineCode>ScrollArea</InlineCode> in <InlineCode>explorer.rs sidebar()</InlineCode> into two — <InlineCode>ScrollArea::vertical().id_salt("exp_tree")</InlineCode> in the top allocation and <InlineCode>id_salt("exp_favs")</InlineCode> inside a bottom-up allocated strip of the computed height — and draw the separator as the strip's top edge instead of an inline <InlineCode>ui.separator()</InlineCode>.</Note>
        </Spec>

        <Spec title="View modes — Grid · List · Detail">
          <Stage variant="tight">
            <div style={{ display: "flex", gap: 16, padding: 14, background: "var(--tasty-bg-panel)" }}>
              <ViewModeColumn title="Grid (icons)" sub="icon 16 + 3-line name (…); image = thumbnail slot"><ExpGridMini /></ViewModeColumn>
              <ViewModeColumn title="List" sub="small icon + name, one dense column"><ExpListMini /></ViewModeColumn>
              <ViewModeColumn title="Detail" sub="sortable columns: Name · Size · Date · Type"><div style={{ display: "flex", flexDirection: "column", flex: 1 }}><DetailHeader /><div style={{ flex: 1, background: "var(--tasty-bg-panel)" }}><DetailRow glyph={ic.folder} name="figma-exports" size="—" date="06-20 14:30" type="Folder" /><DetailRow glyph={ic.file} name="report.pdf" size="2.4 MB" date="06-24 09:12" type="PDF" /><DetailRow glyph={ic.image} name="diagram.png" size="488 KB" date="06-26 18:05" type="PNG" state="selected" glyphColor="var(--tasty-accent-info)" /></div></div></ViewModeColumn>
            </div>
          </Stage>
          <Meta
            specs={[["grid cell", "80px · icon 16 + 3-line label wrap (…)"], ["list row", "24px · icon + name"], ["detail row", "26px · 4 columns"], ["cell states", "default · hover (8%) · selected (12%) · cut (50%) · renaming"], ["sort", "click a column header → ▲ / ▼"]]}
            tokens={[{ tok: "--tasty-overlay-hover", use: "row hover (8%)" }, { tok: "--tasty-surface-active", use: "selected (12%)", color: "var(--tasty-surface-active)" }, { tok: "--tasty-accent-info", use: "image-file glyph", color: "var(--tasty-accent-info)" }]} />
          <Note><b>Grid cell</b> (updated): icon shrinks to <span className="tok">icon_glyph_size_md</span> (16, was 28) and the label wraps up to <b>3 lines</b> at <span className="tok">font_size_caption</span> (11) with <b>… overflow</b> on the last line — replacing the old 12-char hard cut. Height is <b>fixed to 3 label lines</b> so grid rows stay uniform (short names reserve the space, label top-aligned). CELL_W stays 80. Real side: <InlineCode>LayoutJob</InlineCode> with <InlineCode>wrap.max_rows = 3</InlineCode> + <InlineCode>overflow_character = '…'</InlineCode>, drop <InlineCode>truncate(&amp;e.name, 12)</InlineCode>, recompute <InlineCode>cell_h</InlineCode>. <b>Cut</b> items dim to ~50% until pasted. A <b>renaming</b> cell swaps its label for an inline Input (extension preserved). Image cells show a thumbnail in the icon slot at implementation time — here a tinted glyph stands in.</Note>
        </Spec>

        <Spec title="Right-click context menu — target resolves to 4 shapes"
          when={<>Finder/VS Code rule: right-clicking an item <b>inside the selection</b> targets the whole selection; <b>outside</b> it, selection collapses to that one item; on the <b>empty background</b>, the target is the current directory (cwd). Menu items appear/disappear accordingly, grouped by divider into <b>clipboard · file ops · system</b>.</>}>
          <Stage variant="solo center" style={{ padding: 20, background: "var(--tasty-bg-app)" }}>
            <div style={{ display: "flex", gap: 18, flexWrap: "wrap", justifyContent: "center" }}>
              <CtxMenu title="Background (empty area) → cwd">
                <MenuItem label="Copy path" icon={ic.link} />
                <MenuItem label="Add to favorites" icon={ic.star} />
                <MenuItem label="Paste" icon={ic.paste} shortcut={<Kbd keys="Ctrl+V" />} />
              </CtxMenu>
              <CtxMenu title="Single file">
                <MenuItem label="Copy path" icon={ic.link} />
                <MenuItem label="Copy" icon={ic.copy} shortcut={<Kbd keys="Ctrl+C" />} />
                <MenuItem label="Cut" icon={ic.scissors} shortcut={<Kbd keys="Ctrl+X" />} />
                <MenuItem separator />
                <MenuItem label="Rename" icon={ic.rename} shortcut={<Kbd keys="F2" />} />
                <MenuItem label="Move to Trash" icon={ic.trash} danger shortcut={<Kbd keys="Del" />} />
              </CtxMenu>
              <CtxMenu title="Single folder">
                <MenuItem label="Copy path" icon={ic.link} />
                <MenuItem label="Add to favorites" icon={ic.star} />
                <MenuItem label="Copy" icon={ic.copy} shortcut={<Kbd keys="Ctrl+C" />} />
                <MenuItem label="Cut" icon={ic.scissors} shortcut={<Kbd keys="Ctrl+X" />} />
                <MenuItem label="Paste into" icon={ic.paste} />
                <MenuItem separator />
                <MenuItem label="Rename" icon={ic.rename} shortcut={<Kbd keys="F2" />} />
                <MenuItem label="Move to Trash" icon={ic.trash} danger shortcut={<Kbd keys="Del" />} />
                <MenuItem separator />
                <MenuItem label="Open in system" icon={ic.external} />
              </CtxMenu>
              <CtxMenu title="Multiple selected (3)">
                <MenuItem label="Copy paths" icon={ic.link} />
                <MenuItem label="Copy" icon={ic.copy} shortcut={<Kbd keys="Ctrl+C" />} />
                <MenuItem label="Cut" icon={ic.scissors} shortcut={<Kbd keys="Ctrl+X" />} />
                <MenuItem separator />
                <MenuItem label="Move to Trash" icon={ic.trash} danger shortcut={<Kbd keys="Del" />} />
              </CtxMenu>
            </div>
          </Stage>
          <Meta
            specs={[["target", "in-selection → all · out → that one · empty → cwd"], ["favorites", "folder + cwd only"], ["paste", "shown when the clipboard holds files"], ["open in system", "single folder only"], ["groups", "clipboard · file ops · system (dividers)"]]}
            tokens={[{ tok: "--tasty-menu-bg", use: "menu fill", color: "var(--tasty-menu-bg)" }, { tok: "--tasty-menu-border", use: "1px edge" }, { tok: "--tasty-shadow-popover", use: "float" }, { tok: "--tasty-accent-danger", use: "Trash (danger item)", color: "var(--tasty-accent-danger)" }]} />
          <Note>Reuses the shared <span className="ic">MenuItem</span> / Popup visual language — no bespoke menu chrome. Shortcut hints are representative; real keys come from Keybindings settings.</Note>
        </Spec>

        <Spec title="Empty / permission / loading · favorite + rename popups"
          when={<>Status screens fill the content area; the two small editors reuse the Popup/rename visual language.</>}>
          <Stage variant="solo center" style={{ padding: 20, background: "var(--tasty-bg-app)", flexDirection: "column", gap: 16 }}>
            <div style={{ display: "flex", gap: 12, width: "100%", maxWidth: 700 }}>
              <ExpState glyph={ic.folderOpen} title="This folder is empty" />
              <ExpState glyph={ic.lock} glyphColor="var(--tasty-accent-warning)" title="Permission denied" sub="You don't have access to read this folder." />
              <ExpState glyph={<Spinner />} title="Loading…" />
            </div>
            <div style={{ display: "flex", gap: 18, flexWrap: "wrap", justifyContent: "center" }}>
              <div style={{ width: 300, background: "var(--tasty-bg-panel)", border: "1px solid var(--tasty-border-strong)", borderRadius: "var(--tasty-radius)", boxShadow: "var(--tasty-shadow-modal)", overflow: "hidden" }}>
                <div style={{ padding: "12px 14px 10px" }}>
                  <div style={{ fontSize: 14, fontWeight: 600, marginBottom: 10 }}>Add to favorites</div>
                  <div style={{ fontSize: 11, color: "var(--tasty-text-muted)", marginBottom: 4 }}>Path</div>
                  <div style={{ fontSize: 12, fontFamily: "var(--tasty-font-mono)", color: "var(--tasty-text-secondary)", marginBottom: 10 }}>~/Downloads</div>
                  <div style={{ fontSize: 11, color: "var(--tasty-text-muted)", marginBottom: 4 }}>Name</div>
                  <Input block defaultValue="Downloads" />
                </div>
                <div style={{ display: "flex", justifyContent: "flex-end", gap: 8, padding: "0 14px 12px" }}>
                  <Button variant="ghost" size="sm">Cancel</Button><Button variant="primary" size="sm">Add</Button>
                </div>
              </div>
              <div style={{ width: 300, background: "var(--tasty-bg-panel)", border: "1px solid var(--tasty-border-strong)", borderRadius: "var(--tasty-radius)", boxShadow: "var(--tasty-shadow-modal)", overflow: "hidden" }}>
                <div style={{ padding: "12px 14px 10px" }}>
                  <div style={{ fontSize: 14, fontWeight: 600, marginBottom: 2 }}>Rename</div>
                  <div style={{ fontSize: 12, color: "var(--tasty-text-muted)", marginBottom: 10 }}>Press <Kbd keys="↵" /> to confirm, <Kbd keys="Esc" /> to cancel.</div>
                  <Input block autoFocus defaultValue="diagram.png" />
                  <div style={{ fontSize: 11, color: "var(--tasty-text-muted)", marginTop: 6 }}>Extension <b style={{ color: "var(--tasty-text-secondary)" }}>.png</b> preserved.</div>
                </div>
                <div style={{ display: "flex", justifyContent: "flex-end", gap: 8, padding: "0 14px 12px" }}>
                  <Button variant="ghost" size="sm">Cancel</Button><Button variant="primary" size="sm">Rename</Button>
                </div>
              </div>
            </div>
          </Stage>
          <Meta
            specs={[["empty", "centered glyph + line"], ["permission", "warning (peach) tone"], ["loading", <>Spinner — busy-indicator policy</>], ["popups", "Popup language · ghost + primary footer"]]}
            tokens={[{ tok: "--tasty-accent-warning", use: "permission tone", color: "var(--tasty-accent-warning)" }, { tok: "--tasty-spinner-indicator", use: "loading", color: "var(--tasty-spinner-indicator)" }, { tok: "--tasty-shadow-modal", use: "popup lift" }]} />
          <Note>Explorer font (family + size, 14px cap) gets an <b>Appearance › Explorer</b> sub-tab mirroring the Terminal/Markdown font sections — same control layout, deferred to the Settings specimen.</Note>
        </Spec>
      </Section>

      <Section id="markdown" title="Markdown viewer">
        <Spec title="Markdown — read-only viewer (address bar + body)"
          when={<><b style={{ color: "var(--tasty-accent-attention)" }}>Library-driven content (parity exception).</b> The markdown surface is an <b>address-bar chrome</b> over a single vertical scroll body that fills the tile (body on <span className="tok">--tasty-md-doc-bg</span> = crust, reloads on file change). The <b>chrome keeps strict pixel parity</b>; the <b>rendered content below it does not</b> — it renders through <code>egui_commonmark</code>, so the heading size ladder, paragraph leading, and block spacing follow the <b>library's form</b>. What parity binds in the content is <b>color only</b>: Tasty tokens are injected into <code>egui::Visuals</code> (mapping in the next specimen). The drawing below is an <i>approximate</i> reference, not a pixel spec.</>}>
          <Stage variant="solo center" style={{ padding: 20, background: "var(--tasty-bg-app)" }}>
            <MarkdownDoc />
          </Stage>
          <Meta
            specs={[["chrome", <>40px address bar + Go, on <span className="tok">--tasty-bg-sidebar</span></>], ["body", <>13px · line-height 1.6 (library-owned)</>], ["h1", "20px / 700 / primary (content, cap-exempt)"], ["h2", "14px / 700 / primary"], ["h3", "14px / 600 / primary"], ["h4–h6", "13px · weight + color + UPPER steps"], ["code", <>mono on <span className="tok">--tasty-surface-raised</span></>], ["link", <span className="tok">--tasty-accent-primary</span>], ["table", <>grid + zebra, read-only — <span className="tok">--tasty-md-table-*</span></>]]}
            tokens={[{ tok: "--tasty-bg-sidebar", use: "chrome bar", color: "var(--tasty-bg-sidebar)" }, { tok: "--tasty-md-doc-bg", use: "document bed (crust)", color: "var(--tasty-md-doc-bg)" }, { tok: "--tasty-input-bg", use: "address field", color: "var(--tasty-input-bg)" }, { tok: "--tasty-text-secondary", use: "body + headings", color: "var(--tasty-text-secondary)" }, { tok: "--tasty-surface-raised", use: "code bg + inline chip", color: "var(--tasty-surface-raised)" }, { tok: "--tasty-accent-primary", use: "links + Go", color: "var(--tasty-accent-primary)" }, { tok: "--tasty-md-table-border", use: "table grid (surface1)", color: "var(--tasty-md-table-border)" }, { tok: "--tasty-md-table-header-bg", use: "table header (surface0)", color: "var(--tasty-md-table-header-bg)" }, { tok: "--tasty-md-table-row-bg-zebra", use: "zebra row (mantle)", color: "var(--tasty-md-table-row-bg-zebra)" }, { tok: "--tasty-font-size-prose-h1", use: "h1 (cap-exempt)" }]} />
          <Note>Reverses the earlier “no toolbar” decision: the surface now leads with a browser-style <b>address bar</b> (path display/edit) + <b>Go</b> button — <b>chrome, so it stays pixel-parity</b>. The body is <b>library-driven</b> (egui_commonmark): sizes/leading/spacing are the library's, only color is injected. The removed/redefined content tokens (<span className="tok">--tasty-font-size-prose-h2</span>, <span className="tok">--tasty-line-height-prose</span> now removed; <span className="tok">--tasty-font-size-prose-h1</span> → Heading anchor) are covered in the heading-hierarchy specimen.</Note>
        </Spec>
        <Spec title="Markdown — document background (single bed, webview render path)"
          when={<>Under the webview render channel the markdown body is an HTML/CSS document drawn by the host WebView — there is <b>no focused/unfocused signal</b> on that path, so markdown has exactly <b>one</b> background (unlike the terminal, which swaps two). That bed is <span className="tok">--tasty-md-doc-bg</span> → <span className="tok">--tasty-surface-markdown-focused-bg</span> = <b>crust</b>. Pure black is a <i>terminal</i> convention (raw TTY) and sits outside the Catppuccin ramp; prose at the 14px cap on #000 halates. crust is the palette's deepest tone, so every content fill still steps up and stays discriminable: <b>crust</b> (page) &lt; <b>mantle</b> (table zebra) &lt; <b>base</b> &lt; <b>surface0</b> (code + table header) &lt; <b>surface1</b> (table grid). mantle would have swallowed the zebra stripe; base would have flattened the table's own fill against the page.</>}>
          <Stage variant="solo center" style={{ padding: 20, background: "var(--tasty-bg-app)", flexDirection: "column", gap: 12 }}>
            <div style={{ display: "flex", gap: 10, flexWrap: "wrap", justifyContent: "center" }}>
              {[["crust — chosen", "var(--tasty-color-neutral-0)", true], ["mantle — collides with zebra", "var(--tasty-color-neutral-100)", false], ["base — flattens table fill", "var(--tasty-color-neutral-200)", false], ["#000 — terminal only", "var(--tasty-color-black)", false]].map(([label, c, on]) => (
                <div key={label} style={{ width: 148, display: "flex", flexDirection: "column", gap: 6 }}>
                  <div style={{ height: 60, background: c, border: on ? "1px solid var(--tasty-accent-primary)" : "1px solid var(--tasty-border-default)", borderRadius: "var(--tasty-radius)", display: "flex", flexDirection: "column", justifyContent: "flex-end", padding: 6, gap: 3 }}>
                    <div style={{ height: 10, background: "var(--tasty-md-table-row-bg-zebra)" }} />
                    <div style={{ height: 10, background: "var(--tasty-md-code-bg)" }} />
                  </div>
                  <div style={{ fontSize: 11, color: on ? "var(--tasty-text-primary)" : "var(--tasty-text-muted)" }}>{label}</div>
                </div>
              ))}
            </div>
          </Stage>
          <Meta
            specs={[["backgrounds", "one (no focus swap on the webview path)"], ["mocha", "crust #11111b"], ["latte", "crust #dce0e8 (tracks bg-app, no branch)"], ["terminal", "unchanged — pure black stays terminal-only"], ["unfocused_bg", "unused by the webview renderer"]]}
            tokens={[{ tok: "--tasty-md-doc-bg", use: "document bed → --md-bg", color: "var(--tasty-md-doc-bg)" }, { tok: "--tasty-md-code-bg", use: "code fill → --md-code-bg", color: "var(--tasty-md-code-bg)" }, { tok: "--tasty-md-table-row-bg-zebra", use: "zebra → --md-zebra", color: "var(--tasty-md-table-row-bg-zebra)" }, { tok: "--tasty-md-table-border", use: "grid → --md-border", color: "var(--tasty-md-table-border)" }, { tok: "--tasty-md-quote-bar", use: "blockquote bar", color: "var(--tasty-md-quote-bar)" }, { tok: "--tasty-md-rule", use: "horizontal rule", color: "var(--tasty-md-rule)" }]} />
          <Note>Resolves the copy-paste value in <code>[surfaces.markdown].focused_bg</code> (was <code>#000000</code>, identical to terminal). Implementing side: set mocha <code>focused_bg = crust</code>, drop the Latte white override, and keep <code>unfocused_bg</code> as-is — it is dead on the webview path.</Note>
        </Spec>
        <Spec title="Markdown — content color injection (library-driven)"
          when={<>The <b>one thing parity binds in rendered markdown content</b>: color. <code>egui_commonmark</code> reads every color from <code>egui::Visuals</code>, so we override those fields with Tasty theme tokens and the content matches the palette 1:1 across Mocha/Latte — while <b>size ladder, leading, and block spacing stay the library's</b>. Left: the Visuals → token map. Right: the syntect code-highlight theme, keyed to the Catppuccin-derived palette. This mapping (not a pixel drawing) is the source of truth for the source-side transition.</>}>
          <Stage variant="solo center" style={{ padding: 20, background: "var(--tasty-bg-app)" }}>
            <div style={{ width: "100%", maxWidth: 720 }}><MdInjectionMap /></div>
          </Stage>
          <Meta
            specs={[["binds", "color only (injected into egui::Visuals)"], ["library-owned", "heading sizes · leading · block spacing"], ["body", <>override_text_color → <span className="tok">--tasty-text-secondary</span></>], ["strong/heading", <>strong_text_color → <span className="tok">--tasty-text-primary</span></>], ["code bg", <>code_bg / extreme_bg → <span className="tok">--tasty-surface-raised</span></>], ["code theme", "syntect · Tasty palette (right)"]]}
            tokens={[{ tok: "--tasty-text-secondary", use: "body", color: "var(--tasty-text-secondary)" }, { tok: "--tasty-text-primary", use: "headings / strong", color: "var(--tasty-text-primary)" }, { tok: "--tasty-accent-primary", use: "links", color: "var(--tasty-accent-primary)" }, { tok: "--tasty-surface-raised", use: "code fills", color: "var(--tasty-surface-raised)" }, { tok: "--tasty-border-strong", use: "blockquote bar", color: "var(--tasty-border-strong)" }, { tok: "--tasty-separator", use: "code-block border", color: "var(--tasty-separator)" }]} />
          <Note>Code tokens use the palette roles (keyword = <span className="tok">--tasty-color-mauve</span>, string = <span className="tok">--tasty-color-green</span>, function = <span className="tok">--tasty-color-blue</span>, number = <span className="tok">--tasty-color-peach</span>, type = <span className="tok">--tasty-color-yellow</span>, comment = <span className="tok">--tasty-text-muted</span> italic) — supplied to a syntect theme built from the Tasty palette, not syntect's default. Both themes (Mocha/Latte) inherit automatically because the roles re-point per theme.</Note>
        </Spec>
        <Spec title="Address bar states · large-file confirm"
          when={<>The address bar is a browser-style path field. <b>Display</b> shows the current file path (mono, secondary); <b>click</b> to edit (focus ring + caret) and type a new path — <span className="ic">↵</span> or the <b>Go</b> button opens it in place, <span className="ic">Esc</span> reverts to the original. A non-<code>.md</code> extension still opens as markdown. Opening a file <b>over 1 MB</b> first raises a <b>surface-scoped</b> confirm — clamped to the tile (not a window scrim), same 360px shell as <code>markdown_open</code> — with Open / Cancel; Cancel opens nothing.</>}>
          <Stage variant="solo center" style={{ padding: 20, background: "var(--tasty-bg-app)", flexDirection: "column", gap: 16 }}>
            <div style={{ width: 440, maxWidth: "100%", display: "flex", flexDirection: "column", gap: 10 }}>
              <div><div style={{ fontSize: 11, color: "var(--tasty-text-muted)", marginBottom: 6 }}>display (idle)</div>
                <div style={{ border: "1px solid var(--tasty-border-default)", borderRadius: "var(--tasty-radius)", overflow: "hidden" }}><MdAddressBar /></div></div>
              <div><div style={{ fontSize: 11, color: "var(--tasty-text-muted)", marginBottom: 6 }}>editing (focus)</div>
                <div style={{ border: "1px solid var(--tasty-border-default)", borderRadius: "var(--tasty-radius)", overflow: "hidden" }}><MdAddressBar editing path="~/work/tasty/docs/design/systems/theme.md" /></div></div>
            </div>
            <div style={{ position: "relative", width: 440, maxWidth: "100%", height: 232, background: "var(--tasty-bg-panel)",
              border: "1px solid var(--tasty-border-default)", borderRadius: "var(--tasty-radius)", overflow: "hidden" }}>
              <MdAddressBar path="~/work/tasty/docs/CHANGELOG-full.md" />
              <div style={{ padding: "14px 18px", opacity: 0.35 }}><div style={{ ...MD_H[1], margin: 0 }}>Changelog</div><div style={mdBody}>A very long history…</div></div>
              <div style={{ position: "absolute", inset: "40px 0 0 0", background: "rgba(0,0,0,.5)", display: "flex", alignItems: "center", justifyContent: "center", padding: 12 }}>
                <MdLargeFilePopup />
              </div>
            </div>
          </Stage>
          <Meta
            specs={[["field", "mono path · click to edit"], ["idle", <>secondary text, <span className="tok">--tasty-input-border</span></>], ["editing", <>focus ring + caret, <span className="tok">--tasty-input-border-focus</span></>], ["Go", "IconButton · ↵ also confirms"], ["popup scope", "surface (clamped to tile)"], ["popup", "360px · Open / Cancel"]]}
            tokens={[{ tok: "--tasty-input-bg", use: "field fill", color: "var(--tasty-input-bg)" }, { tok: "--tasty-input-border-focus", use: "edit border", color: "var(--tasty-input-border-focus)" }, { tok: "--tasty-border-focus", use: "focus ring", color: "var(--tasty-border-focus)" }, { tok: "--tasty-accent-warning", use: "size chip", color: "var(--tasty-accent-warning)" }, { tok: "--tasty-shadow-modal", use: "popup lift" }, { tok: "--tasty-accent-primary", use: "Open (primary)", color: "var(--tasty-accent-primary)" }]} />
          <Note>The address bar is the <b>shared editable path field</b> (see Explorer section) with a <span className="ic">file</span> leading icon — same field, same edit/navigate/revert contract as the Explorer toolbar, and the same edit-time <b><a href="components.html#forms">AutoComplete</a></b> dropdown (candidates = recent files). Copy: title “Open large file?” · body “Over 1 MB — rendering may be slow.” · size chip formatted (e.g. “3.2 MB”). Surface-scoped (<code>PopupScope::Surface</code>) so it dims only this tile; Cancel replaces nothing. New i18n keys when wiring: <code>markdown.large_file.title / body</code>.</Note>
        </Spec>
      </Section>

      <Section id="html" title="HTML viewer">
        <Spec title="Markdown — heading hierarchy (library-driven)"
          when={<><b style={{ color: "var(--tasty-accent-attention)" }}>Library-driven — not a pixel spec.</b> Under <code>egui_commonmark</code>, heading sizes are <b>interpolated by the library</b> between the <b>Heading anchor</b> (<span className="tok">--tasty-font-size-prose-h1</span> = 20, the only cap-exempt content size) and Body — individual per-level sizes are <b>not</b> set by us (that's why <span className="tok">--tasty-font-size-prose-h2</span> is retired). Weight and color still injected. The ladder below is an <i>approximate</i> reference of the resulting shape, not coordinates to transcribe.</>}>
          <Stage variant="solo center" style={{ padding: 20, background: "var(--tasty-bg-app)" }}>
            <div style={{ background: "var(--tasty-bg-panel)", border: "1px solid var(--tasty-border-default)", borderRadius: "var(--tasty-radius)", padding: 24 }}>
              <TypeScaleSheet />
            </div>
          </Stage>
          <Meta
            specs={[["ladder", "library-interpolated (Heading anchor → Body)"], ["h1", "prose-h1 (20) · Heading anchor · cap-exempt"], ["injected", "weight + color only"], ["h2 token", "retired (library sets intermediate levels)"], ["leading", "library-owned (line-height-prose retired)"], ["small", "11 · muted"]]}
            tokens={[{ tok: "--tasty-font-size-prose-h1", use: "Heading anchor (cap-exempt)" }, { tok: "--tasty-text-primary", use: "headings / strong", color: "var(--tasty-text-primary)" }, { tok: "--tasty-text-secondary", use: "body", color: "var(--tasty-text-secondary)" }, { tok: "--tasty-text-muted", use: "de-emphasis", color: "var(--tasty-text-muted)" }]} />
          <Note>Load-fail / empty / loading states (proposed): a peach-toned <b>"Failed to load"</b> instead of raw <InlineCode>Error:</InlineCode> text, a centered <b>"This file is empty"</b>, and the Spinner for slow loads — all in the Explorer states pattern above. Markdown font gets its own <b>Appearance › Markdown</b> sub-tab. Size/leading here are the library's — only color + weight are ours.</Note>
        </Spec>
      </Section>

      <Section id="html" title="HTML viewer">
        <Spec title="HTML — webview chrome (4 states)"
          when={<>The html surface is <b>thin chrome + a native OS WebView overlay</b> — tasty does <b>not</b> theme the page pixels. What tasty paints is the tile skeleton before/instead of the overlay: <b>boundary</b> (region marker), <b>placeholder</b> (no URL), <b>loading</b> (Spinner), <b>error</b> (danger). Same tile (<span className="tok">--tasty-bg-panel</span> + 1px border), centered content. Toolbar / address bar / nav are <b>not</b> drawn today — left out per the brief (not guessed).</>}>
          <Stage variant="solo center" style={{ padding: 20, background: "var(--tasty-bg-app)", gap: 14, flexWrap: "wrap" }}>
            <div style={{ display: "flex", flexDirection: "column", gap: 6 }}><div style={{ fontSize: 11, color: "var(--tasty-text-muted)" }}>boundary</div><HtmlTile state="boundary" /></div>
            <div style={{ display: "flex", flexDirection: "column", gap: 6 }}><div style={{ fontSize: 11, color: "var(--tasty-text-muted)" }}>placeholder</div><HtmlTile state="placeholder" /></div>
            <div style={{ display: "flex", flexDirection: "column", gap: 6 }}><div style={{ fontSize: 11, color: "var(--tasty-text-muted)" }}>loading</div><HtmlTile state="loading" /></div>
            <div style={{ display: "flex", flexDirection: "column", gap: 6 }}><div style={{ fontSize: 11, color: "var(--tasty-text-muted)" }}>error</div><HtmlTile state="error" /></div>
          </Stage>
          <Meta
            specs={[["content", "native OS WebView (token-agnostic)"], ["tile", <><span className="tok">--tasty-bg-panel</span> + 1px <span className="tok">--tasty-border-default</span></>], ["boundary", "GLOBE + region label + URL"], ["placeholder", "GLOBE (disabled) + 'No page loaded'"], ["loading", "Spinner + 'Loading…'"], ["error", "ALERT (danger) + URL"]]}
            tokens={[{ tok: "--tasty-text-muted", use: "boundary glyph/label", color: "var(--tasty-text-muted)" }, { tok: "--tasty-text-disabled", use: "placeholder / URL", color: "var(--tasty-text-disabled)" }, { tok: "--tasty-spinner-indicator", use: "loading", color: "var(--tasty-spinner-indicator)" }, { tok: "--tasty-accent-danger", use: "error", color: "var(--tasty-accent-danger)" }]} />
        </Spec>
        <Spec title="HTML — settings (Appearance › HTML viewer)">
          <Stage variant="solo center" style={{ padding: 20, background: "var(--tasty-bg-app)" }}>
            <HtmlSettings />
          </Stage>
          <Meta
            specs={[["pattern", "plugin settings page (label + control rows)"], ["default zoom", "number + % suffix"], ["color scheme", "select · follow / light / dark"], ["allow remote", "switch (off default)"], ["sandbox scripts", "switch (on default)"]]}
            tokens={[{ tok: "--tasty-select-bg", use: "scheme select", color: "var(--tasty-select-bg)" }, { tok: "--tasty-switch-track-bg-on", use: "enabled toggle", color: "var(--tasty-switch-track-bg-on)" }, { tok: "--tasty-input-bg", use: "zoom field", color: "var(--tasty-input-bg)" }]} />
          <Note>Reuses the existing plugin-settings row vocabulary (switch 28×16 · select · number) — no bespoke controls. Toolbar / address bar / find-in-page / retry stay <b>open decisions</b> (not in this pass).</Note>
        </Spec>
      </Section>

      <Section id="image" title="Image viewer">
        <Spec title="Image — viewer & edit (paint) modes"
          when={<>The image surface is a single horizontal <b>control bar</b> over a <b>canvas</b>. Two modes toggle the bar: <b>viewer</b> (◀ ▶ ↻ ✎ ＋ · filename (i/n) · right-aligned zoom group) and <b>edit / paint</b> (Save · Cancel · undo/redo · brush size · color · zoom). Canvas fills with the <span className="tok">--tasty-bg-sidebar</span> tone so the image floats; ≤100% fits to window, &gt;100% pans on drag. ◀ ▶ show only when sibling images exist.</>}>
          <Stage variant="solo center" style={{ padding: 20, background: "var(--tasty-bg-app)", gap: 16, flexWrap: "wrap" }}>
            <div style={{ display: "flex", flexDirection: "column", gap: 6 }}><div style={{ fontSize: 11, color: "var(--tasty-text-muted)" }}>viewer · fit (100%)</div><ImgSurface mode="viewer" /></div>
            <div style={{ display: "flex", flexDirection: "column", gap: 6 }}><div style={{ fontSize: 11, color: "var(--tasty-text-muted)" }}>viewer · zoomed + panned (250%)</div><ImgSurface mode="viewer" panned /></div>
          </Stage>
          <Meta
            specs={[["layout", "control bar (40px) + canvas"], ["viewer bar", "◀ ▶ ↻ ✎ ＋ · name (i/n) · zoom"], ["canvas", <><span className="tok">--tasty-bg-sidebar</span> · fit ≤100% · pan &gt;100%</>], ["zoom group", "Fit · ＋ · % · −  (right)"], ["buttons", <>surface-raised + <span className="tok">--tasty-border-default</span></>]]}
            tokens={[{ tok: "--tasty-bg-sidebar", use: "canvas (mantle)", color: "var(--tasty-bg-sidebar)" }, { tok: "--tasty-surface-raised", use: "control buttons", color: "var(--tasty-surface-raised)" }, { tok: "--tasty-text-muted", use: "filename · zoom %", color: "var(--tasty-text-muted)" }]} />
        </Spec>
        <Spec title="Image — paint bar · floating selection · popups"
          when={<>Edit mode swaps in the paint bar (undo/redo with disabled state, brush-size slider, color swatch defaulting to <span className="tok">--tasty-accent-danger</span>). A pasted image becomes a <b>floating selection</b> — accent border + 8 resize handles — that you move/resize before committing. <b>New Image</b> and <b>Save As</b> are small popups.</>}>
          <Stage variant="solo center" style={{ padding: 20, background: "var(--tasty-bg-app)", gap: 16, flexWrap: "wrap" }}>
            <div style={{ display: "flex", flexDirection: "column", gap: 6 }}><div style={{ fontSize: 11, color: "var(--tasty-text-muted)" }}>edit · floating selection</div><ImgSurface mode="edit" floating /></div>
            <div style={{ display: "flex", flexDirection: "column", gap: 12 }}>
              <div style={{ width: 300, background: "var(--tasty-bg-panel)", border: "1px solid var(--tasty-border-strong)", borderRadius: "var(--tasty-radius)", boxShadow: "var(--tasty-shadow-modal)", overflow: "hidden" }}>
                <div style={{ padding: "12px 14px 10px" }}>
                  <div style={{ fontSize: 14, fontWeight: 600, marginBottom: 10 }}>New Image</div>
                  <div style={{ display: "flex", alignItems: "center", gap: 8, fontSize: 13, color: "var(--tasty-text-secondary)" }}>
                    <span>Width</span><Input style={{ width: 64, flex: "none" }} defaultValue="800" /><span style={{ color: "var(--tasty-text-muted)" }}>×</span><span>Height</span><Input style={{ width: 64, flex: "none" }} defaultValue="600" />
                  </div>
                </div>
                <div style={{ display: "flex", justifyContent: "flex-end", gap: 8, padding: "0 14px 12px" }}><Button variant="ghost" size="sm">Cancel</Button><Button variant="primary" size="sm">OK</Button></div>
              </div>
              <div style={{ width: 300, background: "var(--tasty-bg-panel)", border: "1px solid var(--tasty-border-strong)", borderRadius: "var(--tasty-radius)", boxShadow: "var(--tasty-shadow-modal)", overflow: "hidden" }}>
                <div style={{ padding: "12px 14px 10px" }}>
                  <div style={{ fontSize: 14, fontWeight: 600, marginBottom: 10 }}>Save As</div>
                  <div style={{ display: "flex", alignItems: "center", gap: 6 }}>
                    <div style={{ flex: 1 }}><Input block placeholder="path/to/image.png" /></div>
                    <IconButton size="sm" aria-label="Browse">{ic.folderOpen}</IconButton>
                  </div>
                </div>
                <div style={{ display: "flex", justifyContent: "flex-end", gap: 8, padding: "0 14px 12px" }}><Button variant="ghost" size="sm">Cancel</Button><Button variant="primary" size="sm">Save</Button></div>
              </div>
            </div>
          </Stage>
          <Meta
            specs={[["paint bar", "Save · Cancel · ↶ ↷ · brush · color · zoom"], ["undo/redo", "enabled / disabled (text-disabled)"], ["floating sel", "accent border + 8 handles (6px)"], ["commit", "click outside = composite · Esc = cancel"], ["New Image", "Width × Height (1–8192)"], ["Save As", "path input + browse · PNG"]]}
            tokens={[{ tok: "--tasty-accent-primary", use: "floating selection + handles", color: "var(--tasty-accent-primary)" }, { tok: "--tasty-accent-danger", use: "default brush color", color: "var(--tasty-accent-danger)" }, { tok: "--tasty-surface-active", use: "brush slider track", color: "var(--tasty-surface-active)" }, { tok: "--tasty-shadow-modal", use: "popups" }]} />
          <Note>Defaults (brief §6): metadata status-bar, filmstrip, corrupt-image state, async loading indicator, rotate/flip/crop tools, and transparency checkerboard are <b>out of scope</b> this pass — viewer + brush/paste paint only. Load-fail / no-image share one centered <b>"No image loaded"</b> (muted).</Note>
        </Spec>
      </Section>
    </>
  );
}

window.Gallery.mount(
  "plugins",
  NAV,
  {
    title: "Plugin surfaces",
    intro: "Surface kinds contributed by bundled plugins — the Explorer file manager and the content viewers (Markdown · HTML · Image). These fill a work-area tile like the terminal does. Kept on their own page because plugin surfaces grow without bound; the general structural shells stay in Layouts.",
    howto: false,
  },
  <Page />
);
