// Tasty Gallery — Explorer file operations, part 4 (2026-10-10, batch 12):
// address takes file paths + remote check · hidden toggle on wide toolbars · create field width · auto-scroll and drag
// timing · Undo / Redo menu copy · counting stage · Properties rows · low preview. Loaded before explorer-ops.jsx.
const QDS = window.TastyDesignSystem_41fd3f;
const { Section: QSection, Spec: QSpec, Stage: QStage, Meta: QMeta, Note: QNote } = window.Gallery;
const { IconButton: QIconButton, Icon: QIcon, Toast: QToast, MenuItem: QMenuItem, Input: QInput, Spinner: QSpinner } = QDS;
const QK = window.ExplorerKit;
const { XCol, XThemes, XToolbar, XCell } = window.ExplorerOpsParts;

const qMenu = { width: "var(--tasty-size-240)", background: "var(--tasty-menu-bg)", border: "var(--tasty-border-width) solid var(--tasty-menu-border)", borderRadius: "var(--tasty-menu-radius)", padding: "var(--tasty-space-xs)", boxShadow: "var(--tasty-shadow-popover)" };
const qtb = (label, glyph, o = {}) => <QIconButton key={label} size="sm" aria-label={label} title={label} active={o.active}>{glyph}</QIconButton>;
const qSep = <span style={{ width: "var(--tasty-border-width)", height: "var(--tasty-icon-size-md)", background: "var(--tasty-separator)", margin: "0 var(--tasty-space-xs)" }} />;
const QROWS = [
  { glyph: QK.ic.folder, name: "Documents", size: "—", date: "2026-10-02 10:14", type: "Folder" },
  { glyph: QK.ic.folder, name: "mockup-exports", size: "—", date: "2026-06-20 14:30", type: "Folder" },
  { glyph: QK.ic.file, name: "report.pdf", size: "2.4 MB", date: "2026-06-24 09:12", type: "PDF document" },
  { glyph: QK.ic.image, name: "diagram.png", size: "488 KB", date: "2026-06-26 18:05", type: "PNG image", glyphColor: "var(--tasty-accent-info)" },
  { glyph: QK.ic.file, name: "notes.md", size: "12 KB", date: "2026-06-27 11:40", type: "Markdown" },
];
function QBody({ children }) {
  return <div style={{ flex: 1, minWidth: 0, display: "flex", flexDirection: "column", background: "var(--tasty-bg-panel)", position: "relative" }}><QK.DetailHeader /><div style={{ flex: 1, overflow: "hidden", position: "relative" }}>{children}</div></div>;
}
// Address field — idle / pending (remote check). Same box as PathField; trailing slot holds the spinner.
function QAddress({ path, pending, placeholder }) {
  return (
    <div style={{ flex: 1, minWidth: 0, display: "flex", alignItems: "center", gap: 6, height: "var(--tasty-control-height)", padding: "0 var(--tasty-space-sm)", background: "var(--tasty-input-bg)", borderRadius: "var(--tasty-radius)", border: "var(--tasty-border-width) solid " + (pending ? "var(--tasty-input-border-focus)" : "var(--tasty-input-border)") }}>
      <span style={{ display: "inline-flex", flex: "none", color: "var(--tasty-text-muted)" }}>{QK.ic.folderOpen}</span>
      <span style={{ flex: 1, minWidth: 0, fontFamily: "var(--tasty-font-mono)", fontSize: "var(--tasty-font-size-caption)", color: path ? "var(--tasty-text-secondary)" : "var(--tasty-text-placeholder)", overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>{path || placeholder}</span>
      {pending && <span style={{ display: "inline-flex", flex: "none", color: "var(--tasty-spinner-indicator)" }} title="Checking the remote path…"><QSpinner size="var(--tasty-explorer-address-pending-size)" /></span>}
    </div>
  );
}
function QToolbar({ address, hidden }) {
  return (
    <div style={{ display: "flex", alignItems: "center", gap: "var(--tasty-space-sm)", flex: "none", height: 44, padding: "0 var(--tasty-space-sm)", background: "var(--tasty-bg-panel)", borderBottom: "var(--tasty-border-width) solid var(--tasty-separator)" }}>
      <div style={{ display: "flex", alignItems: "center", gap: 1 }}>{qtb("Back", QK.ic.back)}{qtb("Up", QK.ic.up)}{qtb("Refresh", QK.ic.refresh)}</div>
      {address || <QAddress path="~/Downloads" />}
      <div style={{ display: "flex", alignItems: "center", gap: 1 }}>
        {qtb("New folder", <QIcon name="folderPlus" />)}{qtb("New file", <QIcon name="filePlus" />)}{qSep}
        {qtb("Find", <QIcon name="search" />)}{qtb("Preview panel", <QIcon name="columns" />)}
        {qtb(hidden ? "Hide hidden files (Ctrl+Shift+.)" : "Show hidden files (Ctrl+Shift+.)", <QIcon name="eye" />, { active: hidden })}
      </div>
      <QK.SegToggle value="detail" />
    </div>
  );
}
function QHiddenRow({ glyph, name, size, date, type }) {
  const fg = "var(--tasty-explorer-hidden-fg)";
  return <QK.DetailRow glyph={<span style={{ color: fg, display: "inline-flex" }}>{glyph}</span>} name={<span style={{ color: fg }}>{name}</span>} size={size} date={date} type={type} />;
}
function QCreateRow({ value }) {
  return (
    <div style={{ display: "flex", alignItems: "center", gap: "var(--tasty-space-sm)", height: "var(--tasty-table-cell-height)", padding: "0 10px", paddingLeft: "calc(10px + var(--tasty-explorer-create-indent))", background: "var(--tasty-surface-active)" }}>
      <span style={{ display: "inline-flex", flex: "none", color: "var(--tasty-text-muted)" }}>{QK.ic.file}</span>
      <span style={{ flex: "1 1 auto", minWidth: 0, maxWidth: "var(--tasty-explorer-create-field-max-width)" }}><QInput block defaultValue={value} /></span>
      <span style={{ flex: "none", fontSize: "var(--tasty-font-size-caption)", color: "var(--tasty-text-muted)", whiteSpace: "nowrap" }}>in mockup-exports</span>
    </div>
  );
}
const qToastAt = (node) => <div style={{ position: "absolute", right: "var(--tasty-space-sm)", bottom: "calc(var(--tasty-status-bar-height) + var(--tasty-space-sm))", maxWidth: "calc(100% - 2 * var(--tasty-space-sm))" }}>{node}</div>;

function FollowupsB12Section() {
  return (
    <QSection id="followups12" title="Follow-ups — address · hidden toggle · create field · auto-scroll · Undo / Redo menu (2026-10-10 batch 12)">
      <QSpec title="Address field — folders and files · remote check pending"
        when={<>The address field now takes <b>a folder or a file</b>; a file opens its folder and selects it. The placeholder and the not-found toast say <b>path</b>, not folder. <b>Remote</b> explorers must ask the remote whether the path is a file, so on Enter the field <b>keeps the typed path</b> (it does not snap back to the current folder), stays in its focus border and, if the answer has not come after <span className="tok">--tasty-explorer-address-pending-delay</span> 200 ms, shows a <b>Spinner</b> in the trailing slot. The list and the status line do not change: nothing has moved yet. The answer navigates; after 8 s without one the explorer goes to the typed path, as today. <b>Esc</b> during the check cancels it and restores the current path. Local explorers answer at once and never show the pending state.</>}>
        <QStage variant="solo center" style={{ padding: 20, background: "var(--tasty-bg-app)" }}>
          <XThemes render={(l) => (
            l === "Mocha"
              ? <XCell w={560} h={170} toolbar={<QToolbar address={<QAddress path="/srv/app/logs/2026-10-10.log" pending />} />} status="5 items"><QBody>{QROWS.slice(0, 3).map((r, i) => <QK.DetailRow key={i} {...r} />)}</QBody></XCell>
              : <XCell w={560} h={200} toolbar={<QToolbar address={<QAddress placeholder="Go to folder or file…" />} />} status="5 items" overlay={qToastAt(<QToast variant="danger">Path not found: ~/Downloads/report-final.pdf</QToast>)}><QBody>{QROWS.slice(0, 3).map((r, i) => <QK.DetailRow key={i} {...r} />)}</QBody></XCell>
          )} />
        </QStage>
        <QMeta
          specs={[["pending", "remote only · field keeps the typed path + focus border · Spinner 12 in the trailing slot after 200 ms · list + status unchanged · Esc cancels"], ["end", "answer → navigate (file: open folder + select) · 8 s → go to the typed path"], ["not found", "error toast, explorer scope · path = the confirmed absolute path"], ["placeholder", "“Go to folder or file…”"]]}
          tokens={[{ tok: "--tasty-explorer-address-pending-delay", use: "→ motion-ui-fade 200" }, { tok: "--tasty-explorer-address-pending-size", use: "→ icon-size-sm 12" }, { tok: "--tasty-spinner-indicator", use: "spinner", color: "var(--tasty-spinner-indicator)" }]} />
        <QNote>i18n — <code>explorer.address.not_found</code> “Path not found: {"{}"}” / “경로를 찾을 수 없습니다: {"{}"}” / “パスが見つかりません: {"{}"}” · <code>explorer.address.placeholder</code> “Go to folder or file…” / “폴더나 파일로 이동…” / “フォルダーまたはファイルへ移動…” · spinner tooltip <code>explorer.address.checking</code> “Checking the remote path…” / “원격 경로 확인 중…” / “リモートのパスを確認しています…”.</QNote>
      </QSpec>

      <QSpec title="Hidden files on a wide toolbar · create field width"
        when={<>A wide cell gets a <b>hidden files toggle</b> at the end of the view group (after Find and Preview); it folds into <b>More</b> with the rest of the group under <span className="tok">--tasty-explorer-toolbar-compact-below</span> 440. It is an IconButton with the <b>eye</b> glyph that shows <b>pressed</b> while hidden files are shown; its tooltip carries the action words and the shortcut, so the native More menu does not need a shortcut column. The <b>create-in-folder field</b> stops at <span className="tok">--tasty-explorer-create-field-max-width</span> 240 and the muted “in {"{folder}"}” follows it at space-sm, in Detail and List alike. <b>Grid</b> shows no caption (the target cell's drop-target ring already says where). The target ring uses the <b>drag-and-drop ring</b> — radius, not radius-sm — so the two never differ.</>}>
        <QStage variant="solo center" style={{ padding: 20, background: "var(--tasty-bg-app)" }}>
          <XThemes render={(l) => (
            <XCell w={640} h={200} toolbar={<QToolbar hidden={l === "Mocha"} />} status={l === "Mocha" ? "8 items" : "5 items"}><QBody>
              {l === "Mocha" && <><QHiddenRow glyph={QK.ic.folder} name=".git" size="—" date="2026-10-09 18:12" type="Folder" /><QHiddenRow glyph={QK.ic.file} name=".env" size="210 B" date="2026-10-01 07:45" type="ENV file" /></>}
              {l === "Latte" && <><QK.DetailRow {...QROWS[0]} /><div style={{ background: "var(--tasty-explorer-drop-target-bg)", boxShadow: "inset 0 0 0 var(--tasty-border-width) var(--tasty-explorer-drop-target-border)", borderRadius: "var(--tasty-radius)" }}><QK.DetailRow {...QROWS[1]} /></div><QCreateRow value="untitled.txt" /></>}
              <QK.DetailRow {...QROWS[2]} />
            </QBody></XCell>
          )} />
        </QStage>
        <QMeta
          specs={[["toggle", "IconButton sm · eye · pressed (active) while shown · last in the view group · folds into More under 440"], ["tooltip", "“Show hidden files (Ctrl+Shift+.)” / “Hide hidden files (Ctrl+Shift+.)” · macOS (⌘⇧.)"], ["More row", "native menu, no shortcut column — fine"], ["create field", "max explorer-create-field-max-width 240 · caption at space-sm right after it"], ["Grid", "no caption"], ["target ring", "same as the drop ring · radius (4)"]]}
          tokens={[{ tok: "--tasty-explorer-create-field-max-width", use: "→ size-240" }, { tok: "--tasty-explorer-hidden-fg", use: "shown hidden items", color: "var(--tasty-explorer-hidden-fg)" }, { tok: "--tasty-explorer-drop-target-border", use: "target", color: "var(--tasty-explorer-drop-target-border)" }]} />
        <QNote>Tooltip strings reuse <code>explorer.more.hidden_show</code> / <code>.hidden_hide</code> with the bound shortcut appended in parentheses (the shortcut is formatted, not translated). The b11 kit sample with radius-sm is superseded.</QNote>
      </QSpec>

      <QSpec title="Edge auto-scroll · dragging items · tree expand"
        when={<>One rule for every drag that can reach a list edge: <b>drag-select</b>, <b>dragging items</b> in the listing, and dragging over the <b>sidebar tree</b> (each in its own scroll area). Inside the <span className="tok">--tasty-explorer-autoscroll-zone</span> 24 band the list scrolls at <b>speed = 20 rows/s × t²</b>, t = 0 at the band's inner edge and 1 at the list edge, in <b>rows of the current view</b> (Detail row, List row, Grid cell row) so every view moves the same number of items. Past the edge the pointer keeps the <b>full speed</b>; it does not grow with distance. Nothing is drawn for the band. Hovering a closed tree folder during an item drag expands it after <span className="tok">--tasty-explorer-drag-expand-delay</span> 800 ms; the drop-target ring is the waiting cue, no extra mark.</>}>
        <QStage variant="solo center" style={{ padding: 20, background: "var(--tasty-bg-app)" }}>
          <XCol label="spec annotation — the dashed bands are NOT drawn" w={560}>
            <XCell w={560} h={220} status="2 of 5 selected"><QBody>
              {QROWS.map((r, i) => <QK.DetailRow key={i} {...r} state={i >= 3 ? "selected" : undefined} />)}
              <div style={{ position: "absolute", left: 0, right: 0, bottom: 0, height: "var(--tasty-explorer-autoscroll-zone)", border: "var(--tasty-border-width) dashed var(--tasty-text-muted)", pointerEvents: "none", display: "flex", alignItems: "center", justifyContent: "flex-end", padding: "0 var(--tasty-space-sm)", fontFamily: "var(--tasty-font-mono)", fontSize: "var(--tasty-font-size-micro)", color: "var(--tasty-text-muted)" }}>24 · 0 → 20 rows/s</div>
            </QBody></XCell>
          </XCol>
        </QStage>
        <QMeta
          specs={[["band", "explorer-autoscroll-zone 24, top + bottom · no indicator"], ["speed", "20 rows/s × t² (ease-in) · rows of the current view · rule constant"], ["outside", "keeps 20 rows/s · app drives the scroll offset itself"], ["applies to", "drag-select · item drag (listing) · item drag over the sidebar tree"], ["tree expand", "explorer-drag-expand-delay 800 ms · cue = drop-target ring"]]}
          tokens={[{ tok: "--tasty-explorer-autoscroll-zone", use: "→ size-24" }, { tok: "--tasty-explorer-drag-expand-delay", use: "→ duration-800 (new primitive)" }]} />
      </QSpec>

      <QSpec title="Undo / Redo rows — copy · empty history · Windows · shortcuts"
        when={<>The Undo / Redo group sits where it is now: in the empty-area menu between the create / paste group and Properties. It is <b>always there</b>: with no step a row is a disabled bare “Undo” / “Redo”. A row names the step with a gerund (“Undo copying 3 items”), with an en <code>_one</code> form. Rows bound to a command <b>show their first binding</b> in the native shortcut column — Undo and Redo as well as Rename and Trash; one rule for the whole menu. A step that can no longer be undone is disabled with the reason in the tooltip on Linux and macOS; Windows popup menus have no item tooltips, so there the <b>row label itself</b> becomes the reason sentence. The reason is a full sentence (“A newer file is there now”), so the lead-in ends with a period, not a colon.</>}>
        <QStage variant="solo center" style={{ padding: 20, background: "var(--tasty-bg-app)", gap: 16, flexWrap: "wrap", alignItems: "flex-start" }}>
          <XCol label="steps on both sides">
            <div style={qMenu}><QMenuItem label="Paste" shortcut="Ctrl+V" /><QMenuItem separator /><QMenuItem label="Undo copying 3 items" shortcut="Ctrl+Z" /><QMenuItem label="Redo moving 1 item" shortcut="Ctrl+Shift+Z" /><QMenuItem separator /><QMenuItem label="Properties" /></div>
          </XCol>
          <XCol label="empty history">
            <div style={qMenu}><QMenuItem label="Paste" shortcut="Ctrl+V" disabled /><QMenuItem separator /><QMenuItem label="Undo" shortcut="Ctrl+Z" disabled /><QMenuItem label="Redo" shortcut="Ctrl+Shift+Z" disabled /><QMenuItem separator /><QMenuItem label="Properties" /></div>
          </XCol>
          <XCol label="stale — Linux / macOS (tooltip) · Windows (label)">
            <div style={{ display: "flex", flexDirection: "column", gap: 8 }}>
              <div style={qMenu}><QMenuItem label="Undo moving 3 items" shortcut="Ctrl+Z" disabled title="Can't undo. A newer file is there now" /></div>
              <div style={qMenu}><QMenuItem label="Can't undo. A newer file is there now" shortcut="Ctrl+Z" disabled /></div>
            </div>
          </XCol>
        </QStage>
        <QMeta
          specs={[["position", "empty-area menu · create / paste | Undo · Redo | Properties (unchanged)"], ["empty", "always shown · disabled bare Undo / Redo"], ["shortcut column", "every row bound to a command shows its first binding — Undo / Redo included"], ["stale", "disabled · tooltip “Can't undo. {reason}” (GTK · macOS) · Windows: same sentence as the row label"]]}
          tokens={[{ tok: "--tasty-state-disabled-fg", use: "disabled rows (kit)", color: "var(--tasty-state-disabled-fg)" }]} />
        <QNote>i18n — <code>explorer.menu.undo</code> “Undo {"{op}"}” / “실행 취소: {"{op}"}” / “元に戻す: {"{op}"}” · <code>explorer.menu.undo_none</code> “Undo” / “실행 취소” / “元に戻す” · <code>explorer.menu.redo_none</code> “Redo” / “다시 실행” / “やり直す” · <code>explorer.menu.op_copy</code> “copying {"{n}"} items” (<code>_one</code> “copying 1 item”) / “{"{n}"}개 복사” / “{"{n}"} 件のコピー” · <code>explorer.menu.op_move</code> “moving {"{n}"} items” (<code>_one</code> “moving 1 item”) / “{"{n}"}개 이동” / “{"{n}"} 件の移動” · <code>explorer.menu.undo_stale</code> changes to “Can't undo. {"{reason}"}” / “되돌릴 수 없습니다. {"{reason}"}” / “元に戻せません。{"{reason}"}” (reason strings stay as they are) · settings labels confirmed: <code>settings.keybindings.explorer_undo_label</code> “Undo:” / “실행 취소:” / “元に戻す:” · <code>explorer_redo_label</code> “Redo:” / “다시 실행:” / “やり直す:”.</QNote>
      </QSpec>
    </QSection>
  );
}

// ── Properties (b12) — local copy of the YProps frame (explorer-ops.jsx mounts after this file) ──
function QProps({ glyph = QK.ic.file, glyphColor, name, children }) {
  return (
    <div style={{ width: "var(--tasty-explorer-props-width)", maxWidth: "100%", background: "var(--tasty-bg-panel)", border: "var(--tasty-border-width) solid var(--tasty-border-strong)", borderRadius: "var(--tasty-radius)", boxShadow: "var(--tasty-shadow-modal)", overflow: "hidden" }}>
      <div style={{ display: "flex", alignItems: "center", gap: "var(--tasty-space-sm)", padding: "var(--tasty-space-md) var(--tasty-explorer-props-padding-x) var(--tasty-space-sm)" }}>
        <span style={{ display: "inline-flex", flex: "none", color: glyphColor || "var(--tasty-text-muted)" }}>{glyph}</span>
        <span style={{ flex: 1, minWidth: 0, fontSize: "var(--tasty-font-size-max)", fontWeight: "var(--tasty-font-weight-normal)", overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>{name}</span>
        <QIconButton size="sm" aria-label="Close" title="Close">{QK.ic.x}</QIconButton>
      </div>
      <div style={{ padding: "0 var(--tasty-explorer-props-padding-x) var(--tasty-space-md)", display: "flex", flexDirection: "column" }}>{children}</div>
    </div>
  );
}
function QField({ k, v, mono, copy, warn }) {
  return (
    <div style={{ display: "grid", gridTemplateColumns: "var(--tasty-explorer-props-label-width) 1fr", gap: "var(--tasty-space-sm)", alignItems: "start", minHeight: "var(--tasty-explorer-props-row-min-height)", paddingTop: "var(--tasty-explorer-props-row-pad-top)" }}>
      <span style={{ fontSize: "var(--tasty-font-size-caption)", color: "var(--tasty-text-muted)", lineHeight: "var(--tasty-explorer-props-row-line)" }}>{k}</span>
      <span style={{ display: "flex", flexDirection: "column", minWidth: 0 }}>
        <span style={{ display: "flex", alignItems: "flex-start", gap: "var(--tasty-space-xs)", minWidth: 0 }}>
          <span style={{ flex: 1, minWidth: 0, fontFamily: mono ? "var(--tasty-font-mono)" : "inherit", fontSize: mono ? "var(--tasty-font-size-caption)" : "var(--tasty-font-size-body)", lineHeight: "var(--tasty-explorer-props-row-line)", color: "var(--tasty-text-secondary)", overflowWrap: "anywhere" }}>{v}</span>
          {copy && <QIconButton size="sm" aria-label="Copy" title="Copy"><QIcon name="copy" /></QIconButton>}
        </span>
        {warn && <span style={{ display: "flex", alignItems: "center", gap: "var(--tasty-space-xs)", fontSize: "var(--tasty-font-size-caption)", lineHeight: "var(--tasty-explorer-props-row-line)", color: warn[1] }}><QIcon name="alertTriangle" size="var(--tasty-icon-size-xs)" />{warn[0]}</span>}
      </span>
    </div>
  );
}
function QPreview({ h, low }) {
  return (
    <div style={{ width: "var(--tasty-explorer-preview-width)", flex: "none", display: "flex", flexDirection: "column", borderLeft: "var(--tasty-border-width) solid var(--tasty-separator)", background: "var(--tasty-bg-panel)" }}>
      <div style={{ display: "flex", flexDirection: "column", justifyContent: "center", height: "var(--tasty-explorer-preview-header-height)", flex: "none", padding: "0 var(--tasty-space-sm)", borderBottom: low ? "none" : "var(--tasty-border-width) solid var(--tasty-separator)" }}>
        <span style={{ fontSize: "var(--tasty-font-size-body)", color: "var(--tasty-text-primary)" }}>diagram.png</span>
        <span style={{ fontSize: "var(--tasty-font-size-caption)", color: "var(--tasty-text-muted)" }}>PNG image · 640 × 400 · 96 KB</span>
      </div>
      {!low && <div style={{ flex: 1, display: "flex", alignItems: "center", justifyContent: "center", padding: "var(--tasty-space-md)", background: "var(--tasty-bg-sidebar)" }}>
        <div style={{ width: "100%", aspectRatio: "16 / 10", display: "flex", alignItems: "center", justifyContent: "center", border: "var(--tasty-border-width) solid var(--tasty-separator)", borderRadius: "var(--tasty-radius-sm)", background: "var(--tasty-surface-raised)", color: "var(--tasty-text-muted)", fontFamily: "var(--tasty-font-mono)", fontSize: "var(--tasty-font-size-micro)" }}>image · fit</div>
      </div>}
    </div>
  );
}

function RemainingB12Section() {
  const partial = "var(--tasty-explorer-props-partial-fg)";
  const broken = "var(--tasty-explorer-link-broken-fg)";
  return (
    <QSection id="remaining12" title="Remaining UI — counting stage · Properties rows · low preview (b12)">
      <QSpec title="Progress — counting stage · zero-byte work"
        when={<>A copy or move first counts what it will touch. That stage now has its <b>own words</b> — “Counting items to copy · {"{n}"} found” — and <b>no bar</b> (there is nothing to measure against yet); the count refreshes at most 4 times a second. Once counting ends, the line counts <b>files found</b>, not top-level items, and names the file in work: “Copying 1,204 of 200,000 · a/b/file.txt” shows only the name. The bar follows <b>bytes</b>; when the byte total is 0 or unknown it follows the <b>file count</b>, so a tree of empty files still moves.</>}>
        <QStage variant="solo center" style={{ padding: 20, background: "var(--tasty-bg-app)" }}>
          <XThemes render={(l) => (
            <XCell w={520} h={110} status={l === "Mocha" ? "Counting items to copy · 48,210 found" : "Copying 1,204 of 200,000 · file-01204.txt"} statusRight={l === "Latte" ? <span style={{ position: "absolute", left: 0, top: 0, height: "var(--tasty-explorer-progress-height)", width: "100%", background: "var(--tasty-explorer-progress-track)" }}><span style={{ display: "block", height: "100%", width: "0.6%", minWidth: 2, background: "var(--tasty-explorer-progress-fill)" }}></span></span> : null}><div style={{ flex: 1 }} /></XCell>
          )} />
        </QStage>
        <QMeta
          specs={[["counting", "explorer.op.counting_copy / counting_move · no bar · ≤ 4 updates / s"], ["after", "“Copying {i} of {n} · {name}” · n = files found · name = current file name"], ["bar", "bytes; byte total 0 or unknown → file count"]]}
          tokens={[{ tok: "--tasty-explorer-progress-fill", use: "bar", color: "var(--tasty-explorer-progress-fill)" }, { tok: "--tasty-explorer-progress-track", use: "track", color: "var(--tasty-explorer-progress-track)" }]} />
        <QNote>i18n — <code>explorer.op.counting_copy</code> “Counting items to copy · {"{n}"} found” / “복사할 항목 세는 중 · {"{n}"}개 찾음” / “コピーする項目を数えています · {"{n}"} 件” · <code>explorer.op.counting_move</code> “Counting items to move · {"{n}"} found” / “이동할 항목 세는 중 · {"{n}"}개 찾음” / “移動する項目を数えています · {"{n}"} 件”. Numbers use the locale grouping.</QNote>
      </QSpec>

      <QSpec title="Properties — own path · folder permissions · links · partial size"
        when={<>For <b>one item</b>, a <b>Path</b> row (the item's own absolute path, mono, Copy) replaces Location; several items keep <b>Location</b> (their common folder). <b>Folders</b> get the same Permissions row as files. A <b>link</b> shows what the user cares about — its target: Kind “Symbolic link”, Link target, then the target's kind, size and modified time with “Target” labels; the link's own lstat values are not shown. A <b>broken link</b> is Kind <b>“Broken link”</b> (the word the Type column uses) with the warning link glyph in the header and a “Target not found” line under Link target. When a folder size count <b>skipped</b> folders it could not read, a warning line under Size says so.</>}>
        <QStage variant="solo center" style={{ padding: 20, background: "var(--tasty-bg-app)", gap: 16, flexWrap: "wrap", alignItems: "flex-start" }}>
          <XCol label="folder — permissions · partial size" w={360}>
            <QProps glyph={QK.ic.folder} name="folder">
              <QField k="Kind" v="Folder" />
              <QField k="Size" v="2 B · 3 items" warn={["Some folders couldn't be read", partial]} />
              <QField k="Modified" v="2026-10-10 21:51" mono />
              <QField k="Path" v="~/work/sandbox/folder" mono copy />
              <QField k="Permissions" v="rwxrwxr-x · read-only: no" mono />
            </QProps>
          </XCol>
          <XCol label="link" w={360}>
            <QProps glyph={<QIcon name="link" />} name="notes-link.md">
              <QField k="Kind" v="Symbolic link" />
              <QField k="Link target" v="~/work/notes/notes.md" mono copy />
              <QField k="Target kind" v="Markdown" />
              <QField k="Target size" v="12 KB (12,288 bytes)" />
              <QField k="Target modified" v="2026-06-27 11:40" mono />
              <QField k="Path" v="~/work/sandbox/notes-link.md" mono copy />
            </QProps>
          </XCol>
          <XCol label="broken link" w={360}>
            <QProps glyph={<QIcon name="link" />} glyphColor={broken} name="old-config.toml">
              <QField k="Kind" v="Broken link" />
              <QField k="Link target" v="~/.config/tasty/old-config.toml" mono copy warn={["Target not found", broken]} />
              <QField k="Path" v="~/work/sandbox/old-config.toml" mono copy />
            </QProps>
          </XCol>
        </QStage>
        <QMeta
          specs={[["order", "Kind · Size · Modified · Created · Path · Permissions"], ["Path", "one item: own absolute path + Copy (replaces Location) · several: Location (common folder) + Copy"], ["folder", "Permissions row, same format as files"], ["link", "Kind · Link target · Target kind · Target size · Target modified · Path — no lstat values"], ["broken", "Kind “Broken link” · header glyph link in explorer-link-broken-fg · “Target not found” under Link target"], ["partial", "warning line under Size · alertTriangle 12 · explorer-props-partial-fg"]]}
          tokens={[{ tok: "--tasty-explorer-props-partial-fg", use: "→ accent-warning", color: "var(--tasty-explorer-props-partial-fg)" }, { tok: "--tasty-explorer-link-broken-fg", use: "broken link", color: "var(--tasty-explorer-link-broken-fg)" }]} />
        <QNote>i18n — <code>explorer.props.path</code> “Path” / “경로” / “パス” · <code>.target_kind</code> “Target kind” / “대상 종류” / “リンク先の種類” · <code>.target_size</code> “Target size” / “대상 크기” / “リンク先のサイズ” · <code>.target_modified</code> “Target modified” / “대상 수정 시각” / “リンク先の更新日時” · <code>.target_missing</code> “Target not found” / “대상을 찾을 수 없음” / “リンク先が見つかりません” · <code>.partial</code> “Some folders couldn't be read” / “일부 폴더를 읽지 못함” / “一部のフォルダーを読み取れませんでした” · Kind for a broken link reuses <code>explorer.kind.broken_link</code>.</QNote>
      </QSpec>

      <QSpec title="Preview panel in a low cell — header only"
        when={<>When the body under the preview header would be shorter than <span className="tok">--tasty-explorer-preview-body-min-height</span> 120, the body is <b>hidden</b> and only the header stays (name + kind · size), without its bottom rule. A 30 px thumbnail says nothing; the header still tells the user what is selected. The panel keeps its width, and the width rules (hide under preview-min + list-min) are unchanged.</>}>
        <QStage variant="solo center" style={{ padding: 20, background: "var(--tasty-bg-app)", gap: 16, flexWrap: "wrap", alignItems: "flex-start" }}>
          <XCol label="tall cell — body shown" w={640}>
            <XCell w={640} h={300} status="5 items"><QBody>{QROWS.map((r, i) => <QK.DetailRow key={i} {...r} state={i === 3 ? "selected" : undefined} />)}</QBody><QPreview /></XCell>
          </XCol>
          <XCol label="180 cell — body < 120 → header only" w={640}>
            <XCell w={640} h={180} status="5 items"><QBody>{QROWS.slice(2).map((r, i) => <QK.DetailRow key={i} {...r} state={i === 1 ? "selected" : undefined} />)}</QBody><QPreview low /></XCell>
          </XCol>
        </QStage>
        <QMeta
          specs={[["threshold", "body height < explorer-preview-body-min-height 120 → body hidden"], ["kept", "header (name + kind · size), no bottom rule"], ["width", "unchanged rules"]]}
          tokens={[{ tok: "--tasty-explorer-preview-body-min-height", use: "→ size-120" }, { tok: "--tasty-explorer-preview-header-height", use: "40" }]} />
        <QNote><b>Conditional items B1–B7</b> (no design until decided).</QNote>
      </QSpec>
    </QSection>
  );
}

window.ExplorerOpsB12 = { FollowupsB12Section, RemainingB12Section };
