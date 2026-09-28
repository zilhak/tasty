// macOS permissions — Settings › General › Permissions (L2, macOS builds only)
// and the boot "Some permissions are not granted" info modal.
// Design request: design-request/macos-permissions.md (2026-09-28 answer).
//
// Box boundaries for the implementer:
//   pane:  content column (settings 620 cap) > status table [label | status | row action]
//          > detection note > button row > request note (↔ requesting line)
//   modal: InfoModalShell (info_modal.jsx) — title bar > body scroll area > button row
(() => {
const { Button, HelpHint, Spinner, Tag } = window.TastyDesignSystem_41fd3f;
const MpIcon = window.TastyDesignSystem_41fd3f.Icon;

// four states, one glyph each — Unknown and Not observable share the ink, never the glyph
const PERM_STATES = {
  granted:      { word: "Granted",                    glyph: "check",       fg: "var(--tasty-perm-granted-fg)" },
  missing:      { word: "Not granted",                glyph: "alertCircle", fg: "var(--tasty-perm-missing-fg)" },
  unknown:      { word: "Unknown",                    glyph: "helpCircle",  fg: "var(--tasty-perm-unknown-fg)" },
  unobservable: { word: "Cannot check automatically", glyph: "eyeOff",      fg: "var(--tasty-perm-unobservable-fg)" },
};

function PermStatus({ state }) {
  const s = PERM_STATES[state];
  return (
    <span style={{ display: "inline-flex", alignItems: "center", gap: "var(--tasty-perm-status-gap)", color: s.fg,
      fontSize: "var(--tasty-font-size-body)", whiteSpace: "nowrap" }}>
      <span style={{ display: "inline-flex", flex: "none" }}><MpIcon name={s.glyph} size="var(--tasty-icon-size-sm)" /></span>
      <span style={{ color: state === "granted" || state === "missing" ? "var(--tasty-text-primary)" : "var(--tasty-text-muted)" }}>{s.word}</span>
    </span>
  );
}

const PERM_SCENARIOS = {
  none:       { fda: "missing", screen: "missing", ax: "missing" },
  all:        { fda: "granted", screen: "granted", ax: "granted" },
  fdaUnknown: { fda: "unknown", screen: "granted", ax: "granted" },
  requesting: { fda: "missing", screen: "missing", ax: "missing" },
};

const mpNote = { margin: 0, fontSize: "var(--tasty-font-size-caption)", lineHeight: "var(--tasty-line-height-ui)", color: "var(--tasty-text-muted)",
  maxWidth: "var(--tasty-measure-xl)", textWrap: "pretty" };

function PermRow({ label, hint, sub, tag, state, action }) {
  return (
    <div style={{ display: "grid", gridTemplateColumns: "minmax(0,1fr) auto", alignItems: "center", columnGap: "var(--tasty-space-lg)",
      minHeight: "var(--tasty-perm-row-height)", padding: "var(--tasty-space-xs) 0", borderBottom: "var(--tasty-border-width) solid var(--tasty-border-default)" }}>
      <div style={{ minWidth: 0, display: "flex", flexDirection: "column", gap: "var(--tasty-size-2)" }}>
        <span style={{ display: "inline-flex", alignItems: "center", gap: "var(--tasty-help-hint-gap)", flexWrap: "wrap", fontSize: "var(--tasty-font-size-body)", color: "var(--tasty-text-secondary)" }}>
          {label}{hint && <HelpHint label={hint} placement="bottom" />}{tag && <Tag>{tag}</Tag>}
        </span>
        {sub && <span style={{ fontSize: "var(--tasty-font-size-caption)", color: "var(--tasty-text-muted)" }}>{sub}</span>}
      </div>
      <div style={{ display: "flex", alignItems: "center", justifyContent: "flex-end", gap: "var(--tasty-space-md)", flexWrap: "wrap" }}>
        <PermStatus state={state} />
        {action}
      </div>
    </div>
  );
}

function MacPermissionsPane({ scenario = "none", debug = false }) {
  const s = PERM_SCENARIOS[scenario];
  const requesting = scenario === "requesting";
  return (
    <div style={{ display: "flex", flexDirection: "column", gap: "var(--tasty-space-md)" }}>
      <div style={{ display: "flex", flexDirection: "column", borderTop: "var(--tasty-border-width) solid var(--tasty-border-default)" }}>
        <PermRow label="Full Disk Access" state={s.fda}
          hint="macOS has no API to report Full Disk Access, so this state is inferred and can be wrong."
          action={<Button size="sm" variant="secondary">Open System Settings</Button>} />
        <PermRow label="Screen recording" state={s.screen} />
        <PermRow label="Folder access" sub="Downloads · Documents · Desktop · volumes" state="unobservable"
          hint="Checking a folder would itself open a permission prompt, so Tasty does not check." />
        {debug && <PermRow label="Accessibility (key injection)" tag="debug" state={s.ax} />}
      </div>
      <p style={mpNote}>These states are a snapshot from startup, from opening this page, and from this window regaining focus. Nothing in Tasty is blocked by them; they only decide whether the startup notice appears.</p>
      <div style={{ display: "flex", flexWrap: "wrap", gap: "var(--tasty-space-sm)" }}>
        <Button variant="primary" disabled={requesting}>Request all permissions</Button>
      </div>
      {requesting ? (
        <p style={{ ...mpNote, display: "flex", alignItems: "flex-start", gap: "var(--tasty-space-sm)", color: "var(--tasty-text-secondary)" }}>
          <span style={{ display: "inline-flex", flex: "none", marginTop: "var(--tasty-size-1)" }}><Spinner size="var(--tasty-icon-size-sm)" /></span>
          <span>Requesting permissions. Answer each prompt as it appears; the next one shows after you answer.</span>
        </p>
      ) : (
        <p style={mpNote}>Asks for folder access, then screen recording, one prompt at a time. Items you already allowed or denied are not asked again, and a denied item can only be restored in System Settings. Full Disk Access cannot be requested by an app: use Open System Settings in its row and add Tasty yourself.</p>
      )}
    </div>
  );
}

// ── boot info modal ──────────────────────────────────────────────────────
const mpPath = { color: "var(--tasty-text-primary)", fontWeight: "var(--tasty-font-weight-medium)" };
const mpLead = { color: "var(--tasty-text-primary)", fontWeight: "var(--tasty-font-weight-semibold)" };
const mpCmd = { fontFamily: "var(--tasty-font-mono)", fontSize: "var(--tasty-font-size-caption)", color: "var(--tasty-text-primary)",
  background: "var(--tasty-surface-raised)", borderRadius: "var(--tasty-radius-sm)", padding: "0 var(--tasty-space-xs)", overflowWrap: "anywhere" };

function PermNoticeBody() {
  return (
    <>
      <p>Commands you run in Tasty read and write files on your behalf, and macOS attributes that access to Tasty. Without permission, a prompt may interrupt your work or a feature may not work.</p>
      <p>Some permissions currently look like they are not granted. <span style={mpPath}>Settings &gt; General &gt; Permissions</span> shows which ones and what state they are in.</p>
      <p><span style={mpLead}>Full Disk Access:</span> macOS has no way for an app to ask for this, so add Tasty yourself in <span style={mpPath}>System Settings &gt; Privacy &amp; Security &gt; Full Disk Access</span>. Granting it removes the file-access prompts (other apps' data, Downloads, Documents, Desktop and mounted volumes). It does not cover controlling other apps (Automation / Apple Events, e.g. osascript), screen recording, or accessibility: those are separate permissions and will still prompt.</p>
      <p><span style={mpLead}>Screen recording:</span> this is used by the screenshot feature. Turn it on in <span style={mpPath}>System Settings &gt; Privacy &amp; Security &gt; Screen &amp; System Audio Recording</span>. Once you deny it, the app cannot ask again and only System Settings can turn it back on.</p>
      <p>If you build Tasty yourself, sign it with the "Tasty Dev" certificate first (<span style={mpCmd}>./scripts/macos-codesign-identity.sh --create</span>). Ad-hoc signed builds look like a different app to macOS after every rebuild, so the permission is discarded each time.</p>
      <p>Once you grant them, this notice stops appearing. There is no setting to turn it off, because recording that you dismissed it would leave no way to tell you when a permission is reset later.</p>
    </>
  );
}

// One message in the shared InfoModalShell. The ONLY queue message with authored
// emphasis (paths · lead-ins · command chip) and a second button.
function PermissionNoticeModal({ scroll = "top" }) {
  const Shell = window.TastyKit.InfoModalShell;
  return (
    <Shell id="perm-notice" title="Some permissions are not granted" scroll={scroll}
      actions={<><Button variant="secondary">Open permission settings</Button><Button variant="primary">OK</Button></>}>
      {PermNoticeBody().props.children}
    </Shell>
  );
}

window.TastyKit = Object.assign(window.TastyKit || {}, { MacPermissionsPane, PermissionNoticeModal, PermStatus, PERM_STATES });

})();