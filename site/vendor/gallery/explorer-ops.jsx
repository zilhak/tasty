// Tasty Gallery — Explorer file operations (2026-10-09, batch 9), part 2:
// Progress / conflicts / results · Properties / preview / thumbnails · page mount.
const { Section: YSection, Spec: YSpec, Stage: YStage, Meta: YMeta, Note: YNote } = window.Gallery;
const YDS = window.TastyDesignSystem_41fd3f;
const { IconButton: YIconButton, Button: YButton, Spinner: YSpinner, Checkbox: YCheckbox, Icon: YIcon, Toast: YToast, Tag: YTag, MenuItem: YMenuItem } = YDS;
const YK = window.ExplorerKit;
const { XLbl, XCol, XThemes, XToolbar, XCell, XDetail, XROWS, CreateSection, DragSection, SearchSection } = window.ExplorerOpsParts;

const NAV = [
  { id: "create", label: "Create · commands" },
  { id: "drag", label: "Drag & drop" },
  { id: "progress", label: "Progress · conflicts · results" },
  { id: "properties", label: "Properties · preview" },
  { id: "search", label: "Filter · search" },
];

// ── Progress ──────────────────────────────────────────────
function YProgressStatus({ pct = 30, text, bytes, queued, waiting }) {
  return (
    <>
      <span style={{ position: "absolute", left: 0, right: 0, top: "calc(-1 * var(--tasty-border-width))", height: "var(--tasty-explorer-progress-height)", background: "var(--tasty-explorer-progress-track)" }}>
        <span style={{ display: "block", width: pct + "%", height: "100%", background: waiting ? "var(--tasty-accent-warning)" : "var(--tasty-explorer-progress-fill)" }} />
      </span>
      <span style={{ minWidth: 0, overflow: "hidden", textOverflow: "ellipsis", color: waiting ? "var(--tasty-accent-warning)" : "var(--tasty-text-secondary)" }}>{text}</span>
    </>
  );
}
function YStatusRight({ bytes, queued, waiting }) {
  return (
    <span style={{ display: "flex", alignItems: "center", gap: "var(--tasty-space-sm)", flex: "none" }}>
      {bytes && <span style={{ fontFamily: "var(--tasty-font-mono)" }}>{bytes}</span>}
      {queued ? <YTag>+{queued} queued</YTag> : null}
      {waiting ? <YButton variant="ghost" size="sm">Show</YButton> : <YIconButton size="sm" aria-label="Cancel" title="Cancel">{YK.ic.x}</YIconButton>}
    </span>
  );
}
function YQueue() {
  const row = (title, sub, run) => (
    <div style={{ display: "flex", alignItems: "center", gap: "var(--tasty-space-sm)", minHeight: "var(--tasty-menu-item-height)", padding: "var(--tasty-space-xs) var(--tasty-space-sm)" }}>
      <span style={{ display: "inline-flex", flex: "none", color: "var(--tasty-text-muted)" }}>{run ? <YSpinner size="var(--tasty-icon-size-sm)" /> : <YIcon name="layers" size="var(--tasty-icon-size-sm)" />}</span>
      <span style={{ flex: 1, minWidth: 0, display: "flex", flexDirection: "column" }}>
        <span style={{ fontSize: "var(--tasty-font-size-body)", color: "var(--tasty-text-primary)", overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>{title}</span>
        <span style={{ fontSize: "var(--tasty-font-size-caption)", color: "var(--tasty-text-muted)" }}>{sub}</span>
      </span>
      <YIconButton size="sm" aria-label={run ? "Cancel" : "Remove from queue"} title={run ? "Cancel" : "Remove from queue"}>{YK.ic.x}</YIconButton>
    </div>
  );
  return (
    <div style={{ position: "absolute", right: "var(--tasty-space-sm)", bottom: "calc(var(--tasty-status-bar-height) + var(--tasty-space-xs))", width: "var(--tasty-size-288)", background: "var(--tasty-menu-bg)", border: "var(--tasty-border-width) solid var(--tasty-menu-border)", borderRadius: "var(--tasty-menu-radius)", boxShadow: "var(--tasty-shadow-popover)", padding: "var(--tasty-space-xs)" }}>
      {row("Copy 40 items to Documents", "12 of 40 · 1.2 / 3.4 GB", true)}
      {row("Move 3 items to Archive", "Queued", false)}
    </div>
  );
}
function YConflict({ many, folder }) {
  const meta = (k, v) => (
    <div style={{ display: "grid", gridTemplateColumns: "var(--tasty-size-64) 1fr", gap: "var(--tasty-space-sm)", fontSize: "var(--tasty-font-size-caption)" }}>
      <span style={{ color: "var(--tasty-text-muted)" }}>{k}</span><span style={{ fontFamily: "var(--tasty-font-mono)", color: "var(--tasty-text-secondary)" }}>{v}</span>
    </div>
  );
  return (
    <div style={{ width: "var(--tasty-explorer-conflict-width)", maxWidth: "100%", background: "var(--tasty-bg-panel)", border: "var(--tasty-border-width) solid var(--tasty-border-strong)", borderRadius: "var(--tasty-radius)", boxShadow: "var(--tasty-shadow-modal)", overflow: "hidden" }}>
      <div style={{ padding: "var(--tasty-space-md) var(--tasty-size-14) var(--tasty-space-sm)", display: "flex", flexDirection: "column", gap: "var(--tasty-space-sm)" }}>
        <div style={{ fontSize: "var(--tasty-font-size-max)", fontWeight: "var(--tasty-font-weight-semibold)", color: "var(--tasty-text-primary)" }}>{folder ? "A folder named “assets” already exists" : "“report.pdf” already exists"}</div>
        <div style={{ fontSize: "var(--tasty-font-size-caption)", color: "var(--tasty-text-muted)" }}>in ~/Documents</div>
        <div style={{ display: "flex", flexDirection: "column", gap: 2 }}>
          {meta("Existing", folder ? "folder · 214 items" : "2.4 MB · 2026-06-24 09:12")}
          {meta("Incoming", folder ? "folder · 37 items" : "2.6 MB · 2026-10-09 10:02")}
        </div>
        {folder && <div style={{ fontSize: "var(--tasty-font-size-caption)", color: "var(--tasty-text-muted)" }}>Folders are not merged.</div>}
        {many && <YCheckbox label="Do this for the other 3 conflicts" />}
      </div>
      <div style={{ display: "flex", alignItems: "center", gap: "var(--tasty-space-sm)", padding: "0 var(--tasty-size-14) var(--tasty-space-md)" }}>
        <YButton variant="ghost" size="sm">Cancel the rest</YButton>
        <span style={{ flex: 1 }} />
        <YButton variant="secondary" size="sm">Skip</YButton>
        {!folder && <YButton variant="secondary" size="sm">Replace</YButton>}
        <YButton variant="primary" size="sm">Keep both</YButton>
      </div>
    </div>
  );
}
function YToastAt({ children }) {
  return <div style={{ position: "absolute", right: "var(--tasty-space-sm)", bottom: "calc(var(--tasty-status-bar-height) + var(--tasty-space-sm))", maxWidth: "calc(100% - 2 * var(--tasty-space-sm))" }}>{children}</div>;
}
function YResult({ variant, title, lines, more, actions }) {
  return (
    <YToast variant={variant}>
      <div style={{ display: "flex", flexDirection: "column", gap: "var(--tasty-space-xs)", minWidth: 0 }}>
        <div style={{ display: "flex", alignItems: "flex-start", gap: "var(--tasty-space-sm)" }}>
          <span style={{ flex: 1, minWidth: 0 }}>{title}</span>
          <YIconButton size="sm" aria-label="Dismiss" title="Dismiss">{YK.ic.x}</YIconButton>
        </div>
        {lines && <div style={{ display: "flex", flexDirection: "column", gap: 2 }}>
          {lines.map(([p, why]) => (
            <div key={p} style={{ display: "flex", flexDirection: "column", fontSize: "var(--tasty-font-size-caption)" }}>
              <span style={{ fontFamily: "var(--tasty-font-mono)", color: "var(--tasty-text-secondary)", overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap", direction: "rtl", textAlign: "left" }}>{p}</span>
              <span style={{ color: "var(--tasty-text-muted)" }}>{why}</span>
            </div>
          ))}
          {more ? <span style={{ fontSize: "var(--tasty-font-size-caption)", color: "var(--tasty-text-muted)" }}>and {more} more</span> : null}
        </div>}
        {actions && <div style={{ display: "flex", gap: "var(--tasty-space-xs)" }}>{actions}</div>}
      </div>
    </YToast>
  );
}

function ProgressSection() {
  return (
    <YSection id="progress" title="Progress · conflicts · cancel · results · undo">
      <YSpec title="Running — on the explorer's own status line · queue"
        when={<>Work shows in the <b>status line of the explorer that started it</b>, so two explorers in different cells each show only their own. A 2px bar runs along the top edge of the status line (determinate by bytes; no animation), the left side reads <b>“Copying 12 of 40 · {"{current file}"}”</b>, the right side has the byte count, a <b>“+n queued”</b> Tag when more work waits in this explorer, and a <b>Cancel</b> ×. Clicking the left side or the Tag opens the <b>queue</b> popover above the status line: the running job (Cancel) and queued jobs (remove). One job runs at a time per app; a job queued in another explorer shows only there. Nothing takes keyboard focus.</>}>
        <YStage variant="solo center" style={{ padding: 20, background: "var(--tasty-bg-app)" }}>
          <XThemes render={(l) => (
            <XCell w={520} h={l === "Mocha" ? 280 : 200} status={<YProgressStatus pct={34} text="Copying 12 of 40 · report-final-v3-signed.pdf" />} statusRight={<YStatusRight bytes="1.2 / 3.4 GB" queued={1} />} overlay={l === "Mocha" ? <YQueue /> : null}>
              <XDetail />
            </XCell>
          )} />
        </YStage>
        <YMeta
          specs={[["where", "status line of the explorer that started the job (never a window-wide toast)"], ["bar", "explorer-progress-height 2 · top edge of the status line · track / fill · width = bytes done"], ["label", "“{Copying|Moving|Moving to Trash} {i} of {n} · {file}” · caption · ellipsis"], ["bytes", "mono · “1.2 / 3.4 GB” · unknown size → item count only, bar full-width track with no fill"], ["queued", "Tag “+n queued” · opens the queue popover"], ["queue", "menu surface · 288 · above the status line · Spinner row = running (×) · layers row = queued (×)"], ["cancel", "× on the status line · stops items not yet done"]]}
          tokens={[{ tok: "--tasty-explorer-progress-height", use: "→ size-2" }, { tok: "--tasty-explorer-progress-track", use: "→ surface-raised", color: "var(--tasty-explorer-progress-track)" }, { tok: "--tasty-explorer-progress-fill", use: "→ accent-primary", color: "var(--tasty-explorer-progress-fill)" }, { tok: "--tasty-status-bar-height", use: "24" }]} />
      </YSpec>

      <YSpec title="Name conflict — surface-scoped prompt · apply to all · waiting"
        when={<>A conflict <b>pauses</b> the job and asks with a <b>popup scoped to that explorer cell</b> (dims only the cell, like the markdown large-file confirm). It opens only while the explorer has focus; if the user is typing in another surface the status line turns warning-toned — <b>“Waiting for your answer · 1 name conflict”</b> with a <b>Show</b> button — and the popup opens when they come back. Answers: <b>Keep both</b> (primary, the default on <span className="ic">↵</span>; the app names the copy), <b>Skip</b>, <b>Replace</b> (files only, never the default) and <b>Cancel the rest</b> (ghost, left; <span className="ic">Esc</span>). With more conflicts left, a checkbox applies the answer to all of them. A folder colliding with a folder (or a file with a folder) offers only Skip / Keep both — no merge, no replace.</>}>
        <YStage variant="solo center" style={{ padding: 20, background: "var(--tasty-bg-app)", gap: 16, flexWrap: "wrap", alignItems: "flex-start" }}>
          <XCol label="file conflict · 4 left · apply to all" w={520}>
            <XCell w={520} h={300} status={<YProgressStatus pct={50} text="Copying 20 of 40 · paused" />} statusRight={<YStatusRight bytes="1.7 / 3.4 GB" />}
              overlay={<div style={{ position: "absolute", inset: "44px 0 0 0", background: "var(--tasty-scrim-bg)", display: "flex", alignItems: "center", justifyContent: "center", padding: "var(--tasty-space-md)" }}><YConflict many /></div>}>
              <XDetail />
            </XCell>
          </XCol>
          <XCol label="folder conflict — no Replace" w={420}>
            <YConflict folder />
          </XCol>
          <XCol label="waiting — the user is in another surface" w={520}>
            <XThemes render={() => (
              <XCell w={480} h={150} status={<YProgressStatus pct={50} waiting text="Waiting for your answer · 1 name conflict" />} statusRight={<YStatusRight waiting />}>
                <XDetail rows={XROWS.slice(0, 2)} />
              </XCell>
            )} />
          </XCol>
        </YStage>
        <YMeta
          specs={[["scope", "popup clamped to the explorer cell · scrim-bg over the body · shadow-modal"], ["width", "explorer-conflict-width 400"], ["title", "14 semibold · “{name}” already exists · caption “in {folder}”"], ["compare", "Existing / Incoming · size · modified · mono caption"], ["buttons", "Cancel the rest (ghost, left) · Skip · Replace (files only) · Keep both (primary, ↵)"], ["apply to all", "Checkbox, only when more conflicts remain"], ["focus", "opens only when the explorer has focus · otherwise the status line waits (accent-warning) + Show"], ["overwrite", "offered for files from v1 · always an explicit choice"]]}
          tokens={[{ tok: "--tasty-explorer-conflict-width", use: "→ size-400" }, { tok: "--tasty-scrim-bg", use: "cell dim" }, { tok: "--tasty-accent-warning", use: "waiting line + bar", color: "var(--tasty-accent-warning)" }]} />
      </YSpec>

      <YSpec title="Results — done · cancelled · partial + retry · failed · trash unavailable · undo"
        when={<>Results appear as a <b>Toast anchored inside the explorer cell</b>, bottom-right, 8 above the status line (the cell's toasts stack upward). <b>Done</b> and <b>cancelled</b> leave after the standard toast time; anything with a failure <b>stays until dismissed</b>. A partial or failed result lists up to <b>3 failed paths</b> (path ellipsised at the front so the name stays, reason under it) and then “and n more”; <b>Copy paths</b> copies every failed path, <b>Retry</b> runs only the failed and skipped items. A move across disks whose source delete failed is its own line (“copied; original not removed”). Trash never falls back to permanent delete. <b>Undo</b> sits on the done toast for as long as it is visible; its own result reports what could not be undone.</>}>
        <YStage variant="solo center" style={{ padding: 20, background: "var(--tasty-bg-app)", gap: 16, flexWrap: "wrap", alignItems: "flex-start" }}>
          <XCol label="done — in place" w={480}>
            <XCell w={480} h={210} overlay={<YToastAt><YResult variant="success" title="Copied 40 items to Documents" actions={<YButton variant="ghost" size="sm">Undo</YButton>} /></YToastAt>}><XDetail /></XCell>
          </XCol>
          <XThemes gap="var(--tasty-space-md)" render={() => (
            <div style={{ display: "flex", flexDirection: "column", gap: "var(--tasty-space-sm)", width: "var(--tasty-toast-max-width)" }}>
              <YResult variant="info" title="Copy cancelled · 12 of 40 done" />
              <YResult variant="warning" title="Moved 37 of 40 · 3 failed"
                lines={[["~/Downloads/build/artifacts/app-release-unsigned.apk", "Permission denied"], ["~/Downloads/archive.zip", "Copied; original not removed (in use)"], ["~/Downloads/raw/IMG_2031.heic", "No space left on device"]]}
                actions={<><YButton variant="secondary" size="sm">Retry 3</YButton><YButton variant="ghost" size="sm">Copy paths</YButton></>} />
              <YResult variant="danger" title="Couldn't copy 12 items" lines={[["~/Downloads/raw/IMG_2030.heic", "No space left on device"]]} more={11}
                actions={<><YButton variant="secondary" size="sm">Retry 12</YButton><YButton variant="ghost" size="sm">Copy paths</YButton></>} />
              <YResult variant="danger" title="Trash isn't available on this drive. Nothing was deleted." />
              <YResult variant="warning" title="Undid move · 2 items couldn't be put back" lines={[["~/Documents/report.pdf", "A newer file is there now"]]} more={1} />
            </div>
          )} />
        </YStage>
        <YMeta
          specs={[["placement", "Toast inside the cell · bottom-right · status-bar-height + 8 · stack upward"], ["done", "success · “{Copied|Moved} {n} items to {folder}” · Undo (ghost) · standard time"], ["cancelled", "info · “{Copy} cancelled · {i} of {n} done” · standard time · partial file removed by the app"], ["partial", "warning · stays · ≤ 3 paths + “and n more” · Retry n · Copy paths"], ["failed", "danger · stays · same list and actions"], ["path", "mono caption · ellipsis at the front · reason under it (muted)"], ["trash unavailable", "danger · “Trash isn't available on this drive. Nothing was deleted.” · no fallback"], ["undo", "on the done toast while it shows · result toast lists what could not be undone"], ["dismiss", "× on every result"]]}
          tokens={[{ tok: "--tasty-toast-accent-success", use: "done", color: "var(--tasty-toast-accent-success)" }, { tok: "--tasty-toast-accent-warning", use: "partial / undo partial", color: "var(--tasty-toast-accent-warning)" }, { tok: "--tasty-toast-accent-danger", use: "failed / trash", color: "var(--tasty-toast-accent-danger)" }, { tok: "--tasty-toast-max-width", use: "320" }]} />
        <YNote>i18n: <code>explorer.op.copying</code> / <code>moving</code> / <code>trashing</code> “{"{verb}"} {"{i}"} of {"{n}"} · {"{file}"}” · <code>explorer.op.queued</code> “+{"{n}"} queued” · <code>explorer.op.waiting</code> “Waiting for your answer · {"{n}"} name conflict(s)” · <code>explorer.conflict.*</code> (title, in, existing, incoming, no_merge, apply_all, keep_both, skip, replace, cancel_rest) · <code>explorer.result.*</code> (done, cancelled, partial, failed, source_not_removed, more, retry, copy_paths, undo, undo_partial) · <code>explorer.trash.unavailable</code>.</YNote>
      </YSpec>
    </YSection>
  );
}

// ── Properties · preview · thumbnails ─────────────────────
function YField({ k, v, mono, copy, spin }) {
  return (
    <div style={{ display: "grid", gridTemplateColumns: "var(--tasty-explorer-props-label-width) 1fr", gap: "var(--tasty-space-sm)", alignItems: "start", minHeight: "var(--tasty-size-24)", paddingTop: 2 }}>
      <span style={{ fontSize: "var(--tasty-font-size-caption)", color: "var(--tasty-text-muted)", lineHeight: "var(--tasty-size-20)" }}>{k}</span>
      <span style={{ display: "flex", alignItems: "flex-start", gap: "var(--tasty-space-xs)", minWidth: 0 }}>
        {spin && <YSpinner size="var(--tasty-icon-size-sm)" />}
        <span style={{ flex: 1, minWidth: 0, fontFamily: mono ? "var(--tasty-font-mono)" : "inherit", fontSize: mono ? "var(--tasty-font-size-caption)" : "var(--tasty-font-size-body)", lineHeight: "var(--tasty-size-20)", color: "var(--tasty-text-secondary)", overflowWrap: "anywhere" }}>{v}</span>
        {copy && <YIconButton size="sm" aria-label="Copy" title="Copy"><YIcon name="copy" /></YIconButton>}
      </span>
    </div>
  );
}
function YProps({ glyph = YK.ic.file, name, children, note }) {
  return (
    <div style={{ width: "var(--tasty-explorer-props-width)", maxWidth: "100%", background: "var(--tasty-bg-panel)", border: "var(--tasty-border-width) solid var(--tasty-border-strong)", borderRadius: "var(--tasty-radius)", boxShadow: "var(--tasty-shadow-modal)", overflow: "hidden" }}>
      <div style={{ display: "flex", alignItems: "center", gap: "var(--tasty-space-sm)", padding: "var(--tasty-space-md) var(--tasty-size-14) var(--tasty-space-sm)" }}>
        <span style={{ display: "inline-flex", flex: "none", color: "var(--tasty-text-muted)" }}>{glyph}</span>
        <span style={{ flex: 1, minWidth: 0, fontSize: "var(--tasty-font-size-max)", fontWeight: "var(--tasty-font-weight-semibold)", overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>{name}</span>
        <YIconButton size="sm" aria-label="Close" title="Close">{YK.ic.x}</YIconButton>
      </div>
      <div style={{ padding: "0 var(--tasty-size-14) var(--tasty-space-md)", display: "flex", flexDirection: "column" }}>
        {children}
        {note && <div style={{ marginTop: "var(--tasty-space-sm)", fontSize: "var(--tasty-font-size-caption)", color: "var(--tasty-text-muted)" }}>{note}</div>}
      </div>
    </div>
  );
}
function YPreview({ kind }) {
  const head = { text: ["notes.md", "Markdown · 12 KB"], image: ["diagram.png", "PNG · 1280 × 720 · 488 KB"], none: ["archive.zip", "Archive · 64 MB"], loading: ["notes.md", "Markdown · 12 KB"], large: ["server.log", "Log · 38 MB"], error: ["private.key", "File · 3 KB"] }[kind];
  let body;
  if (kind === "text") body = (
    <div style={{ flex: 1, padding: "var(--tasty-space-sm)", fontFamily: "var(--tasty-font-mono)", fontSize: "var(--tasty-font-size-caption)", lineHeight: "var(--tasty-line-height-ui)", color: "var(--tasty-text-secondary)", whiteSpace: "pre", overflow: "hidden", background: "var(--tasty-bg-sidebar)" }}>{"# Notes\n\n- split floor 180\n- favorites pin 240\n- drag: move same disk\n\n## Open\n- thumbnails in Grid\n- preview panel back"}</div>
  );
  else if (kind === "image") body = (
    <div style={{ flex: 1, display: "flex", alignItems: "center", justifyContent: "center", padding: "var(--tasty-space-md)", background: "var(--tasty-bg-sidebar)" }}>
      <div style={{ width: "100%", aspectRatio: "16 / 9", display: "flex", alignItems: "center", justifyContent: "center", border: "var(--tasty-border-width) solid var(--tasty-separator)", borderRadius: "var(--tasty-radius-sm)", background: "var(--tasty-surface-raised)", color: "var(--tasty-text-muted)", fontFamily: "var(--tasty-font-mono)", fontSize: "var(--tasty-font-size-micro)" }}>image · fit</div>
    </div>
  );
  else {
    const st = {
      none: [YK.ic.file, null, "No preview for this file type"],
      loading: [<YSpinner />, null, "Loading preview…"],
      large: [YK.ic.file, null, "Too large to preview", "Over 1 MB."],
      error: [<YIcon name="alertTriangle" size="var(--tasty-icon-size-md)" />, "var(--tasty-explorer-error-fg)", "Can't read this file", null, "Permission denied (os error 13)"],
    }[kind];
    body = <div style={{ flex: 1, display: "flex", padding: "var(--tasty-space-sm)", background: "var(--tasty-bg-sidebar)" }}><YK.ExpState glyph={st[0]} glyphColor={st[1]} title={st[2]} sub={st[3]} reason={st[4]} bg="var(--tasty-bg-sidebar)" /></div>;
  }
  return (
    <div style={{ width: "var(--tasty-explorer-preview-width)", flex: "none", display: "flex", flexDirection: "column", borderLeft: "var(--tasty-border-width) solid var(--tasty-separator)", background: "var(--tasty-bg-panel)" }}>
      <div style={{ display: "flex", flexDirection: "column", justifyContent: "center", height: "var(--tasty-size-40)", flex: "none", padding: "0 var(--tasty-space-sm)", borderBottom: "var(--tasty-border-width) solid var(--tasty-separator)" }}>
        <span style={{ fontSize: "var(--tasty-font-size-body)", color: "var(--tasty-text-primary)", overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>{head[0]}</span>
        <span style={{ fontSize: "var(--tasty-font-size-caption)", color: "var(--tasty-text-muted)" }}>{head[1]}</span>
      </div>
      {body}
    </div>
  );
}
function YThumbCell({ name, kind, state }) {
  const slot = kind === "thumb"
    ? <span style={{ width: "var(--tasty-explorer-grid-thumb-size)", height: "var(--tasty-explorer-grid-thumb-size)", boxSizing: "border-box", border: "var(--tasty-border-width) solid var(--tasty-separator)", borderRadius: "var(--tasty-radius-sm)", background: "var(--tasty-surface-hover)", display: "flex", alignItems: "flex-end", justifyContent: "center", fontFamily: "var(--tasty-font-mono)", fontSize: "var(--tasty-font-size-micro)", color: "var(--tasty-text-muted)" }}>img</span>
    : <span style={{ display: "inline-flex", color: kind === "loading" ? "var(--tasty-accent-info)" : "var(--tasty-text-muted)" }}>{kind === "folder" ? YK.ic.folder : kind === "loading" ? YK.ic.image : YK.ic.file}</span>;
  return (
    <div style={{ width: 80, display: "flex", flexDirection: "column", alignItems: "center", gap: 4, padding: "8px 4px", borderRadius: "var(--tasty-radius)", background: state === "selected" ? "var(--tasty-surface-active)" : "transparent" }}>
      <span style={{ height: "var(--tasty-explorer-grid-thumb-size)", display: "flex", alignItems: "center", justifyContent: "center" }}>{slot}</span>
      <span style={{ fontSize: 11, textAlign: "center", lineHeight: "14px", height: 42, color: "var(--tasty-text-secondary)", wordBreak: "break-word", overflow: "hidden" }}>{name}</span>
    </div>
  );
}

function PropertiesSection() {
  return (
    <YSection id="properties" title="Properties · preview panel · Grid thumbnails">
      <YSpec title="Properties — popup for one item · folder · several · remote"
        when={<>Opened from the <b>Properties</b> context-menu row or the app's properties action, as a popup scoped to the explorer cell (same family as Rename; no scrim needed, <span className="ic">Esc</span> or × closes). It shows the item it was <b>opened for</b> and does not follow later selection changes. Fields are a two-column list: caption label (96) and value; paths and permissions are mono and wrap anywhere, with a Copy button on Location and Link target. Sizes that take time (folders, several items) show a Spinner and fill in when the background count ends; the popup never blocks the list. A <b>remote</b> item shows only what the host lists (name, kind, size, modified, location) and says so.</>}>
        <YStage variant="solo center" style={{ padding: 20, background: "var(--tasty-bg-app)", gap: 16, flexWrap: "wrap", alignItems: "flex-start" }}>
          <XThemes render={() => (
            <YProps glyph={YK.ic.image} name="diagram.png">
              <YField k="Kind" v="PNG image" />
              <YField k="Size" v="488 KB (499,712 bytes)" />
              <YField k="Modified" v="2026-06-26 18:05" mono />
              <YField k="Created" v="2026-06-26 17:58" mono />
              <YField k="Location" v="~/Downloads" mono copy />
              <YField k="Permissions" v="rw-r--r-- · read-only: no" mono />
            </YProps>
          )} />
          <XCol label="folder — size still counting" w={360}>
            <YProps glyph={YK.ic.folder} name="mockup-exports">
              <YField k="Kind" v="Folder" />
              <YField k="Size" v="Counting… 1,204 items · 1.1 GB" spin />
              <YField k="Modified" v="2026-06-20 14:30" mono />
              <YField k="Location" v="~/Downloads" mono copy />
            </YProps>
          </XCol>
          <XCol label="symlink" w={360}>
            <YProps glyph={YK.ic.link} name="current">
              <YField k="Kind" v="Symbolic link" />
              <YField k="Link target" v="~/work/tasty/target/release/tasty" mono copy />
              <YField k="Location" v="~/bin" mono copy />
            </YProps>
          </XCol>
          <XCol label="several items" w={360}>
            <YProps glyph={<YIcon name="layers" />} name="3 items">
              <YField k="Kinds" v="2 files, 1 folder" />
              <YField k="Total size" v="66.4 MB" />
              <YField k="Location" v="~/Downloads" mono copy />
            </YProps>
          </XCol>
          <XCol label="remote item" w={360}>
            <YProps name="build-0412.log" note="Remote — only what the remote host lists is shown.">
              <YField k="Kind" v="File" />
              <YField k="Size" v="2.1 MB" />
              <YField k="Modified" v="2026-10-09 08:12" mono />
              <YField k="Location" v="build-eu:~/logs" mono copy />
            </YProps>
          </XCol>
        </YStage>
        <YMeta
          specs={[["open", "context menu Properties (every shape) · keybinding action explorer.properties"], ["popup", "explorer-props-width 360 · scoped to the cell · shadow-modal · Esc / ×"], ["title", "glyph + name · 14 semibold · ellipsis"], ["fields", "label explorer-props-label-width 96 · caption muted · value body 13 (mono caption for paths / dates / mode) · wrap anywhere"], ["local file", "Kind · Size (with bytes) · Modified · Created · Location · Permissions"], ["folder", "Size counts in the background (Spinner) · item count"], ["symlink", "Link target + Copy"], ["several", "“{n} items” · Kinds · Total size · Location (common parent)"], ["remote", "Kind · Size · Modified · Location · muted note"], ["selection change", "popup keeps its item"]]}
          tokens={[{ tok: "--tasty-explorer-props-width", use: "→ size-360" }, { tok: "--tasty-explorer-props-label-width", use: "→ size-96" }, { tok: "--tasty-shadow-modal", use: "popup" }, { tok: "--tasty-spinner-indicator", use: "counting", color: "var(--tasty-spinner-indicator)" }]} />
      </YSpec>

      <YSpec title="Preview panel — text · image · other · loading · too large · unreadable"
        when={<>The preview panel <b>comes back as a toggle</b> (toolbar view group). It docks on the right of the listing at <span className="tok">--tasty-explorer-preview-width</span> (288), resizable by a splitter between 200 and 460; the width is remembered per explorer. A 40 header shows the name and “kind · size” (image: pixel size). <b>Text</b> files (UTF-8, by extension or content) show their first 64 KB in mono; <b>images</b> fit the panel; anything else, folders included, shows “No preview for this file type” and leaves the facts to the header. A new selection clears the old preview at once and shows <b>Loading preview…</b> until the read ends. Over 1 MB or unreadable shows a state, never content. Remote explorers preview text and images the host returns.</>}>
        <YStage variant="solo center" style={{ padding: 20, background: "var(--tasty-bg-app)", gap: 16, flexWrap: "wrap", alignItems: "flex-start" }}>
          <XCol label="text" w={640}>
            <XCell w={640} h={250} toolbar={<XToolbar preview />} status="5 items · 1 selected"><XDetail states={{ 4: "selected" }} /><YPreview kind="text" /></XCell>
          </XCol>
          <XCol label="image" w={640}>
            <XCell w={640} h={250} toolbar={<XToolbar preview />} status="5 items · 1 selected"><XDetail states={{ 3: "selected" }} /><YPreview kind="image" /></XCell>
          </XCol>
          <div style={{ display: "flex", gap: 12, flexWrap: "wrap" }}>
            {["none", "loading", "large", "error"].map((k) => (
              <XCol key={k} label={k === "none" ? "not supported" : k === "large" ? "too large" : k === "error" ? "unreadable" : "loading"}>
                <div style={{ height: 230, display: "flex", border: "var(--tasty-border-width) solid var(--tasty-border-default)", borderRadius: "var(--tasty-radius)", overflow: "hidden" }}><YPreview kind={k} /></div>
              </XCol>
            ))}
          </div>
        </YStage>
        <YMeta
          specs={[["toggle", "toolbar view group · columns glyph · keybinding action explorer.toggle_preview"], ["panel", "right · explorer-preview-width 288 · splitter 200 … 460 · 1px separator · remembered per explorer"], ["header", "40 · name (body, ellipsis) · kind · size (caption muted)"], ["text", "first 64 KB · mono caption · line-height-ui · on bg-sidebar · no wrap"], ["image", "fit, never upscaled · on bg-sidebar"], ["other / folder", "“No preview for this file type”"], ["loading", "Spinner · “Loading preview…” — the old preview is cleared first"], ["too large", "> 1 MB (app limit) · “Too large to preview”"], ["unreadable", "error tone · OS reason (mono)"], ["narrow cell", "panel hides itself below 200 + list min; toggle stays on"]]}
          tokens={[{ tok: "--tasty-explorer-preview-width", use: "→ size-288" }, { tok: "--tasty-explorer-preview-min-width", use: "→ size-200" }, { tok: "--tasty-explorer-preview-max-width", use: "→ size-460" }, { tok: "--tasty-bg-sidebar", use: "preview bed", color: "var(--tasty-bg-sidebar)" }]} />
      </YSpec>

      <YSpec title="Grid thumbnails — 40 slot for every cell"
        when={<>Thumbnails are <b>Grid only</b> (List and Detail keep their glyphs). Every Grid cell's icon slot becomes <span className="tok">--tasty-explorer-grid-thumb-size</span> (40), so rows stay uniform whether or not a folder holds images; folders and other files keep the 16 glyph centred in it. An image file shows its glyph in accent-info <b>while the thumbnail loads</b>, then the picture fit into 40 × 40 with a 1px separator edge and radius-sm. Thumbnails are made in the background, cached, and skipped over the app's size limit (glyph stays). Remote explorers keep glyphs. Cell width stays 80; the cell grows by 24 in height (16 → 40 slot).</>}>
        <YStage variant="solo center" style={{ padding: 20, background: "var(--tasty-bg-app)" }}>
          <XThemes render={() => (
            <div style={{ display: "flex", gap: 4, flexWrap: "wrap", width: 340, padding: 8, background: "var(--tasty-bg-panel)", borderRadius: "var(--tasty-radius)" }}>
              <YThumbCell name="mockup-exports" kind="folder" />
              <YThumbCell name="diagram.png" kind="thumb" state="selected" />
              <YThumbCell name="screenshot-2026-10-09.png" kind="thumb" />
              <YThumbCell name="IMG_2031.heic" kind="loading" />
              <YThumbCell name="notes.md" kind="file" />
            </div>
          )} />
        </YStage>
        <YMeta
          specs={[["scope", "Grid only · local only"], ["slot", "explorer-grid-thumb-size 40 · every cell"], ["thumbnail", "fit 40 × 40 · 1px separator · radius-sm"], ["loading / over limit", "image glyph 16 · accent-info"], ["cell", "80 wide (unchanged) · +24 tall"]]}
          tokens={[{ tok: "--tasty-explorer-grid-thumb-size", use: "→ size-40" }, { tok: "--tasty-accent-info", use: "image glyph fallback", color: "var(--tasty-accent-info)" }, { tok: "--tasty-separator", use: "thumb edge", color: "var(--tasty-separator)" }]} />
        <YNote>“img” stands in for the decoded picture. Real side: <code>grid_cell()</code> reserves the 40 slot for every entry; the thumbnail cache is keyed by path + mtime.</YNote>
      </YSpec>
    </YSection>
  );
}

function Page() {
  return (
    <>
      <CreateSection />
      <DragSection />
      <ProgressSection />
      <PropertiesSection />
      <SearchSection />
    </>
  );
}

window.Gallery.mount(
  "explorer-ops",
  NAV,
  {
    title: "Explorer — file operations",
    intro: "Design for the explorer file-operation contract (2026-10-09): creating items and where commands live, drag & drop, progress / conflicts / results / undo, properties and preview, and filter / search. Builds on the explorer surface on the Plugin surfaces page and reuses its parts.",
    howto: false,
  },
  <Page />
);
