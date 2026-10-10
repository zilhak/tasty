// Tasty Gallery — Explorer file operations, part 3 (2026-10-10, batch 11):
// kind words · requests refused · Retry of originals · undo titles · remaining states (links, keyboard, marquee,
// hidden files, show in folder, redo, create in folder). Loaded before explorer-ops.jsx, which mounts the page.
const ZDS = window.TastyDesignSystem_41fd3f;
const { Section: ZSection, Spec: ZSpec, Stage: ZStage, Meta: ZMeta, Note: ZNote } = window.Gallery;
const { IconButton: ZIconButton, Button: ZButton, Icon: ZIcon, Toast: ZToast, MenuItem: ZMenuItem, Input: ZInput } = ZDS;
const ZK = window.ExplorerKit;
const { XCol, XThemes, XToolbar, XCell } = window.ExplorerOpsParts;

const zMenu = { width: "var(--tasty-size-240)", background: "var(--tasty-menu-bg)", border: "var(--tasty-border-width) solid var(--tasty-menu-border)", borderRadius: "var(--tasty-menu-radius)", padding: "var(--tasty-space-xs)", boxShadow: "var(--tasty-shadow-popover)" };
const zLink = <span style={{ display: "inline-flex", flex: "none", color: "var(--tasty-explorer-link-glyph)" }}><ZIcon name="link" size="var(--tasty-explorer-link-glyph-size)" /></span>;
function ZName({ text, link, fg }) {
  return <span style={{ display: "inline-flex", alignItems: "center", gap: "var(--tasty-space-xs)", color: fg }}>{text}{link && zLink}</span>;
}
function ZBody({ children }) {
  return <div style={{ flex: 1, minWidth: 0, display: "flex", flexDirection: "column", background: "var(--tasty-bg-panel)", position: "relative" }}><ZK.DetailHeader /><div style={{ flex: 1, overflow: "hidden", position: "relative" }}>{children}</div></div>;
}
function ZCursor({ children, on = true }) {
  return <div style={{ boxShadow: on ? "inset 0 0 0 var(--tasty-explorer-cursor-ring-width) var(--tasty-explorer-cursor-ring)" : "none", borderRadius: "var(--tasty-radius-sm)" }}>{children}</div>;
}
function ZResult({ variant, title, lines, actions }) {
  return (
    <ZToast variant={variant}>
      <div style={{ display: "flex", flexDirection: "column", gap: "var(--tasty-space-xs)", minWidth: 0 }}>
        <div style={{ display: "flex", alignItems: "flex-start", gap: "var(--tasty-space-sm)" }}>
          <span style={{ flex: 1, minWidth: 0 }}>{title}</span>
          <ZIconButton size="sm" aria-label="Dismiss" title="Dismiss">{ZK.ic.x}</ZIconButton>
        </div>
        {lines && lines.map(([p, why]) => (
          <div key={p + why} style={{ display: "flex", flexDirection: "column", fontSize: "var(--tasty-font-size-caption)" }}>
            <span style={{ fontFamily: "var(--tasty-font-mono)", color: "var(--tasty-text-secondary)", overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap", direction: "rtl", textAlign: "left" }}>{p}</span>
            <span style={{ color: "var(--tasty-text-muted)" }}>{why}</span>
          </div>
        ))}
        {actions && <div style={{ display: "flex", gap: "var(--tasty-space-xs)" }}>{actions}</div>}
      </div>
    </ZToast>
  );
}
const zToastAt = (node) => <div style={{ position: "absolute", right: "var(--tasty-space-sm)", bottom: "calc(var(--tasty-status-bar-height) + var(--tasty-space-sm))", maxWidth: "calc(100% - 2 * var(--tasty-space-sm))" }}>{node}</div>;

const ZKIND_ROWS = [
  { glyph: ZK.ic.folder, name: "Documents", size: "—", date: "2026-10-02 10:14", type: "Folder" },
  { glyph: ZK.ic.file, name: "backup-2026.tar.gz", size: "1.2 GB", date: "2026-09-30 22:01", type: "Archive" },
  { glyph: ZK.ic.file, name: "report.pdf", size: "2.4 MB", date: "2026-06-24 09:12", type: "PDF document" },
  { glyph: ZK.ic.image, name: "diagram.png", size: "488 KB", date: "2026-06-26 18:05", type: "PNG image", glyphColor: "var(--tasty-accent-info)" },
  { glyph: ZK.ic.file, name: "notes.md", size: "12 KB", date: "2026-06-27 11:40", type: "Markdown" },
  { glyph: ZK.ic.file, name: <ZName text="link.md" link />, size: "6 B", date: "2026-10-01 08:00", type: "Link to Markdown" },
  { glyph: ZK.ic.file, name: "build.rs", size: "3 KB", date: "2026-10-03 16:20", type: "RS file" },
];

function FollowupsB11Section() {
  return (
    <ZSection id="followups11" title="Follow-ups — kind words · refused requests · Retry of originals · undo titles (2026-10-10 batch 11)">
      <ZSpec title="Kind words — one table for Type, Kind and the preview header · links say what they point to"
        when={<>A short <b>format-name table</b> turns an extension into a word. The same word fills the Detail <b>Type</b> column, Properties <b>Kind</b> and the preview header, so the three never disagree. Anything not in the table falls back to “{"{EXT}"} file”; no extension is “File”. A <b>symlink</b> reads “Link to {"{kind}"}” in Type and in the preview header, and the size is the target's (the preview shows the target). Properties keeps the lstat answer, “Symbolic link”, with the target path. A broken link reads “Broken link”. The byte limit now says <b>“Over 1 MiB.”</b> — every binary limit says MiB.</>}>
        <ZStage variant="solo center" style={{ padding: 20, background: "var(--tasty-bg-app)", gap: 16, flexWrap: "wrap", alignItems: "flex-start" }}>
          <XCol label="Detail — Type column words" w={640}>
            <XCell w={640} h={290} status="7 items"><ZBody>{ZKIND_ROWS.map((r, i) => <ZK.DetailRow key={i} {...r} />)}</ZBody></XCell>
          </XCol>
          <XCol label="preview header — link" w={288}>
            <div style={{ height: "var(--tasty-explorer-preview-header-height)", display: "flex", flexDirection: "column", justifyContent: "center", padding: "0 var(--tasty-space-md)", background: "var(--tasty-bg-panel)", border: "var(--tasty-border-width) solid var(--tasty-border-default)", borderRadius: "var(--tasty-radius)" }}>
              <span style={{ fontSize: "var(--tasty-font-size-body)", color: "var(--tasty-text-primary)" }}>link.md</span>
              <span style={{ fontSize: "var(--tasty-font-size-caption)", color: "var(--tasty-text-muted)" }}>Link to Markdown · 6 B</span>
            </div>
          </XCol>
        </ZStage>
        <ZMeta
          specs={[["table (en / ko / ja)", "md markdown → Markdown / Markdown / Markdown · txt → Text / 텍스트 / テキスト · html htm → HTML · json → JSON · toml → TOML · yaml yml → YAML · csv → CSV · pdf → PDF document / PDF 문서 / PDF 書類 · zip tar gz tgz bz2 xz zst 7z rar → Archive / 압축 파일 / アーカイブ · sh bash zsh fish ps1 → Shell script / 셸 스크립트 / シェルスクリプト · images → “{TYPE} image” / “{TYPE} 이미지” / “{TYPE} 画像”"], ["fallback", "“{EXT} file” / “{EXT} 파일” / “{EXT} ファイル” · no extension File / 파일 / ファイル · Folder / 폴더 / フォルダ"], ["Type column", "same word · ellipsis in the 92 column · full word in the tooltip"], ["link", "Type + preview header “Link to {kind}” / “{kind} 링크” / “{kind} へのリンク” · size = target · Properties Kind stays “Symbolic link” · broken “Broken link” / “끊긴 링크” / “リンク切れ”"], ["byte limit", "“Over 1 MiB.” / “1 MiB 초과” / “1 MiB を超えています。” (NBSP between number and unit)"], ["popup title", "regular weight AND text-primary — no accent colour (`strong`) on any popup title"]]}
          tokens={[{ tok: "--tasty-text-muted", use: "Type column", color: "var(--tasty-text-muted)" }, { tok: "--tasty-explorer-preview-header-height", use: "40" }]} />
        <ZNote>i18n (new): <code>explorer.kind.markdown</code> · <code>.text</code> · <code>.pdf</code> · <code>.archive</code> · <code>.shell</code> · <code>.link_to</code> “Link to {"{kind}"}” · <code>.broken_link</code> “Broken link”. HTML / JSON / TOML / YAML / CSV are the same in all three languages and need no key.</ZNote>
      </ZSpec>

      <ZSpec title="Requests refused — queue full · request too large"
        when={<>A file operation the app can't take is never dropped silently. Both cases show a <b>Warning</b> toast in the explorer's own cell (Surface scope, standard time): the user's action did not happen, so it is more than Info, but nothing was lost — clipboard and selection are unchanged. <b>Queue full</b> carries <b>Show queue</b>, which opens the existing “+n queued” popover above the status line. <b>Too large</b> has no action. The limits (8 waiting, 1 MiB of paths) are implementation details and stay out of the copy.</>}>
        <ZStage variant="solo center" style={{ padding: 20, background: "var(--tasty-bg-app)" }}>
          <XThemes render={(l) => (
            <XCell w={520} h={200} status={l === "Mocha" ? "Copying 3 of 40 · report.pdf" : "5 items"} overlay={zToastAt(l === "Mocha"
              ? <ZResult variant="warning" title="Too many file operations are waiting. Try again when one finishes." actions={<ZButton variant="ghost" size="sm">Show queue</ZButton>} />
              : <ZResult variant="warning" title="Too many items for one file operation. Select fewer items and try again." />)}>
              <ZBody>{ZKIND_ROWS.slice(0, 3).map((r, i) => <ZK.DetailRow key={i} {...r} />)}</ZBody>
            </XCell>
          )} />
        </ZStage>
        <ZMeta
          specs={[["kind", "Warning toast · Surface scope · standard time · not a result card"], ["queue full", "action Show queue → queue popover · explorer.request.queue_full + .show_queue"], ["too large", "no action · explorer.request.too_large"], ["numbers", "not in the copy"]]}
          tokens={[{ tok: "--tasty-toast-accent-warning", use: "both", color: "var(--tasty-toast-accent-warning)" }]} />
        <ZNote>Copy confirmed as proposed — <code>queue_full</code> en “Too many file operations are waiting. Try again when one finishes.” / ko “기다리는 파일 작업이 너무 많습니다. 작업이 하나 끝난 뒤 다시 시도하세요.” / ja “待機中のファイル操作が多すぎます。操作が一つ終わってからもう一度お試しください。” · <code>too_large</code> en “Too many items for one file operation. Select fewer items and try again.” / ko “한 번의 파일 작업에 담기에는 항목이 너무 많습니다. 항목을 줄여 다시 시도하세요.” / ja “一度のファイル操作には項目が多すぎます。項目を減らしてもう一度お試しください。” · <code>show_queue</code> “Show queue” / “대기열 보기” / “キューを表示”.</ZNote>
      </ZSpec>

      <ZSpec title="Retry of originals — its own progress words · one result card · leftover reasons"
        when={<>Retry on an “originals not fully removed” card compares each original with its copy, then deletes the original. It gets its own words: <b>“Checking”</b> while comparing, <b>“Removing originals”</b> while deleting, and the success card <b>“Removed {"{n}"} originals”</b>. When one Retry press runs both a delete of originals and a re-move of failed items, the user sees <b>one card</b> for the press: it appears when both jobs end and its title follows the worse outcome. An original that is kept goes back on the warning card (source_left_move, counting only what is left) with its reason, and Retry n returns. A cancelled delete lands there too, not on an info card.</>}>
        <ZStage variant="solo center" style={{ padding: 20, background: "var(--tasty-bg-app)", gap: 16, flexWrap: "wrap", alignItems: "flex-start" }}>
          <XCol label="running — checking · removing" w={520}>
            <div style={{ display: "flex", flexDirection: "column", gap: 8 }}>
              <XCell w={520} h={90} status="Checking 3 of 12 · photos/2026-09"><div style={{ flex: 1 }} /></XCell>
              <XCell w={520} h={90} status="Removing originals 5 of 12 · archive.zip"><div style={{ flex: 1 }} /></XCell>
            </div>
          </XCol>
          <XCol label="results" w={360}>
            <div style={{ display: "flex", flexDirection: "column", gap: 8, width: "var(--tasty-toast-max-width)", maxWidth: "100%" }}>
              <ZResult variant="success" title="Removed 12 originals" />
              <ZResult variant="warning" title="Moved 40 of 40 · 4 originals not fully removed"
                lines={[["~/Volumes/usb/notes.md", "The copy is missing or changed type, so the original was kept"], ["~/Volumes/usb/photos", "Kept 3 items that aren't in the copy"], ["~/Volumes/usb/mnt-link", "Kept: the copy is the same file as the original"], ["~/Volumes/usb/archive", "Cancelled; the original is still there, in whole or in part"]]}
                actions={<><ZButton variant="secondary" size="sm">Retry 4</ZButton><ZButton variant="ghost" size="sm">Copy paths</ZButton></>} />
            </div>
          </XCol>
        </ZStage>
        <ZMeta
          specs={[["progress", "explorer.progress.checking “Checking {i} of {n} · {name}” → .removing_originals “Removing originals {i} of {n} · {name}”"], ["queue title", "explorer.queue.remove_originals “Remove {n} originals”"], ["success", "explorer.result.removed_originals “Removed {n} originals” · _one “Removed 1 original” · standard time · no Undo"], ["one press = one card", "combined counts · tone of the worse outcome · shown when every job of the press ends"], ["kept", "back on source_left_move (warning · stays · Retry n · Copy paths)"], ["changed after move", "reuse changed_kept (path line = the original) — no new key"]]}
          tokens={[{ tok: "--tasty-toast-accent-warning", use: "kept", color: "var(--tasty-toast-accent-warning)" }, { tok: "--tasty-toast-accent-success", use: "removed", color: "var(--tasty-toast-accent-success)" }]} />
        <ZNote>en / ko / ja — <code>copy_missing</code> “The copy is missing or changed type, so the original was kept” / “사본이 없거나 종류가 바뀌어 원본을 남겨 둠” / “コピーが見つからないか種類が変わったため、元の項目を残しました” · <code>kept_not_in_copy</code> “Kept {"{n}"} items that aren't in the copy” (<code>_one</code> “Kept 1 item that isn't in the copy”) / “사본에 없는 항목 {"{n}"}개를 남겨 둠” / “コピーにない {"{n}"} 件を残しました” · <code>remove_cancelled</code> “Cancelled; the original is still there, in whole or in part” / “취소함. 원본이 전부 또는 일부 남아 있음” / “取り消したため、元の項目の全部または一部が残っています” · <code>same_as_copy</code> “Kept: the copy is the same file as the original” / “사본이 원본과 같은 파일이라 남겨 둠” / “コピーが元と同じファイルのため残しました” · <code>checking</code> “확인 중 {"{n}"}개 중 {"{i}"}개 · {"{name}"}” / “確認中 {"{i}"}/{"{n}"} 件 · {"{name}"}” · <code>removing_originals</code> “원본 지우는 중 {"{n}"}개 중 {"{i}"}개 · {"{name}"}” / “元の項目を削除中 {"{i}"}/{"{n}"} 件 · {"{name}"}” · <code>remove_originals</code> “원본 {"{n}"}개 지우기” / “元の {"{n}"} 件を削除” · <code>removed_originals</code> “원본 {"{n}"}개를 지움” / “元の {"{n}"} 件を削除しました”.</ZNote>
      </ZSpec>

      <ZSpec title="Undo results — kept-only title · mixed · singular rule"
        when={<>An Undo of a copy that only <b>kept</b> changed copies says so in its title. When some items <b>couldn't be removed</b> as well, the failure wins the title (it needs action) and the kept items are still listed with their own line. <b>Singular rule, app-wide:</b> every English count string gets a <code>_one</code> variant used when n = 1 (“1 item”, “1 original”); ko and ja have no plural and use the same string. This covers <code>partial_*</code>, <code>failed_*</code>, <code>undo_*</code> and the new keys here.</>}>
        <ZStage variant="solo center" style={{ padding: 20, background: "var(--tasty-bg-app)" }}>
          <XThemes gap="var(--tasty-space-md)" render={() => (
            <div style={{ display: "flex", flexDirection: "column", gap: "var(--tasty-space-sm)", width: "var(--tasty-toast-max-width)" }}>
              <ZResult variant="warning" title="Undid copy · kept 1 changed item" lines={[["~/Documents/notes.md", "Changed after the copy, so it was kept"]]} />
              <ZResult variant="warning" title="Undid copy · 1 item couldn't be removed" lines={[["~/Documents/locked.db", "Couldn't remove (in use)"], ["~/Documents/notes.md", "Changed after the copy, so it was kept"]]} />
            </div>
          )} />
        </ZStage>
        <ZMeta
          specs={[["kept only", "explorer.result.undo_kept_copy “Undid copy · kept {n} changed items” · _one “Undid copy · kept 1 changed item”"], ["mixed", "undo_partial_copy wins the title · kept lines stay listed"], ["singular", "en `_one` for every count key · ko/ja one string"]]}
          tokens={[{ tok: "--tasty-toast-accent-warning", use: "both", color: "var(--tasty-toast-accent-warning)" }]} />
        <ZNote><code>undo_kept_copy</code> ko “복사 되돌림 · 바뀐 항목 {"{n}"}개 남겨 둠” / ja “コピーを元に戻しました · 変更された {"{n}"} 件を残しました” · <code>undo_partial_copy_one</code> “Undid copy · 1 item couldn't be removed” · <code>undo_partial_move_one</code> “Undid move · 1 item couldn't be put back”.</ZNote>
      </ZSpec>
    </ZSection>
  );
}

function ZMarquee() {
  return <div style={{ position: "absolute", left: 120, top: 40, width: 260, height: 70, background: "var(--tasty-explorer-marquee-bg)", border: "var(--tasty-border-width) solid var(--tasty-explorer-marquee-border)", pointerEvents: "none" }} />;
}
function ZEditRow({ value }) {
  return (
    <div style={{ display: "flex", alignItems: "center", gap: 8, height: "var(--tasty-table-cell-height)", padding: "0 10px", paddingLeft: "calc(10px + var(--tasty-explorer-create-indent))", background: "var(--tasty-surface-active)" }}>
      <span style={{ display: "inline-flex", flex: "none", color: "var(--tasty-text-muted)" }}>{ZK.ic.file}</span>
      <span style={{ flex: 1, minWidth: 0, maxWidth: 260 }}><ZInput block defaultValue={value} /></span>
      <span style={{ fontSize: "var(--tasty-font-size-caption)", color: "var(--tasty-text-muted)", whiteSpace: "nowrap" }}>in mockup-exports</span>
    </div>
  );
}

function RemainingSection() {
  const rows = ZKIND_ROWS;
  return (
    <ZSection id="remaining" title="Remaining states — links · keyboard item · drag-select · hidden files · show in folder · redo · create in folder (b11)">
      <ZSpec title="Links in the listing · broken link"
        when={<>A symlink keeps its <b>target's glyph</b> (folder, file, image) and gets a trailing <b>link</b> glyph 12 after the name in <span className="tok">--tasty-explorer-link-glyph</span>, in all three views (Grid: after the label's last line). Type reads “Link to {"{kind}"}”. A <b>broken</b> link has no target glyph to borrow: the link glyph takes the item slot in <span className="tok">--tasty-explorer-link-broken-fg</span> (warning), the name stays normal ink (no strike-through — nothing was deleted), Type reads “Broken link”, and the tooltip names the missing target.</>}>
        <ZStage variant="solo center" style={{ padding: 20, background: "var(--tasty-bg-app)" }}>
          <XThemes render={() => (
            <XCell w={560} h={170} status="4 items"><ZBody>
              <ZK.DetailRow glyph={ZK.ic.folder} name={<ZName text="projects" link />} size="—" date="2026-10-04 12:00" type="Link to Folder" />
              <ZK.DetailRow {...rows[5]} />
              <ZK.DetailRow glyph={<ZIcon name="link" size="var(--tasty-icon-size-md)" />} glyphColor="var(--tasty-explorer-link-broken-fg)" name="old-config.toml" size="—" date="2026-08-11 09:30" type="Broken link" />
              <ZK.DetailRow {...rows[2]} />
            </ZBody></XCell>
          )} />
        </ZStage>
        <ZMeta
          specs={[["link", "target glyph · trailing link 12 (explorer-link-glyph-size) · gap space-xs · explorer-link-glyph"], ["broken", "link glyph in the item slot · explorer-link-broken-fg → accent-warning · name normal · Type “Broken link” · tooltip “Target not found: {path}”"], ["views", "Detail · List · Grid alike"]]}
          tokens={[{ tok: "--tasty-explorer-link-glyph", use: "→ text-muted", color: "var(--tasty-explorer-link-glyph)" }, { tok: "--tasty-explorer-link-glyph-size", use: "→ icon-size-xs 12" }, { tok: "--tasty-explorer-link-broken-fg", use: "→ accent-warning", color: "var(--tasty-explorer-link-broken-fg)" }]} />
        <ZNote>i18n: <code>explorer.link.target_missing</code> “Target not found: {"{path}"}” / “대상을 찾을 수 없음: {"{path}"}” / “リンク先が見つかりません: {"{path}"}”.</ZNote>
      </ZSpec>

      <ZSpec title="Keyboard current item · drag-select rectangle"
        when={<>Arrow keys, Home / End and PageUp / Down move a <b>current item</b> separate from the selection. It is a 1px <b>inset</b> ring in <span className="tok">--tasty-explorer-cursor-ring</span> (border-focus) and is drawn only after keyboard movement; a mouse click hides it until the next key. On a selected row it sits on top of the selection fill. <b>Drag-select</b> from empty space draws a rectangle with <span className="tok">--tasty-explorer-marquee-bg</span> and a 1px <span className="tok">--tasty-explorer-marquee-border</span> (the drop-target tint pair). Near the top or bottom edge, inside <span className="tok">--tasty-explorer-autoscroll-zone</span> 24, the list scrolls faster the closer the pointer gets; nothing extra is drawn.</>}>
        <ZStage variant="solo center" style={{ padding: 20, background: "var(--tasty-bg-app)" }}>
          <XThemes render={(l) => (
            <XCell w={560} h={200} status={l === "Mocha" ? "2 of 5 selected" : "3 of 5 selected"}><ZBody>
              {rows.slice(0, 5).map((r, i) => {
                const sel = l === "Mocha" ? i === 2 || i === 3 : i >= 1 && i <= 3;
                const row = <ZK.DetailRow {...r} state={sel ? "selected" : undefined} />;
                return l === "Mocha" ? <ZCursor key={i} on={i === 3}>{row}</ZCursor> : <div key={i}>{row}</div>;
              })}
              {l === "Latte" && <ZMarquee />}
            </ZBody></XCell>
          )} />
        </ZStage>
        <ZMeta
          specs={[["current item", "1px inset ring · explorer-cursor-ring → border-focus · keyboard only · over selection"], ["marquee", "fill explorer-marquee-bg (tint-fill 0.12) · 1px explorer-marquee-border (tint-border 0.36) · no radius"], ["auto-scroll", "explorer-autoscroll-zone 24 at top / bottom · speed grows toward the edge · no indicator"], ["samples", "Mocha = keyboard ring on a selected row · Latte = drag-select"]]}
          tokens={[{ tok: "--tasty-explorer-cursor-ring", use: "→ border-focus", color: "var(--tasty-explorer-cursor-ring)" }, { tok: "--tasty-explorer-cursor-ring-width", use: "→ border-width 1" }, { tok: "--tasty-explorer-marquee-bg", use: "fill", color: "var(--tasty-explorer-marquee-bg)" }, { tok: "--tasty-explorer-marquee-border", use: "edge", color: "var(--tasty-explorer-marquee-border)" }, { tok: "--tasty-explorer-autoscroll-zone", use: "→ size-24" }]} />
      </ZSpec>

      <ZSpec title="Hidden files · show in enclosing folder · undo / redo"
        when={<><b>Hidden files</b> toggle from a <b>More</b> row that says the action (“Show hidden files” / “Hide hidden files”, no check mark) and the shortcut Ctrl+Shift+. (macOS ⌘⇧.). Off by default. When shown, hidden items use <span className="tok">--tasty-explorer-hidden-fg</span> for name and glyph — muted ink, not opacity, so they never look like cut items. While they are hidden the status line counts them: “5 items · 3 hidden”. <b>Show in enclosing folder</b> (search results and links) opens the folder that really holds the item, selects it, puts the current item on it and scrolls it into view; no flash. <b>Redo</b> lives beside Undo in the empty-area context menu and on Ctrl+Shift+Z / Ctrl+Y (macOS ⌘⇧Z). Each explorer keeps up to <b>10</b> steps; no history list is shown. A step that can no longer be undone leaves the row disabled with the reason in its tooltip.</>}>
        <ZStage variant="solo center" style={{ padding: 20, background: "var(--tasty-bg-app)", gap: 16, flexWrap: "wrap", alignItems: "flex-start" }}>
          <XCol label="hidden shown" w={400}>
            <XCell w={400} h={180} toolbar={<XToolbar narrow />} status="8 items"><ZBody>
              <ZK.DetailRow glyph={<span style={{ color: "var(--tasty-explorer-hidden-fg)", display: "inline-flex" }}>{ZK.ic.folder}</span>} name={<ZName text=".git" fg="var(--tasty-explorer-hidden-fg)" />} size="—" date="2026-10-09 18:12" type="Folder" />
              <ZK.DetailRow glyph={<span style={{ color: "var(--tasty-explorer-hidden-fg)", display: "inline-flex" }}>{ZK.ic.file}</span>} name={<ZName text=".env" fg="var(--tasty-explorer-hidden-fg)" />} size="210 B" date="2026-10-01 07:45" type="ENV file" />
              {rows.slice(0, 3).map((r, i) => <ZK.DetailRow key={i} {...r} />)}
            </ZBody></XCell>
          </XCol>
          <XCol label="hidden off — status count" w={400}>
            <XCell w={400} h={120} toolbar={<XToolbar narrow />} status="5 items · 3 hidden"><ZBody>{rows.slice(0, 2).map((r, i) => <ZK.DetailRow key={i} {...r} />)}</ZBody></XCell>
          </XCol>
          <XCol label="More — hidden row">
            <div style={zMenu}>
              <ZMenuItem icon={<ZIcon name="folderPlus" />} label="New folder" />
              <ZMenuItem icon={<ZIcon name="search" />} label="Find" />
              <ZMenuItem icon={<ZIcon name="eye" />} label="Show hidden files" shortcut="Ctrl+Shift+." />
            </div>
          </XCol>
          <XCol label="search result — context menu">
            <div style={zMenu}>
              <ZMenuItem label="Open" />
              <ZMenuItem icon={<ZIcon name="folderOpen" />} label="Show in enclosing folder" />
              <ZMenuItem separator />
              <ZMenuItem label="Copy path" />
            </div>
          </XCol>
          <XCol label="empty area — undo / redo">
            <div style={zMenu}>
              <ZMenuItem label="Undo move 3 items" shortcut="Ctrl+Z" />
              <ZMenuItem label="Redo rename" shortcut="Ctrl+Shift+Z" />
              <ZMenuItem separator />
              <ZMenuItem label="Undo trash 2 items" disabled title="Can't undo: the items were emptied from the trash" />
            </div>
          </XCol>
        </ZStage>
        <ZMeta
          specs={[["hidden toggle", "More row · action words · Ctrl+Shift+. / ⌘⇧. · default off · per explorer, remembered"], ["hidden shown", "name + glyph explorer-hidden-fg (→ text-muted) · no opacity"], ["hidden off", "status “{n} items · {h} hidden”"], ["show in folder", "opens the real parent · selects + current item · scrolls into view · no flash"], ["redo", "context menu beside Undo · Ctrl+Shift+Z · Ctrl+Y · ⌘⇧Z"], ["history", "10 steps per explorer · no list · stale step = disabled row + tooltip reason"], ["Undo card", "unchanged — one step, no Redo button"]]}
          tokens={[{ tok: "--tasty-explorer-hidden-fg", use: "→ text-muted", color: "var(--tasty-explorer-hidden-fg)" }]} />
        <ZNote>i18n: <code>explorer.more.hidden_show</code> “Show hidden files” / “숨김 파일 보기” / “隠しファイルを表示” · <code>.hidden_hide</code> “Hide hidden files” / “숨김 파일 숨기기” / “隠しファイルを非表示” · <code>explorer.status.hidden</code> “{"{n}"} hidden” / “숨김 {"{n}"}개” / “非表示 {"{n}"}” · <code>explorer.menu.show_in_folder</code> “Show in enclosing folder” / “들어 있는 폴더에서 보기” / “含まれているフォルダで表示” · <code>explorer.menu.redo</code> “Redo {"{op}"}” / “다시 실행: {"{op}"}” / “やり直す: {"{op}"}” · <code>explorer.menu.undo_stale</code> “Can't undo: {"{reason}"}” / “되돌릴 수 없음: {"{reason}"}” / “元に戻せません: {"{reason}"}”.</ZNote>
      </ZSpec>

      <ZSpec title="Create inside a folder — input under the target row"
        when={<>“New file / New folder” from a <b>folder row's</b> menu puts the input row <b>directly under that folder</b>, indented by <span className="tok">--tasty-explorer-create-indent</span> 16, with a muted “in {"{folder}"}” after the field. The target folder takes the <b>drop-target ring</b> while the field is open, so the destination is visible in every view. In <b>Grid</b> the new cell appears right after the folder cell. Enter creates the item inside the folder (the listing does not navigate); Esc cancels.</>}>
        <ZStage variant="solo center" style={{ padding: 20, background: "var(--tasty-bg-app)" }}>
          <XThemes render={() => (
            <XCell w={560} h={180} status="5 items"><ZBody>
              <ZK.DetailRow {...rows[0]} />
              <div style={{ background: "var(--tasty-explorer-drop-target-bg)", boxShadow: "inset 0 0 0 var(--tasty-border-width) var(--tasty-explorer-drop-target-border)", borderRadius: "var(--tasty-radius-sm)" }}><ZK.DetailRow glyph={ZK.ic.folder} name="mockup-exports" size="—" date="2026-06-20 14:30" type="Folder" /></div>
              <ZEditRow value="untitled.txt" />
              <ZK.DetailRow {...rows[2]} />
            </ZBody></XCell>
          )} />
        </ZStage>
        <ZMeta
          specs={[["input", "under the target row · indent explorer-create-indent 16 · caption “in {folder}” muted"], ["target", "drop-target ring + fill while the field is open"], ["Grid", "new cell right after the folder cell"], ["keys", "Enter creates inside · Esc cancels · listing stays"]]}
          tokens={[{ tok: "--tasty-explorer-create-indent", use: "→ space-lg 16" }, { tok: "--tasty-explorer-drop-target-border", use: "target", color: "var(--tasty-explorer-drop-target-border)" }]} />
        <ZNote>i18n: <code>explorer.create.in_folder</code> “in {"{folder}"}” / “{"{folder}"} 안에” / “{"{folder}"} 内”.</ZNote>
      </ZSpec>
    </ZSection>
  );
}

window.ExplorerOpsB11 = { FollowupsB11Section, RemainingSection };
