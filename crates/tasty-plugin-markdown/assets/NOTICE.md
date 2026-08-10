# mermaid.min.js

- **Source**: `https://cdn.jsdelivr.net/npm/mermaid@11.16.1/dist/mermaid.min.js` (npm package
  `mermaid`, version `11.16.1`, tarball `sha512-TQsq6u22fAn3rek5VOubrhKPo1g5hwC3FXUN9hiyupTckcYiGuuKGkNQrKYwGJkXUxZdojwRG46gsSCFZMDp4g==`).
- **License**: MIT. Confirmed directly against the `LICENSE` file at the matching upstream tag
  (`https://github.com/mermaid-js/mermaid/blob/mermaid%4011.16.1/LICENSE`, copyright 2014-2022
  Knut Sveidqvist) and cross-checked against the npm registry's `license` field for the same
  version — both say `MIT`.
- **Fetched once, offline at runtime**: this file is vendored into the repo and loaded via
  `include_str!` at compile time — no network access happens when tasty runs. The CDN URL above
  is only the one-time source it was pulled from, not a runtime dependency (Tasty's
  offline-first principle).
- The bundle embeds a handful of third-party MIT-licensed snippets (e.g. jQuery's event module,
  a Bezier/Runge-Kutta curve generator) as inline comments — those notices are preserved verbatim
  in `mermaid.min.js` itself and are not restated here.
- To update: re-run the same jsdelivr fetch against a newer `@<version>`, re-verify the LICENSE
  at the matching tag, and update the version/hash above.
