// Info modal SHELL — the boot / runtime notice queue (one message at a time).
// Repo: src/adapters/ui/info_modal.rs (shell) + popup/defs.rs (title in the popup title bar).
// Design request: design-request/macos-permissions-followup.md (2026-09-28).
//
// Shell rules (apply to EVERY queued message, not just the permissions notice):
//   frame   440 wide × 140..360 tall; body scrolls, title bar + button row never do
//   title   the popup TITLE BAR (fixed strip, 14/600, 1px rule below — always drawn)
//   body    13 · text-secondary · paragraph gap 12; plain text unless the message
//           itself authors emphasis (only the permissions notice does)
//   edge    1px rule above the button row ONLY while content is hidden below
//           (the title bar rule already bounds the top)
//   buttons DS Button widget, right-aligned; the dismiss button is Primary, rightmost
//   keys    Enter / Esc = the dismiss button
(() => {
const { Button } = window.TastyDesignSystem_41fd3f;

// scroll: "top" | "mid" | "bottom" | "fits" (no overflow)
function InfoModalShell({ title, children, actions, scroll = "fits", id = "info-modal" }) {
  const ref = React.useRef(null);
  React.useEffect(() => {
    const el = ref.current; if (!el) return;
    const max = el.scrollHeight - el.clientHeight;
    el.scrollTop = scroll === "mid" ? Math.round(max / 2) : scroll === "bottom" ? max : 0;
  }, [scroll]);
  const edgeBottom = scroll === "top" || scroll === "mid";
  return (
    <div role="dialog" aria-modal="true" aria-labelledby={id + "-title"} style={{ width: "var(--tasty-info-modal-width)",
      minHeight: "var(--tasty-info-modal-min-height)", maxHeight: "var(--tasty-info-modal-max-height)",
      display: "flex", flexDirection: "column", background: "var(--tasty-bg-panel)", border: "var(--tasty-border-width) solid var(--tasty-border-frame)",
      borderRadius: "var(--tasty-radius)", boxShadow: "var(--tasty-shadow-modal)", overflow: "hidden" }}>
      <div id={id + "-title"} style={{ flex: "none", padding: "var(--tasty-space-md) var(--tasty-space-lg)",
        fontSize: "var(--tasty-font-size-max)", fontWeight: "var(--tasty-font-weight-semibold)", color: "var(--tasty-text-primary)",
        borderBottom: "var(--tasty-border-width) solid var(--tasty-info-modal-title-edge)" }}>{title}</div>
      <div ref={ref} className="tasty-scroll" style={{ flex: 1, minHeight: 0, overflow: "auto", padding: "var(--tasty-space-md) var(--tasty-space-lg)",
        display: "flex", flexDirection: "column", gap: "var(--tasty-info-modal-para-gap)",
        fontSize: "var(--tasty-font-size-body)", lineHeight: "var(--tasty-line-height-ui)", color: "var(--tasty-text-secondary)" }}>
        {React.Children.map(children, (p) => React.isValidElement(p) ? React.cloneElement(p, { style: { margin: 0, textWrap: "pretty", ...(p.props.style || {}) } }) : p)}
      </div>
      <div style={{ flex: "none", display: "flex", justifyContent: "flex-end", flexWrap: "wrap", gap: "var(--tasty-space-sm)",
        padding: "var(--tasty-space-md) var(--tasty-space-lg)",
        borderTop: "var(--tasty-border-width) solid " + (edgeBottom ? "var(--tasty-info-modal-scroll-edge)" : "transparent") }}>
        {actions || <Button variant="primary">OK</Button>}
      </div>
    </div>
  );
}

// Two of the five plain queue messages, drawn in the shell.
function ThemeNotFoundModal() {
  return (
    <InfoModalShell id="theme-nf" title="Theme not found">
      <p>The theme "gruvbox-hard" set in settings.toml was not found, so the default theme is used.</p>
    </InfoModalShell>
  );
}
function DbInitErrorModal() {
  return (
    <InfoModalShell id="db-err" title="Database initialization error" actions={<Button variant="primary">Quit</Button>}>
      <p>The database file is locked by another Tasty process. Close the other instance and start Tasty again.</p>
    </InfoModalShell>
  );
}

window.TastyKit = Object.assign(window.TastyKit || {}, { InfoModalShell, ThemeNotFoundModal, DbInitErrorModal });
})();
