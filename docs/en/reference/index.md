<!-- source-hash: 2534725ba8b5 -->
# Reference

Lookup contracts and catalogues — **lookup**, not behaviour docs. *Why and how* a feature behaves is in [features/](../features/index.md); this is the single source for *what exists*.

| Document | Content | Code SoT |
|------|------|----------|
| [api.md](api.md) | The whole IPC/CLI surface — methods per namespace + permissions | `crates/tasty-ipc/src/method_meta.rs` |
| [event-catalog.md](event-catalog.md) | Event Bus 1.0 wire contract (public plugin API) | `tasty_plugin_protocol::events` |
| [output-parsers.md](output-parsers.md) | Terminal output parser catalogue | `tasty-output` |
| [environments.md](environments.md) | Per-OS paths and agent bootstrap patterns | — |
| [plan.schema.json](plan.schema.json) | JSON Schema of the shared-context Plan (memory key `tasty.plan.<id>`) | `crates/tasty-memory/src/plan.rs` |

> The *truth* for method signatures and permissions is always the code (`method_meta.rs` / each handler). These documents are human-readable summaries; on conflict, the code wins.
