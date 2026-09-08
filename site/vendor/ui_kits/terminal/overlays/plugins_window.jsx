// Tasty UI kit — Plugins window (lifecycle: install / enable / uninstall).
// Mirrors zilhak/tasty → src/view/plugins.rs + src/view/plugins/ui/{list,add}.rs
// Standalone preview: plugins_window.html
const { Input, Switch, Button, IconButton, Tag, Kbd, Badge } = window.TastyDesignSystem_41fd3f;
const { ic, Icon, Scrim } = window.TastyKit;

// ── Plugins — installed list + add/install new plugins ──────────────────
// Opened from the sidebar "Plugins" entry. The Settings › Plugins tab is for
// per-plugin *configuration*; this window is for lifecycle: enable / disable
// installed plugins, and add new ones from a LOCAL FOLDER via the Add plugin tab.
// There is no online catalog/marketplace (deferred — plugin-marketplace.md §8 /
// ADR #0010): you install from a path or a cloned repo, Tasty verifies the
// folder's tasty-plugin.toml + signature, and copies it into ~/.tasty/plugins.
const PLUGIN_LIST = [
  { id: "git-helper", name: "git-helper", author: "tasty-labs", version: "1.4.2", cat: "Source control",
    installed: true, status: "running", agent: false, installs: "126k", rating: "4.9",
    desc: "Inline git status, blame, and one-key staging inside any terminal surface. Adds a gutter ribbon and a compact branch switcher to the tab strip.",
    perms: ["fs:read", "clipboard", "ipc:git-helper.*"], cmd: "git-helper: open panel", key: "Ctrl+Alt+G" },
  { id: "ai-review", name: "ai-review", author: "tasty-labs", version: "0.9.0", cat: "AI",
    installed: true, status: "agent", agent: true, installs: "84k", rating: "4.7",
    desc: "Agent-driven review of staged diffs. Streams suggested patches into a side surface and lets you apply them hunk by hunk.",
    perms: ["fs:read", "fs:write", "net", "ipc:ai-review.*"], cmd: "ai-review: review staged", key: "Ctrl+Alt+R" },
  { id: "docker", name: "docker", author: "community", version: "2.1.0", cat: "DevOps",
    installed: true, status: "idle", agent: false, installs: "203k", rating: "4.6",
    desc: "A surface for container logs, exec sessions, and compose control. Attaches to the local Docker socket and mirrors `docker ps` live.",
    perms: ["ipc:docker.*", "net"], cmd: "docker: containers", key: "Ctrl+Alt+D" },
  { id: "k8s-lens", name: "k8s-lens", author: "community", version: "0.6.3", cat: "DevOps",
    installed: true, status: "error", agent: false, installs: "57k", rating: "4.1",
    desc: "Kubernetes context switcher and pod log streamer. Currently failing to reach the configured cluster — check kubeconfig.",
    perms: ["net", "fs:read", "ipc:k8s-lens.*"], cmd: "k8s-lens: switch context", key: "Ctrl+Alt+K" },
  { id: "vim-mode", name: "vim-mode", author: "ophen", version: "3.2.1", cat: "Editing",
    installed: false, status: "idle", agent: false, installs: "418k", rating: "4.8",
    desc: "Modal editing keybindings for every text surface — normal/insert/visual modes, registers, and the operators you actually use.",
    perms: ["clipboard", "ipc:vim-mode.*"], cmd: "vim-mode: toggle", key: "Ctrl+Alt+V" },
  { id: "theme-studio", name: "theme-studio", author: "tasty-labs", version: "1.0.4", cat: "Appearance",
    installed: false, status: "idle", agent: false, installs: "92k", rating: "4.9",
    desc: "Live-edit surface colors and export a Catppuccin-compatible theme. Hot-reloads every open pane as you drag.",
    perms: ["fs:read", "fs:write"], cmd: "theme-studio: open", key: "Ctrl+Alt+T" },
  { id: "s3-browser", name: "s3-browser", author: "community", version: "0.4.0", cat: "Cloud",
    installed: false, status: "idle", agent: false, installs: "31k", rating: "4.3",
    desc: "Browse, preview, and stream from S3 buckets as a first-class surface. Drag objects between buckets and your local tree.",
    perms: ["net", "fs:read", "fs:write"], cmd: "s3-browser: open bucket", key: "" },
  { id: "pomodoro", name: "pomodoro", author: "kettle", version: "2.0.0", cat: "Productivity",
    installed: false, status: "idle", agent: false, installs: "19k", rating: "4.5",
    desc: "A focus timer that lives in the status bar, logs sessions per workspace, and dims inactive panes while you're in a sprint.",
    perms: ["ipc:pomodoro.*"], cmd: "pomodoro: start", key: "" },
];

const CAT_COLOR = {
  "Source control": "var(--tasty-accent-primary)",
  "AI": "var(--tasty-accent-agent)",
  "DevOps": "var(--tasty-accent-success)",
  "Editing": "var(--tasty-accent-warning)",
  "Appearance": "var(--tasty-accent-primary)",
  "Cloud": "var(--tasty-accent-agent)",
  "Productivity": "var(--tasty-accent-success)",
};

// ── Needs-attention cases ───────────────────────────────────────────────
// Plugins that are installed/bundled but were REJECTED at registration
// (signature/trust) or are ENABLED-but-failing (health error). Mirrors
// tasty-host-plugin/src/bundle_sig.rs TrustDecision + builtin.rs Skipped
// reasons. NOT a marketplace — only local/bundled plugins (ADR #0010).
const ATTN = {
  "unknown-key":        { sev: "danger",  label: "Signature not trusted",
    blurb: "Signed by a key that isn't in your trust store — registration rejected." },
  "signature-invalid":  { sev: "danger",  label: "Signature invalid",
    blurb: "Signature missing or failed verification — registration rejected." },
  "permissions-changed":{ sev: "warning", label: "Permissions changed",
    blurb: "Manifest permissions changed since you trusted it — re-approval required." },
  "health-error":       { sev: "warning", label: "Runtime error",
    blurb: "Enabled, but failing at runtime." },
};
const SEV_COLOR = { danger: "var(--tasty-accent-danger)", warning: "var(--tasty-accent-warning)" };

// Rejected / unregistered plugins — these never made it into the installed list.
const ATTENTION_LIST = [
  { id: "fleet-sync", name: "fleet-sync", author: "tasty-labs", version: "1.2.0", cat: "DevOps",
    reason: "unknown-key", builtin: true,
    desc: "Bundled cluster fleet sync. Its publisher key was rotated and the signature no longer matches a trusted key.",
    detail: { fingerprint: "a13c 4e7f 2b08 9d51  ·  ed25519",
      note: "The built-in publisher key rotated this release; the bundled signature was made with a key not yet in your trust store. Update Tasty or import the new key to restore it." } },
  { id: "secrets-vault", name: "secrets-vault", author: "community", version: "0.8.4", cat: "Cloud",
    reason: "permissions-changed",
    desc: "Reads and injects secrets into surfaces. You trusted v0.7 — v0.8.4 requests a different permission set.",
    detail: { added: ["fs:write", "net"], removed: ["clipboard"] } },
  { id: "remote-shell", name: "remote-shell", author: "community", version: "1.0.0", cat: "DevOps",
    reason: "signature-invalid",
    desc: "Opens remote SSH surfaces. The bundle's signature file is missing or corrupt.",
    detail: { note: "tasty-plugin.sig is absent or does not match the manifest hash. Re-download the plugin from its source." } },
];

function PluginAvatar({ plugin, size = 36 }) {
  const c = CAT_COLOR[plugin.cat] || "var(--tasty-accent-primary)";
  return (
    <span style={{ width: size, height: size, flex: "none", borderRadius: "var(--tasty-radius)",
      display: "inline-flex", alignItems: "center", justifyContent: "center",
      background: `color-mix(in srgb, ${c} 18%, var(--tasty-surface-raised))`,
      border: `1px solid color-mix(in srgb, ${c} 38%, transparent)`,
      fontFamily: "var(--tasty-font-mono)", fontWeight: 700, color: c,
      fontSize: Math.round(size * 0.42), lineHeight: 1 }}>
      {plugin.name.charAt(0).toUpperCase()}
    </span>
  );
}

// ── Add plugin — local folder install (no catalog) ─────────────────────
// Mirrors src/view/plugins/ui/add.rs: pick a local folder (or clone), read its
// tasty-plugin.toml manifest, judge trust by signature, then Install /
// TrustAndInstall. A sample manifest stands in for a real folder read.
const SAMPLE_MANIFEST = {
  id: "com.aurelia.logwatch", name: "logwatch", version: "0.3.1", author: "aurelia",
  trusted: false, fingerprint: "9f2c 4ad1 b770 e3a6  ·  ed25519",
  desc: "Tails and highlights structured log files as a dedicated surface — severity filters, a jump-to-error gutter, and live follow on the active workspace.",
  perms: ["fs:read", "fs:watch", "ipc:logwatch.*"],
  surfaces: ["logwatch.viewer"],
  path: "~/dev/tasty-logwatch",
};

function AddPluginForm({ onAdded, onCancel }) {
  const [path, setPath] = React.useState("");
  const [manifest, setManifest] = React.useState(null);
  const setPathReset = (v) => { setPath(v); setManifest(null); }; // re-verify after any edit
  const verify = () => {
    const p = path.trim() || SAMPLE_MANIFEST.path;
    setPath(p);
    setManifest({ ...SAMPLE_MANIFEST, path: p });
  };
  const pick = () => setPathReset("~/dev/tasty-logwatch"); // stands in for the native rfd folder picker

  const Mono = ({ children }) => (
    <div style={{ fontFamily: "var(--tasty-font-mono)", fontSize: 10, textTransform: "uppercase",
      letterSpacing: "var(--tasty-letter-spacing-caps)", color: "var(--tasty-text-muted)" }}>{children}</div>
  );
  const code = { fontFamily: "var(--tasty-font-mono)", color: "var(--tasty-text-secondary)" };

  return (
    <div style={{ flex: 1, display: "flex", flexDirection: "column", minWidth: 0 }}>
      <div className="tasty-scroll" style={{ flex: 1, overflow: "auto", padding: "var(--tasty-size-22) var(--tasty-space-xl)",
        display: "flex", flexDirection: "column", gap: "var(--tasty-space-lg)" }}>
        {/* path picker */}
        <div style={{ maxWidth: "var(--tasty-measure-xl)", display: "flex", flexDirection: "column", gap: "var(--tasty-space-sm)" }}>
          <Mono>Plugin folder</Mono>
          <div style={{ display: "flex", gap: 8 }}>
            <Input mono icon={ic.folder} placeholder="~/dev/my-plugin"
              value={path} onChange={(e) => setPathReset(e.target.value)} style={{ flex: 1 }} />
            <Button variant="secondary" leadingIcon={ic.folder} onClick={pick}>Find folder…</Button>
            <Button variant="primary" disabled={!path.trim() && !manifest} onClick={verify}>Verify</Button>
          </div>
          <p style={{ margin: 0, fontSize: 12, lineHeight: "var(--tasty-line-height-ui)", color: "var(--tasty-text-muted)" }}>
            Point Tasty at a local folder containing a <code style={code}>tasty-plugin.toml</code>. Verifying reads its
            manifest and checks the signature; adding copies it into <code style={code}>~/.tasty/plugins</code>.
          </p>
        </div>

        {!manifest ? (
          <div style={{ maxWidth: "var(--tasty-measure-xl)", display: "flex", alignItems: "center", gap: "var(--tasty-space-sm)",
            padding: "var(--tasty-size-14) var(--tasty-space-lg)", borderRadius: "var(--tasty-radius)", border: "var(--tasty-border-width) dashed var(--tasty-border-default)",
            color: "var(--tasty-text-muted)", fontSize: "var(--tasty-font-size-term-sm)" }}>
            <span style={{ display: "inline-flex", color: "var(--tasty-text-muted)" }}>{ic.folder}</span>
            <span>Choose a folder and press <b style={{ color: "var(--tasty-text-secondary)" }}>Verify</b> to read its manifest.</span>
          </div>
        ) : (
          <div style={{ maxWidth: "var(--tasty-measure-xl)", display: "flex", flexDirection: "column", gap: 14 }}>
            {/* manifest preview */}
            <div style={{ border: "var(--tasty-border-width) solid var(--tasty-border-default)", borderRadius: "var(--tasty-radius)",
              background: "var(--tasty-surface-raised)", padding: 16, display: "flex", flexDirection: "column", gap: "var(--tasty-space-md)" }}>
              <div style={{ display: "flex", gap: 12, alignItems: "flex-start" }}>
                <span style={{ width: 42, height: 42, flex: "none", borderRadius: "var(--tasty-radius)",
                  display: "inline-flex", alignItems: "center", justifyContent: "center",
                  background: "color-mix(in srgb, var(--tasty-accent-primary) 16%, var(--tasty-surface-active))",
                  border: "var(--tasty-border-width) solid color-mix(in srgb, var(--tasty-accent-primary) 34%, transparent)",
                  fontFamily: "var(--tasty-font-mono)", fontWeight: "var(--tasty-font-weight-bold)", color: "var(--tasty-accent-primary)", fontSize: "var(--tasty-font-size-max)" }}>
                  {manifest.name.charAt(0).toUpperCase()}
                </span>
                <div style={{ flex: 1, minWidth: 0 }}>
                  <div style={{ display: "flex", alignItems: "center", gap: 8, flexWrap: "wrap" }}>
                    <span style={{ fontSize: "var(--tasty-font-size-max)", fontWeight: "var(--tasty-font-weight-semibold)", color: "var(--tasty-text-primary)" }}>{manifest.name}</span>
                    <Tag>{"v" + manifest.version}</Tag>
                  </div>
                  <div style={{ marginTop: 3, fontFamily: "var(--tasty-font-mono)", fontSize: 11,
                    color: "var(--tasty-text-muted)" }}>{manifest.id} · {manifest.author}</div>
                </div>
              </div>
              <p style={{ margin: 0, fontSize: 13, lineHeight: "var(--tasty-line-height-ui)", color: "var(--tasty-text-secondary)" }}>{manifest.desc}</p>
              <div style={{ display: "flex", flexDirection: "column", gap: "var(--tasty-space-sm)" }}>
                <Mono>Permissions</Mono>
                <div style={{ display: "flex", gap: "var(--tasty-space-sm)", flexWrap: "wrap" }}>{manifest.perms.map((p) => <Tag key={p}>{p}</Tag>)}</div>
              </div>
              <div style={{ display: "flex", flexDirection: "column", gap: "var(--tasty-space-sm)" }}>
                <Mono>Surface kinds</Mono>
                <div style={{ display: "flex", gap: "var(--tasty-space-sm)", flexWrap: "wrap" }}>{manifest.surfaces.map((s) => <Tag key={s}>{s}</Tag>)}</div>
              </div>
              <div style={{ display: "flex", flexDirection: "column", gap: "var(--tasty-space-xs)" }}>
                <Mono>Source</Mono>
                <span style={{ fontFamily: "var(--tasty-font-mono)", fontSize: 12, color: "var(--tasty-text-secondary)",
                  overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>{manifest.path}</span>
              </div>
            </div>

            {/* trust judgment */}
            {manifest.trusted ? (
              <div style={{ display: "flex", alignItems: "center", gap: "var(--tasty-space-sm)", padding: "var(--tasty-space-md) var(--tasty-space-md)",
                borderRadius: "var(--tasty-radius)", fontSize: "var(--tasty-font-size-term-sm)", color: "var(--tasty-accent-success)",
                background: "color-mix(in srgb, var(--tasty-accent-success) 12%, transparent)",
                border: "var(--tasty-border-width) solid color-mix(in srgb, var(--tasty-accent-success) 32%, transparent)" }}>
                <Icon name="shieldCheck" size={16} />
                <span>Signed by a <b>trusted publisher</b> — its key is in your trust store.</span>
              </div>
            ) : (
              <div style={{ display: "flex", flexDirection: "column", gap: 8, padding: "var(--tasty-space-md) var(--tasty-size-14)",
                borderRadius: "var(--tasty-radius)", color: "var(--tasty-text-secondary)",
                background: "color-mix(in srgb, var(--tasty-accent-warning) 11%, transparent)",
                border: "var(--tasty-border-width) solid color-mix(in srgb, var(--tasty-accent-warning) 36%, transparent)" }}>
                <div style={{ display: "flex", alignItems: "center", gap: "var(--tasty-space-sm)", color: "var(--tasty-accent-warning)",
                  fontSize: 13, fontWeight: 600 }}>
                  <Icon name="alertTriangle" size={16} />
                  <span>Unverified publisher</span>
                </div>
                <p style={{ margin: 0, fontSize: "var(--tasty-font-size-term-sm)", lineHeight: "var(--tasty-line-height-ui)" }}>
                  This plugin isn't signed by a key in your trust store. It runs with the permissions above
                  on every launch — review them, and only add plugins from sources you trust.
                </p>
                <div style={{ display: "flex", alignItems: "center", gap: 8, fontFamily: "var(--tasty-font-mono)",
                  fontSize: 11, color: "var(--tasty-text-muted)" }}>
                  <span style={{ color: "var(--tasty-text-secondary)" }}>fingerprint</span>
                  <span>{manifest.fingerprint}</span>
                </div>
              </div>
            )}
          </div>
        )}
      </div>

      {/* action bar */}
      <div style={{ display: "flex", alignItems: "center", gap: "var(--tasty-space-sm)", padding: "var(--tasty-space-md) var(--tasty-size-14)",
        borderTop: "var(--tasty-border-width) solid var(--tasty-separator)", flex: "none" }}>
        {manifest && (
          <span style={{ fontSize: 12, color: "var(--tasty-text-muted)" }}>
            Grants {manifest.perms.length} permission{manifest.perms.length === 1 ? "" : "s"}
          </span>
        )}
        <div style={{ flex: 1 }} />
        <Button variant="ghost" onClick={onCancel}>Cancel</Button>
        {manifest && (manifest.trusted
          ? <Button variant="primary" onClick={() => onAdded(manifest)}>Add plugin</Button>
          : <Button variant="agent" onClick={() => onAdded(manifest)}>Trust &amp; add</Button>)}
      </div>
    </div>
  );
}

// ── Attention tab — rejected / failing plugins with reason + action ─────
function AttentionPanel({ items, onFlash, onConfigure }) {
  const [selId, setSelId] = React.useState(items[0] ? items[0].id : null);
  const sel = items.find((p) => p.id === selId) || items[0] || null;

  const Mono = ({ children }) => (
    <div style={{ fontFamily: "var(--tasty-font-mono)", fontSize: 10, textTransform: "uppercase",
      letterSpacing: "var(--tasty-letter-spacing-caps)", color: "var(--tasty-text-muted)" }}>{children}</div>
  );

  if (items.length === 0)
    return (
      <div style={{ flex: 1, display: "flex", flexDirection: "column", alignItems: "center",
        justifyContent: "center", gap: 12, color: "var(--tasty-text-muted)", padding: 24 }}>
        <span style={{ display: "inline-flex", width: 46, height: 46, borderRadius: "var(--tasty-radius)",
          alignItems: "center", justifyContent: "center", color: "var(--tasty-accent-success)",
          background: "color-mix(in srgb, var(--tasty-accent-success) 12%, transparent)",
          border: "var(--tasty-border-width) solid color-mix(in srgb, var(--tasty-accent-success) 30%, transparent)" }}>
          <Icon name="shieldCheck" size={20} />
        </span>
        <div style={{ fontSize: "var(--tasty-font-size-body)", color: "var(--tasty-text-secondary)" }}>No plugins need attention</div>
        <div style={{ fontSize: 12, maxWidth: "var(--tasty-measure-sm)", textAlign: "center", lineHeight: "var(--tasty-line-height-ui)" }}>
          Rejected or failing plugins show up here with the reason and what to do next.
        </div>
      </div>
    );

  const meta = sel && ATTN[sel.reason];
  const c = sel && SEV_COLOR[meta.sev];
  return (
    <React.Fragment>
      {/* list */}
      <div className="tasty-scroll" style={{ width: "var(--tasty-plugins-list-width)", flex: "none", overflow: "auto", padding: 8,
        background: "var(--tasty-bg-sidebar)", borderRight: "var(--tasty-border-width) solid var(--tasty-separator)",
        display: "flex", flexDirection: "column", gap: 3 }}>
        {items.map((p) => {
          const on = p.id === sel.id;
          const pc = SEV_COLOR[ATTN[p.reason].sev];
          return (
            <div key={p.id} onClick={() => setSelId(p.id)} style={{ display: "flex", alignItems: "center",
              gap: "var(--tasty-space-sm)", padding: "var(--tasty-space-sm) var(--tasty-space-sm)", borderRadius: "var(--tasty-radius)", cursor: "pointer",
              background: on ? "var(--tasty-surface-active)" : "transparent",
              boxShadow: on ? `inset var(--tasty-size-2) 0 0 ${pc}` : "none" }}>
              <PluginAvatar plugin={p} size={32} />
              <div style={{ flex: 1, minWidth: 0 }}>
                <div style={{ fontSize: 13, color: on ? "var(--tasty-text-primary)" : "var(--tasty-text-secondary)",
                  overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>{p.name}</div>
                <div style={{ fontFamily: "var(--tasty-font-mono)", fontSize: "var(--tasty-font-size-micro)", color: pc,
                  overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>{ATTN[p.reason].label}</div>
              </div>
              <span style={{ flex: "none", width: "var(--tasty-status-dot-size)", height: "var(--tasty-status-dot-size)", borderRadius: "50%", background: pc }} />
            </div>
          );
        })}
      </div>

      {/* detail */}
      <div style={{ flex: 1, display: "flex", flexDirection: "column", minWidth: 0 }}>
        <div className="tasty-scroll" style={{ flex: 1, overflow: "auto", padding: "var(--tasty-space-lg)",
          display: "flex", flexDirection: "column", gap: 16 }}>
          {/* identity */}
          <div style={{ display: "flex", gap: "var(--tasty-space-md)", alignItems: "flex-start" }}>
            <PluginAvatar plugin={sel} size={46} />
            <div style={{ flex: 1, minWidth: 0 }}>
              <div style={{ display: "flex", alignItems: "center", gap: 8, flexWrap: "wrap" }}>
                <span style={{ fontSize: "var(--tasty-font-size-max)", fontWeight: "var(--tasty-font-weight-semibold)", color: "var(--tasty-text-primary)" }}>{sel.name}</span>
                <Tag>{"v" + sel.version}</Tag>
                {sel.builtin && <Tag>built-in</Tag>}
              </div>
              <div style={{ marginTop: 4, fontFamily: "var(--tasty-font-mono)", fontSize: 11, color: "var(--tasty-text-muted)",
                display: "flex", alignItems: "center", gap: 8, flexWrap: "wrap" }}>
                <span>{sel.author}</span><span>·</span><span>{sel.cat}</span>
              </div>
            </div>
          </div>

          {sel.desc && <p style={{ margin: 0, fontSize: 13, lineHeight: 1.6,
            color: "var(--tasty-text-secondary)", maxWidth: "var(--tasty-measure-lg)" }}>{sel.desc}</p>}

          {/* reason banner */}
          <div style={{ display: "flex", flexDirection: "column", gap: 8, padding: "var(--tasty-space-md) var(--tasty-size-14)",
            borderRadius: "var(--tasty-radius)", color: "var(--tasty-text-secondary)",
            background: `color-mix(in srgb, ${c} 11%, transparent)`,
            border: `1px solid color-mix(in srgb, ${c} 36%, transparent)` }}>
            <div style={{ display: "flex", alignItems: "center", gap: "var(--tasty-space-sm)", color: c, fontSize: 13, fontWeight: 600 }}>
              <Icon name={meta.sev === "danger" ? "alertCircle" : "alertTriangle"} size={16} />
              <span>{meta.label}</span>
            </div>
            <p style={{ margin: 0, fontSize: "var(--tasty-font-size-term-sm)", lineHeight: "var(--tasty-line-height-ui)" }}>{meta.blurb}</p>
          </div>

          {/* reason-specific detail */}
          {sel.reason === "permissions-changed" && (
            <div style={{ display: "flex", flexDirection: "column", gap: "var(--tasty-space-sm)" }}>
              <Mono>Permission changes</Mono>
              <div style={{ display: "flex", flexDirection: "column", gap: "var(--tasty-space-sm)" }}>
                {(sel.detail.added || []).map((p) => (
                  <div key={p} style={{ display: "flex", alignItems: "center", gap: 8,
                    fontFamily: "var(--tasty-font-mono)", fontSize: 12 }}>
                    <span style={{ color: "var(--tasty-accent-success)", fontWeight: 700, width: 10 }}>+</span>
                    <span style={{ color: "var(--tasty-text-secondary)" }}>{p}</span>
                    <span style={{ fontSize: 11, color: "var(--tasty-text-muted)" }}>newly requested</span>
                  </div>
                ))}
                {(sel.detail.removed || []).map((p) => (
                  <div key={p} style={{ display: "flex", alignItems: "center", gap: 8,
                    fontFamily: "var(--tasty-font-mono)", fontSize: 12 }}>
                    <span style={{ color: "var(--tasty-text-muted)", fontWeight: 700, width: 10 }}>−</span>
                    <span style={{ color: "var(--tasty-text-muted)", textDecoration: "line-through" }}>{p}</span>
                    <span style={{ fontSize: 11, color: "var(--tasty-text-muted)" }}>no longer used</span>
                  </div>
                ))}
              </div>
            </div>
          )}

          {(sel.reason === "unknown-key" || sel.reason === "signature-invalid") && (
            <div style={{ display: "flex", flexDirection: "column", gap: "var(--tasty-space-sm)" }}>
              <Mono>Signature</Mono>
              {sel.detail.fingerprint && (
                <div style={{ display: "flex", alignItems: "center", gap: 8, fontFamily: "var(--tasty-font-mono)",
                  fontSize: "var(--tasty-font-size-caption)", color: "var(--tasty-text-muted)" }}>
                  <span style={{ color: "var(--tasty-text-secondary)" }}>fingerprint</span>
                  <span>{sel.detail.fingerprint}</span>
                </div>
              )}
              {sel.detail.note && <p style={{ margin: 0, fontSize: "var(--tasty-font-size-term-sm)", lineHeight: "var(--tasty-line-height-ui)",
                color: "var(--tasty-text-muted)", maxWidth: "var(--tasty-measure-lg)" }}>{sel.detail.note}</p>}
            </div>
          )}

          {sel.reason === "health-error" && sel.detail.error && (
            <div style={{ display: "flex", flexDirection: "column", gap: "var(--tasty-space-sm)" }}>
              <Mono>Error</Mono>
              <pre style={{ margin: 0, background: "var(--tasty-bg-app)", border: "var(--tasty-border-width) solid var(--tasty-separator)",
                borderRadius: "var(--tasty-radius)", padding: "var(--tasty-space-md) var(--tasty-space-md)", fontFamily: "var(--tasty-font-mono)",
                fontSize: 12, lineHeight: "var(--tasty-line-height-ui)", color: "var(--tasty-accent-danger)", whiteSpace: "pre-wrap" }}>{sel.detail.error}</pre>
            </div>
          )}
        </div>

        {/* action bar */}
        <div style={{ display: "flex", alignItems: "center", gap: "var(--tasty-space-sm)", padding: "var(--tasty-space-md) var(--tasty-size-14)",
          borderTop: "var(--tasty-border-width) solid var(--tasty-separator)", flex: "none" }}>
          <span style={{ fontSize: 12, color: c, display: "inline-flex", alignItems: "center", gap: "var(--tasty-space-sm)" }}>
            <span style={{ width: "var(--tasty-status-dot-size)", height: "var(--tasty-status-dot-size)", borderRadius: "50%", background: c }} />
            {meta.sev === "danger" ? "Not registered" : "Needs review"}
          </span>
          <div style={{ flex: 1 }} />
          {sel.reason === "permissions-changed" && (
            <Button variant="primary" onClick={() => onFlash && onFlash("Re-approve " + sel.name)}>Re-approve</Button>
          )}
          {sel.reason === "health-error" && (
            <Button variant="ghost" leadingIcon={ic.settings}
              onClick={() => onConfigure && onConfigure()}>Configure</Button>
          )}
          {(sel.reason === "unknown-key" || sel.reason === "signature-invalid") && (
            <Button variant="secondary" onClick={() => onFlash && onFlash("Signature details — " + sel.name)}>Details</Button>
          )}
        </div>
      </div>
    </React.Fragment>
  );
}

function PluginsWindow({ onClose, onFlash, onConfigure }) {
  const [tab, setTab] = React.useState("installed"); // installed | attention | add
  const [q, setQ] = React.useState("");
  const [installed, setInstalled] = React.useState(() =>
    Object.fromEntries(PLUGIN_LIST.map((p) => [p.id, p.installed])));
  const [enabled, setEnabled] = React.useState(() =>
    Object.fromEntries(PLUGIN_LIST.map((p) => [p.id, p.installed && p.status !== "idle"])));
  const [selId, setSelId] = React.useState("git-helper");

  const matches = (p) => {
    const n = q.trim().toLowerCase();
    return !n || `${p.name} ${p.author} ${p.cat} ${p.desc}`.toLowerCase().includes(n);
  };
  // The list is INSTALLED-ONLY. There is no catalog/marketplace browse — adding a
  // plugin means pointing Tasty at a LOCAL folder (the “Add plugin” tab). Online
  // catalog browse + install-by-id are deferred (plugin-marketplace.md §8 / ADR #0010).
  const list = PLUGIN_LIST.filter((p) => installed[p.id] && matches(p));
  const sel = PLUGIN_LIST.find((p) => p.id === selId);
  const selVisible = sel && installed[sel.id] && matches(sel);

  const uninstall = (p) => {
    setInstalled((m) => ({ ...m, [p.id]: false }));
    setEnabled((m) => ({ ...m, [p.id]: false }));
    onFlash && onFlash(`Removed ${p.name}`);
  };
  const toggleEnabled = (p) => setEnabled((m) => ({ ...m, [p.id]: !m[p.id] }));

  const installedCount = PLUGIN_LIST.filter((p) => installed[p.id]).length;

  // Attention = rejected/unregistered (signature/trust) + enabled-but-failing
  // (health error, pulled live from the installed list).
  const healthErrors = PLUGIN_LIST
    .filter((p) => installed[p.id] && enabled[p.id] && p.status === "error")
    .map((p) => ({ id: p.id, name: p.name, author: p.author, version: p.version, cat: p.cat,
      reason: "health-error", desc: p.desc,
      detail: { error: "Failed to reach the configured cluster at https://10.0.4.11:6443\nkubeconfig context \"prod\" — connection refused" } }));
  const attention = [...ATTENTION_LIST, ...healthErrors];
  const attentionCount = attention.length;

  const Seg = ({ value, label, count, danger }) => {
    const on = tab === value;
    return (
      <button onClick={() => setTab(value)} style={{ appearance: "none", cursor: "pointer", border: 0,
        height: 26, padding: "0 var(--tasty-space-md)", borderRadius: "var(--tasty-radius-sm)", display: "inline-flex",
        alignItems: "center", gap: "var(--tasty-space-sm)", fontFamily: "var(--tasty-font-ui)", fontSize: "var(--tasty-font-size-term-sm)",
        fontWeight: on ? 600 : 400, color: on ? "var(--tasty-text-primary)" : "var(--tasty-text-muted)",
        background: on ? "var(--tasty-surface-raised)" : "transparent",
        boxShadow: on ? "inset 0 0 0 var(--tasty-border-width) var(--tasty-border-default)" : "none" }}>
        {label}
        {count != null && !danger && <span style={{ fontFamily: "var(--tasty-font-mono)", fontSize: "var(--tasty-font-size-micro)",
          color: on ? "var(--tasty-text-secondary)" : "var(--tasty-text-muted)" }}>{count}</span>}
        {danger && count > 0 && <Badge variant="danger">{count}</Badge>}
      </button>
    );
  };

  const Mono = ({ children }) => (
    <div style={{ fontFamily: "var(--tasty-font-mono)", fontSize: 10, textTransform: "uppercase",
      letterSpacing: "var(--tasty-letter-spacing-caps)", color: "var(--tasty-text-muted)" }}>{children}</div>
  );

  function detail() {
    if (!sel || !selVisible)
      return (
        <div style={{ flex: 1, display: "flex", alignItems: "center", justifyContent: "center",
          fontSize: 13, color: "var(--tasty-text-muted)" }}>
          {list.length ? "Select a plugin" : "No plugins to show"}
        </div>
      );
    const isInstalled = installed[sel.id];
    const isOn = enabled[sel.id];
    return (
      <>
        <div className="tasty-scroll" style={{ flex: 1, overflow: "auto", padding: "var(--tasty-space-lg)",
          display: "flex", flexDirection: "column", gap: 16 }}>
          {/* identity */}
          <div style={{ display: "flex", gap: "var(--tasty-space-md)", alignItems: "flex-start" }}>
            <PluginAvatar plugin={sel} size={46} />
            <div style={{ flex: 1, minWidth: 0 }}>
              <div style={{ display: "flex", alignItems: "center", gap: 8, flexWrap: "wrap" }}>
                <span style={{ fontSize: "var(--tasty-font-size-max)", fontWeight: "var(--tasty-font-weight-semibold)", color: "var(--tasty-text-primary)" }}>{sel.name}</span>
                <Tag>{"v" + sel.version}</Tag>
                {sel.agent && <Tag variant="agent">agent</Tag>}
              </div>
              <div style={{ marginTop: 4, fontFamily: "var(--tasty-font-mono)", fontSize: 11, color: "var(--tasty-text-muted)",
                display: "flex", alignItems: "center", gap: 8, flexWrap: "wrap" }}>
                <span>{sel.author}</span><span>·</span><span>{sel.cat}</span>
              </div>
            </div>

          </div>

          <p style={{ margin: 0, fontSize: 13, lineHeight: 1.6, color: "var(--tasty-text-secondary)", maxWidth: "var(--tasty-measure-lg)" }}>{sel.desc}</p>

          {sel.status === "error" && isInstalled && isOn && (
            <div style={{ display: "flex", alignItems: "center", gap: 8, padding: "var(--tasty-space-sm) var(--tasty-space-md)",
              borderRadius: "var(--tasty-radius)", fontSize: "var(--tasty-font-size-term-sm)", color: "var(--tasty-accent-danger)",
              background: "color-mix(in srgb, var(--tasty-accent-danger) 12%, transparent)",
              border: "var(--tasty-border-width) solid color-mix(in srgb, var(--tasty-accent-danger) 35%, transparent)" }}>
              <Icon name="alertCircle" size={16} />
              <span>Failed to connect. Check the plugin's configuration in Settings.</span>
            </div>
          )}

          <div style={{ display: "flex", flexDirection: "column", gap: "var(--tasty-space-sm)" }}>
            <Mono>Permissions</Mono>
            <div style={{ display: "flex", gap: "var(--tasty-space-sm)", flexWrap: "wrap" }}>
              {sel.perms.map((p) => <Tag key={p}>{p}</Tag>)}
            </div>
          </div>

          {sel.cmd && (
            <div style={{ display: "flex", flexDirection: "column", gap: "var(--tasty-space-sm)" }}>
              <Mono>Command</Mono>
              <div style={{ display: "flex", alignItems: "center", gap: 16, minHeight: "var(--tasty-settings-row-min-height)",
                borderBottom: "var(--tasty-border-width) solid var(--tasty-separator)", paddingBottom: 7 }}>
                <span style={{ flex: 1, fontFamily: "var(--tasty-font-mono)", fontSize: "var(--tasty-font-size-term-sm)", color: "var(--tasty-text-secondary)" }}>{sel.cmd}</span>
                {sel.key && <Kbd keys={sel.key} />}
              </div>
            </div>
          )}
        </div>

        {/* action bar */}
        <div style={{ display: "flex", alignItems: "center", gap: "var(--tasty-space-sm)", padding: "var(--tasty-space-md) var(--tasty-size-14)",
          borderTop: "var(--tasty-border-width) solid var(--tasty-separator)", flex: "none" }}>
          {isInstalled && (
            <>
              <label style={{ display: "flex", alignItems: "center", gap: 8, cursor: "pointer" }}>
                <Switch checked={isOn} onChange={() => toggleEnabled(sel)} />
                <span style={{ fontSize: 13, color: "var(--tasty-text-secondary)" }}>{isOn ? "Enabled" : "Disabled"}</span>
              </label>
              <div style={{ flex: 1 }} />
              <Button variant="ghost" leadingIcon={ic.settings}
                onClick={() => onConfigure && onConfigure()}>Configure</Button>
              <Button variant="secondary" onClick={() => uninstall(sel)}
                style={{ color: "var(--tasty-accent-danger)" }}>Uninstall</Button>
            </>
          )}
        </div>
      </>
    );
  }

  return (
    <Scrim onClose={onClose}>
      <div style={{ width: 820, height: 540, display: "flex", flexDirection: "column", background: "var(--tasty-bg-panel)",
        border: "var(--tasty-border-width) solid var(--tasty-border-strong)", borderRadius: "var(--tasty-radius)", overflow: "hidden",
        boxShadow: "var(--tasty-shadow-modal)" }}>
        {/* header */}
        <div style={{ display: "flex", alignItems: "center", gap: "var(--tasty-space-sm)", height: 48, flex: "none", padding: "0 var(--tasty-size-14)",
          borderBottom: "var(--tasty-border-width) solid var(--tasty-separator)", background: "var(--tasty-bg-sidebar)" }}>
          <span style={{ display: "inline-flex", color: "var(--tasty-brand-melon-flesh)" }}>
            <Icon name="plug" size={16} />
          </span>
          <span style={{ fontSize: 14, fontWeight: 700, color: "var(--tasty-text-primary)", letterSpacing: "var(--tasty-letter-spacing-ui)" }}>Plugins</span>
          <span style={{ width: 1, height: 20, background: "var(--tasty-separator)", margin: "0 var(--tasty-space-sm)" }} />
          <div style={{ display: "flex", gap: 2, padding: 2, borderRadius: "var(--tasty-radius)",
            background: "var(--tasty-surface-active)" }}>
            <Seg value="installed" label="Installed" count={installedCount} />
            <Seg value="attention" label="Attention" count={attentionCount} danger />
            <Seg value="add" label="Add plugin" />
          </div>
          <div style={{ flex: 1 }} />
          {tab === "installed" && (
            <Input icon={ic.search} placeholder="Filter installed…"
              value={q} onChange={(e) => setQ(e.target.value)} style={{ width: "var(--tasty-field-width-lg)" }} />
          )}
          <IconButton aria-label="Close" onClick={onClose}>
            <Icon name="close" />
          </IconButton>
        </div>

        <div style={{ display: "flex", flex: 1, minHeight: 0 }}>
          {tab === "add" ? (
            <AddPluginForm
              onAdded={(m) => { onFlash && onFlash("Added " + m.name); setTab("installed"); }}
              onCancel={() => setTab("installed")} />
          ) : tab === "attention" ? (
            <AttentionPanel items={attention} onFlash={onFlash} onConfigure={onConfigure} />
          ) : (
          <React.Fragment>
          {/* list */}
          <div className="tasty-scroll" style={{ width: "var(--tasty-plugins-list-width)", flex: "none", overflow: "auto", padding: 8,
            background: "var(--tasty-bg-sidebar)", borderRight: "var(--tasty-border-width) solid var(--tasty-separator)",
            display: "flex", flexDirection: "column", gap: 3 }}>
            {list.map((p) => {
              const on = p.id === selId;
              const isOn = installed[p.id] && enabled[p.id];
              return (
                <div key={p.id} onClick={() => setSelId(p.id)} style={{ display: "flex", alignItems: "center",
                  gap: "var(--tasty-space-sm)", padding: "var(--tasty-space-sm) var(--tasty-space-sm)", borderRadius: "var(--tasty-radius)", cursor: "pointer",
                  background: on ? "var(--tasty-surface-active)" : "transparent",
                  boxShadow: on ? "inset var(--tasty-size-2) 0 0 var(--tasty-accent-primary)" : "none" }}>
                  <PluginAvatar plugin={p} size={32} />
                  <div style={{ flex: 1, minWidth: 0 }}>
                    <div style={{ display: "flex", alignItems: "center", gap: "var(--tasty-space-sm)" }}>
                      <span style={{ fontSize: 13, color: on ? "var(--tasty-text-primary)" : "var(--tasty-text-secondary)",
                        overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>{p.name}</span>
                    </div>
                    <div style={{ fontFamily: "var(--tasty-font-mono)", fontSize: "var(--tasty-font-size-micro)", color: "var(--tasty-text-muted)",
                      overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>
                      {p.author} · v{p.version}
                    </div>
                  </div>
                  {installed[p.id] ? (
                    !isOn && <span style={{ flex: "none", fontFamily: "var(--tasty-font-mono)", fontSize: 10,
                      color: "var(--tasty-text-muted)" }}>off</span>
                  ) : (
                    <span style={{ flex: "none", fontFamily: "var(--tasty-font-mono)", fontSize: 10,
                      color: "var(--tasty-text-muted)" }}>{p.cat}</span>
                  )}
                </div>
              );
            })}
            {list.length === 0 && (
              <div style={{ padding: 14, fontSize: "var(--tasty-font-size-term-sm)", color: "var(--tasty-text-muted)", textAlign: "center" }}>
                {q ? "No matches" : tab === "installed" ? "Nothing installed yet" : "All caught up"}
              </div>
            )}
          </div>

          {/* detail */}
          <div style={{ flex: 1, display: "flex", flexDirection: "column", minWidth: 0 }}>
            {detail()}
          </div>
          </React.Fragment>
          )}
        </div>
      </div>
    </Scrim>
  );
}

window.TastyKit = Object.assign(window.TastyKit || {}, { PluginsWindow,
  pluginAttentionCount: ATTENTION_LIST.length + PLUGIN_LIST.filter((p) => p.installed && p.status === "error").length });
