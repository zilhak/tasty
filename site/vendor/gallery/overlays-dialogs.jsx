// Tasty Gallery — Overlays · Dialogs (modal confirm/edit surfaces)
// One of the four Overlays sub-pages. Frame components are shared from
// overlays-shared.jsx (window.OverlaysShared); this file holds only the
// page's specimens + nav. See the other overlays-*.jsx for the rest.
const { Section, Spec, Stage, Meta, Note, Do, Dont, GIcon } = window.Gallery;
const { Kbd } = window.TastyDesignSystem_41fd3f;
const { Backdrop, ApprovalFrame, ConvertFrame, FileHandlerFrame, PresetFrame, MarkdownOpenFrame, RenameFrame, CategoryEditFrame, CategoryDeleteFrame, TransferProgressFrame, TransferErrorFrame } = window.OverlaysShared;

const NAV = [
  { id: "scrim", label: "Scrim & frame" },
  { id: "approval", label: "Agent approval" },
  { id: "convert", label: "Convert surface" },
  { id: "filehandler", label: "File handler picker" },
  { id: "preset", label: "Apply preset" },
  { id: "markdown", label: "Markdown open" },
  { id: "rename", label: "Rename popup" },
  { id: "category", label: "Workspace category" },
  { id: "transfer", label: "Remote transfer" },
];

function Page() {
  return (
    <>
<Section id="scrim" title="Scrim & frame — the shared recipe">
        <Spec title="Every overlay sits on the same scrim"
          when={<>Modals are the <b>one place</b> the system uses shadow. The recipe: a <b>50% black scrim</b> (+1px blur) over the app, a frame of <b>--tasty-bg-panel</b> (or <b>--tasty-surface-raised</b> for the palette) with a <b>1px --tasty-border-strong</b> edge and a soft drop shadow. Click-scrim or <Kbd keys="Esc" /> dismisses.</>}>
          <Stage variant="solo center">
            <Backdrop height={300} blur>
              <div align="center" style={{ width: 320, background: "var(--tasty-bg-panel)", border: "1px solid var(--tasty-border-strong)",
                borderRadius: "var(--tasty-radius)", padding: 18, boxShadow: "0 20px 60px rgba(0,0,0,.55)", textAlign: "center" }}>
                <div style={{ fontSize: 14, fontWeight: 600, marginBottom: 6 }}>This is the frame</div>
                <div style={{ fontSize: 12.5, color: "var(--tasty-text-muted)" }}>--tasty-bg-panel · 1px --tasty-border-strong · soft shadow · sits on a 50% scrim</div>
              </div>
            </Backdrop>
          </Stage>
          <Meta
            specs={[["scrim", "rgba(0,0,0,.5) + blur(1px)"], ["frame fill", <span className="tok">--tasty-bg-panel</span>], ["border", <>1px <span className="tok">--tasty-border-strong</span></>], ["shadow", "0 20px 60px / .55 (modals only)"], ["dismiss", <>scrim click · <span className="ic">Esc</span></>]]}
            tokens={[{ tok: "--tasty-bg-panel", use: "frame", color: "var(--tasty-bg-panel)" }, { tok: "--tasty-border-strong", use: "edge", color: "var(--tasty-border-strong)" }, { tok: "--tasty-radius", use: "4px corners" }]} />
          <Note>The two anchor positions: <b>centered</b> (dialogs, settings) and <b>top-aligned</b> (command palette, ~40px from top). Nothing else.</Note>
        </Spec>
      </Section>

<Section id="approval" title="Agent approval">
        <Spec title="Approval popup — the mauve gate"
          when={<>Shown when an agent requests a privileged or destructive action. The <b>mauve agent accent</b> in the header and tag makes it unmistakable. Show the <b>exact command</b>, the requesting plugin, the target surface ID, and the permission tags. Three responses: deny / allow once / always.</>}>
          <Stage variant="solo center"><Backdrop height={420}><ApprovalFrame /></Backdrop></Stage>
          <Meta
            specs={[["width", "440px"], ["header dot", <span className="tok">--tasty-accent-agent</span>], ["command", "mono on #000"], ["actions", "ghost / secondary / agent"]]}
            tokens={[{ tok: "--tasty-accent-agent", use: "agent gate", color: "var(--tasty-accent-agent)" }, { tok: "--tasty-accent-danger", use: "destructive tag", color: "var(--tasty-accent-danger)" }, { tok: "--tasty-bg-panel", use: "frame", color: "var(--tasty-bg-panel)" }]} />
          <Do><b>Do</b> name the actor and show the literal command. The user is granting authority — make the stakes legible.</Do>
        </Spec>
      </Section>

<Section id="convert" title="Convert surface">
        <Spec title="Convert popup — change the renderer"
          when={<>A small dialog that converts a surface from one kind to another (terminal → markdown / editor / log viewer) <b>without losing the running process or scrollback</b> — only the renderer changes. From-kind shown as a Tag, target as a Select, a one-line hint, cancel/confirm.</>}>
          <Stage variant="solo center"><Backdrop height={280}><div align="center"><ConvertFrame /></div></Backdrop></Stage>
          <Meta
            specs={[["width", "400px"], ["from", "Tag (read-only)"], ["to", "Select of kinds"], ["actions", "Cancel / Convert"]]}
            tokens={[{ tok: "--tasty-bg-panel", use: "frame", color: "var(--tasty-bg-panel)" }, { tok: "--tasty-accent-primary", use: "confirm", color: "var(--tasty-accent-primary)" }, { tok: "--tasty-text-muted", use: "hint", color: "var(--tasty-text-muted)" }]} />
        </Spec>
      </Section>

<Section id="filehandler" title="File handler picker">
        <Spec title="Open with… — pick a handler"
          when={<>Shown when a file can be opened more than one way. A <b>list of handlers</b> — built-in (preview / editor / pager) and <b>plugin-contributed</b> — each with an icon, name, and origin. The current default is tagged; a <b>“Always open …”</b> checkbox sets it. Selected row uses the accent left-bar pattern.</>}>
          <Stage variant="solo center"><Backdrop height={360}><div align="center"><FileHandlerFrame /></div></Backdrop></Stage>
          <Meta
            specs={[["width", "420px"], ["rows", "icon · name · origin"], ["default", "Tag accent"], ["selected", <>2px accent left bar</>], ["footer", "Always-default + actions"]]}
            tokens={[{ tok: "--tasty-surface-active", use: "selected", color: "var(--tasty-surface-active)" }, { tok: "--tasty-accent-primary", use: "select bar", color: "var(--tasty-accent-primary)" }, { tok: "--tasty-accent-agent", use: "plugin handler", color: "var(--tasty-accent-agent)" }]} />
          <Note>Plugin handlers are marked with the mauve agent accent — you always know a third party is handling the open.</Note>
        </Spec>
      </Section>

<Section id="preset" title="Apply preset">
        <Spec title="Apply preset — scope then pick"
          when={<>Applies a saved layout preset at one of three scopes — <b>Workspace / Tab / Pane</b> — chosen with a segmented control in the header. Below it, a selectable list of presets with a mono summary of what each lays out. The confirm button names the active scope (“Apply to workspace”).</>}>
          <Stage variant="solo center"><Backdrop height={340}><div align="center"><PresetFrame /></div></Backdrop></Stage>
          <Meta
            specs={[["width", "440px"], ["scope", "Workspace · Tab · Pane (segmented)"], ["rows", "name + mono summary"], ["confirm", "names the scope"]]}
            tokens={[{ tok: "--tasty-accent-primary", use: "active scope / select", color: "var(--tasty-accent-primary)" }, { tok: "--tasty-surface-active", use: "chosen preset", color: "var(--tasty-surface-active)" }, { tok: "--tasty-font-mono", use: "layout summary" }]} />
        </Spec>
      </Section>

<Section id="markdown" title="Markdown open">
        <Spec title="Markdown open — preview vs raw"
          when={<>A two-choice confirm shown when opening a <code>.md</code> file: <b>rendered preview</b> or <b>raw text</b> in the editor. Two equal card-buttons with the default pre-selected (accent ring), filename in mono, cancel + confirm. The pattern for any “open as A or B” fork.</>}>
          <Stage variant="solo center"><Backdrop height={300}><div align="center"><MarkdownOpenFrame /></div></Backdrop></Stage>
          <Meta
            specs={[["width", "420px"], ["choices", "2 equal card-buttons"], ["default", "accent ring + fill"], ["confirm", "names the choice"]]}
            tokens={[{ tok: "--tasty-accent-primary", use: "selected choice", color: "var(--tasty-accent-primary)" }, { tok: "--tasty-surface-raised", use: "choice fill", color: "var(--tasty-surface-raised)" }, { tok: "--tasty-border-default", use: "unselected edge" }]} />
        </Spec>
      </Section>

<Section id="rename" title="Rename popup">
        <Spec title="Rename — the smallest dialog"
          when={<>A focused single-field dialog for workspace / tab rename. Title, a one-line keyboard hint, an autofocused Input, cancel/confirm. The template for any "edit one value" modal.</>}>
          <Stage variant="solo center"><Backdrop height={260}><RenameFrame /></Backdrop></Stage>
          <Meta
            specs={[["width", "360px"], ["field", "autofocus, full-width"], ["confirm", <span className="ic">↵</span>], ["padding", <span className="tok">--tasty-space-md</span>]]}
            tokens={[{ tok: "--tasty-bg-panel", use: "frame", color: "var(--tasty-bg-panel)" }, { tok: "--tasty-accent-primary", use: "confirm", color: "var(--tasty-accent-primary)" }, { tok: "--tasty-border-focus", use: "field ring", color: "var(--tasty-border-focus)" }]} />
        </Spec>
      </Section>

<Section id="category" title="Workspace category">
        <Spec title="New / rename category — the Rename dialog, reused"
          when={<>Creating and renaming a workspace category reuses the <b>360px single-field Rename dialog</b> verbatim — title, keyboard hint, one autofocused Input, cancel/confirm. Name validation runs on confirm and shows an inline error under the field (the confirm button disables): <b>empty</b> after trim, the reserved word <b>“normal”</b>, or a <b>case-insensitive duplicate</b>.</>}>
          <Stage variant="solo center" style={{ padding: 20, background: "var(--tasty-bg-app)", gap: 18, flexWrap: "wrap" }}>
            <div style={{ display: "flex", flexDirection: "column", gap: 6 }}>
              <div style={{ fontSize: 11, color: "var(--tasty-text-muted)" }}>new — empty field</div>
              <CategoryEditFrame mode="new" />
            </div>
            <div style={{ display: "flex", flexDirection: "column", gap: 6 }}>
              <div style={{ fontSize: 11, color: "var(--tasty-text-muted)" }}>validation — duplicate name</div>
              <CategoryEditFrame mode="new" error="duplicate" />
            </div>
          </Stage>
          <Meta
            specs={[["width", "360px (Rename dialog)"], ["field", "autofocus, full-width"], ["confirm", <span className="ic">↵</span>], ["errors", "empty · reserved ‘normal’ · duplicate"], ["invalid", "inline msg + confirm disabled"]]}
            tokens={[{ tok: "--tasty-bg-panel", use: "frame", color: "var(--tasty-bg-panel)" }, { tok: "--tasty-accent-primary", use: "confirm", color: "var(--tasty-accent-primary)" }, { tok: "--tasty-accent-danger", use: "error field + text", color: "var(--tasty-accent-danger)" }]} />
          <Note>Error copy maps to the existing keys <code>workspace_category.error.empty / reserved / duplicate</code>. No new dialog chrome — this is the Rename template with a validation line.</Note>
        </Spec>
        <Spec title="Delete category — destructive confirm"
          when={<>“Delete category” never deletes immediately — a small centered confirm on the scrim gates it. The action is <b>destructive</b> (danger button + danger glyph). The body states the safe outcome: the category's <b>workspaces are not deleted</b>; they move back to <b>normal</b> (“Workspaces”). Small-modal tone, matching the Rename dialog footprint.</>}>
          <Stage variant="solo center"><Backdrop height={260}><div align="center"><CategoryDeleteFrame /></div></Backdrop></Stage>
          <Meta
            specs={[["width", "380px"], ["trigger", "Delete category (menu/popup)"], ["tone", <>destructive — <span className="tok">--tasty-accent-danger</span></>], ["body", "workspaces move to normal (not deleted)"], ["actions", "Cancel / Delete category"]]}
            tokens={[{ tok: "--tasty-accent-danger", use: "glyph + confirm button", color: "var(--tasty-accent-danger)" }, { tok: "--tasty-bg-panel", use: "frame", color: "var(--tasty-bg-panel)" }, { tok: "--tasty-text-on-accent", use: "button label", color: "var(--tasty-text-on-accent)" }]} />
          <Do><b>Do</b> reassure in the body: reversible-feeling copy (“workspaces move back to Workspaces”) lowers the stakes of an irreversible-sounding verb.</Do>
        </Spec>
      </Section>

      <Section id="transfer" title="Remote transfer">
        <Spec title="Transfer progress — auto-closing status popup"
          when={<>Shown while a file is being received from an attached remote workspace over the bulk channel (begin/chunk/commit). A <b>PopupDef</b> frame on the shared scrim, headless header: download glyph · <b>Receiving file</b> · a right-aligned mono <b>percent</b>. Body: the <b>filename</b> (mono, middle of the frame, ellipsized), the system's first <b>determinate progress bar</b> — a recessed 4px track with an accent fill, <b>no animation</b> (the fill tracks bytes; motion stays 0ms) — and a mono stats line: <b>transferred / total</b> left, rate right. It <b>auto-closes on completion</b>; the single ghost <b>Cancel</b> aborts the transfer. It does <b>not</b> close on outside click — dismissing progress by mis-click would read as an abort.</>}>
          <Stage variant="solo center"><Backdrop height={300}><div align="center"><TransferProgressFrame /></div></Backdrop></Stage>
          <Meta
            specs={[["width", <span className="tok">--tasty-transfer-popup-width</span>], ["header", "download glyph · title · mono %"], ["filename", "mono 13 · ellipsized"], ["bar", "4px track + accent fill · no animation"], ["stats", "done / total · rate (mono, muted)"], ["close", "auto on completion · Cancel aborts"], ["outside click", "does not dismiss"]]}
            tokens={[{ tok: "--tasty-progress-track-bg", use: "recessed track", color: "var(--tasty-progress-track-bg)" }, { tok: "--tasty-progress-fill-bg", use: "determinate fill", color: "var(--tasty-progress-fill-bg)" }, { tok: "--tasty-progress-height", use: "4px thickness" }, { tok: "--tasty-bg-panel", use: "frame", color: "var(--tasty-bg-panel)" }]} />
          <Note>Rate / time-remaining is <b>optional</b> — filename + progress are the contract; drop the right stat when the channel can't estimate it. Multi-file transfers reuse this frame with one filename+bar row per file, newest last — same header, one Cancel for the batch.</Note>
          <Do><b>Do</b> keep the bar still — the fill moves only when bytes land. A shimmering or eased bar would be the only animated chrome in a 0ms-motion terminal.</Do>
        </Spec>
        <Spec title="Transfer failed — rejected & mid-transfer error"
          when={<>Shown when a transfer is <b>rejected before it starts</b> (capacity exceeded) or <b>fails mid-transfer</b> (channel dropped). Danger glyph + <b>Transfer failed</b> title; the body names the <b>file</b> (mono) and quotes the <b>reason</b> verbatim in a mono well — the same command-well pattern as agent approval, so the machine-reported cause is legible and copyable. Rejected → a single <b>Dismiss</b>; mid-transfer failure → <b>Retry</b> (secondary) beside a ghost Dismiss — retry is only offered when re-sending can succeed.</>}>
          <Stage variant="solo center" style={{ padding: 20, background: "var(--tasty-bg-app)", gap: 18, flexWrap: "wrap", alignItems: "flex-start" }}>
            <div style={{ display: "flex", flexDirection: "column", gap: 6 }}>
              <div style={{ fontSize: 11, color: "var(--tasty-text-muted)" }}>rejected — capacity exceeded, no retry</div>
              <TransferErrorFrame />
            </div>
            <div style={{ display: "flex", flexDirection: "column", gap: 6 }}>
              <div style={{ fontSize: 11, color: "var(--tasty-text-muted)" }}>failed mid-transfer — retry offered</div>
              <TransferErrorFrame retry reason="connection lost — remote workspace detached at 34.6 MiB" />
            </div>
          </Stage>
          <Meta
            specs={[["width", <span className="tok">--tasty-transfer-popup-width</span>], ["header", "danger glyph · Transfer failed"], ["reason", "verbatim, mono well on bg-app"], ["rejected", "Dismiss only"], ["failed", "Dismiss (ghost) · Retry (secondary)"], ["dismiss", "button · Esc · scrim click"]]}
            tokens={[{ tok: "--tasty-accent-danger", use: "glyph + reason text", color: "var(--tasty-accent-danger)" }, { tok: "--tasty-bg-app", use: "reason well", color: "var(--tasty-bg-app)" }, { tok: "--tasty-bg-panel", use: "frame", color: "var(--tasty-bg-panel)" }]} />
          <Dont><b>Don't</b> use a danger-filled button here — danger fill is reserved for confirming a destructive act (delete). This popup only acknowledges one; Dismiss stays secondary/ghost.</Dont>
          <Note>Capacity rejection points at its own cure: the reason names the configured limit, which lives in <b>Settings › General › Remote transfer</b> (Overlays › Windows).</Note>
        </Spec>
      </Section>
    </>
  );
}

window.Gallery.mount(
  "overlays-dialogs",
  NAV,
  {
    title: "Dialogs",
    intro: "Centered modal dialogs on the shared scrim — confirm and edit surfaces. Small-to-medium frames, each a pattern to copy: show the stakes, name the actor, keep the dismiss contract. Start with the scrim recipe every overlay is built on.",
    howto: false,
  },
  <Page />
);
