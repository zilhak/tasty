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

# highlight.min.js

- **Source**: `https://cdnjs.cloudflare.com/ajax/libs/highlight.js/11.10.0/highlight.min.js`
  (highlight.js version `11.10.0`, the prebuilt "common" bundle — cdnjs is used here instead of
  jsdelivr/npm because the npm package `highlight.js` ships only source-only CommonJS modules
  (`lib/core.js` + per-language `lib/languages/*.js` requiring each other), not a single-file
  browser-ready bundle; cdnjs publishes the same upstream release already built to a UMD/browser
  global (`window.hljs`), which is what `include_str!` vendoring needs).
  `sha512-eb2a2a6eb70b0070d601d4268919471b8fa6d20fc24ff57d006cb169b1bc8fb264f23debdcae9db9eee8aa9891319a9c61ef5633680f71a34dc67976bb1fb7b4`
  (computed locally with `sha512sum` against the fetched file).
- **License**: BSD-3-Clause. Confirmed directly against the `LICENSE` file at the matching
  upstream tag (`https://github.com/highlightjs/highlight.js/blob/11.10.0/LICENSE`, copyright
  2006 Ivan Sagalaev).
- **Fetched once, offline at runtime**: this file is vendored into the repo and loaded via
  `include_str!` at compile time — no network access happens when tasty runs. The CDN URL above
  is only the one-time source it was pulled from, not a runtime dependency (Tasty's
  offline-first principle).
- **Language coverage**: this is highlight.js's "common" bundle (36 languages, incl. common
  aliases) — `bash, c, cpp, csharp, css, diff, go, graphql, ini, java, javascript, json, kotlin,
  less, lua, makefile, markdown, objectivec, perl, php, php-template, plaintext, python,
  python-repl, r, ruby, rust, scss, shell, sql, swift, typescript, vbnet, wasm, xml, yaml`. TOML
  is covered as an alias of the `ini` grammar (`hljs.getLanguage('toml')` resolves via `ini`'s
  `aliases`), not as a standalone language file. This already covers every language tasty's
  fenced-code-block normalization needs to key off, so no additional per-language files are
  vendored.
- Only the JS engine is vendored — no highlight.js CSS theme (e.g. `github.css`/`github-dark.css`)
  is included. Token colors are mapped in `render.rs` to this plugin's own `--md-*` CSS custom
  properties (the same Theme-derived token set every other rendered element already uses) instead
  of a hardcoded GitHub palette, so highlighted code follows the user's active tasty theme rather
  than always rendering GitHub's fixed colors.
- To update: re-run the same cdnjs fetch against a newer `/<version>/highlight.min.js`, re-verify
  the LICENSE at the matching tag, recompute the sha512, and update the version/hash above.
