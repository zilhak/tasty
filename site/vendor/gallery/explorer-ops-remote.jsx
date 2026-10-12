// Tasty Gallery — Explorer file operations: remote (attach) explorer writes (2026-10-12, request explorer-remote-write).
// D1 unsupported remote · D2 waiting for the remote reply · D3 connection lost · D4 remote progress / queue · D5 drag chip reasons.
// Loaded before explorer-ops.jsx; Y* parts are read from window.ExplorerOpsY at render time.
(() => {
const { Section, Spec, Stage, Meta, Note } = window.Gallery;
const { IconButton, Button, Input, MenuItem, Kbd, Spinner, Icon, Tag } = window.TastyDesignSystem_41fd3f;
const K = window.ExplorerKit;
const { XCol, XThemes, XToolbar, XCell, XDetail, XROWS, XDragChip } = window.ExplorerOpsParts;
const Y = () => window.ExplorerOpsY;

const REASON = {
  old: { en: "Update Tasty on build-eu to change remote files.", ko: "build-eu의 Tasty를 업데이트하면 원격 파일을 바꿀 수 있습니다.", ja: "build-eu の Tasty を更新すると、リモートのファイルを変更できます。" },
  headless: { en: "build-eu can't change files (server without a window).", ko: "build-eu는 파일을 바꿀 수 없습니다(화면 없는 서버).", ja: "build-eu はファイルを変更できません(画面のないサーバー)。" },
};
function Tip({ children }) {
  return <div role="tooltip" style={{ maxWidth: "var(--tasty-size-240)", padding: "var(--tasty-space-xs) var(--tasty-space-sm)", background: "var(--tasty-menu-bg)", border: "var(--tasty-border-width) solid var(--tasty-menu-border)", borderRadius: "var(--tasty-radius)", boxShadow: "var(--tasty-shadow-popover)", fontSize: "var(--tasty-font-size-caption)", color: "var(--tasty-text-primary)", lineHeight: "var(--tasty-line-height-ui)" }}>{children}</div>;
}
// Remote host mark for the progress line + queue: remote glyph + host, workspace-mirror-fg (accent-remote).
function Host({ name = "build-eu" }) {
  return (
    <span style={{ display: "inline-flex", alignItems: "center", gap: "var(--tasty-workspace-mirror-gap)", flex: "none", fontFamily: "var(--tasty-font-mono)", fontSize: "var(--tasty-font-size-caption)", color: "var(--tasty-workspace-mirror-fg)" }}>
      <Icon name="remote" size="var(--tasty-workspace-mirror-icon-size)" />{name}
    </span>
  );
}
function PendingDetailRow({ glyph = K.ic.folder, value }) {
  return (
    <div style={{ display: "grid", gridTemplateColumns: "1fr 80px 132px 92px", alignItems: "center", height: "var(--tasty-table-cell-height)", padding: "0 10px", background: "var(--tasty-surface-active)" }}>
      <span style={{ display: "flex", alignItems: "center", gap: 8, minWidth: 0 }}>
        <span style={{ display: "inline-flex", flex: "none", color: "var(--tasty-text-muted)" }}>{glyph}</span>
        <span style={{ flex: 1, minWidth: 0 }}><Input block readOnly defaultValue={value} /></span>
        <span style={{ display: "inline-flex", flex: "none", color: "var(--tasty-text-muted)" }} title="Waiting for build-eu"><Spinner size="var(--tasty-icon-size-sm)" /></span>
      </span>
      <span /><span /><span />
    </div>
  );
}
function RenamePending({ lang = "en" }) {
  const t = { en: ["Rename", "Renaming on build-eu…", "Cancel", "Rename"], ko: ["이름 바꾸기", "build-eu에서 이름 바꾸는 중…", "취소", "이름 바꾸기"], ja: ["名前の変更", "build-eu で名前を変更中…", "キャンセル", "名前を変更"] }[lang];
  return (
    <div style={{ width: "var(--tasty-size-360)", maxWidth: "100%", background: "var(--tasty-bg-panel)", border: "var(--tasty-border-width) solid var(--tasty-border-strong)", borderRadius: "var(--tasty-radius)", boxShadow: "var(--tasty-shadow-modal)", padding: "var(--tasty-space-md) var(--tasty-size-14)", display: "flex", flexDirection: "column", gap: "var(--tasty-space-sm)" }}>
      <span style={{ fontSize: "var(--tasty-font-size-body)", fontWeight: "var(--tasty-font-weight-semibold)", color: "var(--tasty-text-primary)" }}>{t[0]}</span>
      <Input block readOnly defaultValue="report-final.pdf" />
      <span style={{ display: "flex", alignItems: "center", gap: "var(--tasty-space-sm)", fontSize: "var(--tasty-font-size-caption)", color: "var(--tasty-text-muted)" }}><Spinner size="var(--tasty-icon-size-sm)" />{t[1]}</span>
      <div style={{ display: "flex", justifyContent: "flex-end", gap: "var(--tasty-space-sm)" }}><Button variant="ghost" size="sm" disabled>{t[2]}</Button><Button variant="primary" size="sm" disabled>{t[3]}</Button></div>
    </div>
  );
}
function RemoteQueue() {
  const row = (title, sub, run) => (
    <div style={{ display: "flex", alignItems: "center", gap: "var(--tasty-space-sm)", minHeight: "var(--tasty-menu-item-height)", padding: "var(--tasty-space-xs) var(--tasty-space-sm)" }}>
      <span style={{ display: "inline-flex", flex: "none", color: "var(--tasty-text-muted)" }}>{run ? <Spinner size="var(--tasty-icon-size-sm)" /> : <Icon name="layers" size="var(--tasty-icon-size-sm)" />}</span>
      <span style={{ flex: 1, minWidth: 0, display: "flex", flexDirection: "column" }}>
        <span style={{ fontSize: "var(--tasty-font-size-body)", color: "var(--tasty-text-primary)", overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>{title}</span>
        <span style={{ fontSize: "var(--tasty-font-size-caption)", color: "var(--tasty-text-muted)" }}>{sub}</span>
      </span>
      <IconButton size="sm" aria-label={run ? "Cancel" : "Remove from queue"} title={run ? "Cancel" : "Remove from queue"}>{K.ic.x}</IconButton>
    </div>
  );
  return (
    <div style={{ position: "absolute", right: "var(--tasty-space-sm)", bottom: "calc(var(--tasty-status-bar-height) + var(--tasty-space-xs))", width: "var(--tasty-size-288)", background: "var(--tasty-menu-bg)", border: "var(--tasty-border-width) solid var(--tasty-menu-border)", borderRadius: "var(--tasty-menu-radius)", boxShadow: "var(--tasty-shadow-popover)", padding: "var(--tasty-space-xs)" }}>
      <div style={{ display: "flex", alignItems: "center", gap: "var(--tasty-space-sm)", padding: "var(--tasty-space-xs) var(--tasty-space-sm)", fontSize: "var(--tasty-font-size-caption)", color: "var(--tasty-text-muted)" }}>On <Host /> · one at a time</div>
      {row("Copy 6 items to logs/archive", "2 of 6 · 14 / 52 MB", true)}
      {row("Move to Trash 3 items", "Queued", false)}
    </div>
  );
}

function RemoteWriteSection() {
  const { YResult, YProgressStatus, YStatusRight, YToastAt } = Y();
  const remoteTb = <XToolbar path="build-eu:~/logs" />;
  return (
    <Section id="remotewrite" title="Remote explorer — writes that differ from local (2026-10-12)">
      <Spec title="D1 · A remote that can't write — rows shown disabled, reason in the tooltip and toast"
        when={<>A remote running a new Tasty shows the write rows exactly like local. A remote that <b>can't</b> write (older Tasty, or a headless server) now <b>shows the rows disabled</b> (option b) instead of hiding them. Reason: the rows tell the user the command exists and where it went; hiding them made the menu look broken next to a writable remote in the next cell. The native menu can only grey a row and add a tooltip (Linux, macOS); Windows shows no tooltip, so the shortcut path keeps the <b>toast</b>, and the toast now names the reason. Two reasons, two strings. The toolbar create group follows the same rule (disabled, tooltip = reason), the same way a local folder without write access does.</>}>
        <Stage variant="solo center" style={{ padding: 20, background: "var(--tasty-bg-app)", gap: 18, flexWrap: "wrap", alignItems: "flex-start" }}>
          <K.CtxMenu title="Writable remote (new Tasty)">
            <MenuItem label="Copy path" icon={K.ic.link} />
            <MenuItem label="Copy" icon={K.ic.copy} shortcut={<Kbd keys="Ctrl+C" />} />
            <MenuItem label="Cut" icon={K.ic.scissors} shortcut={<Kbd keys="Ctrl+X" />} />
            <MenuItem label="Paste into" icon={K.ic.paste} />
            <MenuItem separator />
            <MenuItem label="New folder" icon={<Icon name="folderPlus" />} />
            <MenuItem label="New file" icon={<Icon name="filePlus" />} />
            <MenuItem label="Rename" icon={K.ic.rename} />
            <MenuItem label="Move to Trash" icon={K.ic.trash} danger />
          </K.CtxMenu>
          <div style={{ display: "flex", flexDirection: "column", gap: "var(--tasty-space-sm)" }}>
            <K.CtxMenu title="Remote that can't write — disabled">
              <MenuItem label="Copy path" icon={K.ic.link} />
              <MenuItem label="Copy" icon={K.ic.copy} shortcut={<Kbd keys="Ctrl+C" />} />
              <MenuItem label="Cut" icon={K.ic.scissors} disabled />
              <MenuItem label="Paste into" icon={K.ic.paste} disabled />
              <MenuItem separator />
              <MenuItem label="New folder" icon={<Icon name="folderPlus" />} disabled />
              <MenuItem label="New file" icon={<Icon name="filePlus" />} disabled />
              <MenuItem label="Rename" icon={K.ic.rename} disabled />
              <MenuItem label="Move to Trash" icon={K.ic.trash} disabled />
            </K.CtxMenu>
            <Tip>{REASON.old.en}</Tip>
          </div>
          <XCol label="toolbar · can't write — create disabled" w={560}><div style={{ border: "var(--tasty-border-width) solid var(--tasty-border-default)", borderRadius: "var(--tasty-radius)", overflow: "hidden" }}><XToolbar denied path="build-eu:~/logs" /></div></XCol>
          <XCol label="shortcut on a remote that can't write — toast (en · ko · ja)" w={340}>
            <XThemes gap="var(--tasty-space-sm)" render={() => (
              <div style={{ display: "flex", flexDirection: "column", gap: "var(--tasty-space-sm)", width: "var(--tasty-toast-max-width)", maxWidth: "100%" }}>
                <YResult variant="info" title={REASON.old.en} />
                <YResult variant="info" title={REASON.headless.ko} />
                <YResult variant="info" title={REASON.headless.ja} />
              </div>
            )} />
          </XCol>
        </Stage>
        <Meta
          specs={[["choice", "(b) show disabled · tooltip = reason (Linux / macOS)"], ["rows", "Cut · Paste · New folder · New file · Rename · Move to Trash"], ["read rows", "Copy path · Copy · Open stay enabled"], ["toolbar", "create group disabled · tooltip = reason"], ["shortcut", "info toast in the cell · the same reason string"], ["reasons", "old Tasty → update hint · headless → can't change files"]]}
          tokens={[{ tok: "--tasty-state-disabled-fg", use: "disabled row / button", color: "var(--tasty-state-disabled-fg)" }, { tok: "--tasty-toast-accent-info", use: "reason toast", color: "var(--tasty-toast-accent-info)" }]} />
        <Note>i18n: <code>explorer.remote.write_needs_update</code> “Update Tasty on {"{host}"} to change remote files.” / “{"{host}"}의 Tasty를 업데이트하면 원격 파일을 바꿀 수 있습니다.” / “{"{host}"} の Tasty を更新すると、リモートのファイルを変更できます。” · <code>explorer.remote.write_headless</code> “{"{host}"} can't change files (server without a window).” / “{"{host}"}는 파일을 바꿀 수 없습니다(화면 없는 서버).” / “{"{host}"} はファイルを変更できません(画面のないサーバー)。”. The old single “remote write not supported” toast key goes. Tooltip and toast share the key.</Note>
      </Spec>

      <Spec title="D2 · Waiting for the remote — the input row stays, read-only, with a spinner"
        when={<>Local creates the item at once; a remote needs a round trip. After <span className="ic">↵</span> the <b>input row stays where it is</b>, the field turns <b>read-only</b> (text kept, so the user sees what was sent) and a <b>Spinner</b> takes the trailing slot. Nothing moves, so a fast reply looks like the local flow and a slow one still shows that work is in progress. On success the row becomes the real item at its sorted place, selected. On failure the field becomes editable again with the text kept, and the error toast shows as today. <span className="ic">Esc</span> does nothing while waiting: the request is already on the remote. The <b>Rename popup</b> works the same way: it stays open, field read-only, a “Renaming on {"{host}"}…” line with a Spinner, both buttons disabled; it closes on success.</>}>
        <Stage variant="solo center" style={{ padding: 20, background: "var(--tasty-bg-app)", gap: 18, flexWrap: "wrap", alignItems: "flex-start" }}>
          <XCol label="new folder · waiting for build-eu" w={480}>
            <XCell w={480} h={210} toolbar={remoteTb} status="5 items"><XDetail rows={XROWS.slice(1)} before={<PendingDetailRow value="New folder" />} /></XCell>
          </XCol>
          <XCol label="Rename popup · waiting (en · ko · ja)" w={360}>
            <div style={{ display: "flex", flexDirection: "column", gap: "var(--tasty-space-sm)" }}><RenamePending /><RenamePending lang="ko" /><RenamePending lang="ja" /></div>
          </XCol>
        </Stage>
        <Meta
          specs={[["input row", "stays in place · Input readOnly · text kept"], ["indicator", "Spinner icon-size-sm · trailing slot · tooltip “Waiting for {host}”"], ["Esc / blur", "ignored while waiting"], ["success", "row → item at its sorted place, selected"], ["failure", "field editable again, text kept · error toast (unchanged)"], ["Rename popup", "open · field readOnly · Spinner line · Cancel + Rename disabled · closes on success"]]}
          tokens={[{ tok: "--tasty-input-readonly-fg", use: "sent text", color: "var(--tasty-input-readonly-fg)" }, { tok: "--tasty-spinner-indicator", use: "waiting", color: "var(--tasty-spinner-indicator)" }]} />
        <Note>i18n: <code>explorer.remote.waiting</code> “Waiting for {"{host}"}” / “{"{host}"} 응답 기다리는 중” / “{"{host}"} の応答を待っています” · <code>explorer.remote.renaming</code> “Renaming on {"{host}"}…” / “{"{host}"}에서 이름 바꾸는 중…” / “{"{host}"} で名前を変更中…”.</Note>
      </Spec>

      <Spec title="D3 · Connection lost — result unknown"
        when={<>When the link drops mid-job or while a conflict question is open, the remote stops at the item it was on and this side never gets the report. The result card says exactly that: <b>warning</b> tone, it <b>stays until dismissed</b>, and it does <b>not</b> claim that nothing changed. There is no Retry and no Undo (the current state is unknown, so neither can be offered honestly); the only control is ×. A second line says the folder will be read again on reconnect, because that is when the user can see what happened. An open conflict popup for that job closes with it.</>}>
        <Stage variant="solo center" style={{ padding: 20, background: "var(--tasty-bg-app)", gap: 18, flexWrap: "wrap", alignItems: "flex-start" }}>
          <XCol label="in place" w={480}>
            <XCell w={480} h={230} toolbar={remoteTb} status="Disconnected from build-eu" overlay={<YToastAt><YResult variant="warning" title={<><div>Connection to build-eu lost · result unknown</div><div style={{ marginTop: "var(--tasty-space-xs)", fontSize: "var(--tasty-font-size-caption)", color: "var(--tasty-text-secondary)" }}>The copy stopped at the item it was on. Some items may already have changed. The folder reloads when the connection is back.</div></>} /></YToastAt>}><XDetail /></XCell>
          </XCol>
          <XThemes gap="var(--tasty-space-md)" render={() => (
            <div style={{ display: "flex", flexDirection: "column", gap: "var(--tasty-space-sm)", width: "var(--tasty-toast-max-width)", maxWidth: "100%" }}>
              <YResult variant="warning" title={<><div>build-eu 연결이 끊겨 결과를 알 수 없음</div><div style={{ marginTop: "var(--tasty-space-xs)", fontSize: "var(--tasty-font-size-caption)", color: "var(--tasty-text-secondary)" }}>복사는 하던 항목에서 멈췄습니다. 일부 항목은 이미 바뀌었을 수 있습니다. 다시 연결되면 폴더를 다시 읽습니다.</div></>} />
              <YResult variant="warning" title={<><div>build-eu との接続が切れたため結果が不明です</div><div style={{ marginTop: "var(--tasty-space-xs)", fontSize: "var(--tasty-font-size-caption)", color: "var(--tasty-text-secondary)" }}>コピーは処理中の項目で止まりました。一部の項目はすでに変更されている可能性があります。再接続するとフォルダーを再読み込みします。</div></>} />
            </div>
          )} />
        </Stage>
        <Meta
          specs={[["tone", "warning · stays until dismissed"], ["title", "“Connection to {host} lost · result unknown”"], ["body", "caption · text-secondary · stopped at the item · may have changed · reload on reconnect"], ["actions", "× only — no Retry, no Undo"], ["conflict popup", "closes with the job"], ["verb", "{op} = copy / move / move to Trash / rename / new folder / new file"]]}
          tokens={[{ tok: "--tasty-toast-accent-warning", use: "card", color: "var(--tasty-toast-accent-warning)" }]} />
        <Note>i18n: <code>explorer.remote.lost_title</code> “Connection to {"{host}"} lost · result unknown” / “{"{host}"} 연결이 끊겨 결과를 알 수 없음” / “{"{host}"} との接続が切れたため結果が不明です” · <code>explorer.remote.lost_body</code> “The {"{op}"} stopped at the item it was on. Some items may already have changed. The folder reloads when the connection is back.” / “{"{op}"}은(는) 하던 항목에서 멈췄습니다. 일부 항목은 이미 바뀌었을 수 있습니다. 다시 연결되면 폴더를 다시 읽습니다.” / “{"{op}"}は処理中の項目で止まりました。一部の項目はすでに変更されている可能性があります。再接続するとフォルダーを再読み込みします。”</Note>
      </Spec>

      <Spec title="D4 · Remote progress line and queue — host shown"
        when={<>A remote job uses the same status line, bar and Cancel. The host is shown at the <b>start of the line</b> (remote glyph + host, <span className="tok">--tasty-workspace-mirror-fg</span>), because remote work waits in its own queue and runs one job at a time per connection; without the mark a slow job looks stuck behind nothing. “+n queued” counts <b>only this cell's remote queue</b>. The queue popover gets a header line “On {"{host}"} · one at a time” and otherwise the same rows.</>}>
        <Stage variant="solo center" style={{ padding: 20, background: "var(--tasty-bg-app)" }}>
          <XThemes render={(l) => (
            <XCell w={520} h={l === "Mocha" ? 290 : 200} toolbar={remoteTb} status={<><Host /><YProgressStatus pct={27} text="Copying 2 of 6 · nginx-access-2026-10-11.log.gz" /></>} statusRight={<YStatusRight bytes="14 / 52 MB" queued={1} />} overlay={l === "Mocha" ? <RemoteQueue /> : null}>
              <XDetail />
            </XCell>
          )} />
        </Stage>
        <Meta
          specs={[["host mark", "remote glyph (workspace-mirror-icon-size) + host · mono caption · workspace-mirror-fg · first on the line"], ["queued", "“+n queued” = this cell's remote queue"], ["queue header", "“On {host} · one at a time” · caption · muted"], ["rest", "same bar · Cancel · rows as local"]]}
          tokens={[{ tok: "--tasty-workspace-mirror-fg", use: "host mark", color: "var(--tasty-workspace-mirror-fg)" }, { tok: "--tasty-workspace-mirror-gap", use: "glyph ↔ host" }]} />
        <Note>i18n: <code>explorer.remote.queue_title</code> “On {"{host}"} · one at a time” / “{"{host}"} · 한 번에 하나씩” / “{"{host}"} · 一度に 1 件”. The host mark has no string of its own (it is the host name).</Note>
      </Spec>

      <Spec title="D5 · Drag chip — new refusal reasons · conflict popup unchanged"
        when={<>Drops inside one remote now work. Two refusals get their own line-2 reason: between this computer and a remote, or between two remotes, says <b>different computer</b>; a remote that can't write no longer says “read-only” (writable remotes exist now) but <b>“this remote can't change files”</b>. The name-conflict popup is used <b>as is</b> for remote jobs.</>}>
        <Stage variant="solo center" style={{ padding: 20, background: "var(--tasty-bg-app)" }}>
          <XThemes render={() => (
            <div style={{ display: "flex", flexDirection: "column", gap: 10, alignItems: "flex-start" }}>
              <XDragChip label="nginx.conf" op="move" target="conf.d" />
              <XDragChip glyph={<Icon name="layers" />} label="3 items" op="refused" reason="different computer" />
              <XDragChip glyph={<Icon name="layers" />} label="3 items" op="refused" reason="this remote can't change files" />
              <XDragChip glyph={<Icon name="layers" />} label="항목 3개" op="refused" reason="다른 컴퓨터" />
              <XDragChip glyph={<Icon name="layers" />} label="3 個の項目" op="refused" reason="別のコンピューター" />
            </div>
          )} />
        </Stage>
        <Meta
          specs={[["inside one remote", "Move to / Copy to as local"], ["local ↔ remote · remote ↔ remote", "refused · “different computer”"], ["remote can't write", "refused · “this remote can't change files”"], ["conflict popup", "unchanged"]]}
          tokens={[{ tok: "--tasty-explorer-drag-refused-fg", use: "refused line", color: "var(--tasty-explorer-drag-refused-fg)" }]} />
        <Note>i18n: <code>explorer.drag.reason.other_computer</code> “different computer” / “다른 컴퓨터” / “別のコンピューター” · <code>explorer.drag.reason.remote</code> changes from “remote folders are read-only” to “this remote can't change files” / “이 원격은 파일을 바꿀 수 없음” / “このリモートはファイルを変更できません”.</Note>
      </Spec>
    </Section>
  );
}

window.ExplorerOpsRemote = { RemoteWriteSection };
})();
